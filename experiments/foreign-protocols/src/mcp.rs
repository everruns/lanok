//! The Model Context Protocol, from its published JSON Schema (2025-06-18).
//!
//! Not a lanok consumer and never will be: `rmcp` implements MCP, and lanok's
//! own guidance says so. It is here as the hardest available test, because it
//! was designed by people who had never heard of lanok.
//!
//! # It does not fit, and the reason is structural
//!
//! Three MCP methods are **bidirectional**: either side may send them.
//!
//! | Method | Sent by |
//! |---|---|
//! | `ping` | client or server |
//! | `notifications/cancelled` | client or server |
//! | `notifications/progress` | client or server |
//!
//! Lanok's model is that a method has *one* direction, declared once, and the
//! generated stubs are gated by role so calling one the wrong way does not
//! compile. That gating is a feature for a protocol with one-way methods and a
//! wall for a protocol without.
//!
//! The workaround is below: declare each twice, under different Rust names,
//! with the same wire name. The declaration validator rejects a duplicate wire
//! name, so it is not even expressible today. What follows is the surface
//! **minus** the three bidirectional methods, which is what lanok can actually
//! say.
//!
//! # What that means
//!
//! Not that lanok should grow an `either` direction tomorrow. Nothing we own
//! needs one, and the role gating is worth more than symmetric pings. It means
//! the limit is known and written down, rather than discovered by whoever first
//! wants a symmetric `ping` in a protocol of ours.

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

    // `ping`, `notifications/cancelled` and `notifications/progress` are absent.
    // They are bidirectional, and a lanok method has one direction.

    capabilities {
        tools, resources, resources_subscribe, resources_list_changed,
        tools_list_changed, prompts, prompts_list_changed, completions,
        logging, sampling, roots, roots_list_changed, elicitation
    }
}

/// The three MCP methods lanok cannot express, named so the gap is greppable
/// rather than buried in prose.
pub const BIDIRECTIONAL_METHODS: &[&str] =
    &["ping", "notifications/cancelled", "notifications/progress"];

#[cfg(test)]
mod tests {
    use super::*;
    use lanok::Direction;

    #[test]
    fn mcp_declares_except_for_its_bidirectional_methods() {
        // 25 methods in the published schema, minus the three that flow both
        // ways, is what lanok can say.
        assert_eq!(META.methods.len() + BIDIRECTIONAL_METHODS.len(), 25);

        for method in BIDIRECTIONAL_METHODS {
            assert!(
                META.method(method).is_none(),
                "{method} is bidirectional; declaring it would pick a direction MCP does not have"
            );
        }
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
