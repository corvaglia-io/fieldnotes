//! Credential resolution, interactive authorization, and protected delivery.
//!
//! This module is the `0.1.3` integration seam: it turns a Field's **non-secret
//! configuration** into everything core needs to authenticate that Field, runs
//! the interactive authorization once (`fields auth`), and mints a short-lived
//! access token before every collection run so [`mod@crate::sync`] can hand it
//! over on the protected channel A2 section 12 defines.
//!
//! # Who holds what
//!
//! - **Core owns the refresh token.** It is stored by
//!   [`fieldnotes_credentials::CredentialProvider`] (the platform keychain by
//!   default) under a non-secret [`CredentialRef`], and it never leaves this
//!   process. No Field ever sees it, and no code path here puts it in a
//!   [`fieldnotes_field_protocol::message::CollectRequest`], an argument, an
//!   environment, a cursor, a diagnostic, or a notebook file.
//! - **A Field only ever receives an access token**, minted immediately before
//!   its run, delivered on the protected channel ([`channel`]), and expiring on
//!   its own without any revocation step.
//! - **Configuration carries a reference, never material.** `client_id` and
//!   `tenant_id` are non-secret and live in the Field's ordinary `config` map
//!   alongside values like `root_path`; `credential_profile` is the name of the
//!   stored credential, not the credential.
//!
//! # Core also learns *which account* signed in
//!
//! A stored refresh token used to be anonymous: it authenticated *somebody*, and
//! nothing recorded who. That cost a real debugging session and could have cost
//! notebook integrity — see [`mod@account`] for the whole story. So `fields
//! auth` now requests [`IDENTIFICATION_SCOPES`] alongside the Field's own
//! resource scopes, reads the resulting ID token's account claim, and records
//! that non-secret name in the Field's configuration
//! ([`fieldnotes_store::FieldConfig::credential_account`]). It is surfaced by
//! `fields auth`, `fields status`, and every sync report, and a notebook whose
//! Fields disagree gets a prominent warning naming the accounts and the Fields.
//!
//! The account is a **label for a person to confirm**, never an authorization
//! input. Nothing here decides anything on it.
//!
//! # Device-code flow appears nowhere
//!
//! Interactive browser PKCE on an ephemeral loopback redirect is the only
//! implemented flow, exactly as `fieldnotes-credentials` documents. Device-code
//! flow is empirically blocked by Conditional Access in the target tenant
//! (`AADSTS53003`, observed even on a compliant device) and is a phishing
//! vector: a user can be walked into approving an attacker's code on a genuine
//! login page. A Field declaring
//! [`AuthKind::OauthDeviceCode`]
//! is refused rather than accommodated.
//!
//! # Why the collaborators are traits
//!
//! [`Authorizer`], [`AccessTokenSource`], and [`CredentialInspector`] exist so
//! this crate's tests cover the whole decision surface — which configuration is
//! required, what each failure says, when a grant is issued, when a run refuses
//! before spawning anything — without a tenant, a network round trip, a browser,
//! or the developer's real keychain. [`system`] holds the real implementations,
//! which the composition root builds.

pub mod account;
pub mod auth;
pub mod channel;
pub mod system;

use std::collections::BTreeMap;

pub use account::{AccountGroup, AccountMismatch, account_mismatch};

use fieldnotes_credentials::{CredentialError, CredentialRef};
use fieldnotes_field_protocol::message::{AuthDeclaration, AuthKind, Manifest, RefreshOwner};

/// The configuration key naming the stored credential profile.
///
/// This is the reference `docs/security.md` shows (`credential_profile:
/// microsoft_acme`): a non-secret name safe to keep in configuration, in a
/// diagnostic, and in `fields status` output.
pub const PROFILE_KEY: &str = "credential_profile";

/// The configuration key selecting which credential provider stores the
/// refresh token: `keychain` (the default) or `environment`.
pub const PROVIDER_KEY: &str = "credential_provider";

/// The configuration key naming the environment variable the explicit
/// environment provider reads, when it is selected.
pub const ENV_VAR_KEY: &str = "credential_env_var";

/// The configuration key overriding the OAuth public client ID.
pub const CLIENT_ID_KEY: &str = "oauth_client_id";

/// The configuration key overriding the authority tenant segment.
pub const TENANT_KEY: &str = "oauth_tenant_id";

/// The configuration key overriding the authority base URL.
pub const AUTHORITY_KEY: &str = "oauth_authority";

/// The configuration key overriding the loopback redirect path.
pub const REDIRECT_PATH_KEY: &str = "oauth_redirect_path";

/// Every configuration key this module reads, for `fields add` validation and
/// for documentation.
pub const CREDENTIAL_CONFIG_KEYS: [&str; 7] = [
    PROFILE_KEY,
    PROVIDER_KEY,
    ENV_VAR_KEY,
    CLIENT_ID_KEY,
    TENANT_KEY,
    AUTHORITY_KEY,
    REDIRECT_PATH_KEY,
];

