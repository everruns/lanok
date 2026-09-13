"""The serve loop, driven over in-memory streams."""

import io
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from lanok import INVALID_PARAMS, METHOD_NOT_FOUND, RpcError, Server  # noqa: E402


def run(server: Server, *lines: str) -> list[dict]:
    out = io.StringIO()
    server.serve(io.StringIO("".join(f"{line}\n" for line in lines)), out)
    return [json.loads(line) for line in out.getvalue().splitlines() if line.strip()]


def echo_server() -> Server:
    def echo(params):
        if not isinstance(params.get("text"), str):
            raise RpcError("`text` is required", INVALID_PARAMS)
        return {"text": params["text"].upper()}

    return Server("echo", "1.1", capabilities=["uppercase"]).on_request("echo", echo)


def test_answers_the_handshake_without_the_author_writing_one():
    [hello] = run(
        echo_server(),
        json.dumps({"id": 1, "method": "initialize", "params": {"name": "host", "protocol_version": "1.0"}}),
    )
    assert hello["result"]["name"] == "echo"
    assert hello["result"]["protocol_version"] == "1.1"
    assert hello["result"]["capabilities"] == ["uppercase"]


def test_dispatches_registered_methods():
    [response] = run(echo_server(), json.dumps({"id": 7, "method": "echo", "params": {"text": "hi"}}))
    assert response["result"]["text"] == "HI"


def test_an_unknown_method_is_an_error_not_a_silence():
    [response] = run(echo_server(), json.dumps({"id": 2, "method": "nope"}))
    assert response["error"]["code"] == METHOD_NOT_FOUND


def test_a_handler_error_becomes_an_error_response():
    [response] = run(echo_server(), json.dumps({"id": 3, "method": "echo", "params": {}}))
    assert response["error"]["code"] == INVALID_PARAMS


def test_a_handler_bug_is_reported_rather_than_fatal():
    server = Server("s", "1.0").on_request("boom", lambda _params: 1 / 0)
    [response] = run(server, json.dumps({"id": 1, "method": "boom"}))
    assert "handler raised" in response["error"]["message"]


def test_progress_notifications_arrive_before_the_response():
    def work(context, _params):
        for step in (1, 2, 3):
            context.notify("progress", {"step": step})
        return "done"

    messages = run(Server("w", "1.0").on_request("work", work), json.dumps({"id": 1, "method": "work"}))
    assert [m.get("method") for m in messages] == ["progress", "progress", "progress", None]


def test_handlers_can_see_what_the_peer_advertised():
    server = Server("s", "1.0").on_request(
        "check", lambda context, _params: {"streams": context.peer_supports("streaming"), "who": context.peer_name}
    )
    messages = run(
        server,
        json.dumps(
            {
                "id": 1,
                "method": "initialize",
                "params": {"name": "host", "protocol_version": "1.0", "capabilities": ["streaming"]},
            }
        ),
        json.dumps({"id": 2, "method": "check"}),
    )
    assert messages[1]["result"] == {"streams": True, "who": "host"}


def test_junk_lines_are_skipped_rather_than_fatal():
    server = echo_server()
    messages = run(server, "not json at all", "", json.dumps({"id": 1, "method": "echo", "params": {"text": "ok"}}))
    assert messages[0]["result"]["text"] == "OK"
    assert server.skipped_lines == 1


def test_an_unsolicited_response_is_dropped_not_answered():
    assert run(echo_server(), json.dumps({"id": 1, "result": "unexpected"})) == []


def test_notifications_are_observed_and_never_answered():
    seen = []
    server = Server("s", "1.0").on_notification("tick", lambda params: seen.append(params))
    assert run(server, json.dumps({"method": "tick", "params": {"n": 1}})) == []
    assert seen == [{"n": 1}]


def test_end_of_input_ends_the_loop():
    assert run(echo_server()) == []
