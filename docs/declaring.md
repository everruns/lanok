# Declaring a protocol

One `protocol!` block is the single source of truth: stubs, handlers, dispatch,
the vocabulary, and the schema artifacts all come from it.

## Grammar

```rust
lanok::protocol! {
    name    = "echo";       // required
    version = "1.2";        // required, MAJOR.MINOR
    min     = "1.1";        // optional; defaults to MAJOR.0

    /// Doc comments ride along into meta.json.
    initiator fn echo(EchoParams) -> EchoResult;
    initiator fn ping();
    responder notify "echo/progress" progress(ProgressParams);
    responder fn "ui/ask" ui_ask(AskParams) -> AskResult requires "ui_ask";

    capabilities { ui_ask }
}
```

Each method reads: **who sends it**, **whether it expects a reply**, an optional
**wire name** when the Rust identifier cannot spell it, the **identifier**, its
**params**, its **result**, and an optional **capability** it needs.

| Piece | Values |
|-------|--------|
| direction | `initiator` (the side that opens the connection) or `responder` |
| kind | `fn` (expects a response) or `notify` (fire and forget) |
| wire name | a string literal before the identifier, e.g. `"tool/call"` |
| params | a type in the parens, or empty |
| result | `-> Type`, or omitted |
| gating | `requires "token"` |

## What it generates

| Item | Purpose |
|------|---------|
| `PROTOCOL_NAME`, `PROTOCOL_VERSION`, `MIN_PROTOCOL_VERSION` | constants |
| `NEGOTIATION` | hand straight to `Peer::handshake` |
| `META` | the vocabulary as data, serialized to `meta.json` |
| `method::*`, `capability::*` | names, so no call site spells one as a string |
| `InitiatorApi`, `ResponderApi` | typed stubs, implemented for `Peer` |
| `InitiatorHandler`, `ResponderHandler` | what each side answers |
| `InitiatorDispatch`, `ResponderDispatch` | adapters onto `lanok::Handler` |
| `schema_document()` | behind your crate's `schema` feature |

## Role gating is by trait

The stubs for each direction live on a different trait, both implemented for
`Peer`. Importing `InitiatorApi` gets you the initiator's methods and not the
responder's, so calling a method in the wrong direction **does not compile**
rather than failing at runtime on the far side.

## Handlers refuse, they do not hang

Every method on a handler trait defaults to `method not found`. Implement only
what you answer:

```rust
#[lanok::async_trait]
impl ResponderHandler for MyServer {
    async fn echo(&self, params: EchoParams) -> Result<EchoResult, RpcError> {
        Ok(EchoResult { text: params.text })
    }
    // `ping` is left unimplemented and refuses politely.
}
```

## Capabilities gate locally

`requires "ui_ask"` means the stub checks `peer.supports("ui_ask")` **before
touching the wire** and returns `CAPABILITY_UNSUPPORTED` if the peer never
advertised it. No round trip is wasted discovering that an older peer cannot do
something.

A gated *notification* is dropped instead of refused: a notification carries no
id, so there is nobody to report a refusal to.

## What the macro rejects

These are compile errors because each of them otherwise produces something
subtly wrong:

- **A duplicate method.** Two declarations, one wire name.
- **Declaring `initialize` or `initialized`.** They are the shared handshake;
  shadowing them shadows version and capability negotiation.
- **A notification with a result.** It carries no id, so there is nothing to
  answer.
- **A `min` that is not a possible minimum**: a different major (peers across a
  major cannot talk at all), or newer than `version`.
- **`requires` naming a capability absent from the `capabilities` block.** This
  one matters most: a typo fails *closed* at runtime, making the method
  permanently and silently unavailable.

## Reverse requests

`responder fn` is a reverse request: the server asks the client something. It
needs no special framing, because a message is classified by its fields rather
than by direction, and each direction has its own id space.

The responder must be a concurrent `Peer`, not a `SimpleServer`: a serial loop
cannot wait for a reply while it is busy producing one. See
[servers](servers.md).
