//! A kit for building JSON-RPC 2.0 protocols.
//!
//! Lanok is not a protocol. It is what you build one *with*: framing, id
//! correlation, the version handshake, capability gating, schema artifacts, and
//! the Python and TypeScript SDK runtimes, so the only thing a project writes
//! is the part that is actually its own, the methods and their payloads.
//!
//! # The one idea
//!
//! There is no client type and no server type. There is one symmetric [`Peer`],
//! and direction is a property declared on each *method*:
//!
//! ```
//! # use serde::{Deserialize, Serialize};
//! #[derive(Serialize, Deserialize)]
//! pub struct EchoParams { pub text: String }
//! #[derive(Serialize, Deserialize)]
//! pub struct EchoResult { pub text: String }
//! #[derive(Serialize, Deserialize)]
//! pub struct AskParams { pub question: String }
//! #[derive(Serialize, Deserialize)]
//! pub struct AskResult { pub answer: String }
//!
//! lanok::protocol! {
//!     name    = "echo";
//!     version = "1.0";
//!
//!     /// Uppercase some text.
//!     initiator fn echo(EchoParams) -> EchoResult;
//!
//!     /// A reverse call: the server asks the client something mid-request.
//!     responder fn "ui/ask" ui_ask(AskParams) -> AskResult requires "ui_ask";
//!
//!     capabilities { ui_ask }
//! }
//!
//! assert_eq!(PROTOCOL_VERSION.to_string(), "1.0");
//! assert_eq!(method::ECHO, "echo");
//! assert!(META.is_bidirectional());
//! ```
//!
//! A line is classified by its fields, never by the pipe it arrived on, so a
//! reverse request is an additive declaration rather than a redesign.
//!
//! # What the declaration generates
//!
//! | Item | Purpose |
//! |------|---------|
//! | `PROTOCOL_VERSION`, `MIN_PROTOCOL_VERSION`, `NEGOTIATION` | version negotiation, ready to hand to [`Peer::handshake`] |
//! | `META` | the vocabulary as data, serialized to `meta.json` |
//! | `method::*`, `capability::*` | names, so a call site never spells one as a string |
//! | `InitiatorApi`, `ResponderApi` | typed stubs, implemented for [`Peer`], gated by role |
//! | `InitiatorHandler`, `ResponderHandler` | what each side answers, every method defaulting to `method not found` |
//! | `InitiatorDispatch`, `ResponderDispatch` | adapters onto [`Handler`] |
//!
//! # Choosing a server shape
//!
//! [`SimpleServer`] is blocking, serial, and compiles no async runtime: turn
//! off the `async` feature and tokio leaves the dependency graph entirely. It
//! is the right answer for an extension author writing forty lines of tool
//! handler. [`Peer`] is the answer when requests must overlap or flow both
//! ways. Handlers are signature compatible, so moving between them is a move.

#![forbid(unsafe_code)]

pub use lanok_core::{
    Capabilities, Direction, Id, IdAllocator, Incompatible, JSONRPC_VERSION, Malformed, Message,
    MethodKind, MethodMeta, Negotiation, ProtocolMeta, RpcError, Value, Version, codes,
};
pub use lanok_peer::{Context, Hello, INITIALIZE, INITIALIZED, SimpleServer};

#[cfg(feature = "async")]
pub use lanok_peer::{Handler, HandlerFuture, NoHandler, Peer, PeerBuilder, PeerInfo, Router};

#[cfg(feature = "async")]
pub use lanok_transport::{
    ChildTransport, NdjsonTransport, StderrSink, StdioTransport, Transport, duplex,
};

/// Re-exported for generated code, so a protocol crate needs no direct
/// dependency on either.
#[cfg(feature = "async")]
pub use async_trait::async_trait;
pub use serde_json::{from_value, to_value};

#[cfg(feature = "macros")]
pub use lanok_macros::protocol;

/// Everything a protocol crate typically imports.
pub mod prelude {
    pub use lanok_core::{Capabilities, Negotiation, RpcError, Value, Version};
    #[cfg(feature = "async")]
    pub use lanok_peer::{Handler, Peer, Router};
    pub use lanok_peer::{Hello, SimpleServer};
    #[cfg(feature = "async")]
    pub use lanok_transport::{ChildTransport, StdioTransport, Transport, duplex};
}
