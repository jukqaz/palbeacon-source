use std::{sync::Arc, time::SystemTime};

use pal_companion_service::{GroundedAssistantRequest, GroundedAssistantResponse};
use reqwest::{
    Client, RequestBuilder, StatusCode, Url,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use zeroize::Zeroizing;

use crate::{
    AccessAuthError, AuthorizationServerMetadata, BrowserLauncher, CloudError, CloudErrorCode,
    DiscoveryChallenge, DynamicClientRegistration, LoopbackCallback, ManagedOAuthPolicy,
    PkceMaterial, ProtectedResourceMetadata, RegisteredPublicClient, ValidatedAuthorizationServer,
    access_auth::MAX_OAUTH_RESPONSE_BYTES,
    oauth_registration::MAX_CLIENT_ID_BYTES,
    oauth_tokens::{OAuthTokenResponse, OAuthTokenSet},
};

const JSON_CONTENT_TYPE: &str = "application/json";
const MAX_AUTHORIZATION_CODE_BYTES: usize = 8_192;

/// Opaque authenticated session handle. It deliberately exposes no token,
/// cookie, URL parameter, serialization or Debug representation.
pub struct AccessSessionLease {
    session: Arc<OAuthTokenSet>,
}

impl AccessSessionLease {
    pub fn from_token_response(
        response: OAuthTokenResponse,
        received_at: SystemTime,
    ) -> Result<Self, AccessAuthError> {
        Ok(Self {
            session: Arc::new(OAuthTokenSet::from_response(response, received_at)?),
        })
    }

    pub fn expires_at(&self) -> SystemTime {
        self.session.expires_at()
    }

    pub fn needs_refresh(&self, now: SystemTime) -> bool {
        self.session.needs_refresh(now)
    }

    fn authorize_http(&self, request: RequestBuilder) -> Result<RequestBuilder, CloudError> {
        let value = Zeroizing::new(format!("Bearer {}", self.session.access_token()));
        let header = HeaderValue::from_str(&value)
            .map_err(|_| CloudError::new(CloudErrorCode::ProtocolRejected))?;
        Ok(request.header(AUTHORIZATION, header))
    }

    pub(crate) fn refresh_token(&self) -> &str {
        self.session.refresh_token()
    }
}

impl Clone for AccessSessionLease {
    fn clone(&self) -> Self {
        Self {
            session: Arc::clone(&self.session),
        }
    }
}

impl std::fmt::Debug for AccessSessionLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccessSessionLease")
            .field("session", &"<redacted>")
            .finish()
    }
}

pub struct AuthorizationCodeGrant {
    code: SecretString,
    pkce: PkceMaterial,
}

impl AuthorizationCodeGrant {
    pub fn new(code: SecretString, pkce: PkceMaterial) -> Result<Self, AccessAuthError> {
        if code.expose_secret().is_empty()
            || code.expose_secret().len() > MAX_AUTHORIZATION_CODE_BYTES
            || code
                .expose_secret()
                .bytes()
                .any(|byte| byte.is_ascii_control())
        {
            return Err(AccessAuthError::InvalidTokenResponse);
        }
        Ok(Self { code, pkce })
    }
}

impl std::fmt::Debug for AuthorizationCodeGrant {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AuthorizationCodeGrant")
            .field("code", &"<redacted>")
            .field("pkce", &"<redacted>")
            .finish()
    }
}

pub struct PalCloudClient {
    client: Client,
    policy: ManagedOAuthPolicy,
}

impl PalCloudClient {
    pub fn new(policy: ManagedOAuthPolicy) -> Result<Self, AccessAuthError> {
        policy.validate()?;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(policy.metadata_timeout)
            .timeout(policy.metadata_timeout)
            .build()
            .map_err(|_| AccessAuthError::InvalidPolicy)?;
        Ok(Self { client, policy })
    }

    /// Sends only the bounded grounded-assistant contract. This is not a
    /// generic HTTP tunnel and callers cannot supply a URL or headers.
    pub async fn grounded_assistant(
        &self,
        lease: &AccessSessionLease,
        request: &GroundedAssistantRequest,
    ) -> Result<GroundedAssistantResponse, CloudError> {
        request
            .validate()
            .map_err(|_| CloudError::new(CloudErrorCode::ProtocolRejected))?;
        let endpoint = exact_api_url(&self.policy.resource, "api/v1/me/assistant/grounded")?;
        let builder = self
            .client
            .post(endpoint)
            .header(CONTENT_TYPE, JSON_CONTENT_TYPE)
            .json(request);
        let response = lease
            .authorize_http(builder)?
            .send()
            .await
            .map_err(|_| CloudError::new(CloudErrorCode::NetworkUnavailable))?;
        decode_json_response(response).await
    }