/// The beta default public client ID: the Microsoft Graph PowerShell
/// first-party public client.
///
/// **Overridable, and deliberately not compiled in as an unchangeable
/// default.** It is here because it is a verified, consent-friendly public
/// client that works in the target tenant with an ephemeral loopback redirect,
/// which is what makes an out-of-the-box beta possible without every user first
/// creating an app registration.
///
/// Using a first-party client ID has a real, documented cost: **reads are
/// attributed to that application in the tenant's sign-in logs**, so an
/// administrator auditing access sees "Microsoft Graph PowerShell", not
/// "Fieldnotes". A deployment should move to its own app registration and set
/// [`CLIENT_ID_KEY`] accordingly.
pub const DEFAULT_CLIENT_ID: &str = "14d82eec-204b-4c2f-b7e8-296a70dab67e";

/// The default authority tenant segment.
///
/// The literal string `organizations` resolves to the signed-in user's home
/// tenant, which avoids hardcoding a tenant GUID into a shipped default while
/// still excluding personal Microsoft accounts.
pub const DEFAULT_TENANT: &str = "organizations";

/// The default authority base URL.
pub const DEFAULT_AUTHORITY: &str = "https://login.microsoftonline.com";

/// The default loopback redirect path: none at all.
///
/// The port is always ephemeral — the listener binds `127.0.0.1:0` and the
/// operating system picks an unused port for that one attempt — because an
/// authorization server waives the port when matching a loopback redirect URI.
/// It does not waive the path. A public-client loopback registration is
/// conventionally `http://localhost` with no path, so sending any path makes
/// the request fail as a redirect-URI mismatch. Deployments whose registration
/// does name a path can set one through [`REDIRECT_PATH_KEY`].
pub const DEFAULT_REDIRECT_PATH: &str = "";

/// The keychain service name Fieldnotes stores its entries under.
pub const KEYCHAIN_SERVICE: &str = "fieldnotes";

/// The scope core adds to an authorization request so a refresh token is
/// actually issued.
///
/// Every resource scope comes from the Field's own manifest: core requests
/// exactly what the Field declares, plus this one and
/// [`IDENTIFICATION_SCOPES`], and never a broader set.
pub const OFFLINE_ACCESS_SCOPE: &str = "offline_access";

/// The OpenID Connect scope that makes the authorization server tell core
/// **which account** just signed in, by returning an ID token.
pub const OPENID_SCOPE: &str = "openid";

/// The OpenID Connect scope that makes Microsoft Entra include the
/// human-recognizable `preferred_username` claim in that ID token.
pub const PROFILE_SCOPE: &str = "profile";

/// The two scopes core adds **for identification, not for access**.
///
/// Neither grants access to any data. `openid` exists precisely to tell a
/// client who signed in, and `profile` is what makes the resulting ID token
/// carry a name a person can recognize. Neither requires administrative
/// consent.
///
/// They are requested because without them a stored credential is anonymous,
/// and that anonymity has already cost a real debugging session: three Fields
/// authenticated in three separate browser flows can silently be three
/// different principals, because a browser reuses whatever sign-in session is
/// already open. The quiet version of that mistake — an administrator who
/// *does* have a mailbox — fills a notebook with the wrong person's mail with
/// nothing in any output to suggest it. Identifying the account at
/// authorization time is what turns that from invisible into obvious.
///
/// The `email` scope is deliberately **not** requested: `profile` already
/// yields a recognizable name, so `email` would add a second personal-data
/// claim for no additional answer.
///
/// What the resulting account identifier is for is narrow and documented in
/// [`fieldnotes_credentials::oauth::id_token`]: display and confirmation.
/// **Nothing may authorize anything on it.**
pub const IDENTIFICATION_SCOPES: [&str; 2] = [OPENID_SCOPE, PROFILE_SCOPE];

/// Which credential provider stores this profile's refresh token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderChoice {
    /// The platform keychain (macOS Keychain, Windows Credential Manager,
    /// Linux Secret Service), under [`KEYCHAIN_SERVICE`].
    Keychain,
    /// The explicit, opt-in environment-variable provider for CI and headless
    /// use. Read-only: it cannot store a refresh token, so `fields auth`
    /// refuses for it rather than appearing to have saved something.
    Environment {
        /// The variable read into core's memory. Never copied into a child
        /// process's environment.
        variable: String,
    },
}

impl ProviderChoice {
    /// A stable lowercase label for output.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderChoice::Keychain => "keychain",
            ProviderChoice::Environment { .. } => "environment",
        }
    }
}

/// Everything core needs to authenticate one configured Field, resolved from
/// its non-secret configuration.
///
/// Carries no credential material of any kind, which is why it is safe to
/// return from a use case and render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialSettings {
    /// The non-secret name the refresh token is stored under.
    pub profile: CredentialRef,
    /// Where the refresh token is stored.
    pub provider: ProviderChoice,
    /// The OAuth public client ID.
    pub client_id: String,
    /// The authority tenant segment.
    pub tenant: String,
    /// The authority base URL.
    pub authority: String,
    /// The loopback redirect path.
    pub redirect_path: String,
}

