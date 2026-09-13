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

1. `just check` is green on latest `main`.
2. `just conform` passes against all three implementations.
3. Move `CHANGELOG.md`'s `## [Unreleased]` entries under a new version heading
   with today's date.
4. Bump `version` in `[workspace.package]`, in every
   `[workspace.dependencies]` entry for an internal crate, in
   `sdks/python/pyproject.toml`, and in `sdks/typescript/package.json`.
5. `just publish-dry-run`.
6. Tag `v<version>` and push. CI publishes.

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

Dependencies first: `lanok-core`, `lanok-transport`, `lanok-peer`,
`lanok-macros`, `lanok-schema`, `lanok`, `lanok-clap`, `lanok-cli`. Each needs
its dependencies visible in the index before it can publish, so the CI job waits
between steps.

`echo-protocol` is `publish = false`: it is a worked example, not a library.
