//! The worked example protocol.
//!
//! Small on purpose, but not a toy: it exercises every shape a real lanok
//! protocol uses, which is what makes it worth running in CI.
//!
//! * a forward request with typed params and result (`echo`),
//! * a **reverse** request the responder issues mid-handler (`ui/ask`), gated
//!   on a capability the caller must advertise,
//! * a notification streaming progress while a request is still open,
//! * a method with neither params nor result (`ping`).
//!
//! Both server flavours are built from this one declaration: `echo-server`
//! runs the blocking [`SimpleServer`](lanok::SimpleServer), and the async
//! [`Peer`](lanok::Peer) path is driven by `echo-client --reverse`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EchoParams {
    /// The text to transform.
    pub text: String,
    /// Uppercase it. Defaults to false, like every optional lanok field, so a
    /// peer that predates this field still parses.
    #[serde(default)]
    pub shout: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EchoResult {
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AskParams {
    pub question: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AskResult {
    pub answer: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ProgressParams {
    pub step: u32,
    pub of: u32,
}

lanok::protocol! {
    name    = "echo";
    version = "1.0";

    /// Transform some text and hand it back.
    initiator fn echo(EchoParams) -> EchoResult;

    /// Liveness check. No params, no result.
    initiator fn ping();

    /// Progress while an `echo` is still open.
    responder notify "echo/progress" progress(ProgressParams);

    /// Ask the caller a question and wait for the answer. The reverse
    /// direction: the responder is the one making the request.
    responder fn "ui/ask" ui_ask(AskParams) -> AskResult requires "ui_ask";

    capabilities { ui_ask }
}