impl CredentialSettings {
    /// The authorize endpoint this configuration resolves to.
    #[must_use]
    pub fn authorize_endpoint(&self) -> String {
        format!(
            "{}/{}/oauth2/v2.0/authorize",
            self.authority.trim_end_matches('/'),
            self.tenant
        )
    }

    /// The token endpoint this configuration resolves to.
    #[must_use]
    pub fn token_endpoint(&self) -> String {
        format!(
            "{}/{}/oauth2/v2.0/token",
            self.authority.trim_end_matches('/'),
            self.tenant
        )
    }

    /// Whether the configured client ID is the shared first-party default,
    /// which attributes reads to that application in the tenant's sign-in
    /// logs.
    #[must_use]
    pub fn uses_shared_client_id(&self) -> bool {
        self.client_id == DEFAULT_CLIENT_ID
    }
}

/// Why a credential operation could not produce a usable token.
///
/// Every variant's text is actionable and secret-free: the whole point of this
/// type is that an expired or revoked credential produces an instruction, not a
/// stack trace.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CredentialFailure {
    /// A Field needs authentication but its configuration does not say which
    /// credential to use, or says it invalidly.
    NotConfigured {
        /// The Field.
        field_id: String,
        /// What is missing or wrong.
        detail: String,
    },
    /// A Field declares an authentication shape this release does not perform.
    Unsupported {
        /// The Field.
        field_id: String,
        /// Why, in reviewable terms.
        detail: String,
    },
    /// Nothing is stored for this profile yet.
    NotAuthenticated {
        /// The Field.
        field_id: String,
        /// The profile that has no credential.
        profile: String,
    },
    /// The stored refresh token can no longer be refreshed.
    Expired {
        /// The Field.
        field_id: String,
        /// The profile whose credential expired.
        profile: String,
    },
    /// The stored refresh token was revoked upstream (consent withdrawn, a
    /// password change, an administrator action).
    Revoked {
        /// The Field.
        field_id: String,
        /// The profile whose credential was revoked.
        profile: String,
    },
    /// Authorization was explicitly declined.
    Denied {
        /// The Field.
        field_id: String,
        /// The profile that was not authorized.
        profile: String,
    },
    /// The credential store itself could not be used.
    ProviderUnavailable {
        /// The profile that could not be read.
        profile: String,
        /// The provider's own secret-free explanation.
        reason: String,
    },
    /// Any other credential-backend failure, including a token endpoint that
    /// could not be reached.
    Backend {
        /// The profile involved.
        profile: String,
        /// The backend's own secret-free explanation.
        reason: String,
    },
    /// An interactive authorization was needed where none can run.
    NotInteractive {
        /// The Field that needs authenticating.
        field_id: String,
    },
    /// The protected channel could not be established or served.
    Channel {
        /// What went wrong, in reviewable terms.
        detail: String,
    },
}

impl CredentialFailure {
    /// A stable lowercase kind label for machine-readable output.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            CredentialFailure::NotConfigured { .. } => "credential_not_configured",
            CredentialFailure::Unsupported { .. } => "credential_unsupported",
            CredentialFailure::NotAuthenticated { .. } => "credential_absent",
            CredentialFailure::Expired { .. } => "credential_expired",
            CredentialFailure::Revoked { .. } => "credential_revoked",
            CredentialFailure::Denied { .. } => "credential_denied",
            CredentialFailure::ProviderUnavailable { .. } => "credential_provider_unavailable",
            CredentialFailure::Backend { .. } => "credential_backend",
            CredentialFailure::NotInteractive { .. } => "credential_not_interactive",
            CredentialFailure::Channel { .. } => "credential_channel",
        }
    }

    /// Whether the fix is for the user to run `fieldnotes fields auth`.
    #[must_use]
    pub fn needs_reauthentication(&self) -> bool {
        matches!(
            self,
            CredentialFailure::NotAuthenticated { .. }
                | CredentialFailure::Expired { .. }
                | CredentialFailure::Revoked { .. }
                | CredentialFailure::Denied { .. }
        )
    }

    /// Maps a [`CredentialError`] from the credential layer, attributing it to
    /// one Field and profile.
    #[must_use]
    pub fn from_credential_error(field_id: &str, profile: &str, error: &CredentialError) -> Self {
        match error {
            CredentialError::Absent => CredentialFailure::NotAuthenticated {
                field_id: field_id.to_owned(),
                profile: profile.to_owned(),
            },
            CredentialError::Expired => CredentialFailure::Expired {
                field_id: field_id.to_owned(),
                profile: profile.to_owned(),
            },
            CredentialError::Revoked => CredentialFailure::Revoked {
                field_id: field_id.to_owned(),
                profile: profile.to_owned(),
            },
            CredentialError::Denied => CredentialFailure::Denied {
                field_id: field_id.to_owned(),
                profile: profile.to_owned(),
            },
            CredentialError::Unavailable(reason) => CredentialFailure::ProviderUnavailable {
                profile: profile.to_owned(),
                reason: reason.clone(),
            },
            other => CredentialFailure::Backend {
                profile: profile.to_owned(),
                reason: other.to_string(),
            },
        }
    }
}

