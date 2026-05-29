//! Beacon client -- thin WebSocket adapter over the v0 beacon protocol.
//!
//! Deprecated: scheduled for removal in 3.x; v3 multi-user is via git or any
//! filesystem-mirroring transport.

#![allow(deprecated)]
//!
//! Behind the `beacon` feature flag. Brings in `tokio` +
//! `tokio-tungstenite`; when the feature is off the rest of
//! `nomograph-claim` stays async-free so single-host CLI workloads don't
//! pay the runtime cost.
//!
//! Protocol (see `nomograph/beacon/src/types.ts`):
//!
//! ```text
//!   GET wss://<host>/project/<project_slug>
//!     Authorization: Bearer <forge_token>
//!
//!   server -> client: {"kind":"hello","project":"...","session_id":"...","peer_count":N}
//!   client <-> client (via relay):
//!       {"kind":"change","nonce":"<b64>","ciphertext":"<b64>",
//!        "aad":{"project":"...","sender":"...","ts":<ms>}}
//! ```
//!
//! Ciphertext is opaque to the beacon; the relay only validates envelope
//! shape and AAD scope. This client encrypts with
//! [`crate::crypto::encrypt`] and decrypts with [`crate::crypto::decrypt`]
//! using a key derived via [`crate::crypto::derive_key`].
//!
//! # Scope
//!
//! v0 ships: connect, wait-for-hello, send change, receive change. No
//! auto-reconnect, no backpressure, no directive frames. Enough for the
//! Andrew+Josh multi-user test on Wednesday; productionization tracked
//! separately.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use futures_util::{SinkExt, StreamExt};
use http::{HeaderValue, Request};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::crypto::{Key, Nonce, decrypt, encrypt};
use crate::error::{Error, Result};

/// AAD attached to every change envelope. Must match the beacon's
/// `ChangeAad` shape bit-for-bit; beacon validates `project` against the
/// DO's scope and rejects mismatches.
#[deprecated(
    since = "3.0.0-pre.1",
    note = "scheduled for removal in 3.x; v3 multi-user is via git or any filesystem-mirroring transport."
)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeAad {
    pub project: String,
    pub sender: String,
    pub ts: i64,
}

impl ChangeAad {
    /// Serialize AAD to the exact JSON bytes the beacon will use for
    /// relay matching. Those same bytes must be authenticated by the
    /// AEAD on both ends, so we compute a canonical form here.
    fn canonical_bytes(&self) -> Vec<u8> {
        // Field order matches the TypeScript frame validator
        // (project, sender, ts). Keep it stable.
        let mut buf = Vec::with_capacity(128);
        buf.extend_from_slice(b"{\"project\":");
        buf.extend_from_slice(serde_json::to_string(&self.project).unwrap().as_bytes());
        buf.extend_from_slice(b",\"sender\":");
        buf.extend_from_slice(serde_json::to_string(&self.sender).unwrap().as_bytes());
        buf.extend_from_slice(b",\"ts\":");
        buf.extend_from_slice(self.ts.to_string().as_bytes());
        buf.push(b'}');
        buf
    }
}

/// Server -> client greeting sent immediately after the WS upgrade.
#[deprecated(
    since = "3.0.0-pre.1",
    note = "scheduled for removal in 3.x; v3 multi-user is via git or any filesystem-mirroring transport."
)]
#[derive(Debug, Clone, Deserialize)]
pub struct Hello {
    pub project: String,
    pub session_id: String,
    pub peer_count: u32,
}

/// Client -> beacon -> peers. We serialize this exactly; the beacon
/// re-serializes on relay but validates shape first, so unknown fields
/// don't survive.
#[derive(Debug, Clone, Serialize)]
struct ChangeOut<'a> {
    kind: &'static str,
    nonce: &'a str,
    ciphertext: &'a str,
    aad: ChangeAad,
}

/// Frame we expect to receive from the relay. `untagged` on the enum
/// keeps parsing tolerant: an unknown `kind` falls through to `Unknown`
/// so we can keep the socket alive and just skip the frame.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
enum FrameIn {
    #[serde(rename = "hello")]
    Hello {
        project: String,
        session_id: String,
        peer_count: u32,
    },
    #[serde(rename = "change")]
    Change {
        nonce: String,
        ciphertext: String,
        aad: ChangeAad,
    },
    #[serde(rename = "directive")]
    #[allow(dead_code)] // reserved; v0 ignores directives
    Directive {
        nonce: String,
        ciphertext: String,
        #[serde(default)]
        aad: serde_json::Value,
    },
    #[serde(rename = "error")]
    Error { reason: String },
    #[serde(other)]
    Unknown,
}

