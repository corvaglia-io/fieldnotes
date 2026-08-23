//! `fields auth <field_id>`: authenticate one configured Field, once.
//!
//! # What the command does, in order
//!
//! 1. Refuse `self`, and refuse an unconfigured Field ID.
//! 2. Resolve credential settings from the Field's **non-secret
//!    configuration**. A misconfiguration fails here, before any process is
//!    started.
//! 3. Refuse a non-interactive invocation, before anything could open a
//!    browser.
//! 4. Run `describe` — which A2 guarantees carries no credential grant — to
//!    learn the scopes the Field declares, and record the manifest snapshot the
//!    same way `sync` does.
//! 5. Run the interactive authorization-code flow with PKCE on an ephemeral
//!    loopback redirect, and store the resulting **refresh token** under the
//!    configured profile.
//! 6. Report what happened, naming no material.
//!
//! # Why this command runs `describe`
//!
//! Scopes are per Field and come from the Field's own manifest. Reading them
//! from the stored snapshot instead would deadlock a fresh install: `sync`
//! refuses without a credential, so no snapshot would ever be recorded, so
//! `fields auth` would have no scopes to request. `describe` is the one
//! operation that is safe to run first, because the protocol gives it no
//! credential grant, no cursor, and no staging directory.
//!
//! # Nothing here prints or returns token material
//!
//! [`AuthOutcome`] carries only the profile name, the provider, the non-secret
//! client and tenant identifiers, the requested scopes, and when the *access*
//! token minted during authorization expires. The refresh token goes straight
//! from the token endpoint into the credential provider inside
//! `fieldnotes-credentials` and is never returned to this layer at all.

use fieldnotes_domain::{Clock, RandomSource};
use fieldnotes_store::{Notebook, read_field_config};

use crate::error::AppError;
use crate::kernel::{Kernel, SELF_FIELD};

use super::{AuthRequirement, Authorizer, CredentialFailure, CredentialSettings};

/// One `fields auth` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthRequest {
    /// The Field to authenticate.
    pub field_id: String,
    /// Whether an interactive authorization may run at all.
    ///
    /// A scheduled or otherwise non-interactive invocation sets this `false`
    /// and is told to run the command from an interactive session, rather than
    /// having a browser opened on a desktop nobody is looking at. The
    /// composition root decides this; nothing in this crate reads the
    /// environment to guess it.
    pub interactive: bool,
}

/// What one successful authorization did. Carries no credential material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthOutcome {
    /// The Field that was authenticated.
    pub field_id: String,
    /// The non-secret profile name the refresh token is now stored under.
    pub profile: String,
    /// Which provider stores it.
    pub provider: String,
    /// The non-secret OAuth client ID that was used.
    pub client_id: String,
    /// The authority tenant segment that was used.
    pub tenant: String,
    /// The scopes that were requested, including `offline_access`.
    pub scopes: Vec<String>,
    /// When the access token minted during authorization expires, if it could
    /// be rendered. The access token itself is discarded here.
    pub access_token_expires_at: Option<String>,
    /// Whether the shared first-party client ID was used, which attributes
    /// reads to that application in the tenant's sign-in logs.
    pub uses_shared_client_id: bool,
}

/// Authenticates one configured Field.
///
/// `authorizer` performs the interactive flow and the storage; the real one is
/// [`super::system::SystemCredentials`].
pub fn authenticate_field<C: Clock, R: RandomSource>(
    kernel: &mut Kernel<C, R>,
    notebook: &Notebook,
    request: &AuthRequest,
    authorizer: &mut dyn Authorizer,
) -> Result<AuthOutcome, AppError> {
    if request.field_id == SELF_FIELD {
        return Err(AppError::CannotConfigureSelf);
    }
    let config = read_field_config(notebook, &request.field_id)?.ok_or_else(|| {
        AppError::FieldNotConfigured {
            id: request.field_id.clone(),
        }
    })?;
    // Configuration first: a Field whose credential configuration is wrong or
    // missing is reported without starting a process.
    let settings = super::settings_from_config(&request.field_id, &config.config)?;
    if !request.interactive {
        return Err(AppError::Credential(CredentialFailure::NotInteractive {
            field_id: request.field_id.clone(),
        }));
    }
    let manifest = crate::sync::describe_field(kernel, &config)?;
    let manifest_json =
        serde_json::to_value(&manifest).map_err(|error| AppError::InvalidManifest {
            message: format!("the reported manifest could not be re-encoded: {error}"),
        })?;
    if let Some(stored) = &config.manifest {
        crate::fields::check_manifest_agreement(stored, &manifest_json)?;
    }
    crate::fields::record_manifest(notebook, &config.id, manifest_json)?;

    let requirement = super::requirement_of_manifest(&request.field_id, &manifest)?;
    authorize_resolved(request, &settings, &requirement, authorizer)
}

