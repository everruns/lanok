#!/usr/bin/env python3
"""The echo protocol in Python, on the symmetric peer.

The difference from ``server.py`` is the last thing a non-Rust implementation
was missing: this one **asks the caller a question mid-request**. `ui/ask` is a
reverse request, sent by the responder, and a serial server cannot do it because
it cannot wait for a reply while producing one.

Driven by the Rust client:

    cargo run -p echo-protocol --bin echo-client -- \\
        --server python3 examples/echo-python/peer_server.py
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "sdks" / "python"))

from lanok import INVALID_PARAMS, Hello, Peer, Router, RpcError, stdio  # noqa: E402
from lanok._generated_echo import PROTOCOL, UI_ASK, VERSION, method  # noqa: E402


def echo(peer: Peer, params):
    if not isinstance(params, dict) or not isinstance(params.get("text"), str):
        raise RpcError("`text` must be a string", INVALID_PARAMS)

    for step in (1, 2, 3):
        peer.notify(method.ECHO_PROGRESS, {"step": step, "of": 3})

    # The reverse request. Gated on the caller having advertised it, so an
    # older host that cannot answer is not left hanging.
    if peer.supports(UI_ASK):
        answer = peer.request(method.UI_ASK, {"question": "shout?"})
        shout = answer.get("answer") == "yes"
    else:
        shout = bool(params.get("shout"))

    return {"text": params["text"].upper() if shout else params["text"]}


def main() -> None:
    router = Router().on_request(method.ECHO, echo).on_request(method.PING, lambda _p: None)
    peer = Peer(
        router,
        serve_handshake=Hello(PROTOCOL, VERSION, [UI_ASK]),
    ).connect(stdio())
    # Serve until the caller closes our stdin.
    peer.wait_closed()


if __name__ == "__main__":
    main()
