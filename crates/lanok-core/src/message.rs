//! The wire envelope: one JSON object per message, classified by its fields.
//!
//! **Classification is by field, never by direction.** A line bearing `method`
//! is a request (with `id`) or a notification (without); a line without
//! `method` is a response, routed by `id`. Nothing here asks which pipe the
//! line arrived on, and that single decision is what makes a reverse request an
//! additive declaration instead of a redesign.
//!
//! `jsonrpc: "2.0"` is written on every outbound message and never required on
//! an inbound one. Emitting it makes the wire literally JSON-RPC 2.0, so
//! off-the-shelf clients in any language can drive it; not requiring it keeps
//! peers that predate the field readable.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{Id, RpcError};

/// The value of the `jsonrpc` member on every outbound message.
pub const JSONRPC_VERSION: &str = "2.0";

/// One parsed message.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /// Bears `method` and `id`: expects a response.
    Request {
        id: Id,
        method: String,
        params: Value,
    },
    /// Bears `method` and no `id`: fire and forget.
    Notification { method: String, params: Value },
    /// Bears `id` and no `method`: the answer to a request we sent.
    Response {
        id: Id,
        payload: Result<Value, RpcError>,
    },
}

/// Why a line was not a usable message.
#[derive(Debug)]
pub enum Malformed {
    /// Not valid JSON.
    NotJson(serde_json::Error),
    /// Valid JSON, but not an object.
    NotAnObject,
    /// An object with neither `method` nor `id`: unclassifiable.
    Unclassifiable,
    /// `method` was present but not a string.
    BadMethod,
}

impl std::fmt::Display for Malformed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Malformed::NotJson(e) => write!(f, "not valid JSON: {e}"),
            Malformed::NotAnObject => write!(f, "not a JSON object"),
            Malformed::Unclassifiable => write!(f, "neither a method nor an id"),
            Malformed::BadMethod => write!(f, "`method` is not a string"),
        }
    }
}

impl std::error::Error for Malformed {}

impl Message {
    pub fn request(id: impl Into<Id>, method: impl Into<String>, params: Value) -> Self {
        Message::Request {
            id: id.into(),
            method: method.into(),
            params,
        }
    }

    pub fn notification(method: impl Into<String>, params: Value) -> Self {
        Message::Notification {
            method: method.into(),
            params,
        }
    }

    pub fn result(id: impl Into<Id>, result: Value) -> Self {
        Message::Response {
            id: id.into(),
            payload: Ok(result),
        }
    }

    pub fn error(id: impl Into<Id>, error: RpcError) -> Self {
        Message::Response {
            id: id.into(),
            payload: Err(error),
        }
    }

    /// The method name, for the two variants that carry one.
    pub fn method(&self) -> Option<&str> {
        match self {
            Message::Request { method, .. } | Message::Notification { method, .. } => Some(method),
            Message::Response { .. } => None,
        }
    }

    /// Classify one JSON value.
    pub fn from_value(value: Value) -> Result<Message, Malformed> {
        let Value::Object(object) = value else {
            return Err(Malformed::NotAnObject);
        };
        let id = object.get("id").filter(|v| !v.is_null()).cloned();
        let method = object.get("method");

        match (method, id) {
            (Some(method), id) => {
                let method = method.as_str().ok_or(Malformed::BadMethod)?.to_string();
                let params = object.get("params").cloned().unwrap_or(Value::Null);
                Ok(match id {
                    Some(id) => Message::Request {
                        // An id that is neither number nor string is not a
                        // usable correlation key, so treat it as absent and let
                        // the message stand as a notification rather than
                        // dropping the line outright.
                        id: match serde_json::from_value(id) {
                            Ok(id) => id,
                            Err(_) => return Ok(Message::Notification { method, params }),
                        },
                        method,
                        params,
                    },
                    None => Message::Notification { method, params },
                })
            }
            (None, Some(id)) => {
                let id = serde_json::from_value(id).map_err(Malformed::NotJson)?;
                let payload = match object.get("error") {
                    // A malformed error object still means failure, so keep the
                    // response rather than losing the outcome to a parse error.
                    Some(error) => {
                        Err(serde_json::from_value(error.clone()).unwrap_or_else(|_| {
                            RpcError::internal("peer sent a malformed error object")
                        }))
                    }
                    None => Ok(object.get("result").cloned().unwrap_or(Value::Null)),
                };
                Ok(Message::Response { id, payload })
            }
            (None, None) => Err(Malformed::Unclassifiable),
        }
    }

    /// Parse one line of newline-delimited JSON.
    pub fn from_line(line: &str) -> Result<Message, Malformed> {
        let value = serde_json::from_str(line).map_err(Malformed::NotJson)?;
        Message::from_value(value)
    }

