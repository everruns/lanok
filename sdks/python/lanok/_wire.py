"""JSON-RPC 2.0 framing.

The Python half of the same contract the Rust core implements, and it is the
same three rules: a message is classified by its fields rather than by which
pipe it arrived on, ``jsonrpc: "2.0"`` is written on everything outbound and
required on nothing inbound, and a line that does not parse is skipped rather
than fatal.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any

JSONRPC_VERSION = "2.0"

# Reserved JSON-RPC codes, then lanok's, chosen outside the reserved range.
PARSE_ERROR = -32700
INVALID_REQUEST = -32600
METHOD_NOT_FOUND = -32601
INVALID_PARAMS = -32602
INTERNAL_ERROR = -32603
REQUEST_CANCELLED = -32800
REQUEST_TIMEOUT = -32801
CAPABILITY_UNSUPPORTED = -32802
VERSION_INCOMPATIBLE = -32803
TRANSPORT_CLOSED = -32804


class RpcError(Exception):
    """An error that can be returned to the peer.

    Raise it from a handler; the serve loop turns it into an error response.
    Any other exception becomes INTERNAL_ERROR, so a handler bug is reported
    rather than silently closing the connection.
    """

    def __init__(
        self,
        message: str,
        code: int = INTERNAL_ERROR,
        data: Any = None,
        retryable: bool = False,
    ) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.data = data
        self.is_retryable = retryable

    def retryable(self) -> "RpcError":
        """Hint that the failure is worth retrying: a rate limit, an overloaded
        upstream, anything where the same call may succeed later."""
        self.is_retryable = True
        return self

    def to_wire(self) -> dict[str, Any]:
        error: dict[str, Any] = {"code": self.code, "message": self.message}
        # Omitted when false, so an error that never sets it looks exactly as it
        # did before the field existed.
        if self.is_retryable:
            error["retryable"] = True
        if self.data is not None:
            error["data"] = self.data
        return error


@dataclass
class Request:
    id: Any
    method: str
    params: Any = None


@dataclass
class Notification:
    method: str
    params: Any = None


@dataclass
class Response:
    id: Any
    result: Any = None
    error: RpcError | None = None


@dataclass
class Malformed:
    """A line that was not a usable message. Kept rather than raised, so a
    caller can count and log skips without wrapping every read in a try."""

    line: str
    reason: str
    extra: dict[str, Any] = field(default_factory=dict)


Message = Request | Notification | Response | Malformed


def classify(line: str) -> Message:
    """Classify one line by its fields. Never by direction."""
    try:
        value = json.loads(line)
    except ValueError as e:
        return Malformed(line, f"not valid JSON: {e}")
    if not isinstance(value, dict):
        return Malformed(line, "not a JSON object")

    method = value.get("method")
    # A null id means "could not determine the id" in JSON-RPC, so it is not a
    # correlation key: treat it as absent.
    identifier = value.get("id")
    has_id = identifier is not None

    if method is not None:
        if not isinstance(method, str):
            return Malformed(line, "`method` is not a string")
        params = value.get("params")
        return Request(identifier, method, params) if has_id else Notification(method, params)

    if has_id:
        if "error" in value:
            raw = value["error"]
            if isinstance(raw, dict):
                return Response(
                    identifier,
                    error=RpcError(
                        str(raw.get("message", "")),
                        int(raw.get("code", INTERNAL_ERROR)),
                        raw.get("data"),
                        bool(raw.get("retryable", False)),
                    ),
                )
            # A malformed error object still means failure; losing the outcome
            # to a parse error would be worse than losing the detail.
            return Response(identifier, error=RpcError("peer sent a malformed error object"))
        return Response(identifier, result=value.get("result"))

    return Malformed(line, "neither a method nor an id")


def request(identifier: Any, method: str, params: Any = None) -> str:
    message: dict[str, Any] = {"jsonrpc": JSONRPC_VERSION, "id": identifier, "method": method}
    if params is not None:
        message["params"] = params
    return json.dumps(message)


def notification(method: str, params: Any = None) -> str:
    message: dict[str, Any] = {"jsonrpc": JSONRPC_VERSION, "method": method}
    if params is not None:
        message["params"] = params
    return json.dumps(message)


def result(identifier: Any, value: Any) -> str:
    # `result` is written even when null: JSON-RPC requires exactly one of
    # result/error, so omitting it makes a successful empty response
    # unclassifiable.
    return json.dumps({"jsonrpc": JSONRPC_VERSION, "id": identifier, "result": value})


def error(identifier: Any, err: RpcError) -> str:
    return json.dumps({"jsonrpc": JSONRPC_VERSION, "id": identifier, "error": err.to_wire()})
