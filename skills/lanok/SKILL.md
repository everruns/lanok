---
name: lanok
description: >-
  Build JSON-RPC 2.0 protocols with lanok: declare a protocol with the
  `protocol!` macro, serve it from Rust, Python, or TypeScript, drive it with
  the symmetric peer, and keep schema artifacts and SDKs in lockstep. Use when
  adding or changing a protocol method, wiring a stdio server or host, adding
  reverse (server to client) requests, generating SDKs, or writing conformance
  vectors.
---

# Lanok protocols

Lanok is a kit for building JSON-RPC 2.0 protocols. Not a protocol itself, and
not an MCP or ACP client.

```
protocol! declaration  ->  stubs + handlers + dispatch + meta.json + schema.json + SDKs
```

## The one idea

There is no client type and no server type. One symmetric `Peer`, and
**direction is declared per method**. A line is classified by its fields
(`method` present means request or notification, absent means response), never
by which pipe it arrived on, so a reverse request is an additive declaration
rather than a redesign.

## Declare

```rust
lanok::protocol! {
    name    = "echo";
    version = "1.0";
    min     = "1.0";                      // optional, defaults to MAJOR.0

    /// Doc comments land in meta.json.
    initiator fn echo(EchoParams) -> EchoResult;
    initiator fn ping();                                  // no params, no result
    responder notify "echo/progress" progress(ProgressParams);
    responder fn "ui/ask" ui_ask(AskParams) -> AskResult requires "ui_ask";

    capabilities {
        /// What advertising this token promises. The doc lands on
        /// `capability::UI_ASK`.
        ui_ask,
    }
}
```

`initiator` = sent by the side that opens the connection. `responder` = a
reverse message. A string literal before the identifier is the wire name when
the Rust name cannot spell it.

Generates: `PROTOCOL_VERSION`, `NEGOTIATION`, `META`, `method::*`,
`capability::*`, `InitiatorApi`/`ResponderApi` (typed stubs on `Peer`),
`InitiatorHandler`/`ResponderHandler` (defaults refuse), matching `*Dispatch`
adapters, and `schema_document()` behind your crate's `schema` feature.

**Rejected at compile time**, each because it fails badly at runtime otherwise:
a duplicate method; declaring `initialize`/`initialized`; a notification with a
result; a `min` with a different major or newer than `version`; and `requires`
naming a capability missing from the block (a typo there makes the method
permanently and silently unavailable).

## Serve

Pick by need, not by capability. Handlers port between the two.

```rust
// Serial, blocking, no async runtime in the graph at all.
SimpleServer::new(PROTOCOL_NAME, PROTOCOL_VERSION)
    .capability("ui_ask")
    .on_request(method::ECHO, |params| Ok(params))
    .on_request_with(method::WORK, |context, params| {     // streams progress
        context.notify(method::PROGRESS, json!({ "step": 1 }));
        Ok(params)
    })
    .serve_stdio()
```

```rust
// Concurrent, both directions.
let peer = Peer::builder()
    .handler(ResponderDispatch::new(MyServer))
    .serve_handshake(Hello::new(PROTOCOL_NAME, PROTOCOL_VERSION).capability("ui_ask"))
    .connect(StdioTransport::new());
```

`SimpleServer` cannot serve reverse requests: a serial loop cannot wait for a
reply while producing one.

## Drive

```rust
let transport = ChildTransport::spawn_logging(command, Arc::new(|l: &str| eprintln!("[server] {l}")))?;
let ours = Hello::new("my-host", PROTOCOL_VERSION).capability("ui_ask");
let peer = Peer::builder()
    .handler(InitiatorDispatch::new(MyClient))
    .serve_handshake(ours.clone())
    .connect(transport);
let server = peer.handshake(&ours, NEGOTIATION).await?;
let result = peer.echo(EchoParams { text: "hi".into(), shout: true }).await?;
```

Always `serve_handshake` on a peer that receives connections: capability state
lives on the peer, so it is what makes `supports()` true on that side.

## Artifacts and SDKs

```bash
just schema                                              # regenerate, --check in CI
lanok describe --schema schema/v1                        # what does this protocol speak
lanok gen python     --schema schema/v1 --out sdks/python/lanok/_generated_x.py
lanok gen typescript --schema schema/v1 --out sdks/typescript/src/generatedX.ts
lanok conform --vectors schema/v1/conformance.json -- ./my-server
```

Artifacts are committed and drift-guarded. Change a `protocol!` block, run
`just schema`, commit both files, regenerate the SDKs.

## Rules that are easy to break

- **Only protocol JSON on stdout.** Logging goes to stderr. A host sees the
  damage as `skipped_lines`.
- **Never `deny_unknown_fields`; every added field is `#[serde(default)]`.**
  This is what makes a minor version bump safe.
- **Never repurpose a field or remove a capability token.** Both are major
  changes.
- **`lanok-core` takes serde and nothing else.** No tokio, no schemars outside
  the optional feature. CI checks the dependency graph.
- **`SimpleServer` must not pull tokio.** If a change makes the blocking path
  need a runtime, the change is wrong.

## Python and TypeScript

Both languages ship the same two shapes Rust does, and both can be either end
of a connection.

```python
from lanok import Hello, Peer, Router, Server, connect_child, stdio

Server("echo", "1.0").on_request("echo", lambda p: p).serve()        # serial

peer = connect_child(["./srv"], Router(), serve_handshake=ours)      # drive one
peer.handshake(ours); peer.request("echo", {"text": "hi"})

Peer(router, serve_handshake=Hello("echo","1.0",["ui_ask"])).connect(stdio())  # reverse-capable
```

```ts
import { Peer, Router, Server, connectChild, stdio } from "lanok";
```

A reverse request needs `Peer`, not `Server`: a serial loop cannot wait for a
reply while producing one. Handlers take `params` or `(peer, params)` in Python,
`(params)` or `(params, peer)` in TypeScript.

`just matrix` runs every client against every server, nine combinations.

## References

- [`references/cookbook.md`](references/cookbook.md), recipes: reverse
  requests, progress, cancellation, timeouts, testing with `duplex()`.
- [`references/wire.md`](references/wire.md), the contract: framing, versions,
  capabilities, error codes.
- The repository's `docs/sdks.md` for the full Python and TypeScript guide.
