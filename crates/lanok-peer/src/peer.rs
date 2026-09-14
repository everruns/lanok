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
//! * A dropped or timed-out request future frees its slot and runs the
//!   protocol's abandonment hook, so the peer can stop work nobody is waiting
//!   for. Lanok notices; the protocol decides what that means on the wire.
//! * When the connection ends, every pending request fails at once rather than
//!   waiting out its individual timeout.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use lanok_core::{Id, IdAllocator, Message, RpcError, Value};
use lanok_transport::Transport;
use tokio::sync::{Notify, mpsc, oneshot};

use crate::context::Context;
use crate::handler::{Handler, NoHandler};
use crate::handshake::{Hello, INITIALIZE, INITIALIZED};

type Pending = HashMap<Id, oneshot::Sender<Result<Value, RpcError>>>;

/// A request whose caller went away before the response arrived.
#[derive(Debug, Clone)]
pub struct Abandoned {
    /// The id the peer knows this request by.
    pub id: Id,
    /// The method that was called, so a hook can arm only for the methods its
    /// protocol says are cancelable.
    pub method: String,
    /// Whether the caller gave up because its deadline passed, rather than
    /// dropping the future.
    pub timed_out: bool,
}

/// What to do when a request is abandoned.
///
/// Lanok owns the mechanism, noticing the abandonment and handing over the id;
/// the protocol owns what goes on the wire. That split is deliberate, because
/// there is no single right answer: mira's `cancel` is an acknowledged request,
/// LSP's `$/cancelRequest` is a notification carrying `{id}`, and MCP's
/// `notifications/cancelled` carries `{requestId, reason}`. A hook expresses
/// all three, and the arming policy too, where a single method name could only
/// express one.
///
/// Called from a `Drop`, so it must not block. Use [`Peer::notify`] directly,
/// or spawn for anything that needs to await.
pub type AbandonHook = Arc<dyn Fn(&Peer, &Abandoned) + Send + Sync>;

/// A [`Peer`] handle that does not keep the connection open.
///
/// Held by anything that outlives a request but must not outlive the
/// connection: the dispatch task, and the [`Context`](crate::Context) it hands
/// the handler.
#[derive(Clone)]
pub(crate) struct WeakPeer(Weak<Inner>);

impl WeakPeer {
    pub(crate) fn upgrade(&self) -> Option<Peer> {
        self.0.upgrade().map(Peer)
    }
}

struct Inner {
    outbound: mpsc::UnboundedSender<Message>,
    /// `None` once the connection has ended, which is how a late caller learns
    /// the peer is gone without waiting for a timeout.
    pending: Mutex<Option<Pending>>,
    ids: IdAllocator,
    request_timeout: Option<Duration>,
    on_abandon: Option<AbandonHook>,
    /// The peer's handshake, `None` until it arrives. `Hello` and not a
    /// reduced copy of it: the protocol's own `info` travels there, and a
    /// summary struct was quietly dropping it.
    info: Mutex<Option<Hello>>,
    closed: AtomicBool,
    /// What to answer the handshake request with, when this peer serves one.
    serves_handshake: Option<Hello>,
    /// The handshake's method names. Configurable because a protocol that
    /// already exists gets to keep its own: MCP's completion notification is
    /// `notifications/initialized`, not `initialized`.
    handshake_method: String,
    initialized_method: String,
    /// Raised by [`Peer::shutdown`]. The pump selects on it, so a local
    /// shutdown does not depend on every handle being dropped first.
    ///
    /// Shared with the pump by `Arc` rather than reached through the peer:
    /// the pump must not hold anything strong across its select, or the
    /// outbound sender it would keep alive would stop the last handle's drop
    /// from ever closing the connection.
    stop: Arc<Notify>,
    /// The pump task, so `shutdown` can await the transport's release rather
    /// than return before the child process is reaped.
    pump: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Peer")
            .field("closed", &self.closed)
            .field("serves_handshake", &self.serves_handshake.is_some())
            .field("on_abandon", &self.on_abandon.is_some())
            .finish_non_exhaustive()
    }
}

