use std::{
    net::{Ipv4Addr, SocketAddr},
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use pal_domain::Freshness;
use pal_protocol::v2::{
    ClockProbeRequest, ClockProbeResponse, CoordinateSpace, GetDescriptorRequest,
    GetDescriptorResponse, HeadingSource, ResumeCursor, SubscribeRequest, SubscribeResponse,
    position_telemetry_service_client::PositionTelemetryServiceClient,
    position_telemetry_service_server::{PositionTelemetryService, PositionTelemetryServiceServer},
};
use pal_state::{Event, PositionSource, PositionSourceEvent, StateReducer, server_agent_channel};
use pal_telemetry::{
    AllowlistEntry, ClientAllowlist, LatestConnectOutcome, LatestPublishOutcome, LatestTelemetry,
    NetworkConsumer, NetworkConsumerConfig, NetworkConsumerError, PositionTelemetryEndpoint,
    certificate_sha256,
};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, SanType, date_time_ymd, string::Ia5String,
};
use tokio::{net::TcpListener, sync::oneshot};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{
    Code, Request, Response, Status,
    transport::{
        Certificate, Channel, ClientTlsConfig, Endpoint, Identity, Server, ServerTlsConfig,
    },
};

type TestStream =
    Pin<Box<dyn futures_core::Stream<Item = Result<SubscribeResponse, Status>> + Send + 'static>>;

#[derive(Clone)]
struct OversizedDescriptorService;

#[tonic::async_trait]
impl PositionTelemetryService for OversizedDescriptorService {
    type SubscribeStream = TestStream;

    async fn get_descriptor(
        &self,
        _request: Request<GetDescriptorRequest>,
    ) -> Result<Response<GetDescriptorResponse>, Status> {
        let mut value = descriptor();
        value.agent_version = "x".repeat(70 * 1_024);
        Ok(Response::new(value))
    }

    async fn subscribe(
        &self,
        _request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        Ok(Response::new(Box::pin(tokio_stream::empty())))
    }

    async fn clock_probe(
        &self,
        request: Request<ClockProbeRequest>,
    ) -> Result<Response<ClockProbeResponse>, Status> {
        let request = request.into_inner();
        Ok(Response::new(ClockProbeResponse {
            nonce: request.nonce,
            client_send_unix_ms: request.client_send_unix_ms,
            agent_receive_unix_ms: request.client_send_unix_ms.saturating_add(1),
            agent_send_unix_ms: request.client_send_unix_ms.saturating_add(2),
        }))
    }
}

#[derive(Clone)]
struct ReconnectScriptService {
    subscription_count: Arc<AtomicUsize>,
    resumes: Arc<Mutex<Vec<Option<ResumeCursor>>>>,
}

#[derive(Clone)]
struct InvalidClockService {
    probe_count: Arc<AtomicUsize>,
}

#[tonic::async_trait]
impl PositionTelemetryService for ReconnectScriptService {
    type SubscribeStream = TestStream;

    async fn get_descriptor(
        &self,
        _request: Request<GetDescriptorRequest>,
    ) -> Result<Response<GetDescriptorResponse>, Status> {
        Ok(Response::new(descriptor()))
    }

    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let request = request.into_inner();
        self.resumes.lock().unwrap().push(request.resume);
        let index = self.subscription_count.fetch_add(1, Ordering::SeqCst);
        let value = match index {
            0 => envelope_with_boot(SUBJECT, [0x11; 16], 7, 7.0),
            1 => envelope_with_boot(SUBJECT, [0x11; 16], 8, 8.0),
            2 => envelope_with_boot(SUBJECT, [0x22; 16], 1, 21.0),
            3 => envelope_with_boot(SUBJECT, [0x11; 16], 9, 9.0),
            _ => envelope_with_boot(SUBJECT, [0x22; 16], 2, 22.0),
        };
        Ok(Response::new(Box::pin(tokio_stream::iter([Ok(value)]))))
    }

    async fn clock_probe(
        &self,
        request: Request<ClockProbeRequest>,
    ) -> Result<Response<ClockProbeResponse>, Status> {
        let request = request.into_inner();
        Ok(Response::new(ClockProbeResponse {
            nonce: request.nonce,
            client_send_unix_ms: request.client_send_unix_ms,
            agent_receive_unix_ms: request.client_send_unix_ms.saturating_add(1),
            agent_send_unix_ms: request.client_send_unix_ms.saturating_add(1),
        }))
    }
}

#[tonic::async_trait]
impl PositionTelemetryService for InvalidClockService {
    type SubscribeStream = TestStream;

    async fn get_descriptor(
        &self,
        _request: Request<GetDescriptorRequest>,
    ) -> Result<Response<GetDescriptorResponse>, Status> {
        Ok(Response::new(descriptor()))
    }

