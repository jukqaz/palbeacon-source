use std::{
    error::Error as StdError,
    fmt,
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use getrandom::fill as fill_random;
use pal_protocol::v2::{
    ClockProbeRequest, ClockProbeResponse, GetDescriptorRequest, ResumeCursor, SubscribeRequest,
    SubscribeResponse, position_telemetry_service_client::PositionTelemetryServiceClient,
};
use pal_state::{
    ClockInvalidReason, ServerAgentControlOutcome, ServerAgentFrame, ServerAgentIngestOutcome,
    ServerAgentSender, ServerAgentSourceError,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use thiserror::Error;
use tokio::sync::watch;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};
use zeroize::Zeroizing;

use crate::{
    ClockError, ClockEstimator, ClockProbeObservation, MAX_DECODED_MESSAGE_SIZE, PROBE_CADENCE,
    ValidationError, reconnect_backoff, validate_clock_probe_request,
    validate_clock_probe_response, validate_descriptor, validate_envelope,
    wire::validate_canonical_sha256,
};

pub struct TelemetryIngest {
    sender: ServerAgentSender,
    generation: u64,
    coordinate_profile_sha256: String,
    clock: ClockEstimator,
    clock_invalid_emitted: bool,
    clock_was_established: bool,
    connected: bool,
}

impl fmt::Debug for TelemetryIngest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED TELEMETRY INGEST]")
    }
}

impl TelemetryIngest {
    pub fn new(
        sender: ServerAgentSender,
        generation: u64,
        coordinate_profile_sha256: impl Into<String>,
        maximum_rtt_ms: u64,
    ) -> Result<Self, IngestError> {
        let coordinate_profile_sha256 = coordinate_profile_sha256.into();
        validate_canonical_sha256(&coordinate_profile_sha256)?;
        if generation == 0
            || maximum_rtt_ms == 0
            || sender.connect(generation) != ServerAgentControlOutcome::Accepted
        {
            return Err(IngestError::Connection);
        }
        Ok(Self {
            sender,
            generation,
            coordinate_profile_sha256,
            clock: ClockEstimator::new(maximum_rtt_ms),
            clock_invalid_emitted: false,
            clock_was_established: false,
            connected: true,
        })
    }

    pub fn accept_clock_probe(
        &mut self,
        request: &ClockProbeRequest,
        response: &ClockProbeResponse,
        client_receive_unix_ms: i64,
        observed_at_monotonic_ms: u64,
    ) -> Result<(), IngestError> {
        let result = self.accept_clock_probe_inner(
            request,
            response,
            client_receive_unix_ms,
            observed_at_monotonic_ms,
        );
        if result.is_err() {
            self.clock.invalidate();
            self.clock_invalid_emitted = true;
            let _ = self
                .sender
                .clock_invalid(self.generation, ClockInvalidReason::ProbeInvalid);
        } else {
            self.clock_invalid_emitted = false;
            self.clock_was_established = true;
        }
        result
    }

    fn accept_clock_probe_inner(
        &mut self,
        request: &ClockProbeRequest,
        response: &ClockProbeResponse,
        client_receive_unix_ms: i64,
        observed_at_monotonic_ms: u64,
    ) -> Result<(), IngestError> {
        validate_clock_probe_request(request)?;
        validate_clock_probe_response(response)?;
        if response.client_send_unix_ms != request.client_send_unix_ms {
            return Err(IngestError::Clock);
        }
        self.clock.observe(
            ClockProbeObservation {
                expected_nonce: request.nonce.clone(),
                echoed_nonce: response.nonce.clone(),
                client_send_unix_ms: request.client_send_unix_ms,
                agent_receive_unix_ms: response.agent_receive_unix_ms,
                agent_send_unix_ms: response.agent_send_unix_ms,
                client_receive_unix_ms,
            },
            observed_at_monotonic_ms,
        )?;
        Ok(())
    }

