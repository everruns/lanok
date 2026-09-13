//! The symmetric peer.
//!
//! There is no client type and no server type here. One [`Peer`] issues
//! requests and answers them at the same time, over one connection, and which
//! methods flow in which direction is a property of the protocol declaration
//! rather than of this code.
//!
//! # Shape
//!
//! One task owns the transport. It `select!`s between the next inbound message
//! and the next queued outbound one, so reads and writes interleave without a
//! lock and without splitting the transport in two. Inbound requests are
//! dispatched onto their own tasks, so a slow handler never blocks the reader
//! and a peer can have many requests in flight in both directions at once.
//!
//! # What it guarantees
//!
//! * A response is routed to the caller that registered its id, and each
//!   direction has its own id space.
//! * A dropped request future frees its slot and, if the protocol declares a
//!   cancel notification, tells the peer to stop working. A caller that times
//!   out or is cancelled does not leave the other side burning cycles.
//! * When the connection ends, every pending request fails at once rather than
//!   waiting out its individual timeout.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use lanok_core::{Capabilities, Id, IdAllocator, Message, RpcError, Value, Version};
use lanok_transport::Transport;
use tokio::sync::{mpsc, oneshot};

use crate::handler::{Handler, NoHandler};
use crate::handshake::{Hello, INITIALIZE, INITIALIZED};

type Pending = HashMap<Id, oneshot::Sender<Result<Value, RpcError>>>;

/// What the peer learned about the other side during the handshake.
#[derive(Clone, Debug, Default)]
pub struct PeerInfo {
    pub name: String,
    pub version: Option<Version>,
    pub capabilities: Capabilities,
}

#[derive(Debug)]
struct Inner {
    outbound: mpsc::UnboundedSender<Message>,
    /// `None` once the connection has ended, which is how a late caller learns
    /// the peer is gone without waiting for a timeout.
    pending: Mutex<Option<Pending>>,
    ids: IdAllocator,
    request_timeout: Option<Duration>,
    cancel_method: Option<String>,
    info: Mutex<PeerInfo>,
    closed: AtomicBool,
    /// What to answer `initialize` with, when this peer serves the handshake.
    serves_handshake: Option<Hello>,
}

/// A live connection to another peer. Cheap to clone; clones share one
/// connection and issue requests concurrently.
#[derive(Clone, Debug)]
pub struct Peer(Arc<Inner>);

/// Configures a peer before it is connected.
#[derive(Debug)]
pub struct PeerBuilder {
    handler: Arc<dyn Handler>,
    request_timeout: Option<Duration>,
    cancel_method: Option<String>,
    serves_handshake: Option<Hello>,
}

impl std::fmt::Debug for dyn Handler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Handler")
    }
}

impl Default for PeerBuilder {
    fn default() -> Self {
        PeerBuilder {
            handler: Arc::new(NoHandler),
            // No deadline by default. A host that wants one sets it; imposing
            // a default would silently break the legitimate long call (a model
            // request, a build) that these protocols exist to carry.
            request_timeout: None,
            cancel_method: None,
            serves_handshake: None,
        }
    }
}

impl PeerBuilder {
    /// Install the handler that answers inbound requests and notifications.
    pub fn handler(mut self, handler: impl Handler) -> Self {
        self.handler = Arc::new(handler);
        self
    }

    /// Install a handler already behind an `Arc`, so the caller can keep one.
    pub fn shared_handler(mut self, handler: Arc<dyn Handler>) -> Self {
        self.handler = handler;
        self
    }

    /// Fail a request that has not been answered within `timeout`.
    pub fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = Some(timeout);
        self
    }

    /// The notification this protocol uses to abandon an in-flight request.
    ///
    /// Opt in, because the method name belongs to the protocol rather than to
    /// lanok. With it set, dropping a request future sends
    /// `{ "method": <name>, "params": { "id": <id> } }` so the peer can stop
    /// work the caller no longer wants.
    pub fn cancel_notification(mut self, method: impl Into<String>) -> Self {
        self.cancel_method = Some(method.into());
        self
    }

    /// Answer `initialize` with `ours`, rather than passing it to the handler.
    ///
    /// The peer, not the handler, owns the handshake, for the same reason
    /// [`SimpleServer`](crate::SimpleServer) owns it: capability state lives on
    /// the peer, so answering here is what makes [`Peer::supports`] true on the
    /// responding side too. Without it a server could answer requests but never
    /// learn what its caller can do, which is exactly what a reverse request
    /// needs to know.
    pub fn serve_handshake(mut self, ours: Hello) -> Self {
        self.serves_handshake = Some(ours);
        self
    }

    /// Start serving over `transport`.
    pub fn connect(self, transport: impl Transport) -> Peer {
        let (outbound, outbound_rx) = mpsc::unbounded_channel();
        let peer = Peer(Arc::new(Inner {
            outbound,
            pending: Mutex::new(Some(Pending::new())),
            ids: IdAllocator::new(),
            request_timeout: self.request_timeout,
            cancel_method: self.cancel_method,
            info: Mutex::new(PeerInfo::default()),
            closed: AtomicBool::new(false),
            serves_handshake: self.serves_handshake,
        }));

        // The pump holds a *weak* reference on purpose. A strong one would
        // keep the outbound channel's sender count above zero forever, so
        // dropping the last Peer handle would never close the connection and
        // the task would outlive everything that could use it.
        tokio::spawn(pump(
            Box::new(transport),
            outbound_rx,
            Arc::downgrade(&peer.0),
            self.handler,
        ));
        peer
    }
}

