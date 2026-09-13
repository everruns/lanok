"""Lanok: build JSON-RPC 2.0 protocols in Python.

The framing and the serve loop live here, written once. A protocol's own wire
types are generated from its committed artifacts with ``lanok gen python``, so
nothing in this package is protocol-specific and no protocol re-implements a
JSON-RPC loop.

    from lanok import Server

    Server("echo", "1.0").on_request(
        "echo", lambda params: {"text": params["text"].upper()}
    ).serve()

Keep stdout clean: only protocol JSON belongs there, and logging belongs on
stderr.
"""

from ._server import INITIALIZE, INITIALIZED, Context, Server
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
    "Context",
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
    "REQUEST_CANCELLED",
    "REQUEST_TIMEOUT",
    "Request",
    "Response",
    "RpcError",
    "Server",
    "TRANSPORT_CLOSED",
    "VERSION_INCOMPATIBLE",
    "Version",
    "accepts",
    "classify",
]

__version__ = "0.1.0"
