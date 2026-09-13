//! The `lanok` binary.
//!
//! Two jobs, both about making a protocol usable outside the crate that
//! declares it: generating SDK wire types from its committed artifacts, and
//! replaying its conformance vectors against an implementation in any
//! language.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use lanok_schema::ProtocolIndex;

mod codegen;
mod conform;

#[derive(Parser)]
#[command(name = "lanok", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate SDK wire types from a protocol's committed artifacts.
    Gen {
        /// `python` or `typescript`.
        #[arg(value_parser = parse_language)]
        language: codegen::Language,
        /// The artifact directory, e.g. `schema/v1`.
        #[arg(long)]
        schema: PathBuf,
        /// Where to write. Defaults to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Fail if the file on disk is not what would be generated, instead of
        /// writing. This is the drift guard for CI.
        #[arg(long)]
        check: bool,
    },

    /// Describe a protocol from its artifacts, in prose.
    Describe {
        /// The artifact directory, e.g. `schema/v1`.
        #[arg(long)]
        schema: PathBuf,
    },

    /// Replay conformance vectors against a server process.
    Conform {
        /// The suite to run.
        #[arg(long)]
        vectors: PathBuf,
        /// The server command, after `--`.
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },

    /// Print a starting conformance suite for a protocol that has none.
    Vectors {
        /// The artifact directory, e.g. `schema/v1`.
        #[arg(long)]
        schema: PathBuf,
    },
}

fn parse_language(raw: &str) -> Result<codegen::Language, String> {
    raw.parse().map_err(|e: anyhow::Error| e.to_string())
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Gen {
            language,
            schema,
            out,
            check,
        } => generate(language, &schema, out.as_deref(), check),

        Command::Describe { schema } => {
            let index = ProtocolIndex::load(&schema)
                .with_context(|| format!("could not load artifacts from {}", schema.display()))?;
            print!("{}", describe(&index));
            Ok(())
        }

        Command::Conform { vectors, command } => {
            let failed = conform::run(&vectors, &command)?;
            if failed > 0 {
                std::process::exit(1);
            }
            Ok(())
        }

        Command::Vectors { schema } => {
            let index = ProtocolIndex::load(&schema)
                .with_context(|| format!("could not load artifacts from {}", schema.display()))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&conform::template(&index.name, &index.version))?
            );
            Ok(())
        }
    }
}

fn generate(
    language: codegen::Language,
    schema: &std::path::Path,
    out: Option<&std::path::Path>,
    check: bool,
) -> Result<()> {
    let index = ProtocolIndex::load(schema)
        .with_context(|| format!("could not load artifacts from {}", schema.display()))?;
    let rendered = codegen::render(&index, language);

    let Some(path) = out else {
        print!("{rendered}");
        return Ok(());
    };

    if check {
        let committed = std::fs::read_to_string(path).unwrap_or_default();
        if committed == rendered {
            println!("{} is up to date", path.display());
            return Ok(());
        }
        eprintln!(
            "{} no longer matches {}\n\nthe protocol changed; regenerate with:\n  lanok gen {} \
             --schema {} --out {}",
            path.display(),
            schema.display(),
            match language {
                codegen::Language::Python => "python",
                codegen::Language::TypeScript => "typescript",
            },
            schema.display(),
            path.display(),
        );
        std::process::exit(1);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, &rendered)
        .with_context(|| format!("could not write {}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

/// The same report `lanok-clap`'s doctor prints, from artifacts rather than
/// from a compiled-in protocol, so it works on a protocol this build has never
/// heard of.
fn describe(index: &ProtocolIndex) -> String {
    let mut out = format!(
        "protocol   {} {}\naccepts    {} and up, same major\n",
        index.name, index.version, index.min_version
    );
    let reverse = index.sent_by_responder().count();
    out.push_str(&format!(
        "shape      {}\n",
        if reverse > 0 {
            "bidirectional (both sides send messages)"
        } else {
            "one direction (only the initiator sends)"
        }
    ));
    out.push_str(&format!(
        "capabilities  {}\n",
        if index.capabilities.is_empty() {
            "none declared".to_string()
        } else {
            index.capabilities.join(", ")
        }
    ));

    for (label, methods) in [
        (
            "initiator -> responder",
            index.sent_by_initiator().collect::<Vec<_>>(),
        ),
        (
            "responder -> initiator",
            index.sent_by_responder().collect::<Vec<_>>(),
        ),
    ] {
        if methods.is_empty() {
            continue;
        }
        out.push_str(&format!("\n{label}\n"));
        for method in methods {
            let kind = if method.expects_response() {
                "request"
            } else {
                "notify "
            };
            let gate = method
                .requires
                .as_ref()
                .map(|token| format!("  [requires {token}]"))
                .unwrap_or_default();
            out.push_str(&format!("  {kind}  {}{gate}\n", method.name));
            if !method.doc.is_empty() {
                out.push_str(&format!("           {}\n", method.doc));
            }
        }
    }
    out
}
