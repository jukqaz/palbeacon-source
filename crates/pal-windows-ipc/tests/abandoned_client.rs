#![cfg(windows)]

use std::{
    fs::OpenOptions,
    sync::mpsc,
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
    session::{LocalSession, SessionFailure, run_session},
};
use tokio_util::sync::CancellationToken;

fn endpoint() -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!(
        r"\\.\pipe\PalBeacon.Test.abandoned.{}.{nonce}",
        std::process::id()
    )
}

fn envelope(message_id: u64, payload: Payload) -> LocalEnvelope {
    LocalEnvelope {
        protocol_version: PROTOCOL_VERSION,
        connection_id: vec![17; 16],
        message_id,
        reply_to_message_id: None,
        payload: Some(payload),
    }
}

fn hello() -> LocalEnvelope {
    envelope(
        1,
        Payload::ClientHello(ClientHello {
            role: ClientRole::ManagementUi as i32,
            process_id: std::process::id(),
            minimum_protocol_version: PROTOCOL_VERSION,
            maximum_protocol_version: PROTOCOL_VERSION,
        }),
    )
}

struct EchoSession {
    gate: HelloGate,
}

impl EchoSession {
    const fn new() -> Self {
        Self {
            gate: HelloGate::new(SessionPolicy::management_ui()),
        }
    }
}

impl LocalSession for EchoSession {
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
        FrameCodec::encode(&LocalEnvelope {
            protocol_version: PROTOCOL_VERSION,
            connection_id: envelope.connection_id.clone(),
            message_id: 1,
            reply_to_message_id: Some(envelope.message_id),
            payload: Some(Payload::Error(ProtocolError {
                code: ProtocolErrorCode::None as i32,
                field: String::new(),
                offending_value: String::new(),
            })),
        })
        .map(Some)
        .map_err(SessionFailure::other)
    }

    fn disconnected(&mut self) {}
}

#[test]
fn a_client_that_disconnects_before_accept_does_not_destroy_the_listener() {
    let endpoint = endpoint();
    let listener = PipeListener::bind(&endpoint, SessionPolicy::management_ui(), 1).unwrap();

    let abandoned = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&endpoint)
        .unwrap();
    drop(abandoned);

    let cancel = CancellationToken::new();
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let server_cancel = cancel.clone();
    let server = thread::spawn(move || match listener.accept(&server_cancel) {
        Ok(connection) => {
            result_tx.send(Ok(())).unwrap();
            let _ = run_session(connection, EchoSession::new(), server_cancel);
        }
        Err(error) => result_tx.send(Err(error)).unwrap(),
    });

    let mut client =
        PipeClient::connect(&endpoint, hello(), Duration::from_secs(2)).expect("second client");
    let response = client
        .request(
            envelope(2, Payload::Error(ProtocolError::default())),
            Duration::from_secs(2),
        )
        .expect("round trip after an abandoned client");
    assert_eq!(response.reply_to_message_id, Some(2));

    drop(client);
    cancel.cancel();
    result_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("server result")
        .expect("listener accepts the second client");
    server.join().unwrap();
}
