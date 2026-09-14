//! What a `protocol!` declaration generates, exercised over a real connection.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use lanok::prelude::*;
use lanok::{Direction, MethodKind, codes, duplex};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EchoParams {
    pub text: String,
}
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EchoResult {
    pub text: String,
}
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AskParams {
    pub question: String,
}
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AskResult {
    pub answer: String,
}
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProgressParams {
    pub step: u32,
}

lanok::protocol! {
    name    = "echo";
    version = "1.2";
    min     = "1.1";

    /// Uppercase some text.
    initiator fn echo(EchoParams) -> EchoResult;

    /// Report progress while a call is open.
    responder notify "echo/progress" progress(ProgressParams);

    /// Ask the caller a question mid-request.
    responder fn "ui/ask" ui_ask(AskParams) -> AskResult requires "ui_ask";

    /// A method with neither params nor result.
    initiator fn ping();

    capabilities {
        /// The caller answers `ui/ask`, so the responder may turn the
        /// connection around mid-request.
        ui_ask,
    }
}

#[test]
fn the_vocabulary_is_available_as_data() {
    assert_eq!(PROTOCOL_NAME, "echo");
    assert_eq!(PROTOCOL_VERSION.to_string(), "1.2");
    assert_eq!(MIN_PROTOCOL_VERSION.to_string(), "1.1");
    assert_eq!(NEGOTIATION.current, PROTOCOL_VERSION);

    assert_eq!(method::ECHO, "echo");
    assert_eq!(method::PROGRESS, "echo/progress");
    assert_eq!(method::UI_ASK, "ui/ask");
    assert_eq!(capability::UI_ASK, "ui_ask");

    let ask = META.method("ui/ask").unwrap();
    assert_eq!(ask.direction, Direction::Responder);
    assert_eq!(ask.kind, MethodKind::Request);
    assert_eq!(ask.requires, Some("ui_ask"));
    assert_eq!(ask.doc, "Ask the caller a question mid-request.");

    assert_eq!(
        META.method("echo/progress").unwrap().kind,
        MethodKind::Notification
    );
    assert!(META.is_bidirectional());
    assert_eq!(META.capabilities, &["ui_ask"]);

    // The payload types each method carries, by name. Without these an SDK
    // generator reading meta.json knows which methods exist but not what any
    // of them takes, so it can only emit string constants.
    let echo = META.method("echo").unwrap();
    assert_eq!(echo.params, Some("EchoParams"));
    assert_eq!(echo.result, Some("EchoResult"));
    // A notification has no result, and a bare method neither.
    assert_eq!(META.method("echo/progress").unwrap().result, None);
    assert_eq!(META.method("ping").unwrap().params, None);

    // The names are `$defs` keys, so they must be what the schema is keyed by
    // rather than the path as written.
    for declared in META.methods {
        for ty in [declared.params, declared.result].into_iter().flatten() {
            assert!(!ty.contains("::"), "`{ty}` is a path, not a $defs key");
        }
    }
}

#[test]
fn the_declaration_also_generates_its_schema_artifacts() {
    // Same declaration, same method list: this is what makes drift impossible
    // rather than merely discouraged.
    let document = schema_document();
    let schema = document.schema();

    assert_eq!(schema["protocol"], "echo");
    assert_eq!(schema["version"], "1.2");
    for declared in META.methods {
        assert!(
            schema["messages"].get(declared.name).is_some(),
            "`{}` is declared but missing from schema.json",
            declared.name
        );
    }
    // A method with neither params nor result still appears, so the artifact
    // lists the whole vocabulary rather than only the interesting parts.
    assert!(schema["messages"]["ping"].as_object().unwrap().is_empty());
    assert!(schema["messages"]["echo"]["params"].is_object());
    assert!(schema["$defs"]["EchoParams"].is_object());
}

#[test]
fn meta_serializes_to_the_committed_artifact_shape() {
    let value = serde_json::to_value(META).unwrap();
    assert_eq!(value["name"], "echo");
    assert_eq!(value["version"], "1.2");
    assert_eq!(value["min_version"], "1.1");
    assert_eq!(value["methods"][0]["name"], "echo");
    assert_eq!(value["methods"][0]["direction"], "initiator");
    assert_eq!(value["methods"][0]["params"], "EchoParams");
    assert_eq!(value["methods"][0]["result"], "EchoResult");
}

/// A responder that uppercases, reports progress, and asks a question.
struct Server {
    peer: Arc<std::sync::OnceLock<Peer>>,
}

#[lanok::async_trait]
impl ResponderHandler for Server {
    async fn echo(&self, _cx: Context, params: EchoParams) -> Result<EchoResult, RpcError> {
        let peer = self.peer.get().expect("peer installed before serving");

        // Both generated stub kinds, in one handler: a notification out and a
        // reverse request that blocks until the initiator answers.
        peer.progress(ProgressParams { step: 1 });

        let asked = peer
            .ui_ask(AskParams {
                question: "shout?".into(),
            })
            .await?;

        Ok(EchoResult {
            text: if asked.answer == "yes" {
                params.text.to_uppercase()
            } else {
                params.text
            },
        })
    }

    async fn ping(&self, _cx: Context) -> Result<(), RpcError> {
        Ok(())
    }
}