    pub async fn discover_authorization_server(
        &self,
    ) -> Result<ValidatedAuthorizationServer, AccessAuthError> {
        let session_endpoint = self
            .policy
            .resource
            .join("v1/native/session")
            .map_err(|_| AccessAuthError::InvalidPolicy)?;
        let response = self
            .client
            .get(session_endpoint)
            .send()
            .await
            .map_err(|_| AccessAuthError::InvalidMetadata)?;
        let challenge =
            DiscoveryChallenge::parse(response.status(), response.headers(), &self.policy)?;
        let protected: ProtectedResourceMetadata = self
            .get_oauth_json(challenge.protected_resource_metadata_url)
            .await?;
        let issuer = protected.validate(&self.policy)?;
        let authorization_metadata_url = issuer
            .join(".well-known/oauth-authorization-server")
            .map_err(|_| AccessAuthError::InvalidMetadata)?;
        let server: AuthorizationServerMetadata =
            self.get_oauth_json(authorization_metadata_url).await?;
        server.validate(&issuer, &self.policy)
    }

    pub async fn register_public_client(
        &self,
        server: &ValidatedAuthorizationServer,
        callback: &LoopbackCallback,
    ) -> Result<RegisteredPublicClient, AccessAuthError> {
        let registration =
            DynamicClientRegistration::loopback(callback.redirect_uri(), &self.policy.resource)?;
        let response = self
            .client
            .post(server.registration_endpoint.clone())
            .header(CONTENT_TYPE, JSON_CONTENT_TYPE)
            .json(&registration)
            .send()
            .await
            .map_err(|_| AccessAuthError::InvalidRegistration)?;
        if !response.status().is_success() {
            return Err(AccessAuthError::InvalidRegistration);
        }
        let registered: RegisteredPublicClient =
            decode_oauth_json(response, AccessAuthError::InvalidRegistration).await?;
        registered.validate()
    }

    pub async fn interactive_login<B: BrowserLauncher>(
        &self,
        server: &ValidatedAuthorizationServer,
        registered: &RegisteredPublicClient,
        callback: LoopbackCallback,
        browser: B,
    ) -> Result<AccessSessionLease, CloudError> {
        validate_client_id(&registered.client_id)?;
        if registered
            .client_secret
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        {
            return Err(CloudError::new(CloudErrorCode::ProtocolRejected));
        }
        let pkce = PkceMaterial::generate()
            .map_err(|_| CloudError::new(CloudErrorCode::ProtocolRejected))?;
        let mut authorization_url = server.authorization_endpoint.clone();
        {
            let mut query = authorization_url.query_pairs_mut();
            query
                .clear()
                .append_pair("response_type", "code")
                .append_pair("client_id", &registered.client_id)
                .append_pair("redirect_uri", callback.redirect_uri().as_str())
                .append_pair("code_challenge", pkce.challenge())
                .append_pair("code_challenge_method", "S256")
                .append_pair("state", callback.state_parameter())
                .append_pair("resource", self.policy.resource.as_str());
        }
        if authorization_url.as_str().len() > 8 * 1024 {
            return Err(CloudError::new(CloudErrorCode::ProtocolRejected));
        }
        browser
            .open(&authorization_url)
            .map_err(|_| CloudError::new(CloudErrorCode::LoginRequired))?;
        let redirect_uri = callback.redirect_uri().clone();
        let code = tokio::task::spawn_blocking(move || callback.wait())
            .await
            .map_err(|_| CloudError::new(CloudErrorCode::LoginRequired))?
            .map_err(|_| CloudError::new(CloudErrorCode::LoginRequired))?;
        let grant = AuthorizationCodeGrant::new(code, pkce)
            .map_err(|_| CloudError::new(CloudErrorCode::ProtocolRejected))?;
        self.exchange_authorization_code(server, &registered.client_id, &redirect_uri, grant)
            .await
    }