    pub fn accept(
        &mut self,
        value: SubscribeResponse,
        received_at_monotonic_ms: u64,
    ) -> Result<ServerAgentIngestOutcome, IngestError> {
        validate_envelope(&value, &self.coordinate_profile_sha256)?;
        let Some(estimate) = self.clock.current(received_at_monotonic_ms) else {
            if !self.clock_invalid_emitted {
                let reason = if self.clock_was_established {
                    ClockInvalidReason::EvidenceExpired
                } else {
                    ClockInvalidReason::ProbeInvalid
                };
                let _ = self.sender.clock_invalid(self.generation, reason);
                self.clock_invalid_emitted = true;
            }
            return Err(IngestError::ClockUnavailable);
        };
        let frame = ServerAgentFrame::new(
            value.world_alias,
            &value.subject_id,
            &value.boot_id,
            self.generation,
            value.sequence,
            value.position_x,
            value.position_y,
            value.position_z,
            value.heading_degrees,
            estimate.age_upper_bound_ms(value.age_at_emit_ms),
        );
        self.sender
            .publish_position(frame, received_at_monotonic_ms)
            .map_err(IngestError::State)
    }

    pub fn disconnect(&mut self) {
        if self.connected {
            let _ = self.sender.disconnect(self.generation);
            self.connected = false;
        }
    }
}

impl Drop for TelemetryIngest {
    fn drop(&mut self) {
        self.disconnect();
    }
}

#[derive(Debug, Error)]
pub enum IngestError {
    #[error("telemetry wire validation failed")]
    Validation,
    #[error("telemetry connection transition rejected")]
    Connection,
    #[error("telemetry clock evidence failed")]
    Clock,
    #[error("telemetry clock evidence unavailable")]
    ClockUnavailable,
    #[error("server-agent state rejected telemetry")]
    State(#[source] ServerAgentSourceError),
}

impl From<ValidationError> for IngestError {
    fn from(_: ValidationError) -> Self {
        Self::Validation
    }
}

impl From<ClockError> for IngestError {
    fn from(_: ClockError) -> Self {
        Self::Clock
    }
}

#[derive(Clone)]
pub struct NetworkConsumerConfig {
    endpoint_uri: String,
    domain_name: String,
    ca_certificate_pem: Vec<u8>,
    client_certificate_pem: Vec<u8>,
    client_private_key_pem: Zeroizing<Vec<u8>>,
    world_alias: String,
    subject_id: [u8; 32],
    coordinate_profile_sha256: String,
    maximum_rtt_ms: u64,
    descriptor_pins: Option<DescriptorPins>,
}

#[derive(Clone)]
struct DescriptorPins {
    gate_profile_sha256: String,
    server_fingerprint: String,
}

impl fmt::Debug for NetworkConsumerConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED NETWORK CONSUMER CONFIG]")
    }
}

