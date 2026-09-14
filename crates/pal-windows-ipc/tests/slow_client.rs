#![cfg(windows)]

use std::{
    fs::OpenOptions,
    io::Write,
    sync::mpsc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use pal_protocol::{
    FrameCodec, MAX_FRAME_LEN, PROTOCOL_VERSION,
    v2::{ClientHello, ClientRole, LocalEnvelope, ProtocolError, local_envelope::Payload},
};
use pal_windows_ipc::{
    handshake::{HelloGate, SessionPolicy},
    server::PipeListener,
    session::{LocalSession, PipeError, SessionFailure, run_session},
};
use tokio_util::sync::CancellationToken;

fn endpoint() -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!(
        r"\\.\pipe\PalBeacon.Test.slow.{}.{nonce}",
        std::process::id()
    )
}

fn envelope(message_id: u64, payload: Payload) -> LocalEnvelope {
    LocalEnvelope {
        protocol_version: PROTOCOL_VERSION,
        connection_id: vec![9; 16],
        message_id,
        reply_to_message_id: None,
        payload: Some(payload),
    }
}

struct LargeResponseSession {
    gate: HelloGate,
}

impl LargeResponseSession {
    const fn new() -> Self {
        Self {
            gate: HelloGate::new(SessionPolicy::management_ui()),
        }
    }
}

impl LocalSession for LargeResponseSession {
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
        Ok(Some(vec![0x5a; MAX_FRAME_LEN]))
    }

    fn disconnected(&mut self) {}
}

#[test]
fn a_slow_reader_is_disconnected_without_an_unbounded_response_queue() {
    let endpoint = endpoint();
    let listener =
        PipeListener::bind(&endpoint, SessionPolicy::management_ui(), 1).expect("listener");
    let cancel = CancellationToken::new();
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let server_cancel = cancel.clone();
    let server = thread::spawn(move || {
        let connection = listener.accept(&server_cancel).expect("accept");
        let result = run_session(connection, LargeResponseSession::new(), server_cancel);
        result_tx.send(result).unwrap();
    });

    let mut client = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&endpoint)
        .expect("raw named-pipe client");
    let hello = envelope(
        1,
        Payload::ClientHello(ClientHello {
            role: ClientRole::ManagementUi as i32,
            process_id: std::process::id(),
            minimum_protocol_version: PROTOCOL_VERSION,
            maximum_protocol_version: PROTOCOL_VERSION,
        }),
    );
    let first = envelope(2, Payload::Error(ProtocolError::default()));
    let second = envelope(3, Payload::Error(ProtocolError::default()));
    client
        .write_all(&FrameCodec::encode(&hello).unwrap())
        .unwrap();
    client
        .write_all(&FrameCodec::encode(&first).unwrap())
        .unwrap();
    client
        .write_all(&FrameCodec::encode(&second).unwrap())
        .unwrap();

    let result = result_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("bounded server result");
    assert!(matches!(result, Err(PipeError::WriteTimeout)));

    cancel.cancel();
    drop(client);
    server.join().unwrap();
}