impl core::fmt::Display for CredentialFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CredentialFailure::NotConfigured { field_id, detail } => write!(
                f,
                "Field `{field_id}` needs authentication but its configuration does not say which \
                 credential to use: {detail}"
            ),
            CredentialFailure::Unsupported { field_id, detail } => {
                write!(f, "Field `{field_id}` cannot be authenticated: {detail}")
            }
            CredentialFailure::NotAuthenticated { field_id, profile } => write!(
                f,
                "no credential is stored for profile `{profile}`; run `fieldnotes fields auth \
                 {field_id}` to authenticate it"
            ),
            CredentialFailure::Expired { field_id, profile } => write!(
                f,
                "the stored credential for profile `{profile}` has expired and can no longer be \
                 refreshed; run `fieldnotes fields auth {field_id}` to authenticate again"
            ),
            CredentialFailure::Revoked { field_id, profile } => write!(
                f,
                "the stored credential for profile `{profile}` was revoked upstream; run \
                 `fieldnotes fields auth {field_id}` to authenticate again"
            ),
            CredentialFailure::Denied { field_id, profile } => write!(
                f,
                "authorization for profile `{profile}` was declined; run `fieldnotes fields auth \
                 {field_id}` to try again"
            ),
            CredentialFailure::ProviderUnavailable { profile, reason } => write!(
                f,
                "the credential store holding profile `{profile}` is unavailable: {reason}"
            ),
            CredentialFailure::Backend { profile, reason } => {
                write!(f, "profile `{profile}` could not be used: {reason}")
            }
            CredentialFailure::NotInteractive { field_id } => write!(
                f,
                "authenticating Field `{field_id}` opens a browser, which a non-interactive run \
                 must not do; run `fieldnotes fields auth {field_id}` from an interactive session \
                 instead"
            ),
            CredentialFailure::Channel { detail } => write!(
                f,
                "the protected credential channel could not be established: {detail}"
            ),
        }
    }
}

impl std::error::Error for CredentialFailure {}

/// What a Field's manifest asks core to do about authentication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthRequirement {
    /// Whether a credential must be delivered at all.
    pub required: bool,
    /// The least-privilege scopes the Field declared, without
    /// [`OFFLINE_ACCESS_SCOPE`].
    pub scopes: Vec<String>,
}

impl AuthRequirement {
    /// The scopes an authorization request asks for: exactly what the Field
    /// declared, plus `offline_access` so a refresh token is issued, plus
    /// [`IDENTIFICATION_SCOPES`] so core learns which account signed in.
    ///
    /// The resource scopes are still exactly the Field's own least-privilege
    /// declaration. The three core adds grant access to nothing: one makes a
    /// refresh token possible at all, and two make the signed-in principal
    /// nameable. A scope already declared is not requested twice.
    #[must_use]
    pub fn authorization_scopes(&self) -> Vec<String> {
        let mut scopes = self.scopes.clone();
        for added in
            core::iter::once(OFFLINE_ACCESS_SCOPE).chain(IDENTIFICATION_SCOPES.iter().copied())
        {
            if !scopes.iter().any(|scope| scope == added) {
                scopes.push(added.to_owned());
            }
        }
        scopes
    }
}

/// Reads a Field's manifest authentication declaration, refusing the shapes
/// this release does not perform.
///
/// Refusals are deliberate rather than best-effort accommodation:
///
/// - **device code** is refused outright (see this module's documentation);
/// - **a Field-owned refresh** is refused because core owns refresh and never
///   hands a Field a refresh token;
/// - **API-token, basic, and client-credentials** shapes have no `fields auth`
///   flow in this release, so a Field declaring one is told so instead of
///   starting a run that cannot authenticate.
pub fn requirement_of(
    field_id: &str,
    auth: &AuthDeclaration,
) -> Result<AuthRequirement, CredentialFailure> {
    let unsupported = |detail: String| CredentialFailure::Unsupported {
        field_id: field_id.to_owned(),
        detail,
    };
    if auth.kind == AuthKind::None
        && !auth.credential_profile_required
        && !auth.protected_channel_required
    {
        return Ok(AuthRequirement {
            required: false,
            scopes: Vec::new(),
        });
    }
    match auth.kind {
        AuthKind::OauthAuthorizationCode => {}
        AuthKind::OauthDeviceCode => {
            return Err(unsupported(
                "it declares OAuth device-code flow, which Fieldnotes implements nowhere: it is a \
                 phishing vector and is blocked by Conditional Access in tenants that require a \
                 compliant device (AADSTS53003). Declare `oauth_authorization_code` instead, which \
                 core performs interactively on an ephemeral loopback redirect."
                    .to_owned(),
            ));
        }
        AuthKind::None => {
            return Err(unsupported(
                "it declares auth kind `none` while still requiring a credential profile or the \
                 protected channel, so core cannot tell what to deliver"
                    .to_owned(),
            ));
        }
        other => {
            return Err(unsupported(format!(
                "it declares auth kind `{}`, and this release performs only \
                 `oauth_authorization_code`",
                serde_json::to_value(other)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "unknown".to_owned())
            )));
        }
    }
    if auth.refresh_owner == RefreshOwner::Field {
        return Err(unsupported(
            "it declares that the Field owns refresh, which would require handing it a refresh \
             token; core owns refresh and delivers only a short-lived access token, so the \
             manifest must declare `refresh_owner: core`"
                .to_owned(),
        ));
    }
    let scopes = auth.scopes.clone().unwrap_or_default();
    if scopes.is_empty() {
        return Err(unsupported(
            "it requires authentication but declares no scopes, and core requests only what a \
             Field declares rather than guessing a broader set"
                .to_owned(),
        ));
    }
    Ok(AuthRequirement {
        required: true,
        scopes,
    })
}

