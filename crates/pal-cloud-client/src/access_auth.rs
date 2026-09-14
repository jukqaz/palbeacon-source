use std::{
    collections::BTreeSet,
    time::{Duration, SystemTime},
};

use reqwest::Url;
use tokio::sync::Mutex;

use crate::{
    AccessAuthError, AccessSessionLease, BrowserLauncher, LoopbackCallback, PalCloudClient,
    RegisteredPublicClient, ValidatedAuthorizationServer,
};

pub const REQUIRED_METADATA_TIMEOUT: Duration = Duration::from_secs(5);
pub const REQUIRED_CALLBACK_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub const MAX_OAUTH_RESPONSE_BYTES: usize = 65_536;
pub const MAX_AUTHORIZATION_SERVER_ORIGINS: usize = 4;
pub const PROTECTED_RESOURCE_METADATA_PATH: &str =
    ".well-known/cloudflare-access-protected-resource/";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginInteraction {
    Allowed,
    Forbidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudSessionState {
    SignedOut,
    Ready { expires_at: SystemTime },
}

/// Strict configuration for one private Cloudflare Access application.
///
/// The resource is intentionally an origin, not an arbitrary API URL. OAuth
/// discovery may only move to an explicitly allowlisted authorization-server
/// origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedOAuthPolicy {
    pub resource: Url,
    pub protected_resource_metadata_url: Url,
    pub allowed_authorization_server_origins: BTreeSet<String>,
    pub metadata_timeout: Duration,
    pub callback_timeout: Duration,
}

