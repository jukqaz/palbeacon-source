use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use pal_rest::{
    BasicAuthSecret, ClientConfig, EndpointKind, PalRestClient, PlayerSelector, Pseudonymizer,
    RestError,
};

const METRICS: &[u8] = include_bytes!("../../../tests/fixtures/rest/metrics.json");
const INFO: &[u8] = include_bytes!("../../../tests/fixtures/rest/info.json");
const GAME_DATA: &[u8] = include_bytes!("../../../tests/fixtures/rest/game-data-sensitive.json");
const SETTINGS: &[u8] = br#"{
    "ExpRate":1.0,
    "PalCaptureRate":1.0,
    "PalSpawnNumRate":1.0,
    "CollectionDropRate":1.0,
    "CollectionObjectHpRate":1.0,
    "CollectionObjectRespawnSpeedRate":1.0,
    "EnemyDropItemRate":2.0,
    "BaseCampWorkerMaxNum":15,
    "PalEggDefaultHatchingTime":0.0,
    "WorkSpeedRate":1.0,
    "bEnableFastTravel":true
}"#;

struct TestServer {
    base_url: String,
    request: mpsc::Receiver<Vec<u8>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn respond(response: Vec<u8>) -> Self {
        Self::respond_after(Duration::ZERO, response)
    }

