"""Transports: how a message gets from one peer to the other.

Frame-oriented like the Rust trait, not byte-oriented: a transport hands the
peer one whole line and takes one whole line back, so framing is the adapter's
problem and the peer never sees a partial read.

Three adapters, matching the Rust set: this process's own stdio, a spawned
child process, and an in-memory pair for tests.
"""

from __future__ import annotations

import queue
import subprocess
import sys
import threading
from typing import Callable, Protocol, TextIO


class Transport(Protocol):
    """A bidirectional stream of whole protocol messages."""

    def recv_line(self) -> str | None:
        """The next line, or ``None`` at end of stream. Blocks."""

    def send_line(self, line: str) -> None:
        """Write one line. Must be safe to call from any thread."""

    def close(self) -> None:
        """Release the underlying resource. Idempotent."""


class StreamTransport:
    """Newline-delimited JSON over an arbitrary reader and writer."""

    def __init__(self, reader: TextIO, writer: TextIO) -> None:
        self._reader = reader
        self._writer = writer
        self._write_lock = threading.Lock()
        self._closed = False

    def recv_line(self) -> str | None:
        line = self._reader.readline()
        # readline returns "" only at EOF; a blank line is "\n".
        return None if line == "" else line.rstrip("\n").rstrip("\r")

    def send_line(self, line: str) -> None:
        # Serialized because handlers answer on worker threads and may emit
        # notifications while the main thread is writing a request.
        with self._write_lock:
            if self._closed:
                return
            self._writer.write(line + "\n")
            # Flush per message. A protocol peer is interactive, so a buffered
            # response that arrives at process exit is a hang, not latency.
            self._writer.flush()

    def close(self) -> None:
        with self._write_lock:
            self._closed = True
        try:
            self._writer.flush()
        except (ValueError, OSError):
            pass


def stdio() -> StreamTransport:
    """This process's own stdin and stdout.

    Keep stdout clean: only protocol JSON belongs there, logging on stderr.
    """
    return StreamTransport(sys.stdin, sys.stdout)


class ChildTransport:
    """A spawned child process speaking ndjson over its stdin and stdout.

    Its stderr is drained from the moment it starts, sink or no sink: an unread
    pipe blocks a chatty server forever once the buffer fills, and the symptom
    is a hang rather than an error.
    """

    def __init__(
        self,
        command: list[str],
        on_stderr: Callable[[str], None] | None = None,
    ) -> None:
        self._process = subprocess.Popen(  # noqa: S603 - the caller chose the command
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        assert self._process.stdin and self._process.stdout and self._process.stderr
        self._inner = StreamTransport(self._process.stdout, self._process.stdin)

        stderr = self._process.stderr
        self._drain = threading.Thread(
            target=_drain_stderr, args=(stderr, on_stderr), daemon=True
        )
        self._drain.start()

    @property
    def pid(self) -> int:
        return self._process.pid

    def recv_line(self) -> str | None:
        return self._inner.recv_line()

    def send_line(self, line: str) -> None:
        self._inner.send_line(line)

    def close(self) -> None:
        """Shut stdin, let the child exit, kill only if it overstays, then wait
        for the stderr drain.

        Killing first discards whatever stderr was still in the pipe, which is
        exactly the output someone debugging a server that died on startup
        needs.
        """
        self._inner.close()
        try:
            if self._process.stdin:
                self._process.stdin.close()
        except (ValueError, OSError):
            pass
        try:
            self._process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            self._process.kill()
            self._process.wait(timeout=2)
        self._drain.join(timeout=1)


def _drain_stderr(stream: TextIO, sink: Callable[[str], None] | None) -> None:
    try:
        for line in stream:
            if sink is not None:
                sink(line.rstrip("\n"))
    except (ValueError, OSError):
        pass


class _QueueTransport:
    """One half of an in-memory pair."""

    def __init__(self, inbox: queue.Queue, outbox: queue.Queue) -> None:
        self._inbox = inbox
        self._outbox = outbox
        self._closed = False

    def recv_line(self) -> str | None:
        line = self._inbox.get()
        return None if line is None else line

    def send_line(self, line: str) -> None:
        if not self._closed:
            self._outbox.put(line)

    def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        # Tell the far side its stream ended.
        self._outbox.put(None)


def duplex() -> tuple[_QueueTransport, _QueueTransport]:
    """An in-memory transport pair, for driving both sides in one test.

    The two halves are crossed: what one sends, the other receives.
    """
    a_to_b: queue.Queue = queue.Queue()
    b_to_a: queue.Queue = queue.Queue()
    return _QueueTransport(b_to_a, a_to_b), _QueueTransport(a_to_b, b_to_a)
