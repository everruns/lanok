//! Capability tokens.
//!
//! A capability is a bare string a peer advertises in the handshake. It is the
//! unit of optionality in a lanok protocol: a method that needs one is
//! unavailable, and a generated stub short-circuits before touching the wire,
//! until the peer says it is there. That keeps "this peer is older and cannot
//! stream" a typed local answer rather than a round trip ending in
//! `method not found`.
//!
//! Adding a token is a minor version bump. Removing one is a major bump.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// The set of capability tokens one peer advertised.
///
/// Ordered, so a serialized handshake is stable and diffable.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Capabilities(BTreeSet<String>);

impl Capabilities {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the peer advertised `token`.
    pub fn supports(&self, token: &str) -> bool {
        self.0.contains(token)
    }

    /// Add a token. Returns self, so declarations read as one expression.
    pub fn with(mut self, token: impl Into<String>) -> Self {
        self.0.insert(token.into());
        self
    }

    pub fn insert(&mut self, token: impl Into<String>) {
        self.0.insert(token.into());
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The tokens both peers advertised: what this connection can actually do.
    pub fn intersect(&self, other: &Capabilities) -> Capabilities {
        Capabilities(self.0.intersection(&other.0).cloned().collect())
    }
}

impl<S: Into<String>> FromIterator<S> for Capabilities {
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
        Capabilities(iter.into_iter().map(Into::into).collect())
    }
}

impl<'a> IntoIterator for &'a Capabilities {
    type Item = &'a str;
    type IntoIter = Box<dyn Iterator<Item = &'a str> + 'a>;
    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_only_what_was_advertised() {
        let caps = Capabilities::new().with("tools").with("streaming");
        assert!(caps.supports("tools"));
        assert!(!caps.supports("ui_ask"));
        assert_eq!(caps.len(), 2);
    }

    #[test]
    fn serializes_as_a_sorted_array() {
        let caps: Capabilities = ["streaming", "tools", "hooks"].into_iter().collect();
        assert_eq!(
            serde_json::to_string(&caps).unwrap(),
            r#"["hooks","streaming","tools"]"#
        );
    }

    #[test]
    fn intersection_is_what_the_connection_can_do() {
        let ours: Capabilities = ["tools", "streaming", "trace"].into_iter().collect();
        let theirs: Capabilities = ["tools", "ui_ask"].into_iter().collect();
        let shared = ours.intersect(&theirs);
        assert!(shared.supports("tools"));
        assert!(!shared.supports("streaming"));
        assert!(!shared.supports("ui_ask"));
    }

    #[test]
    fn empty_by_default() {
        assert!(Capabilities::new().is_empty());
    }
}
