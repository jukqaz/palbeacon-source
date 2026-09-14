use std::collections::HashSet;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use bytes::Bytes;
use http_body_util::{BodyExt as _, Empty};
use hyper::{
    Method, Request, StatusCode,
    header::{
        ACCEPT, ACCEPT_ENCODING, AUTHORIZATION, CONNECTION, CONTENT_ENCODING, CONTENT_LENGTH,
        CONTENT_TYPE, HOST, HeaderMap, HeaderValue,
    },
};
use hyper_util::rt::TokioIo;
use tokio::{net::TcpStream, time::timeout};

use crate::{
    EndpointKind, LiveSnapshotRequest, PlayerSelector, Pseudonymizer, RestError,
    SafePlayerObservation, SanitizedServerInfo, ServerLiveGameDataV1, ServerLiveInfoV1,
    ServerLiveMetricsV1, ServerLivePlayerV1, ServerLiveSectionV1, ServerLiveSettingsV1,
    ServerLiveSnapshotV1, ServerMetrics, ServerSettings, decode_info, decode_live_game_data,
    decode_live_info, decode_live_metrics, decode_live_players, decode_live_settings,
    decode_metrics, decode_selected_player, decode_settings, sanitize_player, sanitize_server_info,
};

const DEFAULT_MAX_BODY_BYTES: usize = 32 * 1024 * 1024;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_AUTH_COMPONENT_BYTES: usize = 4 * 1024;

/// Verifies that a newly connected socket terminates at the trusted server.
///
/// The client invokes this after the TCP handshake but before constructing or
/// sending any authenticated HTTP request. Implementations must verify this
/// exact full tuple; checking only the current listener is insufficient.
pub trait ConnectedStreamVerifier: Send + Sync + 'static {
    fn verify(&self, local_address: SocketAddr, peer_address: SocketAddr) -> bool;
}

/// Basic Auth material that is redacted in formatting and overwritten on drop.
pub struct BasicAuthSecret {
    username: Vec<u8>,
    password: Vec<u8>,
}

impl BasicAuthSecret {
    pub fn new(
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Result<Self, RestError> {
        let secret = Self {
            username: username.into().into_bytes(),
            password: password.into().into_bytes(),
        };
        if secret.username.is_empty()
            || secret.password.is_empty()
            || secret.username.len() > MAX_AUTH_COMPONENT_BYTES
            || secret.password.len() > MAX_AUTH_COMPONENT_BYTES
            || secret.username.contains(&b':')
            || secret
                .username
                .iter()
                .chain(&secret.password)
                .any(u8::is_ascii_control)
        {
            return Err(RestError::InvalidClientConfig);
        }
        Ok(secret)
    }

    fn username(&self) -> &str {
        std::str::from_utf8(&self.username).expect("constructed from a Rust String")
    }

    fn password(&self) -> &str {
        std::str::from_utf8(&self.password).expect("constructed from a Rust String")
    }
}

impl fmt::Debug for BasicAuthSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

impl fmt::Display for BasicAuthSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

impl Drop for BasicAuthSecret {
    fn drop(&mut self) {
        self.username.fill(0);
        self.password.fill(0);
    }
}

/// Explicit endpoint policy.
///
/// Loopback is allowed by default. An RFC1918 address or a remote HTTPS base
/// URL must be listed exactly before credentials can be attached to requests.
#[derive(Clone, Default)]
pub struct EndpointPolicy {
    private_allowlist: HashSet<IpAddr>,
    remote_https_allowlist: HashSet<String>,
}

impl fmt::Debug for EndpointPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EndpointPolicy")
            .field("private_allowlist", &"[REDACTED]")
            .field("private_allowlist_count", &self.private_allowlist.len())
            .field("remote_https_allowlist", &"[REDACTED]")
            .field(
                "remote_https_allowlist_count",
                &self.remote_https_allowlist.len(),
            )
            .finish()
    }
}

impl EndpointPolicy {
    pub fn loopback_only() -> Self {
        Self::default()
    }