/// A live connection to another peer. Cheap to clone; clones share one
/// connection and issue requests concurrently.
#[derive(Clone, Debug)]
pub struct Peer(Arc<Inner>);

/// Configures a peer before it is connected.
pub struct PeerBuilder {
    handler: Arc<dyn Handler>,
    request_timeout: Option<Duration>,
    on_abandon: Option<AbandonHook>,
    serves_handshake: Option<Hello>,
    handshake_method: String,
    initialized_method: String,
}

impl std::fmt::Debug for PeerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PeerBuilder")
            .field("request_timeout", &self.request_timeout)
            .field("on_abandon", &self.on_abandon.is_some())
            .field("serves_handshake", &self.serves_handshake.is_some())
            .finish_non_exhaustive()
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
            on_abandon: None,
            serves_handshake: None,
            handshake_method: INITIALIZE.to_string(),
            initialized_method: INITIALIZED.to_string(),
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

    /// Run `hook` when a caller abandons a request, so the peer can stop work
    /// nobody is waiting for.
    ///
    /// The hook decides everything the protocol owns: whether to send anything
    /// at all, whether it is a notification or a request, what the params are
    /// called, and which methods are worth cancelling. It receives the method
    /// name and whether the caller timed out, and can consult
    /// [`Peer::supports`] to stay quiet against a peer that never advertised
    /// cancellation.
    ///
    /// ```no_run
    /// # use lanok_peer::{Peer, PeerBuilder};
    /// # use std::sync::Arc;
    /// # let builder = Peer::builder();
    /// // mira: an acknowledged request, only for cancelable methods, only when
    /// // the study said it can.
    /// builder.on_abandon(Arc::new(|peer: &Peer, abandoned| {
    ///     if !peer.supports("cancel")
    ///         || !matches!(abandoned.method.as_str(), "run" | "execute" | "score")
    ///     {
    ///         return;
    ///     }
    ///     let peer = peer.clone();
    ///     let id = abandoned.id.clone();
    ///     tokio::spawn(async move {
    ///         let _ = peer.request("cancel", serde_json::json!({ "id": id })).await;
    ///     });
    /// }));
    /// ```
    pub fn on_abandon(mut self, hook: AbandonHook) -> Self {
        self.on_abandon = Some(hook);
        self
    }

    /// Send `method` as a notification carrying `{ "id": <id> }` when a request
    /// is abandoned.
    ///
    /// The LSP-shaped convenience over [`PeerBuilder::on_abandon`], which is
    /// what most protocols want and what `$/cancelRequest` does.
    pub fn cancel_notification(self, method: impl Into<String>) -> Self {
        let method = method.into();
        self.on_abandon(Arc::new(move |peer: &Peer, abandoned: &Abandoned| {
            peer.notify(method.clone(), serde_json::json!({ "id": abandoned.id }));
        }))
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

    /// Rename the handshake's two methods.
    ///
    /// Defaults to `initialize` and `initialized`, the convention a new
    /// protocol should follow. A protocol that already exists keeps its own:
    /// MCP completes the handshake with `notifications/initialized`.
    pub fn handshake_methods(
        mut self,
        request: impl Into<String>,
        notification: impl Into<String>,
    ) -> Self {
        self.handshake_method = request.into();
        self.initialized_method = notification.into();
        self
    }

    /// Start serving over `transport`.
    pub fn connect(self, transport: impl Transport) -> Peer {
        let (outbound, outbound_rx) = mpsc::unbounded_channel();
        let stop = Arc::new(Notify::new());
        let peer = Peer(Arc::new(Inner {
            outbound,
            pending: Mutex::new(Some(Pending::new())),
            ids: IdAllocator::new(),
            request_timeout: self.request_timeout,
            on_abandon: self.on_abandon,
            info: Mutex::new(None),
            closed: AtomicBool::new(false),
            serves_handshake: self.serves_handshake,
            handshake_method: self.handshake_method,
            initialized_method: self.initialized_method,
            stop: stop.clone(),
            pump: Mutex::new(None),
        }));

        // The pump holds a *weak* reference on purpose. A strong one would
        // keep the outbound channel's sender count above zero forever, so
        // dropping the last Peer handle would never close the connection and
        // the task would outlive everything that could use it.
        let task = tokio::spawn(pump(
            Box::new(transport),
            outbound_rx,
            Arc::downgrade(&peer.0),
            self.handler,
            stop,
        ));
        *peer.0.pump.lock().expect("pump lock") = Some(task);
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
        let mut guard = RequestGuard {
            peer: self.clone(),
            id: id.clone(),
            method: method.clone(),
            armed: true,
            timed_out: false,
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
                    // Still armed, and flagged, so the guard fires on the way
                    // out and the hook can tell a deadline from a dropped
                    // future.
                    guard.timed_out = true;
                    return Err(RpcError::timeout(format!("no response within {limit:?}")));
                }
            },
            None => rx.await,
        };

        // The response landed, so there is nothing left to cancel.
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
        self.notify_message(Message::notification(method, params));
    }

    /// Queue an already-built notification. What [`Context::notify`] sends, so
    /// a handler's progress and a caller's notification take one path out.
    pub(crate) fn notify_message(&self, message: Message) {
        let _ = self.0.outbound.send(message);
    }

    /// A handle that does not keep the connection open.
    pub(crate) fn downgrade(&self) -> WeakPeer {
        WeakPeer(Arc::downgrade(&self.0))
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
        let theirs: Hello = self.call(self.0.handshake_method.clone(), ours).await?;

        if let Err(reason) = negotiation.accepts(theirs.protocol_version) {
            return Err(RpcError::new(
                lanok_core::codes::VERSION_INCOMPATIBLE,
                format!(
                    "peer speaks {} but this build speaks {} (min {}): {reason}",
                    theirs.protocol_version, negotiation.current, negotiation.min
                ),
            ));
        }

        self.set_peer_info(theirs.clone());

        // Only after accepting: a peer that gets `initialized` has been told the
        // connection is live, and telling it so before the version check would
        // be a lie we then hang up on.
        self.notify_initialized();
        Ok(theirs)
    }

    /// Run a handshake whose payloads are the protocol's own, not lanok's.
    ///
    /// [`Peer::handshake`] is the convention, and a new protocol should take
    /// it. A protocol that already exists usually cannot: mira's `initialize`
    /// answers with its eval catalogue, MCP's with `serverInfo` and a nested
    /// `capabilities` object. Neither is a [`Hello`], and neither should have
    /// to become one to use a peer.
    ///
    /// This sends `ours` and deserializes the reply, and does nothing else. The
    /// caller checks the version and calls [`Peer::record_peer`] with whatever
    /// it found, which is what lights up [`Peer::supports`] for capability
    /// gating.
    pub async fn handshake_with<P, R>(&self, method: &str, ours: &P) -> Result<R, RpcError>
    where
        P: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        self.call(method, ours).await
    }

    /// Tell the peer what the other side is and can do.
    ///
    /// Set for you by [`Peer::handshake`] and by serving one. Public so a
    /// protocol running its own handshake through [`Peer::handshake_with`]
    /// still gets capability gating rather than having to reimplement it.
    pub fn record_peer(&self, hello: Hello) {
        self.set_peer_info(hello);
    }

    /// Announce that the handshake was accepted, using this peer's configured
    /// notification name.
    ///
    /// Sent for you by [`Peer::handshake`]. Send it yourself after a
    /// [`Peer::handshake_with`] you accepted, and only then: a peer told the
    /// connection is live before the version check has been told something you
    /// then hang up on.
    pub fn notify_initialized(&self) {
        self.notify(self.0.initialized_method.clone(), Value::Null);
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
            .as_ref()
            .is_some_and(|hello| hello.capabilities.supports(token))
    }

    /// The peer's handshake, or `None` before it has arrived.
    pub fn peer_info(&self) -> Option<Hello> {
        self.0.info.lock().expect("info lock").clone()
    }

    pub(crate) fn set_peer_info(&self, hello: Hello) {
        *self.0.info.lock().expect("info lock") = Some(hello);
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

    /// End the connection and wait for the transport to be released.
    ///
    /// Dropping every handle also ends a connection, but says nothing about
    /// *when*: the pump notices asynchronously, and `close` on a child-process
    /// transport is what shuts stdin, waits out the exit grace, and drains
    /// stderr. A caller that needs the child reaped before it returns, or the
    /// socket closed before it rebinds, has no way to observe any of that from
    /// a drop. This does: it signals the pump and awaits it.
    ///
    /// Idempotent, and safe to call with other handles still alive: they see a
    /// closed connection and fail their requests with
    /// [`RpcError::transport_closed`]. Calling it from inside a handler would
    /// deadlock (the pump would be waiting on the handler), so don't.
    pub async fn shutdown(&self) {
        self.0.stop.notify_one();
        let task = self.0.pump.lock().expect("pump lock").take();
        if let Some(task) = task {
            let _ = task.await;
        }
        // A handle is alive by definition (`self`), so the pump's own
        // `fail_all_pending` may have been skipped on the way out.
        self.fail_all_pending(RpcError::transport_closed());
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
    method: String,
    armed: bool,
    timed_out: bool,
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
        // The protocol decides what abandonment means on the wire: whether to
        // send anything, a notification or a request, and for which methods.
        // Lanok only notices and hands over the id.
        if let Some(hook) = &self.peer.0.on_abandon {
            hook(
                &self.peer,
                &Abandoned {
                    id: self.id.clone(),
                    method: self.method.clone(),
                    timed_out: self.timed_out,
                },
            );
        }
    }
}