impl Peer {
    pub fn builder() -> PeerBuilder {
        PeerBuilder::default()
    }

    /// Issue a request and wait for its response.
    ///
    /// Dropping the returned future frees the pending slot and, when the
    /// protocol declares one, sends the cancel notification.
    pub async fn request(
        &self,
        method: impl Into<String>,
        params: Value,
    ) -> Result<Value, RpcError> {
        let method = method.into();
        let id = self.0.ids.next();
        let (tx, rx) = oneshot::channel();

        match self.0.pending.lock().expect("pending lock").as_mut() {
            Some(pending) => {
                pending.insert(id.clone(), tx);
            }
            None => return Err(RpcError::transport_closed()),
        }

        // Armed from here: if this future is dropped or times out before the
        // response lands, the guard cleans up and cancels.
        let guard = RequestGuard {
            peer: self.clone(),
            id: id.clone(),
            armed: true,
        };

        if self
            .0
            .outbound
            .send(Message::request(id, method, params))
            .is_err()
        {
            return Err(RpcError::transport_closed());
        }

        let outcome = match self.0.request_timeout {
            Some(limit) => match tokio::time::timeout(limit, rx).await {
                Ok(received) => received,
                Err(_) => {
                    return Err(RpcError::timeout(format!("no response within {limit:?}")));
                }
            },
            None => rx.await,
        };

        // The response landed, so there is nothing left to cancel.
        let mut guard = guard;
        guard.armed = false;

        outcome.unwrap_or_else(|_| Err(RpcError::transport_closed()))
    }

    /// Issue a request and deserialize its result.
    pub async fn call<P, R>(&self, method: impl Into<String>, params: &P) -> Result<R, RpcError>
    where
        P: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        let params = serde_json::to_value(params)
            .map_err(|e| RpcError::invalid_params(format!("params are not serializable: {e}")))?;
        let result = self.request(method, params).await?;
        serde_json::from_value(result)
            .map_err(|e| RpcError::internal(format!("malformed result: {e}")))
    }

    /// Send a notification. Fire and forget, by definition.
    pub fn notify(&self, method: impl Into<String>, params: Value) {
        let _ = self.0.outbound.send(Message::notification(method, params));
    }

    /// Run the handshake: send `initialize`, check the reply's version against
    /// `negotiation`, record what the peer can do, then send `initialized`.
    ///
    /// After this returns, [`Peer::supports`] is populated, so generated stubs
    /// can gate on capability without a round trip.
    pub async fn handshake(
        &self,
        ours: &Hello,
        negotiation: lanok_core::Negotiation,
    ) -> Result<Hello, RpcError> {
        let theirs: Hello = self.call(INITIALIZE, ours).await?;

        if let Err(reason) = negotiation.accepts(theirs.protocol_version) {
            return Err(RpcError::new(
                lanok_core::codes::VERSION_INCOMPATIBLE,
                format!(
                    "peer speaks {} but this build speaks {} (min {}): {reason}",
                    theirs.protocol_version, negotiation.current, negotiation.min
                ),
            ));
        }

        self.set_peer_info(PeerInfo {
            name: theirs.name.clone(),
            version: Some(theirs.protocol_version),
            capabilities: theirs.capabilities.clone(),
        });

        // Only after accepting: a peer that gets `initialized` has been told the
        // connection is live, and telling it so before the version check would
        // be a lie we then hang up on.
        self.notify(INITIALIZED, Value::Null);
        Ok(theirs)
    }

    /// Whether the peer advertised `token` during the handshake.
    ///
    /// Generated stubs consult this before writing to the wire, so an
    /// unsupported method is a typed local answer rather than a round trip
    /// ending in `method not found`.
    pub fn supports(&self, token: &str) -> bool {
        self.0
            .info
            .lock()
            .expect("info lock")
            .capabilities
            .supports(token)
    }

    /// What the handshake learned about the other side.
    pub fn peer_info(&self) -> PeerInfo {
        self.0.info.lock().expect("info lock").clone()
    }

    pub(crate) fn set_peer_info(&self, info: PeerInfo) {
        *self.0.info.lock().expect("info lock") = info;
    }

    /// Whether the connection has ended.
    pub fn is_closed(&self) -> bool {
        self.0.closed.load(Ordering::Acquire)
    }

    /// Wait until the connection ends.
    pub async fn closed(&self) {
        while !self.is_closed() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn send_message(&self, message: Message) {
        let _ = self.0.outbound.send(message);
    }

    /// Fail every pending request at once. Called when the connection ends, so
    /// callers learn immediately instead of waiting out individual timeouts.
    fn fail_all_pending(&self, error: RpcError) {
        self.0.closed.store(true, Ordering::Release);
        let pending = self.0.pending.lock().expect("pending lock").take();
        for (_, waiter) in pending.unwrap_or_default() {
            let _ = waiter.send(Err(error.clone()));
        }
    }

    fn complete(&self, id: &Id, payload: Result<Value, RpcError>) {
        let waiter = self
            .0
            .pending
            .lock()
            .expect("pending lock")
            .as_mut()
            .and_then(|pending| pending.remove(id));
        // No waiter means the caller already gave up. Dropping the response is
        // correct: a best-effort cancel races exactly this way.
        if let Some(waiter) = waiter {
            let _ = waiter.send(payload);
        }
    }
}