/// Reads a manifest's authentication requirement.
pub fn requirement_of_manifest(
    field_id: &str,
    manifest: &Manifest,
) -> Result<AuthRequirement, CredentialFailure> {
    requirement_of(field_id, &manifest.auth)
}

/// Resolves credential settings from a Field's non-secret configuration map.
///
/// Only [`PROFILE_KEY`] is required; every other key has a documented default,
/// and every one of them is overridable, so no deployment is stuck with the
/// shared first-party client ID or the `organizations` authority.
pub fn settings_from_config(
    field_id: &str,
    config: &BTreeMap<String, String>,
) -> Result<CredentialSettings, CredentialFailure> {
    let missing = |detail: String| CredentialFailure::NotConfigured {
        field_id: field_id.to_owned(),
        detail,
    };
    let value = |key: &str| -> Option<&str> {
        config
            .get(key)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
    };
    let profile_text = value(PROFILE_KEY).ok_or_else(|| {
        missing(format!(
            "set `--config {PROFILE_KEY}=<name>` to the non-secret profile name the credential is \
             stored under (a name, never a secret)"
        ))
    })?;
    let profile = CredentialRef::parse(profile_text).map_err(|error| {
        missing(format!(
            "`{PROFILE_KEY}={profile_text}` is invalid: {error}"
        ))
    })?;
    let provider = match value(PROVIDER_KEY) {
        None | Some("keychain") => ProviderChoice::Keychain,
        Some("environment") => ProviderChoice::Environment {
            variable: value(ENV_VAR_KEY)
                .map(str::to_owned)
                .unwrap_or_else(|| default_env_variable(profile.as_str())),
        },
        Some(other) => {
            return Err(missing(format!(
                "`{PROVIDER_KEY}={other}` is not a provider; use `keychain` (the default) or \
                 `environment` (explicit, read-only, for CI and headless use)"
            )));
        }
    };
    let authority = value(AUTHORITY_KEY).unwrap_or(DEFAULT_AUTHORITY).to_owned();
    if !authority.starts_with("https://") {
        return Err(missing(format!(
            "`{AUTHORITY_KEY}={authority}` must be an https URL; core never sends a credential \
             over plaintext http"
        )));
    }
    let redirect_path = value(REDIRECT_PATH_KEY)
        .unwrap_or(DEFAULT_REDIRECT_PATH)
        .to_owned();
    if !redirect_path.is_empty() && !redirect_path.starts_with('/') {
        return Err(missing(format!(
            "`{REDIRECT_PATH_KEY}={redirect_path}` must begin with `/`, or be \
             empty for a registration that names no path"
        )));
    }
    Ok(CredentialSettings {
        profile,
        provider,
        client_id: value(CLIENT_ID_KEY).unwrap_or(DEFAULT_CLIENT_ID).to_owned(),
        tenant: value(TENANT_KEY).unwrap_or(DEFAULT_TENANT).to_owned(),
        authority,
        redirect_path,
    })
}

/// The environment variable name the environment provider defaults to for one
/// profile.
#[must_use]
pub fn default_env_variable(profile: &str) -> String {
    format!("FIELDNOTES_CREDENTIAL_{}", profile.to_ascii_uppercase())
}

/// Whether a Field's configuration names a credential profile at all.
///
/// This is what lets `sync` decide to resolve and mint a credential **before it
/// spawns anything**: the manifest is the authority on what a Field needs, but
/// reading a manifest means running `describe`, and a missing credential must
/// fail earlier than that. Configuration is available with no child process at
/// all, and an authenticating Field cannot work without this key anyway.
#[must_use]
pub fn config_declares_credential(config: &BTreeMap<String, String>) -> bool {
    config
        .get(PROFILE_KEY)
        .is_some_and(|value| !value.trim().is_empty())
}

/// Validates the credential-related keys of a `config` map at configuration
/// time, so a typo is reported by `fields add` rather than by the first sync.
///
/// Silent for a map that names no credential profile: a Field that needs none
/// is the common case.
pub fn validate_config(
    field_id: &str,
    config: &BTreeMap<String, String>,
) -> Result<(), CredentialFailure> {
    if !config_declares_credential(config) {
        // A provider or endpoint override with no profile is a configuration
        // mistake worth naming, because nothing would ever read it.
        for key in CREDENTIAL_CONFIG_KEYS
            .iter()
            .filter(|key| **key != PROFILE_KEY)
        {
            if config.contains_key(*key) {
                return Err(CredentialFailure::NotConfigured {
                    field_id: field_id.to_owned(),
                    detail: format!(
                        "`{key}` was set but `{PROFILE_KEY}` was not, so nothing would ever read it"
                    ),
                });
            }
        }
        return Ok(());
    }
    settings_from_config(field_id, config).map(|_| ())
}

