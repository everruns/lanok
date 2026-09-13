//! The yolop extension protocol (YEP), from `yolop/crates/yolop-yep/src/meta.rs`.
//!
//! A lanok consumer, so this is the control: if YEP did not fit, the extraction
//! would be wrong on its own terms. It fits exactly, including the reverse
//! request (`ui/ask`) and every capability gate.

/// Payloads are out of scope; the experiment is about the method surface.
type Json = serde_json::Value;

lanok::protocol! {
    name    = "yep";
    version = "1.0";

    /// Handshake: negotiate version, pass config; server replies with contributions.
    initiator fn initialize(Json) -> Json;
    /// Notification: handshake complete.
    initiator notify initialized();
    /// Invoke a tool the server declared.
    initiator fn "tool/call" tool_call(Json) -> Json requires "tools";
    /// Fire a subscribed lifecycle hook (pre_tool_use/post_tool_use).
    initiator fn "hook/fire" hook_fire(Json) -> Json requires "hooks";
    /// Request a fresh dynamic system-prompt contribution.
    initiator fn "prompt/contribution" prompt_contribution(Json) -> Json
        requires "dynamic_prompt";
    /// Run a manifest-declared slash command; the result is shown to the user.
    initiator fn "command/execute" command_execute(Json) -> Json requires "commands";
    /// Notify the server its config changed.
    initiator notify "config/changed" config_changed(Json);
    /// Notification: forward one agentic-lifecycle event to an extension.
    initiator notify "trace/event" trace_event(Json) requires "trace";
    /// Request a graceful stop.
    initiator notify shutdown();

    /// Notification: streamed progress for an in-flight tool/call.
    responder notify "tool/update" tool_update(Json) requires "streaming";
    /// Notification: a short capability status the host surfaces in its status bar.
    responder notify "status/changed" status_changed(Json) requires "status";
    /// Ask the user a question and wait for a typed answer (TUI hosts only).
    responder fn "ui/ask" ui_ask(Json) -> Json requires "ui_ask";

    capabilities {
        tools, streaming, prompt, dynamic_prompt, hooks,
        mcp_servers, status, commands, ui_ask, trace
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lanok::{Direction, MethodKind};

    #[test]
    fn yep_declares_exactly() {
        assert_eq!(META.methods.len(), 12);
        assert_eq!(META.capabilities.len(), 10);

        // The reverse request that motivated the symmetric peer.
        let ask = META.method("ui/ask").unwrap();
        assert_eq!(ask.direction, Direction::Responder);
        assert_eq!(ask.kind, MethodKind::Request);
        assert_eq!(ask.requires, Some("ui_ask"));

        // Streamed progress rides back while a tool/call is open.
        let update = META.method("tool/update").unwrap();
        assert_eq!(update.direction, Direction::Responder);
        assert_eq!(update.kind, MethodKind::Notification);

        assert!(META.is_bidirectional());
    }
}