    pub fn allow_private_ip(mut self, address: IpAddr) -> Result<Self, RestError> {
        if !is_rfc1918(address) {
            return Err(RestError::EndpointNotAllowed);
        }
        self.private_allowlist.insert(address);
        Ok(self)
    }

    pub fn allow_exact_remote_https(mut self, base_url: &str) -> Result<Self, RestError> {
        self.remote_https_allowlist
            .insert(normalize_remote_https_base_url(base_url)?);
        Ok(self)
    }

    fn permits_local_address(&self, address: IpAddr) -> bool {
        address.is_loopback() || self.private_allowlist.contains(&address)
    }

    fn permits_remote_https(&self, normalized_base_url: &str) -> bool {
        self.remote_https_allowlist.contains(normalized_base_url)
    }
}

#[derive(Clone)]
pub struct ClientConfig {
    base_url: reqwest::Url,
    explicit_insecure_http_endpoint: Option<SocketAddr>,
    connect_timeout: Duration,
    request_timeout: Duration,
    max_decoded_body_bytes: usize,
}

impl fmt::Debug for ClientConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientConfig")
            .field("base_url", &"[REDACTED]")
            .field(
                "has_explicit_insecure_http_endpoint",
                &self.explicit_insecure_http_endpoint.is_some(),
            )
            .field("connect_timeout", &self.connect_timeout)
            .field("request_timeout", &self.request_timeout)
            .field("max_decoded_body_bytes", &self.max_decoded_body_bytes)
            .finish()
    }
}

impl ClientConfig {
    pub fn loopback(base_url: &str) -> Result<Self, RestError> {
        Self::new(base_url, EndpointPolicy::loopback_only())
    }

    pub fn new(base_url: &str, policy: EndpointPolicy) -> Result<Self, RestError> {
        let mut base_url = reqwest::Url::parse(base_url).map_err(|_| RestError::InvalidEndpoint)?;
        validate_endpoint(&base_url, &policy)?;
        if base_url.path() == "/v1/api" {
            base_url.set_path("/v1/api/");
        }
        Ok(Self {
            base_url,
            explicit_insecure_http_endpoint: None,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            max_decoded_body_bytes: DEFAULT_MAX_BODY_BYTES,
        })
    }

    /// Builds a client configuration bound to one exact literal HTTP socket.
    /// This is an explicit cleartext exception and cannot be created from the
    /// broader endpoint policy used by probes and tests.
    pub fn explicit_insecure_http(endpoint: SocketAddr) -> Result<Self, RestError> {
        if endpoint.port() == 0 {
            return Err(RestError::InvalidEndpoint);
        }
        let base_url = reqwest::Url::parse(&format!("http://{endpoint}/v1/api/"))
            .map_err(|_| RestError::InvalidEndpoint)?;
        validate_explicit_insecure_http_endpoint(&base_url, endpoint)?;
        Ok(Self {
            base_url,
            explicit_insecure_http_endpoint: Some(endpoint),
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            max_decoded_body_bytes: DEFAULT_MAX_BODY_BYTES,
        })
    }

    pub const fn max_decoded_body_bytes(&self) -> usize {
        self.max_decoded_body_bytes
    }

    pub const fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    pub const fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub fn with_max_decoded_body_bytes(mut self, limit: usize) -> Result<Self, RestError> {
        if limit == 0 {
            return Err(RestError::InvalidClientConfig);
        }
        self.max_decoded_body_bytes = limit;
        Ok(self)
    }

    pub fn with_timeouts(
        mut self,
        connect_timeout: Duration,
        request_timeout: Duration,
    ) -> Result<Self, RestError> {
        if connect_timeout.is_zero() || request_timeout.is_zero() {
            return Err(RestError::InvalidClientConfig);
        }
        self.connect_timeout = connect_timeout;
        self.request_timeout = request_timeout;
        Ok(self)
    }

    fn socket_address(&self) -> Result<SocketAddr, RestError> {
        let address = url_ip_address(&self.base_url).map_err(|_| RestError::InvalidEndpoint)?;
        let port = self
            .base_url
            .port_or_known_default()
            .ok_or(RestError::InvalidEndpoint)?;
        Ok(SocketAddr::new(address, port))
    }

