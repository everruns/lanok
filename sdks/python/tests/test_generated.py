"""The generated wire types must describe the protocol they came from."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from lanok._generated_echo import METHODS, MIN_VERSION, PROTOCOL, UI_ASK, VERSION, method  # noqa: E402


def test_constants_match_the_declaration():
    assert PROTOCOL == "echo"
    assert VERSION == "1.0"
    assert MIN_VERSION == "1.0"
    assert UI_ASK == "ui_ask"


def test_every_method_name_is_available_without_string_literals():
    assert method.ECHO == "echo"
    assert method.ECHO_PROGRESS == "echo/progress"
    assert method.UI_ASK == "ui/ask"


def test_directions_and_gating_survive_generation():
    assert METHODS["echo"]["direction"] == "initiator"
    assert METHODS["ui/ask"]["direction"] == "responder"
    assert METHODS["ui/ask"]["requires"] == "ui_ask"
    assert METHODS["echo/progress"]["kind"] == "notification"
    assert METHODS["echo"]["requires"] is None
