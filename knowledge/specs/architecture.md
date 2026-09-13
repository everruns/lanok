---
type: Architecture Specification
title: Lanok Architecture Specification
description: Defines the crate split, the symmetric peer, and why each seam in lanok sits where it does.
---

# Architecture

The design of record for how lanok is put together, and why each seam sits
where it does.

## The one idea

There is no client type and no server type. There is one symmetric `Peer`, and
**direction is a property declared on each method, not on the process**.

This follows from how a message is classified. A line bearing `method` is a
request (with `id`) or a notification (without); a line without `method` is a
response. Nothing in that rule mentions which pipe the line arrived on. So a
protocol that starts out one-directional can grow a reverse request without
changing its framing, its parser, or its version major: the change is a
declaration, not a redesign.

Every other decision here is downstream of that.

## Crate split

```
lanok-core        wire types. serde only. no async, no I/O.
lanok-transport   the Transport trait and its adapters.
lanok-peer        the symmetric peer, and SimpleServer.
lanok-macros      the protocol! declaration macro.
lanok-schema      schema.json + meta.json emission, drift guard.
lanok-clap        optional clap helpers.
lanok-cli         the `lanok` binary: gen, conform, describe.
lanok             facade; what a protocol crate depends on.
```

The split is driven by two audiences that must not pay for each other:

- **A protocol crate publishing payload types** must not drag in a peer, a
  runtime, or a schema generator. Hence `lanok-core` at serde, with schemars
  behind an optional feature. CI asserts this directly by inspecting the
  dependency graph rather than trusting review.

  The rule covers *features*, not only dependencies. Cargo unifies features
  across the whole graph, so a feature `lanok-core` enables on a shared crate
  is one it imposes on every consumer. `serde_json/preserve_order` was enabled
  here once and silently reordered every JSON artifact mira generated, with
  identical content, the moment mira took the dependency. Enable nothing on a
  shared dependency that the core does not itself require.
- **An extension author writing forty lines of handler** must not compile an
  async runtime. Hence `SimpleServer` in `lanok-peer` behind default features,
  with tokio arriving only with the `async` feature.

`lanok-macros` is separate because a proc-macro crate cannot export anything
else. `lanok-cli` is separate because a codegen driver is not a runtime
dependency of anything.

## Transport

The trait is **frame-oriented**, not byte-oriented: an adapter hands the peer
one whole message and takes one whole message back. Framing is therefore the
adapter's problem, which is what lets ndjson-over-stdio and one-JSON-per-frame
over WebSocket be the same trait rather than two shapes with a shim between.

`Transport::recv` must be **cancellation safe**. The peer drives it inside a
`select!` against its outbound queue, so the future is dropped every time a
write wins the race. An adapter that loses buffered input on drop silently eats
messages under load, which is close to undiagnosable from outside. This is why
the ndjson adapter owns its read buffer instead of using `tokio::io::Lines`.

Planned adapters: WebSocket, and streaming HTTP (POST out, SSE in). The second
requires a **resumable pending map**, since request ids must survive a
reconnected stream. That constraint is designed in now rather than retrofitted.

## The peer

One task owns the transport and selects between the next inbound message and
the next queued outbound one. No lock, no split transport. Inbound requests are
dispatched onto their own tasks, so a slow handler never blocks the reader.

Two ownership rules, both of which exist because violating them produced hangs:

- The pump holds a **weak** reference to the peer. A strong one keeps the
  outbound channel's sender count above zero forever, so dropping the last
  handle never closes the connection.
- An in-flight handler task also holds a weak reference. It may answer while
  the connection is up but must not keep it up, or one slow request pins the
  connection open long after every handle is gone.

The peer, not the handler, owns the handshake when configured to serve one.
Capability state lives on the peer, so answering there is what makes
`supports()` true on the responding side, which is exactly what a reverse
request needs to know.

## Mechanism here, policy in the protocol

Lanok owns what is genuinely the same for every protocol, and refuses to decide
what is not. The line is not always obvious, and cancellation is where it was
drawn wrong first.

Cancellation began as one builder setting, `cancel_notification("$/cancel")`: a
method name, always sent as a notification, always carrying `{"id": n}`, always
armed for every request. That is one point in a space with at least four
occupants. mira's `cancel` is an acknowledged *request*, armed only for
cancelable methods and only against a study that advertised the capability.
LSP's `$/cancelRequest` is a notification with `{id}`. MCP's
`notifications/cancelled` carries `{requestId, reason}`.

So the peer now provides the mechanism, [`AbandonHook`]: it notices that a
caller went away, and hands over the id, the method, and whether the caller
timed out or dropped. What goes on the wire, and whether anything does, belongs
to the protocol. `cancel_notification` survives as a three-line convenience
over the hook, because the LSP shape is common.

The general rule, worth applying before adding the next setting: if two real
protocols would fill a knob differently, it is not a knob, it is a hook. A
configuration option that only one of them can use is a guess wearing an API.

## Two server shapes

|                  | `SimpleServer`            | `Peer`                        |
|------------------|---------------------------|-------------------------------|
| concurrency      | one request at a time     | many, both directions         |
| runtime          | none, blocking std I/O    | tokio                         |
| reverse requests | no (notifications out)    | yes                           |
| cancellation     | n/a                       | cancel-on-drop                |

Both consume the same generated dispatch, so promotion is a move rather than a
rewrite. The `SimpleServer` limits are stated up front rather than discovered:
a serial loop cannot wait for a reply while it is busy producing one.

## Artifacts

`meta.json` is the vocabulary, `schema.json` the shapes. Both are generated
from the same `protocol!` declaration that generates the stubs, which is what
makes drift impossible rather than merely discouraged. They are committed and
guarded in CI.

`lanok-core`'s metadata types are `&'static` and serialize-only, because they
describe a protocol compiled into a binary. Tools that read `meta.json` need
owned types, so those live in `lanok-schema`.

## Non-goals

- **Not an MCP or ACP client.** Those arrive as `rmcp` and
  `agent-client-protocol`. Lanok serves protocols a project owns.
- **Not a transport abstraction for general RPC.** The trait exists to let one
  peer serve several wire substrates, not to be a networking library.
- **Not a schema language.** Payload types are Rust types; JSON Schema is
  emitted from them, never the other way around.
