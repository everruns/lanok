# Contributing to Lanok

Thanks for helping! Lanok is part of the [Everruns](https://github.com/everruns)
ecosystem.

## Development

```bash
git clone https://github.com/everruns/lanok
cd lanok
just check     # fmt + clippy -D warnings + tests + schema drift
just example   # drive the echo protocol end to end over a real child process
```

The whole workspace builds in seconds: nothing here pulls a heavy dependency
tree, and keeping it that way is a design constraint rather than an accident.

## Ground rules

- **Tests with behaviour.** New transports, peer behaviour, or macro output
  ship with tests. Fix bugs by first writing a failing test.
- **Keep `lanok-core` at serde.** No async, no I/O, no schemars outside the
  optional `schema` feature. A protocol crate must be publishable as types
  alone.
- **Keep `SimpleServer` free of tokio.** An extension author writing forty
  lines of handler should not compile an async runtime.
- **The wire is forward-compatible by construction.** Payloads never
  `deny_unknown_fields`; additions are `#[serde(default)]` and bump the minor.
  See [knowledge/specs/protocol-contract.md](knowledge/specs/protocol-contract.md).
- **Artifacts stay in lockstep.** Change a `protocol!` block, run `just schema`,
  commit the regenerated `schema.json` and `meta.json`. CI fails on drift.
- **Docs in sync.** User-facing changes update `docs/`; design changes update
  the [`knowledge/`](knowledge/index.md) bundle, and significant ones add an
  entry to [`knowledge/log.md`](knowledge/log.md).
- **Conventional commits.** e.g. `feat(peer): add request timeout`,
  `fix(transport): drain child stderr on close`.
- **No em-dashes** in prose, commits, or PR bodies. A comma, colon, or separate
  sentence says the same thing.

## Pull requests

1. Branch off the latest `main`.
2. `just check` is green.
3. Update `CHANGELOG.md` under `## [Unreleased]`.
4. Open the PR with a clear description of the change and its motivation.

See [AGENTS.md](AGENTS.md) for the architecture notes and the full checklist,
and [knowledge/specs/release-process.md](knowledge/specs/release-process.md) for
how releases ship.

## Branch protection (the merge gate)

`main` requires the `Check` status from `.github/workflows/ci.yml`. It rolls up
lint, audit, test, example, and SDK jobs, so a single required check covers the
whole gate. Squash and merge; never merge red CI.
