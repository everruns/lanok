//! Transports: how a [`Message`] gets from one peer to the other.
//!
//! The trait is **frame-oriented**, not byte-oriented. A transport hands the
//! peer one whole message at a time and takes one whole message back, so
//! framing is the adapter's problem and the peer never sees a partial line.
//! That is what lets ndjson-over-stdio and, later, one-JSON-per-WebSocket-frame
//! be the same trait rather than two shapes with a shim between them.
//!
//! Three adapters ship today:
//!
//! * [`StdioTransport`], this process's own stdin and stdout. What a server
//!   spawned over a pipe uses.
//! * [`ChildTransport`], a spawned child process, with its stderr drained to a
//!   sink so a chatty server cannot deadlock on a full pipe.
//! * [`duplex`], an in-memory pair, so a host and a server can be driven
//!   against each other in one test with no process at all.
//!
//! ```
//! # use lanok_transport::{duplex, Transport};
//! # use lanok_core::Message;
//! # use serde_json::json;
//! # #[tokio::main] async fn main() {
//! let (mut a, mut b) = duplex();
//! a.send(Message::notification("ping", json!({}))).await.unwrap();
//! let received = b.recv().await.unwrap().unwrap();
//! assert_eq!(received.method(), Some("ping"));
//! # }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

mod child;
mod ndjson;

use std::io;

use async_trait::async_trait;
use lanok_core::Message;

pub use child::{ChildTransport, StderrSink};
pub use ndjson::{NdjsonTransport, StdioTransport, duplex};

/// A bidirectional stream of whole protocol messages.
///
/// Implementors own their framing. `recv` returning `None` is a clean close;
/// returning `Some(Err(..))` is a broken one, and the peer fails every pending
/// request either way.
#[async_trait]
pub trait Transport: Send + std::fmt::Debug + 'static {
    /// The next message, or `None` at end of stream.
    ///
    /// **Must be cancellation safe.** The peer drives `recv` inside a
    /// `select!` against its outbound queue, so this future is dropped every
    /// time a write wins the race. An implementation that loses buffered input
    /// on drop will silently eat messages under load, which is close to
    /// impossible to diagnose from the outside. Keep partial reads in the
    /// transport, not in the future.
    ///
    /// A line that is not a usable message is skipped rather than returned as
    /// an error: one peer writing garbage should not tear down a connection
    /// that is otherwise healthy. Implementors count skips and expose the
    /// count.
    async fn recv(&mut self) -> Option<io::Result<Message>>;

    /// Write one message.
    async fn send(&mut self, message: Message) -> io::Result<()>;

    /// Release the underlying resource. Idempotent.
    async fn close(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Transport for Box<dyn Transport> {
    async fn recv(&mut self) -> Option<io::Result<Message>> {
        (**self).recv().await
    }
    async fn send(&mut self, message: Message) -> io::Result<()> {
        (**self).send(message).await
    }
    async fn close(&mut self) -> io::Result<()> {
        (**self).close().await
    }
}