/// Cleans up a request whose future went away before the response arrived.
struct RequestGuard {
    peer: Peer,
    id: Id,
    armed: bool,
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        // Always free the slot, so an abandoned request cannot leak the map.
        let _ = self
            .peer
            .0
            .pending
            .lock()
            .expect("pending lock")
            .as_mut()
            .and_then(|pending| pending.remove(&self.id));

        if !self.armed {
            return;
        }
        // Best effort: tell the peer to stop. No pending slot is registered for
        // the ack, so if the peer answers, the reader drops it.
        if let Some(method) = &self.peer.0.cancel_method {
            self.peer
                .notify(method.clone(), serde_json::json!({ "id": self.id }));
        }
    }
}

/// The one task that owns the transport.
async fn pump(
    mut transport: Box<dyn Transport>,
    mut outbound: mpsc::UnboundedReceiver<Message>,
    peer: Weak<Inner>,
    handler: Arc<dyn Handler>,
) {
    loop {
        // Nothing strong is held across this await, so the moment the last
        // Peer handle drops, `outbound.recv()` resolves to None and the loop
        // ends. Only `transport` is borrowed inside the select, and its recv
        // future is dropped before the send below borrows it again, which the
        // Transport contract requires to be free.
        let event = tokio::select! {
            incoming = transport.recv() => Event::Inbound(incoming),
            queued = outbound.recv() => Event::Outbound(queued),
        };

        // Every handle is gone: nobody can send, and nobody is waiting.
        let Some(inner) = peer.upgrade() else { break };
        let live = Peer(inner);

        match event {
            Event::Inbound(None) | Event::Inbound(Some(Err(_))) => break,
            Event::Inbound(Some(Ok(message))) => dispatch(message, &live, &handler),
            Event::Outbound(None) => break,
            Event::Outbound(Some(message)) => {
                if transport.send(message).await.is_err() {
                    break;
                }
            }
        }
    }

    let _ = transport.close().await;
    // Only if someone is still holding a handle: otherwise there is no pending
    // map left to fail, and no caller left to tell.
    if let Some(inner) = peer.upgrade() {
        Peer(inner).fail_all_pending(RpcError::transport_closed());
    }
}

enum Event {
    Inbound(Option<std::io::Result<Message>>),
    Outbound(Option<Message>),
}

fn dispatch(message: Message, peer: &Peer, handler: &Arc<dyn Handler>) {
    match message {
        Message::Response { id, payload } => peer.complete(&id, payload),

        // The handshake is answered by the peer when it is configured to serve
        // one, so a protocol's handler trait only ever covers the protocol's
        // own methods.
        // Only when this peer was configured to serve one. A peer answering
        // `initialize` from its own handler keeps doing so: intercepting
        // unconditionally would quietly steal the method from it.
        Message::Request { id, method, params }
            if method == INITIALIZE && peer.0.serves_handshake.is_some() =>
        {
            let ours = peer
                .0
                .serves_handshake
                .as_ref()
                .expect("checked by the guard");
            if let Ok(theirs) = serde_json::from_value::<Hello>(params) {
                peer.set_peer_info(PeerInfo {
                    name: theirs.name,
                    version: Some(theirs.protocol_version),
                    capabilities: theirs.capabilities,
                });
            }
            peer.send_message(match serde_json::to_value(ours) {
                Ok(value) => Message::result(id, value),
                Err(e) => Message::error(
                    id,
                    RpcError::internal(format!("handshake is not serializable: {e}")),
                ),
            });
        }
        // Acknowledged by having been received. Nothing to route.
        Message::Notification { method, .. } if method == INITIALIZED => {}

        Message::Notification { method, params } => handler.notification(method, params),
        Message::Request { id, method, params } => {
            // Each request gets its own task, so a slow handler never blocks the
            // reader and requests are answered concurrently.
            //
            // The task holds a weak reference: an in-flight handler may answer
            // if the connection is still up, but must not keep it up. A strong
            // clone here means one slow request pins the whole connection open
            // long after every handle has been dropped.
            let peer = Arc::downgrade(&peer.0);
            let handler = handler.clone();
            tokio::spawn(async move {
                let outcome = handler.request(method, params).await;
                let Some(inner) = peer.upgrade() else { return };
                Peer(inner).send_message(match outcome {
                    Ok(result) => Message::result(id, result),
                    Err(error) => Message::error(id, error),
                });
            });
        }
    }
}
