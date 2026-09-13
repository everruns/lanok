//! Building the machine-readable description of a protocol.
//!
//! Two artifacts come out, and they answer different questions:
//!
//! * `meta.json`, the **vocabulary**: which methods exist, which way each one
//!   flows, what capability it needs. Small enough to read, and the thing an
//!   SDK generator walks.
//! * `schema.json`, the **shapes**: Draft 2020-12 for every payload, so an
//!   author writing a server in any language can validate what they send.
//!
//! Both are generated from the same `protocol!` declaration, which is what
//! keeps them honest. Nothing here is hand-maintained.

use std::collections::BTreeMap;

use lanok_core::ProtocolMeta;
use schemars::{JsonSchema, SchemaGenerator, generate::SchemaSettings};
use serde_json::{Map, Value, json};

/// One method's payload shapes.
#[derive(Default)]
struct Shapes {
    params: Option<Value>,
    result: Option<Value>,
}

/// Collects payload schemas and emits the two artifacts.
///
/// A `protocol!` declaration generates the calls, so the document cannot drift
/// from the methods it describes.
pub struct Document {
    meta: ProtocolMeta,
    generator: SchemaGenerator,
    messages: BTreeMap<String, Shapes>,
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("protocol", &self.meta.name)
            .field("messages", &self.messages.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl Document {
    pub fn new(meta: ProtocolMeta) -> Self {
        Document {
            meta,
            generator: SchemaSettings::draft2020_12().into_generator(),
            messages: BTreeMap::new(),
        }
    }

    /// A request with both params and result.
    pub fn request<P: JsonSchema, R: JsonSchema>(mut self, method: &str) -> Self {
        let params = self.generator.subschema_for::<P>().to_value();
        let result = self.generator.subschema_for::<R>().to_value();
        let entry = self.messages.entry(method.to_string()).or_default();
        entry.params = Some(params);
        entry.result = Some(result);
        self
    }

    /// A request that takes params and answers with nothing meaningful.
    pub fn request_params<P: JsonSchema>(mut self, method: &str) -> Self {
        let params = self.generator.subschema_for::<P>().to_value();
        self.messages.entry(method.to_string()).or_default().params = Some(params);
        self
    }

    /// A request that takes nothing and answers with a payload.
    pub fn request_result<R: JsonSchema>(mut self, method: &str) -> Self {
        let result = self.generator.subschema_for::<R>().to_value();
        self.messages.entry(method.to_string()).or_default().result = Some(result);
        self
    }

    /// A request with neither params nor result. Recorded anyway, so the
    /// artifact lists every method rather than only the interesting ones.
    pub fn request_bare(mut self, method: &str) -> Self {
        self.messages.entry(method.to_string()).or_default();
        self
    }

    /// A notification carrying params.
    pub fn notification<P: JsonSchema>(self, method: &str) -> Self {
        self.request_params::<P>(method)
    }

    /// A notification carrying nothing.
    pub fn notification_bare(self, method: &str) -> Self {
        self.request_bare(method)
    }

    /// The `schema.json` document: every payload shape, plus the envelopes.
    pub fn schema(&self) -> Value {
        let mut messages = Map::new();
        for (method, shapes) in &self.messages {
            let mut entry = Map::new();
            if let Some(params) = &shapes.params {
                entry.insert("params".into(), params.clone());
            }
            if let Some(result) = &shapes.result {
                entry.insert("result".into(), result.clone());
            }
            messages.insert(method.clone(), Value::Object(entry));
        }

        let mut document = Map::new();
        document.insert(
            "$schema".into(),
            json!("https://json-schema.org/draft/2020-12/schema"),
        );
        document.insert(
            "title".into(),
            json!(format!("{} protocol", self.meta.name)),
        );
        document.insert("protocol".into(), json!(self.meta.name));
        document.insert("version".into(), json!(self.meta.version.to_string()));
        document.insert("messages".into(), Value::Object(messages));
        document.insert("$defs".into(), self.definitions());
        Value::Object(document)
    }

    /// The `meta.json` document: the vocabulary, as declared.
    pub fn meta(&self) -> Value {
        serde_json::to_value(self.meta).expect("protocol metadata is always serializable")
    }

    fn definitions(&self) -> Value {
        let defs = self.generator.definitions();
        Value::Object(defs.clone())
    }
}
