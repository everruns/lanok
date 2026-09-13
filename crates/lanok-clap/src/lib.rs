//! clap helpers, so every protocol binary describes and checks itself the same
//! way.
//!
//! The point is not to save a few lines of argument parsing. It is that a
//! generic tool, a support request, or a new contributor can point *any* lanok
//! server at `meta` or `doctor` and get an answer, without that server's author
//! having thought to provide one.
//!
//! ```no_run
//! use clap::Parser;
//!
//! # mod my_protocol {
//! #     pub const META: lanok_core::ProtocolMeta = lanok_core::ProtocolMeta {
//! #         name: "demo", version: lanok_core::Version::new(1, 0),
//! #         min_version: lanok_core::Version::new(1, 0),
//! #         methods: &[], capabilities: &[],
//! #     };
//! # }
//! #[derive(Parser)]
//! struct Cli {
//!     #[command(flatten)]
//!     transport: lanok_clap::TransportArgs,
//!     #[command(subcommand)]
//!     builtin: Option<lanok_clap::Builtin>,
//! }
//!
//! let cli = Cli::parse();
//! if let Some(builtin) = cli.builtin {
//!     builtin.run(my_protocol::META, None);
//!     return;
//! }
//! // ... otherwise serve on cli.transport
//! ```

#![forbid(unsafe_code)]

use clap::{Args, Subcommand, ValueEnum};
use lanok_core::{Direction, MethodKind, ProtocolMeta};
use serde_json::Value;

/// Which transport a binary should serve on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum TransportKind {
    /// This process's stdin and stdout, newline-delimited JSON.
    #[default]
    Stdio,
}

/// Transport selection flags.
///
/// One option today. It is a flag rather than an assumption because the
/// WebSocket and streaming-HTTP adapters are a planned addition, and a binary
/// built against this will accept them without changing its argument surface.
#[derive(Args, Clone, Debug, Default)]
pub struct TransportArgs {
    /// How to talk to the peer.
    #[arg(long, value_enum, default_value_t = TransportKind::Stdio)]
    pub transport: TransportKind,
}

/// Subcommands every lanok binary can offer for free.
#[derive(Subcommand, Clone, Debug)]
pub enum Builtin {
    /// Print this binary's JSON Schema.
    Schema,
    /// Print this binary's protocol vocabulary as JSON.
    Meta,
    /// Explain what this binary speaks, in prose.
    Doctor,
}

impl Builtin {
    /// Run the subcommand against a protocol's compiled-in metadata.
    ///
    /// `schema` is the generated document, when the binary was built with its
    /// `schema` feature. Without it, `schema` says so rather than printing
    /// nothing and looking broken.
    pub fn run(&self, meta: ProtocolMeta, schema: Option<&Value>) {
        match self {
            Builtin::Meta => println!(
                "{}",
                serde_json::to_string_pretty(&meta).expect("metadata is serializable")
            ),
            Builtin::Schema => match schema {
                Some(document) => println!(
                    "{}",
                    serde_json::to_string_pretty(document).expect("schema is serializable")
                ),
                None => {
                    eprintln!(
                        "this binary was built without its `schema` feature, so it carries no \
                         payload schemas; rebuild with --features schema"
                    );
                    std::process::exit(1);
                }
            },
            Builtin::Doctor => print!("{}", report(meta)),
        }
    }
}

/// A human-readable description of what a binary speaks.
pub fn report(meta: ProtocolMeta) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "protocol   {} {}\naccepts    {} and up, same major\n",
        meta.name, meta.version, meta.min_version
    ));
    out.push_str(&format!(
        "shape      {}\n",
        if meta.is_bidirectional() {
            "bidirectional (both sides send requests)"
        } else {
            "one direction (only the initiator sends requests)"
        }
    ));

    if meta.capabilities.is_empty() {
        out.push_str("capabilities  none declared\n");
    } else {
        out.push_str(&format!("capabilities  {}\n", meta.capabilities.join(", ")));
    }

    for (label, direction) in [
        ("initiator -> responder", Direction::Initiator),
        ("responder -> initiator", Direction::Responder),
    ] {
        let methods: Vec<_> = meta.sent_by(direction).collect();
        if methods.is_empty() {
            continue;
        }
        out.push_str(&format!("\n{label}\n"));
        for method in methods {
            let kind = match method.kind {
                MethodKind::Request => "request",
                MethodKind::Notification => "notify ",
            };
            let gate = match method.requires {
                Some(token) => format!("  [requires {token}]"),
                None => String::new(),
            };
            out.push_str(&format!("  {kind}  {}{gate}\n", method.name));
            if !method.doc.is_empty() {
                out.push_str(&format!("           {}\n", method.doc));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use lanok_core::{MethodMeta, Version};

    use super::*;

    const META: ProtocolMeta = ProtocolMeta {
        name: "echo",
        version: Version::new(1, 2),
        min_version: Version::new(1, 1),
        methods: &[
            MethodMeta {
                name: "echo",
                direction: Direction::Initiator,
                kind: MethodKind::Request,
                doc: "Transform some text.",
                requires: None,
            },
            MethodMeta {
                name: "ui/ask",
                direction: Direction::Responder,
                kind: MethodKind::Request,
                doc: "",
                requires: Some("ui_ask"),
            },
        ],
        capabilities: &["ui_ask"],
    };

    #[test]
    fn the_report_answers_what_a_reader_actually_asks() {
        let report = report(META);
        assert!(report.contains("echo 1.2"));
        assert!(report.contains("1.1 and up"));
        assert!(report.contains("bidirectional"));
        assert!(report.contains("initiator -> responder"));
        assert!(report.contains("responder -> initiator"));
        assert!(report.contains("[requires ui_ask]"));
        assert!(report.contains("Transform some text."));
    }

    #[test]
    fn a_one_direction_protocol_says_so() {
        const ONE_WAY: ProtocolMeta = ProtocolMeta {
            methods: &[MethodMeta {
                name: "run",
                direction: Direction::Initiator,
                kind: MethodKind::Request,
                doc: "",
                requires: None,
            }],
            capabilities: &[],
            ..META
        };
        let report = report(ONE_WAY);
        assert!(report.contains("one direction"));
        assert!(report.contains("none declared"));
        assert!(!report.contains("responder -> initiator"));
    }
}