impl NetworkConsumerConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        endpoint_uri: impl Into<String>,
        domain_name: impl Into<String>,
        ca_certificate_pem: impl AsRef<[u8]>,
        client_certificate_pem: impl AsRef<[u8]>,
        client_private_key_pem: impl AsRef<[u8]>,
        world_alias: impl Into<String>,
        subject_id: impl AsRef<[u8]>,
        coordinate_profile_sha256: impl Into<String>,
        maximum_rtt_ms: u64,
    ) -> Result<Self, NetworkConsumerError> {
        let endpoint_uri = endpoint_uri.into();
        let domain_name = domain_name.into();
        let world_alias = world_alias.into();
        let subject_id: [u8; 32] = subject_id
            .as_ref()
            .try_into()
            .map_err(|_| NetworkConsumerError::Configuration)?;
        let coordinate_profile_sha256 = coordinate_profile_sha256.into();
        validate_canonical_sha256(&coordinate_profile_sha256)
            .map_err(|_| NetworkConsumerError::Configuration)?;
        if !endpoint_uri.starts_with("https://")
            || domain_name.is_empty()
            || !domain_name.is_ascii()
            || world_alias.is_empty()
            || world_alias.len() > 64
            || !world_alias.is_ascii()
            || maximum_rtt_ms == 0
            || ca_certificate_pem.as_ref().is_empty()
            || client_certificate_pem.as_ref().is_empty()
            || client_private_key_pem.as_ref().is_empty()
        {
            return Err(NetworkConsumerError::Configuration);
        }
        Endpoint::from_shared(endpoint_uri.clone())
            .map_err(|_| NetworkConsumerError::Configuration)?;
        validate_tls_material(
            ca_certificate_pem.as_ref(),
            client_certificate_pem.as_ref(),
            client_private_key_pem.as_ref(),
        )?;
        Ok(Self {
            endpoint_uri,
            domain_name,
            ca_certificate_pem: ca_certificate_pem.as_ref().to_vec(),
            client_certificate_pem: client_certificate_pem.as_ref().to_vec(),
            client_private_key_pem: Zeroizing::new(client_private_key_pem.as_ref().to_vec()),
            world_alias,
            subject_id,
            coordinate_profile_sha256,
            maximum_rtt_ms,
            descriptor_pins: None,
        })
    }

    pub fn with_descriptor_pins(
        mut self,
        gate_profile_sha256: impl Into<String>,
        server_fingerprint: impl Into<String>,
    ) -> Result<Self, NetworkConsumerError> {
        let gate_profile_sha256 = gate_profile_sha256.into();
        let server_fingerprint = server_fingerprint.into();
        validate_canonical_sha256(&gate_profile_sha256)
            .map_err(|_| NetworkConsumerError::Configuration)?;
        validate_canonical_sha256(&server_fingerprint)
            .map_err(|_| NetworkConsumerError::Configuration)?;
        if gate_profile_sha256.bytes().all(|byte| byte == b'0')
            || server_fingerprint.bytes().all(|byte| byte == b'0')
        {
            return Err(NetworkConsumerError::Configuration);
        }
        self.descriptor_pins = Some(DescriptorPins {
            gate_profile_sha256,
            server_fingerprint,
        });
        Ok(self)
    }
}

fn validate_tls_material(
    ca_certificate_pem: &[u8],
    client_certificate_pem: &[u8],
    client_private_key_pem: &[u8],
) -> Result<(), NetworkConsumerError> {
    let ca_certificates = CertificateDer::pem_slice_iter(ca_certificate_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| NetworkConsumerError::Configuration)?;
    if ca_certificates.is_empty() {
        return Err(NetworkConsumerError::Configuration);
    }
    let mut roots = rustls::RootCertStore::empty();
    let (accepted_roots, rejected_roots) = roots.add_parsable_certificates(ca_certificates);
    if accepted_roots == 0 || rejected_roots != 0 {
        return Err(NetworkConsumerError::Configuration);
    }

    let client_certificates = CertificateDer::pem_slice_iter(client_certificate_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| NetworkConsumerError::Configuration)?;
    if client_certificates.is_empty() {
        return Err(NetworkConsumerError::Configuration);
    }
    let private_key = PrivateKeyDer::from_pem_slice(client_private_key_pem)
        .map_err(|_| NetworkConsumerError::Configuration)?;

    rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_client_auth_cert(client_certificates, private_key)
        .map_err(|_| NetworkConsumerError::Configuration)?;
    Ok(())
}

pub struct NetworkConsumer {
    config: NetworkConsumerConfig,
    sender: ServerAgentSender,
    initial_generation: u64,
    monotonic_epoch: MonotonicEpoch,
    resume: Arc<Mutex<Option<ResumeCursor>>>,
}

/// Shared monotonic clock epoch for transport receive timestamps and overlay freshness.
///
/// A consumer and its state reducer must use the same epoch. Otherwise a later-created
/// overlay clock can temporarily understate sample age after startup.
#[derive(Clone, Copy, Debug)]
pub struct MonotonicEpoch {
    origin: Instant,
}

impl MonotonicEpoch {
    pub fn now() -> Self {
        Self {
            origin: Instant::now(),
        }
    }

    pub const fn new_at(origin: Instant) -> Self {
        Self { origin }
    }

