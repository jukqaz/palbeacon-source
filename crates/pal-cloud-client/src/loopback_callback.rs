use std::{
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    time::{Duration, Instant},
};

use reqwest::Url;
use secrecy::{ExposeSecret, SecretString};
use subtle::ConstantTimeEq as _;

use crate::{AccessAuthError, RandomMaterial, access_auth::REQUIRED_CALLBACK_TIMEOUT};

const MAX_CALLBACK_BYTES: usize = 8 * 1024;
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const CALLBACK_SUCCESS: &[u8] = concat!(
    "HTTP/1.1 200 OK\r\n",
    "Content-Type: text/html; charset=utf-8\r\n",
    "Cache-Control: no-store\r\n",
    "Connection: close\r\n\r\n",
    "<!doctype html><meta charset=utf-8><title>Pal Companion</title>",
    "<p>인증이 완료되었습니다. 이 창을 닫아도 됩니다.</p>"
)
.as_bytes();
const CALLBACK_FAILURE: &[u8] = concat!(
    "HTTP/1.1 400 Bad Request\r\n",
    "Content-Type: text/html; charset=utf-8\r\n",
    "Cache-Control: no-store\r\n",
    "Connection: close\r\n\r\n",
    "<!doctype html><meta charset=utf-8><title>Pal Companion</title>",
    "<p>인증 요청을 확인할 수 없습니다.</p>"
)
.as_bytes();

pub struct LoopbackCallback {
    listener: TcpListener,
    redirect_uri: Url,
    state: SecretString,
    nonce: SecretString,
    timeout: Duration,
}

impl LoopbackCallback {
    pub fn bind(timeout: Duration) -> Result<Self, AccessAuthError> {
        if timeout != REQUIRED_CALLBACK_TIMEOUT {
            return Err(AccessAuthError::InvalidPolicy);
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| AccessAuthError::CallbackRejected)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| AccessAuthError::CallbackRejected)?;
        let port = listener
            .local_addr()
            .map_err(|_| AccessAuthError::CallbackRejected)?
            .port();
        let state = SecretString::from(RandomMaterial::generate()?.base64url());
        let nonce = SecretString::from(RandomMaterial::generate()?.base64url());
        let redirect_uri = Url::parse(&format!(
            "http://127.0.0.1:{port}/oauth/callback/{}",
            nonce.expose_secret()
        ))
        .map_err(|_| AccessAuthError::CallbackRejected)?;
        Ok(Self {
            listener,
            redirect_uri,
            state,
            nonce,
            timeout,
        })
    }

    pub(crate) fn redirect_uri(&self) -> &Url {
        &self.redirect_uri
    }

    pub(crate) fn state_parameter(&self) -> &str {
        self.state.expose_secret()
    }

    pub(crate) fn wait(self) -> Result<SecretString, AccessAuthError> {
        let deadline = Instant::now() + self.timeout;
        loop {
            match self.listener.accept() {
                Ok((mut stream, peer)) => {
                    let result = self.handle_connection(&mut stream, peer);
                    let response = if result.is_ok() {
                        CALLBACK_SUCCESS
                    } else {
                        CALLBACK_FAILURE
                    };
                    let _ = stream.write_all(response);
                    let _ = stream.flush();
                    return result;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(AccessAuthError::CallbackRejected);
                    }
                    std::thread::sleep(ACCEPT_POLL_INTERVAL);
                }
                Err(_) => return Err(AccessAuthError::CallbackRejected),
            }
        }
    }

    fn handle_connection(
        &self,
        stream: &mut TcpStream,
        peer: SocketAddr,
    ) -> Result<SecretString, AccessAuthError> {
        if peer.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
            return Err(AccessAuthError::CallbackRejected);
        }
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|_| AccessAuthError::CallbackRejected)?;
        let mut buffer = Vec::with_capacity(1024);
        let mut chunk = [0_u8; 1024];
        let header_end = loop {
            let read = stream
                .read(&mut chunk)
                .map_err(|_| AccessAuthError::CallbackRejected)?;
            if read == 0 {
                return Err(AccessAuthError::CallbackRejected);
            }
            if buffer.len().saturating_add(read) > MAX_CALLBACK_BYTES {
                return Err(AccessAuthError::CallbackRejected);
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(index) = find_header_end(&buffer) {
                break index;
            }
        };
        if header_end != buffer.len() {
            return Err(AccessAuthError::CallbackRejected);
        }
        parse_callback_request(
            &buffer,
            self.redirect_uri.port().unwrap_or_default(),
            self.nonce.expose_secret(),
            self.state.expose_secret(),
        )
    }
}

impl std::fmt::Debug for LoopbackCallback {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoopbackCallback")
            .field("listener", &"127.0.0.1:<ephemeral>")
            .field("redirect_uri", &"<redacted>")
            .field("state", &"<redacted>")
            .field("nonce", &"<redacted>")
            .field("timeout", &self.timeout)
            .finish()
    }
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