    fn is_remote_https(&self) -> bool {
        self.base_url.scheme() == "https"
    }

    fn is_explicit_insecure_http(&self) -> bool {
        self.explicit_insecure_http_endpoint
            .is_some_and(|endpoint| {
                validate_explicit_insecure_http_endpoint(&self.base_url, endpoint).is_ok()
            })
    }
}

/// A bounded response plus independent request/decode timing.
#[derive(Debug)]
pub struct TimedResponse<T> {
    pub value: T,
    pub response_latency: Duration,
    pub decode_latency: Duration,
    pub body_bytes: usize,
    pub actor_count: usize,
    pub rest_completed_at: SystemTime,
    pub completed_monotonic: Instant,
}

pub struct PalRestClient {
    transport: ClientTransport,
    config: ClientConfig,
    auth: BasicAuthSecret,
}

enum ClientTransport {
    Reusable(reqwest::Client),
    Verified(Arc<dyn ConnectedStreamVerifier>),
}

impl PalRestClient {
    /// Creates the reusable general-purpose client used by probes and tests.
    /// Production callers with process-bound credentials must use
    /// [`Self::new_verified`].
    pub fn new_unverified_for_probe(
        config: ClientConfig,
        auth: BasicAuthSecret,
    ) -> Result<Self, RestError> {
        let client = reusable_client(&config)?;
        Ok(Self {
            transport: ClientTransport::Reusable(client),
            config,
            auth,
        })
    }

    /// Creates the production client for one explicitly allowed remote HTTPS
    /// base URL. TLS hostname validation remains enabled, redirects and system
    /// proxies remain disabled, and credentials cannot cross to another origin.
    pub fn new_explicit_remote_https(
        config: ClientConfig,
        auth: BasicAuthSecret,
    ) -> Result<Self, RestError> {
        if !config.is_remote_https() {
            return Err(RestError::EndpointNotAllowed);
        }
        let client = reusable_client(&config)?;
        Ok(Self {
            transport: ClientTransport::Reusable(client),
            config,
            auth,
        })
    }

    /// Creates a production client for one consented literal cleartext HTTP
    /// socket. Redirects and system proxies remain disabled. This does not
    /// provide confidentiality or server authentication.
    pub fn new_explicit_insecure_http(
        config: ClientConfig,
        auth: BasicAuthSecret,
    ) -> Result<Self, RestError> {
        if !config.is_explicit_insecure_http() {
            return Err(RestError::EndpointNotAllowed);
        }
        let client = reusable_client(&config)?;
        Ok(Self {
            transport: ClientTransport::Reusable(client),
            config,
            auth,
        })
    }

    /// Creates a client that opens and verifies a fresh loopback TCP connection
    /// before every authenticated HTTP/1 request.
    pub fn new_verified(
        config: ClientConfig,
        auth: BasicAuthSecret,
        verifier: Arc<dyn ConnectedStreamVerifier>,
    ) -> Result<Self, RestError> {
        if !config.socket_address()?.ip().is_loopback() {
            return Err(RestError::EndpointNotAllowed);
        }
        Ok(Self {
            transport: ClientTransport::Verified(verifier),
            config,
            auth,
        })
    }

    pub async fn game_data(
        &self,
        selector: &PlayerSelector,
        pseudonymizer: &Pseudonymizer,
        world_alias: &str,
    ) -> Result<TimedResponse<SafePlayerObservation>, RestError> {
        let measured = self.fetch(EndpointKind::GameData).await?;
        let decode_started = Instant::now();
        let selected = decode_selected_player(measured.body.as_ref(), selector)?;
        let safe = sanitize_player(selected, pseudonymizer, world_alias)?;
        let decode_latency = decode_started.elapsed();
        let actor_count = safe.actor_count;
        Ok(measured.finish(safe, decode_latency, actor_count))
    }

