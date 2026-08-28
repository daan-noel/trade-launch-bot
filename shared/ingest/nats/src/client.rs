//! Minimal NATS **core** client — connect, subscribe, receive.
//!
//! Deliberately hand-rolled rather than pulled from a crate: every NATS client on
//! crates.io hard-depends on `nkeys` for nkey/JWT auth, which brings an
//! ed25519/curve25519-dalek 4.x stack whose `zeroize` requirement cannot co-exist
//! with the `curve25519-dalek 3.2.1` that solana 1.17.27 pins across this
//! workspace. The subset actually needed here is a short line protocol, and
//! owning it also means owning the read loop — which is the part that decides
//! whether the server declares us a slow consumer and hangs up.
//!
//! Supports: plaintext `nats://`, optional user/password or token auth from the
//! URL, multiple seed addresses, `SUB` with an optional queue group, and inline
//! `PING`/`PONG` keepalive. Does **not** support TLS, JetStream, request/reply,
//! or nkey auth — none of which a broadcast relay consumer needs. A server that
//! demands TLS or nkeys is reported as a clear configuration error rather than
//! silently failing to connect.
//!
//! Wire protocol: <https://docs.nats.io/reference/reference-protocols/nats-protocol>

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{
    tcp::{OwnedReadHalf, OwnedWriteHalf},
    TcpStream,
};
use tokio::time::timeout;
use tracing::{debug, info, warn};

/// Read buffer for the socket. Frames observed on the pump.fun curve subject
/// average ~17 KB, so this holds several without a syscall per message.
const READ_BUFFER: usize = 256 * 1024;

/// Hard ceiling on a single frame, independent of what the server advertises —
/// a corrupt length header must not turn into a multi-gigabyte allocation.
const MAX_FRAME: usize = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum NatsError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("connect to {0} timed out")]
    ConnectTimeout(String),
    #[error("server closed the connection")]
    Closed,
    #[error("server error: {0}")]
    Server(String),
    #[error("malformed {frame} frame: {detail}")]
    Protocol { frame: &'static str, detail: String },
    #[error("no usable address in url {0}")]
    NoAddress(String),
    #[error("server requires TLS, which this client does not implement - use a plaintext nats:// endpoint or terminate TLS ahead of it")]
    TlsRequired,
    #[error("server requires nkey/JWT auth, which this client does not implement - use user/password or token auth")]
    NkeyRequired,
}

/// The subset of the server's `INFO` this client acts on.
#[derive(Debug, Default, Clone)]
pub struct ServerInfo {
    pub server_name: String,
    pub version: String,
    pub max_payload: usize,
    pub tls_required: bool,
    pub auth_required: bool,
    pub nkey_required: bool,
}

impl ServerInfo {
    /// Parsed leniently: an unknown or reshaped `INFO` must not stop the client
    /// from connecting, so every field falls back to a benign default.
    fn parse(json: &str) -> Self {
        let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
        Self {
            server_name: v
                .get("server_name")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string(),
            version: v
                .get("version")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string(),
            max_payload: v
                .get("max_payload")
                .and_then(|x| x.as_u64())
                .unwrap_or(1024 * 1024) as usize,
            tls_required: v
                .get("tls_required")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
            auth_required: v
                .get("auth_required")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
            nkey_required: v.get("nonce").is_some(),
        }
    }
}

/// Credentials lifted out of a `nats://user:pass@host` / `nats://token@host` URL.
#[derive(Default)]
struct Credentials {
    user: Option<String>,
    pass: Option<String>,
    token: Option<String>,
}

/// One live connection to a NATS server.
pub struct NatsConn {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    /// Reused across frames so a steady stream costs no header allocations.
    line: Vec<u8>,
    payload: Vec<u8>,
    info: ServerInfo,
}

impl NatsConn {
    /// Connect to the first reachable address in `url`.
    ///
    /// `url` is one or more comma-separated `nats://[creds@]host:port` entries;
    /// the scheme is optional and the port defaults to 4222. Seeds are tried in
    /// order and the last error is reported if all fail.
    pub async fn connect(
        url: &str,
        client_name: &str,
        connect_timeout: Duration,
    ) -> Result<Self, NatsError> {
        let seeds: Vec<&str> = url
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if seeds.is_empty() {
            return Err(NatsError::NoAddress(url.to_string()));
        }

        let mut last: Option<NatsError> = None;
        for seed in seeds {
            let (addr, creds) = parse_url(seed);
            match Self::connect_one(&addr, &creds, client_name, connect_timeout).await {
                Ok(c) => return Ok(c),
                Err(e) => {
                    warn!("NATS: {addr} unavailable - {e}");
                    last = Some(e);
                }
            }
        }
        Err(last.unwrap_or_else(|| NatsError::NoAddress(url.to_string())))
    }

