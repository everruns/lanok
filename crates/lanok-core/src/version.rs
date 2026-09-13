//! `MAJOR.MINOR` version negotiation.
//!
//! The contract every lanok protocol inherits (see
//! `specs/protocol-contract.md`):
//!
//! * The **major** changes only on a breaking wire change. Different majors
//!   cannot talk.
//! * The **minor** increments for backwards-compatible additions: a new method,
//!   a new optional field, a new capability token. A newer peer tolerates a
//!   missing addition; an older peer ignores what it does not know.
//! * A build declares the oldest peer it still accepts, so dropping support for
//!   an ancient minor is an explicit act rather than a silent regression.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A parsed `MAJOR.MINOR` protocol version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "String"))]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32) -> Self {
        Version { major, minor }
    }

    /// Whether two versions can talk at all. Same major, minors are additive.
    pub fn compatible_with(self, other: Version) -> bool {
        self.major == other.major
    }
}

/// Why a version string could not be parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseVersionError(String);

impl fmt::Display for ParseVersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` is not a MAJOR.MINOR protocol version", self.0)
    }
}

impl std::error::Error for ParseVersionError {}

impl FromStr for Version {
    type Err = ParseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || ParseVersionError(s.to_string());
        let (major, minor) = s.split_once('.').ok_or_else(invalid)?;
        // Reject a third component rather than silently ignoring it: a peer
        // sending `1.2.3` has a different idea of the contract than we do, and
        // guessing which two numbers it meant is how skew becomes a bug report.
        if minor.contains('.') {
            return Err(invalid());
        }
        Ok(Version {
            major: major.parse().map_err(|_| invalid())?,
            minor: minor.parse().map_err(|_| invalid())?,
        })
    }
}

impl TryFrom<String> for Version {
    type Error = ParseVersionError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<Version> for String {
    fn from(value: Version) -> Self {
        value.to_string()
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// What one build implements, and the oldest peer it still accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Negotiation {
    /// The version this build implements and advertises.
    pub current: Version,
    /// The oldest version this build can still talk to.
    pub min: Version,
}

/// Why a peer's version was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Incompatible {
    /// Different major: the wire formats are not the same protocol.
    MajorMismatch,
    /// Same major, but older than the oldest minor this build supports.
    TooOld,
}

impl Negotiation {
    /// Declare a build that accepts any peer sharing its major.
    pub const fn new(current: Version) -> Self {
        Negotiation {
            current,
            min: Version::new(current.major, 0),
        }
    }

    /// Declare a build that has dropped support for the minors below `min`.
    pub const fn with_min(current: Version, min: Version) -> Self {
        Negotiation { current, min }
    }

    /// Whether this build can talk to a peer advertising `peer`.
    ///
    /// A *newer* minor is always accepted: by the additive contract, everything
    /// this build understands is still there, and what it does not understand
    /// it ignores.
    pub fn accepts(self, peer: Version) -> Result<(), Incompatible> {
        if peer.major != self.current.major {
            return Err(Incompatible::MajorMismatch);
        }
        if peer < self.min {
            return Err(Incompatible::TooOld);
        }
        Ok(())
    }
}

impl fmt::Display for Incompatible {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Incompatible::MajorMismatch => write!(f, "incompatible major version"),
            Incompatible::TooOld => write!(f, "older than the oldest supported version"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u32, minor: u32) -> Version {
        Version::new(major, minor)
    }

    #[test]
    fn parses_and_renders() {
        assert_eq!("1.2".parse::<Version>().unwrap(), v(1, 2));
        assert_eq!(v(1, 2).to_string(), "1.2");
        assert_eq!(serde_json::from_str::<Version>("\"3.0\"").unwrap(), v(3, 0));
        assert_eq!(serde_json::to_string(&v(3, 0)).unwrap(), "\"3.0\"");
    }

    #[test]
    fn rejects_malformed_versions() {
        for bad in ["1", "1.2.3", "", "x.y", "1.", ".1", "-1.0"] {
            assert!(bad.parse::<Version>().is_err(), "{bad} should not parse");
        }
    }

    #[test]
    fn same_major_talks_across_minors() {
        let build = Negotiation::new(v(1, 1));
        assert_eq!(build.accepts(v(1, 0)), Ok(()));
        assert_eq!(build.accepts(v(1, 1)), Ok(()));
        // A newer peer is fine: additions are ignorable by contract.
        assert_eq!(build.accepts(v(1, 9)), Ok(()));
    }

    #[test]
    fn different_majors_cannot_talk() {
        let build = Negotiation::new(v(1, 1));
        assert_eq!(build.accepts(v(2, 0)), Err(Incompatible::MajorMismatch));
        assert_eq!(build.accepts(v(0, 9)), Err(Incompatible::MajorMismatch));
    }

    #[test]
    fn a_dropped_minor_is_refused_explicitly() {
        let build = Negotiation::with_min(v(1, 5), v(1, 2));
        assert_eq!(build.accepts(v(1, 1)), Err(Incompatible::TooOld));
        assert_eq!(build.accepts(v(1, 2)), Ok(()));
    }

    #[test]
    fn ordering_is_by_major_then_minor() {
        assert!(v(1, 2) < v(1, 10));
        assert!(v(1, 10) < v(2, 0));
    }
}
