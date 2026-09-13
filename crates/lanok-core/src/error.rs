//! The JSON-RPC error object, plus the codes lanok protocols share.
//!
//! `retryable` is a top-level field, beside `code` and `message`. It rode
//! inside `data` first, on the argument that JSON-RPC 2.0 enumerates the
//! members of an error object. That argument does not survive contact: the spec
//! says the error member *must* contain `code` and `message` and *may* contain
//! `data`, and nowhere forbids more. Both protocols this kit exists to serve
//! already put the flag at the top level, so burying it bought strict-looking
//! conformance at the price of a wire break for every consumer. Two out of two
//! is not a sample size worth overruling.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Error codes. The reserved range is JSON-RPC's; the rest are lanok's, chosen
/// outside it so they cannot collide with a future reserved assignment.
pub mod codes {
    /// Invalid JSON was received.
    pub const PARSE_ERROR: i64 = -32700;
    /// The JSON sent is not a valid request object.
    pub const INVALID_REQUEST: i64 = -32600;
    /// The method does not exist, or is not available in this direction.
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// Invalid method parameters.
    pub const INVALID_PARAMS: i64 = -32602;
    /// Internal JSON-RPC error.
    pub const INTERNAL_ERROR: i64 = -32603;

    /// The request was cancelled before it completed. Shares LSP's value, which
    /// is the closest thing to a convention outside the reserved range.
    pub const REQUEST_CANCELLED: i64 = -32800;
    /// The request outlived its deadline.
    pub const REQUEST_TIMEOUT: i64 = -32801;
    /// The peer never advertised the capability this method requires.
    pub const CAPABILITY_UNSUPPORTED: i64 = -32802;
    /// The peers cannot talk: incompatible protocol versions.
    pub const VERSION_INCOMPATIBLE: i64 = -32803;
    /// The connection closed with the request still in flight.
    pub const TRANSPORT_CLOSED: i64 = -32804;
}

/// A JSON-RPC 2.0 error object.
///
/// Parses leniently: a peer that sends a bare `{"message": "..."}` still
/// deserializes, with `code` defaulting to [`codes::INTERNAL_ERROR`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RpcError {
    #[serde(default = "default_code")]
    pub code: i64,
    pub message: String,
    /// Whether the sender hinted this failure is worth retrying: a rate limit,
    /// an overloaded upstream. Omitted from the wire when false, so an error
    /// that never sets it looks exactly as it did before the field existed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

fn default_code() -> i64 {
    codes::INTERNAL_ERROR
}

impl RpcError {
    /// An error with an explicit code.
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        RpcError {
            code,
            message: message.into(),
            retryable: false,
            data: None,
        }
    }

    /// A non-retryable internal error carrying only a message.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(codes::INTERNAL_ERROR, message)
    }

    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            codes::METHOD_NOT_FOUND,
            format!("method not found: {method}"),
        )
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(codes::INVALID_PARAMS, message)
    }

    pub fn cancelled() -> Self {
        Self::new(codes::REQUEST_CANCELLED, "request cancelled")
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(codes::REQUEST_TIMEOUT, message)
    }

    pub fn transport_closed() -> Self {
        Self::new(
            codes::TRANSPORT_CLOSED,
            "the peer closed the connection with this request in flight",
        )
    }

    /// The peer never advertised `capability`, so the method is unavailable.
    /// A generated stub returns this without writing to the wire.
    pub fn capability_unsupported(method: &str, capability: &str) -> Self {
        Self::new(
            codes::CAPABILITY_UNSUPPORTED,
            format!(
                "{method} requires the `{capability}` capability, which the peer did not advertise"
            ),
        )
    }

    /// Attach structured data.
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    /// Mark the failure as worth retrying: a rate limit, an overloaded
    /// upstream, anything where the same call may succeed later.
    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }

    /// Whether the sender hinted this failure is worth retrying.
    ///
    /// Reads the field; kept as a method so call sites that ask a question read
    /// like one.
    pub fn is_retryable(&self) -> bool {
        self.retryable
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (code {})", self.message, self.code)
    }
}

impl std::error::Error for RpcError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_bare_message() {
        let error: RpcError = serde_json::from_str(r#"{"message":"boom"}"#).unwrap();
        assert_eq!(error.code, codes::INTERNAL_ERROR);
        assert_eq!(error.message, "boom");
    }

    #[test]
    fn ignores_unknown_fields() {
        let error: RpcError =
            serde_json::from_str(r#"{"code":-1,"message":"x","future":"field"}"#).unwrap();
        assert_eq!(error.code, -1);
    }

    #[test]
    fn retryable_is_a_top_level_field_and_round_trips() {
        let error = RpcError::internal("rate limited").retryable();
        assert!(error.is_retryable());

        let wire = serde_json::to_value(&error).unwrap();
        assert_eq!(wire["retryable"], true);

        let back: RpcError = serde_json::from_value(wire).unwrap();
        assert!(back.is_retryable());
    }

    #[test]
    fn retryable_is_omitted_when_false() {
        // An error that never sets it looks exactly as it did before the field
        // existed, so adding it changed no existing wire.
        let wire = serde_json::to_value(RpcError::internal("boom")).unwrap();
        assert!(wire.get("retryable").is_none());
    }

    #[test]
    fn retryable_and_data_are_independent() {
        let error = RpcError::internal("slow down")
            .with_data(json!({ "retry_after_ms": 500 }))
            .retryable();
        assert!(error.is_retryable());
        assert_eq!(error.data.unwrap()["retry_after_ms"], 500);
    }

    #[test]
    fn plain_errors_are_not_retryable() {
        assert!(!RpcError::internal("boom").is_retryable());
        assert!(
            !RpcError::internal("boom")
                .with_data(json!("opaque"))
                .is_retryable()
        );
    }
}
