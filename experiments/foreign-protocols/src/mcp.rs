//! The Model Context Protocol, from its published JSON Schema (2025-06-18).
//!
//! Not a lanok consumer and never will be: `rmcp` implements MCP, and lanok's
//! own guidance says so. It is here as the hardest available test, because it
//! was designed by people who had never heard of lanok.
//!
//! # It fits now, and did not when the experiment was written
//!
//! Three MCP methods are **bidirectional**: `ping`, `notifications/cancelled`
//! and `notifications/progress` may be sent by either side. Lanok's model was
//! that a method has one direction, declared once, with role-gated stubs, so
//! these were the one part of MCP it could not say. Declaring each twice under
//! different Rust names is rejected by the validator as a duplicate wire name,
//! which is the right answer to the wrong question.
//!
//! That gap is what produced [`lanok::Direction::Either`]. All 25 methods now
//! declare. An `either` method gives up the compile-time role gating, which is
//! why it is a deliberate third option rather than the default: its stubs live
//! on `SharedApi`, and both handler traits carry it because it can arrive from
//! either side.
//!
//! # And it is not just expressible, it works
//!
//! `tests/rmcp_interop.rs` drives this declaration against `rmcp`, the official
//! Rust MCP SDK, and holds a full session with it: handshake, `ping`,
//! `tools/list`, `tools/call`, a reverse `elicitation/create` answered from
//! lanok's handler, and `notifications/progress` arriving mid-request. The
//! payloads are rmcp's own types, so rmcp is the one judging the wire.

type Json = serde_json::Value;

lanok::protocol! {
    name    = "mcp";
    version = "1.0";

    // Client to server.
    /// Negotiate version and capabilities. MCP's payloads are its own, which is
    /// why a peer must be able to run a handshake that is not lanok's `Hello`.
    initiator fn initialize(Json) -> Json;
    /// Note the name: MCP completes the handshake with a namespaced
    /// notification, not `initialized`.
    initiator notify "notifications/initialized" initialized();
    initiator fn "tools/list" tools_list(Json) -> Json requires "tools";
    initiator fn "tools/call" tools_call(Json) -> Json requires "tools";
    initiator fn "resources/list" resources_list(Json) -> Json requires "resources";
    initiator fn "resources/read" resources_read(Json) -> Json requires "resources";
    initiator fn "resources/templates/list" resources_templates_list(Json) -> Json
        requires "resources";
    initiator fn "resources/subscribe" resources_subscribe(Json) -> Json
        requires "resources_subscribe";
    initiator fn "resources/unsubscribe" resources_unsubscribe(Json) -> Json
        requires "resources_subscribe";
    initiator fn "prompts/list" prompts_list(Json) -> Json requires "prompts";
    initiator fn "prompts/get" prompts_get(Json) -> Json requires "prompts";
    initiator fn "completion/complete" completion_complete(Json) -> Json
        requires "completions";
    initiator fn "logging/setLevel" logging_set_level(Json) -> Json requires "logging";
    initiator notify "notifications/roots/list_changed" roots_list_changed()
        requires "roots_list_changed";

    // Server to client: MCP's reverse channel, and the thing that makes it a
    // fair test rather than a formality.
    /// The server asks the client's model for a completion.
    responder fn "sampling/createMessage" sampling_create_message(Json) -> Json
        requires "sampling";
    /// The server asks the client which roots it may touch.
    responder fn "roots/list" roots_list(Json) -> Json requires "roots";
    /// The server asks the user for structured input.
    responder fn "elicitation/create" elicitation_create(Json) -> Json
        requires "elicitation";
    responder notify "notifications/message" logging_message(Json) requires "logging";
    responder notify "notifications/resources/updated" resources_updated(Json)
        requires "resources_subscribe";
    responder notify "notifications/resources/list_changed" resources_list_changed()
        requires "resources_list_changed";
    responder notify "notifications/tools/list_changed" tools_list_changed()
        requires "tools_list_changed";
    responder notify "notifications/prompts/list_changed" prompts_list_changed()
        requires "prompts_list_changed";

    // Either side may send these three. This is the shape that did not exist
    // in lanok until MCP was declared here.
    /// Liveness check, in whichever direction.
    either fn ping(Json) -> Json;
    /// Abandon an in-flight request. Either side may give up on the other.
    either notify "notifications/cancelled" cancelled(Json);
    /// Progress on a long call, reported by whoever is doing the work.
    either notify "notifications/progress" progress(Json);

    capabilities {
        tools, resources, resources_subscribe, resources_list_changed,
        tools_list_changed, prompts, prompts_list_changed, completions,
        logging, sampling, roots, roots_list_changed, elicitation
    }
}

/// The three MCP methods that have no direction, named so the shape is
/// greppable rather than buried in prose. Lanok could not express them at all
/// until [`lanok::Direction::Either`] existed.
pub const BIDIRECTIONAL_METHODS: &[&str] =
    &["ping", "notifications/cancelled", "notifications/progress"];

#[cfg(test)]
mod tests {
    use super::*;
    use lanok::Direction;

    #[test]
    fn mcp_declares_in_full() {
        // Every method in the published 2025-06-18 schema.
        assert_eq!(META.methods.len(), 25);

        for method in BIDIRECTIONAL_METHODS {
            assert_eq!(
                META.method(method).unwrap().direction,
                Direction::Either,
                "{method} has no direction in MCP, and must not be given one here"
            );
        }

        // Either side may send them, so they count for both columns and are
        // declared as neither.
        assert_eq!(META.declared_by(Direction::Either).count(), 3);
    }

    #[test]
    fn the_reverse_channel_is_the_part_that_matters() {
        // A protocol designed with no knowledge of lanok, using the reverse
        // direction for exactly what lanok's reverse direction is for: asking
        // the other side something mid-request.
        for method in ["sampling/createMessage", "roots/list", "elicitation/create"] {
            assert_eq!(META.method(method).unwrap().direction, Direction::Responder);
        }
        assert!(META.is_bidirectional());
    }

    #[test]
    fn the_handshake_notification_is_not_called_initialized() {
        // Which is why the peer's handshake method names are configurable.
        assert!(META.method("notifications/initialized").is_some());
        assert!(META.method("initialized").is_none());
    }
}
