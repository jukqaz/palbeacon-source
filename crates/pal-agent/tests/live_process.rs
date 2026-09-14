#![cfg(windows)]

use std::{
    env,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    process::{self, Child, Command, Stdio},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use pal_agent::{LiveProcessError, inspect_live_server};
use pal_rest::{BasicAuthSecret, ClientConfig, PalRestClient, RestError};

const METRICS: &[u8] = include_bytes!("../../../tests/fixtures/rest/metrics.json");

#[test]
fn loopback_listener_is_bound_to_the_exact_live_process() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1/api", listener.local_addr().unwrap());

    let identity = inspect_live_server(&endpoint, process::id()).unwrap();

    assert_eq!(identity.process_id(), process::id());
    assert_ne!(identity.creation_time_100ns(), 0);
    assert_ne!(identity.executable_sha256(), [0; 32]);
    identity.revalidate(&endpoint).unwrap();
}

#[test]
fn held_identity_refuses_a_listener_that_disappears_after_startup() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1/api", listener.local_addr().unwrap());
    let identity = inspect_live_server(&endpoint, process::id()).unwrap();

    drop(listener);

    assert_eq!(
        identity.revalidate(&endpoint).unwrap_err(),
        LiveProcessError::ListenerUnavailable
    );
}

#[test]
fn wrong_pid_and_remote_or_unbound_endpoints_are_refused() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1/api", listener.local_addr().unwrap());
    assert_eq!(
        inspect_live_server(&endpoint, process::id().wrapping_add(1)).unwrap_err(),
        LiveProcessError::ListenerOwnerMismatch
    );
    assert_eq!(
        inspect_live_server("http://192.0.2.1:8212/v1/api", process::id()).unwrap_err(),
        LiveProcessError::RemoteEndpoint
    );
    assert_eq!(
        inspect_live_server("http://127.0.0.1:9/v1/api", process::id()).unwrap_err(),
        LiveProcessError::ListenerUnavailable
    );
}

#[test]
fn held_identity_refuses_process_exit_even_when_another_process_can_take_the_port() {
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let mut child = spawn_listener_helper(address.to_string());
    let endpoint = format!("http://{address}/v1/api");
    let identity = inspect_live_server(&endpoint, child.id()).unwrap();

    child.kill().unwrap();
    child.wait().unwrap();
    let _takeover = wait_for_rebind(address.to_string());

    assert_eq!(
        identity.revalidate(&endpoint).unwrap_err(),
        LiveProcessError::InvalidProcess
    );
}

#[tokio::test]
async fn takeover_after_listener_precheck_sends_zero_credentials() {
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let mut child = spawn_listener_helper(address.to_string());
    let endpoint = format!("http://{address}/v1/api");
    let identity = inspect_live_server(&endpoint, child.id()).unwrap();
    identity.revalidate(&endpoint).unwrap();
    child.stdin.as_mut().unwrap().write_all(b"CLOSE\n").unwrap();
    child.stdin.as_mut().unwrap().flush().unwrap();
    let takeover = wait_for_rebind(address.to_string());
    assert!(
        child.try_wait().unwrap().is_none(),
        "signed process must remain alive while another process owns the connection"
    );
    let (sent, received) = mpsc::channel();
    let attacker = thread::spawn(move || {
        let (mut stream, _) = takeover.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        sent.send(bytes).unwrap();
    });
    let identity = Arc::new(identity);
    let rest_client = PalRestClient::new_verified(
        ClientConfig::loopback(&endpoint).unwrap(),
        BasicAuthSecret::new("admin", "must-not-leak").unwrap(),
        identity,
    )
    .unwrap();

    assert!(matches!(
        rest_client.metrics().await,
        Err(RestError::ConnectionUntrusted)
    ));
    let bytes = received.recv_timeout(Duration::from_secs(2)).unwrap();
    attacker.join().unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(
        bytes.is_empty(),
        "takeover received authenticated HTTP bytes"
    );
}

#[tokio::test]
async fn verified_established_connection_owned_by_live_process_succeeds() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let endpoint = format!("http://{address}/v1/api");
    let identity = Arc::new(inspect_live_server(&endpoint, process::id()).unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream);
        assert!(
            String::from_utf8_lossy(&request)
                .to_ascii_lowercase()
                .contains("\r\nauthorization: basic ")
        );
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            METRICS.len()
        );
        stream.write_all(headers.as_bytes()).unwrap();
        stream.write_all(METRICS).unwrap();
        stream.flush().unwrap();
    });
    let rest_client = PalRestClient::new_verified(
        ClientConfig::loopback(&endpoint).unwrap(),
        BasicAuthSecret::new("admin", "test-password").unwrap(),
        identity,
    )
    .unwrap();

    assert_eq!(rest_client.metrics().await.unwrap().value.server_fps, 57);
    server.join().unwrap();
}

fn read_http_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
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

fn spawn_listener_helper(address: String) -> Child {
    let mut child = Command::new(env::current_exe().unwrap())
        .args(["--exact", "listener_helper_process", "--nocapture"])
        .env("PAL_AGENT_LISTENER_HELPER", address)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut line = String::new();
    while Instant::now() < deadline {
        line.clear();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        if line.contains("LISTENER_READY") {
            return child;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("listener helper did not become ready");
}

fn wait_for_rebind(address: String) -> TcpListener {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match TcpListener::bind(&address) {
            Ok(listener) => return listener,
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Err(error) => panic!("failed to take over helper port: {error}"),
        }
    }
}

#[test]
fn listener_helper_process() {
    let Ok(address) = env::var("PAL_AGENT_LISTENER_HELPER") else {
        return;
    };
    let listener = TcpListener::bind(address).unwrap();
    println!("LISTENER_READY");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    if line.trim() == "CLOSE" {
        drop(listener);
        line.clear();
        let _ = std::io::stdin().read_line(&mut line);
    }
}