    async fn subscribe(
        &self,
        _request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let value = envelope_with_boot(SUBJECT, [0x11; 16], 1, 41.0);
        let stream = async_stream::stream! {
            yield Ok(value);
            std::future::pending::<()>().await;
        };
        Ok(Response::new(Box::pin(stream)))
    }

    async fn clock_probe(
        &self,
        request: Request<ClockProbeRequest>,
    ) -> Result<Response<ClockProbeResponse>, Status> {
        let request = request.into_inner();
        let mut nonce = request.nonce;
        if self.probe_count.fetch_add(1, Ordering::SeqCst) > 0 {
            nonce[0] ^= 0xff;
        }
        Ok(Response::new(ClockProbeResponse {
            nonce,
            client_send_unix_ms: request.client_send_unix_ms,
            agent_receive_unix_ms: request.client_send_unix_ms.saturating_add(1),
            agent_send_unix_ms: request.client_send_unix_ms.saturating_add(1),
        }))
    }
}

const SUBJECT: [u8; 32] = [0x42; 32];
const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROFILE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CLIENT_URI: &str = "spiffe://palcompanion/core/trusted";

struct Cert {
    cert_pem: String,
    cert_der: Vec<u8>,
    key_pem: String,
}

struct Pki {
    ca_pem: String,
    server: Cert,
    trusted: Cert,
    wrong_san: Cert,
    expired: Cert,
    untrusted: Cert,
}

struct RunningServer {
    address: SocketAddr,
    latest: LatestTelemetry,
    shutdown: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

struct RunningReconnectServer {
    address: SocketAddr,
    resumes: Arc<Mutex<Vec<Option<ResumeCursor>>>>,
    shutdown: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

struct RunningInvalidClockServer {
    address: SocketAddr,
    probe_count: Arc<AtomicUsize>,
    shutdown: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

fn issue(issuer: &CertifiedIssuer<'_, KeyPair>, dns: Option<&str>, uri: Option<&str>) -> Cert {
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::default();
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "synthetic-test-only");
    params.extended_key_usages = if dns.is_some() {
        vec![ExtendedKeyUsagePurpose::ServerAuth]
    } else {
        vec![ExtendedKeyUsagePurpose::ClientAuth]
    };
    if let Some(dns) = dns {
        params
            .subject_alt_names
            .push(SanType::DnsName(Ia5String::try_from(dns).unwrap()));
    }
    if let Some(uri) = uri {
        params
            .subject_alt_names
            .push(SanType::URI(Ia5String::try_from(uri).unwrap()));
    }
    let cert = params.signed_by(&key, issuer).unwrap();
    Cert {
        cert_pem: cert.pem(),
        cert_der: cert.der().to_vec(),
        key_pem: key.serialize_pem(),
    }
}

fn issue_expired(issuer: &CertifiedIssuer<'_, KeyPair>, uri: &str) -> Cert {
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::default();
    params.not_before = date_time_ymd(2000, 1, 1);
    params.not_after = date_time_ymd(2001, 1, 1);
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    params
        .subject_alt_names
        .push(SanType::URI(Ia5String::try_from(uri).unwrap()));
    let cert = params.signed_by(&key, issuer).unwrap();
    Cert {
        cert_pem: cert.pem(),
        cert_der: cert.der().to_vec(),
        key_pem: key.serialize_pem(),
    }
}

fn pki() -> Pki {
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate().unwrap()).unwrap();
    let server = issue(&ca, Some("localhost"), None);
    let trusted = issue(&ca, None, Some(CLIENT_URI));
    let wrong_san = issue(&ca, None, Some("spiffe://palcompanion/core/wrong"));
    let expired = issue_expired(&ca, CLIENT_URI);