/// The one task that owns the transport.
async fn pump(
    mut transport: Box<dyn Transport>,
    mut outbound: mpsc::UnboundedReceiver<Message>,
    peer: Weak<Inner>,
    handler: Arc<dyn Handler>,
    stop: Arc<Notify>,
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
            _ = stop.notified() => Event::Stop,
        };

        // Every handle is gone: nobody can send, and nobody is waiting.
        let Some(inner) = peer.upgrade() else { break };
        let live = Peer(inner);

        match event {
            Event::Stop => break,
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
    /// [`Peer::shutdown`] was called.
    Stop,
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
            if method == peer.0.handshake_method && peer.0.serves_handshake.is_some() =>
        {
            let ours = peer
                .0
                .serves_handshake
                .as_ref()
                .expect("checked by the guard");
            if let Ok(theirs) = serde_json::from_value::<Hello>(params) {
                peer.set_peer_info(theirs);
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
        Message::Notification { method, .. } if method == peer.0.initialized_method => {}

        Message::Notification { method, params } => {
            handler.notification(Context::for_peer(peer.downgrade(), None), method, params)
        }
        Message::Request { id, method, params } => {
            // Each request gets its own task, so a slow handler never blocks the
            // reader and requests are answered concurrently.
            //
            // The task holds a weak reference: an in-flight handler may answer
            // if the connection is still up, but must not keep it up. A strong
            // clone here means one slow request pins the whole connection open
            // long after every handle has been dropped.
            let weak = peer.downgrade();
            let cx = Context::for_peer(weak.clone(), Some(id.clone()));
            let handler = handler.clone();
            tokio::spawn(async move {
                let outcome = handler.request(cx, method, params).await;
                let Some(peer) = weak.upgrade() else { return };
                peer.send_message(match outcome {
                    Ok(result) => Message::result(id, result),
                    Err(error) => Message::error(id, error),
                });
            });
        }
    }
}
