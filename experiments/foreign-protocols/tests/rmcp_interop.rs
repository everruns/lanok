//! The falsification test the declaration experiment could not do on its own:
//! connect lanok's MCP declaration to a **real, third-party MCP
//! implementation** and see whether they talk.
//!
//! `experiments/foreign-protocols/src/mcp.rs` shows that MCP's method surface
//! is *expressible* in `lanok::protocol!`. That is a statement about the macro,
//! not about the wire, and the crate docs have said so: "what remains unproven
//! is wire identity". This file proves it, or fails to.
//!
//! The other side is `rmcp`, the official Rust MCP SDK from the
//! modelcontextprotocol organisation. It was written with no knowledge of
//! lanok, it owns the payload types, and it decides what is and is not
//! acceptable on the wire. Nothing here reimplements MCP: every payload that
//! crosses is an `rmcp::model` type, serialised by rmcp's own derives, so a
//! byte lanok gets wrong is a byte rmcp rejects.
//!
//! What this does and does not license anyone to claim: lanok can *speak* MCP
//! well enough for a real implementation to hold a full session with it. It is
//! still not an MCP client, and `rmcp` remains the answer for anyone who wants
//! one. See `knowledge/specs/foreign-protocols.md`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use foreign_protocols::mcp::*;
use lanok::{Capabilities, Hello, NdjsonTransport, Peer, RpcError, Version};
use rmcp::ServiceExt;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, ElicitRequestParams, ElicitationSchema,
    Implementation, InitializeRequestParams, InitializeResult, ListToolsResult,
    ProgressNotificationParam, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// The third-party side: a real MCP server, built on rmcp's own handler trait.
// ---------------------------------------------------------------------------

/// One tool, `greet`, which on the way to its answer does the two things that
/// make this a real test rather than a request/response formality: it streams a
/// `notifications/progress` (an `either` method, the shape MCP forced into
/// lanok) and it turns the connection around to ask the client something
/// (`elicitation/create`, the reverse channel).
#[derive(Clone)]
struct GreetServer;

impl rmcp::ServerHandler for GreetServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = implementation("greet-server", "1.0.0");
        info.instructions = Some("A server that greets, after asking how.".into());
        info
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        Ok(ListToolsResult {
            meta: None,
            tools: vec![Tool::new(
                "greet",
                "Greet someone, asking the client how first.",
                Arc::new(
                    json!({
                        "type": "object",
                        "properties": { "name": { "type": "string" } },
                        "required": ["name"],
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )],
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let name = request
            .arguments
            .as_ref()
            .and_then(|args| args.get("name"))
            .and_then(|name| name.as_str())
            .unwrap_or("world")
            .to_string();

        // `notifications/progress`: either side may send it, which is the case
        // that produced `Direction::Either`. Here the third-party side sends it
        // and lanok must accept it as an unsolicited inbound notification.
        let _ = context
            .peer
            .notify_progress(progress_at(&context, 1.0, "asking the client"))
            .await;

        // The reverse channel: the server asks the client. A lanok peer answers
        // this from its `InitiatorHandler`.
        let asked = context
            .peer
            .create_elicitation(ElicitRequestParams::FormElicitationParams {
                meta: None,
                message: format!("How should I greet {name}?"),
                requested_schema: ElicitationSchema::builder()
                    .string_property("greeting", |s| s)
                    .build()
                    .expect("a valid elicitation schema"),
            })
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;

        let greeting = asked
            .content
            .as_ref()
            .and_then(|content| content.get("greeting"))
            .and_then(|greeting| greeting.as_str())
            .unwrap_or("Hello")
            .to_string();

        let _ = context
            .peer
            .notify_progress(progress_at(&context, 2.0, "answering"))
            .await;

        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "{greeting}, {name}!"
        ))]))
    }
}

/// rmcp's model types are `#[non_exhaustive]`, so they are built rather than
/// written out. That is rmcp's call to make, and the point of using its types.
fn implementation(name: &str, version: &str) -> Implementation {
    let mut implementation = Implementation::default();
    implementation.name = name.into();
    implementation.version = version.into();
    implementation
}