    pub async fn metrics(&self) -> Result<TimedResponse<ServerMetrics>, RestError> {
        let measured = self.fetch(EndpointKind::Metrics).await?;
        let decode_started = Instant::now();
        let metrics = decode_metrics(measured.body.as_ref())?;
        Ok(measured.finish(metrics, decode_started.elapsed(), 0))
    }

    pub async fn info(
        &self,
        pseudonymizer: &Pseudonymizer,
        world_alias: &str,
    ) -> Result<TimedResponse<SanitizedServerInfo>, RestError> {
        let measured = self.fetch(EndpointKind::Info).await?;
        let decode_started = Instant::now();
        let raw = decode_info(measured.body.as_ref())?;
        let info = sanitize_server_info(raw, pseudonymizer, world_alias)?;
        Ok(measured.finish(info, decode_started.elapsed(), 0))
    }

    pub async fn settings(&self) -> Result<TimedResponse<ServerSettings>, RestError> {
        let measured = self.fetch(EndpointKind::Settings).await?;
        let decode_started = Instant::now();
        let settings = decode_settings(measured.body.as_ref())?;
        Ok(measured.finish(settings, decode_started.elapsed(), 0))
    }

    pub async fn live_info(&self) -> Result<TimedResponse<ServerLiveInfoV1>, RestError> {
        let measured = self.fetch(EndpointKind::Info).await?;
        let decode_started = Instant::now();
        let info = decode_live_info(measured.body.as_ref())?;
        Ok(measured.finish(info, decode_started.elapsed(), 0))
    }

    pub async fn live_metrics(&self) -> Result<TimedResponse<ServerLiveMetricsV1>, RestError> {
        let measured = self.fetch(EndpointKind::Metrics).await?;
        let decode_started = Instant::now();
        let metrics = decode_live_metrics(measured.body.as_ref())?;
        Ok(measured.finish(metrics, decode_started.elapsed(), 0))
    }

    pub async fn live_players(&self) -> Result<TimedResponse<Vec<ServerLivePlayerV1>>, RestError> {
        let measured = self.fetch(EndpointKind::Players).await?;
        let decode_started = Instant::now();
        let players = decode_live_players(measured.body.as_ref())?;
        Ok(measured.finish(players, decode_started.elapsed(), 0))
    }

    pub async fn live_settings(&self) -> Result<TimedResponse<ServerLiveSettingsV1>, RestError> {
        let measured = self.fetch(EndpointKind::Settings).await?;
        let decode_started = Instant::now();
        let settings = decode_live_settings(measured.body.as_ref())?;
        Ok(measured.finish(settings, decode_started.elapsed(), 0))
    }

    /// Explicit heavy-read endpoint. Normal snapshot polling does not call it.
    pub async fn live_game_data(&self) -> Result<TimedResponse<ServerLiveGameDataV1>, RestError> {
        let measured = self.fetch(EndpointKind::GameData).await?;
        let decode_started = Instant::now();
        let game_data = decode_live_game_data(measured.body.as_ref())?;
        let actor_count =
            usize::try_from(game_data.total_actor_count).map_err(|_| RestError::Decode {
                endpoint: EndpointKind::GameData,
            })?;
        Ok(measured.finish(game_data, decode_started.elapsed(), actor_count))
    }

    /// Collects independent live sections without letting one endpoint failure
    /// discard the successful results from the others.
    pub async fn collect_server_live_snapshot(
        &self,
        profile_id: &str,
        request: LiveSnapshotRequest,
    ) -> ServerLiveSnapshotV1 {
        let info = async {
            if request.include_info {
                ServerLiveSectionV1::from_result(self.live_info().await)
            } else {
                ServerLiveSectionV1::not_requested()
            }
        };
        let metrics = async {
            if request.include_metrics {
                ServerLiveSectionV1::from_result(self.live_metrics().await)
            } else {
                ServerLiveSectionV1::not_requested()
            }
        };
        let players = async {
            if request.include_players {
                ServerLiveSectionV1::from_result(self.live_players().await)
            } else {
                ServerLiveSectionV1::not_requested()
            }
        };
        let settings = async {
            if request.include_settings {
                ServerLiveSectionV1::from_result(self.live_settings().await)
            } else {
                ServerLiveSectionV1::not_requested()
            }
        };
        let game_data = async {
            if !request.include_game_data {
                return ServerLiveSectionV1::not_requested();
            }
            match self.live_game_data().await {
                Err(RestError::UnexpectedStatus(404)) => ServerLiveSectionV1::unsupported(),
                result => ServerLiveSectionV1::from_result(result),
            }
        };

        let (info, metrics, players, settings, game_data) =
            tokio::join!(info, metrics, players, settings, game_data);
        ServerLiveSnapshotV1::new(
            profile_id.to_owned(),
            info,
            metrics,
            players,
            settings,
            game_data,
        )
    }

