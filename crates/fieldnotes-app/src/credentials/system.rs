//! The real credential collaborators: platform keychain, HTTPS token endpoint,
//! system browser.
//!
//! Everything in this module touches something a test must not: the developer's
//! keychain, the network, or a browser window. It is therefore deliberately
//! thin — it selects a provider, builds an
//! [`AccessTokenProvider`],
//! runs the loopback listener, and translates failures into
//! [`CredentialFailure`]. Every decision worth testing lives in
//! [`super`] and in [`super::auth`], behind the [`Authorizer`],
//! [`AccessTokenSource`], and [`CredentialInspector`] traits this module
//! implements.
//!
//! # The browser is opened here, and nowhere else
//!
//! `fieldnotes-credentials` deliberately does not launch a browser; that is the
//! composition root's job, and this is the closest thing to one that can still
//! be shared between entry points. The URL is built from non-secret values
//! (client ID, endpoint, scopes, the PKCE challenge, and the CSRF `state`) and
//! is always printed as well as opened, so a user on a machine where the
//! opener fails can still finish the flow.
//!
//! No entry point that is not the interactive `fields auth` command ever
//! constructs an [`Authorizer`]: a collection run only ever refreshes silently,
//! so a scheduled `sync` cannot open a window on someone's desktop.

use std::time::Duration;

use fieldnotes_credentials::oauth::{
    AccessToken, AccessTokenProvider, AuthorizeRequest, LoopbackError, LoopbackListener,
    UreqTokenTransport,
};
use fieldnotes_credentials::pkce::{CodeVerifier, State};
use fieldnotes_credentials::{
    CredentialError, CredentialProvider, env::EnvCredentialProvider,
    keychain::KeyringCredentialProvider,
};
use fieldnotes_domain::{Clock, Datetime, RandomSource};

use super::{
    AccessTokenSource, Authorized, Authorizer, CredentialFailure, CredentialInspector,
    CredentialSettings, CredentialState, KEYCHAIN_SERVICE, ProviderChoice,
};

/// How long the loopback listener waits for the browser to come back.
///
/// Long enough for a real sign-in with a second factor, and bounded so a user
/// who closed the window gets an answer instead of a hung terminal.
pub const DEFAULT_CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

/// Builds the credential provider the settings select.
///
/// The only two providers that exist: the platform keychain, and the explicit
/// opt-in environment variable. There is deliberately no third, and no silent
/// fallback between them — `docs/security.md` requires that an unavailable
/// keychain be explained rather than quietly replaced.
fn provider_for(settings: &CredentialSettings) -> Box<dyn CredentialProvider> {
    match &settings.provider {
        ProviderChoice::Keychain => Box::new(KeyringCredentialProvider::new(KEYCHAIN_SERVICE)),
        ProviderChoice::Environment { variable } => {
            Box::new(EnvCredentialProvider::new(variable.clone()))
        }
    }
}

/// Reads the credential store to answer whether a profile is authenticated.
fn probe(settings: &CredentialSettings) -> CredentialState {
    match provider_for(settings).retrieve(&settings.profile) {
        Ok(_) => CredentialState::Stored,
        Err(CredentialError::Absent) => CredentialState::Absent,
        Err(CredentialError::Unavailable(reason)) => CredentialState::Unavailable(reason),
        Err(other) => CredentialState::Unavailable(other.to_string()),
    }
}

/// Mints an access token from a stored refresh token: the real keychain, the
/// real token endpoint.
///
/// Owns its clock rather than borrowing one, so it can be shared (as an
/// [`std::sync::Arc`]) by a whole `sync` invocation. This is the value a
/// composition root injects into
/// [`SyncOptions::credentials`](crate::sync::SyncOptions::credentials).
pub struct SystemTokenSource<C> {
    clock: C,
    transport: UreqTokenTransport,
}

impl<C: Clock> SystemTokenSource<C> {
    /// Builds the real token source over an owned clock.
    #[must_use]
    pub fn new(clock: C) -> Self {
        SystemTokenSource {
            clock,
            transport: UreqTokenTransport::default(),
        }
    }
}

impl<C: Clock> AccessTokenSource for SystemTokenSource<C> {
    fn mint(
        &self,
        field_id: &str,
        settings: &CredentialSettings,
    ) -> Result<AccessToken, CredentialFailure> {
        let provider = provider_for(settings);
        let broker = AccessTokenProvider::new(
            provider.as_ref(),
            &self.transport,
            &self.clock,
            settings.token_endpoint(),
            settings.client_id.clone(),
        );
        broker
            .mint_access_token(&settings.profile)
            .map_err(|error| {
                CredentialFailure::from_credential_error(
                    field_id,
                    settings.profile.as_str(),
                    &error,
                )
            })
    }
}

impl<C: Clock> CredentialInspector for SystemTokenSource<C> {
    fn state(&self, settings: &CredentialSettings) -> CredentialState {
        probe(settings)
    }
}

/// The real credential collaborators.
pub struct SystemCredentials<'a> {
    clock: &'a dyn Clock,
    random: &'a mut dyn RandomSource,
    announce: &'a dyn Fn(&str),
    transport: UreqTokenTransport,
    offset_minutes: i16,
    open_browser: bool,
    callback_timeout: Duration,
}

