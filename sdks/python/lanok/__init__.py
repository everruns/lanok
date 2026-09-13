"""Lanok: build JSON-RPC 2.0 protocols in Python.

The framing, the serve loop, and the symmetric peer live here, written once. A
protocol's own wire types are generated from its committed artifacts with
``lanok gen python``, so nothing in this package is protocol-specific and no
protocol re-implements a JSON-RPC loop.

Two shapes, matching the Rust kit:

``Server`` is serial and blocking. It answers one request at a time and cannot
send one, which is all a small tool server needs::

    from lanok import Server

    Server("echo", "1.0").on_request(
        "echo", lambda params: {"text": params["text"].upper()}
    ).serve()

``Peer`` is the symmetric one. It issues requests and answers them at the same
time, so it can be either end of a connection, including the reverse
direction::

    from lanok import Hello, Peer, Router, connect_child

    peer = connect_child(["./my-server"], Router().on_request("ui/ask", answer))
    peer.handshake(Hello("my-host", "1.0", ["ui_ask"]))
    peer.request("echo", {"text": "hi"})

Keep stdout clean: only protocol JSON belongs there, and logging belongs on
stderr.
"""

from ._peer import Hello, Peer, Router, connect_child
from ._server import INITIALIZE, INITIALIZED, Context, Server
from ._transport import ChildTransport, StreamTransport, Transport, duplex, stdio
from ._version import Version, accepts
from ._wire import (
    CAPABILITY_UNSUPPORTED,
    INTERNAL_ERROR,
    INVALID_PARAMS,
    INVALID_REQUEST,
    JSONRPC_VERSION,
    METHOD_NOT_FOUND,
    PARSE_ERROR,
    REQUEST_CANCELLED,
    REQUEST_TIMEOUT,
    TRANSPORT_CLOSED,
    VERSION_INCOMPATIBLE,
    Malformed,
    Notification,
    Request,
    Response,
    RpcError,
    classify,
)

__all__ = [
    "CAPABILITY_UNSUPPORTED",
    "ChildTransport",
    "Context",
    "Hello",
    "INITIALIZE",
    "INITIALIZED",
    "INTERNAL_ERROR",
    "INVALID_PARAMS",
    "INVALID_REQUEST",
    "JSONRPC_VERSION",
    "METHOD_NOT_FOUND",
    "Malformed",
    "Notification",
    "PARSE_ERROR",
    "Peer",
    "REQUEST_CANCELLED",
    "REQUEST_TIMEOUT",
    "Request",
    "Response",
    "Router",
    "RpcError",
    "Server",
    "StreamTransport",
    "TRANSPORT_CLOSED",
    "Transport",
    "VERSION_INCOMPATIBLE",
    "Version",
    "accepts",
    "classify",
    "connect_child",
    "duplex",
    "stdio",
]

__version__ = "0.1.0"