impl ManagedOAuthPolicy {
    pub fn new(
        resource: Url,
        allowed_authorization_server_origins: impl IntoIterator<Item = String>,
    ) -> Result<Self, AccessAuthError> {
        let protected_resource_metadata_url = resource
            .join(PROTECTED_RESOURCE_METADATA_PATH)
            .map_err(|_| AccessAuthError::InvalidPolicy)?;
        let allowed_authorization_server_origins = allowed_authorization_server_origins
            .into_iter()
            .map(|raw| {
                let parsed = Url::parse(&raw).map_err(|_| AccessAuthError::InvalidPolicy)?;
                if parsed.as_str() != raw {
                    return Err(AccessAuthError::InvalidPolicy);
                }
                normalized_origin(&parsed)
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        let value = Self {
            resource,
            protected_resource_metadata_url,
            allowed_authorization_server_origins,
            metadata_timeout: REQUIRED_METADATA_TIMEOUT,
            callback_timeout: REQUIRED_CALLBACK_TIMEOUT,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), AccessAuthError> {
        validate_exact_origin(&self.resource)?;
        let expected_metadata = self
            .resource
            .join(PROTECTED_RESOURCE_METADATA_PATH)
            .map_err(|_| AccessAuthError::InvalidPolicy)?;
        if self.protected_resource_metadata_url != expected_metadata
            || self.metadata_timeout != REQUIRED_METADATA_TIMEOUT
            || self.callback_timeout != REQUIRED_CALLBACK_TIMEOUT
            || self.allowed_authorization_server_origins.is_empty()
            || self.allowed_authorization_server_origins.len() > MAX_AUTHORIZATION_SERVER_ORIGINS
        {
            return Err(AccessAuthError::InvalidPolicy);
        }
        for origin in &self.allowed_authorization_server_origins {
            let parsed = Url::parse(origin).map_err(|_| AccessAuthError::InvalidPolicy)?;
            validate_exact_origin(&parsed)?;
            if normalized_origin(&parsed)? != *origin {
                return Err(AccessAuthError::InvalidPolicy);
            }
        }
        Ok(())
    }

    pub fn resource_origin(&self) -> String {
        self.resource.origin().ascii_serialization()
    }

    pub(crate) fn permits_authorization_server(
        &self,
        issuer: &Url,
    ) -> Result<bool, AccessAuthError> {
        Ok(self
            .allowed_authorization_server_origins
            .contains(&normalized_origin(issuer)?))
    }
}

#[derive(Default)]
struct AuthState {
    server: Option<ValidatedAuthorizationServer>,
    client: Option<RegisteredPublicClient>,
    session: Option<AccessSessionLease>,
}

/// Memory-only Access session owner.
///
/// The mutex deliberately spans refresh or interactive authorization so one
/// browser flow serves every concurrent waiter. Nothing in this type can be
/// serialized or returned through local IPC.
pub struct AccessAuthManager<B: BrowserLauncher + 'static> {
    client: PalCloudClient,
    browser: B,
    policy: ManagedOAuthPolicy,
    state: Mutex<AuthState>,
}

impl<B: BrowserLauncher + 'static> AccessAuthManager<B> {
    pub fn new(policy: ManagedOAuthPolicy, browser: B) -> Result<Self, AccessAuthError> {
        let client = PalCloudClient::new(policy.clone())?;
        Ok(Self {
            client,
            browser,
            policy,
            state: Mutex::new(AuthState::default()),
        })
    }

    pub async fn ensure_session(
        &self,
        min_remaining: Duration,
        interaction: LoginInteraction,
    ) -> Result<AccessSessionLease, AccessAuthError> {
        if min_remaining > Duration::from_secs(900) {
            return Err(AccessAuthError::InvalidPolicy);
        }
        let now = SystemTime::now();
        let mut state = self.state.lock().await;
        if let Some(session) = state.session.as_ref()
            && has_remaining(session, now, min_remaining)
            && !session.needs_refresh(now)
        {
            return Ok(session.clone());
        }

        if let (Some(server), Some(client), Some(session)) = (
            state.server.as_ref(),
            state.client.as_ref(),
            state.session.as_ref(),
        ) {
            match self
                .client
                .refresh_session(server, &client.client_id, session)
                .await
            {
                Ok(refreshed) => {
                    state.session = Some(refreshed.clone());
                    return Ok(refreshed);
                }
                Err(_) => {
                    state.session = None;
                }
            }
        }

        if interaction == LoginInteraction::Forbidden {
            return Err(AccessAuthError::LoginRequired);
        }

        let server = match state.server.clone() {
            Some(server) => server,
            None => self.client.discover_authorization_server().await?,
        };
        // The listener is bound before DCR so the registered redirect URI can
        // never race a later ephemeral-port allocation.
        let callback = LoopbackCallback::bind(self.policy.callback_timeout)?;
        let registered = self
            .client
            .register_public_client(&server, &callback)
            .await?;
        let session = self
            .client
            .interactive_login(&server, &registered, callback, &self.browser)
            .await
            .map_err(|_| AccessAuthError::LoginRequired)?;
        state.server = Some(server);
        state.client = Some(registered);
        state.session = Some(session.clone());
        Ok(session)
    }

    pub async fn logout(&self) {
        self.state.lock().await.session = None;
    }

    pub async fn invalidate_on_status(&self, status: reqwest::StatusCode) {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            self.state.lock().await.session = None;
        }
    }

    pub async fn session_state(&self) -> CloudSessionState {
        self.state
            .lock()
            .await
            .session
            .as_ref()
            .map_or(CloudSessionState::SignedOut, |session| {
                CloudSessionState::Ready {
                    expires_at: session.expires_at(),
                }
            })
    }

    pub fn cloud_client(&self) -> &PalCloudClient {
        &self.client
    }
}

impl<B: BrowserLauncher + 'static> std::fmt::Debug for AccessAuthManager<B> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccessAuthManager")
            .field("resource", &self.policy.resource_origin())
            .field("browser", &"<redacted>")
            .field("state", &"<redacted>")
            .finish()
    }
}

impl<B> BrowserLauncher for &B
where
    B: BrowserLauncher,
{
    fn open(&self, authorization_url: &Url) -> Result<(), AccessAuthError> {
        (*self).open(authorization_url)
    }
}

fn has_remaining(session: &AccessSessionLease, now: SystemTime, minimum: Duration) -> bool {
    session
        .expires_at()
        .duration_since(now)
        .is_ok_and(|remaining| remaining > minimum)
}

