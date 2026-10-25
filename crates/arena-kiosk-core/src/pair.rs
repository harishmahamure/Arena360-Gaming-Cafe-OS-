use std::sync::{Arc, Mutex};
use std::time::Duration;

use arena_proto::{AppError, PAIR_PATH, PairRequest, PairResponse};
use rustls::pki_types::ServerName;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;

use crate::client::{hex_lower, parse_fingerprint, pinned_config_with};

pub const PAIR_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_ANSWER_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairError {
    BadInput(String),
    Io(String),
    Tls(String),
    PinMismatch {
        expected: String,
        presented: String,
    },
    Hub {
        error: AppError,
        retry_after_s: Option<u32>,
    },
    BadAnswer(String),
}

impl std::fmt::Display for PairError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadInput(m) => write!(f, "bad input: {m}"),
            Self::Io(m) => write!(f, "i/o: {m}"),
            Self::Tls(m) => write!(f, "tls: {m}"),
            Self::PinMismatch {
                expected,
                presented,
            } => write!(
                f,
                "tls: the hub certificate {presented} does not match the pinned {expected}"
            ),
            Self::Hub { error, .. } => write!(f, "{error}"),
            Self::BadAnswer(m) => write!(f, "unexpected answer: {m}"),
        }
    }
}

impl std::error::Error for PairError {}

fn server_name(host: &str) -> Result<ServerName<'static>, PairError> {
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => Ok(ServerName::IpAddress(ip.into())),
        Err(_) => ServerName::try_from(host.to_owned())
            .map_err(|e| PairError::BadInput(format!("host: {e}"))),
    }
}

pub async fn pair(
    host: &str,
    port: u16,
    pinned_fingerprint: &str,
    request: &PairRequest,
    wait: Duration,
) -> Result<PairResponse, PairError> {
    let bare = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let expected = hex_lower(
        &parse_fingerprint(pinned_fingerprint).map_err(|e| PairError::BadInput(e.to_string()))?,
    );
    let presented: Arc<Mutex<Option<[u8; 32]>>> = Arc::default();
    let config = pinned_config_with(pinned_fingerprint, Arc::clone(&presented))
        .map_err(|e| PairError::BadInput(e.to_string()))?;
    let name = server_name(&bare)?;
    let body = serde_json::to_string(request).map_err(|e| PairError::BadInput(e.to_string()))?;
    let attempt = async {
        let tcp = TcpStream::connect((bare.as_str(), port))
            .await
            .map_err(|e| PairError::Io(e.to_string()))?;
        let _ = tcp.set_nodelay(true);
        let mut tls = TlsConnector::from(config)
            .connect(name, tcp)
            .await
            .map_err(|e| match presented.lock().ok().and_then(|p| *p) {
                Some(got) => PairError::PinMismatch {
                    expected: expected.clone(),
                    presented: hex_lower(&got),
                },
                None => PairError::Tls(e.to_string()),
            })?;
        let head = format!(
            "POST {PAIR_PATH} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        tls.write_all(head.as_bytes())
            .await
            .map_err(|e| PairError::Io(e.to_string()))?;
        tls.write_all(body.as_bytes())
            .await
            .map_err(|e| PairError::Io(e.to_string()))?;
        let mut answer = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = match tls.read(&mut chunk).await {
                Ok(n) => n,
                Err(e) if !answer.is_empty() && e.kind() == std::io::ErrorKind::UnexpectedEof => 0,
                Err(e) => return Err(PairError::Io(e.to_string())),
            };
            if n == 0 {
                break;
            }
            answer.extend_from_slice(&chunk[..n]);
            if answer.len() > MAX_ANSWER_BYTES {
                return Err(PairError::BadAnswer("the answer is too long".into()));
            }
        }
        Ok(answer)
    };
    let answer = timeout(wait, attempt)
        .await
        .map_err(|_| PairError::Io("the hub did not answer in time".into()))??;
    let parsed = parse_answer(&answer)?;
    if !parsed.hub_fingerprint.eq_ignore_ascii_case(&expected) {
        return Err(PairError::PinMismatch {
            expected,
            presented: parsed.hub_fingerprint.to_ascii_lowercase(),
        });
    }
    Ok(parsed)
}

fn parse_answer(answer: &[u8]) -> Result<PairResponse, PairError> {
    let text = std::str::from_utf8(answer).map_err(|e| PairError::BadAnswer(e.to_string()))?;
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| PairError::BadAnswer("not an http answer".into()))?;
    let mut lines = head.split("\r\n");
    let status: u16 = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| PairError::BadAnswer("no status".into()))?;
    let header = |wanted: &str| {
        head.split("\r\n").skip(1).find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case(wanted)
                .then(|| value.trim().to_owned())
        })
    };
    let retry_after_s = header("retry-after").and_then(|v| v.parse().ok());
    let length: Option<usize> = header("content-length").and_then(|v| v.parse().ok());
    if status == 200 && length != Some(body.len()) {
        return Err(PairError::BadAnswer(
            "the answer is shorter or longer than its length".into(),
        ));
    }
    if status == 200 {
        return serde_json::from_str(body).map_err(|e| PairError::BadAnswer(e.to_string()));
    }
    match serde_json::from_str::<AppError>(body) {
        Ok(error) => Err(PairError::Hub {
            error,
            retry_after_s,
        }),
        Err(_) => Err(PairError::BadAnswer(format!("status {status}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::ErrorCode;

    #[test]
    fn answers_are_parsed_with_their_retry_after() {
        let body = r#"{"device_id":"D","token":"t","hub_fingerprint":"f"}"#;
        let ok = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        assert_eq!(parse_answer(ok.as_bytes()).unwrap().device_id, "D");
        for (length, sent) in [
            (Some(body.len() + 5), body),
            (Some(body.len()), &body[..10]),
            (None, body),
        ] {
            let head = length
                .map(|n| format!("Content-Length: {n}\r\n"))
                .unwrap_or_default();
            let cut = format!("HTTP/1.1 200 OK\r\n{head}\r\n{sent}");
            assert!(matches!(
                parse_answer(cut.as_bytes()).unwrap_err(),
                PairError::BadAnswer(_)
            ));
        }
        let paused = b"HTTP/1.1 429 Too Many Requests\r\nretry-after: 42\r\n\r\n{\"code\":\"PAIR_PIN_PAUSED\",\"message\":\"m\"}";
        match parse_answer(paused).unwrap_err() {
            PairError::Hub {
                error,
                retry_after_s,
            } => {
                assert_eq!(error.code, ErrorCode::PairPinPaused);
                assert_eq!(retry_after_s, Some(42));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            parse_answer(b"HTTP/1.1 404 Not Found\r\n\r\nnot found").unwrap_err(),
            PairError::BadAnswer(_)
        ));
    }
}