    async fn connect_one(
        addr: &str,
        creds: &Credentials,
        client_name: &str,
        connect_timeout: Duration,
    ) -> Result<Self, NatsError> {
        let stream = match timeout(connect_timeout, TcpStream::connect(addr)).await {
            Ok(r) => r?,
            Err(_) => return Err(NatsError::ConnectTimeout(addr.to_string())),
        };
        // Curve traffic is a stream of small-ish frames where latency matters more
        // than packet efficiency.
        let _ = stream.set_nodelay(true);

        let (rh, writer) = stream.into_split();
        let mut conn = Self {
            reader: BufReader::with_capacity(READ_BUFFER, rh),
            writer,
            line: Vec::with_capacity(256),
            payload: Vec::with_capacity(64 * 1024),
            info: ServerInfo::default(),
        };

        // A healthy nats-server sends INFO immediately on accept. Silence here is
        // the signature of an interception layer (a VPN or proxy that completes
        // the TCP handshake itself), not of a busy server - so time it out rather
        // than hang forever waiting.
        let info_line = match timeout(connect_timeout, conn.read_line()).await {
            Ok(r) => r?,
            Err(_) => return Err(NatsError::ConnectTimeout(addr.to_string())),
        };
        let info_json = info_line
            .strip_prefix("INFO ")
            .ok_or_else(|| NatsError::Protocol {
                frame: "INFO",
                detail: format!("expected INFO, got {:?}", truncate(&info_line)),
            })?;
        conn.info = ServerInfo::parse(info_json);

        if conn.info.tls_required {
            return Err(NatsError::TlsRequired);
        }
        if conn.info.nkey_required && creds.user.is_none() && creds.token.is_none() {
            return Err(NatsError::NkeyRequired);
        }

        // `headers: false` keeps the read path on plain MSG frames - the server
        // strips any headers a publisher set rather than sending HMSG.
        let connect = serde_json::json!({
            "verbose": false,
            "pedantic": false,
            "tls_required": false,
            "name": client_name,
            "lang": "rust",
            "version": env!("CARGO_PKG_VERSION"),
            "protocol": 1,
            "headers": false,
            "no_responders": false,
            "echo": true,
            "user": creds.user,
            "pass": creds.pass,
            "auth_token": creds.token,
        });
        conn.write_all(format!("CONNECT {connect}\r\n").as_bytes())
            .await?;
        // PING/PONG round-trip turns a rejected CONNECT (bad auth) into an error
        // here instead of a silent disconnect on the first read.
        conn.write_all(b"PING\r\n").await?;

        loop {
            let line = conn.read_line().await?;
            if line.starts_with("PONG") {
                break;
            }
            if let Some(err) = line.strip_prefix("-ERR") {
                return Err(NatsError::Server(err.trim().trim_matches('\'').to_string()));
            }
            // +OK (verbose servers) and a re-sent INFO are both fine; keep waiting.
        }

        info!(
            addr,
            server = %conn.info.server_name,
            version = %conn.info.version,
            max_payload = conn.info.max_payload,
            "NATS: handshake complete"
        );
        Ok(conn)
    }

    /// What the server advertised at connect time.
    pub fn info(&self) -> &ServerInfo {
        &self.info
    }

    /// Subscribe `subject` under `sid`, optionally joining a queue group.
    pub async fn subscribe(
        &mut self,
        subject: &str,
        queue: Option<&str>,
        sid: u64,
    ) -> Result<(), NatsError> {
        let cmd = match queue {
            Some(q) => format!("SUB {subject} {q} {sid}\r\n"),
            None => format!("SUB {subject} {sid}\r\n"),
        };
        self.write_all(cmd.as_bytes()).await
    }

    /// Read until the next application message, answering `PING` inline.
    ///
    /// Returns the message payload. Control frames (`PING`, `PONG`, `+OK`, a
    /// re-sent `INFO`) are handled here and never surface to the caller;
    /// `-ERR` becomes [`NatsError::Server`].
    ///
    /// Cancel-safe only at whole-frame granularity: dropping the future
    /// mid-frame desynchronises the stream, so callers must select on it and
    /// then reconnect rather than resume.
    pub async fn next_message(&mut self) -> Result<Vec<u8>, NatsError> {
        loop {
            let line = self.read_line().await?;

            if let Some(rest) = line.strip_prefix("MSG ") {
                let len = parse_msg_len(rest)?;
                if len > MAX_FRAME {
                    return Err(NatsError::Protocol {
                        frame: "MSG",
                        detail: format!("payload {len} exceeds the {MAX_FRAME} byte ceiling"),
                    });
                }
                // +2 consumes the trailing CRLF in the same read.
                self.payload.clear();
                self.payload.resize(len + 2, 0);
                self.reader.read_exact(&mut self.payload).await?;
                self.payload.truncate(len);
                return Ok(std::mem::take(&mut self.payload));
            }

            if line.starts_with("PING") {
                self.write_all(b"PONG\r\n").await?;
                continue;
            }
            if line.starts_with("PONG") || line.starts_with("+OK") {
                continue;
            }
            if let Some(rest) = line.strip_prefix("INFO ") {
                // Servers re-send INFO on cluster topology changes.
                self.info = ServerInfo::parse(rest);
                continue;
            }
            if let Some(err) = line.strip_prefix("-ERR") {
                return Err(NatsError::Server(err.trim().trim_matches('\'').to_string()));
            }
            if line.is_empty() {
                continue;
            }
            debug!("NATS: ignoring unknown frame {:?}", truncate(&line));
        }
    }

