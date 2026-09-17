---
type: Process Specification
title: Release Process Specification
description: Defines lanok's versioning scheme and the steps for publishing crates and SDK packages.
---

# Release process

## Versioning

Every crate in the workspace shares one version, set in
`[workspace.package]`. They implement one contract and are developed together;
independent versions would buy precision nobody needs and cost a lockstep
nobody can see.

The Python and TypeScript SDKs track the same number, for the same reason.

Pre-1.0, the Rust API may break on a minor. The *wire* contract in
[protocol-contract.md](protocol-contract.md) is separate and binds every
protocol built on lanok from its own 1.0.

## Cutting a release

Lanok has no pull requests, so there is no merge for a reviewer to gate on. The
gate is moved earlier and made explicit: **an agent asks a human to approve the
highlights, and nothing is written until they do.** After that the release is
mechanical, which is the right split, because everything after the prep commit
is irreversible in practice (crates.io has no unpublish, only yank).

The procedure lives in the [`release` skill](../../.claude/skills/release/SKILL.md).
In outline:

1. Every gate green on the commit being released: `just check`, `just conform`
   against all three implementations, `just publish-dry-run`.
2. The change set built mechanically from the commit log, reconciled against
   `CHANGELOG.md`'s `## [Unreleased]`.
3. **The human approves the version and the highlights.** Up to here nothing has
   been written, so cancelling costs nothing.
4. Only then: move `[Unreleased]` under a version heading with today's date,
   bump `version` in `[workspace.package]`, in every `[workspace.dependencies]`
   entry for an internal crate, in `sdks/python/pyproject.toml`, and in
   `sdks/typescript/package.json`; `cargo update -w`; commit
   `chore(release): prepare vX.Y.Z` directly to `main` and push.
5. CI does the rest, and the registries are verified rather than trusted.

## What CI does

`release.yml` triggers on a `chore(release): prepare vX.Y.Z` subject landing on
`main` (or a manual dispatch from `main`). It re-derives the version, refuses if
the commit subject, `Cargo.toml`, both SDK manifests and the internal dependency
requirements do not all agree, refuses if `CHANGELOG.md` has no section for it,
tags `vX.Y.Z`, creates the GitHub release with that section as its notes, and
dispatches `publish.yml`.

Those re-checks duplicate what the skill already did. That is deliberate: the
skill is one way to reach this state and a human with a terminal is another, and
the irreversible half should not depend on which one ran.

`publish.yml` publishes the crates in dependency waves and both SDKs, then polls
the index until every crate serves the new version. Every step skips what is
already published, so a release that fails halfway is finished by re-dispatching
the workflow rather than by hand.

## The README is rendered off-GitHub

`crates.io` renders `README.md` outside the repository, so every image and link
in it is absolute. A relative path works on GitHub and silently breaks on the
crate page, which is the copy most readers see first.

## Prerequisites

| What | Where | For |
|---|---|---|
| `CARGO_REGISTRY_TOKEN` | repository secret | the eight crates |
| `release` environment | repository settings | every publish job runs in it |
| PyPI trusted publisher | project `lanok` | owner `everruns`, repo `lanok`, workflow `publish.yml`, environment `release` |
| npm trusted publisher | package `@lanok/rpc` | the `lanok` scope must exist; same repo, workflow and environment |

Both SDK publishers use OIDC, so no npm or PyPI token is stored. They must be
registered before the first release, or the SDK jobs fail while the crates go
up, which is exactly the half-published state the idempotent steps exist to
avoid.

## Why the dry run is one workspace invocation

Per-crate dry runs cannot work at a version bump. Each crate's generated
manifest drops the `path` of its internal dependencies, so
`cargo publish -p lanok-peer` resolves `lanok-core = "^X.Y.Z"` against the
crates.io index, where the new version does not exist yet. It fails while
*packaging*, before any build, so `--no-verify` does not help either.

With `--workspace`, cargo resolves the siblings being published together
against the workspace, so everything packages and verifies. Publish order is
then cargo's job.

## Publish order

Each crate needs its internal dependencies resolvable in the index before its
own verify build runs, so publishing goes in waves with a wait between them:

1. `lanok-core`, `lanok-macros` (no internal dependencies)
2. `lanok-transport`, `lanok-schema`, `lanok-clap` (need `lanok-core`)
3. `lanok-peer` (needs `lanok-transport`), `lanok-cli` (needs `lanok-schema`)
4. `lanok` (the facade, needs all five libraries)

The SDKs publish independently of the crates and of each other. They are native
runtimes, not bindings, so a registry hiccup on one side must not block the
other; they carry the workspace version because they implement the same protocol
contract, not because they are built from the same source.

`echo-protocol` is `publish = false`: it is a worked example, not a library.

## Once a version is live

mira and yolop pin lanok as a git dependency while it is unpublished. After the
first release they move to version requirements, in those repositories.
