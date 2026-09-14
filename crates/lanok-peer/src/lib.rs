//! The symmetric peer, and the blocking server for those who want no runtime.
//!
//! Two shapes, one dispatch surface:
//!
//! | | [`SimpleServer`] | [`Peer`] |
//! |---|---|---|
//! | concurrency | one request at a time | many, in both directions |
//! | runtime | none, blocking std I/O | tokio |
//! | reverse requests | no (notifications out only) | yes |
//! | cancellation | n/a | cancel-on-drop |
//! | fits | extension servers, scaffolded tools | hosts, and servers with real concurrency |
//!
//! Promoting a server from one to the other does not touch its handlers: both
//! hand them the same [`Context`], so a handler that streams progress or reads
//! the request id keeps working across the move.
//!
//! Disable the default `async` feature to get [`SimpleServer`] alone, with no
//! async runtime in the dependency graph at all.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

mod context;
mod handshake;
mod simple;

pub use context::Context;
pub use handshake::{Hello, INITIALIZE, INITIALIZED};
pub use simple::SimpleServer;

#[cfg(feature = "async")]
mod handler;
#[cfg(feature = "async")]
mod peer;

#[cfg(feature = "async")]
pub use handler::{Handler, HandlerFuture, NoHandler, Router};
#[cfg(feature = "async")]
pub use peer::{AbandonHook, Abandoned, Peer, PeerBuilder};
