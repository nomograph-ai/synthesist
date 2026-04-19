//! Integration test: BeaconClient talks to a mock beacon server and
//! round-trips an encrypted change frame.
//!
//! The mock server is a tokio-tungstenite listener that mimics the v0
//! beacon protocol just enough to exercise the client:
//!   1. Accept a WS upgrade, require `Authorization: Bearer <token>`.
//!   2. Send `hello`.
//!   3. For the first `change` frame received, echo it back to the same
//!      socket. That's equivalent to "relay received our frame to a
//!      peer" from the client's POV.
//!
//! Not a substitute for the real CF Workers beacon (which validates
//! forge tokens against GitLab), but it pins the on-the-wire framing +
//! AEAD contract.

#![cfg(feature = "beacon")]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use nomograph_claim::beacon_client::BeaconClient;
use nomograph_claim::crypto::derive_key;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

async fn spawn_mock(
    expect_token: &'static str,
    ready: oneshot::Sender<SocketAddr>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
        let addr = listener.local_addr().expect("local_addr");
        ready.send(addr).unwrap();

        let (stream, _peer) = listener.accept().await.expect("accept");
        let bearer_seen = Arc::new(std::sync::Mutex::new(false));
        let bearer_seen_cb = bearer_seen.clone();

        // The callback signature is fixed by tokio-tungstenite's
        // accept_hdr_async: `FnOnce(&Request, Response) -> Result<Response, ErrorResponse>`.
        // The Response type is large (~136 bytes); we can't shrink it.
        #[allow(clippy::result_large_err)]
        let ws = accept_hdr_async(stream, move |req: &Request, resp: Response| {
            let got_auth = req
                .headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if got_auth == format!("Bearer {}", expect_token) {
                *bearer_seen_cb.lock().unwrap() = true;
            }
            Ok(resp)
        })
        .await
        .expect("ws accept");

        let (mut tx, mut rx) = ws.split();

        // Send hello.
        let hello = json!({
            "kind": "hello",
            "project": "nomograph/claim",
            "session_id": "mock-do-id",
            "peer_count": 0,
        });
        tx.send(Message::Text(hello.to_string()))
            .await
            .expect("send hello");

        // Echo the first change frame back to the same socket so our
        // client's recv_change loop sees it.
        while let Some(msg) = rx.next().await {
            match msg {
                Ok(Message::Text(t)) => {
                    let parsed: serde_json::Value =
                        serde_json::from_str(&t).expect("valid JSON from client");
                    if parsed.get("kind").and_then(|v| v.as_str()) == Some("change") {
                        tx.send(Message::Text(t)).await.expect("echo");
                        break;
                    }
                }
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }

        // Give the client time to pull the echo before we drop the socket.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = tx.send(Message::Close(None)).await;

        assert!(
            *bearer_seen.lock().unwrap(),
            "mock did not observe the Bearer token"
        );
    })
}

#[tokio::test(flavor = "current_thread")]
async fn connect_send_recv_round_trip() {
    let (tx, rx) = oneshot::channel();
    let _mock = spawn_mock("fake-forge-token", tx).await;
    let addr = rx.await.unwrap();
    let url = format!("ws://{}", addr);

    let mut client = BeaconClient::connect(
        &url,
        "fake-forge-token",
        "nomograph/claim",
        "user:gitlab:andunn",
    )
    .await
    .expect("connect");

    assert_eq!(client.hello().project, "nomograph/claim");
    assert_eq!(client.hello().peer_count, 0);

    let key = derive_key(b"shared-passphrase", "nomograph/claim").expect("derive_key");
    let payload = b"automerge_save_incremental_bytes_here";
    client
        .send_change(&key, payload)
        .await
        .expect("send_change");

    // Echo-as-peer: recv should decrypt what we just sent.
    let recovered = client
        .recv_change(&key)
        .await
        .expect("recv_change")
        .expect("peer frame");
    assert_eq!(recovered, payload);

    client.close().await.expect("close");
}
