# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **A handler gets the request it is answering.** Every handler now takes a
  `Context` as its first argument: `cx.id()` is the id of the request being
  answered (`None` in a notification handler), `cx.notify` emits against that
  id before the response lands, and `cx.supports` / `cx.peer` read the peer's
  handshake. A handler could previously see its method name and its params,
  which is everything about a request except which request it is. A protocol
  that streams progress has to name the request the progress belongs to, and
  one whose `cancel` aborts an in-flight call has to find that call by id, so
  both of lanok's own protocols answered by abandoning the generated dispatch
  and writing a serve loop by hand.

  The context is on every method, including those that ignore it, which take
  `_cx`. `Router` gains `on_request_with` and `on_notification_with` for the
  hand-dispatch path, leaving `on_request` as the short form. `SimpleServer`'s
  `Context` is this same type, so promoting a server between the two shapes
  really does leave its handlers alone; its `peer_supports` is now `supports`,
  matching `Peer::supports`. `Context::detached()` builds one attached to
  nothing, for calling a handler straight from a test.

- **One representation of the peer's handshake.** `Peer` stored the handshake
  in a `PeerInfo` summary that carried name, version and capabilities, and
  `SimpleServer` handed handlers the `Hello` itself. Two types for one fact,
  and the summary was lossy: `Hello::info` is the free-form field a protocol
  puts everything else in, and every path through the async peer dropped it,
  the handshake it sent and the one it served alike. `PeerInfo` is gone;
  `Peer::peer_info` and `Peer::record_peer` speak `Hello`. `peer_info` returns
  `Option<Hello>`, so "the handshake has not happened" is now distinct from
  "a peer that named itself the empty string".

### Added

- **`MethodMeta` carries each method's payload type names**, so `meta.json`
  says what a method takes and answers, not only that it exists. An SDK
  generator reading the artifact could previously emit a string constant and a
  dictionary; it can now emit a typed method. The value is the declared type's
  last path segment, which is the key `schema.json`'s `$defs` are under, so the
  two artifacts join up. Adding the fields changes any hand-written
  `MethodMeta` literal.

- **`Peer::shutdown`**, which ends a connection and waits for it. Dropping
  every handle also ends one, but says nothing about *when*, and `close` on a
  child-process transport is what shuts stdin, waits out the exit grace and
  drains stderr. A caller that needs the child reaped before it returns could
  not observe any of that from a drop: `closed()` only ever fired on the
  remote's EOF, because the local close it was meant to observe requires the
  last handle to be gone, and then there is nobody left to await it. Found
  migrating mira's host, whose `shutdown()` awaits the study's exit.

- **Documented capability tokens.** `capabilities { ... }` accepts a doc
  comment per token, and the declaration's prose becomes the generated const's
  doc rather than a generated one-liner. A capability token is a promise about
  behaviour, and that promise is what the other side needs; without this, moving
  a protocol's tokens into `protocol!` traded its own prose for a placeholder. A
  token declared twice is now rejected on the declaration instead of failing as
  a duplicate const in generated code.
- **Interop test against `rmcp`**, the official Rust MCP SDK: the MCP
  declaration in `experiments/foreign-protocols/` now holds a full session with
  a real third-party implementation, covering the handshake, `ping`,
  `tools/list`, `tools/call`, a reverse `elicitation/create` and inbound
  `notifications/progress`. Payloads are rmcp's own model types, so rmcp is the
  one judging the wire. Lanok is still not an MCP client.

- **`Direction::Either`**: a method both sides may send. Its stubs live on a
  separate `SharedApi` trait so importing both roles cannot make a call
  ambiguous, and both handler traits carry it because it can arrive from either
  side. It gives up compile-time role gating, so it is a deliberate third
  option, not the default. Motivated by MCP's `ping`,
  `notifications/cancelled` and `notifications/progress`, which have no
  direction.
- `ProtocolMeta::declared_by`, the counterpart to `sent_by`: what a method was
  literally declared as, so a report lists an `either` method once rather than
  in both columns.

- **A symmetric `Peer` in the Python and TypeScript SDKs.** Both languages can
  now be either end of a connection, reverse requests included, rather than
  only responding. Python uses threads so `peer.request(...)` returns a value
  in a plain script; TypeScript uses promises. Each ships transports to match:
  this process's stdio, a spawned child with its stderr drained, and an
  in-memory `duplex()` for tests.
- `scripts/matrix.sh` and `just matrix`: every client against every server
  across all three languages, nine combinations, each including a reverse
  request. Runs in CI.
- `docs/sdks.md`, the public guide to writing servers **and** clients in Python
  and TypeScript.
