//! The echo server, in whichever flavour the caller asks for.
//!
//! Default is the blocking [`SimpleServer`]: no async runtime, one request at a
//! time. That is what an extension author writing a small server should reach
//! for, and running it in CI keeps the claim honest.
//!
//! `--async` runs the same protocol on a [`Peer`], which additionally issues
//! the reverse `ui/ask` request. The handlers are the same shape either way.

use echo_protocol::*;
use lanok::{Hello, Peer, RpcError, SimpleServer, StdioTransport};

fn main() -> std::io::Result<()> {
    if std::env::args().any(|arg| arg == "--async") {
        return serve_async();
    }
    serve_blocking()
}

/// The serial flavour. Progress notifications still stream, because a caller
/// watching a long request needs to see something.
fn serve_blocking() -> std::io::Result<()> {
    SimpleServer::new(PROTOCOL_NAME, PROTOCOL_VERSION)
        .on_request_with(method::ECHO, |context, params| {
            let params: EchoParams = serde_json::from_value(params)
                .map_err(|e| RpcError::invalid_params(format!("malformed echo params: {e}")))?;

            for step in 1..=3 {
                context.notify(
                    method::PROGRESS,
                    serde_json::json!({ "step": step, "of": 3 }),
                );
            }

            let text = if params.shout {
                params.text.to_uppercase()
            } else {
                params.text
            };
            serde_json::to_value(EchoResult { text })
                .map_err(|e| RpcError::internal(format!("result is not serializable: {e}")))
        })
        .on_request(method::PING, |_| Ok(serde_json::Value::Null))
        .serve_stdio()
}

/// The concurrent flavour, which can also call back into the initiator.
fn serve_async() -> std::io::Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let peer_slot = std::sync::Arc::new(std::sync::OnceLock::<Peer>::new());
            let handler = Server {
                peer: peer_slot.clone(),
            };

            let peer = Peer::builder()
                .handler(ResponderDispatch::new(handler))
                .serve_handshake(
                    Hello::new(PROTOCOL_NAME, PROTOCOL_VERSION).capability(capability::UI_ASK),
                )
                .connect(StdioTransport::new());
            peer_slot.set(peer.clone()).expect("set once");

            // Serve until the initiator closes stdin.
            peer.closed().await;
            Ok(())
        })
}

struct Server {
    peer: std::sync::Arc<std::sync::OnceLock<Peer>>,
}

#[lanok::async_trait]
impl ResponderHandler for Server {
    async fn echo(&self, params: EchoParams) -> Result<EchoResult, RpcError> {
        let peer = self.peer.get().expect("peer installed before serving");

        for step in 1..=3 {
            peer.progress(ProgressParams { step, of: 3 });
        }

        // The reverse request. The generated stub refuses locally when the
        // caller never advertised `ui_ask`, so no round trip is wasted.
        let shout = if peer.supports(capability::UI_ASK) {
            peer.ui_ask(AskParams {
                question: "shout?".into(),
            })
            .await?
            .answer
                == "yes"
        } else {
            params.shout
        };

        Ok(EchoResult {
            text: if shout {
                params.text.to_uppercase()
            } else {
                params.text
            },
        })
    }

    async fn ping(&self) -> Result<(), RpcError> {
        Ok(())
    }
}