/// Whether a credential is currently stored for a profile.
///
/// Deliberately coarse: it answers `fields status`'s question ("is this Field
/// authenticated?") without attempting a sync, and it never reports anything
/// about the value itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialState {
    /// This Field needs no credential.
    NotRequired,
    /// This Field needs a credential but none is configured.
    NotConfigured,
    /// A credential is stored and can be read.
    Stored,
    /// No credential is stored: the Field has not been authenticated.
    Absent,
    /// The credential store could not be consulted.
    Unavailable(String),
}

impl CredentialState {
    /// A stable lowercase label for machine-readable output.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            CredentialState::NotRequired => "not_required",
            CredentialState::NotConfigured => "not_configured",
            CredentialState::Stored => "stored",
            CredentialState::Absent => "absent",
            CredentialState::Unavailable(_) => "unavailable",
        }
    }
}

/// Runs one interactive authorization and stores the resulting refresh token.
///
/// The real implementation ([`system::SystemCredentials`]) opens a browser and
/// makes network calls; a test implementation does neither, which is how this
/// crate covers `fields auth`'s decisions without a tenant.
pub trait Authorizer {
    /// Authorizes `settings` for exactly `scopes` and stores the refresh token
    /// under the settings' profile.
    ///
    /// Returns only non-secret facts: no implementation may return, log, or
    /// print token material.
    fn authorize(
        &mut self,
        field_id: &str,
        settings: &CredentialSettings,
        scopes: &[String],
    ) -> Result<Authorized, CredentialFailure>;
}

/// The non-secret outcome of one successful authorization.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Authorized {
    /// The scopes that were requested.
    pub scopes: Vec<String>,
    /// When the access token minted during authorization expires, rendered as
    /// an explicit-offset RFC 3339 instant. Reported so a user can see that
    /// the delivered material is short-lived; the access token itself is
    /// discarded here, since `fields auth` runs no collection.
    pub access_token_expires_at: Option<String>,
    /// **Which account** the browser actually signed in as, read from the ID
    /// token [`IDENTIFICATION_SCOPES`] made the authorization server return.
    ///
    /// `None` means the server returned no readable ID token, which is not a
    /// failure: the credential was stored and the account is simply unknown.
    ///
    /// This is for display and confirmation. It is never used to grant or deny
    /// anything; see [`fieldnotes_credentials::oauth::id_token`].
    pub account: Option<String>,
}

/// Mints a short-lived access token for an already-authorized profile.
pub trait AccessTokenSource {
    /// Loads the stored refresh token, redeems it, rotates the stored value
    /// when the authorization server returns a new one, and returns the minted
    /// access token.
    fn mint(
        &self,
        field_id: &str,
        settings: &CredentialSettings,
    ) -> Result<fieldnotes_credentials::oauth::AccessToken, CredentialFailure>;
}

/// Where a collection run gets its access tokens.
///
/// Defaults to [`CredentialSource::None`] rather than to the real keychain-and-
/// network implementation, and that default is load-bearing: minting a token
/// needs the wall clock and an HTTPS call, and this crate's contract is that
/// only a composition root touches either. A run that needs a credential and
/// was given no source is refused, exactly as a run with no stored credential
/// is, rather than the library quietly reaching for the network itself.
#[derive(Clone, Default)]
pub enum CredentialSource {
    /// No source. A Field needing a credential is refused.
    #[default]
    None,
    /// A source the composition root supplied.
    ///
    /// [`Send`] and [`Sync`] because the same options value drives every
    /// configured Field in one `sync` invocation.
    Injected(std::sync::Arc<dyn AccessTokenSource + Send + Sync>),
}

impl core::fmt::Debug for CredentialSource {
    /// Names the variant only. A source holds a keychain handle and an HTTP
    /// agent, and neither has anything a diagnostic should print.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CredentialSource::None => f.write_str("CredentialSource::None"),
            CredentialSource::Injected(_) => f.write_str("CredentialSource::Injected"),
        }
    }
}

impl CredentialSource {
    /// Mints an access token, or reports that no source was supplied.
    pub fn mint(
        &self,
        field_id: &str,
        settings: &CredentialSettings,
    ) -> Result<fieldnotes_credentials::oauth::AccessToken, CredentialFailure> {
        match self {
            CredentialSource::None => Err(CredentialFailure::Unsupported {
                field_id: field_id.to_owned(),
                detail: "this run was started without a credential source, so core cannot mint an \
                         access token for it"
                    .to_owned(),
            }),
            CredentialSource::Injected(source) => source.mint(field_id, settings),
        }
    }
}

/// Answers whether a credential is stored, without minting anything.
pub trait CredentialInspector {
    /// Probes the credential store for `settings`' profile.
    fn state(&self, settings: &CredentialSettings) -> CredentialState;
}

