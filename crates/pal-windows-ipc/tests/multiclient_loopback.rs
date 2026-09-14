#![cfg(windows)]

use std::{
    sync::Arc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use pal_protocol::{
    FrameCodec, PROTOCOL_VERSION,
    v2::{
        ClientHello, ClientRole, LocalEnvelope, ProtocolError, ProtocolErrorCode,
        local_envelope::Payload,
    },
};
use pal_windows_ipc::{
    client::PipeClient,
    handshake::{HelloGate, SessionPolicy},
    server::PipeListener,
    session::{LocalSession, PipeError, SessionFailure, run_session},
};
use tokio_util::sync::CancellationToken;

fn endpoint(label: &str) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!(
        r"\\.\pipe\PalBeacon.Test.{label}.{}.{nonce}",
        std::process::id()
    )
}

fn envelope(connection: u8, message_id: u64, payload: Payload) -> LocalEnvelope {
    LocalEnvelope {
        protocol_version: PROTOCOL_VERSION,
        connection_id: vec![connection; 16],
        message_id,
        reply_to_message_id: None,
        payload: Some(payload),
    }
}

fn hello(connection: u8) -> LocalEnvelope {
    envelope(
        connection,
        1,
        Payload::ClientHello(ClientHello {
            role: ClientRole::ManagementUi as i32,
            process_id: std::process::id(),
            minimum_protocol_version: PROTOCOL_VERSION,
            maximum_protocol_version: PROTOCOL_VERSION,
        }),
    )
}

struct CorrelatedSession {
    gate: HelloGate,
    next_response_id: u64,
}

impl CorrelatedSession {
    const fn new() -> Self {
        Self {
            gate: HelloGate::new(SessionPolicy::management_ui()),
            next_response_id: 1,
        }
    }
}

impl LocalSession for CorrelatedSession {
    fn handle(&mut self, envelope: LocalEnvelope) -> Result<Option<Vec<u8>>, SessionFailure> {
        if !self.gate.is_established() {
            self.gate
                .evaluate(&envelope)
                .map_err(SessionFailure::other)?;
            return Ok(None);
        }
        self.gate
            .validate_established(&envelope)
            .map_err(SessionFailure::other)?;
        let response = LocalEnvelope {
            protocol_version: PROTOCOL_VERSION,
            connection_id: envelope.connection_id.clone(),
            message_id: self.next_response_id,
            reply_to_message_id: Some(envelope.message_id),
            payload: Some(Payload::Error(ProtocolError {
                code: ProtocolErrorCode::None as i32,
                field: String::new(),
                offending_value: String::new(),
            })),
        };
        self.next_response_id += 1;
        FrameCodec::encode(&response)
            .map(Some)
            .map_err(SessionFailure::other)
    }

    fn disconnected(&mut self) {}
}

fn round_trip(endpoint: &str, connection: u8, message_id: u64) -> LocalEnvelope {
    let mut client =
        PipeClient::connect(endpoint, hello(connection), Duration::from_secs(2)).unwrap();
    client
        .request(
            envelope(
                connection,
                message_id,
                Payload::Error(ProtocolError::default()),
            ),
            Duration::from_secs(2),
        )
        .unwrap()
}

#[test]
fn two_clients_complete_correlated_requests_concurrently() {
    let endpoint = endpoint("multi");
    let listener =
        Arc::new(PipeListener::bind(&endpoint, SessionPolicy::management_ui(), 2).unwrap());
    let cancel = CancellationToken::new();
    let server = thread::spawn({
        let listener = Arc::clone(&listener);
        let cancel = cancel.clone();
        move || {
            let mut workers = Vec::new();
            for _ in 0..2 {
                let connection = listener.accept(&cancel).unwrap();
                let worker_cancel = cancel.clone();
                workers.push(thread::spawn(move || {
                    run_session(connection, CorrelatedSession::new(), worker_cancel)
                }));
            }
            for worker in workers {
                match worker.join().unwrap() {
                    Ok(()) | Err(PipeError::Cancelled) => {}
                    Err(error) => panic!("pipe session failed before teardown: {error}"),
                }
            }
        }
    });

    let first = thread::spawn({
        let endpoint = endpoint.clone();
        move || round_trip(&endpoint, 11, 101)
    });
    let second = thread::spawn({
        let endpoint = endpoint.clone();
        move || round_trip(&endpoint, 22, 202)
    });

    assert_eq!(first.join().unwrap().reply_to_message_id, Some(101));
    assert_eq!(second.join().unwrap().reply_to_message_id, Some(202));
    cancel.cancel();
    server.join().unwrap();
}
