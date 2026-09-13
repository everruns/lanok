//! Methods either side may send.
//!
//! MCP's `ping`, `notifications/cancelled` and `notifications/progress` have no
//! direction: either peer may send them. Declaring one `either` costs the
//! compile-time role gating that makes a wrong-direction call an error, which
//! is why it is not the default.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use lanok::prelude::*;
use lanok::{Direction, duplex};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PingParams {}
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PongResult {
    pub ok: bool,
}
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProgressParams {
    pub step: u32,
}
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunParams {
    pub what: String,
}
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunResult {
    pub done: bool,
}
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AskParams {
    pub question: String,
}
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AskResult {
    pub answer: String,
}

lanok::protocol! {
    name    = "symmetric";
    version = "1.0";

    initiator fn run(RunParams) -> RunResult;
    responder fn "ui/ask" ui_ask(AskParams) -> AskResult;

    /// Either side may ping the other.
    either fn ping(PingParams) -> PongResult;
    /// And either side may report progress.
    either notify "notifications/progress" progress(ProgressParams);
}

#[test]
fn either_appears_in_both_directions_of_the_vocabulary() {
    assert_eq!(META.method("ping").unwrap().direction, Direction::Either);

    // Either side may send it, so it counts for both...
    assert_eq!(META.sent_by(Direction::Initiator).count(), 3);
    assert_eq!(META.sent_by(Direction::Responder).count(), 3);
    // ...and for neither when asking what was literally declared one-way.
    assert_eq!(META.declared_by(Direction::Initiator).count(), 1);
    assert_eq!(META.declared_by(Direction::Responder).count(), 1);
    assert_eq!(META.declared_by(Direction::Either).count(), 2);
}

#[test]
fn either_methods_reach_the_schema_artifacts() {
    let schema = schema_document().schema();
    assert!(schema["messages"]["ping"]["params"].is_object());
    assert!(schema["messages"]["notifications/progress"]["params"].is_object());

    let meta = serde_json::to_value(META).unwrap();
    let ping = meta["methods"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "ping")
        .unwrap();
    assert_eq!(ping["direction"], "either");
}

struct Both {
    answered: Arc<AtomicU32>,
    progress_seen: Arc<AtomicU32>,
}

// An `either` method arrives from both sides, so it is on both handler traits
// and both dispatchers route it.
#[lanok::async_trait]
impl ResponderHandler for Both {
    async fn ping(&self, _params: PingParams) -> Result<PongResult, RpcError> {
        self.answered.fetch_add(1, Ordering::SeqCst);
        Ok(PongResult { ok: true })
    }
    fn progress(&self, _params: ProgressParams) {
        self.progress_seen.fetch_add(1, Ordering::SeqCst);
    }
}

#[lanok::async_trait]
impl InitiatorHandler for Both {
    async fn ping(&self, _params: PingParams) -> Result<PongResult, RpcError> {
        self.answered.fetch_add(1, Ordering::SeqCst);
        Ok(PongResult { ok: true })
    }
    fn progress(&self, _params: ProgressParams) {
        self.progress_seen.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn either_is_callable_and_answerable_from_both_ends() {
    let answered = Arc::new(AtomicU32::new(0));
    let progress_seen = Arc::new(AtomicU32::new(0));
    let (ta, tb) = duplex();

    let initiator = Peer::builder()
        .handler(InitiatorDispatch::new(Both {
            answered: answered.clone(),
            progress_seen: progress_seen.clone(),
        }))
        .connect(ta);
    let responder = Peer::builder()
        .handler(ResponderDispatch::new(Both {
            answered: answered.clone(),
            progress_seen: progress_seen.clone(),
        }))
        .connect(tb);

    // One shared stub, called from both ends of the same connection.
    assert!(SharedApi::ping(&initiator, PingParams {}).await.unwrap().ok);
    assert!(SharedApi::ping(&responder, PingParams {}).await.unwrap().ok);
    assert_eq!(answered.load(Ordering::SeqCst), 2);

    SharedApi::progress(&initiator, ProgressParams { step: 1 });
    SharedApi::progress(&responder, ProgressParams { step: 2 });
    for _ in 0..100 {
        if progress_seen.load(Ordering::SeqCst) == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(progress_seen.load(Ordering::SeqCst), 2);
}
