use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use pal_rest::{BasicAuthSecret, ClientConfig, ConnectedStreamVerifier, PalRestClient, RestError};

const METRICS: &[u8] = include_bytes!("../../../tests/fixtures/rest/metrics.json");

struct RecordingVerifier {
    allow: bool,
    calls: AtomicUsize,
    tuples: Mutex<Vec<(SocketAddr, SocketAddr)>>,
}

impl RecordingVerifier {
    fn new(allow: bool) -> Self {
        Self {
            allow,
            calls: AtomicUsize::new(0),
            tuples: Mutex::new(Vec::new()),
        }
    }
}

impl ConnectedStreamVerifier for RecordingVerifier {
    fn verify(&self, local_address: SocketAddr, peer_address: SocketAddr) -> bool {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.tuples
            .lock()
            .unwrap()
            .push((local_address, peer_address));
        self.allow
    }
}

#[tokio::test]
async fn verifier_rejection_sends_zero_http_bytes_or_authorization() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sent, received) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        sent.send(bytes).unwrap();
    });
    let verifier = Arc::new(RecordingVerifier::new(false));
    let client = PalRestClient::new_verified(
        ClientConfig::loopback(&format!("http://{address}/v1/api")).unwrap(),
        BasicAuthSecret::new("admin", "must-not-leak").unwrap(),
        verifier.clone(),
    )
    .unwrap();

    assert!(matches!(
        client.metrics().await,
        Err(RestError::ConnectionUntrusted)
    ));
    let bytes = received.recv_timeout(Duration::from_secs(2)).unwrap();
    server.join().unwrap();
    assert!(bytes.is_empty(), "untrusted connection received HTTP bytes");
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
    let tuples = verifier.tuples.lock().unwrap();
    assert_eq!(tuples.len(), 1);
    assert_eq!(tuples[0].1, address);
}

#[tokio::test]
async fn verified_request_uses_the_same_connected_socket_and_succeeds() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sent, received) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, peer) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let request = read_request(&mut stream);
        sent.send((peer, request)).unwrap();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            METRICS.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.write_all(METRICS).unwrap();
        stream.flush().unwrap();
    });
    let verifier = Arc::new(RecordingVerifier::new(true));
    let client = PalRestClient::new_verified(
        ClientConfig::loopback(&format!("http://{address}/v1/api")).unwrap(),
        BasicAuthSecret::new("admin", "test-password").unwrap(),
        verifier.clone(),
    )
    .unwrap();

    let metrics = client.metrics().await.unwrap();
    assert_eq!(metrics.value.server_fps, 57);
    let (server_peer, request) = received.recv_timeout(Duration::from_secs(2)).unwrap();
    server.join().unwrap();
    let request = String::from_utf8(request).unwrap();
    assert!(request.starts_with("GET /v1/api/metrics HTTP/1.1\r\n"));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("\r\nauthorization: basic ")
    );
    assert!(!request.contains("test-password"));
    let tuples = verifier.tuples.lock().unwrap();
    assert_eq!(tuples.as_slice(), &[(server_peer, address)]);
}

#[tokio::test]
async fn every_verified_request_uses_a_fresh_verified_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let request = read_request(&mut stream);
            assert!(!request.is_empty());
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                METRICS.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(METRICS).unwrap();
            stream.flush().unwrap();
        }
    });
    let verifier = Arc::new(RecordingVerifier::new(true));
    let client = PalRestClient::new_verified(
        ClientConfig::loopback(&format!("http://{address}/v1/api")).unwrap(),
        BasicAuthSecret::new("admin", "test-password").unwrap(),
        verifier.clone(),
    )
    .unwrap();

    client.metrics().await.unwrap();
    client.metrics().await.unwrap();
    server.join().unwrap();
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 2);
    assert_eq!(verifier.tuples.lock().unwrap().len(), 2);
}

fn read_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1_024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..count]);
    }
    request
}
