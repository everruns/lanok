"""MAJOR.MINOR negotiation, matching the Rust contract exactly."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, order=True)
class Version:
    major: int
    minor: int

    @staticmethod
    def parse(text: str) -> "Version":
        parts = text.split(".")
        if len(parts) != 2:
            raise ValueError(f"`{text}` is not a MAJOR.MINOR protocol version")
        try:
            return Version(int(parts[0]), int(parts[1]))
        except ValueError as e:
            raise ValueError(f"`{text}` is not a MAJOR.MINOR protocol version") from e

    def __str__(self) -> str:
        return f"{self.major}.{self.minor}"

    def compatible_with(self, other: "Version") -> bool:
        return self.major == other.major


def accepts(current: Version, minimum: Version, peer: Version) -> bool:
    """Whether a build speaking `current`, supporting back to `minimum`, can
    talk to a peer speaking `peer`.

    A newer minor is always accepted: by the additive contract everything this
    build understands is still there, and what it does not understand it
    ignores.
    """
    return peer.major == current.major and peer >= minimum