    let mut other_params = CertificateParams::default();
    other_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let other_ca =
        CertifiedIssuer::self_signed(other_params, KeyPair::generate().unwrap()).unwrap();
    let untrusted = issue(&other_ca, None, Some(CLIENT_URI));
    Pki {
        ca_pem: ca.pem(),
        server,
        trusted,
        wrong_san,
        expired,
        untrusted,
    }
}

fn descriptor() -> GetDescriptorResponse {
    GetDescriptorResponse {
        protocol_version: pal_domain::PROTOCOL_VERSION,
        agent_version: "0.1.0".to_owned(),
        rest_server_version: "v0.6".to_owned(),
        server_fingerprint: HASH.to_owned(),
        gate_profile_sha256: HASH.to_owned(),
        selected_interval_ms: 500,
        world_alias: "world-a".to_owned(),
        coordinate_space: CoordinateSpace::OfficialGameDataWorldV1 as i32,
        coordinate_profile_sha256: PROFILE.to_owned(),
    }
}

async fn connect(
    address: SocketAddr,
    ca_pem: &str,
    client: &Cert,
) -> Result<PositionTelemetryServiceClient<Channel>, tonic::transport::Error> {
    let tls = ClientTlsConfig::new()
        .domain_name("localhost")
        .ca_certificate(Certificate::from_pem(ca_pem))
        .identity(Identity::from_pem(&client.cert_pem, &client.key_pem));
    let channel = Endpoint::from_shared(format!("https://{address}"))
        .unwrap()
        .tls_config(tls)?
        .connect()
        .await?;
    Ok(PositionTelemetryServiceClient::new(channel))
}

async fn start_server(pki: &Pki, allowlist: ClientAllowlist) -> RunningServer {
    let latest = LatestTelemetry::new("world-a", SUBJECT, PROFILE).unwrap();
    let service =
        PositionTelemetryEndpoint::new(descriptor(), PROFILE, latest.clone(), allowlist).unwrap();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown, shutdown_rx) = oneshot::channel();
    let tls = ServerTlsConfig::new()
        .identity(Identity::from_pem(
            &pki.server.cert_pem,
            &pki.server.key_pem,
        ))
        .client_ca_root(Certificate::from_pem(&pki.ca_pem));
    let task = tokio::spawn(async move {
        Server::builder()
            .tls_config(tls)
            .unwrap()
            .add_service(service.into_server())
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    RunningServer {
        address,
        latest,
        shutdown,
        task,
    }
}

async fn start_oversized_server(pki: &Pki) -> RunningServer {
    let latest = LatestTelemetry::new("world-a", SUBJECT, PROFILE).unwrap();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown, shutdown_rx) = oneshot::channel();
    let tls = ServerTlsConfig::new()
        .identity(Identity::from_pem(
            &pki.server.cert_pem,
            &pki.server.key_pem,
        ))
        .client_ca_root(Certificate::from_pem(&pki.ca_pem));
    let task = tokio::spawn(async move {
        Server::builder()
            .tls_config(tls)
            .unwrap()
            .add_service(PositionTelemetryServiceServer::new(
                OversizedDescriptorService,
            ))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    RunningServer {
        address,
        latest,
        shutdown,
        task,
    }
}

async fn start_reconnect_server(pki: &Pki) -> RunningReconnectServer {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown, shutdown_rx) = oneshot::channel();
    let resumes = Arc::new(Mutex::new(Vec::new()));
    let service = ReconnectScriptService {
        subscription_count: Arc::new(AtomicUsize::new(0)),
        resumes: Arc::clone(&resumes),
    };
    let tls = ServerTlsConfig::new()
        .identity(Identity::from_pem(
            &pki.server.cert_pem,
            &pki.server.key_pem,
        ))
        .client_ca_root(Certificate::from_pem(&pki.ca_pem));
    let task = tokio::spawn(async move {
        Server::builder()
            .tls_config(tls)
            .unwrap()
            .add_service(PositionTelemetryServiceServer::new(service))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    RunningReconnectServer {
        address,
        resumes,
        shutdown,
        task,
    }
}

async fn start_invalid_clock_server(pki: &Pki) -> RunningInvalidClockServer {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown, shutdown_rx) = oneshot::channel();
    let probe_count = Arc::new(AtomicUsize::new(0));
    let service = InvalidClockService {
        probe_count: Arc::clone(&probe_count),
    };
    let tls = ServerTlsConfig::new()
        .identity(Identity::from_pem(
            &pki.server.cert_pem,
            &pki.server.key_pem,
        ))
        .client_ca_root(Certificate::from_pem(&pki.ca_pem));
    let task = tokio::spawn(async move {
        Server::builder()
            .tls_config(tls)
            .unwrap()
            .add_service(PositionTelemetryServiceServer::new(service))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    RunningInvalidClockServer {
        address,
        probe_count,
        shutdown,
        task,
    }
}

fn envelope(subject: [u8; 32], sequence: u64, x: f64) -> SubscribeResponse {
    envelope_with_boot(subject, [0x11; 16], sequence, x)
}

fn envelope_with_boot(
    subject: [u8; 32],
    boot_id: [u8; 16],
    sequence: u64,
    x: f64,
) -> SubscribeResponse {
    SubscribeResponse {
        protocol_version: pal_domain::PROTOCOL_VERSION,
        world_alias: "world-a".to_owned(),
        subject_id: subject.to_vec(),
        boot_id: boot_id.to_vec(),
        sequence,
        rest_completed_at_unix_ms: 1_700_000_000_000,
        age_at_emit_ms: 10,
        position_x: x,
        position_y: x + 1.0,
        position_z: x + 2.0,
        heading_degrees: Some(45.0),
        heading_source: HeadingSource::RotationZValidated as i32,
        trace_id: [0x33; 16].to_vec(),
        coordinate_space: CoordinateSpace::OfficialGameDataWorldV1 as i32,
        coordinate_profile_sha256: PROFILE.to_owned(),
    }
}

#[tokio::test]
async fn real_rustls_http2_mtls_fails_closed_for_identity_and_pairing() {
    let pki = pki();
    let trusted_fingerprint = certificate_sha256(&pki.trusted.cert_der);
    let wrong_san_fingerprint = certificate_sha256(&pki.wrong_san.cert_der);
    let expired_fingerprint = certificate_sha256(&pki.expired.cert_der);
    let allowlist = ClientAllowlist::new([
        AllowlistEntry::new(trusted_fingerprint.clone(), CLIENT_URI, "world-a", SUBJECT).unwrap(),
        AllowlistEntry::new(wrong_san_fingerprint, CLIENT_URI, "world-a", SUBJECT).unwrap(),
        AllowlistEntry::new(expired_fingerprint, CLIENT_URI, "world-a", SUBJECT).unwrap(),
    ]);
    let server = start_server(&pki, allowlist).await;

    let mut trusted = connect(server.address, &pki.ca_pem, &pki.trusted)
        .await
        .unwrap();
    let response = trusted
        .get_descriptor(GetDescriptorRequest {
            protocol_version: pal_domain::PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    assert_eq!(response.into_inner().world_alias, "world-a");

    let mut untrusted = connect(server.address, &pki.ca_pem, &pki.untrusted)
        .await
        .unwrap();
    assert!(
        untrusted
            .get_descriptor(GetDescriptorRequest {
                protocol_version: pal_domain::PROTOCOL_VERSION,
            })
            .await
            .is_err()
    );
    let mut expired = connect(server.address, &pki.ca_pem, &pki.expired)
        .await
        .unwrap();
    assert!(
        expired
            .get_descriptor(GetDescriptorRequest {
                protocol_version: pal_domain::PROTOCOL_VERSION,
            })
            .await
            .is_err()
    );

    let mut wrong_san = connect(server.address, &pki.ca_pem, &pki.wrong_san)
        .await
        .unwrap();
    assert_eq!(
        wrong_san
            .get_descriptor(GetDescriptorRequest {
                protocol_version: pal_domain::PROTOCOL_VERSION,
            })
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );

    for (world_alias, subject_id) in [("world-b", SUBJECT), ("world-a", [0x99; 32])] {
        let result = trusted
            .subscribe(SubscribeRequest {
                protocol_version: pal_domain::PROTOCOL_VERSION,
                world_alias: world_alias.to_owned(),
                subject_id: subject_id.to_vec(),
                resume: None,
            })
            .await;
        assert_eq!(result.err().unwrap().code(), Code::PermissionDenied);
    }

    let _ = server.shutdown.send(());
    server.task.await.unwrap();

    let revoked = AllowlistEntry::new(trusted_fingerprint.clone(), CLIENT_URI, "world-a", SUBJECT)
        .unwrap()
        .revoked();
    let stale_active_duplicate =
        AllowlistEntry::new(trusted_fingerprint, CLIENT_URI, "world-a", SUBJECT).unwrap();
    let revoked_server = start_server(
        &pki,
        ClientAllowlist::new([revoked, stale_active_duplicate]),
    )
    .await;
    let mut revoked_client = connect(revoked_server.address, &pki.ca_pem, &pki.trusted)
        .await
        .unwrap();
    assert_eq!(
        revoked_client
            .get_descriptor(GetDescriptorRequest {
                protocol_version: pal_domain::PROTOCOL_VERSION,
            })
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let _ = revoked_server.shutdown.send(());
    revoked_server.task.await.unwrap();

    let wrong_world = AllowlistEntry::new(
        certificate_sha256(&pki.trusted.cert_der),
        CLIENT_URI,
        "world-b",
        SUBJECT,
    )
    .unwrap();
    let wrong_world_server = start_server(&pki, ClientAllowlist::new([wrong_world])).await;
    let mut wrong_world_client = connect(wrong_world_server.address, &pki.ca_pem, &pki.trusted)
        .await
        .unwrap();
    assert_eq!(
        wrong_world_client
            .get_descriptor(GetDescriptorRequest {
                protocol_version: pal_domain::PROTOCOL_VERSION,
            })
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let _ = wrong_world_server.shutdown.send(());
    wrong_world_server.task.await.unwrap();
}

#[tokio::test]
async fn authorized_subscription_never_emits_another_subjects_latest_value() {
    let pki = pki();
    let allowlist = ClientAllowlist::new([AllowlistEntry::new(
        certificate_sha256(&pki.trusted.cert_der),
        CLIENT_URI,
        "world-a",
        SUBJECT,
    )
    .unwrap()]);
    let server = start_server(&pki, allowlist).await;
    assert_eq!(server.latest.connect(1), LatestConnectOutcome::Accepted);
    assert_eq!(
        server
            .latest
            .publish(1, envelope(SUBJECT, 1, 10.0))
            .unwrap(),
        LatestPublishOutcome::Accepted
    );
    assert_eq!(
        server
            .latest
            .publish(1, envelope([0x99; 32], 2, 999_999.0))
            .unwrap(),
        LatestPublishOutcome::IdentityMismatchDropped
    );

    let mut client = connect(server.address, &pki.ca_pem, &pki.trusted)
        .await
        .unwrap();
    let mut stream = client
        .subscribe(SubscribeRequest {
            protocol_version: pal_domain::PROTOCOL_VERSION,
            world_alias: "world-a".to_owned(),
            subject_id: SUBJECT.to_vec(),
            resume: None,
        })
        .await
        .unwrap()
        .into_inner();
    let received = stream.message().await.unwrap().unwrap();
    assert_eq!(received.subject_id, SUBJECT);
    assert_eq!(received.sequence, 1);
    assert_eq!(received.position_x, 10.0);

    drop(stream);
    drop(client);
    let _ = server.shutdown.send(());
    server.task.await.unwrap();
}

#[tokio::test]
async fn network_consumer_uses_real_mtls_and_disconnects_state_when_stream_ends() {
    let pki = pki();
    let allowlist = ClientAllowlist::new([AllowlistEntry::new(
        certificate_sha256(&pki.trusted.cert_der),
        CLIENT_URI,
        "world-a",
        SUBJECT,
    )
    .unwrap()]);
    let server = start_server(&pki, allowlist).await;
    server.latest.connect(1);
    server
        .latest
        .publish(1, envelope(SUBJECT, 1, 10.0))
        .unwrap();

    let config = NetworkConsumerConfig::new(
        format!("https://{}", server.address),
        "localhost",
        pki.ca_pem.as_bytes(),
        pki.trusted.cert_pem.as_bytes(),
        pki.trusted.key_pem.as_bytes(),
        "world-a",
        SUBJECT,
        PROFILE,
        500,
    )
    .unwrap();
    let (sender, mut source) = server_agent_channel("world-a", SUBJECT).unwrap();
    let consumer = NetworkConsumer::new(config, sender, 9).unwrap();
    let task = tokio::spawn(async move { consumer.run_once().await });

    let connected = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if let Some(event) = source.poll(0).unwrap() {
                break event;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        connected,
        PositionSourceEvent::Connected { generation: 9 }
    ));

    let sample = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if let Some(PositionSourceEvent::Sample(sample)) = source.poll(0).unwrap() {
                break sample;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(sample.sequence(), 1);

    server.latest.close();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert!(matches!(
        source.poll(0).unwrap(),
        Some(PositionSourceEvent::Disconnected { generation: 9 })
    ));
    let _ = server.shutdown.send(());
    server.task.await.unwrap();
}

#[tokio::test]
async fn network_consumer_rejects_oversized_grpc_message_at_codec_boundary() {
    let pki = pki();
    let server = start_oversized_server(&pki).await;
    let config = NetworkConsumerConfig::new(
        format!("https://{}", server.address),
        "localhost",
        pki.ca_pem.as_bytes(),
        pki.trusted.cert_pem.as_bytes(),
        pki.trusted.key_pem.as_bytes(),
        "world-a",
        SUBJECT,
        PROFILE,
        500,
    )
    .unwrap();
    let (sender, _) = server_agent_channel("world-a", SUBJECT).unwrap();
    let error = NetworkConsumer::new(config, sender, 1)
        .unwrap()
        .run_once()
        .await
        .unwrap_err();
    assert_eq!(error, NetworkConsumerError::MessageTooLarge);

    let _ = server.shutdown.send(());
    server.task.await.unwrap();
}

#[tokio::test]
async fn network_consumer_fails_terminally_when_gate_or_server_descriptor_pin_differs() {
    let pki = pki();
    let allowlist = ClientAllowlist::new([AllowlistEntry::new(
        certificate_sha256(&pki.trusted.cert_der),
        CLIENT_URI,
        "world-a",
        SUBJECT,
    )
    .unwrap()]);
    let server = start_server(&pki, allowlist).await;
    let config = NetworkConsumerConfig::new(
        format!("https://{}", server.address),
        "localhost",
        pki.ca_pem.as_bytes(),
        pki.trusted.cert_pem.as_bytes(),
        pki.trusted.key_pem.as_bytes(),
        "world-a",
        SUBJECT,
        PROFILE,
        500,
    )
    .unwrap()
    .with_descriptor_pins("cc".repeat(32), HASH)
    .unwrap();
    let (sender, _) = server_agent_channel("world-a", SUBJECT).unwrap();

    let error = NetworkConsumer::new(config, sender, 1)
        .unwrap()
        .run_once()
        .await
        .unwrap_err();
    assert_eq!(error, NetworkConsumerError::Identity);
    assert_eq!(error.class(), pal_telemetry::NetworkFailureClass::Terminal);

    let _ = server.shutdown.send(());
    server.task.await.unwrap();
}

#[tokio::test]
async fn run_with_reconnect_resumes_same_boot_replaces_boot_and_never_advances_rejected_cursor() {
    let pki = pki();
    let server = start_reconnect_server(&pki).await;
    let config = NetworkConsumerConfig::new(
        format!("https://{}", server.address),
        "localhost",
        pki.ca_pem.as_bytes(),
        pki.trusted.cert_pem.as_bytes(),
        pki.trusted.key_pem.as_bytes(),
        "world-a",
        SUBJECT,
        PROFILE,
        500,
    )
    .unwrap();
    let (sender, mut source) = server_agent_channel("world-a", SUBJECT).unwrap();
    let diagnostics = sender.clone();
    let result = NetworkConsumer::new(config, sender, 10)
        .unwrap()
        .run_with_reconnect(5)
        .await;
    assert_eq!(result.unwrap_err(), NetworkConsumerError::Disconnected);
    assert_eq!(diagnostics.diagnostics().accepted_samples, 4);
    assert_eq!(diagnostics.diagnostics().retired_boot_drops, 1);
    assert_eq!(diagnostics.diagnostics().old_generation_drops, 0);
    for generation in 10..=14 {
        assert_eq!(
            source.poll(0).unwrap(),
            Some(PositionSourceEvent::Connected { generation })
        );
        assert_eq!(
            source.poll(0).unwrap(),
            Some(PositionSourceEvent::Disconnected { generation })
        );
    }
    assert!(source.poll(0).unwrap().is_none());

    let resumes = server.resumes.lock().unwrap().clone();
    assert_eq!(resumes.len(), 5);
    assert_eq!(resumes[0], None);
    assert_eq!(resumes[1].as_ref().unwrap().sequence, 7);
    assert_eq!(resumes[2].as_ref().unwrap().sequence, 8);
    assert_eq!(resumes[3].as_ref().unwrap().boot_id, [0x22; 16]);
    assert_eq!(resumes[3].as_ref().unwrap().sequence, 1);
    assert_eq!(resumes[4], resumes[3]);

    let _ = server.shutdown.send(());
    server.task.await.unwrap();
}

#[tokio::test]
async fn cancellation_supervisor_keeps_generations_monotonic_and_disconnects_on_shutdown() {
    let pki = pki();
    let server = start_reconnect_server(&pki).await;
    let config = NetworkConsumerConfig::new(
        format!("https://{}", server.address),
        "localhost",
        pki.ca_pem.as_bytes(),
        pki.trusted.cert_pem.as_bytes(),
        pki.trusted.key_pem.as_bytes(),
        "world-a",
        SUBJECT,
        PROFILE,
        500,
    )
    .unwrap();
    let (sender, mut source) = server_agent_channel("world-a", SUBJECT).unwrap();
    let diagnostics = sender.clone();
    let (cancel, cancellation) = tokio::sync::watch::channel(false);
    let consumer = NetworkConsumer::new(config, sender, 40).unwrap();
    let task = tokio::spawn(async move { consumer.run_until_cancelled(cancellation).await });

    let mut connected = Vec::new();
    while connected.len() < 3 {
        if let PositionSourceEvent::Connected { generation } =
            next_source_event_realtime(&mut source).await
        {
            connected.push(generation);
        }
    }
    let shutdown_started = std::time::Instant::now();
    cancel.send(true).unwrap();

    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap(),
        Ok(())
    );
    assert_eq!(connected, [40, 41, 42]);
    assert_eq!(diagnostics.diagnostics().old_generation_drops, 0);
    assert!(shutdown_started.elapsed() < std::time::Duration::from_millis(500));

    let _ = server.shutdown.send(());
    server.task.await.unwrap();
}

#[tokio::test]
async fn cancellation_interrupts_an_active_stream_within_the_overlay_shutdown_budget() {
    let pki = pki();
    let allowlist = ClientAllowlist::new([AllowlistEntry::new(
        certificate_sha256(&pki.trusted.cert_der),
        CLIENT_URI,
        "world-a",
        SUBJECT,
    )
    .unwrap()]);
    let server = start_server(&pki, allowlist).await;
    server.latest.connect(1);
    server
        .latest
        .publish(1, envelope(SUBJECT, 1, 10.0))
        .unwrap();
    let config = NetworkConsumerConfig::new(
        format!("https://{}", server.address),
        "localhost",
        pki.ca_pem.as_bytes(),
        pki.trusted.cert_pem.as_bytes(),
        pki.trusted.key_pem.as_bytes(),
        "world-a",
        SUBJECT,
        PROFILE,
        500,
    )
    .unwrap();
    let (sender, mut source) = server_agent_channel("world-a", SUBJECT).unwrap();
    let (cancel, cancellation) = tokio::sync::watch::channel(false);
    let consumer = NetworkConsumer::new(config, sender, 50).unwrap();
    let task = tokio::spawn(async move { consumer.run_until_cancelled(cancellation).await });
    assert!(matches!(
        next_source_event_realtime(&mut source).await,
        PositionSourceEvent::Connected { generation: 50 }
    ));
    assert!(matches!(
        next_source_event_realtime(&mut source).await,
        PositionSourceEvent::Sample(_)
    ));

    let shutdown_started = std::time::Instant::now();
    cancel.send(true).unwrap();
    assert_eq!(task.await.unwrap(), Ok(()));
    assert!(shutdown_started.elapsed() < std::time::Duration::from_millis(500));

    let _ = server.shutdown.send(());
    server.task.await.unwrap();
}

#[tokio::test]
async fn cancellation_interrupts_transport_connect_within_the_overlay_shutdown_budget() {
    let pki = pki();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (accepted_sender, accepted) = tokio::sync::oneshot::channel();
    let blackhole = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let _ = accepted_sender.send(());
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        drop(socket);
    });
    let config = NetworkConsumerConfig::new(
        format!("https://{address}"),
        "localhost",
        pki.ca_pem.as_bytes(),
        pki.trusted.cert_pem.as_bytes(),
        pki.trusted.key_pem.as_bytes(),
        "world-a",
        SUBJECT,
        PROFILE,
        500,
    )
    .unwrap();
    let (sender, _) = server_agent_channel("world-a", SUBJECT).unwrap();
    let (cancel, cancellation) = tokio::sync::watch::channel(false);
    let consumer = NetworkConsumer::new(config, sender, 55).unwrap();
    let task = tokio::spawn(async move { consumer.run_until_cancelled(cancellation).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), accepted)
        .await
        .unwrap()
        .unwrap();

    let shutdown_started = std::time::Instant::now();
    cancel.send(true).unwrap();
    assert_eq!(task.await.unwrap(), Ok(()));
    assert!(shutdown_started.elapsed() < std::time::Duration::from_millis(500));
    blackhole.abort();
}

#[tokio::test]
async fn permanent_tls_peer_identity_failure_is_terminal_without_retrying_forever() {
    let pki = pki();
    let allowlist = ClientAllowlist::new([AllowlistEntry::new(
        certificate_sha256(&pki.trusted.cert_der),
        CLIENT_URI,
        "world-a",
        SUBJECT,
    )
    .unwrap()]);
    let server = start_server(&pki, allowlist).await;
    let config = NetworkConsumerConfig::new(
        format!("https://{}", server.address),
        "wrong-name.invalid",
        pki.ca_pem.as_bytes(),
        pki.trusted.cert_pem.as_bytes(),
        pki.trusted.key_pem.as_bytes(),
        "world-a",
        SUBJECT,
        PROFILE,
        500,
    )
    .unwrap();
    let (sender, mut source) = server_agent_channel("world-a", SUBJECT).unwrap();
    let (_cancel, cancellation) = tokio::sync::watch::channel(false);
    let consumer = NetworkConsumer::new(config, sender, 60).unwrap();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        consumer.run_until_cancelled(cancellation),
    )
    .await
    .expect("TLS peer authentication failure is terminal");

    assert_eq!(result, Err(NetworkConsumerError::TlsAuthentication));
    assert_eq!(
        NetworkConsumerError::TlsAuthentication.class(),
        pal_telemetry::NetworkFailureClass::Terminal
    );
    assert_eq!(source.poll(0).unwrap(), None);

    let _ = server.shutdown.send(());
    server.task.await.unwrap();
}

#[tokio::test]
async fn unavailable_agent_endpoint_keeps_retrying_until_cancelled() {
    let pki = pki();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let config = NetworkConsumerConfig::new(
        format!("https://{address}"),
        "localhost",
        pki.ca_pem.as_bytes(),
        pki.trusted.cert_pem.as_bytes(),
        pki.trusted.key_pem.as_bytes(),
        "world-a",
        SUBJECT,
        PROFILE,
        500,
    )
    .unwrap();
    let (sender, mut source) = server_agent_channel("world-a", SUBJECT).unwrap();
    let (cancel, cancellation) = tokio::sync::watch::channel(false);
    let consumer = NetworkConsumer::new(config, sender, 70).unwrap();
    let task = tokio::spawn(async move { consumer.run_until_cancelled(cancellation).await });

    tokio::time::sleep(std::time::Duration::from_millis(1_000)).await;
    assert!(
        !task.is_finished(),
        "connection refusal remains recoverable"
    );
    assert_eq!(source.poll(0).unwrap(), None);

    cancel.send(true).unwrap();
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_millis(500), task)
            .await
            .expect("cancellation remains prompt")
            .unwrap(),
        Ok(())
    );
}

#[tokio::test(start_paused = true)]
async fn invalid_clock_is_observable_before_disconnect_and_hides_the_retained_sample() {
    let pki = pki();
    let server = start_invalid_clock_server(&pki).await;
    let config = NetworkConsumerConfig::new(
        format!("https://{}", server.address),
        "localhost",
        pki.ca_pem.as_bytes(),
        pki.trusted.cert_pem.as_bytes(),
        pki.trusted.key_pem.as_bytes(),
        "world-a",
        SUBJECT,
        PROFILE,
        500,
    )
    .unwrap();
    let (sender, mut source) = server_agent_channel("world-a", SUBJECT).unwrap();
    let consumer = NetworkConsumer::new(config, sender, 15).unwrap();
    let task = tokio::spawn(async move { consumer.run_once().await });
    let mut reducer = StateReducer::default();

    let connected = next_source_event(&mut source).await;
    assert!(matches!(
        connected,
        PositionSourceEvent::Connected { generation: 15 }
    ));
    reducer
        .apply(Event::PositionSource(connected))
        .expect("connected event applies");

    let sample = next_source_event(&mut source).await;
    assert!(matches!(sample, PositionSourceEvent::Sample(_)));
    reducer
        .apply(Event::PositionSource(sample))
        .expect("sample applies");
    assert!(reducer.has_trusted_position());

    tokio::time::advance(pal_telemetry::PROBE_CADENCE).await;
    assert_eq!(task.await.unwrap(), Err(NetworkConsumerError::Clock));
    assert_eq!(server.probe_count.load(Ordering::SeqCst), 2);

    let clock_invalid = next_source_event(&mut source).await;
    assert!(matches!(
        clock_invalid,
        PositionSourceEvent::ClockInvalid { generation: 15, .. }
    ));
    reducer
        .apply(Event::PositionSource(clock_invalid))
        .expect("clock-invalid event applies");
    assert!(!reducer.has_trusted_position());
    let stale = reducer.core_state(31_000);
    assert!(stale.position_sample().is_some());
    assert_eq!(stale.freshness(), Freshness::Stale);

    let disconnected = next_source_event(&mut source).await;
    assert!(matches!(
        disconnected,
        PositionSourceEvent::Disconnected { generation: 15 }
    ));
    reducer
        .apply(Event::PositionSource(disconnected))
        .expect("disconnected event applies");
    let offline = reducer.core_state(31_001);
    assert!(offline.position_sample().is_some());
    assert_eq!(offline.freshness(), Freshness::Offline);
    assert!(source.poll(31_001).unwrap().is_none());

    let _ = server.shutdown.send(());
    server.task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn reconnect_fails_closed_after_clock_evidence_failure() {
    let pki = pki();
    let server = start_invalid_clock_server(&pki).await;
    let config = NetworkConsumerConfig::new(
        format!("https://{}", server.address),
        "localhost",
        pki.ca_pem.as_bytes(),
        pki.trusted.cert_pem.as_bytes(),
        pki.trusted.key_pem.as_bytes(),
        "world-a",
        SUBJECT,
        PROFILE,
        500,
    )
    .unwrap();
    let (sender, mut source) = server_agent_channel("world-a", SUBJECT).unwrap();
    let consumer = NetworkConsumer::new(config, sender, 21).unwrap();
    let task = tokio::spawn(async move { consumer.run_with_reconnect(2).await });

    assert_eq!(
        next_source_event(&mut source).await,
        PositionSourceEvent::Connected { generation: 21 }
    );
    assert!(matches!(
        next_source_event(&mut source).await,
        PositionSourceEvent::Sample(_)
    ));
    advance_until_probe_count(&server.probe_count, 2).await;
    assert_eq!(task.await.unwrap(), Err(NetworkConsumerError::Clock));
    assert_eq!(server.probe_count.load(Ordering::SeqCst), 2);

    let expected = [
        PositionSourceEvent::ClockInvalid {
            generation: 21,
            reason: pal_state::ClockInvalidReason::ProbeInvalid,
        },
        PositionSourceEvent::Disconnected { generation: 21 },
    ];
    for expected in expected {
        assert_eq!(next_source_event(&mut source).await, expected);
    }
    assert!(source.poll(31_000).unwrap().is_none());

    let _ = server.shutdown.send(());
    server.task.await.unwrap();
}

async fn next_source_event(source: &mut impl PositionSource) -> PositionSourceEvent {
    for _ in 0..10_000 {
        if let Some(event) = source.poll(0).expect("source remains valid") {
            return event;
        }
        tokio::time::advance(std::time::Duration::from_millis(1)).await;
    }
    panic!("source event was not produced after bounded scheduler progress");
}

async fn next_source_event_realtime(source: &mut impl PositionSource) -> PositionSourceEvent {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if let Some(event) = source.poll(0).expect("source remains valid") {
                return event;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("source event was not produced before the realtime deadline")
}

async fn advance_until_probe_count(probe_count: &AtomicUsize, expected: usize) {
    for _ in 0..40_000 {
        if probe_count.load(Ordering::SeqCst) >= expected {
            return;
        }
        tokio::time::advance(std::time::Duration::from_millis(1)).await;
    }
    panic!("clock probes did not reach the expected count");
}
