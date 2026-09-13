# Servers and peers

Two shapes ship. Pick by what your server actually needs, not by what sounds
more capable.

|                  | `SimpleServer`          | `Peer`                        |
|------------------|-------------------------|-------------------------------|
| concurrency      | one request at a time   | many, in both directions      |
| runtime          | none, blocking std I/O  | tokio                         |
| reverse requests | no                      | yes                           |
| cancellation     | n/a                     | cancel-on-drop                |
| notifications    | yes, outbound           | yes, both ways                |
| fits             | tool servers, extensions| hosts, servers with real concurrency |

Handlers are signature compatible, so outgrowing one is a move rather than a
rewrite.

## SimpleServer

```rust
SimpleServer::new(PROTOCOL_NAME, PROTOCOL_VERSION)
    .capability("uppercase")
    .on_request(method::ECHO, |params| Ok(params))
    .serve_stdio()
```

Turn off the `async` feature and tokio leaves the dependency graph entirely.
That is the point: somebody writing forty lines of tool handler should not have
to compile a runtime or learn one.

Streaming progress while a request is open takes the context form:

```rust
.on_request_with(method::ECHO, |context, params| {
    for step in 1..=3 {
        context.notify(method::PROGRESS, json!({ "step": step, "of": 3 }));
    }
    Ok(params)
})
```

The notifications are written before the response, so a caller watching a long
request sees something happening.

`context.peer_supports(token)` tells you what the caller advertised, because the
handshake is answered by the server itself.

## Peer

```rust
let peer = Peer::builder()
    .handler(ResponderDispatch::new(MyServer))
    .serve_handshake(Hello::new(PROTOCOL_NAME, PROTOCOL_VERSION).capability("ui_ask"))
    .request_timeout(Duration::from_secs(30))
    .cancel_notification("$/cancel")
    .connect(transport);
```

- `serve_handshake` makes this peer answer `initialize` itself. Do this on any
  peer that receives connections: capability state lives on the peer, so this
  is what makes `peer.supports(..)` true on the *responding* side, which is
  exactly what a reverse request needs to know.
- `request_timeout` is off by default. Imposing one silently breaks the
  legitimate long call these protocols exist to carry.
- `cancel_notification` is opt-in because the method name belongs to your
  protocol. With it set, dropping a request future tells the peer to stop.

### Reverse requests

The server asks the client something, mid-handler, on the same connection:

```rust
#[lanok::async_trait]
impl ResponderHandler for MyServer {
    async fn echo(&self, params: EchoParams) -> Result<EchoResult, RpcError> {
        let peer = self.peer.get().expect("installed before serving");
        let answer = peer.ui_ask(AskParams { question: "shout?".into() }).await?;
        Ok(EchoResult {
            text: if answer.answer == "yes" { params.text.to_uppercase() } else { params.text },
        })
    }
}
```

The handler needs the peer, and the peer needs the handler, so the usual shape
is a `OnceLock` set immediately after `connect`.

## Transports

| Adapter | Use |
|---------|-----|
| `StdioTransport` | a server speaking on its own stdin and stdout |
| `ChildTransport` | a host driving a spawned server |
| `duplex()` | both sides in one test, no process at all |

`ChildTransport` drains the child's stderr from the moment it starts, because an
unread pipe blocks a chatty server forever at around 8 KiB of logging and the
symptom is a hang rather than an error. `spawn_logging` sends those lines
somewhere useful.

`close()` is a sequence, not a kill: it shuts stdin, gives the child a grace
period to exit, kills only if it overstays, then waits for the stderr drain. The
last step is what puts a dying server's final log lines in front of whoever is
debugging it.

## Testing

`duplex()` connects two peers in memory, so a host and a server, including a
reverse request between them, run in one `#[tokio::test]` with no process, no
ports, and no sleeping:

```rust
let (a, b) = duplex();
let client = Peer::builder().handler(InitiatorDispatch::new(Client)).connect(a);
let server = Peer::builder().handler(ResponderDispatch::new(Server)).connect(b);
```