    /// Render to a JSON value, always carrying `jsonrpc: "2.0"`.
    pub fn to_value(&self) -> Value {
        let mut object = Map::new();
        object.insert("jsonrpc".into(), json!(JSONRPC_VERSION));
        match self {
            Message::Request { id, method, params } => {
                object.insert("id".into(), json!(id));
                object.insert("method".into(), json!(method));
                if !params.is_null() {
                    object.insert("params".into(), params.clone());
                }
            }
            Message::Notification { method, params } => {
                object.insert("method".into(), json!(method));
                if !params.is_null() {
                    object.insert("params".into(), params.clone());
                }
            }
            Message::Response { id, payload } => {
                object.insert("id".into(), json!(id));
                match payload {
                    // `result` is written even when null: JSON-RPC requires
                    // exactly one of result/error, so omitting it would make a
                    // successful empty response unclassifiable.
                    Ok(result) => {
                        object.insert("result".into(), result.clone());
                    }
                    Err(error) => {
                        object.insert("error".into(), json!(error));
                    }
                }
            }
        }
        Value::Object(object)
    }

    /// Render to one line of newline-delimited JSON. Never contains a newline.
    pub fn to_line(&self) -> String {
        self.to_value().to_string()
    }
}

impl Serialize for Message {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Message {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Message::from_value(value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_field_not_by_direction() {
        assert!(matches!(
            Message::from_line(r#"{"id":1,"method":"a","params":{}}"#).unwrap(),
            Message::Request { .. }
        ));
        assert!(matches!(
            Message::from_line(r#"{"method":"a"}"#).unwrap(),
            Message::Notification { .. }
        ));
        assert!(matches!(
            Message::from_line(r#"{"id":1,"result":7}"#).unwrap(),
            Message::Response { payload: Ok(_), .. }
        ));
        assert!(matches!(
            Message::from_line(r#"{"id":1,"error":{"code":-1,"message":"x"}}"#).unwrap(),
            Message::Response {
                payload: Err(_),
                ..
            }
        ));
    }

    #[test]
    fn every_outbound_message_carries_jsonrpc_2_0() {
        for message in [
            Message::request(1u64, "a", json!({})),
            Message::notification("a", Value::Null),
            Message::result(1u64, json!(7)),
            Message::error(1u64, RpcError::internal("x")),
        ] {
            assert_eq!(message.to_value()["jsonrpc"], "2.0");
        }
    }

    #[test]
    fn inbound_jsonrpc_field_is_optional() {
        // A peer predating the field stays readable.
        let parsed = Message::from_line(r#"{"id":4,"method":"run"}"#).unwrap();
        assert_eq!(parsed.method(), Some("run"));
    }

    #[test]
    fn a_null_id_reads_as_a_notification() {
        // JSON-RPC uses a null id for "could not determine the id"; treating it
        // as a correlation key would route a response to nothing.
        assert!(matches!(
            Message::from_line(r#"{"method":"a","id":null}"#).unwrap(),
            Message::Notification { .. }
        ));
    }

    #[test]
    fn string_ids_round_trip() {
        let parsed = Message::from_line(r#"{"id":"abc","method":"a"}"#).unwrap();
        let Message::Request { id, .. } = &parsed else {
            panic!("expected a request");
        };
        assert_eq!(id, &Id::Text("abc".into()));
        assert_eq!(parsed.to_value()["id"], "abc");
    }

    #[test]
    fn missing_params_becomes_null_and_is_not_re_emitted() {
        let parsed = Message::from_line(r#"{"id":1,"method":"a"}"#).unwrap();
        assert!(parsed.to_value().get("params").is_none());
    }

    #[test]
    fn a_successful_null_result_stays_classifiable() {
        let line = Message::result(1u64, Value::Null).to_line();
        assert!(line.contains("\"result\":null"));
        assert!(matches!(
            Message::from_line(&line).unwrap(),
            Message::Response { payload: Ok(_), .. }
        ));
    }

    #[test]
    fn a_malformed_error_object_still_reads_as_failure() {
        let parsed = Message::from_line(r#"{"id":1,"error":"just a string"}"#).unwrap();
        assert!(matches!(
            parsed,
            Message::Response {
                payload: Err(_),
                ..
            }
        ));
    }

    #[test]
    fn rejects_unclassifiable_lines() {
        assert!(matches!(
            Message::from_line("{}"),
            Err(Malformed::Unclassifiable)
        ));
        assert!(matches!(
            Message::from_line("[]"),
            Err(Malformed::NotAnObject)
        ));
        assert!(matches!(
            Message::from_line("nope"),
            Err(Malformed::NotJson(_))
        ));
    }

    #[test]
    fn lines_never_contain_a_newline() {
        let message = Message::request(1u64, "a", json!({ "text": "one\ntwo" }));
        assert!(!message.to_line().contains('\n'));
    }

    #[test]
    fn round_trips_through_a_line() {
        let original = Message::request(9u64, "tool/call", json!({ "name": "ls" }));
        let parsed = Message::from_line(&original.to_line()).unwrap();
        assert_eq!(original, parsed);
    }
}