    async fn fetch(&self, endpoint: EndpointKind) -> Result<MeasuredBody, RestError> {
        match &self.transport {
            ClientTransport::Reusable(client) => self.fetch_reusable(client, endpoint).await,
            ClientTransport::Verified(verifier) => self.fetch_verified(verifier, endpoint).await,
        }
    }

    async fn fetch_reusable(
        &self,
        client: &reqwest::Client,
        endpoint: EndpointKind,
    ) -> Result<MeasuredBody, RestError> {
        let url = self
            .config
            .base_url
            .join(endpoint.path())
            .map_err(|_| RestError::InvalidEndpoint)?;
        let started = Instant::now();
        let mut response = self.request_reusable(client, url).await?;

        validate_response(response.status(), response.headers(), &self.config)?;
        let mut body = SensitiveBody::default();
        while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
            append_bounded(&mut body, &chunk, self.config.max_decoded_body_bytes)?;
        }
        Ok(MeasuredBody::completed(body, started))
    }

    async fn request_reusable(
        &self,
        client: &reqwest::Client,
        url: reqwest::Url,
    ) -> Result<reqwest::Response, RestError> {
        client
            .get(url)
            .header(ACCEPT, "application/json")
            .header(ACCEPT_ENCODING, "identity")
            .basic_auth(self.auth.username(), Some(self.auth.password()))
            .send()
            .await
            .map_err(map_reqwest_error)
    }

    async fn fetch_verified(
        &self,
        verifier: &Arc<dyn ConnectedStreamVerifier>,
        endpoint: EndpointKind,
    ) -> Result<MeasuredBody, RestError> {
        let url = self
            .config
            .base_url
            .join(endpoint.path())
            .map_err(|_| RestError::InvalidEndpoint)?;
        let server_address = self.config.socket_address()?;
        let started = Instant::now();
        let stream = timeout(
            self.config.connect_timeout,
            TcpStream::connect(server_address),
        )
        .await
        .map_err(|_| RestError::Timeout)?
        .map_err(|_| RestError::Transport)?;
        let local_address = stream.local_addr().map_err(|_| RestError::Transport)?;
        let peer_address = stream.peer_addr().map_err(|_| RestError::Transport)?;
        if peer_address != server_address || !verifier.verify(local_address, peer_address) {
            return Err(RestError::ConnectionUntrusted);
        }

        timeout(
            self.config.request_timeout,
            self.fetch_verified_stream(stream, url, started),
        )
        .await
        .map_err(|_| RestError::Timeout)?
    }

    async fn fetch_verified_stream(
        &self,
        stream: TcpStream,
        url: reqwest::Url,
        started: Instant,
    ) -> Result<MeasuredBody, RestError> {
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake::<_, Empty<Bytes>>(TokioIo::new(stream))
                .await
                .map_err(|_| RestError::Transport)?;
        let _connection = ConnectionTask(tokio::spawn(async move {
            let _ = connection.await;
        }));
        let request = self.verified_request(&url)?;
        let mut response = sender
            .send_request(request)
            .await
            .map_err(|_| RestError::Transport)?;
        validate_response(response.status(), response.headers(), &self.config)?;

        let mut body = SensitiveBody::default();
        while let Some(frame) = response.body_mut().frame().await {
            let frame = frame.map_err(|_| RestError::Transport)?;
            if let Ok(data) = frame.into_data() {
                append_bounded(&mut body, &data, self.config.max_decoded_body_bytes)?;
            }
        }
        Ok(MeasuredBody::completed(body, started))
    }

    fn verified_request(&self, url: &reqwest::Url) -> Result<Request<Empty<Bytes>>, RestError> {
        let server_address = self.config.socket_address()?;
        let mut cleartext = SensitiveBody(Vec::with_capacity(
            self.auth
                .username
                .len()
                .saturating_add(self.auth.password.len())
                .saturating_add(1),
        ));
        cleartext.0.extend_from_slice(&self.auth.username);
        cleartext.0.push(b':');
        cleartext.0.extend_from_slice(&self.auth.password);
        let encoded_capacity = cleartext.0.len().div_ceil(3).saturating_mul(4);
        let mut authorization = SensitiveBody(vec![0_u8; 6_usize.saturating_add(encoded_capacity)]);
        authorization.0[..6].copy_from_slice(b"Basic ");
        let encoded_len = BASE64_STANDARD
            .encode_slice(&cleartext.0, &mut authorization.0[6..])
            .map_err(|_| RestError::InvalidClientConfig)?;
        authorization.0.truncate(6 + encoded_len);
        let mut authorization_header = HeaderValue::from_bytes(&authorization.0)
            .map_err(|_| RestError::InvalidClientConfig)?;
        authorization_header.set_sensitive(true);

        Request::builder()
            .method(Method::GET)
            .uri(url.path())
            .header(HOST, server_address.to_string())
            .header(ACCEPT, "application/json")
            .header(ACCEPT_ENCODING, "identity")
            .header(CONNECTION, "close")
            .header(AUTHORIZATION, authorization_header)
            .body(Empty::new())
            .map_err(|_| RestError::InvalidClientConfig)
    }
}

