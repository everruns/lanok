"""The symmetric peer, driven over an in-memory duplex.

Mirrors crates/lanok-peer/tests/peer.rs case for case, so a divergence in
behaviour shows up as a failure in one language and not the other.
"""

import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import pytest  # noqa: E402

from lanok import (  # noqa: E402
    METHOD_NOT_FOUND,
    TRANSPORT_CLOSED,
    VERSION_INCOMPATIBLE,
    Hello,
    Peer,
    Router,
    RpcError,
    duplex,
)


def pair(client_router=None, server_router=None, **options):
    a, b = duplex()
    server = Peer(server_router or Router(), **options.pop("server_options", {})).connect(b)
    client = Peer(client_router or Router(), **options).connect(a)
    return client, server


def test_a_request_gets_its_response():
    client, server = pair(server_router=Router().on_request("echo", lambda p: p))
    try:
        assert client.request("echo", {"text": "hi"}) == {"text": "hi"}
    finally:
        client.close()
        server.close()


def test_both_directions_carry_requests_at_once():
    # The whole point of the symmetric peer: the server calls back into the
    # client while it is answering the client's own request.
    a, b = duplex()

    client = Peer(Router().on_request("ui/ask", lambda _p: {"answer": "blue"})).connect(a)
    server = Peer(
        Router().on_request(
            "tool/call",
            lambda peer, _p: {"used": peer.request("ui/ask", {"q": "colour?"})["answer"]},
        )
    ).connect(b)
    try:
        assert client.request("tool/call", {}) == {"used": "blue"}
    finally:
        client.close()
        server.close()


def test_slow_handlers_do_not_block_other_requests():
    def slow(_params):
        time.sleep(0.3)
        return "slow"

    client, server = pair(
        server_router=Router().on_request("slow", slow).on_request("fast", lambda _p: "fast")
    )
    try:
        result: list = []
        thread = threading.Thread(target=lambda: result.append(client.request("slow")))
        thread.start()
        time.sleep(0.05)
        # Issued second, must come back first.
        assert client.request("fast") == "fast"
        thread.join(timeout=5)
        assert result == ["slow"]
    finally:
        client.close()
        server.close()


def test_an_unknown_method_is_an_error_not_a_hang():
    client, server = pair()
    try:
        with pytest.raises(RpcError) as raised:
            client.request("nope")
        assert raised.value.code == METHOD_NOT_FOUND
    finally:
        client.close()
        server.close()


def test_a_handler_error_reaches_the_caller_intact():
    def fail(_params):
        raise RpcError("upstream is busy", -32050).retryable()

    client, server = pair(server_router=Router().on_request("fail", fail))
    try:
        with pytest.raises(RpcError) as raised:
            client.request("fail")
        assert raised.value.code == -32050
        assert raised.value.message == "upstream is busy"
        assert raised.value.is_retryable, "the retryable hint must survive the wire"
    finally:
        client.close()
        server.close()


def test_a_handler_bug_is_reported_rather_than_fatal():
    client, server = pair(server_router=Router().on_request("boom", lambda _p: 1 / 0))
    try:
        with pytest.raises(RpcError) as raised:
            client.request("boom")
        assert "handler raised" in raised.value.message
    finally:
        client.close()
        server.close()


def test_closing_the_connection_fails_every_pending_request_at_once():
    client, server = pair(server_router=Router().on_request("never", lambda _p: time.sleep(30)))
    failure: list = []

    def call():
        try:
            client.request("never")
        except RpcError as e:
            failure.append(e)

    thread = threading.Thread(target=call)
    thread.start()
    time.sleep(0.1)
    server.close()

    # Without the drain this would hang until the request's own timeout, which
    # is the bug the drain exists to prevent.
    thread.join(timeout=5)
    assert not thread.is_alive(), "pending requests must fail as soon as the connection ends"
    assert failure and failure[0].code == TRANSPORT_CLOSED
    client.close()


def test_a_request_past_its_timeout_fails_locally():
    client, server = pair(
        server_router=Router().on_request("slow", lambda _p: time.sleep(30)),
        request_timeout=0.1,
    )
    try:
        with pytest.raises(RpcError):
            client.request("slow")
    finally:
        client.close()
        server.close()


def test_abandoning_a_request_cancels_it_on_the_peer():
    cancels: list = []
    client, server = pair(
        server_router=Router()
        .on_request("slow", lambda _p: time.sleep(30))
        .on_notification("$/cancel", lambda p: cancels.append(p)),
        request_timeout=0.1,
        cancel_notification="$/cancel",
    )
    try:
        with pytest.raises(RpcError):
            client.request("slow")
        for _ in range(100):
            if cancels:
                break
            time.sleep(0.01)
        assert len(cancels) == 1, "a caller that gave up must tell the peer to stop working"
    finally:
        client.close()
        server.close()


def test_the_handshake_records_version_and_capabilities():
    a, b = duplex()
    server = Peer(
        serve_handshake=Hello("test-server", "1.2", ["streaming", "tools"])
    ).connect(b)
    client = Peer().connect(a)
    try:
        theirs = client.handshake(Hello("test-client", "1.0"))
        assert theirs.name == "test-server"
        assert theirs.protocol_version == "1.2"
        assert client.supports("tools")
        assert not client.supports("ui_ask")
        # The responding side learns about the caller too, which is what a
        # reverse request needs to know.
        assert server.peer_hello is not None
        assert server.peer_hello.name == "test-client"
    finally:
        client.close()
        server.close()


def test_an_incompatible_major_is_refused():
    a, b = duplex()
    server = Peer(serve_handshake=Hello("future", "2.0")).connect(b)
    client = Peer().connect(a)
    try:
        with pytest.raises(RpcError) as raised:
            client.handshake(Hello("client", "1.0"))
        assert raised.value.code == VERSION_INCOMPATIBLE
        # Nothing was recorded, so a stub cannot be fooled into thinking the
        # peer supports something on a connection that was refused.
        assert not client.supports("anything")
    finally:
        client.close()
        server.close()


def test_notifications_flow_without_a_response():
    seen: list = []
    client, server = pair(server_router=Router().on_notification("tick", lambda p: seen.append(p)))
    try:
        for _ in range(3):
            client.notify("tick", {})
        for _ in range(100):
            if len(seen) == 3:
                break
            time.sleep(0.01)
        assert len(seen) == 3
    finally:
        client.close()
        server.close()


def test_ids_are_per_direction():
    # Both sides number from 1 independently. If responses were keyed by id
    # alone across directions, these would collide.
    a, b = duplex()
    client = Peer(Router().on_request("from_b", lambda _p: "a answered")).connect(a)
    server = Peer(
        Router().on_request("from_a", lambda peer, _p: {"nested": peer.request("from_b")})
    ).connect(b)
    try:
        assert client.request("from_a") == {"nested": "a answered"}
    finally:
        client.close()
        server.close()


def test_junk_lines_are_skipped_rather_than_fatal():
    a, b = duplex()
    server = Peer(Router().on_request("echo", lambda p: p)).connect(b)
    client = Peer().connect(a)
    try:
        a.send_line("not json at all")
        assert client.request("echo", {"ok": True}) == {"ok": True}
        for _ in range(100):
            if server.skipped_lines:
                break
            time.sleep(0.01)
        assert server.skipped_lines == 1
    finally:
        client.close()
        server.close()


def test_a_peer_is_a_context_manager():
    a, b = duplex()
    server = Peer(Router().on_request("echo", lambda p: p)).connect(b)
    with Peer().connect(a) as client:
        assert client.request("echo", 1) == 1
    assert client.is_closed
    server.close()