fn progress_at(
    context: &RequestContext<RoleServer>,
    at: f64,
    message: &str,
) -> ProgressNotificationParam {
    let mut param =
        ProgressNotificationParam::new(rmcp::model::ProgressToken(context.id.clone()), at);
    param.total = Some(2.0);
    param.message = Some(message.into());
    param
}

// ---------------------------------------------------------------------------
// The lanok side: an MCP client, from the declaration and nothing else.
// ---------------------------------------------------------------------------

/// Everything the server may ask us. These are the generated
/// `responder`-and-`either` methods of `mcp::protocol!`, defaulting to
/// `method not found`, so only what a client actually answers is written here.
struct Client {
    progress: Arc<AtomicU32>,
}

#[lanok::async_trait]
impl InitiatorHandler for Client {
    /// `elicitation/create`, declared `responder` because only a server sends
    /// it. rmcp's `ElicitResult` is what it wants back.
    async fn elicitation_create(&self, params: Value) -> Result<Value, RpcError> {
        // rmcp's own type, deserialised from what lanok carried. If a byte were
        // wrong, this is where it would show.
        let request: ElicitRequestParams = serde_json::from_value(params)
            .map_err(|e| RpcError::internal(format!("bad elicitation params: {e}")))?;
        let ElicitRequestParams::FormElicitationParams { message, .. } = &request else {
            return Err(RpcError::internal("expected a form elicitation"));
        };
        assert!(
            message.contains("greet"),
            "the server's question should reach us intact: {message}"
        );
        Ok(json!({
            "action": "accept",
            "content": { "greeting": "Vitayu" },
        }))
    }

    /// `notifications/progress`, declared `either`. Sent here by the server.
    fn progress(&self, _params: Value) {
        self.progress.fetch_add(1, Ordering::SeqCst);
    }
}

/// MCP announces capabilities as a nested object; lanok gates on flat tokens.
/// Flattening is the adopter's job, which is the honest shape: lanok does not
/// know what a foreign protocol's capability document means.
/// The peer's handshake, in lanok's shape.
///
/// MCP dates its protocol versions ("2025-06-18") where lanok negotiates
/// MAJOR.MINOR, so there is nothing to put in `protocol_version`: rmcp has
/// already negotiated, and lanok is only being told what the server can do.
/// The real version travels in `info`, which is free-form for exactly this.
fn hello_from(server: &InitializeResult) -> Hello {
    Hello {
        name: server.server_info.name.clone(),
        protocol_version: Version::new(0, 0),
        capabilities: tokens(server),
        info: serde_json::json!({ "mcp_protocol_version": server.protocol_version }),
    }
}

fn tokens(server: &InitializeResult) -> Capabilities {
    let mut caps = Capabilities::new();
    let advertised = &server.capabilities;
    if advertised.tools.is_some() {
        caps.insert(capability::TOOLS);
    }
    if advertised.resources.is_some() {
        caps.insert(capability::RESOURCES);
    }
    if advertised.prompts.is_some() {
        caps.insert(capability::PROMPTS);
    }
    if advertised.logging.is_some() {
        caps.insert(capability::LOGGING);
    }
    if advertised.completions.is_some() {
        caps.insert(capability::COMPLETIONS);
    }
    caps
}