struct ConnectionTask(tokio::task::JoinHandle<()>);

impl Drop for ConnectionTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct MeasuredBody {
    body: SensitiveBody,
    response_latency: Duration,
    rest_completed_at: SystemTime,
    completed_monotonic: Instant,
}

impl MeasuredBody {
    fn completed(body: SensitiveBody, started: Instant) -> Self {
        let completed_monotonic = Instant::now();
        Self {
            body,
            response_latency: completed_monotonic.duration_since(started),
            rest_completed_at: SystemTime::now(),
            completed_monotonic,
        }
    }

    fn finish<T>(
        mut self,
        value: T,
        decode_latency: Duration,
        actor_count: usize,
    ) -> TimedResponse<T> {
        let body_bytes = self.body.0.len();
        self.body.0.fill(0);
        TimedResponse {
            value,
            response_latency: self.response_latency,
            decode_latency,
            body_bytes,
            actor_count,
            rest_completed_at: self.rest_completed_at,
            completed_monotonic: self.completed_monotonic,
        }
    }
}

fn validate_response(
    status: StatusCode,
    headers: &HeaderMap,
    config: &ClientConfig,
) -> Result<(), RestError> {
    if status.as_u16() == 401 {
        return Err(RestError::Unauthorized);
    }
    if status.is_redirection() {
        return Err(RestError::RedirectRejected);
    }
    if status.as_u16() != 200 {
        return Err(RestError::UnexpectedStatus(status.as_u16()));
    }
    if headers.get(CONTENT_ENCODING).is_some_and(|value| {
        value
            .to_str()
            .map_or(true, |value| !value.eq_ignore_ascii_case("identity"))
    }) {
        return Err(RestError::CompressionNotAllowed);
    }
    if !headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(is_json_content_type)
    {
        return Err(RestError::ContentTypeRejected);
    }
    if headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|length| length > config.max_decoded_body_bytes as u64)
    {
        return Err(RestError::ResponseTooLarge);
    }
    Ok(())
}

fn append_bounded(body: &mut SensitiveBody, chunk: &[u8], limit: usize) -> Result<(), RestError> {
    let next_len = body
        .0
        .len()
        .checked_add(chunk.len())
        .ok_or(RestError::ResponseTooLarge)?;
    if next_len > limit {
        return Err(RestError::ResponseTooLarge);
    }
    body.0.extend_from_slice(chunk);
    Ok(())
}

