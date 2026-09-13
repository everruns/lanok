# lanok (Python)

Build JSON-RPC 2.0 protocols in Python, on the same wire contract the Rust core
implements.

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

A protocol's own wire types are generated, never hand-written:

```bash
lanok gen python --schema path/to/schema/v1 --out lanok/_generated_myproto.py
```

Tests: `python3 -m pytest -q`.
