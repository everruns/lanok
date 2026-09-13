# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

### Changed

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
