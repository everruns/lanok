//! The handshake every lanok protocol shares.
//!
//! Two method names are conventional rather than configurable, because a
//! convention is what lets a generic tool (`lanok conform`, a doctor
//! subcommand, a third-party client) talk to a protocol it has never seen:
//! `initialize` is a request, `initialized` is the notification that follows
//! the reply. Everything a protocol wants beyond version and capabilities
//! travels in [`Hello::info`], which is free-form by design.

use lanok_core::{Capabilities, Value, Version};
use serde::{Deserialize, Serialize};

/// The handshake request, and the reply to it. Both directions send the same
/// shape: each side states who it is and what it can do.
pub const INITIALIZE: &str = "initialize";
/// Sent by the initiator once it has accepted the reply.
pub const INITIALIZED: &str = "initialized";

/// Who a peer is and what it supports.
///
/// Parses leniently, like every lanok payload, with exactly one exception:
/// `protocol_version` is required. A peer that will not say which version it
/// speaks cannot be negotiated with, and defaulting the field would turn that
/// into a mystery failure three methods later.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Hello {
    /// Implementation name, for logs and error messages. Not identity.
    #[serde(default)]
    pub name: String,
    /// The `MAJOR.MINOR` protocol version this peer implements.
    pub protocol_version: Version,
    /// Optional tokens this peer supports.
    #[serde(default)]
    pub capabilities: Capabilities,
    /// Anything else the protocol wants in its handshake. Opaque to lanok.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub info: Value,
}

impl Hello {
    pub fn new(name: impl Into<String>, protocol_version: Version) -> Self {
        Hello {
            name: name.into(),
            protocol_version,
            capabilities: Capabilities::new(),
            info: Value::Null,
        }
    }

    pub fn capability(mut self, token: impl Into<String>) -> Self {
        self.capabilities.insert(token);
        self
    }

    pub fn with_info(mut self, info: Value) -> Self {
        self.info = info;
        self
    }
}