    pub fn elapsed_ms(self) -> u64 {
        monotonic_millis(self.origin)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkFailureClass {
    Transient,
    Terminal,
}

impl fmt::Debug for NetworkConsumer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED NETWORK CONSUMER]")
    }
}

impl NetworkConsumer {
    pub fn new(
        config: NetworkConsumerConfig,
        sender: ServerAgentSender,
        initial_generation: u64,
    ) -> Result<Self, NetworkConsumerError> {
        Self::new_at(config, sender, initial_generation, MonotonicEpoch::now())
    }

    pub fn new_at(
        config: NetworkConsumerConfig,
        sender: ServerAgentSender,
        initial_generation: u64,
        monotonic_epoch: MonotonicEpoch,
    ) -> Result<Self, NetworkConsumerError> {
        if initial_generation == 0 {
            return Err(NetworkConsumerError::Configuration);
        }
        Ok(Self {
            config,
            sender,
            initial_generation,
            monotonic_epoch,
            resume: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn run_once(&self) -> Result<(), NetworkConsumerError> {
        self.run_session(self.initial_generation).await
    }

    pub async fn run_with_reconnect(
        &self,
        maximum_sessions: usize,
    ) -> Result<(), NetworkConsumerError> {
        if maximum_sessions == 0 {
            return Err(NetworkConsumerError::Configuration);
        }
        let mut last = NetworkConsumerError::Disconnected;
        for attempt in 0..maximum_sessions {
            let generation = self
                .initial_generation
                .checked_add(attempt as u64)
                .ok_or(NetworkConsumerError::Configuration)?;
            match self.run_session(generation).await {
                Ok(()) => return Ok(()),
                Err(error) if error.class() == NetworkFailureClass::Terminal => {
                    return Err(error);
                }
                Err(error) => last = error,
            }
            if attempt + 1 < maximum_sessions {
                tokio::time::sleep(reconnect_backoff(attempt)).await;
            }
        }
        Err(last)
    }

    /// Supervises transient transport failures until cancellation while monotonically
    /// increasing the server-agent connection generation. Terminal identity, validation,
    /// codec, configuration, and state failures fail closed instead of reconnecting.
    pub async fn run_until_cancelled(
        &self,
        mut cancellation: watch::Receiver<bool>,
    ) -> Result<(), NetworkConsumerError> {
        let mut attempt = 0_u64;
        loop {
            if *cancellation.borrow() {
                return Ok(());
            }
            let generation = self
                .initial_generation
                .checked_add(attempt)
                .ok_or(NetworkConsumerError::Configuration)?;
            let result = tokio::select! {
                biased;
                changed = cancellation.changed() => {
                    match changed {
                        Ok(()) if *cancellation.borrow() => return Ok(()),
                        Ok(()) => continue,
                        Err(_) => return Ok(()),
                    }
                }
                result = self.run_session(generation) => result,
            };
            match result {
                Ok(()) => return Ok(()),
                Err(error) if error.class() == NetworkFailureClass::Terminal => {
                    return Err(error);
                }
                Err(_) => {}
            }
            let backoff_index = usize::try_from(attempt).unwrap_or(usize::MAX);
            attempt = attempt
                .checked_add(1)
                .ok_or(NetworkConsumerError::Configuration)?;
            tokio::select! {
                biased;
                changed = cancellation.changed() => {
                    match changed {
                        Ok(()) if *cancellation.borrow() => return Ok(()),
                        Ok(()) => {}
                        Err(_) => return Ok(()),
                    }
                }
                _ = tokio::time::sleep(reconnect_backoff(backoff_index)) => {}
            }
        }
    }

    async fn run_session(&self, generation: u64) -> Result<(), NetworkConsumerError> {
        let channel = self.connect_channel().await?;
        let mut client = PositionTelemetryServiceClient::new(channel)
            .max_decoding_message_size(MAX_DECODED_MESSAGE_SIZE)
            .max_encoding_message_size(MAX_DECODED_MESSAGE_SIZE);
        let descriptor = client
            .get_descriptor(GetDescriptorRequest {
                protocol_version: pal_domain::PROTOCOL_VERSION,
            })
            .await
            .map_err(network_rpc_error)?
            .into_inner();
        validate_descriptor(&descriptor, &self.config.coordinate_profile_sha256)
            .map_err(|_| NetworkConsumerError::Validation)?;
        if descriptor.world_alias != self.config.world_alias {
            return Err(NetworkConsumerError::Identity);
        }
        if let Some(pins) = self.config.descriptor_pins.as_ref()
            && (descriptor.gate_profile_sha256 != pins.gate_profile_sha256
                || descriptor.server_fingerprint != pins.server_fingerprint)
        {
            return Err(NetworkConsumerError::Identity);
        }
        let mut ingest = TelemetryIngest::new(
            self.sender.clone(),
            generation,
            &self.config.coordinate_profile_sha256,
            self.config.maximum_rtt_ms,
        )
        .map_err(|_| NetworkConsumerError::Ingest)?;
        probe_clock(&mut client, &mut ingest, self.monotonic_epoch).await?;

        let resume = self.resume.lock().expect("resume cursor poisoned").clone();
        let mut stream = client
            .subscribe(SubscribeRequest {
                protocol_version: pal_domain::PROTOCOL_VERSION,
                world_alias: self.config.world_alias.clone(),
                subject_id: self.config.subject_id.to_vec(),
                resume,
            })
            .await
            .map_err(network_rpc_error)?
            .into_inner();
        let mut probes = tokio::time::interval(PROBE_CADENCE);
        probes.tick().await;
        loop {
            tokio::select! {
                value = stream.message() => {
                    let value = value
                        .map_err(network_rpc_error)?
                        .ok_or(NetworkConsumerError::Disconnected)?;
                    if value.world_alias != self.config.world_alias
                        || value.subject_id != self.config.subject_id
                    {
                        return Err(NetworkConsumerError::Identity);
                    }
                    let now = self.monotonic_epoch.elapsed_ms();
                    let outcome = ingest
                        .accept(value.clone(), now)
                        .map_err(|_| NetworkConsumerError::Ingest)?;
                    if outcome == ServerAgentIngestOutcome::Accepted {
                        *self.resume.lock().expect("resume cursor poisoned") = Some(ResumeCursor {
                            boot_id: value.boot_id,
                            sequence: value.sequence,
                        });
                    }
                }
                _ = probes.tick() => {
                    probe_clock(&mut client, &mut ingest, self.monotonic_epoch).await?;
                }
            }
        }
    }

    async fn connect_channel(&self) -> Result<Channel, NetworkConsumerError> {
        let tls = ClientTlsConfig::new()
            .domain_name(self.config.domain_name.clone())
            .ca_certificate(Certificate::from_pem(
                self.config.ca_certificate_pem.clone(),
            ))
            .identity(Identity::from_pem(
                self.config.client_certificate_pem.clone(),
                self.config.client_private_key_pem.as_slice(),
            ));
        Endpoint::from_shared(self.config.endpoint_uri.clone())
            .map_err(|_| NetworkConsumerError::Configuration)?
            .tls_config(tls)
            .map_err(|_| NetworkConsumerError::Configuration)?
            .connect()
            .await
            .map_err(|error| classify_transport_error(&error))
    }
}

fn classify_transport_error(error: &(dyn StdError + 'static)) -> NetworkConsumerError {
    let mut current = Some(error);
    while let Some(source) = current {
        let nested_rustls_error = source
            .downcast_ref::<std::io::Error>()
            .and_then(std::io::Error::get_ref)
            .and_then(|inner| inner.downcast_ref::<rustls::Error>())
            .is_some();
        if source.downcast_ref::<rustls::Error>().is_some() || nested_rustls_error {
            return NetworkConsumerError::TlsAuthentication;
        }
        current = source.source();
    }
    NetworkConsumerError::Transport
}

async fn probe_clock(
    client: &mut PositionTelemetryServiceClient<Channel>,
    ingest: &mut TelemetryIngest,
    monotonic_epoch: MonotonicEpoch,
) -> Result<(), NetworkConsumerError> {
    let mut nonce = [0_u8; 16];
    fill_random(&mut nonce).map_err(|_| NetworkConsumerError::Clock)?;
    let request = ClockProbeRequest {
        protocol_version: pal_domain::PROTOCOL_VERSION,
        nonce: nonce.to_vec(),
        client_send_unix_ms: unix_millis()?,
    };
    let response = client
        .clock_probe(request.clone())
        .await
        .map_err(network_rpc_error)?
        .into_inner();
    // The wire clock is millisecond-granular. Include one local millisecond in
    // the conservative RTT bound rather than accepting zero-latency evidence.
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    ingest
        .accept_clock_probe(
            &request,
            &response,
            unix_millis()?,
            monotonic_epoch.elapsed_ms(),
        )
        .map_err(|_| NetworkConsumerError::Clock)
}

fn unix_millis() -> Result<i64, NetworkConsumerError> {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| NetworkConsumerError::Clock)?
        .as_millis();
    i64::try_from(value).map_err(|_| NetworkConsumerError::Clock)
}

fn monotonic_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn network_rpc_error(status: tonic::Status) -> NetworkConsumerError {
    match status.code() {
        tonic::Code::OutOfRange | tonic::Code::ResourceExhausted => {
            NetworkConsumerError::MessageTooLarge
        }
        tonic::Code::InvalidArgument
        | tonic::Code::NotFound
        | tonic::Code::AlreadyExists
        | tonic::Code::PermissionDenied
        | tonic::Code::FailedPrecondition
        | tonic::Code::Unimplemented
        | tonic::Code::DataLoss
        | tonic::Code::Unauthenticated => NetworkConsumerError::RpcTerminal,
        _ => NetworkConsumerError::Rpc,
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum NetworkConsumerError {
    #[error("telemetry client configuration is invalid")]
    Configuration,
    #[error("telemetry transport connection failed")]
    Transport,
    #[error("telemetry TLS peer authentication failed")]
    TlsAuthentication,
    #[error("telemetry RPC failed")]
    Rpc,
    #[error("telemetry RPC was permanently refused")]
    RpcTerminal,
    #[error("telemetry message exceeded the codec limit")]
    MessageTooLarge,
    #[error("telemetry identity binding failed")]
    Identity,
    #[error("telemetry validation failed")]
    Validation,
    #[error("telemetry clock evidence failed")]
    Clock,
    #[error("telemetry ingest failed")]
    Ingest,
    #[error("telemetry stream disconnected")]
    Disconnected,
}

impl NetworkConsumerError {
    pub const fn class(self) -> NetworkFailureClass {
        match self {
            Self::Transport | Self::Rpc | Self::Disconnected => NetworkFailureClass::Transient,
            Self::Configuration
            | Self::TlsAuthentication
            | Self::RpcTerminal
            | Self::MessageTooLarge
            | Self::Identity
            | Self::Validation
            | Self::Clock
            | Self::Ingest => NetworkFailureClass::Terminal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authentication_and_protocol_rpc_failures_are_terminal() {
        for code in [
            tonic::Code::InvalidArgument,
            tonic::Code::PermissionDenied,
            tonic::Code::FailedPrecondition,
            tonic::Code::Unimplemented,
            tonic::Code::DataLoss,
            tonic::Code::Unauthenticated,
        ] {
            assert_eq!(
                network_rpc_error(tonic::Status::new(code, "sensitive detail")),
                NetworkConsumerError::RpcTerminal
            );
        }
        for code in [
            tonic::Code::Cancelled,
            tonic::Code::Unknown,
            tonic::Code::DeadlineExceeded,
            tonic::Code::Aborted,
            tonic::Code::Internal,
            tonic::Code::Unavailable,
        ] {
            assert_eq!(
                network_rpc_error(tonic::Status::new(code, "sensitive detail")),
                NetworkConsumerError::Rpc
            );
        }
    }
}
