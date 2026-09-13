"""The framing rules, which must match the Rust core exactly."""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import pytest  # noqa: E402

from lanok import (  # noqa: E402
    INTERNAL_ERROR,
    Malformed,
    Notification,
    Request,
    Response,
    RpcError,
    Version,
    accepts,
    classify,
)
from lanok import _wire  # noqa: E402


def test_classifies_by_field_not_by_direction():
    assert isinstance(classify('{"id":1,"method":"a"}'), Request)
    assert isinstance(classify('{"method":"a"}'), Notification)
    assert isinstance(classify('{"id":1,"result":7}'), Response)
    assert isinstance(classify('{"id":1,"error":{"code":-1,"message":"x"}}'), Response)


def test_a_null_id_reads_as_a_notification():
    # JSON-RPC uses a null id for "could not determine the id"; routing a
    # response to it would route to nothing.
    assert isinstance(classify('{"method":"a","id":null}'), Notification)


def test_inbound_jsonrpc_field_is_optional():
    parsed = classify('{"id":4,"method":"run"}')
    assert isinstance(parsed, Request)
    assert parsed.method == "run"


def test_every_outbound_message_carries_jsonrpc_2_0():
    for line in [
        _wire.request(1, "a", {}),
        _wire.notification("a"),
        _wire.result(1, 7),
        _wire.error(1, RpcError("x")),
    ]:
        assert json.loads(line)["jsonrpc"] == "2.0"


def test_a_successful_null_result_stays_classifiable():
    line = _wire.result(1, None)
    assert json.loads(line)["result"] is None
    parsed = classify(line)
    assert isinstance(parsed, Response) and parsed.error is None


def test_string_ids_round_trip():
    parsed = classify('{"id":"abc","method":"a"}')
    assert isinstance(parsed, Request) and parsed.id == "abc"


def test_a_malformed_error_object_still_reads_as_failure():
    parsed = classify('{"id":1,"error":"just a string"}')
    assert isinstance(parsed, Response) and parsed.error is not None


def test_unclassifiable_lines_are_reported_not_raised():
    for line in ["{}", "[]", "nope", '{"id":1,"method":5}']:
        assert isinstance(classify(line), Malformed)


def test_retryable_rides_in_data_and_stays_conformant():
    error = RpcError("rate limited").retryable()
    assert error.is_retryable
    wire = error.to_wire()
    assert sorted(wire) == ["code", "data", "message"]
    assert classify(_wire.error(1, error)).error.is_retryable


def test_a_bare_error_defaults_to_internal():
    parsed = classify('{"id":1,"error":{"message":"boom"}}')
    assert parsed.error.code == INTERNAL_ERROR


@pytest.mark.parametrize("bad", ["1", "1.2.3", "", "x.y"])
def test_rejects_a_malformed_version(bad):
    with pytest.raises(ValueError):
        Version.parse(bad)


def test_negotiation_matches_the_rust_contract():
    current, minimum = Version.parse("1.5"), Version.parse("1.2")
    assert accepts(current, minimum, Version.parse("1.2"))
    # A newer minor is fine: additions are ignorable by contract.
    assert accepts(current, minimum, Version.parse("1.9"))
    assert not accepts(current, minimum, Version.parse("1.1"))
    assert not accepts(current, minimum, Version.parse("2.0"))
