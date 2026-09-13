//! Peer behaviour, driven over an in-memory duplex so both sides run in one
//! test with no process involved.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use lanok_core::{Capabilities, Negotiation, RpcError, Value, Version, codes};
use lanok_peer::{Hello, Peer, Router};
use lanok_transport::duplex;
use serde_json::json;

fn v(major: u32, minor: u32) -> Version {
    Version::new(major, minor)
}

/// Two connected peers, each with its own handler.
fn pair(a: Router, b: Router) -> (Peer, Peer) {
    let (ta, tb) = duplex();
    (
        Peer::builder().handler(a).connect(ta),
        Peer::builder().handler(b).connect(tb),
    )
}

#[tokio::test]
async fn a_request_gets_its_response() {
    let server = Router::new().on_request("echo", |params| async move { Ok(params) });
    let (client, _server) = pair(Router::new(), server);

    let result = client
        .request("echo", json!({ "text": "hi" }))
        .await
        .unwrap();
    assert_eq!(result["text"], "hi");
}

#[tokio::test]
async fn both_directions_carry_requests_at_once() {
    // The whole point of the symmetric peer: the "server" calls back into the
    // "client" while it is answering the client's own request.
    let (ta, tb) = duplex();

    let client = Peer::builder()
        .handler(
            Router::new().on_request("ui/ask", |_| async move { Ok(json!({ "answer": "blue" })) }),
        )
        .connect(ta);

    let server_peer: Arc<std::sync::OnceLock<Peer>> = Arc::new(std::sync::OnceLock::new());
    let for_handler = server_peer.clone();
    let server = Peer::builder()
        .handler(Router::new().on_request("tool/call", move |_| {
            let peer = for_handler.clone();
            async move {
                // Reverse request, mid-flight, on the same connection.
                let answer = peer
                    .get()
                    .expect("peer is set before any request arrives")
                    .request("ui/ask", json!({ "question": "favourite colour?" }))
                    .await?;
                Ok(json!({ "used": answer["answer"] }))
            }
        }))
        .connect(tb);
    server_peer.set(server).unwrap();

    let result = client.request("tool/call", json!({})).await.unwrap();
    assert_eq!(result["used"], "blue");
}

#[tokio::test]
async fn slow_handlers_do_not_block_other_requests() {
    let server = Router::new()
        .on_request("slow", |_| async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            Ok(json!("slow"))
        })
        .on_request("fast", |_| async move { Ok(json!("fast")) });
    let (client, _server) = pair(Router::new(), server);

    let slow = {
        let c = client.clone();
        tokio::spawn(async move { c.request("slow", Value::Null).await })
    };
    // Issued second, must come back first.
    let fast = client.request("fast", Value::Null).await.unwrap();
    assert_eq!(fast, "fast");
    assert_eq!(slow.await.unwrap().unwrap(), "slow");
}

#[tokio::test]
async fn an_unknown_method_is_an_error_not_a_hang() {
    let (client, _server) = pair(Router::new(), Router::new());
    let error = client.request("nope", Value::Null).await.unwrap_err();
    assert_eq!(error.code, codes::METHOD_NOT_FOUND);
}

#[tokio::test]
async fn a_handler_error_reaches_the_caller_intact() {
    let server = Router::new().on_request("fail", |_| async move {
        Err(RpcError::new(-32050, "upstream is busy").retryable())
    });
    let (client, _server) = pair(Router::new(), server);

    let error = client.request("fail", Value::Null).await.unwrap_err();
    assert_eq!(error.code, -32050);
    assert_eq!(error.message, "upstream is busy");
    assert!(
        error.is_retryable(),
        "the retryable hint must survive the wire"
    );
}