impl<'a> SystemCredentials<'a> {
    /// Builds the real collaborators.
    ///
    /// `announce` receives non-secret progress text (the authorize URL, and
    /// what is about to happen). `offset_minutes` is the client-local UTC
    /// offset reported expiry instants are rendered in, so `fields auth`
    /// output matches every other datetime Fieldnotes prints.
    pub fn new(
        clock: &'a dyn Clock,
        random: &'a mut dyn RandomSource,
        announce: &'a dyn Fn(&str),
        offset_minutes: i16,
        open_browser: bool,
    ) -> Self {
        SystemCredentials {
            clock,
            random,
            announce,
            transport: UreqTokenTransport::default(),
            offset_minutes,
            open_browser,
            callback_timeout: DEFAULT_CALLBACK_TIMEOUT,
        }
    }

    /// Overrides how long the loopback listener waits for the redirect.
    #[must_use]
    pub fn with_callback_timeout(mut self, timeout: Duration) -> Self {
        self.callback_timeout = timeout;
        self
    }

    fn render_expiry(&self, token: &AccessToken) -> Option<String> {
        Datetime::from_unix_millis(token.expires_at_unix_millis(), self.offset_minutes)
            .ok()
            .map(|datetime| datetime.to_string())
    }
}

impl CredentialInspector for SystemCredentials<'_> {
    fn state(&self, settings: &CredentialSettings) -> CredentialState {
        probe(settings)
    }
}

impl Authorizer for SystemCredentials<'_> {
    fn authorize(
        &mut self,
        field_id: &str,
        settings: &CredentialSettings,
        scopes: &[String],
    ) -> Result<Authorized, CredentialFailure> {
        if let ProviderChoice::Environment { variable } = &settings.provider {
            return Err(CredentialFailure::NotConfigured {
                field_id: field_id.to_owned(),
                detail: format!(
                    "this profile uses the explicit environment-variable provider, which is \
                     read-only and cannot be written by an interactive authorization. Authenticate \
                     on a machine with a keychain and place the resulting refresh token in \
                     {variable} out of band, or set `--config {}=keychain`.",
                    super::PROVIDER_KEY
                ),
            });
        }

        // The listener binds first, because its ephemeral port is part of the
        // redirect URI the authorize request has to carry.
        let listener = LoopbackListener::bind(settings.redirect_path.clone()).map_err(|error| {
            CredentialFailure::Backend {
                profile: settings.profile.as_str().to_owned(),
                reason: format!("could not open the loopback redirect listener: {error}"),
            }
        })?;
        let redirect_uri = listener
            .redirect_uri()
            .map_err(|error| CredentialFailure::Backend {
                profile: settings.profile.as_str().to_owned(),
                reason: format!("could not read the loopback redirect address: {error}"),
            })?;

        let verifier = CodeVerifier::generate(self.random);
        let state = State::generate(self.random);
        let url = AuthorizeRequest {
            authorize_endpoint: settings.authorize_endpoint(),
            client_id: settings.client_id.clone(),
            redirect_uri: redirect_uri.clone(),
            scopes: scopes.to_vec(),
            state: state.clone(),
            code_challenge: verifier.challenge(),
        }
        .url();

        (self.announce)(&format!(
            "Authenticating Field {field_id} as profile `{}`.\n  scopes    {}\n  redirect  \
             {redirect_uri}\nIf your browser does not open, visit this URL:\n  {url}\n",
            settings.profile,
            scopes.join(" ")
        ));
        if self.open_browser {
            open_url(&url);
        }

        let callback = listener
            .await_callback(&state, self.callback_timeout)
            .map_err(|error| loopback_failure(field_id, settings, error))?;

        let provider = provider_for(settings);
        let broker = AccessTokenProvider::new(
            provider.as_ref(),
            &self.transport,
            self.clock,
            settings.token_endpoint(),
            settings.client_id.clone(),
        );
        let authorization = broker
            .complete_authorization(
                &settings.profile,
                &callback.code,
                &redirect_uri,
                verifier.as_str(),
            )
            .map_err(|error| {
                CredentialFailure::from_credential_error(
                    field_id,
                    settings.profile.as_str(),
                    &error,
                )
            })?;
        Ok(Authorized {
            scopes: scopes.to_vec(),
            access_token_expires_at: self.render_expiry(&authorization.access_token),
            // The non-secret account name, and only that. The ID token it was
            // read from never reached this crate: `fieldnotes-credentials`
            // confines it to the function that parses the token-endpoint
            // response, and hands back the extracted label alone.
            account: authorization
                .account
                .map(|account| account.as_str().to_owned()),
        })
    }
}

fn loopback_failure(
    field_id: &str,
    settings: &CredentialSettings,
    error: LoopbackError,
) -> CredentialFailure {
    match error {
        LoopbackError::AuthorizationDenied { .. } => CredentialFailure::Denied {
            field_id: field_id.to_owned(),
            profile: settings.profile.as_str().to_owned(),
        },
        other => CredentialFailure::Backend {
            profile: settings.profile.as_str().to_owned(),
            reason: other.to_string(),
        },
    }
}

/// Opens `url` in the user's browser, best effort.
///
/// Best effort on purpose: the URL was printed first, so a failed launch costs
/// a copy and paste rather than the whole flow. The URL is always passed as a
/// single argument to a program that takes a URL — never interpolated into a
/// shell command line — so nothing in it can become a second command. On
/// Windows that means `rundll32 url.dll,FileProtocolHandler` rather than
/// `cmd /C start`, which would put the URL through the command interpreter's
/// own quoting rules.
fn open_url(url: &str) {
    let mut command = if cfg!(target_os = "macos") {
        let mut command = std::process::Command::new("open");
        command.arg(url);
        command
    } else if cfg!(target_os = "windows") {
        let mut command = std::process::Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler").arg(url);
        command
    } else {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(url);
        command
    };
    let _ = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
