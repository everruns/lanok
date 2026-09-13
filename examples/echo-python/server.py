#!/usr/bin/env python3
"""The echo protocol, in Python.

The point of this file is what it does *not* contain: no JSON-RPC loop, no
framing, no handshake. Those come from the lanok SDK. What is here is the
protocol's own logic, which is the only part that belongs to the protocol.

The same conformance vectors that check the Rust server check this one, so
"Python is a first-class implementation" is a fact in CI rather than a claim.
"""

import sys
from pathlib import Path

# Run from a checkout without installing.
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "sdks" / "python"))

from lanok import INVALID_PARAMS, RpcError, Server  # noqa: E402
from lanok._generated_echo import METHODS, PROTOCOL, UI_ASK, VERSION, method  # noqa: E402


def echo(context, params):
    if not isinstance(params, dict) or not isinstance(params.get("text"), str):
        raise RpcError("`text` must be a string", INVALID_PARAMS)

    for step in (1, 2, 3):
        context.notify(method.ECHO_PROGRESS, {"step": step, "of": 3})

    text = params["text"]
    return {"text": text.upper() if params.get("shout") else text}


def main() -> None:
    assert PROTOCOL == "echo"
    assert method.UI_ASK in METHODS

    (
        Server(PROTOCOL, VERSION, capabilities=[UI_ASK])
        .on_request(method.ECHO, echo)
        .on_request(method.PING, lambda _params: None)
        .serve()
    )


if __name__ == "__main__":
    main()
