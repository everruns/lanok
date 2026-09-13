# lanok (Python)

Build JSON-RPC 2.0 protocols in Python, on the same wire contract the Rust core
implements. Python can be **either end** of a connection.

## A serial server

One request at a time, no concurrency model to learn. The right answer for a
small tool server.

```python
from lanok import Server

Server("echo", "1.0").on_request(
    "echo", lambda params: {"text": params["text"].upper()}
).serve()
```

The handshake is answered for you, so version and capability reporting cannot
drift between implementations of one protocol. Keep stdout clean: only protocol
JSON belongs there, and logging belongs on stderr.

Streaming progress while a request is open takes a two-argument handler:

```python
def work(context, params):
    for step in (1, 2, 3):
        context.notify("progress", {"step": step, "of": 3})
    return {"done": True}
```

## Driving a server

```python
from lanok import Hello, Router, connect_child

ours = Hello("my-host", "1.0", ["ui_ask"])
router = Router().on_request("ui/ask", lambda p: {"answer": "yes"})

with connect_child(["./my-server"], router, serve_handshake=ours) as peer:
    peer.handshake(ours)
    print(peer.request("echo", {"text": "hi"}))
```

## A server that asks questions back

A reverse request needs `Peer`, not `Server`: a serial loop cannot wait for a
reply while it is busy producing one.

```python
from lanok import Hello, Peer, Router, stdio

def echo(peer, params):
    if peer.supports("ui_ask"):
        answer = peer.request("ui/ask", {"question": "shout?"})
        ...

peer = Peer(
    Router().on_request("echo", echo),
    serve_handshake=Hello("echo", "1.0", ["ui_ask"]),
).connect(stdio())
peer.wait_closed()
```

## Generated types

A protocol's own wire types are generated, never hand-written:

```bash
lanok gen python --schema path/to/schema/v1 --out lanok/_generated_myproto.py
```

Full guide: [docs/sdks.md](https://github.com/everruns/lanok/blob/main/docs/sdks.md).

Tests: `python3 -m pytest -q`.
