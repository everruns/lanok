# Lanok

A kit for building JSON-RPC 2.0 protocols over stdio, and later over WebSocket
and streaming HTTP.

Lanok is not a protocol. It is what you build one *with*: the framing, the id
correlation, the version handshake, the capability gating, the schema
artifacts, and the Python and TypeScript SDKs, so the only thing your project
writes is the part that is actually yours, the methods and their payloads.

```
ланок, of links; ланка, a link in a chain
```

## The idea

There is no client type and no server type. There is one **symmetric `Peer`**,
and direction is a property declared on each *method*, not on the process.

<img src="https://raw.githubusercontent.com/everruns/lanok/main/docs/assets/lanok-peer.svg" alt="Two peers over one connection: peer A sends echo and ping and answers ui/ask, while peer B sends ui/ask and answers echo and ping, so requests and responses flow in both directions at once" width="720" />

Both sides send. Both sides answer, over one connection, with each direction
owning its own id space. Which methods go which way is declared, not built in:

```rust
lanok::protocol! {
    name = "echo";
    version = "1.0";

    /// Handshake: negotiate version and capabilities.
    initiator fn initialize(InitializeParams) -> InitializeResult;

    /// Ordinary forward call.
    initiator fn echo(EchoParams) -> EchoResult;

    /// A reverse call: the server asks the client a question mid-request.
    responder fn "ui/ask" ui_ask(AskParams) -> AskResult requires "ui_ask";

    capabilities { ui_ask }
}
```

That block generates role-gated typed stubs, a handler trait with an exhaustive
dispatcher, capability gating that short-circuits before touching the wire, and
the method vocabulary as data for the schema artifacts. A reverse request is an
additive declaration, never a redesign, because a line is classified by its
fields (`method` present means request or notification, absent means response)
and never by which pipe it arrived on.

## Install

```bash
cargo add lanok
```

The CLI, for SDK codegen and conformance runs:

```bash
cargo install lanok-cli   # installs the `lanok` binary
```

## A server

```rust
use lanok::{RpcError, SimpleServer};

fn main() -> std::io::Result<()> {
    SimpleServer::new("echo", "1.0")
        .capability("ui_ask")
        .on_request("echo", |params| {
            let text = params.get("text").and_then(|v| v.as_str()).unwrap_or_default();
            Ok(serde_json::json!({ "text": text.to_uppercase() }))
        })
        .serve_stdio()
}
```

`SimpleServer` is blocking and serial, and compiles no async runtime. Servers
that need many requests in flight at once use `Peer` instead, with the same
generated dispatch, so promoting one to the other touches no handlers.

## A client

```rust
use lanok::{Peer, transport::ChildTransport};

let transport = ChildTransport::spawn(tokio::process::Command::new("./my-server")).await?;
let peer = Peer::builder().connect(transport);
let hello = peer.handshake("echo", "1.0").await?;
let result = peer.request("echo", serde_json::json!({ "text": "hi" })).await?;
```

## What ships

| Crate | Role |
|-------|------|
| `lanok` | facade: re-exports the pieces below |
| `lanok-core` | wire types, ids, errors, version negotiation. serde only |
| `lanok-transport` | `Transport` trait: stdio, child process, in-memory duplex |
| `lanok-peer` | the symmetric peer, and the blocking `SimpleServer` |
| `lanok-macros` | the `protocol!` declaration macro |
| `lanok-schema` | `schema.json` + `meta.json` emission, drift guard |
| `lanok-clap` | `TransportArgs` and builtin subcommands |
| `lanok-cli` | the `lanok` binary: `gen`, `conform` |

Python and TypeScript runtimes live in [`sdks/`](https://github.com/everruns/lanok/blob/main/sdks), so a protocol's
non-Rust SDK is generated types on a shared peer loop rather than a hand-rolled
JSON-RPC loop per protocol per language. Both languages ship the same two shapes
Rust has, so either can be **either end** of a connection, reverse requests
included. `just matrix` runs all nine client/server combinations across the
three languages.

## Documentation

- [docs/](https://github.com/everruns/lanok/blob/main/docs/README.md), the public guides: getting started, the `protocol!`
  grammar, servers and peers, the wire, and Python and TypeScript.
- [knowledge/](https://github.com/everruns/lanok/blob/main/knowledge/index.md), the OKF bundle: architecture, contracts,
  and process, the design of record.
- [CONTRIBUTING.md](https://github.com/everruns/lanok/blob/main/CONTRIBUTING.md), how to build and what the merge gate is.

## License

MIT. Part of the [Everruns](https://github.com/everruns) ecosystem.
