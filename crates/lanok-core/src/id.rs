//! Request identifiers and their allocation.
//!
//! JSON-RPC 2.0 permits a string or a number as an id. Lanok *accepts* both,
//! so an off-the-shelf client that numbers its requests with strings
//! interoperates, and *emits* only numbers, so its own ids stay cheap to
//! allocate and compare. A peer therefore never has to reason about two id
//! shapes on the outbound path.
//!
//! Each direction owns its own id space. A server's `id: 1` and a client's
//! `id: 1` are unrelated requests, because a response is routed by the pending
//! map of whichever side originally sent the request.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// A JSON-RPC request id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(untagged)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Id {
    /// The shape lanok emits.
    Number(u64),
    /// Accepted from peers that number their requests with strings.
    Text(String),
}

impl Id {
    /// The numeric value, when this id is a number.
    pub fn as_number(&self) -> Option<u64> {
        match self {
            Id::Number(n) => Some(*n),
            Id::Text(_) => None,
        }
    }
}

impl From<u64> for Id {
    fn from(value: u64) -> Self {
        Id::Number(value)
    }
}

impl From<String> for Id {
    fn from(value: String) -> Self {
        Id::Text(value)
    }
}

impl From<&str> for Id {
    fn from(value: &str) -> Self {
        Id::Text(value.to_string())
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Id::Number(n) => write!(f, "{n}"),
            Id::Text(s) => write!(f, "{s}"),
        }
    }
}

/// Hands out the ids for one direction of one connection.
///
/// Starts at 1: a zero id is legal JSON-RPC but reads as "unset" in enough
/// implementations that avoiding it costs nothing and saves a support thread.
#[derive(Debug)]
pub struct IdAllocator(AtomicU64);

impl IdAllocator {
    pub fn new() -> Self {
        IdAllocator(AtomicU64::new(1))
    }

    /// The next id in this direction's space.
    pub fn next(&self) -> Id {
        Id::Number(self.0.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for IdAllocator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_monotonically_from_one() {
        let ids = IdAllocator::new();
        assert_eq!(ids.next(), Id::Number(1));
        assert_eq!(ids.next(), Id::Number(2));
        assert_eq!(ids.next(), Id::Number(3));
    }

    #[test]
    fn accepts_both_json_shapes() {
        let n: Id = serde_json::from_str("7").unwrap();
        assert_eq!(n, Id::Number(7));
        let s: Id = serde_json::from_str("\"req-7\"").unwrap();
        assert_eq!(s, Id::Text("req-7".into()));
    }

    #[test]
    fn emits_numbers_untagged() {
        assert_eq!(serde_json::to_string(&Id::Number(7)).unwrap(), "7");
        assert_eq!(
            serde_json::to_string(&Id::Text("a".into())).unwrap(),
            "\"a\""
        );
    }
}