pub(crate) fn normalized_origin(url: &Url) -> Result<String, AccessAuthError> {
    validate_exact_origin(url)?;
    Ok(url.origin().ascii_serialization())
}

fn validate_exact_origin(url: &Url) -> Result<(), AccessAuthError> {
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(AccessAuthError::InvalidPolicy);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(resource: &str, issuer: &str) -> Result<ManagedOAuthPolicy, AccessAuthError> {
        ManagedOAuthPolicy::new(Url::parse(resource).unwrap(), [issuer.to_owned()])
    }

    #[test]
    fn accepts_one_exact_https_resource_and_allowlisted_origin() {
        let policy = policy(
            "https://pal.example.com/",
            "https://example.cloudflareaccess.com/",
        )
        .unwrap();

        assert_eq!(policy.resource_origin(), "https://pal.example.com");
        assert_eq!(
            policy.protected_resource_metadata_url.as_str(),
            "https://pal.example.com/.well-known/cloudflare-access-protected-resource/"
        );
    }

    #[test]
    fn rejects_non_origin_resources_and_noncanonical_issuer_origins() {
        for resource in [
            "http://pal.example.com/",
            "https://user@pal.example.com/",
            "https://pal.example.com/api",
            "https://pal.example.com/?x=1",
            "https://pal.example.com/#fragment",
        ] {
            assert_eq!(
                policy(resource, "https://example.cloudflareaccess.com/"),
                Err(AccessAuthError::InvalidPolicy),
                "{resource}"
            );
        }
        for issuer in [
            "http://example.cloudflareaccess.com/",
            "https://example.cloudflareaccess.com/oauth",
            "https://example.cloudflareaccess.com/?x=1",
            "https://EXAMPLE.cloudflareaccess.com/",
        ] {
            assert_eq!(
                policy("https://pal.example.com/", issuer),
                Err(AccessAuthError::InvalidPolicy),
                "{issuer}"
            );
        }
    }

    #[test]
    fn rejects_relaxed_timeouts_or_too_many_issuers() {
        let mut policy = policy(
            "https://pal.example.com/",
            "https://example.cloudflareaccess.com/",
        )
        .unwrap();
        policy.metadata_timeout = Duration::from_secs(6);
        assert_eq!(policy.validate(), Err(AccessAuthError::InvalidPolicy));

        policy.metadata_timeout = REQUIRED_METADATA_TIMEOUT;
        policy.allowed_authorization_server_origins = (0..5)
            .map(|index| format!("https://{index}.cloudflareaccess.com/"))
            .collect();
        assert_eq!(policy.validate(), Err(AccessAuthError::InvalidPolicy));
    }

    #[derive(Clone, Copy)]
    struct NeverBrowser;

    impl BrowserLauncher for NeverBrowser {
        fn open(&self, _authorization_url: &Url) -> Result<(), AccessAuthError> {
            Err(AccessAuthError::BrowserLaunchFailed)
        }
    }

    #[tokio::test]
    async fn signed_out_state_and_debug_never_contain_auth_material() {
        let manager = AccessAuthManager::new(
            policy(
                "https://pal.example.com/",
                "https://example.cloudflareaccess.com/",
            )
            .unwrap(),
            NeverBrowser,
        )
        .unwrap();
        assert_eq!(manager.session_state().await, CloudSessionState::SignedOut);
        let rendered = format!("{manager:?}");
        assert!(rendered.contains("https://pal.example.com"));
        assert!(rendered.contains("<redacted>"));
    }

    #[tokio::test]
    async fn forbidden_interaction_never_opens_a_browser() {
        let manager = AccessAuthManager::new(
            policy(
                "https://pal.example.com/",
                "https://example.cloudflareaccess.com/",
            )
            .unwrap(),
            NeverBrowser,
        )
        .unwrap();
        assert!(matches!(
            manager
                .ensure_session(Duration::from_secs(1), LoginInteraction::Forbidden)
                .await,
            Err(AccessAuthError::LoginRequired)
        ));
    }
}
