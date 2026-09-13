//! Wire types for JSON-RPC 2.0 protocols: envelopes, ids, errors, version
//! negotiation, and capability tokens.
//!
//! This crate is deliberately the boring half. It has no I/O, no async runtime,
//! and one dependency, serde, so a protocol can publish its payload types
//! without dragging in a peer, a transport, or a schema generator. The moving
//! parts live in `lanok-transport` and `lanok-peer`.
//!
//! # The one idea
//!
//! A message is classified by its **fields**, never by the pipe it arrived on:
//!
//! | `method` | `id` | meaning |
//! |----------|------|---------|
//! | yes      | yes  | [`Message::Request`] |
//! | yes      | no   | [`Message::Notification`] |
//! | no       | yes  | [`Message::Response`] |
//!
//! Because nothing in that table mentions a direction, a protocol that starts
//! out client-to-server can grow a reverse request without changing its
//! framing, its parser, or its version major.
//!
//! ```
//! use lanok_core::{Message, RpcError};
//! use serde_json::json;
//!
//! let line = Message::request(1u64, "echo", json!({ "text": "hi" })).to_line();
//! assert!(line.contains(r#""jsonrpc":"2.0""#));
//!
//! match Message::from_line(&line).unwrap() {
//!     Message::Request { method, .. } => assert_eq!(method, "echo"),
//!     _ => unreachable!(),
//! }
//!
//! // Errors are strictly conformant: code, message, and optional data.
//! let error = RpcError::internal("upstream is busy").retryable();
//! assert!(error.is_retryable());
//! ```

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

mod capability;
mod error;
mod id;
mod message;
mod version;

pub use capability::Capabilities;
pub use error::{RpcError, codes};
pub use id::{Id, IdAllocator};
pub use message::{JSONRPC_VERSION, Malformed, Message};
pub use version::{Incompatible, Negotiation, ParseVersionError, Version};
