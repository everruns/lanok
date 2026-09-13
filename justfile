# Lanok development commands.
# Install just: cargo install just  (or `cargo binstall just`)
# Usage: just <recipe>   (or: just --list)

# Default: show available recipes.
default:
    @just --list

# === Build & test ===

# Build the whole workspace.
build:
    cargo build --workspace

# Build and install the local lanok CLI binary.
install:
    cargo install --path crates/lanok-cli --bin lanok --locked --force

# Run all tests.
test:
    cargo test --workspace --all-features

# === Lint & format ===

# Auto-fix formatting and clippy lints.
fmt:
    cargo fmt --all
    cargo clippy --all-targets --fix --allow-dirty --allow-staged 2>/dev/null || true

# Format-check, clippy (deny warnings), tests, and the schema drift guard.
# This is the CI gate.
check:
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features
    just knowledge
    # The committed artifacts under examples/echo/schema/ must match the
    # protocol declaration; regenerate with `just schema`.
    cargo run -q -p echo-protocol --bin echo-schema-gen -- --check

# OKF conformance for the knowledge bundle, plus intra-bundle link resolution.
knowledge:
    python3 scripts/validate_okf.py knowledge --check-links
    python3 scripts/test_validate_okf.py

# Regenerate the echo example's committed schema.json + meta.json. Run after
# changing its `protocol!` block.
schema:
    cargo run -q -p echo-protocol --bin echo-schema-gen

# Build the API docs with warnings denied (as CI does).
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features

# === Examples ===

# Drive the echo protocol end to end: a real client process spawns a real
# server process over stdio, exercising both directions of the peer.
#
# The client spawns the server binary, so both are built first: `cargo run
# --bin X` builds only X.
example:
    cargo build -q -p echo-protocol --bins
    cargo run -q -p echo-protocol --bin echo-client

# Run the blocking SimpleServer flavour of the same example (no async runtime
# in the server).
example-blocking:
    cargo build -q -p echo-protocol --bins
    cargo run -q -p echo-protocol --bin echo-client -- --blocking

# === SDKs ===

# Python SDK: generated wire types in sync with the schema, then the tests.
test-py:
    cargo run -q -p lanok-cli -- gen python \
        --schema examples/echo/schema/v1 --out sdks/python/lanok/_generated_echo.py --check
    cd sdks/python && python3 -m pytest -q

# TypeScript SDK: generated wire types in sync with the schema, build, tests.
test-ts:
    cargo run -q -p lanok-cli -- gen typescript \
        --schema examples/echo/schema/v1 --out sdks/typescript/src/generatedEcho.ts --check
    cd sdks/typescript && npm ci && npm test

# Replay the protocol's conformance vectors against every implementation:
# Rust, Python, and TypeScript. One suite, three servers.
conform: build-ts-sdk
    cargo build -q -p echo-protocol --bins
    cargo run -q -p lanok-cli -- conform \
        --vectors examples/echo/schema/v1/conformance.json -- ./target/debug/echo-server
    cargo run -q -p lanok-cli -- conform \
        --vectors examples/echo/schema/v1/conformance.json \
        -- python3 examples/echo-python/server.py
    cargo run -q -p lanok-cli -- conform \
        --vectors examples/echo/schema/v1/conformance.json \
        -- node examples/echo-typescript/server.mjs

# Build the TypeScript SDK's dist/, which the TS example server imports.
build-ts-sdk:
    cd sdks/typescript && npm ci && npm run build

# Every client against every server, across all three languages. Nine
# combinations, each including a reverse request.
matrix: build-ts-sdk
    cargo build -q -p echo-protocol --bins
    ./scripts/matrix.sh

# Describe a protocol from its committed artifacts.
describe:
    cargo run -q -p lanok-cli -- describe --schema examples/echo/schema/v1

# === Release ===

# Verify every publishable crate can be packaged (files, version drift).
#
# One --workspace invocation, not one per crate: at a version bump each crate's
# generated manifest drops the `path` of its internal deps, so a per-crate
# dry-run resolves `lanok-core = "^X.Y.Z"` against the crates.io index, where
# the new version does not exist yet. With --workspace cargo resolves the
# siblings being published together against the workspace instead.
publish-dry-run:
    cargo publish --dry-run --workspace

# Pre-PR gate: fmt, clippy, tests, drift. The publish dry-run is a release-time
# concern, so it stays out of the per-PR path.
pre-pr: check
    @echo "Pre-PR checks passed"
