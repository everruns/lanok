#!/usr/bin/env python3
"""Drive the echo protocol from Python.

The other half of the story: Python is not only something you can implement a
server in, it is something you can drive one from. This spawns whatever server
it is pointed at, runs the handshake, calls forward, and answers the reverse
`ui/ask` request the server sends back.

    python3 examples/echo-python/client.py ./target/debug/echo-server --async
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "sdks" / "python"))

from lanok import Hello, Router, connect_child  # noqa: E402
from lanok._generated_echo import PROTOCOL, UI_ASK, VERSION, method  # noqa: E402

progress_seen = 0


def on_progress(params):
    global progress_seen
    progress_seen += 1
    print(f"progress {params['step']}/{params['of']}")


def on_ask(params):
    """The reverse request: the server is asking us."""
    print(f"server asks: {params['question']}")
    return {"answer": "yes"}


def main() -> int:
    command = sys.argv[1:] or ["./target/debug/echo-server", "--async"]

    router = Router().on_request(method.UI_ASK, on_ask).on_notification(
        method.ECHO_PROGRESS, on_progress
    )
    ours = Hello("echo-client-python", VERSION, [UI_ASK])

    with connect_child(
        command,
        router,
        serve_handshake=ours,
        request_timeout=30,
        on_stderr=lambda line: print(f"[server] {line}", file=sys.stderr),
    ) as peer:
        server = peer.handshake(ours)
        print(
            f"connected to {server.name} speaking {server.protocol_version} "
            f"({len(server.capabilities)} capabilities)"
        )

        peer.request(method.PING)
        print("ping ok")

        result = peer.request(method.ECHO, {"text": "hello from python", "shout": True})
        print(f"echo -> {result['text']}")

        assert result["text"] == "HELLO FROM PYTHON", result
        assert progress_seen == 3, f"expected 3 progress notifications, saw {progress_seen}"

    print(f"ok: python client, {progress_seen} progress notifications, reverse channel exercised")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
