//! Regenerates the committed artifacts under `schema/v1`, and with `--check`
//! fails when the declaration has moved on without them.
//!
//! This repository has a `just schema` alias, so the drift message names it.
//! Without `regenerate_with`, the message derives the command from this
//! binary's own name, which is right for a project that has no alias.

fn main() -> std::io::Result<()> {
    lanok::schema::Artifacts::new(concat!(env!("CARGO_MANIFEST_DIR"), "/schema/v1"))
        .regenerate_with("just schema")
        .run_cli(&echo_protocol::schema_document())
}