fn parse_callback_request(
    bytes: &[u8],
    port: u16,
    expected_nonce: &str,
    expected_state: &str,
) -> Result<SecretString, AccessAuthError> {
    let request = std::str::from_utf8(bytes).map_err(|_| AccessAuthError::CallbackRejected)?;
    let mut lines = request.split("\r\n");
    let request_line = lines.next().ok_or(AccessAuthError::CallbackRejected)?;
    let parts = request_line.split(' ').collect::<Vec<_>>();
    if parts.len() != 3 || parts[0] != "GET" || parts[2] != "HTTP/1.1" {
        return Err(AccessAuthError::CallbackRejected);
    }
    let mut host = None;
    for line in lines {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(AccessAuthError::CallbackRejected);
        };
        let name = name.trim();
        let value = value.trim();
        if name.eq_ignore_ascii_case("host") {
            if host.replace(value).is_some() {
                return Err(AccessAuthError::CallbackRejected);
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            || (name.eq_ignore_ascii_case("content-length") && value != "0")
        {
            return Err(AccessAuthError::CallbackRejected);
        }
    }
    let expected_host = format!("127.0.0.1:{port}");
    if host != Some(expected_host.as_str()) {
        return Err(AccessAuthError::CallbackRejected);
    }
    let parsed = Url::parse(&format!("http://127.0.0.1:{port}{}", parts[1]))
        .map_err(|_| AccessAuthError::CallbackRejected)?;
    let expected_path = format!("/oauth/callback/{expected_nonce}");
    constant_time_equal(parsed.path().as_bytes(), expected_path.as_bytes())?;

    let mut code = None;
    let mut oauth_error = None;
    let mut state = None;
    for (name, value) in parsed.query_pairs() {
        match name.as_ref() {
            "code" if code.is_none() => code = Some(value.into_owned()),
            "error" if oauth_error.is_none() => oauth_error = Some(value.into_owned()),
            "state" if state.is_none() => state = Some(value.into_owned()),
            _ => return Err(AccessAuthError::CallbackRejected),
        }
    }
    let state = state.ok_or(AccessAuthError::CallbackRejected)?;
    constant_time_equal(state.as_bytes(), expected_state.as_bytes())?;
    match (code, oauth_error) {
        (Some(code), None)
            if !code.is_empty()
                && code.len() <= MAX_CALLBACK_BYTES
                && !code.bytes().any(|byte| byte.is_ascii_control()) =>
        {
            Ok(SecretString::from(code))
        }
        _ => Err(AccessAuthError::CallbackRejected),
    }
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> Result<(), AccessAuthError> {
    if left.len() != right.len() || !bool::from(left.ct_eq(right)) {
        Err(AccessAuthError::CallbackRejected)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(target: &str, port: u16, extra_headers: &str) -> Vec<u8> {
        format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra_headers}\r\n")
            .into_bytes()
    }

    #[test]
    fn exact_loopback_callback_returns_only_a_secret_code() {
        let bytes = request(
            "/oauth/callback/nonce?code=code-value&state=state-value",
            49152,
            "",
        );
        let code = parse_callback_request(&bytes, 49152, "nonce", "state-value").unwrap();
        assert_eq!(code.expose_secret(), "code-value");
    }

    #[test]
    fn rejects_wrong_host_path_state_method_body_or_duplicate_query() {
        let valid_target = "/oauth/callback/nonce?code=code-value&state=state-value";
        let cases = [
            request(valid_target, 49153, ""),
            request(
                "/oauth/callback/wrong?code=code-value&state=state-value",
                49152,
                "",
            ),
            request(
                "/oauth/callback/nonce?code=code-value&state=wrong",
                49152,
                "",
            ),
            request(
                "/oauth/callback/nonce?code=a&code=b&state=state-value",
                49152,
                "",
            ),
            request(valid_target, 49152, "Content-Length: 1\r\n"),
        ];
        for bytes in cases {
            assert!(matches!(
                parse_callback_request(&bytes, 49152, "nonce", "state-value"),
                Err(AccessAuthError::CallbackRejected)
            ));
        }
        let post = b"POST /oauth/callback/nonce?code=a&state=state-value HTTP/1.1\r\nHost: 127.0.0.1:49152\r\n\r\n";
        assert!(parse_callback_request(post, 49152, "nonce", "state-value").is_err());
    }

    #[test]
    fn debug_never_contains_state_nonce_or_redirect_uri() {
        let callback = LoopbackCallback::bind(REQUIRED_CALLBACK_TIMEOUT).unwrap();
        let rendered = format!("{callback:?}");
        assert!(!rendered.contains(callback.state_parameter()));
        assert!(!rendered.contains(callback.nonce.expose_secret()));
        assert!(!rendered.contains(callback.redirect_uri().as_str()));
    }
}
