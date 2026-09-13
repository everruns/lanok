//! SimpleServer behaviour. Plain `#[test]`, no runtime: that is the point of
//! this server, and a test that needed one would prove the opposite.

use std::io::Write;
use std::sync::{Arc, Mutex};

use lanok_core::{Message, RpcError, Value, codes};
use lanok_peer::SimpleServer;
use serde_json::json;

/// A writer the test can read back.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn messages(&self) -> Vec<Message> {
        let bytes = self.0.lock().unwrap().clone();
        String::from_utf8(bytes)
            .unwrap()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| Message::from_line(l).expect("server wrote a valid message"))
            .collect()
    }
}

/// Drive `server` with `input` and collect everything it wrote.
fn run(server: SimpleServer, input: &str) -> Vec<Message> {
    let captured = Captured::default();
    server
        .serve(input.as_bytes(), Box::new(captured.clone()))
        .unwrap();
    captured.messages()
}

fn result_of(messages: &[Message], id: u64) -> &Result<Value, RpcError> {
    messages
        .iter()
        .find_map(|m| match m {
            Message::Response { id: got, payload } if got.as_number() == Some(id) => Some(payload),
            _ => None,
        })
        .expect("no response with that id")
}

fn echo_server() -> SimpleServer {
    SimpleServer::new("echo", "1.1".parse().unwrap())
        .capability("uppercase")
        .on_request("echo", |params| {
            let text = params
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("`text` is required"))?;
            Ok(json!({ "text": text.to_uppercase() }))
        })
}

#[test]
fn answers_the_handshake_without_the_author_writing_one() {
    let messages = run(
        echo_server(),
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"name":"host","protocol_version":"1.0"}}
"#,
    );

    let hello = result_of(&messages, 1).as_ref().unwrap();
    assert_eq!(hello["name"], "echo");
    assert_eq!(hello["protocol_version"], "1.1");
    assert_eq!(hello["capabilities"], json!(["uppercase"]));
}

#[test]
fn dispatches_registered_methods() {
    let messages = run(
        echo_server(),
        "{\"id\":7,\"method\":\"echo\",\"params\":{\"text\":\"hi\"}}\n",
    );
    assert_eq!(result_of(&messages, 7).as_ref().unwrap()["text"], "HI");
}

#[test]
fn an_unknown_method_is_an_error_not_a_silence() {
    let messages = run(echo_server(), "{\"id\":2,\"method\":\"nope\"}\n");
    let error = result_of(&messages, 2).as_ref().unwrap_err();
    assert_eq!(error.code, codes::METHOD_NOT_FOUND);
}

#[test]
fn a_handler_error_becomes_an_error_response() {
    let messages = run(
        echo_server(),
        "{\"id\":3,\"method\":\"echo\",\"params\":{}}\n",
    );
    let error = result_of(&messages, 3).as_ref().unwrap_err();
    assert_eq!(error.code, codes::INVALID_PARAMS);
}

#[test]
fn progress_notifications_arrive_before_the_response() {
    // A serial server still streams: this is what keeps a long tool call from
    // looking like a hang.
    let server = SimpleServer::new("worker", "1.0".parse().unwrap()).on_request_with(
        "work",
        |context, _| {
            for step in 1..=3 {
                context.notify("progress", json!({ "step": step }));
            }
            Ok(json!("done"))
        },
    );

    let messages = run(server, "{\"id\":1,\"method\":\"work\"}\n");
    let methods: Vec<_> = messages.iter().map(|m| m.method()).collect();
    assert_eq!(
        methods,
        vec![Some("progress"), Some("progress"), Some("progress"), None],
        "progress must be written while the request is still open"
    );
}

#[test]
fn handlers_can_see_what_the_peer_advertised() {
    let server =
        SimpleServer::new("s", "1.0".parse().unwrap()).on_request_with("check", |cx, _| {
            Ok(json!({
                "peer_streams": cx.peer_supports("streaming"),
                "peer_name": cx.peer().map(|h| h.name).unwrap_or_default(),
            }))
        });

    let messages = run(
        server,
        concat!(
            r#"{"id":1,"method":"initialize","params":{"name":"host","protocol_version":"1.0","capabilities":["streaming"]}}"#,
            "\n",
            r#"{"id":2,"method":"check"}"#,
            "\n"
        ),
    );

    let checked = result_of(&messages, 2).as_ref().unwrap();
    assert_eq!(checked["peer_streams"], true);
    assert_eq!(checked["peer_name"], "host");
}

#[test]
fn junk_lines_are_skipped_rather_than_fatal() {
    let messages = run(
        echo_server(),
        concat!(
            "not json at all\n",
            "\n",
            "{\"id\":1,\"method\":\"echo\",\"params\":{\"text\":\"ok\"}}\n"
        ),
    );
    assert_eq!(result_of(&messages, 1).as_ref().unwrap()["text"], "OK");
}

#[test]
fn an_unsolicited_response_is_dropped_not_answered() {
    // A serial server never issues requests, so a response to it is noise.
    let messages = run(echo_server(), "{\"id\":1,\"result\":\"unexpected\"}\n");
    assert!(messages.is_empty());
}

#[test]
fn notifications_are_observed_and_never_answered() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let collector = seen.clone();
    let server =
        SimpleServer::new("s", "1.0".parse().unwrap()).on_notification("tick", move |_, params| {
            collector.lock().unwrap().push(params);
        });

    let messages = run(server, "{\"method\":\"tick\",\"params\":{\"n\":1}}\n");
    assert!(messages.is_empty(), "a notification gets no response");
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn end_of_input_ends_the_loop() {
    assert!(run(echo_server(), "").is_empty());
}