- `echo-client --server <cmd>` drives an arbitrary implementation, which is how
  the Python and TypeScript servers are exercised against the Rust client.

### Added

- `Peer::handshake_with`, `Peer::record_peer`, and
  `PeerBuilder::handshake_methods`: a protocol can run a handshake with its own
  payload types and its own method names, and still get capability gating.
  `Peer::handshake` remains the convention for new protocols. `protocol!` no
  longer refuses to declare `initialize` / `initialized`, since a protocol whose
  handshake payloads are its own has to be able to type them.
- `experiments/foreign-protocols`: YEP, ACP and MCP declared from their real
  method surfaces, run in CI. See `knowledge/specs/foreign-protocols.md`.

### Changed

- **Cancellation is a hook, not a setting.** `PeerBuilder::on_abandon` hands the
  protocol the abandoned request's id and method, and whether the caller timed
  out or dropped, and lets it decide what goes on the wire. The old
  `cancel_notification(name)` encoded one protocol's answer as the contract:
  always a notification, always `{"id": n}`, always armed for every request.
  mira needs an acknowledged request armed per method and gated on a
  capability; LSP and MCP each differ again. `cancel_notification` remains as a
  convenience over the hook.

- **`RpcError::retryable` is a top-level field**, not a key inside `data`. It
  is omitted when false, so it changes no existing wire. The original placement
  argued that JSON-RPC enumerates the members of an error object; the spec says
  `code` and `message` are required and `data` is optional, and does not forbid
  more. Both protocols this kit serves already carried the flag at the top
  level, so the strict reading bought nothing and cost every consumer a wire
  break.

- `lanok-core` no longer enables `serde_json/preserve_order`. Cargo unifies
  features across the dependency graph, so it was imposing insertion-ordered
  JSON maps on every consumer: taking the dependency reordered every schema
  artifact mira generates, with identical content. Found while adopting
  lanok-core in mira.

- `Artifacts::run_cli` no longer takes a regenerate-command string. The command
  a drift failure tells the reader to run is derived from the generator
  binary's own name, so it is correct in any project without being configured.
  Projects with a shorter way in opt into it with
  `.regenerate_with("just schema")`. The old signature hardcoded lanok's own
  justfile recipe into every user's failure message.
- `specs/` is now the [`knowledge/`](knowledge/index.md) Open Knowledge Format
  bundle, matching the sibling Everruns repositories: typed frontmatter on
  every concept, a reserved `index.md` and `log.md`, and CI validation of
  conformance and intra-bundle links.

### Fixed

- CI built only `echo-client` before running it, so the example died spawning
  an unbuilt `echo-server`; and `npm ci` ran without a committed lockfile.
- The getting-started guide showed a schema generator's `fn main` with none of
  the plumbing around it: no feature, no `[[bin]]`, no `JsonSchema` derives, and
  no command to run it.

## [0.1.0] - 2026-09-13

Initial release.

### Added

- `lanok-core`: JSON-RPC 2.0 wire types with field-based classification, id
  allocation, `RpcError` with standard codes plus a `retryable` hint,
  `MAJOR.MINOR` version negotiation, and capability tokens. Depends on serde
  alone: no async, no I/O.
- `lanok-transport`: the frame-oriented `Transport` trait with adapters for
  ndjson over this process's stdio, a spawned child process (with stderr
  drained to a sink), and an in-memory duplex pair for tests.
- `lanok-peer`: the symmetric `Peer`. One reader task, one writer task, a
  pending map keyed by id, per-request timeouts, cancel-on-drop, and a handshake
  driver that negotiates version and capabilities. Requests flow in both
  directions over one connection. `SimpleServer` is the blocking serial
  alternative for servers that must not compile an async runtime.
- `lanok-macros`: the `protocol!` declaration macro. One block emits typed
  role-gated stubs, a handler trait with an exhaustive dispatcher, capability
  gating, and the method vocabulary as data.
- `lanok-schema`: JSON Schema and `meta.json` emission from the declaration,
  plus `assert_no_drift!` so a protocol change cannot land without regenerating
  the committed artifacts.
- `lanok-clap`: `TransportArgs` and the builtin `schema` / `meta` / `doctor`
  subcommands, so every server binary describes and self-tests itself.
- `lanok-cli`: the `lanok` binary. `gen` emits Python and TypeScript SDK wire
  types from committed schema artifacts; `conform` replays conformance vectors
  against a server process in any language.
- Python and TypeScript SDK runtimes (`sdks/`): framing, peer loop, and
  dispatch written once per language, with generated protocol types on top.
- `examples/echo`: a two-direction protocol driven end to end in CI, over a
  real child process, in both the async and blocking server flavours.