#[tokio::test]
async fn closing_the_connection_fails_every_pending_request_at_once() {
    let server = Router::new().on_request("never", |_| async move {
        // Outlives the connection on purpose.
        tokio::time::sleep(Duration::from_secs(3600)).await;
        Ok(Value::Null)
    });
    let (ta, tb) = duplex();
    let client = Peer::builder().connect(ta);
    let server = Peer::builder().handler(server).connect(tb);

    let waiting = {
        let c = client.clone();
        tokio::spawn(async move { c.request("never", Value::Null).await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    drop(server);

    // Without the drain this would hang until the request's own timeout, which
    // is the bug the drain exists to prevent.
    let error = tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .expect("pending requests must fail as soon as the connection ends")
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, codes::TRANSPORT_CLOSED);
    assert!(client.is_closed());
}

#[tokio::test]
async fn a_request_past_its_timeout_fails_locally() {
    let server = Router::new().on_request("slow", |_| async move {
        tokio::time::sleep(Duration::from_secs(3600)).await;
        Ok(Value::Null)
    });
    let (ta, tb) = duplex();
    let client = Peer::builder()
        .request_timeout(Duration::from_millis(100))
        .connect(ta);
    let _server = Peer::builder().handler(server).connect(tb);

    let error = client.request("slow", Value::Null).await.unwrap_err();
    assert_eq!(error.code, codes::REQUEST_TIMEOUT);
}

#[tokio::test]
async fn abandoning_a_request_cancels_it_on_the_peer() {
    let cancels = Arc::new(AtomicU32::new(0));
    let counted = cancels.clone();
    let server = Router::new()
        .on_request("slow", |_| async move {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            Ok(Value::Null)
        })
        .on_notification("$/cancel", move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
        });

    let (ta, tb) = duplex();
    let client = Peer::builder()
        .cancel_notification("$/cancel")
        .request_timeout(Duration::from_millis(100))
        .connect(ta);
    let _server = Peer::builder().handler(server).connect(tb);

    let _ = client.request("slow", Value::Null).await;

    for _ in 0..100 {
        if cancels.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        cancels.load(Ordering::SeqCst),
        1,
        "a caller that gave up must tell the peer to stop working"
    );
}

#[tokio::test]
async fn the_handshake_records_version_and_capabilities() {
    let (ta, tb) = duplex();
    let client = Peer::builder().connect(ta);
    let _server = Peer::builder()
        .handler(Router::new().on_request("initialize", |_| async move {
            Ok(json!({
                "name": "test-server",
                "protocol_version": "1.2",
                "capabilities": ["streaming", "tools"],
            }))
        }))
        .connect(tb);

    let theirs = client
        .handshake(
            &Hello::new("test-client", v(1, 0)),
            Negotiation::new(v(1, 0)),
        )
        .await
        .unwrap();

    assert_eq!(theirs.name, "test-server");
    assert_eq!(theirs.protocol_version, v(1, 2));
    assert!(client.supports("tools"));
    assert!(!client.supports("ui_ask"));
    assert_eq!(client.peer_info().version, Some(v(1, 2)));
}

#[tokio::test]
async fn an_incompatible_major_is_refused() {
    let (ta, tb) = duplex();
    let client = Peer::builder().connect(ta);
    let _server = Peer::builder()
        .handler(Router::new().on_request("initialize", |_| async move {
            Ok(json!({ "name": "future", "protocol_version": "2.0" }))
        }))
        .connect(tb);

    let error = client
        .handshake(&Hello::new("client", v(1, 0)), Negotiation::new(v(1, 0)))
        .await
        .unwrap_err();
    assert_eq!(error.code, codes::VERSION_INCOMPATIBLE);
    // Nothing was recorded, so a stub cannot be fooled into thinking the peer
    // supports something on a connection that was refused.
    assert_eq!(client.peer_info().capabilities, Capabilities::new());
}

#[tokio::test]
async fn notifications_flow_without_a_response() {
    let seen = Arc::new(AtomicU32::new(0));
    let counted = seen.clone();
    let server = Router::new().on_notification("tick", move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    let (client, _server) = pair(Router::new(), server);

    for _ in 0..3 {
        client.notify("tick", json!({}));
    }
    for _ in 0..100 {
        if seen.load(Ordering::SeqCst) == 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(seen.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn ids_are_per_direction() {
    // Both sides number from 1 independently. If the peer keyed responses by id
    // alone across directions, these would collide.
    let (ta, tb) = duplex();
    let b_peer: Arc<std::sync::OnceLock<Peer>> = Arc::new(std::sync::OnceLock::new());

    let a = Peer::builder()
        .handler(Router::new().on_request("from_b", |_| async move { Ok(json!("a answered")) }))
        .connect(ta);

    // B answers A's request by calling back into A over the same connection,
    // so both id spaces are in use with the same numbers at the same time.
    let for_handler = b_peer.clone();
    let b = Peer::builder()
        .handler(Router::new().on_request("from_a", move |_| {
            let peer = for_handler.clone();
            async move {
                let back = peer.get().unwrap().request("from_b", Value::Null).await?;
                Ok(json!({ "nested": back }))
            }
        }))
        .connect(tb);
    b_peer.set(b.clone()).unwrap();

    let result = a.request("from_a", Value::Null).await.unwrap();
    assert_eq!(result["nested"], "a answered");
    let _ = b;
}
