"""The symmetric peer.

The Python counterpart of Rust's ``Peer``, and the same idea: there is no client
type and no server type. One peer issues requests and answers them at the same
time, over one connection, and which methods flow in which direction is a
property of the protocol rather than of this code.

Threads, not asyncio, deliberately. The audience is someone writing a study or
an extension as a plain script, and ``peer.request(...)`` returning a value is
what that person expects. An event loop would be a second thing to learn before
answering three methods.

Shape: one reader thread owns the transport's input, routing responses to the
caller that registered the id and dispatching inbound requests onto a worker
pool, so a slow handler never blocks the reader. Writes are serialized by the
transport.
"""

from __future__ import annotations

import itertools
import threading
from concurrent.futures import Future, ThreadPoolExecutor
from dataclasses import dataclass, field
from typing import Any, Callable, Iterable

from ._transport import Transport
from ._version import Version, accepts
from ._wire import (
    METHOD_NOT_FOUND,
    TRANSPORT_CLOSED,
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


@dataclass
class Hello:
    """Who a peer is and what it supports.

    ``protocol_version`` is the one required field: a peer that will not say
    which version it speaks cannot be negotiated with, and defaulting it turns
    that into a mystery failure three methods later.
    """

    name: str
    protocol_version: str
    capabilities: list[str] = field(default_factory=list)
    info: Any = None

    def to_wire(self) -> dict[str, Any]:
        payload: dict[str, Any] = {
            "name": self.name,
            "protocol_version": self.protocol_version,
            "capabilities": sorted(self.capabilities),
        }
        if self.info is not None:
            payload["info"] = self.info
        return payload

    @staticmethod
    def from_wire(value: Any) -> "Hello":
        value = value if isinstance(value, dict) else {}
        version = value.get("protocol_version")
        if not isinstance(version, str):
            raise RpcError("peer did not state a protocol_version")
        capabilities = value.get("capabilities")
        return Hello(
            name=str(value.get("name", "")),
            protocol_version=version,
            capabilities=list(capabilities) if isinstance(capabilities, list) else [],
            info=value.get("info"),
        )

    def supports(self, token: str) -> bool:
        return token in self.capabilities


class Router:
    """A method table, for peers that dispatch by hand.

    Request handlers take ``params`` or ``(peer, params)``; the second form is
    how a handler issues a reverse request or a notification mid-flight.
    """

    def __init__(self) -> None:
        self._requests: dict[str, Callable[..., Any]] = {}
        self._notifications: dict[str, Callable[..., Any]] = {}

    def on_request(self, method: str, handler: Callable[..., Any]) -> "Router":
        self._requests[method] = handler
        return self

    def on_notification(self, method: str, handler: Callable[..., Any]) -> "Router":
        self._notifications[method] = handler
        return self

    def request(self, peer: "Peer", method: str, params: Any) -> Any:
        handler = self._requests.get(method)
        if handler is None:
            raise RpcError(f"method not found: {method}", METHOD_NOT_FOUND)
        return _invoke(handler, peer, params)

    def notification(self, peer: "Peer", method: str, params: Any) -> None:
        handler = self._notifications.get(method)
        if handler is not None:
            _invoke(handler, peer, params)


def _invoke(handler: Callable[..., Any], peer: "Peer", params: Any) -> Any:
    """Call a handler with or without the peer, whichever it accepts."""
    import inspect

    try:
        arity = len(inspect.signature(handler).parameters)
    except (TypeError, ValueError):
        arity = 1
    return handler(peer, params) if arity >= 2 else handler(params)


class Peer:
    """A live connection to another peer.

    >>> from lanok import duplex
    >>> a, b = duplex()
    >>> server = Peer(Router().on_request("echo", lambda p: p)).connect(b)
    >>> client = Peer().connect(a)
    >>> client.request("echo", {"text": "hi"})
    {'text': 'hi'}
    >>> client.close(); server.close()
    """

    def __init__(
        self,
        handler: Router | None = None,
        *,
        serve_handshake: Hello | None = None,
        request_timeout: float | None = None,
        cancel_notification: str | None = None,
        max_workers: int = 8,
    ) -> None:
        self._handler = handler if handler is not None else Router()
        self._serve_handshake = serve_handshake
        self._request_timeout = request_timeout
        self._cancel_method = cancel_notification

        self._transport: Transport | None = None
        self._ids = itertools.count(1)
        self._pending: dict[int, Future] | None = {}
        self._lock = threading.Lock()
        self._workers = ThreadPoolExecutor(max_workers=max_workers)
        self._reader: threading.Thread | None = None
        self._closed = threading.Event()

        self.peer_hello: Hello | None = None
        self.skipped_lines = 0

    # -- lifecycle ---------------------------------------------------------

    def connect(self, transport: Transport) -> "Peer":
        """Start serving over `transport`. Returns self, so it chains."""
        self._transport = transport
        self._reader = threading.Thread(target=self._pump, daemon=True)
        self._reader.start()
        return self

    def close(self) -> None:
        """End the connection and fail every pending request at once.

        At once, rather than letting each wait out its own timeout: a caller
        whose connection is gone should learn immediately.
        """
        if self._closed.is_set():
            return
        self._closed.set()
        if self._transport is not None:
            self._transport.close()
        self._fail_all_pending(RpcError("the connection closed", TRANSPORT_CLOSED))
        self._workers.shutdown(wait=False)

    @property
    def is_closed(self) -> bool:
        return self._closed.is_set()

    def wait_closed(self, timeout: float | None = None) -> bool:
        """Block until the connection ends. A server's main loop."""
        return self._closed.wait(timeout)

    def __enter__(self) -> "Peer":
        return self

    def __exit__(self, *_exc: object) -> None:
        self.close()

    # -- sending -----------------------------------------------------------

    def request(self, method: str, params: Any = None, *, timeout: float | None = None) -> Any:
        """Issue a request and wait for its response.

        Raises `RpcError` on a peer error, a timeout, or a closed connection.
        """
        if self._transport is None:
            raise RpcError("peer is not connected", TRANSPORT_CLOSED)

        identifier = next(self._ids)
        waiter: Future = Future()
        with self._lock:
            if self._pending is None:
                raise RpcError("the connection closed", TRANSPORT_CLOSED)
            self._pending[identifier] = waiter

        self._transport.send_line(_wire.request(identifier, method, params))

        limit = timeout if timeout is not None else self._request_timeout
        try:
            return waiter.result(timeout=limit)
        except TimeoutError:
            # The caller gave up, so free the slot and, when the protocol says
            # how, tell the peer to stop working.
            self._abandon(identifier)
            raise RpcError(f"no response to `{method}` within {limit}s", TRANSPORT_CLOSED) from None

    def notify(self, method: str, params: Any = None) -> None:
        """Send a notification. Fire and forget, by definition."""
        if self._transport is not None and not self._closed.is_set():
            self._transport.send_line(_wire.notification(method, params))

    def handshake(self, ours: Hello, minimum: str | None = None) -> Hello:
        """Send `initialize`, check the reply's version, record what the peer
        can do, then send `initialized`.

        After this returns, :meth:`supports` is populated, which is what a
        reverse request needs to know.
        """
        theirs = Hello.from_wire(self.request(INITIALIZE, ours.to_wire()))

        current = Version.parse(ours.protocol_version)
        floor = Version.parse(minimum) if minimum else Version(current.major, 0)
        if not accepts(current, floor, Version.parse(theirs.protocol_version)):
            raise RpcError(
                f"peer speaks {theirs.protocol_version} but this build speaks "
                f"{ours.protocol_version} (min {floor})",
                _wire.VERSION_INCOMPATIBLE,
            )

        self.peer_hello = theirs
        # Only after accepting: a peer told the connection is live before the
        # version check has been told something we then hang up on.
        self.notify(INITIALIZED)
        return theirs

    def supports(self, token: str) -> bool:
        """Whether the peer advertised `token` during the handshake."""
        return self.peer_hello is not None and self.peer_hello.supports(token)

    # -- internals ---------------------------------------------------------

    def _abandon(self, identifier: int) -> None:
        with self._lock:
            if self._pending is not None:
                self._pending.pop(identifier, None)
        if self._cancel_method:
            # Best effort: no slot is registered for an ack, so if the peer
            # answers, the reader drops it.
            self.notify(self._cancel_method, {"id": identifier})

    def _fail_all_pending(self, error: RpcError) -> None:
        with self._lock:
            pending, self._pending = self._pending, None
        for waiter in (pending or {}).values():
            if not waiter.done():
                waiter.set_exception(error)

    def _complete(self, identifier: Any, message: Response) -> None:
        with self._lock:
            waiter = (self._pending or {}).pop(identifier, None)
        # No waiter means the caller already gave up. Dropping the response is
        # correct: a best-effort cancel races exactly this way.
        if waiter is None or waiter.done():
            return
        if message.error is not None:
            waiter.set_exception(message.error)
        else:
            waiter.set_result(message.result)

    def _pump(self) -> None:
        transport = self._transport
        assert transport is not None
        try:
            while True:
                line = transport.recv_line()
                if line is None:
                    break
                if not line.strip():
                    continue
                message = classify(line)

                if isinstance(message, Malformed):
                    # One bad line should not take down a healthy connection.
                    self.skipped_lines += 1
                elif isinstance(message, Response):
                    self._complete(message.id, message)
                elif isinstance(message, Notification):
                    if message.method != INITIALIZED:
                        self._workers.submit(self._handle_notification, message)
                else:
                    assert isinstance(message, Request)
                    self._workers.submit(self._handle_request, message)
        finally:
            self.close()

    def _handle_notification(self, message: Notification) -> None:
        try:
            self._handler.notification(self, message.method, message.params)
        except Exception:  # noqa: BLE001 - a notification has nobody to tell
            pass

    def _handle_request(self, message: Request) -> None:
        transport = self._transport
        if transport is None:
            return
        try:
            value = self._answer(message)
        except RpcError as e:
            transport.send_line(_wire.error(message.id, e))
            return
        except Exception as e:  # noqa: BLE001 - a handler bug is reportable
            transport.send_line(_wire.error(message.id, RpcError(f"handler raised: {e}")))
            return
        transport.send_line(_wire.result(message.id, value))

    def _answer(self, message: Request) -> Any:
        # The peer, not the handler, answers the handshake when configured to.
        # Capability state lives here, so answering here is what makes
        # supports() true on the responding side.
        if message.method == INITIALIZE and self._serve_handshake is not None:
            try:
                self.peer_hello = Hello.from_wire(message.params)
            except RpcError:
                pass
            return self._serve_handshake.to_wire()
        return self._handler.request(self, message.method, message.params)


def connect_child(
    command: Iterable[str],
    handler: Router | None = None,
    *,
    on_stderr: Callable[[str], None] | None = None,
    **options: Any,
) -> Peer:
    """Spawn a server and connect a peer to it, in one call."""
    from ._transport import ChildTransport

    return Peer(handler, **options).connect(ChildTransport(list(command), on_stderr))