    pub async fn exchange_authorization_code(
        &self,
        server: &ValidatedAuthorizationServer,
        client_id: &str,
        redirect_uri: &Url,
        grant: AuthorizationCodeGrant,
    ) -> Result<AccessSessionLease, CloudError> {
        validate_client_and_redirect(client_id, redirect_uri)?;
        let form = AuthorizationCodeForm {
            grant_type: "authorization_code",
            client_id,
            redirect_uri: redirect_uri.as_str(),
            code: grant.code.expose_secret(),
            code_verifier: grant.pkce.verifier(),
            resource: self.policy.resource.as_str(),
        };
        let response = self
            .client
            .post(server.token_endpoint.clone())
            .form(&form)
            .send()
            .await
            .map_err(|_| CloudError::new(CloudErrorCode::NetworkUnavailable))?;
        let token: OAuthTokenResponse = decode_json_response(response).await?;
        AccessSessionLease::from_token_response(token, SystemTime::now())
            .map_err(|_| CloudError::new(CloudErrorCode::ProtocolRejected))
    }

    pub async fn refresh_session(
        &self,
        server: &ValidatedAuthorizationServer,
        client_id: &str,
        current: &AccessSessionLease,
    ) -> Result<AccessSessionLease, CloudError> {
        validate_client_id(client_id)?;
        let form = RefreshTokenForm {
            grant_type: "refresh_token",
            client_id,
            refresh_token: current.refresh_token(),
            resource: self.policy.resource.as_str(),
        };
        let response = self
            .client
            .post(server.token_endpoint.clone())
            .form(&form)
            .send()
            .await
            .map_err(|_| CloudError::new(CloudErrorCode::NetworkUnavailable))?;
        let refreshed: OAuthRefreshResponse = decode_json_response(response).await?;
        let token = OAuthTokenResponse {
            token_type: refreshed.token_type,
            access_token: refreshed.access_token,
            refresh_token: refreshed
                .refresh_token
                .unwrap_or_else(|| current.refresh_token().to_owned()),
            expires_in: refreshed.expires_in,
        };
        AccessSessionLease::from_token_response(token, SystemTime::now())
            .map_err(|_| CloudError::new(CloudErrorCode::ProtocolRejected))
    }

    async fn get_oauth_json<T: DeserializeOwned>(
        &self,
        endpoint: Url,
    ) -> Result<T, AccessAuthError> {
        let response = self
            .client
            .get(endpoint)
            .send()
            .await
            .map_err(|_| AccessAuthError::InvalidMetadata)?;
        if !response.status().is_success() {
            return Err(AccessAuthError::InvalidMetadata);
        }
        decode_oauth_json(response, AccessAuthError::InvalidMetadata).await
    }
}

#[derive(Serialize)]
struct AuthorizationCodeForm<'a> {
    grant_type: &'static str,
    client_id: &'a str,
    redirect_uri: &'a str,
    code: &'a str,
    code_verifier: &'a str,
    resource: &'a str,
}

#[derive(Serialize)]
struct RefreshTokenForm<'a> {
    grant_type: &'static str,
    client_id: &'a str,
    refresh_token: &'a str,
    resource: &'a str,
}

#[derive(Deserialize)]
struct OAuthRefreshResponse {
    token_type: String,
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    expires_in: u64,
}

fn validate_client_and_redirect(client_id: &str, redirect_uri: &Url) -> Result<(), CloudError> {
    validate_client_id(client_id)?;
    if redirect_uri.scheme() != "http"
        || redirect_uri.host_str() != Some("127.0.0.1")
        || redirect_uri.port().is_none()
        || !redirect_uri.username().is_empty()
        || redirect_uri.password().is_some()
        || redirect_uri.query().is_some()
        || redirect_uri.fragment().is_some()
        || !redirect_uri.path().starts_with("/oauth/callback/")
    {
        return Err(CloudError::new(CloudErrorCode::ProtocolRejected));
    }
    Ok(())
}

fn validate_client_id(client_id: &str) -> Result<(), CloudError> {
    if client_id.is_empty()
        || client_id.len() > MAX_CLIENT_ID_BYTES
        || client_id.bytes().any(|byte| byte.is_ascii_control())
    {
        Err(CloudError::new(CloudErrorCode::ProtocolRejected))
    } else {
        Ok(())
    }
}

fn exact_api_url(resource: &Url, path: &str) -> Result<Url, CloudError> {
    let endpoint = resource
        .join(path)
        .map_err(|_| CloudError::new(CloudErrorCode::ProtocolRejected))?;
    if endpoint.origin() != resource.origin()
        || endpoint.scheme() != "https"
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(CloudError::new(CloudErrorCode::ProtocolRejected));
    }
    Ok(endpoint)
}

