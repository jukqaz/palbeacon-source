use std::{env, process::ExitCode, sync::Arc, time::SystemTime};

use pal_agent::{
    AgentPipeline, HealthMonitor, Poller, PollerExit, RestGameDataSource, RestTransport,
    SecretBundle, SingleInstanceGuard, bind_preverified_gate, build_agent_descriptor,
    finalize_remote_startup_gate, finalize_startup_gate, inspect_live_server,
    load_protected_config, parse_command, parse_gate_profile, parse_verification_key,
    preverify_startup_gate, read_protected_file,
};
use pal_rest::{ClientConfig, PalRestClient, Pseudonymizer};
use pal_telemetry::{LatestTelemetry, PositionTelemetryEndpoint};
use thiserror::Error;
use tokio::{net::TcpListener, sync::watch};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};
use zeroize::Zeroizing;

const MAX_PROFILE_BYTES: usize = 1024 * 1024;
const MAX_SECRET_BYTES: usize = 64 * 1024;
const MAX_KEY_BYTES: usize = 1024 * 1024;
const MAX_CERTIFICATE_BYTES: usize = 1024 * 1024;

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("pal-agent runtime initialization failed");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), RuntimeError> {
    let command = parse_command(env::args_os()).map_err(|_| RuntimeError::Configuration)?;
    let config =
        load_protected_config(&command.config_path).map_err(|_| RuntimeError::ProtectedFile)?;

    let profile_bytes = read_protected_file(config.profile_path(), MAX_PROFILE_BYTES)
        .map_err(|_| RuntimeError::ProtectedFile)?;
    let profile = parse_gate_profile(&profile_bytes).map_err(|_| RuntimeError::GateRefused)?;

    let verification_key_bytes = Zeroizing::new(
        read_protected_file(config.gate_verification_key_path(), MAX_SECRET_BYTES)
            .map_err(|_| RuntimeError::ProtectedFile)?,
    );
    let verification_key =
        parse_verification_key(&verification_key_bytes).map_err(|_| RuntimeError::ProtectedFile)?;
    let preverified = preverify_startup_gate(
        &config.startup_gate_config(),
        &profile,
        &verification_key,
        SystemTime::now(),
    )
    .map_err(|_| RuntimeError::GateRefused)?;
    let live_server = match config.rest_transport() {
        RestTransport::LocalProcess => {
            let live_server = Arc::new(
                inspect_live_server(config.rest_base_url(), preverified.process_id())
                    .map_err(|_| RuntimeError::GateRefused)?,
            );
            bind_preverified_gate(&preverified, live_server.as_ref())
                .map_err(|_| RuntimeError::GateRefused)?;
            Some(live_server)
        }
        RestTransport::ExplicitRemoteHttps => None,
    };

    let secret_bytes = Zeroizing::new(
        read_protected_file(config.secrets_path(), MAX_SECRET_BYTES)
            .map_err(|_| RuntimeError::ProtectedFile)?,
    );
    let secrets = SecretBundle::parse(&secret_bytes).map_err(|_| RuntimeError::ProtectedFile)?;
    let expected_player_subject = secrets
        .expected_player_subject_id()
        .map_err(|_| RuntimeError::ProtectedFile)?;
    let _instance = SingleInstanceGuard::acquire(config.world_alias(), &expected_player_subject)
        .map_err(|_| RuntimeError::AlreadyRunning)?;
    let pseudonymizer = Arc::new(Pseudonymizer::new(
        secrets
            .pseudonymization_key()
            .map_err(|_| RuntimeError::ProtectedFile)?,
    ));
    let rest_config = ClientConfig::new(
        config.rest_base_url(),
        config
            .endpoint_policy()
            .map_err(|_| RuntimeError::Configuration)?,
    )
    .map_err(|_| RuntimeError::Configuration)?;
    let rest_auth = secrets
        .rest_auth()
        .map_err(|_| RuntimeError::ProtectedFile)?;
    let rest_client = Arc::new(
        match (&live_server, config.rest_transport()) {
            (Some(live_server), RestTransport::LocalProcess) => {
                PalRestClient::new_verified(rest_config, rest_auth, live_server.clone())
            }
            (None, RestTransport::ExplicitRemoteHttps) => {
                PalRestClient::new_explicit_remote_https(rest_config, rest_auth)
            }
            _ => return Err(RuntimeError::Configuration),
        }
        .map_err(|_| RuntimeError::Configuration)?,
    );

    // Local mode binds each fresh connection to the signed process before credentials leave
    // this process. Remote mode instead uses the exact HTTPS allowlist and TLS validation.
    if let Some(live_server) = &live_server {
        live_server
            .revalidate(config.rest_base_url())
            .map_err(|_| RuntimeError::GateRefused)?;
    }
    let current_info = rest_client
        .info(pseudonymizer.as_ref(), config.world_alias())
        .await
        .map_err(|_| RuntimeError::GateRefused)?
        .value;
    if let Some(live_server) = &live_server {
        live_server
            .revalidate(config.rest_base_url())
            .map_err(|_| RuntimeError::GateRefused)?;
    }
    let approved = match &live_server {
        Some(live_server) => finalize_startup_gate(
            &preverified,
            &current_info,
            live_server.as_ref(),
            SystemTime::now(),
        ),
        None => finalize_remote_startup_gate(&preverified, &current_info, SystemTime::now()),
    }
    .map_err(|_| RuntimeError::GateRefused)?;

    let server_certificate = Zeroizing::new(
        read_protected_file(config.server_certificate_path(), MAX_CERTIFICATE_BYTES)
            .map_err(|_| RuntimeError::ProtectedFile)?,
    );
    let server_private_key = Zeroizing::new(
        read_protected_file(config.server_private_key_path(), MAX_KEY_BYTES)
            .map_err(|_| RuntimeError::ProtectedFile)?,
    );
    let client_ca_certificate = Zeroizing::new(
        read_protected_file(config.client_ca_certificate_path(), MAX_CERTIFICATE_BYTES)
            .map_err(|_| RuntimeError::ProtectedFile)?,
    );
    let identity = Identity::from_pem(server_certificate.as_slice(), server_private_key.as_slice());
    let client_ca = Certificate::from_pem(client_ca_certificate.as_slice());
    let tls = ServerTlsConfig::new()
        .identity(identity)
        .client_ca_root(client_ca);

    let latest = LatestTelemetry::new(
        config.world_alias(),
        expected_player_subject,
        approved.coordinate_profile_sha256.clone(),
    )
    .map_err(|_| RuntimeError::Telemetry)?;
    let health = HealthMonitor::default();
    let pipeline = AgentPipeline::new(
        config.world_alias(),
        expected_player_subject,
        approved.coordinate_profile_sha256.clone(),
        approved.rotation_z_validated,
        latest.clone(),
        health.clone(),
    )
    .map_err(|_| RuntimeError::Telemetry)?;
    let selector = secrets
        .selector()
        .map_err(|_| RuntimeError::ProtectedFile)?;
    let source = Arc::new(match live_server {
        Some(live_server) => RestGameDataSource::new_local_process(
            rest_client,
            selector,
            pseudonymizer,
            config.world_alias(),
            live_server,
            config.rest_base_url(),
        ),
        None => RestGameDataSource::new_explicit_remote_https(
            rest_client,
            selector,
            pseudonymizer,
            config.world_alias(),
            current_info,
        ),
    });
    let poller = Poller::new(approved.selected_interval, source, pipeline, health)
        .map_err(|_| RuntimeError::GateRefused)?;

    let descriptor = build_agent_descriptor(config.world_alias(), &approved)
        .map_err(|_| RuntimeError::Telemetry)?;
    let allowlist = config
        .client_allowlist(&expected_player_subject)
        .map_err(|_| RuntimeError::Configuration)?;
    let latest_on_trust_loss = latest.clone();
    let service = PositionTelemetryEndpoint::new(
        descriptor,
        approved.coordinate_profile_sha256,
        latest,
        allowlist,
    )
    .map_err(|_| RuntimeError::Telemetry)?;
    let server = Server::builder()
        .tls_config(tls)
        .map_err(|_| RuntimeError::Telemetry)?
        .add_service(service.into_server());
    let listener = TcpListener::bind(config.listen_address())
        .await
        .map_err(|_| RuntimeError::Telemetry)?;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    // Subscribe before the poller can publish trust loss. A late subscriber would otherwise
    // consider an already-published `true` current and wait forever for another change.
    let mut service_shutdown_rx = shutdown_tx.subscribe();
    let poll_shutdown = shutdown_tx.clone();
    let poll_task = tokio::spawn(async move {
        let exit = poller.run(shutdown_rx).await;
        if exit == PollerExit::TrustLost {
            latest_on_trust_loss.close();
            let _ = poll_shutdown.send(true);
        }
        exit
    });
    let shutdown_signal = {
        let shutdown_tx = shutdown_tx.clone();
        async move {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                changed = service_shutdown_rx.changed() => {
                    let _ = changed;
                }
            }
            let _ = shutdown_tx.send(true);
        }
    };
    let result = server
        .serve_with_incoming_shutdown(TcpListenerStream::new(listener), shutdown_signal)
        .await;
    let _ = shutdown_tx.send(true);
    let poll_exit = poll_task.await.map_err(|_| RuntimeError::Telemetry)?;
    result.map_err(|_| RuntimeError::Telemetry)?;
    if poll_exit == PollerExit::TrustLost {
        return Err(RuntimeError::GateRefused);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
enum RuntimeError {
    #[error("pal-agent configuration was refused")]
    Configuration,
    #[error("pal-agent protected file policy was refused")]
    ProtectedFile,
    #[error("pal-agent Gate A startup was refused")]
    GateRefused,
    #[error("pal-agent single-instance lock is already held")]
    AlreadyRunning,
    #[error("pal-agent telemetry runtime failed")]
    Telemetry,
}
