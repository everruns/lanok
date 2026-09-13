//! Reading the artifacts back.
//!
//! `lanok-core`'s metadata types are `&'static` and serialize-only: they
//! describe a protocol compiled into a binary. A tool consuming `meta.json`
//! (the SDK generators, `lanok conform`) needs owned types instead, so they
//! live here rather than complicating the core.

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

/// One method, as read from `meta.json`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct MethodEntry {
    pub name: String,
    /// `initiator` or `responder`: who sends it.
    pub direction: String,
    /// `request` or `notification`.
    pub kind: String,
    #[serde(default)]
    pub doc: String,
    #[serde(default)]
    pub requires: Option<String>,
}

impl MethodEntry {
    pub fn expects_response(&self) -> bool {
        self.kind == "request"
    }

    pub fn sent_by_initiator(&self) -> bool {
        self.direction == "initiator"
    }

    /// A name safe to use as an identifier in a generated SDK, derived from the
    /// wire name: `tool/call` becomes `tool_call`.
    pub fn ident(&self) -> String {
        self.name
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect()
    }
}

/// A protocol's committed vocabulary and payload shapes.
#[derive(Clone, Debug, Deserialize)]
pub struct ProtocolIndex {
    pub name: String,
    pub version: String,
    pub min_version: String,
    pub methods: Vec<MethodEntry>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// The payload shapes from `schema.json`, keyed by method. Empty when only
    /// `meta.json` was loaded.
    #[serde(skip)]
    pub schema: Value,
}

impl ProtocolIndex {
    /// Load both artifacts from a directory such as `schema/v1`.
    pub fn load(dir: impl AsRef<Path>) -> std::io::Result<Self> {
        let dir = dir.as_ref();
        let meta = std::fs::read_to_string(dir.join("meta.json"))?;
        let mut index: ProtocolIndex = serde_json::from_str(&meta).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}/meta.json is malformed: {e}", dir.display()),
            )
        })?;

        let schema = std::fs::read_to_string(dir.join("schema.json"))?;
        index.schema = serde_json::from_str(&schema).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}/schema.json is malformed: {e}", dir.display()),
            )
        })?;
        Ok(index)
    }

    /// The methods one side sends.
    pub fn sent_by_initiator(&self) -> impl Iterator<Item = &MethodEntry> {
        self.methods.iter().filter(|m| m.sent_by_initiator())
    }

    pub fn sent_by_responder(&self) -> impl Iterator<Item = &MethodEntry> {
        self.methods.iter().filter(|m| !m.sent_by_initiator())
    }

    /// The `$defs` block, for a generator emitting named types.
    pub fn definitions(&self) -> &Value {
        static EMPTY: Value = Value::Null;
        self.schema.get("$defs").unwrap_or(&EMPTY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wire_name_becomes_a_usable_identifier() {
        let method = |name: &str| MethodEntry {
            name: name.into(),
            direction: "initiator".into(),
            kind: "request".into(),
            doc: String::new(),
            requires: None,
        };
        assert_eq!(method("tool/call").ident(), "tool_call");
        assert_eq!(method("echo").ident(), "echo");
        assert_eq!(method("ui/ask-now").ident(), "ui_ask_now");
    }
}