async fn decode_json_response<T: DeserializeOwned>(
    mut response: reqwest::Response,
) -> Result<T, CloudError> {
    let status = response.status();
    if !status.is_success() {
        return Err(CloudError::new(map_status(status)));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim();
    if content_type != JSON_CONTENT_TYPE {
        return Err(CloudError::new(CloudErrorCode::ProtocolRejected));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_OAUTH_RESPONSE_BYTES as u64)
    {
        return Err(CloudError::new(CloudErrorCode::ProtocolRejected));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| CloudError::new(CloudErrorCode::NetworkUnavailable))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_OAUTH_RESPONSE_BYTES {
            return Err(CloudError::new(CloudErrorCode::ProtocolRejected));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| CloudError::new(CloudErrorCode::ProtocolRejected))
}

async fn decode_oauth_json<T: DeserializeOwned>(
    mut response: reqwest::Response,
    error: AccessAuthError,
) -> Result<T, AccessAuthError> {
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim();
    if content_type != JSON_CONTENT_TYPE
        || response
            .content_length()
            .is_some_and(|length| length > MAX_OAUTH_RESPONSE_BYTES as u64)
    {
        return Err(error);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| error.clone())? {
        if bytes.len().saturating_add(chunk.len()) > MAX_OAUTH_RESPONSE_BYTES {
            return Err(error);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| error)
}

fn map_status(status: StatusCode) -> CloudErrorCode {
    match status {
        StatusCode::UNAUTHORIZED => CloudErrorCode::AccessSessionInvalid,
        StatusCode::FORBIDDEN => CloudErrorCode::AuthenticatedForbidden,
        StatusCode::CONFLICT => CloudErrorCode::SourceConflict,
        StatusCode::TOO_MANY_REQUESTS => CloudErrorCode::QuotaExceeded,
        _ => CloudErrorCode::ProtocolRejected,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        time::{Duration, SystemTime},
    };

    use reqwest::Url;

    use super::*;

    fn lease() -> AccessSessionLease {
        AccessSessionLease::from_token_response(
            OAuthTokenResponse {
                token_type: "Bearer".to_owned(),
                access_token: "ACCESS_MARKER".to_owned(),
                refresh_token: "REFRESH_MARKER".to_owned(),
                expires_in: 300,
            },
            SystemTime::UNIX_EPOCH,
        )
        .unwrap()
    }

    #[test]
    fn authorized_request_has_one_bearer_header_and_no_url_secret() {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let request = lease()
            .authorize_http(client.get("https://pal.example.com/api/v1/me"))
            .unwrap()
            .build()
            .unwrap();
        let values = request.headers().get_all(AUTHORIZATION);
        assert_eq!(values.iter().count(), 1);
        assert_eq!(
            values.iter().next().unwrap().to_str().unwrap(),
            "Bearer ACCESS_MARKER"
        );
        assert!(!request.url().as_str().contains("ACCESS_MARKER"));
        assert!(request.headers().get("cookie").is_none());
    }

    #[test]
    fn lease_debug_redacts_both_tokens_and_clone_shares_expiry() {
        let lease = lease();
        let clone = lease.clone();
        let rendered = format!("{lease:?}");
        assert!(!rendered.contains("ACCESS_MARKER"));
        assert!(!rendered.contains("REFRESH_MARKER"));
        assert_eq!(clone.expires_at(), lease.expires_at());
        assert_eq!(lease.refresh_token(), "REFRESH_MARKER");
    }

    #[test]
    fn cloud_client_disables_redirects_and_accepts_only_strict_policy() {
        let policy = ManagedOAuthPolicy::new(
            Url::parse("https://pal.example.com/").unwrap(),
            BTreeSet::from(["https://team.cloudflareaccess.com/".to_owned()]),
        )
        .unwrap();
        assert!(PalCloudClient::new(policy).is_ok());
    }

    #[test]
    fn refresh_window_stays_sixty_seconds() {
        let received = SystemTime::UNIX_EPOCH;
        let lease = AccessSessionLease::from_token_response(
            OAuthTokenResponse {
                token_type: "bearer".to_owned(),
                access_token: "a".to_owned(),
                refresh_token: "r".to_owned(),
                expires_in: 120,
            },
            received,
        )
        .unwrap();
        assert!(!lease.needs_refresh(received + Duration::from_secs(59)));
        assert!(lease.needs_refresh(received + Duration::from_secs(60)));
    }
}
