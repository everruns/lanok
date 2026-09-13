# Lanok Knowledge

This directory is Lanok's Open Knowledge Format (OKF) bundle: the durable
architecture, policy, and development-process memory used by maintainers and
coding agents.

Read this index first, then open only the concepts relevant to the task and
follow their links. Public documentation lives in [`README.md`](../README.md)
and [`docs/`](../docs/README.md); it must not link back into this internal
bundle.

## Architecture

- [Architecture](specs/architecture.md), the crate split, the symmetric peer,
  transports, and why each seam sits where it does.
- [SDKs](specs/sdks.md), the split between hand-written runtimes and generated
  protocol code, and the two mechanisms that hold them at parity.

## Contracts

- [Protocol contract](specs/protocol-contract.md), the framing, versioning,
  capability, and forward-compatibility rules every protocol built on lanok
  inherits. Binding from a protocol's own 1.0, independent of lanok's version.

## Engineering processes

- [Documentation](specs/documentation.md), which surface owns which fact, and
  the committed-SVG diagram convention.
- [Release process](specs/release-process.md), versioning across the workspace
  and both SDKs, and the crates.io publishing flow.
