//! Regenerates the committed artifacts under `schema/v1`, and with `--check`
//! fails when the declaration has moved on without them.

fn main() -> std::io::Result<()> {
    lanok::schema::Artifacts::new(concat!(env!("CARGO_MANIFEST_DIR"), "/schema/v1"))
        .run_cli(&echo_protocol::schema_document(), "just schema")
}