/// Connected beacon client.
///
/// Build with [`BeaconClient::connect`]; exchange change frames via
/// [`BeaconClient::send_change`] and [`BeaconClient::recv_change`];
/// close the underlying WS with [`BeaconClient::close`]. The struct
/// cannot be cloned because it owns the WS; multiplex via mpsc if
/// multiple tasks need to send.
#[deprecated(
    since = "3.0.0-pre.1",
    note = "scheduled for removal in 3.x; v3 multi-user is via git or any filesystem-mirroring transport."
)]
pub struct BeaconClient {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    project: String,
    sender: String,
    hello: Hello,
}

impl BeaconClient {
    /// Connect to `wss://<host>/project/<project>` with `Authorization:
    /// Bearer <forge_token>` and wait for the `hello` frame.
    ///
    /// `sender` becomes the `aad.sender` field on every change we emit;
    /// beacon uses it for attribution/logging, not auth. Pass the same
    /// asserter string you'd use in [`crate::Claim::new`]
    /// (e.g. `"user:gitlab:andunn"`).
    pub async fn connect(
        beacon_url: &str,
        forge_token: &str,
        project: &str,
        sender: &str,
    ) -> Result<Self> {
        let mut url: Url = beacon_url.parse().map_err(|e| {
            Error::Other(format!(
                "invalid beacon URL `{beacon_url}` ({e}); expected wss://host[:port]"
            ))
        })?;
        // Append `/project/<project>` while handling existing path.
        if !url.path().ends_with('/') {
            let path = format!("{}/", url.path());
            url.set_path(&path);
        }
        url.set_path(&format!(
            "{}project/{}",
            url.path(),
            urlencoding_minimal(project)
        ));

        let mut req = Request::builder()
            .method("GET")
            .uri(url.as_str())
            .header("Host", url.host_str().unwrap_or(""))
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_ws_key())
            .body(())
            .map_err(|e| Error::Other(format!("build request: {e}")))?;
        req.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {forge_token}")).map_err(|e| {
                Error::Other(format!(
                    "invalid forge token as header ({e}); strip newlines"
                ))
            })?,
        );

        let (mut ws, _resp) = tokio_tungstenite::connect_async(req)
            .await
            .map_err(|e| Error::Other(format!("beacon connect failed: {e}")))?;

        // Wait for the hello frame. If the beacon sends anything else
        // first (error or unexpected), surface it.
        let hello = loop {
            let Some(msg) = ws.next().await else {
                return Err(Error::Other(
                    "beacon closed before sending hello; check token and project access".into(),
                ));
            };
            match msg.map_err(|e| Error::Other(format!("ws recv: {e}")))? {
                Message::Text(t) => match serde_json::from_str::<FrameIn>(&t) {
                    Ok(FrameIn::Hello {
                        project: p,
                        session_id,
                        peer_count,
                    }) => {
                        break Hello {
                            project: p,
                            session_id,
                            peer_count,
                        };
                    }
                    Ok(FrameIn::Error { reason }) => {
                        return Err(Error::Other(format!("beacon error frame: {reason}")));
                    }
                    Ok(_) => continue,
                    Err(e) => {
                        return Err(Error::Other(format!(
                            "beacon sent malformed JSON ({e}): {t}"
                        )));
                    }
                },
                Message::Ping(p) => {
                    let _ = ws.send(Message::Pong(p)).await;
                }
                Message::Close(_) => {
                    return Err(Error::Other("beacon closed during handshake".into()));
                }
                _ => continue,
            }
        };

        Ok(Self {
            ws,
            project: project.to_string(),
            sender: sender.to_string(),
            hello,
        })
    }

    /// Observed hello frame captured at connect time.
    pub fn hello(&self) -> &Hello {
        &self.hello
    }

    /// Encrypt `change_bytes` (typically the output of Automerge
    /// `save_incremental` or a single `changes/<hash>.amc` file's bytes)
    /// and send it to the beacon for relay.
    ///
    /// AAD carries `project`, `sender`, and a millisecond Unix timestamp.
    /// The AEAD tag binds the AAD to the ciphertext, so peers reject
    /// frames whose AAD was rewritten in flight.
    pub async fn send_change(&mut self, key: &Key, change_bytes: &[u8]) -> Result<()> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as i64;
        let aad = ChangeAad {
            project: self.project.clone(),
            sender: self.sender.clone(),
            ts,
        };
        let aad_bytes = aad.canonical_bytes();
        let (nonce, ct) = encrypt(key, change_bytes, &aad_bytes)?;
        let nonce_b64 = B64.encode(nonce);
        let ct_b64 = B64.encode(&ct);
        let frame = ChangeOut {
            kind: "change",
            nonce: &nonce_b64,
            ciphertext: &ct_b64,
            aad,
        };
        let text =
            serde_json::to_string(&frame).map_err(|e| Error::Other(format!("serialize: {e}")))?;
        self.ws
            .send(Message::Text(text))
            .await
            .map_err(|e| Error::Other(format!("ws send: {e}")))?;
        Ok(())
    }

    /// Await the next peer change and return the decrypted bytes.
    ///
    /// Returns `Ok(None)` when the socket closes cleanly.
    /// Non-change frames (directives, errors, unknown kinds) are
    /// skipped silently EXCEPT `error`: those bubble up so callers can
    /// log and decide whether to reconnect.
    pub async fn recv_change(&mut self, key: &Key) -> Result<Option<Vec<u8>>> {
        while let Some(msg) = self.ws.next().await {
            match msg.map_err(|e| Error::Other(format!("ws recv: {e}")))? {
                Message::Text(t) => {
                    let frame: FrameIn = serde_json::from_str(&t)
                        .map_err(|e| Error::Other(format!("parse frame: {e}")))?;
                    match frame {
                        FrameIn::Change {
                            nonce,
                            ciphertext,
                            aad,
                        } => {
                            // Beacon already checks aad.project == DO scope.
                            // Repeat the check so a rogue peer who slipped
                            // through can't spoof scope on our end.
                            if aad.project != self.project {
                                continue;
                            }
                            let nonce_bytes = B64
                                .decode(nonce.as_bytes())
                                .map_err(|e| Error::Other(format!("decode nonce b64: {e}")))?;
                            if nonce_bytes.len() != 12 {
                                return Err(Error::Crypto(format!(
                                    "peer frame nonce is {} bytes, need 12",
                                    nonce_bytes.len()
                                )));
                            }
                            let mut nonce: Nonce = [0u8; 12];
                            nonce.copy_from_slice(&nonce_bytes);
                            let ct = B64
                                .decode(ciphertext.as_bytes())
                                .map_err(|e| Error::Other(format!("decode ct b64: {e}")))?;
                            let aad_bytes = aad.canonical_bytes();
                            let pt = decrypt(key, &nonce, &ct, &aad_bytes)?;
                            return Ok(Some(pt));
                        }
                        FrameIn::Error { reason } => {
                            return Err(Error::Other(format!("beacon error: {reason}")));
                        }
                        FrameIn::Hello { .. } | FrameIn::Directive { .. } | FrameIn::Unknown => {
                            continue;
                        }
                    }
                }
                Message::Ping(p) => {
                    let _ = self.ws.send(Message::Pong(p)).await;
                }
                Message::Pong(_) => continue,
                Message::Close(_) => return Ok(None),
                _ => continue,
            }
        }
        Ok(None)
    }

    /// Send a close frame and await the WS shutdown.
    pub async fn close(mut self) -> Result<()> {
        self.ws
            .close(None)
            .await
            .map_err(|e| Error::Other(format!("ws close: {e}")))?;
        Ok(())
    }
}

/// Minimal URL path segment encoder. tokio-tungstenite ships no encoder
/// and we want to avoid the `urlencoding` crate for a single function.
/// Percent-encodes everything that isn't `A-Za-z0-9-._~`.
fn urlencoding_minimal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// Generate a random 16-byte base64 Sec-WebSocket-Key.
fn generate_ws_key() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut buf);
    B64.encode(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aad_canonical_bytes_is_stable() {
        let a = ChangeAad {
            project: "nomograph/keaton".into(),
            sender: "user:gitlab:andunn".into(),
            ts: 1_776_000_000_000,
        };
        assert_eq!(
            std::str::from_utf8(&a.canonical_bytes()).unwrap(),
            r#"{"project":"nomograph/keaton","sender":"user:gitlab:andunn","ts":1776000000000}"#
        );
    }

    #[test]
    fn urlencoding_minimal_handles_slash_and_colon() {
        assert_eq!(
            urlencoding_minimal("nomograph/keaton"),
            "nomograph%2Fkeaton"
        );
        assert_eq!(urlencoding_minimal("a b"), "a%20b");
        assert_eq!(urlencoding_minimal("abc"), "abc");
    }
}