struct Client {
    progress_seen: Arc<AtomicU32>,
}

#[lanok::async_trait]
impl InitiatorHandler for Client {
    async fn ui_ask(&self, _cx: Context, params: AskParams) -> Result<AskResult, RpcError> {
        assert_eq!(params.question, "shout?");
        Ok(AskResult {
            answer: "yes".into(),
        })
    }

    fn progress(&self, _cx: Context, params: ProgressParams) {
        assert_eq!(params.step, 1);
        self.progress_seen.fetch_add(1, Ordering::SeqCst);
    }
}

/// Connect both sides and run the real handshake, so capability state on each
/// peer is whatever negotiation actually produced.
async fn connect(
    client: Client,
    server: Server,
    client_caps: &[&str],
    server_caps: &[&str],
) -> (Peer, Peer) {
    let (ta, tb) = duplex();

    let mut ours = Hello::new("test-client", PROTOCOL_VERSION);
    for token in client_caps {
        ours = ours.capability(*token);
    }
    let mut theirs = Hello::new(PROTOCOL_NAME, PROTOCOL_VERSION);
    for token in server_caps {
        theirs = theirs.capability(*token);
    }

    let initiator = Peer::builder()
        .handler(InitiatorDispatch::new(client))
        .serve_handshake(ours.clone())
        .connect(ta);
    let responder = Peer::builder()
        .handler(ResponderDispatch::new(server))
        .serve_handshake(theirs)
        .connect(tb);

    initiator.handshake(&ours, NEGOTIATION).await.unwrap();
    (initiator, responder)
}

#[tokio::test]
async fn generated_stubs_carry_a_call_in_both_directions() {
    let progress_seen = Arc::new(AtomicU32::new(0));
    let peer_slot = Arc::new(std::sync::OnceLock::new());

    let (client, server) = connect(
        Client {
            progress_seen: progress_seen.clone(),
        },
        Server {
            peer: peer_slot.clone(),
        },
        &["ui_ask"],
        &["ui_ask"],
    )
    .await;
    peer_slot.set(server.clone()).unwrap();

    // The handshake, not a test hook, is what makes the reverse stub callable.
    assert!(
        server.supports("ui_ask"),
        "handshake must record the caller's capabilities"
    );

    let result = client
        .echo(EchoParams {
            text: "hello".into(),
        })
        .await
        .unwrap();

    assert_eq!(result.text, "HELLO");
    assert_eq!(progress_seen.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_method_with_no_params_or_result_round_trips() {
    let peer_slot = Arc::new(std::sync::OnceLock::new());
    let (client, server) = connect(
        Client {
            progress_seen: Arc::new(AtomicU32::new(0)),
        },
        Server {
            peer: peer_slot.clone(),
        },
        &[],
        &[],
    )
    .await;
    peer_slot.set(server).unwrap();

    client.ping().await.unwrap();
}

#[tokio::test]
async fn a_gated_method_refuses_locally_when_the_peer_cannot_do_it() {
    let peer_slot = Arc::new(std::sync::OnceLock::new());
    let (_client, server) = connect(
        Client {
            progress_seen: Arc::new(AtomicU32::new(0)),
        },
        Server {
            peer: peer_slot.clone(),
        },
        &[],
        &[],
    )
    .await;
    peer_slot.set(server.clone()).unwrap();

    // The caller advertised nothing, so the reverse stub must not reach the wire.
    let error = server
        .ui_ask(AskParams {
            question: "shout?".into(),
        })
        .await
        .unwrap_err();

    assert_eq!(error.code, codes::CAPABILITY_UNSUPPORTED);
    assert!(error.message.contains("ui_ask"));
}

#[tokio::test]
async fn an_unimplemented_handler_method_refuses_rather_than_hangs() {
    // Client implements ui_ask but a bare handler implements nothing.
    struct Bare;
    #[lanok::async_trait]
    impl InitiatorHandler for Bare {}

    let (ta, tb) = duplex();
    let caller = Hello::new("bare", PROTOCOL_VERSION).capability("ui_ask");
    let initiator = Peer::builder()
        .handler(InitiatorDispatch::new(Bare))
        .serve_handshake(caller.clone())
        .connect(ta);
    let responder = Peer::builder()
        .serve_handshake(Hello::new(PROTOCOL_NAME, PROTOCOL_VERSION))
        .connect(tb);
    initiator.handshake(&caller, NEGOTIATION).await.unwrap();

    let error = responder
        .ui_ask(AskParams {
            question: "anyone home?".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, codes::METHOD_NOT_FOUND);
}

#[tokio::test]
async fn malformed_params_become_invalid_params_not_a_panic() {
    let peer_slot = Arc::new(std::sync::OnceLock::new());
    let (client, server) = connect(
        Client {
            progress_seen: Arc::new(AtomicU32::new(0)),
        },
        Server {
            peer: peer_slot.clone(),
        },
        &["ui_ask"],
        &["ui_ask"],
    )
    .await;
    peer_slot.set(server).unwrap();

    // Bypass the typed stub to send something the declaration does not allow.
    let error = client
        .request(method::ECHO, serde_json::json!({ "text": 42 }))
        .await
        .unwrap_err();
    assert_eq!(error.code, codes::INVALID_PARAMS);
}
