# Cookbook

## Reverse request from inside a handler

The handler needs the peer and the peer needs the handler, so set a `OnceLock`
right after `connect`.

```rust
struct Server { peer: Arc<OnceLock<Peer>> }

#[lanok::async_trait]
impl ResponderHandler for Server {
    async fn echo(&self, params: EchoParams) -> Result<EchoResult, RpcError> {
        let peer = self.peer.get().expect("installed before serving");
        if peer.supports(capability::UI_ASK) {
            let answer = peer.ui_ask(AskParams { question: "shout?".into() }).await?;
            // ...
        }
        Ok(EchoResult { text: params.text })
    }
}

let peer = Peer::builder().handler(ResponderDispatch::new(server)).connect(transport);
slot.set(peer.clone()).unwrap();
```

## Progress while a request is open

Serial server:

```rust
.on_request_with(method::WORK, |context, params| {
    for step in 1..=3 { context.notify(method::PROGRESS, json!({ "step": step, "of": 3 })); }
    Ok(json!("done"))
})
```

Peer: call the generated notification stub, `peer.progress(ProgressParams { .. })`.

## Timeouts and cancellation

```rust
Peer::builder()
    .request_timeout(Duration::from_secs(30))
    .cancel_notification("$/cancel")   // your protocol's name for it
```

No timeout by default: a default would silently break the legitimate long call
these protocols exist to carry. With `cancel_notification` set, dropping a
request future tells the peer to stop working.

## Testing without a process

```rust
let (a, b) = duplex();
let client = Peer::builder().handler(InitiatorDispatch::new(Client)).connect(a);
let server = Peer::builder().handler(ResponderDispatch::new(Server)).connect(b);
```

Both sides in one `#[tokio::test]`. No ports, no sleeping, no child process.
Reverse requests work here exactly as they do over a pipe.

## Driving a child process

```rust
let transport = ChildTransport::spawn_logging(
    Command::new("./my-server"),
    Arc::new(|line: &str| eprintln!("[server] {line}")),
)?;
```

Always use the logging form when debugging: the child's stderr is the only
window into why it died. `transport.skipped_lines()` above zero means the server
is printing non-protocol output to stdout.

## Adding a method without breaking anyone

1. Add the declaration, bump the **minor**.
2. New payload fields get `#[serde(default)]`.
3. `just schema`, commit the artifacts.
4. Regenerate the SDKs.
5. Add a conformance case.

An older peer answers `method not found` for the new method, which is why the
addition is safe.

## Gating a method on a capability

```rust
responder fn "ui/ask" ui_ask(AskParams) -> AskResult requires "ui_ask";
capabilities { ui_ask }
```

The stub returns `CAPABILITY_UNSUPPORTED` locally when the peer never advertised
the token, so no round trip is spent discovering it. A gated notification is
dropped silently instead: no id, nobody to report to.

## A server in Python or TypeScript

```python
from lanok import Server
Server("echo", "1.0").on_request("echo", lambda p: {"text": p["text"].upper()}).serve()
```

```ts
await new Server("echo", "1.0")
  .onRequest("echo", (p) => ({ text: String(p.text).toUpperCase() }))
  .serve();
```

Both are serial, both answer the handshake for you, and neither can serve a
reverse request. Run the protocol's conformance suite against them.
