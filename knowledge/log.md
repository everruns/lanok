# Knowledge Log

## 2026-09-14, A handler gets the request it is answering

- [Architecture](specs/architecture.md): every handler now takes a `Context`
  alongside its params, carrying the request's id, a `notify` that streams
  against that id, and the peer's handshake. The id is the part a handler
  cannot reconstruct, and both a protocol that reports progress and one whose
  `cancel` aborts an in-flight call need it. Without it, mira and YEP each
  abandoned the generated dispatch for a hand-written serve loop, so the kit
  was generating dispatch that the protocols it exists for could not use.
- It is on every method, ignored ones included. Declaring per method which want
  it would put a dispatch detail in the protocol's vocabulary, and make a
  handler that starts streaming a change to the declaration rather than to its
  own body.
- One `Context` across both server shapes, which is what the promotion claim
  between them was already asserting. `SimpleServer` handed one to its handlers
  and `Peer` handed its handlers nothing, so "promoting does not touch your
  handlers" was true of the direction nobody travels.
- The peer's handshake is now kept as the `Hello` that arrived rather than a
  three-field summary of it. The summary dropped `info`, which is the field the
  handshake exists to carry protocol-specific data in.

## 2026-09-13, A third party judges the wire

- [Foreign protocol fit](specs/foreign-protocols.md): lanok's MCP declaration
  now holds a full session with `rmcp`, the official Rust MCP SDK, over one
  in-memory pipe with nothing translating between them. Handshake, `ping`,
  `tools/list`, `tools/call`, a reverse `elicitation/create` and two
  `notifications/progress`, with every payload an `rmcp::model` type.
- Declaring a method surface says something about the macro. Only an
  implementation that never heard of lanok can say anything about the wire, so
  that is the test worth having, and it runs in CI.
- It does not make lanok an MCP client and must not be read that way. ACP has
  no equivalent test yet.

## 2026-09-13, A method may have no direction

- [Foreign protocol fit](specs/foreign-protocols.md): declaring MCP found the
  one thing lanok could not say, and `Direction::Either` is the answer. Stubs
  go on a separate `SharedApi` so importing both roles stays unambiguous; both
  handler traits carry the method and both dispatchers route it, because it can
  arrive from either side. All 25 MCP methods now declare.
- It gives up compile-time role gating, so it is a third option rather than the
  default. Reach for it when a method genuinely has no direction, not to save a
  declaration.
- Recorded alongside it: the experiment proves the method surface is
  expressible and says nothing about wire identity, which is a different claim
  needing a different test.

## 2026-09-13, The handshake is the protocol's, and MCP does not fit

- [Foreign protocol fit](specs/foreign-protocols.md): YEP, ACP, and MCP
  declared from their real method surfaces, in CI. ACP's
  `session/request_permission` is the same shape as yolop's `ui/ask` in a
  protocol with no connection to lanok, which is the strongest evidence the
  symmetric peer describes something real rather than two authors' habits.
- **MCP does not fit.** `ping`, `notifications/cancelled` and
  `notifications/progress` are bidirectional, and a lanok method has one
  direction. Recorded as a known limit, not scheduled: nothing we own needs a
  symmetric ping, and role gating is worth more than one.
- The experiment changed the design on the way. The handshake was lanok's, not
  the protocol's: payload shape and both method names were hardcoded, so mira
  (which answers `initialize` with its eval catalogue) and MCP (which completes
  with `notifications/initialized`) could not use it. `handshake_with`,
  `record_peer`, and `handshake_methods` fix that, and `protocol!` no longer
  refuses to declare the handshake.

## 2026-09-13, Cancellation is a hook, not a setting

- `cancel_notification(name)` encoded one protocol's answer as the contract:
  always a notification, always `{"id": n}`, always armed. mira needs an
  acknowledged request, armed per method and gated on a capability; LSP and MCP
  each differ again. Replaced by [`AbandonHook`], which hands the protocol the
  id, the method, and whether the caller timed out, and lets it decide what to
  send. `cancel_notification` remains as a convenience over it.
- The rule this generalizes into, now in [architecture](specs/architecture.md):
  if two real protocols would fill a knob differently, it is not a knob, it is
  a hook. A configuration option only one consumer can use is a guess wearing
  an API.

## 2026-09-13, lanok-core must not impose serde_json features

- Found while adopting lanok-core in mira: enabling `preserve_order` on
  serde_json in lanok's workspace switched *mira's* `serde_json::Map` from
  sorted to insertion order, because cargo unifies features across the whole
  dependency graph. Every committed schema artifact in mira reordered, with
  identical content.
- A wire-types crate has no business changing how its consumers serialize JSON.
  `preserve_order` is gone, and the rule generalizes: a feature lanok-core
  enables on a shared dependency is a feature it imposes on every downstream
  crate. See [architecture](specs/architecture.md).
- A lanok-core test had been asserting JSON key *order*, which only held while
  `preserve_order` was on. The contract is the key set; object order is not
  meaningful in JSON.

## 2026-09-13, Documentation surfaces and the diagram convention

- [Documentation](specs/documentation.md): five surfaces, each owning something
  the others do not, and the rule that `README.md` and `docs/` never link into
  `knowledge/`. The wire has two descriptions on purpose, one normative and one
  readable, and they move together.
- Diagrams are committed SVG under `docs/assets/`. The constraint worth writing
  down is that GitHub renders one file on both a white and a near-black page,
  so every text element sits on a fill the diagram draws. Free-floating slate
  labels read on one theme and disappear on the other, which makes a diagram
  silently useless for half its readers.

## 2026-09-13, Python and TypeScript became full peers

- [SDKs](specs/sdks.md): both runtimes now ship a symmetric `Peer` alongside the
  serial `Server`, so either language can be either end of a connection,
  reverse requests included. "No client peer" was recorded as a deliberate
  scope decision and is no longer the right one: a protocol declares direction
  per method, so an SDK that can only respond made the kit's organizing idea a
  Rust-only feature.
- Parity is now enforced by two mechanisms rather than one. The conformance
  suite runs against every server implementation, and `scripts/matrix.sh` runs
  every client against every server, nine combinations, each including a
  reverse request. A language that could only serve, or only drive, would be
  missing a row or a column.

## 2026-09-13, Lanok's initial knowledge bundle

- [Architecture](specs/architecture.md): the crate split is driven by two
  audiences that must not pay for each other, a protocol crate publishing
  payload types and an extension author writing a small handler. `lanok-core`
  is held at serde and `lanok-peer`'s blocking path at no async runtime, both
  asserted in CI against the dependency graph rather than left to review.
- [Protocol contract](specs/protocol-contract.md): classification is by field
  and never by direction, which is what makes a reverse request additive.
  `jsonrpc: "2.0"` is written outbound and required on nothing inbound. Lanok
  being pre-1.0 never licenses breaking a downstream wire.
- Two peer ownership rules exist because violating them produced hangs: the
  pump and in-flight handler tasks both hold weak references, so neither a
  dropped handle nor a slow request keeps a connection alive.
