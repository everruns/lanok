"""The serve loop: read a line, answer it, write the answer.

Serial and blocking, the Python counterpart of Rust's ``SimpleServer``, and for
the same reason: the person writing a small server in Python should not have to
bring asyncio to answer three methods.

The handshake is answered here rather than by the author, so version and
capability reporting cannot drift between implementations of the same protocol.
"""

from __future__ import annotations

import sys
from typing import Any, Callable, Iterable, TextIO

from ._version import Version
from ._wire import (
    METHOD_NOT_FOUND,
    Malformed,
    Notification,
    Request,
    Response,
    RpcError,
    classify,
)
from . import _wire

INITIALIZE = "initialize"
INITIALIZED = "initialized"

RequestHandler = Callable[..., Any]


class Context:
    """What a handler can do besides returning a value."""

    def __init__(self, out: TextIO, server: "Server") -> None:
        self._out = out
        self._server = server

    def notify(self, method: str, params: Any = None) -> None:
        """Emit a notification now, before the request's own response.

        This is how a serial server streams progress: the caller sees these
        while the request is still open.
        """
        self._out.write(_wire.notification(method, params) + "\n")
        self._out.flush()

    def peer_supports(self, token: str) -> bool:
        return token in self._server.peer_capabilities

    @property
    def peer_name(self) -> str:
        return self._server.peer_name


class Server:
    """A serial stdio server.

    >>> server = Server("echo", "1.0").on_request("echo", lambda p: {"text": p["text"].upper()})
    >>> sorted(server.methods)
    ['echo']
    """

    def __init__(
        self,
        name: str,
        version: str,
        capabilities: Iterable[str] = (),
        info: Any = None,
    ) -> None:
        self.name = name
        self.version = Version.parse(version)
        self.capabilities = sorted(set(capabilities))
        self.info = info
        self._requests: dict[str, RequestHandler] = {}
        self._notifications: dict[str, RequestHandler] = {}
        self.peer_capabilities: list[str] = []
        self.peer_name = ""
        self.skipped_lines = 0

    @property
    def methods(self) -> list[str]:
        return sorted(self._requests)

    def capability(self, token: str) -> "Server":
        if token not in self.capabilities:
            self.capabilities = sorted(self.capabilities + [token])
        return self

    def on_request(self, method: str, handler: RequestHandler) -> "Server":
        """Answer `method`. The handler takes ``params``, or ``(context, params)``
        when it wants to stream progress."""
        self._requests[method] = handler
        return self

    def on_notification(self, method: str, handler: RequestHandler) -> "Server":
        self._notifications[method] = handler
        return self

    def serve(self, stdin: TextIO | None = None, stdout: TextIO | None = None) -> None:
        """Serve until end of input."""
        source = stdin if stdin is not None else sys.stdin
        out = stdout if stdout is not None else sys.stdout
        context = Context(out, self)

        for line in source:
            if not line.strip():
                continue
            message = classify(line)

            if isinstance(message, Malformed):
                # A peer writing one bad line should not take the connection
                # down. Counted so a caller can notice a server printing
                # non-protocol output to stdout.
                self.skipped_lines += 1
                continue

            if isinstance(message, Response):
                # A serial server never issues requests, so a response to it is
                # unsolicited noise.
                continue

            if isinstance(message, Notification):
                if message.method == INITIALIZED:
                    continue
                handler = self._notifications.get(message.method)
                if handler is not None:
                    self._call(handler, context, message.params)
                continue

            assert isinstance(message, Request)
            try:
                value = self._answer(context, message)
            except RpcError as e:
                out.write(_wire.error(message.id, e) + "\n")
                out.flush()
                continue
            except Exception as e:  # noqa: BLE001 - a handler bug is reportable, not fatal
                out.write(
                    _wire.error(message.id, RpcError(f"handler raised: {e}")) + "\n"
                )
                out.flush()
                continue

            out.write(_wire.result(message.id, value) + "\n")
            out.flush()

    def _answer(self, context: Context, message: Request) -> Any:
        if message.method == INITIALIZE:
            theirs = message.params if isinstance(message.params, dict) else {}
            self.peer_name = str(theirs.get("name", ""))
            capabilities = theirs.get("capabilities")
            self.peer_capabilities = list(capabilities) if isinstance(capabilities, list) else []
            hello: dict[str, Any] = {
                "name": self.name,
                "protocol_version": str(self.version),
                "capabilities": self.capabilities,
            }
            if self.info is not None:
                hello["info"] = self.info
            return hello

        handler = self._requests.get(message.method)
        if handler is None:
            raise RpcError(f"method not found: {message.method}", METHOD_NOT_FOUND)
        return self._call(handler, context, message.params)

    @staticmethod
    def _call(handler: RequestHandler, context: Context, params: Any) -> Any:
        """Call a handler with or without the context, whichever it accepts.

        Two-argument handlers are how progress gets streamed; one-argument ones
        are the common case and should not have to accept a parameter they
        ignore.
        """
        import inspect

        try:
            arity = len(inspect.signature(handler).parameters)
        except (TypeError, ValueError):
            arity = 1
        return handler(context, params) if arity >= 2 else handler(params)
