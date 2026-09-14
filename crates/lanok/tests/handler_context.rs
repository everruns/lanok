//! A handler gets the request it is answering, not just its params.
//!
//! Before [`Context`], a handler saw a method name and a payload: everything
//! about the request except which request it was. A protocol that streams
//! progress has to name the request the progress belongs to, and one whose
//! `cancel` aborts an in-flight call has to find that call by id. Both were
//! reasons to abandon the generated dispatch and write a serve loop by hand,
//! which is how a protocol ends up maintaining its own decode and encode.

use std::sync::{Arc, Mutex};

use lanok::duplex;
use lanok::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkParams {
    pub text: String,
}
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkResult {
    /// The id the handler saw, echoed back so the test can compare the two
    /// routes the id travelled.
    pub answered_for: String,
}
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProgressParams {
    pub request: String,
    pub streamed: bool,
}
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NoteParams {
    pub text: String,
}

lanok::protocol! {
    name    = "ctx";
    version = "1.0";
    min     = "1.0";

    /// Answered by the responder, which reports progress as it goes.
    initiator fn work(WorkParams) -> WorkResult;

    /// Progress, correlated to the request that produced it.
    responder notify "work/progress" progress(ProgressParams);

    /// A notification inbound to the responder, which has no request behind it.
    initiator notify note(NoteParams);

    capabilities {
        /// The caller wants progress notifications.
        streaming,
    }
}

#[derive(Default)]
struct Server {
    /// What `cx.supports` said inside the handler.
    saw_streaming: Arc<Mutex<Option<bool>>>,
    /// Whether the `note` handler was given an id. It must not be.
    note_had_id: Arc<Mutex<Option<bool>>>,
}

#[lanok::async_trait]
impl ResponderHandler for Server {
    async fn work(&self, cx: Context, params: WorkParams) -> Result<WorkResult, RpcError> {
        let id = cx.id().expect("a request has an id").to_string();
        *self.saw_streaming.lock().unwrap() = Some(cx.supports("streaming"));

        // Emitted while the request is still open, and named after it. This is
        // the whole point: the caller can attribute it without guessing.
        cx.notify(method::PROGRESS, json!({ "request": id, "streamed": true }));

        let _ = params;
        Ok(WorkResult { answered_for: id })
    }

    fn note(&self, cx: Context, _params: NoteParams) {
        *self.note_had_id.lock().unwrap() = Some(cx.id().is_some());
    }
}

#[derive(Default)]
struct Client {
    progress: Arc<Mutex<Vec<ProgressParams>>>,
}

#[lanok::async_trait]
impl InitiatorHandler for Client {
    fn progress(&self, _cx: Context, params: ProgressParams) {
        self.progress.lock().unwrap().push(params);
    }
}

#[tokio::test]
async fn the_handler_knows_which_request_it_is_answering() {
    let progress = Arc::new(Mutex::new(Vec::new()));
    let saw_streaming = Arc::new(Mutex::new(None));
    let note_had_id = Arc::new(Mutex::new(None));

    let (ta, tb) = duplex();
    let ours = Hello::new("test-client", PROTOCOL_VERSION).capability(capability::STREAMING);
    let initiator = Peer::builder()
        .handler(InitiatorDispatch::new(Client {
            progress: progress.clone(),
        }))
        .serve_handshake(ours.clone())
        .connect(ta);
    let _responder = Peer::builder()
        .handler(ResponderDispatch::new(Server {
            saw_streaming: saw_streaming.clone(),
            note_had_id: note_had_id.clone(),
        }))
        .serve_handshake(Hello::new(PROTOCOL_NAME, PROTOCOL_VERSION))
        .connect(tb);
    initiator.handshake(&ours, NEGOTIATION).await.unwrap();

    let result = initiator
        .work(WorkParams { text: "hi".into() })
        .await
        .unwrap();

    // The id the handler saw is the id of the call that is now returning, so a
    // caller can join the progress it already received to this result.
    {
        let streamed = progress.lock().unwrap();
        assert_eq!(streamed.len(), 1, "one progress notification arrived");
        assert!(streamed[0].streamed);
        assert_eq!(streamed[0].request, result.answered_for);
    }
    assert!(
        !result.answered_for.is_empty(),
        "the handler saw a real id, not a placeholder"
    );

    // The handshake reached the context, so a handler decides whether to stream
    // the same way the generated stubs decide whether to send.
    assert_eq!(*saw_streaming.lock().unwrap(), Some(true));

    // A second call, to prove the id is the request's and not some constant
    // that happened to match: two calls, two ids, each progress line carrying
    // its own.
    let again = initiator
        .work(WorkParams { text: "hi".into() })
        .await
        .unwrap();
    assert_ne!(again.answered_for, result.answered_for);
    let streamed = progress.lock().unwrap();
    assert_eq!(streamed.len(), 2);
    assert_eq!(streamed[1].request, again.answered_for);
}

#[tokio::test]
async fn a_notification_handler_has_no_request_to_name() {
    let note_had_id = Arc::new(Mutex::new(None));

    let (ta, tb) = duplex();
    let ours = Hello::new("test-client", PROTOCOL_VERSION);
    let initiator = Peer::builder()
        .handler(InitiatorDispatch::new(Client::default()))
        .serve_handshake(ours.clone())
        .connect(ta);
    let _responder = Peer::builder()
        .handler(ResponderDispatch::new(Server {
            note_had_id: note_had_id.clone(),
            ..Server::default()
        }))
        .serve_handshake(Hello::new(PROTOCOL_NAME, PROTOCOL_VERSION))
        .connect(tb);
    initiator.handshake(&ours, NEGOTIATION).await.unwrap();

    initiator.note(NoteParams { text: "fyi".into() });

    // Nothing to await: a notification has no reply. Poll until the responder
    // has seen it rather than sleeping for a fixed time.
    for _ in 0..100 {
        if note_had_id.lock().unwrap().is_some() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        *note_had_id.lock().unwrap(),
        Some(false),
        "a notification is not a request, so there is no id to hand over"
    );
}
