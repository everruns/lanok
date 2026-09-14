//! A blocking, serial server. No async runtime anywhere in its graph.
//!
//! This exists because of who writes servers. An extension author adding forty
//! lines of tool handler should not have to compile tokio, learn a runtime, or
//! reason about cancellation. [`SimpleServer`] reads a line, answers it, writes
//! the answer, and repeats, on one thread.
//!
//! The price is honest and stated up front: one request at a time, and no
//! reverse requests (a serial loop cannot wait for a reply while it is busy
//! producing one). Notifications still flow outward, so progress reporting
//! works. A server that outgrows either limit moves to
//! [`Peer`](crate::Peer) and keeps its handlers, because the dispatch signature
//! is the same.

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};

use lanok_core::{Capabilities, Message, RpcError, Value, Version};

use crate::context::{Context, SharedWriter};
use crate::handshake::{Hello, INITIALIZE};

type RequestFn = Box<dyn Fn(&Context, Value) -> Result<Value, RpcError> + Send>;
type NotificationFn = Box<dyn Fn(&Context, Value) + Send>;

/// A serial stdio server.
///
/// ```no_run
/// use lanok_peer::SimpleServer;
///
/// # fn main() -> std::io::Result<()> {
/// SimpleServer::new("echo", "1.0".parse().unwrap())
///     .on_request("echo", |params| {
///         let text = params.get("text").and_then(|v| v.as_str()).unwrap_or_default();
///         Ok(serde_json::json!({ "text": text.to_uppercase() }))
///     })
///     .serve_stdio()
/// # }
/// ```
pub struct SimpleServer {
    name: String,
    version: Version,
    capabilities: Capabilities,
    info: Value,
    requests: BTreeMap<String, RequestFn>,
    notifications: BTreeMap<String, NotificationFn>,
}

impl std::fmt::Debug for SimpleServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimpleServer")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("capabilities", &self.capabilities)
            .field("methods", &self.requests.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl SimpleServer {
    pub fn new(name: impl Into<String>, version: Version) -> Self {
        SimpleServer {
            name: name.into(),
            version,
            capabilities: Capabilities::new(),
            info: Value::Null,
            requests: BTreeMap::new(),
            notifications: BTreeMap::new(),
        }
    }

    /// Advertise a capability token in the handshake.
    pub fn capability(mut self, token: impl Into<String>) -> Self {
        self.capabilities.insert(token);
        self
    }

    /// Protocol-specific handshake payload.
    pub fn info(mut self, info: Value) -> Self {
        self.info = info;
        self
    }

    /// Answer `method` with a function of its params.
    pub fn on_request<F>(mut self, method: impl Into<String>, handler: F) -> Self
    where
        F: Fn(Value) -> Result<Value, RpcError> + Send + 'static,
    {
        self.requests
            .insert(method.into(), Box::new(move |_, params| handler(params)));
        self
    }

    /// Answer `method` with a function that can also emit progress.
    pub fn on_request_with<F>(mut self, method: impl Into<String>, handler: F) -> Self
    where
        F: Fn(&Context, Value) -> Result<Value, RpcError> + Send + 'static,
    {
        self.requests.insert(method.into(), Box::new(handler));
        self
    }

    /// Observe a notification.
    pub fn on_notification<F>(mut self, method: impl Into<String>, handler: F) -> Self
    where
        F: Fn(&Context, Value) + Send + 'static,
    {
        self.notifications.insert(method.into(), Box::new(handler));
        self
    }

    /// Serve on this process's stdin and stdout until end of input.
    pub fn serve_stdio(self) -> io::Result<()> {
        let stdin = io::stdin();
        self.serve(stdin.lock(), Box::new(io::stdout()))
    }

    /// Serve on an arbitrary reader and writer. Returns at end of input.
    pub fn serve(self, reader: impl BufRead, writer: Box<dyn Write + Send>) -> io::Result<()> {
        let writer: SharedWriter = Arc::new(Mutex::new(writer));
        let peer = Arc::new(Mutex::new(None));

        for line in BufReader::new(reader).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            // A line that is not a message is skipped, not fatal: a peer
            // writing one bad line should not take the connection down.
            let Ok(message) = Message::from_line(&line) else {
                continue;
            };

            match message {
                Message::Request { id, method, params } => {
                    // One context per request, because it carries that
                    // request's id.
                    let context =
                        Context::for_serial(writer.clone(), peer.clone(), Some(id.clone()));
                    let outcome = self.answer(&context, &peer, &method, params);
                    let response = match outcome {
                        Ok(result) => Message::result(id, result),
                        Err(error) => Message::error(id, error),
                    };
                    let mut out = writer.lock().expect("writer lock");
                    writeln!(out, "{}", response.to_line())?;
                    out.flush()?;
                }
                Message::Notification { method, params } => {
                    if let Some(handler) = self.notifications.get(&method) {
                        let context = Context::for_serial(writer.clone(), peer.clone(), None);
                        handler(&context, params);
                    }
                }
                // A serial server never issues requests, so any response is
                // unsolicited. Dropping it is the only sane answer.
                Message::Response { .. } => {}
            }
        }
        Ok(())
    }

    fn answer(
        &self,
        context: &Context,
        peer: &Arc<Mutex<Option<Hello>>>,
        method: &str,
        params: Value,
    ) -> Result<Value, RpcError> {
        // The handshake is answered by the server itself. Every lanok protocol
        // has it, and making each author reimplement version and capability
        // reporting is how they drift.
        if method == INITIALIZE {
            if let Ok(theirs) = serde_json::from_value::<Hello>(params) {
                *peer.lock().expect("peer lock") = Some(theirs);
            }
            let ours = Hello {
                name: self.name.clone(),
                protocol_version: self.version,
                capabilities: self.capabilities.clone(),
                info: self.info.clone(),
            };
            return serde_json::to_value(ours)
                .map_err(|e| RpcError::internal(format!("handshake is not serializable: {e}")));
        }

        match self.requests.get(method) {
            Some(handler) => handler(context, params),
            None => Err(RpcError::method_not_found(method)),
        }
    }
}
