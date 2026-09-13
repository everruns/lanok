## Coding-agent guidance

### Style

Telegraph. Drop filler/grammar. Min tokens.

Prose in this repository, `knowledge/`, `docs/`, commit messages, and PR bodies
alike, uses no em-dashes: a comma, colon, or separate sentence says the same
thing without the AI tell.

### Critical thinking

Fix root cause. Unsure: read more code; if stuck, ask with short options.
Unrecognized changes: assume another agent; keep going. If it causes issues,
stop and ask.

### Principles

- Always work on top of the latest `main` from remote. In worktrees: fetch
  `origin/main`, then rebase before editing.
- Important decisions as comments on top of the relevant file/function.
- Code testable, smoke-testable, runnable locally.
- Small, incremental, PR-sized changes.
- No backward-compat needed pre-1.0 (internal code). The *wire* is the
  exception: the compatibility contract in `knowledge/specs/protocol-contract.md` binds
  every protocol built on lanok from its own 1.0, so lanok's own pre-1.0 status
  never licenses breaking a downstream wire.
- Write a failing test before fixing a bug.
- Everything runnable and tested, no theoretical code. Don't stop until e2e
  works; verify before declaring done.

### What lanok is

A kit for building JSON-RPC 2.0 protocols. Not a protocol itself, and not an
MCP or ACP client: those arrive as `rmcp` and `agent-client-protocol`. Lanok
serves protocols a project *owns*, today the yolop extension protocol (YEP) and
the mira eval protocol.

One organizing idea, and every design question resolves against it: **there is
no client type and no server type, only a symmetric `Peer`**. Direction is a
property declared on each method, not on the process. A role selects which
stubs are callable and which handler table is installed. This is why a reverse
request is an additive declaration rather than a redesign.

### Architecture at a glance

```
crates/lanok-core       wire types: envelopes, ids, RpcError, version
                        negotiation, capability tokens. Pure data. No I/O, no
                        async, serde only.
crates/lanok-transport  the Transport trait plus adapters: ndjson over stdio,
                        child process, in-memory duplex for tests.
crates/lanok-peer       the symmetric peer (reader task, pending map,
                        cancel-on-drop, handshake, dispatch) and SimpleServer,
                        a blocking serial loop with no async runtime.
crates/lanok-macros     the `protocol!` declaration macro.
crates/lanok-schema     schemars-backed schema.json + meta.json emission and
                        the drift guard.
crates/lanok-clap       optional clap helpers: TransportArgs, builtin
                        subcommands.
crates/lanok-cli        the `lanok` binary: gen (SDK codegen), conform.
crates/lanok            facade crate authors depend on; re-exports the rest.
examples/echo           a two-direction protocol exercised end to end in CI.
sdks/                   Python and TypeScript runtimes the generated protocol
                        code sits on. See knowledge/specs/sdks.md.
```

### Gotchas

- **`lanok-core` takes no dependency but serde.** No tokio, no async, no
  schemars outside the optional `schema` feature. A protocol crate must be
  publishable as types alone, without dragging in the peer.
- **`SimpleServer` must not pull tokio.** An extension author writing forty
  lines of tool handler should not compile an async runtime. If a change makes
  `lanok-peer`'s blocking path depend on tokio, the change is wrong.
- Payloads parse leniently: never `deny_unknown_fields`, always
  `#[serde(default)]` on additions. The drift guard enforces the artifacts, not
  the leniency, so review it by hand.
- `jsonrpc: "2.0"` is written on every outbound line and never required on an
  inbound one. Peers predating the field stay readable.
- Generated artifacts (`schema/`, SDK wire types) are committed and drift
  guarded. Change the protocol, run `just schema`, commit both.

### Knowledge

[`knowledge/`](knowledge/index.md) is this repository's Open Knowledge Format
bundle and the design of record. Read the index first, then only the concepts
the task touches. New code complies with them or proposes a change there.

| Concept | Description |
|---------|-------------|
| [architecture](knowledge/specs/architecture.md) | Crate split, the symmetric peer, transports, why the seams sit where they do |
| [protocol-contract](knowledge/specs/protocol-contract.md) | Versioning, capability negotiation, forward-compat rules every lanok protocol inherits |
| [sdks](knowledge/specs/sdks.md) | Python and TypeScript runtimes, codegen, the drift guard |
| [documentation](knowledge/specs/documentation.md) | Which surface owns which fact, and the committed-SVG diagram convention |
| [release-process](knowledge/specs/release-process.md) | Versioning, crates.io publishing flow |

When a change alters durable architecture, policy, or process, update the
affected concept in the same change, and add an entry to
[`knowledge/log.md`](knowledge/log.md) for a significant one. Transient plans
and source-level detail stay out of the bundle.

### Local dev

```bash
just --list     # all recipes
just build      # cargo build --workspace
just test       # cargo test --workspace
just check      # fmt --check + clippy -D warnings + test + schema drift
just pre-pr     # check
just example    # drive the echo example end to end over a real child process
just knowledge  # OKF conformance + intra-bundle links (when knowledge/ changed)
```

### Documentation

- **Public docs** live in `docs/`, indexed by `docs/README.md`.
- **API docs** are rustdoc on the crates; `cargo doc --no-deps --open` to
  preview. CI builds docs with `-D warnings`.
- **The agent skill** is `skills/lanok/`, installed by `./skills.sh`.

### Git and commits

- Conventional Commits: `type(scope): description`. Types: `feat`, `fix`,
  `docs`, `refactor`, `test`, `chore`. Use `chore` for `knowledge/`, `AGENTS.md`, or
  CI metadata.
- **Never add Claude/session/AI attribution** in commits, PRs, docs, or code
  comments (no `Co-Authored-By: Claude`, no "Generated with Claude Code").
- Commit attribution must be a real human user. `.claude/hooks/fix-git-identity.sh`
  (a SessionStart hook) sets it; if git identity is missing or agent-like, stop
  and ask before committing.
- Stage files explicitly by name. Avoid broad `git add .` / `git add -A`.

### Pre-PR checklist

- `just check` passes (fmt, clippy `-D warnings`, tests, schema drift).
- New behaviour has tests; the echo example still runs (`just example`).
- Public-facing changes update `docs/` and `CHANGELOG.md`.
- Specs updated if a design decision changed.
