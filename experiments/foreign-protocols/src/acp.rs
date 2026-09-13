//! The Agent Client Protocol, from `agent-client-protocol-schema` 1.7.0.
//!
//! Not a lanok consumer: yolop speaks ACP through the upstream
//! `agent-client-protocol` crate, and should keep doing so. It is here as an
//! independently designed protocol, to see whether lanok's model describes
//! something real or only describes its own two authors.
//!
//! **It fits.** Every method has one direction, requests and notifications are
//! distinguishable, and the stable surface declares without contortion. Two
//! observations rather than problems:
//!
//! * ACP gates optional methods on a **nested capabilities object**
//!   (`clientCapabilities.fs.readTextFile`), not on flat string tokens. Lanok's
//!   `requires "token"` is flat. Modelling a nested gate as a flat token works
//!   (the names below are invented for the experiment) but a real adoption
//!   would need either flattening at the boundary or nested capability paths.
//! * The unstable surface (`nes/*`, `providers/*`, `mcp/*`) is feature-gated in
//!   the Rust crate. Lanok has no equivalent of a per-method feature gate;
//!   mira solves the same problem with a `protocol-unstable` cargo feature
//!   around the declarations, which would work here too.

type Json = serde_json::Value;

lanok::protocol! {
    name    = "acp";
    version = "2.1";

    // Client to agent. ACP's "agent methods" are the ones the client sends.
    /// Negotiate protocol version and capabilities.
    initiator fn initialize(Json) -> Json;
    /// Authenticate with the agent.
    initiator fn "auth/login" auth_login(Json) -> Json;
    initiator fn "auth/logout" auth_logout(Json) -> Json;
    /// Create a new session.
    initiator fn "session/new" session_new(Json) -> Json;
    initiator fn "session/load" session_load(Json) -> Json requires "load_session";
    initiator fn "session/list" session_list(Json) -> Json;
    initiator fn "session/resume" session_resume(Json) -> Json;
    initiator fn "session/delete" session_delete(Json) -> Json;
    initiator fn "session/close" session_close(Json) -> Json;
    initiator fn "session/set_config_option" session_set_config_option(Json) -> Json;
    /// Send a prompt turn. The long call the reverse channel exists to serve.
    initiator fn "session/prompt" session_prompt(Json) -> Json;
    /// Cancel an in-flight turn. A notification, not a request.
    initiator notify "session/cancel" session_cancel(Json);

    // Agent to client: the reverse direction, and the reason ACP needs one.
    /// Streamed session updates while a prompt turn is running.
    responder notify "session/update" session_update(Json);
    /// Ask the user to approve a tool call, mid-turn.
    responder fn "session/request_permission" session_request_permission(Json) -> Json;
    /// Ask the user for structured input.
    responder fn "elicitation/create" elicitation_create(Json) -> Json;
    responder notify "elicitation/complete" elicitation_complete(Json);
    /// Read and write through the client, so the agent touches no disk itself.
    responder fn "fs/read_text_file" fs_read_text_file(Json) -> Json requires "fs_read";
    responder fn "fs/write_text_file" fs_write_text_file(Json) -> Json requires "fs_write";
    /// Run commands in the client's terminal.
    responder fn "terminal/create" terminal_create(Json) -> Json requires "terminal";
    responder fn "terminal/output" terminal_output(Json) -> Json requires "terminal";
    responder fn "terminal/wait_for_exit" terminal_wait_for_exit(Json) -> Json requires "terminal";
    responder fn "terminal/kill" terminal_kill(Json) -> Json requires "terminal";
    responder fn "terminal/release" terminal_release(Json) -> Json requires "terminal";

    // Flat stand-ins for ACP's nested capability object. See the module docs.
    capabilities { load_session, fs_read, fs_write, terminal }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lanok::{Direction, MethodKind};

    #[test]
    fn acp_declares_without_contortion() {
        assert_eq!(META.methods.len(), 23);

        // The agent asks the client for permission while a prompt turn runs:
        // the same shape as yolop's ui/ask, in a protocol with no connection to
        // lanok at all.
        let permission = META.method("session/request_permission").unwrap();
        assert_eq!(permission.direction, Direction::Responder);
        assert_eq!(permission.kind, MethodKind::Request);

        // Cancellation is a notification here, and a request in mira. This is
        // the divergence that made lanok's cancel a hook rather than a setting.
        let cancel = META.method("session/cancel").unwrap();
        assert_eq!(cancel.direction, Direction::Initiator);
        assert_eq!(cancel.kind, MethodKind::Notification);

        assert!(META.is_bidirectional());
    }
}