#[tokio::test]
async fn lanok_holds_a_full_mcp_session_with_rmcp() {
    // One in-memory pipe, both ends real: rmcp's transport on one side,
    // lanok's ndjson transport on the other. Nothing translates between them.
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);

    let server = tokio::spawn(async move {
        let running = GreetServer.serve(server_io).await.expect("rmcp serves");
        running.waiting().await.expect("rmcp runs to completion");
    });

    let progress = Arc::new(AtomicU32::new(0));
    let (client_read, client_write) = tokio::io::split(client_io);
    let peer = Peer::builder()
        .handler(InitiatorDispatch::new(Client {
            progress: progress.clone(),
        }))
        // MCP's handshake is its own: `initialize`, completed by
        // `notifications/initialized`, not lanok's `initialized`.
        .handshake_methods("initialize", "notifications/initialized")
        .request_timeout(std::time::Duration::from_secs(10))
        .connect(NdjsonTransport::new(client_read, client_write));

    // 1. Handshake, in MCP's payloads rather than lanok's `Hello`.
    let ours = InitializeRequestParams::new(
        rmcp::model::ClientCapabilities::builder()
            .enable_elicitation()
            .build(),
        implementation("lanok-experiment", "0.0.0"),
    );
    let server_info: InitializeResult = peer
        .handshake_with("initialize", &ours)
        .await
        .expect("rmcp accepts lanok's initialize");

    assert_eq!(server_info.server_info.name, "greet-server");
    assert!(
        server_info.capabilities.tools.is_some(),
        "the server advertises tools"
    );

    // Feed the negotiated capabilities back in, so the generated `requires`
    // gates answer from what this peer actually advertised.
    peer.record_peer(hello_from(&server_info));
    peer.notify_initialized();

    // 2. `ping`, declared `either`. The direction that did not exist in lanok
    //    until MCP was written down, answered by a real MCP implementation.
    peer.ping(json!({})).await.expect("rmcp answers ping");

    // 3. `tools/list`, gated on the `tools` capability the server just
    //    advertised. Deserialised into rmcp's own result type.
    let tools: ListToolsResult = serde_json::from_value(
        peer.tools_list(json!({}))
            .await
            .expect("rmcp answers tools/list"),
    )
    .expect("rmcp's own ListToolsResult round-trips");
    assert_eq!(tools.tools.len(), 1);
    assert_eq!(tools.tools[0].name, "greet");

    // 4. `tools/call`, which makes the server turn the connection around:
    //    progress notifications and a reverse `elicitation/create` that this
    //    peer's handler answers, all inside one outstanding request.
    let called: CallToolResult = serde_json::from_value(
        peer.tools_call(json!({ "name": "greet", "arguments": { "name": "Kyiv" } }))
            .await
            .expect("rmcp answers tools/call"),
    )
    .expect("rmcp's own CallToolResult round-trips");

    let text = called
        .content
        .first()
        .and_then(|first| first.as_text())
        .map(|text| text.text.clone())
        .expect("a text result");
    assert_eq!(
        text, "Vitayu, Kyiv!",
        "the greeting the server used is the one lanok answered its elicitation with"
    );

    assert_eq!(
        progress.load(Ordering::SeqCst),
        2,
        "both progress notifications reached the lanok handler"
    );

    drop(peer);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), server).await;
}

/// A capability the server did not advertise is refused locally, before the
/// wire. rmcp would answer `method not found`; lanok never asks.
#[tokio::test]
async fn an_unadvertised_capability_never_reaches_the_server() {
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        if let Ok(running) = GreetServer.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });

    let (client_read, client_write) = tokio::io::split(client_io);
    let peer = Peer::builder()
        .handler(InitiatorDispatch::new(Client {
            progress: Arc::new(AtomicU32::new(0)),
        }))
        .handshake_methods("initialize", "notifications/initialized")
        .request_timeout(std::time::Duration::from_secs(10))
        .connect(NdjsonTransport::new(client_read, client_write));

    let ours = InitializeRequestParams::new(
        rmcp::model::ClientCapabilities::default(),
        implementation("lanok-experiment", "0.0.0"),
    );
    let server_info: InitializeResult = peer.handshake_with("initialize", &ours).await.unwrap();
    peer.record_peer(hello_from(&server_info));
    peer.notify_initialized();

    // This server advertises `tools` and nothing else.
    let refused = peer.prompts_list(json!({})).await.unwrap_err();
    assert_eq!(refused.code, lanok::codes::CAPABILITY_UNSUPPORTED);
    assert!(
        refused.message.contains("prompts"),
        "the refusal names the missing capability: {}",
        refused.message
    );

    drop(peer);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), server).await;
}
