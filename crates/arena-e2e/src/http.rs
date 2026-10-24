//! A minimal HTTP/1.1 client for the Hub's loopback API (`127.0.0.1:<health port>`): one
//! request per connection, `Connection: close`, JSON in and out. No HTTP client crate.

use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// How long one request may take.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The Hub's loopback API.
#[derive(Debug, Clone)]
pub struct Hub {
    pub port: u16,
}

/// A response: the status and the JSON body (`Null` when empty or not JSON).
#[derive(Debug, Clone)]
pub struct Res {
    pub status: u16,
    pub body: Value,
}

impl Res {
    /// The body's error `code`, if any.
    pub fn code(&self) -> Option<&str> {
        self.body.get("code").and_then(Value::as_str)
    }
}

impl Hub {
    pub async fn get(&self, path: &str, token: Option<&str>) -> Result<Res, String> {
        self.call("GET", path, token, None).await
    }

    pub async fn post(&self, path: &str, token: Option<&str>, body: &Value) -> Result<Res, String> {
        self.call("POST", path, token, Some(body)).await
    }

    async fn call(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<&Value>,
    ) -> Result<Res, String> {
        tokio::time::timeout(TIMEOUT, self.call_inner(method, path, token, body))
            .await
            .map_err(|_| format!("{method} {path}: timed out"))?
    }

    async fn call_inner(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<&Value>,
    ) -> Result<Res, String> {
        let mut s = TcpStream::connect(("127.0.0.1", self.port))
            .await
            .map_err(|e| format!("{method} {path}: connect: {e}"))?;
        let body = body.map(Value::to_string).unwrap_or_default();
        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\nAccept: application/json\r\n",
            self.port
        );
        if let Some(t) = token {
            req.push_str(&format!("Authorization: Bearer {t}\r\n"));
        }
        if method == "POST" {
            req.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            ));
        }
        req.push_str("\r\n");
        req.push_str(&body);
        s.write_all(req.as_bytes())
            .await
            .map_err(|e| format!("{method} {path}: write: {e}"))?;
        let mut raw = Vec::new();
        s.read_to_end(&mut raw)
            .await
            .map_err(|e| format!("{method} {path}: read: {e}"))?;
        parse_response(&raw).map_err(|e| format!("{method} {path}: {e}"))
    }
}

/// Parses a whole HTTP/1.1 response (plain or chunked body).
pub fn parse_response(raw: &[u8]) -> Result<Res, String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("no header end")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split_ascii_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or("no status line")?;
    let chunked = lines.any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    let rest = &raw[split + 4..];
    let body = if chunked {
        dechunk(rest)?
    } else {
        rest.to_vec()
    };
    let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
    Ok(Res { status, body })
}

fn dechunk(mut rest: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let end = rest
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("bad chunk")?;
        let size = usize::from_str_radix(
            String::from_utf8_lossy(&rest[..end])
                .split(';')
                .next()
                .unwrap_or("")
                .trim(),
            16,
        )
        .map_err(|_| "bad chunk size")?;
        rest = &rest[end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if rest.len() < size {
            return Err("short chunk".into());
        }
        out.extend_from_slice(&rest[..size]);
        rest = rest.get(size + 2..).unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_chunked_bodies() {
        let r = parse_response(b"HTTP/1.1 200 OK\r\ncontent-length: 8\r\n\r\n{\"a\":1}\n").unwrap();
        assert_eq!((r.status, r.body["a"].as_i64()), (200, Some(1)));
        let r = parse_response(
            b"HTTP/1.1 403 Forbidden\r\nTransfer-Encoding: chunked\r\n\r\n5\r\n{\"cod\r\nb\r\ne\":\"PIN_X\"}\r\n0\r\n\r\n",
        )
        .unwrap();
        assert_eq!(r.status, 403);
        assert_eq!(r.code(), Some("PIN_X"));
        let r = parse_response(b"HTTP/1.1 204 No Content\r\n\r\n").unwrap();
        assert_eq!((r.status, r.body.clone()), (204, Value::Null));
        assert!(parse_response(b"garbage").is_err());
    }
}