    fn respond_after(delay: Duration, response: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, request) = mpsc::channel();
        let thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request_bytes = read_request(&mut stream);
            sender.send(request_bytes).unwrap();
            thread::sleep(delay);
            let _ = stream.write_all(&response);
            let _ = stream.flush();
        });
        Self {
            base_url: format!("http://{address}/v1/api"),
            request,
            thread: Some(thread),
        }
    }

    fn finish(mut self) -> Vec<u8> {
        let request = self.request.recv_timeout(Duration::from_secs(2)).unwrap();
        self.thread.take().unwrap().join().unwrap();
        request
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
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

fn response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut output = format!("HTTP/1.1 {status}\r\nConnection: close\r\n").into_bytes();
    for (name, value) in headers {
        output.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    output.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
    output.extend_from_slice(body);
    output
}

fn client(server: &TestServer) -> PalRestClient {
    PalRestClient::new_unverified_for_probe(
        ClientConfig::loopback(&server.base_url).unwrap(),
        BasicAuthSecret::new("admin", "test-password").unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn decodes_bounded_metrics_and_sends_basic_auth_only_to_fixed_endpoint() {
    let server = TestServer::respond(response(
        "200 OK",
        &[("Content-Type", "application/json; charset=utf-8")],
        METRICS,
    ));
    let result = client(&server).metrics().await.unwrap();

    assert_eq!(result.value.server_fps, 57);
    assert_eq!(result.body_bytes, METRICS.len());
    assert_eq!(result.actor_count, 0);
    let request = String::from_utf8(server.finish()).unwrap();
    assert!(request.starts_with("GET /v1/api/metrics HTTP/1.1\r\n"));
    assert!(request.contains("\r\naccept-encoding: identity\r\n"));
    assert!(request.contains("\r\nauthorization: Basic "));
    assert!(!request.contains("test-password"));
}

#[tokio::test]
async fn game_data_returns_only_sanitized_selected_player() {
    let server = TestServer::respond(response(
        "200 OK",
        &[("Content-Type", "application/json")],
        GAME_DATA,
    ));
    let result = client(&server)
        .game_data(
            &PlayerSelector::user_id("selected-user").unwrap(),
            &Pseudonymizer::new([5_u8; 32]),
            "world-a",
        )
        .await
        .unwrap();

    assert_eq!(result.value.x, 12_345.5);
    assert_eq!(result.actor_count, 4);
    server.finish();
}

#[tokio::test]
async fn info_returns_only_sanitized_server_identity() {
    let server = TestServer::respond(response(
        "200 OK",
        &[("Content-Type", "application/json")],
        INFO,
    ));
    let result = client(&server)
        .info(&Pseudonymizer::new([6_u8; 32]), "world-a")
        .await
        .unwrap();

    assert_eq!(result.value.version, "v1.0.1");
    assert_eq!(result.value.server_subject_id.len(), 64);
    server.finish();
}

#[tokio::test]
async fn settings_uses_the_fixed_read_only_endpoint_and_discards_unneeded_fields() {
    let server = TestServer::respond(response(
        "200 OK",
        &[("Content-Type", "application/json")],
        SETTINGS,
    ));
    let result = client(&server).settings().await.unwrap();

    assert_eq!(result.value.enemy_drop_rate, 2.0);
    assert_eq!(result.value.base_worker_limit, 15);
    assert!(result.value.fast_travel_enabled);
    let request = String::from_utf8(server.finish()).unwrap();
    assert!(request.starts_with("GET /v1/api/settings HTTP/1.1\r\n"));
    assert!(request.contains("\r\nauthorization: Basic "));
}

#[tokio::test]
async fn unauthorized_redirect_and_non_200_success_fail_closed() {
    for (response_bytes, expected) in [
        (
            response("401 Unauthorized", &[], b"secret-body"),
            RestError::Unauthorized,
        ),
        (
            response(
                "302 Found",
                &[("Location", "http://203.0.113.1/private")],
                b"",
            ),
            RestError::RedirectRejected,
        ),
        (
            response("204 No Content", &[], b""),
            RestError::UnexpectedStatus(204),
        ),
    ] {
        let server = TestServer::respond(response_bytes);
        let error = client(&server).metrics().await.unwrap_err();
        assert_eq!(
            std::mem::discriminant(&error),
            std::mem::discriminant(&expected)
        );
        server.finish();
    }
}

#[tokio::test]
async fn timeout_is_bounded_and_response_body_is_never_reported() {
    let server = TestServer::respond_after(
        Duration::from_millis(200),
        response(
            "500 Internal Server Error",
            &[],
            b"highly-sensitive-response-fragment",
        ),
    );
    let config = ClientConfig::loopback(&server.base_url)
        .unwrap()
        .with_timeouts(Duration::from_millis(25), Duration::from_millis(25))
        .unwrap();
    let client = PalRestClient::new_unverified_for_probe(
        config,
        BasicAuthSecret::new("admin", "password").unwrap(),
    )
    .unwrap();

    let error = client.metrics().await.unwrap_err();
    assert!(matches!(error, RestError::Timeout));
    assert!(!format!("{error:?}").contains("highly-sensitive"));
    assert!(!error.to_string().contains("highly-sensitive"));
    server.finish();
}

#[tokio::test]
async fn declared_and_streamed_oversize_bodies_are_rejected() {
    let declared = TestServer::respond(response(
        "200 OK",
        &[("Content-Type", "application/json")],
        &[b'x'; 65],
    ));
    let declared_client = PalRestClient::new_unverified_for_probe(
        ClientConfig::loopback(&declared.base_url)
            .unwrap()
            .with_max_decoded_body_bytes(64)
            .unwrap(),
        BasicAuthSecret::new("admin", "password").unwrap(),
    )
    .unwrap();
    assert!(matches!(
        declared_client.metrics().await,
        Err(RestError::ResponseTooLarge)
    ));
    declared.finish();

    let chunked_body =
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n20\r\nxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\r\n21\r\nyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy\r\n0\r\n\r\n".to_vec();
    let streamed = TestServer::respond(chunked_body);
    let streamed_client = PalRestClient::new_unverified_for_probe(
        ClientConfig::loopback(&streamed.base_url)
            .unwrap()
            .with_max_decoded_body_bytes(64)
            .unwrap(),
        BasicAuthSecret::new("admin", "password").unwrap(),
    )
    .unwrap();
    assert!(matches!(
        streamed_client.metrics().await,
        Err(RestError::ResponseTooLarge)
    ));
    streamed.finish();
}

#[tokio::test]
async fn compression_non_json_and_malformed_json_fail_closed() {
    let cases = [
        (
            response(
                "200 OK",
                &[
                    ("Content-Type", "application/json"),
                    ("Content-Encoding", "gzip"),
                ],
                b"compressed-secret",
            ),
            EndpointKind::Metrics,
        ),
        (
            response("200 OK", &[("Content-Type", "text/plain")], METRICS),
            EndpointKind::Metrics,
        ),
        (
            response(
                "200 OK",
                &[("Content-Type", "application/json")],
                b"{malformed",
            ),
            EndpointKind::Metrics,
        ),
    ];

    for (response_bytes, _endpoint) in cases {
        let server = TestServer::respond(response_bytes);
        assert!(client(&server).metrics().await.is_err());
        server.finish();
    }
}
