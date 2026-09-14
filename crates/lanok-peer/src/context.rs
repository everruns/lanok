//! What a handler knows about the request it is answering.
//!
//! A handler used to receive a method name and a blob of params, which is
//! everything about the request except which request it is. That is enough for
//! a pure function of its arguments and nothing else. A protocol that reports
//! progress has to name the request the progress belongs to; a protocol whose
//! `cancel` aborts an in-flight call has to find that call by id. Both were
//! writing their own serve loop to get at an id lanok already had.
//!
//! The same type reaches handlers on both paths, which is what makes the
//! promise in [`SimpleServer`](crate::SimpleServer)'s docs true: a server that
//! outgrows the serial loop moves to [`Peer`](crate::Peer) and keeps its
//! handlers, because the dispatch signature is the same.

use std::io::Write;
use std::sync::{Arc, Mutex};

use lanok_core::{Id, Message, Value};

use crate::handshake::Hello;
#[cfg(feature = "async")]
use crate::peer::WeakPeer;

/// The write half, shared with handlers so they can emit progress.
pub(crate) type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// The request a handler is answering, and what it can do besides return a
/// value.
///
/// Cheap to clone: everything inside is shared or small.
#[derive(Clone)]
pub struct Context {
    id: Option<Id>,
    wire: Wire,
}

/// Where a notification goes. An enum rather than a trait object because there
/// are exactly two ways to be serving, and both are in this crate.
#[derive(Clone)]
enum Wire {
    /// The serial server writes the line itself, under the writer lock that
    /// also guards the response, so a progress notification never interleaves
    /// with one.
    Serial {
        writer: SharedWriter,
        peer: Arc<Mutex<Option<Hello>>>,
    },
    /// The async peer hands the message to its outbound queue.
    ///
    /// Weak on purpose, and for the same reason the dispatch task is: an
    /// in-flight handler may answer while the connection is up, but must not
    /// be what keeps it up.
    #[cfg(feature = "async")]
    Peer(WeakPeer),
    /// No connection behind it. What [`Context::detached`] makes, so a handler
    /// can be called from a test without standing up a transport.
    Detached,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Context {
    /// A context attached to nothing: no id, notifications go nowhere, and the
    /// peer supports nothing.
    ///
    /// For calling a handler directly, which is how a handler's own tests
    /// should run. A handler that needs a real id or a real peer is testing
    /// the serve loop, not itself, and wants a transport.
    pub fn detached() -> Self {
        Context {
            id: None,
            wire: Wire::Detached,
        }
    }

    pub(crate) fn for_serial(
        writer: SharedWriter,
        peer: Arc<Mutex<Option<Hello>>>,
        id: Option<Id>,
    ) -> Self {
        Context {
            id,
            wire: Wire::Serial { writer, peer },
        }
    }

    #[cfg(feature = "async")]
    pub(crate) fn for_peer(peer: WeakPeer, id: Option<Id>) -> Self {
        Context {
            id,
            wire: Wire::Peer(peer),
        }
    }

    /// The id of the request being answered, or `None` inside a notification
    /// handler, where there is no request and nothing to reply to.
    pub fn id(&self) -> Option<&Id> {
        self.id.as_ref()
    }

    /// Emit a notification now, before this request's own response.
    ///
    /// This is how a handler streams progress: the caller sees the
    /// notifications while the request is still open. Correlating them is the
    /// protocol's business, and [`Context::id`] is what it correlates on.
    pub fn notify(&self, method: impl Into<String>, params: Value) {
        let message = Message::notification(method, params);
        match &self.wire {
            Wire::Serial { writer, .. } => {
                let mut writer = writer.lock().expect("writer lock");
                let _ = writeln!(writer, "{}", message.to_line());
                let _ = writer.flush();
            }
            // Gone means the connection ended while this handler ran. The
            // response is about to be dropped for the same reason, so a lost
            // progress notification is the lesser half of an outcome nobody
            // will read.
            #[cfg(feature = "async")]
            Wire::Peer(peer) => {
                if let Some(peer) = peer.upgrade() {
                    peer.notify_message(message);
                }
            }
            Wire::Detached => {}
        }
    }

    /// Whether the connected peer advertised `token` in its handshake.
    ///
    /// The same question [`Peer::supports`](crate::Peer::supports) answers, so
    /// a handler deciding whether to stream asks it the same way the generated
    /// stubs do before writing to the wire.
    pub fn supports(&self, token: &str) -> bool {
        self.peer_hello()
            .is_some_and(|hello| hello.capabilities.supports(token))
    }

    /// The peer's handshake, once it has arrived.
    pub fn peer(&self) -> Option<Hello> {
        self.peer_hello()
    }

    fn peer_hello(&self) -> Option<Hello> {
        match &self.wire {
            Wire::Serial { peer, .. } => peer.lock().expect("peer lock").clone(),
            #[cfg(feature = "async")]
            Wire::Peer(peer) => peer.upgrade().and_then(|peer| peer.peer_info()),
            Wire::Detached => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_detached_context_answers_rather_than_panics() {
        let cx = Context::detached();
        assert!(cx.id().is_none());
        assert!(!cx.supports("anything"));
        assert!(cx.peer().is_none());
        // Goes nowhere, but a handler under test may call it.
        cx.notify("progress", Value::Null);
    }
}