/// A [`CredentialInspector`] that consults nothing and reports nothing.
///
/// Used where credential state is deliberately not probed — `fields status` in
/// a test, or a caller that only wants configuration facts.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoInspector;

impl CredentialInspector for NoInspector {
    fn state(&self, _settings: &CredentialSettings) -> CredentialState {
        CredentialState::NotConfigured
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldnotes_field_protocol::grammar::ConstFalse;

    fn config(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn declaration(
        kind: AuthKind,
        refresh_owner: RefreshOwner,
        scopes: &[&str],
    ) -> AuthDeclaration {
        AuthDeclaration {
            kind,
            credential_profile_required: kind != AuthKind::None,
            protected_channel_required: kind != AuthKind::None,
            scopes: if scopes.is_empty() {
                None
            } else {
                Some(scopes.iter().map(|scope| (*scope).to_owned()).collect())
            },
            refresh_owner,
            writes_to_source: ConstFalse,
        }
    }

    #[test]
    fn a_profile_name_is_the_only_required_configuration() -> Result<(), CredentialFailure> {
        let settings =
            settings_from_config("outlook_mail_work", &config(&[(PROFILE_KEY, "work")]))?;
        assert_eq!(settings.profile.as_str(), "work");
        assert_eq!(settings.client_id, DEFAULT_CLIENT_ID);
        assert_eq!(settings.tenant, DEFAULT_TENANT);
        assert!(settings.uses_shared_client_id());
        assert_eq!(
            settings.authorize_endpoint(),
            "https://login.microsoftonline.com/organizations/oauth2/v2.0/authorize"
        );
        assert_eq!(
            settings.token_endpoint(),
            "https://login.microsoftonline.com/organizations/oauth2/v2.0/token"
        );
        assert_eq!(settings.provider, ProviderChoice::Keychain);
        Ok(())
    }

    #[test]
    fn every_default_is_overridable() -> Result<(), CredentialFailure> {
        let settings = settings_from_config(
            "outlook_mail_work",
            &config(&[
                (PROFILE_KEY, "work"),
                (CLIENT_ID_KEY, "11111111-2222-3333-4444-555555555555"),
                (TENANT_KEY, "contoso.onmicrosoft.com"),
                (AUTHORITY_KEY, "https://login.microsoftonline.us"),
                (REDIRECT_PATH_KEY, "/fieldnotes-callback"),
            ]),
        )?;
        assert_eq!(settings.client_id, "11111111-2222-3333-4444-555555555555");
        assert!(!settings.uses_shared_client_id());
        assert_eq!(
            settings.authorize_endpoint(),
            "https://login.microsoftonline.us/contoso.onmicrosoft.com/oauth2/v2.0/authorize"
        );
        assert_eq!(settings.redirect_path, "/fieldnotes-callback");
        Ok(())
    }

    #[test]
    fn a_missing_profile_is_reported_actionably() {
        let failure = settings_from_config("outlook_mail_work", &config(&[]))
            .err()
            .unwrap_or_else(|| panic!("a config with no profile must not resolve"));
        assert_eq!(failure.kind(), "credential_not_configured");
        assert!(failure.to_string().contains(PROFILE_KEY));
    }

    #[test]
    fn a_plaintext_authority_is_refused() {
        assert!(
            settings_from_config(
                "outlook_mail_work",
                &config(&[
                    (PROFILE_KEY, "work"),
                    (AUTHORITY_KEY, "http://example.test")
                ]),
            )
            .is_err()
        );
    }

    #[test]
    fn the_environment_provider_is_explicit_and_names_its_variable() -> Result<(), CredentialFailure>
    {
        let settings = settings_from_config(
            "outlook_mail_ci",
            &config(&[(PROFILE_KEY, "ci_work"), (PROVIDER_KEY, "environment")]),
        )?;
        assert_eq!(
            settings.provider,
            ProviderChoice::Environment {
                variable: "FIELDNOTES_CREDENTIAL_CI_WORK".to_owned()
            }
        );
        let named = settings_from_config(
            "outlook_mail_ci",
            &config(&[
                (PROFILE_KEY, "ci_work"),
                (PROVIDER_KEY, "environment"),
                (ENV_VAR_KEY, "MY_TOKEN"),
            ]),
        )?;
        assert_eq!(
            named.provider,
            ProviderChoice::Environment {
                variable: "MY_TOKEN".to_owned()
            }
        );
        Ok(())
    }

    #[test]
    fn an_unknown_provider_is_refused_rather_than_silently_defaulted() {
        assert!(
            settings_from_config(
                "outlook_mail_work",
                &config(&[(PROFILE_KEY, "work"), (PROVIDER_KEY, "file")]),
            )
            .is_err()
        );
    }

    #[test]
    fn a_field_needing_no_credential_has_no_requirement() -> Result<(), CredentialFailure> {
        let requirement = requirement_of(
            "local_work",
            &declaration(AuthKind::None, RefreshOwner::NotApplicable, &[]),
        )?;
        assert!(!requirement.required);
        Ok(())
    }

    #[test]
    fn device_code_flow_is_refused_everywhere() {
        let failure = requirement_of(
            "outlook_mail_work",
            &declaration(
                AuthKind::OauthDeviceCode,
                RefreshOwner::Core,
                &["Mail.Read"],
            ),
        )
        .err()
        .unwrap_or_else(|| panic!("device code must be refused"));
        assert_eq!(failure.kind(), "credential_unsupported");
        let text = failure.to_string();
        assert!(text.contains("AADSTS53003"), "{text}");
        assert!(text.contains("oauth_authorization_code"), "{text}");
    }

    #[test]
    fn a_field_owned_refresh_is_refused_because_core_owns_refresh() {
        let failure = requirement_of(
            "outlook_mail_work",
            &declaration(
                AuthKind::OauthAuthorizationCode,
                RefreshOwner::Field,
                &["Mail.Read"],
            ),
        )
        .err()
        .unwrap_or_else(|| panic!("a Field-owned refresh must be refused"));
        assert!(failure.to_string().contains("refresh_owner: core"));
    }

    #[test]
    fn scopes_come_from_the_manifest_plus_offline_access_and_identification()
    -> Result<(), CredentialFailure> {
        let requirement = requirement_of(
            "outlook_mail_work",
            &declaration(
                AuthKind::OauthAuthorizationCode,
                RefreshOwner::Core,
                &["Mail.Read"],
            ),
        )?;
        // The Field's own declaration is untouched: the added scopes are core's,
        // and none of them grants access to anything.
        assert_eq!(requirement.scopes, vec!["Mail.Read".to_owned()]);
        assert_eq!(
            requirement.authorization_scopes(),
            vec![
                "Mail.Read".to_owned(),
                OFFLINE_ACCESS_SCOPE.to_owned(),
                OPENID_SCOPE.to_owned(),
                PROFILE_SCOPE.to_owned(),
            ]
        );
        // `email` is deliberately never requested: `profile` already yields a
        // recognizable name.
        assert!(
            !requirement
                .authorization_scopes()
                .iter()
                .any(|scope| scope == "email")
        );
        // Already declared: not added twice.
        let declared = requirement_of(
            "outlook_mail_work",
            &declaration(
                AuthKind::OauthAuthorizationCode,
                RefreshOwner::Core,
                &["Mail.Read", OFFLINE_ACCESS_SCOPE, OPENID_SCOPE],
            ),
        )?;
        assert_eq!(
            declared.authorization_scopes(),
            vec![
                "Mail.Read".to_owned(),
                OFFLINE_ACCESS_SCOPE.to_owned(),
                OPENID_SCOPE.to_owned(),
                PROFILE_SCOPE.to_owned(),
            ]
        );
        Ok(())
    }

    #[test]
    fn a_field_declaring_no_scopes_is_refused_rather_than_given_a_guess() {
        assert!(
            requirement_of(
                "outlook_mail_work",
                &declaration(AuthKind::OauthAuthorizationCode, RefreshOwner::Core, &[]),
            )
            .is_err()
        );
    }

    #[test]
    fn an_override_with_no_profile_is_a_configuration_mistake() {
        assert!(validate_config("local_work", &config(&[])).is_ok());
        assert!(validate_config("local_work", &config(&[("root_path", "/tmp")])).is_ok());
        assert!(
            validate_config(
                "outlook_mail_work",
                &config(&[(TENANT_KEY, "organizations")])
            )
            .is_err()
        );
        assert!(validate_config("outlook_mail_work", &config(&[(PROFILE_KEY, "work")])).is_ok());
    }

    #[test]
    fn every_failure_is_actionable_and_carries_no_material() {
        let failures = [
            CredentialFailure::NotAuthenticated {
                field_id: "outlook_mail_work".to_owned(),
                profile: "work".to_owned(),
            },
            CredentialFailure::Expired {
                field_id: "outlook_mail_work".to_owned(),
                profile: "work".to_owned(),
            },
            CredentialFailure::Revoked {
                field_id: "outlook_mail_work".to_owned(),
                profile: "work".to_owned(),
            },
            CredentialFailure::Denied {
                field_id: "outlook_mail_work".to_owned(),
                profile: "work".to_owned(),
            },
        ];
        for failure in failures {
            assert!(failure.needs_reauthentication());
            let text = failure.to_string();
            assert!(
                text.contains("fieldnotes fields auth outlook_mail_work"),
                "{text}"
            );
        }
        assert!(
            !CredentialFailure::Channel {
                detail: "no listener".to_owned()
            }
            .needs_reauthentication()
        );
    }

    #[test]
    fn credential_errors_map_to_actionable_failures() {
        for (error, kind) in [
            (CredentialError::Absent, "credential_absent"),
            (CredentialError::Expired, "credential_expired"),
            (CredentialError::Revoked, "credential_revoked"),
            (CredentialError::Denied, "credential_denied"),
            (
                CredentialError::Unavailable("locked".to_owned()),
                "credential_provider_unavailable",
            ),
            (
                CredentialError::Backend("odd".to_owned()),
                "credential_backend",
            ),
        ] {
            assert_eq!(
                CredentialFailure::from_credential_error("outlook_mail_work", "work", &error)
                    .kind(),
                kind
            );
        }
    }
}
