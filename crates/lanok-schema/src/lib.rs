//! Schema and vocabulary artifacts for a lanok protocol, and the drift guard
//! that keeps them honest.
//!
//! A `protocol!` declaration generates a `schema_document()` behind the
//! protocol crate's own `schema` feature, so the artifacts describe exactly the
//! methods that were declared. Committing them makes the wire reviewable in a
//! diff and gives every non-Rust implementation something to validate against.
//!
//! A generator binary is then eight lines:
//!
//! ```no_run
//! # mod my_protocol {
//! #     pub fn schema_document() -> lanok_schema::Document { unimplemented!() }
//! # }
//! # use lanok_schema::Artifacts;
//! fn main() -> std::io::Result<()> {
//!     Artifacts::new(concat!(env!("CARGO_MANIFEST_DIR"), "/schema/v1"))
//!         .run_cli(&my_protocol::schema_document(), "just schema")
//! }
//! ```
//!
//! Reach it through the facade as `lanok::schema::Artifacts`, so a protocol
//! crate has one dependency rather than two to keep version-matched.
//!
//! Run it to regenerate; run it with `--check` in CI to fail on drift.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

mod artifacts;
mod document;
mod read;

pub use artifacts::{Artifacts, Drift, DriftReason, in_crate_dir};
pub use document::Document;
pub use read::{MethodEntry, ProtocolIndex};
