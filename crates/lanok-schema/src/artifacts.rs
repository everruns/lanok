//! Writing the artifacts, and failing when they drift.
//!
//! The committed `schema.json` and `meta.json` are generated, never hand
//! edited. A drift guard in CI is what makes that true in practice rather than
//! in the contributing guide: change a `protocol!` block without regenerating
//! and the build fails with the exact command to run.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::Document;

/// One artifact that no longer matches the declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    /// The file that is stale, or missing.
    pub path: PathBuf,
    pub reason: DriftReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftReason {
    Missing,
    Stale,
}

impl std::fmt::Display for Drift {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = match self.reason {
            DriftReason::Missing => "is missing",
            DriftReason::Stale => "no longer matches the protocol declaration",
        };
        write!(f, "{} {what}", self.path.display())
    }
}

impl std::error::Error for Drift {}

/// The committed artifact directory for one protocol, e.g. `schema/v1`.
#[derive(Debug, Clone)]
pub struct Artifacts {
    dir: PathBuf,
    regenerate_with: Option<String>,
}

impl Artifacts {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Artifacts {
            dir: dir.into(),
            regenerate_with: None,
        }
    }

    /// Override the command a drift failure tells the reader to run.
    ///
    /// The default is derived from this binary's own name, which is correct
    /// for anyone who has not aliased it. Set this only when your project has a
    /// shorter way in, such as a `just` recipe or an npm script: a failure
    /// naming a command the reader does not have is worse than one naming the
    /// long form.
    pub fn regenerate_with(mut self, command: impl Into<String>) -> Self {
        self.regenerate_with = Some(command.into());
        self
    }

    /// What a drift failure tells the reader to run.
    ///
    /// Derived from argv[0] rather than hardcoded, so the message is right in
    /// any project without that project configuring anything.
    fn regenerate_hint(&self) -> String {
        if let Some(command) = &self.regenerate_with {
            return command.clone();
        }
        std::env::args()
            .next()
            .as_deref()
            .map(Path::new)
            .and_then(Path::file_stem)
            .map(|name| format!("cargo run --bin {}", name.to_string_lossy()))
            .unwrap_or_else(|| "this protocol's schema generator".to_string())
    }

    pub fn schema_path(&self) -> PathBuf {
        self.dir.join("schema.json")
    }

    pub fn meta_path(&self) -> PathBuf {
        self.dir.join("meta.json")
    }

    /// Write both artifacts, creating the directory if needed.
    ///
    /// Returns the paths that actually changed, so a generator can say
    /// "already up to date" instead of touching mtimes for no reason.
    pub fn write(&self, document: &Document) -> io::Result<Vec<PathBuf>> {
        fs::create_dir_all(&self.dir)?;
        let mut changed = Vec::new();
        for (path, value) in [
            (self.schema_path(), document.schema()),
            (self.meta_path(), document.meta()),
        ] {
            let rendered = render(&value);
            if fs::read_to_string(&path).ok().as_deref() != Some(rendered.as_str()) {
                fs::write(&path, &rendered)?;
                changed.push(path);
            }
        }
        Ok(changed)
    }

    /// Every artifact that is missing or stale. Empty means in lockstep.
    pub fn drift(&self, document: &Document) -> Vec<Drift> {
        [
            (self.schema_path(), document.schema()),
            (self.meta_path(), document.meta()),
        ]
        .into_iter()
        .filter_map(|(path, value)| match fs::read_to_string(&path) {
            Err(_) => Some(Drift {
                path,
                reason: DriftReason::Missing,
            }),
            Ok(committed) if committed != render(&value) => Some(Drift {
                path,
                reason: DriftReason::Stale,
            }),
            Ok(_) => None,
        })
        .collect()
    }

    /// Run a generator's `main`: write the artifacts, or with `--check` report
    /// drift and exit non-zero.
    ///
    /// Every protocol crate needs this same eight lines of binary, so it lives
    /// here once. A drift failure names the command to run, which is the
    /// difference between a useful CI failure and a puzzle; see
    /// [`Artifacts::regenerate_with`] for where that command comes from.
    pub fn run_cli(&self, document: &Document) -> io::Result<()> {
        let regenerate_with = self.regenerate_hint();
        let check = std::env::args().any(|arg| arg == "--check");
        if check {
            let drift = self.drift(document);
            if drift.is_empty() {
                println!("artifacts in {} are up to date", self.dir.display());
                return Ok(());
            }
            for entry in &drift {
                eprintln!("drift: {entry}");
            }
            eprintln!("\nthe protocol declaration changed; run `{regenerate_with}`");
            std::process::exit(1);
        }

        let changed = self.write(document)?;
        if changed.is_empty() {
            println!("artifacts in {} are up to date", self.dir.display());
        } else {
            for path in changed {
                println!("wrote {}", path.display());
            }
        }
        Ok(())
    }
}

/// Pretty-printed with a trailing newline, so the committed file is diffable
/// and does not fight whatever editor opens it next.
fn render(value: &Value) -> String {
    let mut rendered = serde_json::to_string_pretty(value).expect("artifacts are serializable");
    rendered.push('\n');
    rendered
}

/// Resolve a path relative to the crate that calls it, so a generator works
/// from any working directory.
pub fn in_crate_dir(manifest_dir: &str, relative: impl AsRef<Path>) -> PathBuf {
    Path::new(manifest_dir).join(relative)
}
