# Getting started

You are going to build a small protocol, serve it, and drive it. About ten
minutes.

## Install

```toml
[dependencies]
lanok = "0.1"
serde = { version = "1", features = ["derive"] }
```

The CLI, for artifacts and conformance runs:

```bash
cargo install lanok-cli   # installs the `lanok` binary
```

## 1. Declare the protocol

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct EchoParams {
    pub text: String,
    /// Optional, so a peer that predates this field still parses.
    #[serde(default)]
    pub shout: bool,
}

#[derive(Serialize, Deserialize)]
pub struct EchoResult {
    pub text: String,
}

lanok::protocol! {
    name    = "echo";
    version = "1.0";

    /// Transform some text and hand it back.
    initiator fn echo(EchoParams) -> EchoResult;
}
```

`initiator` means the side that opens the connection sends this method. You now
have `PROTOCOL_VERSION`, `NEGOTIATION`, `META`, `method::ECHO`, an `InitiatorApi`
trait of typed stubs, a `ResponderHandler` trait to implement, and a
`ResponderDispatch` that wires them together.

## 2. Write the server

```rust
use lanok::{RpcError, SimpleServer};

fn main() -> std::io::Result<()> {
    SimpleServer::new(PROTOCOL_NAME, PROTOCOL_VERSION)
        .on_request(method::ECHO, |params| {
            let params: EchoParams = serde_json::from_value(params)
                .map_err(|e| RpcError::invalid_params(format!("{e}")))?;
            let text = if params.shout { params.text.to_uppercase() } else { params.text };
            Ok(serde_json::json!({ "text": text }))
        })
        .serve_stdio()
}
```

No async runtime. `SimpleServer` reads a line, answers it, writes the answer.
The `initialize` handshake is answered for you.

> Keep stdout clean. Only protocol JSON belongs there; logging belongs on
> stderr. A server that prints anything else to stdout is the single most common
> authoring mistake, and `ChildTransport::skipped_lines` is how a host notices.

## 3. Drive it

```rust
use lanok::{ChildTransport, Hello, Peer};
use tokio::process::Command;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let transport = ChildTransport::spawn(Command::new("./my-server"))?;
    let ours = Hello::new("my-host", PROTOCOL_VERSION);

    let peer = Peer::builder().serve_handshake(ours.clone()).connect(transport);
    let server = peer.handshake(&ours, NEGOTIATION).await?;
    println!("connected to {} speaking {}", server.name, server.protocol_version);

    let result = peer.echo(EchoParams { text: "hi".into(), shout: true }).await?;
    assert_eq!(result.text, "HI");
    Ok(())
}
```

`peer.echo(..)` is the generated stub. It is typed, and it exists only on the
initiator side: calling a responder-only method here would not compile.

## 4. Commit the artifacts

Add a generator binary:

```rust
fn main() -> std::io::Result<()> {
    lanok::schema::Artifacts::new(concat!(env!("CARGO_MANIFEST_DIR"), "/schema/v1"))
        .run_cli(&my_protocol::schema_document(), "just schema")
}
```

Run it to write `schema/v1/{schema.json,meta.json}`, and run it with `--check`
in CI. Now a protocol change that does not regenerate the artifacts fails the
build.

```bash
lanok describe --schema schema/v1
```

## 5. Check every implementation against one suite

```bash
lanok vectors --schema schema/v1 > schema/v1/conformance.json   # a starting suite
lanok conform --vectors schema/v1/conformance.json -- ./my-server
```

Add cases as the protocol grows. The same file runs against a Python or
TypeScript implementation, which is what keeps them first-class.

## Where next

- [Declaring a protocol](declaring.md) for the full grammar, reverse requests,
  and capabilities.
- [Servers and peers](servers.md) for when `SimpleServer` is not enough.
- [The wire](wire.md) for the compatibility rules your protocol inherits.
