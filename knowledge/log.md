# Knowledge Log

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