impl Drop for MeasuredBody {
    fn drop(&mut self) {
        self.body.0.fill(0);
    }
}

#[derive(Default)]
struct SensitiveBody(Vec<u8>);

impl AsRef<[u8]> for SensitiveBody {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for SensitiveBody {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

fn map_reqwest_error(error: reqwest::Error) -> RestError {
    if error.is_timeout() {
        RestError::Timeout
    } else {
        RestError::Transport
    }
}

fn is_json_content_type(value: &str) -> bool {
    value
        .split(';')
        .next()
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("application/json"))
}

fn validate_endpoint(url: &reqwest::Url, policy: &EndpointPolicy) -> Result<(), RestError> {
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "/v1/api" | "/v1/api/")
    {
        return Err(RestError::InvalidEndpoint);
    }
    match url.scheme() {
        "http" => {
            let address = url_ip_address(url).map_err(|_| RestError::EndpointNotAllowed)?;
            if !policy.permits_local_address(address) {
                return Err(RestError::EndpointNotAllowed);
            }
        }
        "https" => {
            let normalized = normalized_base_url(url)?;
            if !policy.permits_remote_https(&normalized) {
                return Err(RestError::EndpointNotAllowed);
            }
        }
        _ => return Err(RestError::InvalidEndpoint),
    }
    Ok(())
}

fn validate_explicit_insecure_http_endpoint(
    url: &reqwest::Url,
    endpoint: SocketAddr,
) -> Result<(), RestError> {
    if endpoint.port() == 0
        || !is_explicit_http_unicast(endpoint.ip())
        || url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/v1/api/"
        || url_ip_address(url).ok() != Some(endpoint.ip())
        || url.port_or_known_default() != Some(endpoint.port())
    {
        return Err(RestError::EndpointNotAllowed);
    }
    Ok(())
}

fn is_explicit_http_unicast(address: IpAddr) -> bool {
    let address = match address {
        IpAddr::V6(address) => address
            .to_ipv4_mapped()
            .map_or(IpAddr::V6(address), IpAddr::V4),
        address => address,
    };
    !address.is_unspecified()
        && !address.is_multicast()
        && !matches!(address, IpAddr::V4(address) if address == std::net::Ipv4Addr::BROADCAST)
}

fn reusable_client(config: &ClientConfig) -> Result<reqwest::Client, RestError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(config.connect_timeout)
        .timeout(config.request_timeout)
        .build()
        .map_err(|_| RestError::InvalidClientConfig)
}

fn normalize_remote_https_base_url(base_url: &str) -> Result<String, RestError> {
    let url = reqwest::Url::parse(base_url).map_err(|_| RestError::InvalidEndpoint)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "/v1/api" | "/v1/api/")
        || url.host_str().is_none()
    {
        return Err(RestError::InvalidEndpoint);
    }
    normalized_base_url(&url)
}

fn normalized_base_url(url: &reqwest::Url) -> Result<String, RestError> {
    let host = url.host().ok_or(RestError::InvalidEndpoint)?;
    let port = url
        .port_or_known_default()
        .ok_or(RestError::InvalidEndpoint)?;
    // `Host` formats IPv6 with exactly one bracket pair, while domains and
    // IPv4 remain unbracketed. Avoid `host_str`, whose IPv6 form is already
    // bracketed and is easy to accidentally bracket a second time.
    let authority = format!("{host}:{port}");
    Ok(format!("https://{authority}/v1/api/"))
}

fn url_ip_address(url: &reqwest::Url) -> Result<IpAddr, RestError> {
    let host = url.host().ok_or(RestError::InvalidEndpoint)?;
    let serialized = host.to_string();
    let address = serialized
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(&serialized);
    address
        .parse::<IpAddr>()
        .map_err(|_| RestError::InvalidEndpoint)
}

fn is_rfc1918(address: IpAddr) -> bool {
    let IpAddr::V4(address) = address else {
        return false;
    };
    let octets = address.octets();
    octets[0] == 10
        || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 168)
}
