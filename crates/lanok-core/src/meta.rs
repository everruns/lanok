//! The protocol vocabulary, as data.
//!
//! A `protocol!` declaration emits one of these. It is what `meta.json` is
//! serialized from, what the SDK generators read, and what a `doctor`
//! subcommand prints. Keeping it `&'static` means a protocol crate carries its
//! own description with no runtime construction and no allocation.
//!
//! These types are `Serialize` only. They describe a protocol compiled into a
//! binary, so they are written, never read: a tool that consumes `meta.json`
//! parses the owned mirror in `lanok-schema` instead.
//!
//! Direction names who **sends** a method, not who is what. `Initiator` is the
//! side that opens the connection and sends `initialize`; `Responder` is the
//! side that answers it. A method declared `Responder` is a reverse request,
//! which in lanok is an ordinary declaration rather than a special case.

use serde::Serialize;

use crate::Version;

/// Which side sends a method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Sent by the side that opened the connection.
    Initiator,
    /// Sent by the side that answered. A reverse message.
    Responder,
    /// Sent by either side. Rare, and worth being sure about before reaching
    /// for it: a method declared this way loses the compile-time role gating
    /// that makes calling one the wrong way an error rather than a runtime
    /// surprise. MCP's `ping`, `notifications/cancelled` and
    /// `notifications/progress` are the motivating real examples.
    Either,
}

/// Whether a method expects a response.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MethodKind {
    /// Carries an id and expects a response.
    Request,
    /// Fire and forget.
    Notification,
}

/// One method in a protocol's vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct MethodMeta {
    /// The name on the wire, e.g. `tool/call`.
    pub name: &'static str,
    pub direction: Direction,
    pub kind: MethodKind,
    /// The declaration's doc comment, so `meta.json` documents itself.
    #[serde(skip_serializing_if = "str::is_empty")]
    pub doc: &'static str,
    /// The capability token this method needs, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires: Option<&'static str>,
}

/// A protocol's full vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ProtocolMeta {
    pub name: &'static str,
    pub version: Version,
    /// The oldest peer version this build accepts.
    pub min_version: Version,
    pub methods: &'static [MethodMeta],
    /// Every capability token the protocol defines.
    pub capabilities: &'static [&'static str],
}

impl ProtocolMeta {
    /// Look one method up by its wire name.
    pub fn method(&self, name: &str) -> Option<&MethodMeta> {
        self.methods.iter().find(|m| m.name == name)
    }

    /// The methods one side may send, including the ones either side may.
    pub fn sent_by(&self, direction: Direction) -> impl Iterator<Item = &MethodMeta> {
        self.methods
            .iter()
            .filter(move |m| m.direction == direction || m.direction == Direction::Either)
    }

    /// The methods declared as sent by exactly `direction`, excluding
    /// [`Direction::Either`]. What a report showing one column at a time wants.
    pub fn declared_by(&self, direction: Direction) -> impl Iterator<Item = &MethodMeta> {
        self.methods
            .iter()
            .filter(move |m| m.direction == direction)
    }

    /// Whether the protocol declares any message the responder may send. A
    /// protocol where this is false is using one direction of a bidirectional
    /// peer, which is fine, and the answer is worth showing in a doctor report.
    pub fn is_bidirectional(&self) -> bool {
        self.sent_by(Direction::Responder).next().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const META: ProtocolMeta = ProtocolMeta {
        name: "demo",
        version: Version::new(1, 1),
        min_version: Version::new(1, 0),
        methods: &[
            MethodMeta {
                name: "tool/call",
                direction: Direction::Initiator,
                kind: MethodKind::Request,
                doc: "Invoke a tool.",
                requires: Some("tools"),
            },
            MethodMeta {
                name: "ui/ask",
                direction: Direction::Responder,
                kind: MethodKind::Request,
                doc: "",
                requires: None,
            },
        ],
        capabilities: &["tools"],
    };

    #[test]
    fn looks_methods_up_by_wire_name() {
        assert_eq!(META.method("tool/call").unwrap().requires, Some("tools"));
        assert!(META.method("nope").is_none());
    }

    #[test]
    fn reports_each_direction() {
        assert_eq!(META.sent_by(Direction::Initiator).count(), 1);
        assert_eq!(META.sent_by(Direction::Responder).count(), 1);
        assert!(META.is_bidirectional());
    }

    #[test]
    fn either_counts_for_both_sides() {
        const WITH_EITHER: ProtocolMeta = ProtocolMeta {
            methods: &[
                MethodMeta {
                    name: "run",
                    direction: Direction::Initiator,
                    kind: MethodKind::Request,
                    doc: "",
                    requires: None,
                },
                MethodMeta {
                    name: "ping",
                    direction: Direction::Either,
                    kind: MethodKind::Request,
                    doc: "",
                    requires: None,
                },
            ],
            ..META
        };
        // Either side may send `ping`, so it appears in both columns.
        assert_eq!(WITH_EITHER.sent_by(Direction::Initiator).count(), 2);
        assert_eq!(WITH_EITHER.sent_by(Direction::Responder).count(), 1);
        // And in neither when asking what was literally declared one-way.
        assert_eq!(WITH_EITHER.declared_by(Direction::Initiator).count(), 1);
        assert_eq!(WITH_EITHER.declared_by(Direction::Responder).count(), 0);
        // The responder can send `ping`, so the connection does carry traffic
        // in that direction, which is what the question means.
        assert!(WITH_EITHER.is_bidirectional());
    }

    #[test]
    fn either_serializes_as_itself() {
        assert_eq!(
            serde_json::to_value(Direction::Either).unwrap(),
            serde_json::json!("either")
        );
    }

    #[test]
    fn serializes_for_meta_json() {
        let value = serde_json::to_value(META).unwrap();
        assert_eq!(value["version"], "1.1");
        assert_eq!(value["methods"][0]["direction"], "initiator");
        assert_eq!(value["methods"][0]["kind"], "request");
        // An empty doc and an absent capability are omitted, so the artifact
        // stays readable instead of full of nulls.
        assert!(value["methods"][1].get("doc").is_none());
        assert!(value["methods"][1].get("requires").is_none());
    }
}
