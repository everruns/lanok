# Knowledge Log

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