    async fn write_all(&mut self, bytes: &[u8]) -> Result<(), NatsError> {
        self.writer.write_all(bytes).await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// One protocol line, CRLF stripped. `Closed` on clean EOF.
    async fn read_line(&mut self) -> Result<String, NatsError> {
        self.line.clear();
        let n = self.reader.read_until(b'\n', &mut self.line).await?;
        if n == 0 {
            return Err(NatsError::Closed);
        }
        while matches!(self.line.last(), Some(b'\n' | b'\r')) {
            self.line.pop();
        }
        Ok(String::from_utf8_lossy(&self.line).into_owned())
    }
}

/// Byte count from a `MSG <subject> <sid> [reply-to] <#bytes>` header.
fn parse_msg_len(rest: &str) -> Result<usize, NatsError> {
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // subject + sid + size, with an optional reply-to before the size.
    if !(3..=4).contains(&fields.len()) {
        return Err(NatsError::Protocol {
            frame: "MSG",
            detail: format!("expected 3 or 4 fields, got {}", fields.len()),
        });
    }
    fields
        .last()
        .and_then(|s| s.parse::<usize>().ok())
        .ok_or_else(|| NatsError::Protocol {
            frame: "MSG",
            detail: format!("unparseable byte count in {:?}", truncate(rest)),
        })
}

/// Split `nats://user:pass@host:port` into a dialable `host:port` plus creds.
fn parse_url(url: &str) -> (String, Credentials) {
    let rest = url
        .strip_prefix("nats://")
        .or_else(|| url.strip_prefix("tls://"))
        .unwrap_or(url);

    let (creds, host) = match rest.rsplit_once('@') {
        Some((auth, host)) => {
            let c = match auth.split_once(':') {
                Some((u, p)) => Credentials {
                    user: Some(u.to_string()),
                    pass: Some(p.to_string()),
                    token: None,
                },
                // A bare value before `@` is a token, not a username.
                None => Credentials {
                    token: Some(auth.to_string()),
                    ..Default::default()
                },
            };
            (c, host)
        }
        None => (Credentials::default(), rest),
    };

    let host = host.trim_end_matches('/');
    let addr = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:4222")
    };
    (addr, creds)
}

fn truncate(s: &str) -> &str {
    let n = s.len().min(120);
    // Never split a UTF-8 code point when trimming for a log line.
    let mut end = n;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msg_header_accepts_both_arities() {
        assert_eq!(parse_msg_len("subject 1 512").unwrap(), 512);
        assert_eq!(
            parse_msg_len("helius.raw.bondingcurve 7 17004").unwrap(),
            17004
        );
        // With a reply-to subject.
        assert_eq!(parse_msg_len("subject 1 _INBOX.x 64").unwrap(), 64);
    }

    #[test]
    fn a_malformed_msg_header_is_rejected() {
        assert!(parse_msg_len("subject").is_err());
        assert!(parse_msg_len("subject 1 notanumber").is_err());
        assert!(parse_msg_len("a b c d e f").is_err());
    }

    #[test]
    fn urls_split_into_address_and_credentials() {
        let (a, c) = parse_url("nats://3.78.182.30:4222");
        assert_eq!(a, "3.78.182.30:4222");
        assert!(c.user.is_none() && c.token.is_none());

        // Port defaults to the NATS standard.
        let (a, _) = parse_url("relay.internal");
        assert_eq!(a, "relay.internal:4222");

        let (a, c) = parse_url("nats://alice:s3cret@host:4222");
        assert_eq!(a, "host:4222");
        assert_eq!(c.user.as_deref(), Some("alice"));
        assert_eq!(c.pass.as_deref(), Some("s3cret"));

        let (a, c) = parse_url("nats://sometoken@host:4222");
        assert_eq!(a, "host:4222");
        assert_eq!(c.token.as_deref(), Some("sometoken"));
        assert!(c.user.is_none());
    }

    /// The real INFO captured from the relay on 2026-08-25.
    #[test]
    fn server_info_parses_the_observed_relay() {
        let info = ServerInfo::parse(
            r#"{"server_id":"ND","server_name":"n1","version":"2.14.2","proto":1,
                "max_payload":1048576,"headers":true,"cluster":"my_cluster"}"#,
        );
        assert_eq!(info.version, "2.14.2");
        assert_eq!(info.max_payload, 1_048_576);
        assert!(!info.tls_required);
        assert!(!info.auth_required);
        assert!(!info.nkey_required);
    }

    #[test]
    fn server_info_survives_unexpected_shapes() {
        let info = ServerInfo::parse("not json at all");
        assert_eq!(info.max_payload, 1024 * 1024);
        assert!(!info.tls_required);
    }

    #[test]
    fn tls_and_nkey_requirements_are_detected() {
        assert!(ServerInfo::parse(r#"{"tls_required":true}"#).tls_required);
        assert!(ServerInfo::parse(r#"{"nonce":"abc"}"#).nkey_required);
    }
}