/// Authorizes an already-resolved Field: the decision surface, with no process
/// and no notebook access.
///
/// Separate from [`authenticate_field`] so the ordering and the refusals are
/// testable without a Field executable, a tenant, a browser, or a keychain.
pub fn authorize_resolved(
    request: &AuthRequest,
    settings: &CredentialSettings,
    requirement: &AuthRequirement,
    authorizer: &mut dyn Authorizer,
) -> Result<AuthOutcome, AppError> {
    if !request.interactive {
        return Err(AppError::Credential(CredentialFailure::NotInteractive {
            field_id: request.field_id.clone(),
        }));
    }
    if !requirement.required {
        return Err(AppError::Credential(CredentialFailure::Unsupported {
            field_id: request.field_id.clone(),
            detail: "its manifest declares that it needs no credential, so there is nothing to \
                     authenticate"
                .to_owned(),
        }));
    }
    let scopes = requirement.authorization_scopes();
    let authorized = authorizer.authorize(&request.field_id, settings, &scopes)?;
    Ok(AuthOutcome {
        field_id: request.field_id.clone(),
        profile: settings.profile.as_str().to_owned(),
        provider: settings.provider.as_str().to_owned(),
        client_id: settings.client_id.clone(),
        tenant: settings.tenant.clone(),
        scopes: authorized.scopes,
        access_token_expires_at: authorized.access_token_expires_at,
        uses_shared_client_id: settings.uses_shared_client_id(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{Authorized, PROFILE_KEY, PROVIDER_KEY, settings_from_config};
    use std::collections::BTreeMap;

    /// An [`Authorizer`] that records what it was asked for and never touches
    /// a browser, a network, or a keychain.
    struct RecordingAuthorizer {
        requested: Vec<String>,
        answer: Result<Authorized, CredentialFailure>,
    }

    impl RecordingAuthorizer {
        fn granting() -> Self {
            RecordingAuthorizer {
                requested: Vec::new(),
                answer: Ok(Authorized {
                    scopes: Vec::new(),
                    access_token_expires_at: Some("2026-08-23T10:00:00+02:00".to_owned()),
                }),
            }
        }

        fn failing(failure: CredentialFailure) -> Self {
            RecordingAuthorizer {
                requested: Vec::new(),
                answer: Err(failure),
            }
        }
    }

    impl Authorizer for RecordingAuthorizer {
        fn authorize(
            &mut self,
            _field_id: &str,
            _settings: &CredentialSettings,
            scopes: &[String],
        ) -> Result<Authorized, CredentialFailure> {
            self.requested = scopes.to_vec();
            match &self.answer {
                Ok(authorized) => Ok(Authorized {
                    scopes: scopes.to_vec(),
                    access_token_expires_at: authorized.access_token_expires_at.clone(),
                }),
                Err(failure) => Err(failure.clone()),
            }
        }
    }

    fn settings() -> CredentialSettings {
        let mut config = BTreeMap::new();
        config.insert(PROFILE_KEY.to_owned(), "work".to_owned());
        settings_from_config("outlook_mail_work", &config)
            .unwrap_or_else(|error| panic!("fixture configuration must resolve: {error}"))
    }

    fn request(interactive: bool) -> AuthRequest {
        AuthRequest {
            field_id: "outlook_mail_work".to_owned(),
            interactive,
        }
    }

    fn requirement(scopes: &[&str]) -> AuthRequirement {
        AuthRequirement {
            required: true,
            scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
        }
    }

    #[test]
    fn a_successful_authorization_requests_the_declared_scopes_plus_offline_access()
    -> Result<(), AppError> {
        let mut authorizer = RecordingAuthorizer::granting();
        let outcome = authorize_resolved(
            &request(true),
            &settings(),
            &requirement(&["Mail.Read"]),
            &mut authorizer,
        )?;
        assert_eq!(
            authorizer.requested,
            vec!["Mail.Read".to_owned(), "offline_access".to_owned()]
        );
        assert_eq!(outcome.profile, "work");
        assert_eq!(outcome.provider, "keychain");
        assert!(outcome.uses_shared_client_id);
        assert_eq!(
            outcome.access_token_expires_at.as_deref(),
            Some("2026-08-23T10:00:00+02:00")
        );
        Ok(())
    }

    #[test]
    fn a_non_interactive_invocation_is_told_what_to_run_and_never_reaches_the_authorizer() {
        let mut authorizer = RecordingAuthorizer::granting();
        let error = authorize_resolved(
            &request(false),
            &settings(),
            &requirement(&["Mail.Read"]),
            &mut authorizer,
        )
        .err()
        .unwrap_or_else(|| panic!("a non-interactive authorization must be refused"));
        assert_eq!(error.kind(), "credential_not_interactive");
        assert!(
            error
                .to_string()
                .contains("fieldnotes fields auth outlook_mail_work")
        );
        assert!(
            authorizer.requested.is_empty(),
            "nothing may be requested before the refusal"
        );
    }

    #[test]
    fn a_field_needing_no_credential_has_nothing_to_authenticate() {
        let mut authorizer = RecordingAuthorizer::granting();
        let error = authorize_resolved(
            &request(true),
            &settings(),
            &AuthRequirement {
                required: false,
                scopes: Vec::new(),
            },
            &mut authorizer,
        )
        .err()
        .unwrap_or_else(|| panic!("a Field needing no credential must be refused"));
        assert_eq!(error.kind(), "credential_unsupported");
        assert!(authorizer.requested.is_empty());
    }

    #[test]
    fn a_declined_consent_is_reported_as_an_instruction_not_a_backend_error() {
        let mut authorizer = RecordingAuthorizer::failing(CredentialFailure::Denied {
            field_id: "outlook_mail_work".to_owned(),
            profile: "work".to_owned(),
        });
        let error = authorize_resolved(
            &request(true),
            &settings(),
            &requirement(&["Mail.Read"]),
            &mut authorizer,
        )
        .err()
        .unwrap_or_else(|| panic!("a declined authorization must fail"));
        assert_eq!(error.kind(), "credential_denied");
        assert!(
            error
                .to_string()
                .contains("fieldnotes fields auth outlook_mail_work")
        );
    }

    #[test]
    fn the_read_only_environment_provider_cannot_be_authorized_interactively() {
        // The refusal itself lives in the real authorizer, so what this proves
        // is that the settings carry the choice through to it unchanged.
        let mut config = BTreeMap::new();
        config.insert(PROFILE_KEY.to_owned(), "ci_work".to_owned());
        config.insert(PROVIDER_KEY.to_owned(), "environment".to_owned());
        let settings = settings_from_config("outlook_mail_ci", &config)
            .unwrap_or_else(|error| panic!("fixture configuration must resolve: {error}"));
        let mut authorizer = RecordingAuthorizer::failing(CredentialFailure::NotConfigured {
            field_id: "outlook_mail_ci".to_owned(),
            detail: "read-only".to_owned(),
        });
        assert!(
            authorize_resolved(
                &AuthRequest {
                    field_id: "outlook_mail_ci".to_owned(),
                    interactive: true,
                },
                &settings,
                &requirement(&["Mail.Read"]),
                &mut authorizer,
            )
            .is_err()
        );
        assert_eq!(settings.provider.as_str(), "environment");
    }
}
