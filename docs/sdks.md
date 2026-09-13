# Python and TypeScript

Both languages get the same two shapes Rust has, and both can be **either end**
of a connection.

| | `Server` | `Peer` |
|---|---|---|
| concurrency | one request at a time | many, in both directions |
| can send requests | no | yes |
| reverse requests | no | yes |
| fits | a small tool server | a host, or a server that asks questions back |

## Install

```bash
pip install lanok          # Python
npm install @lanok/rpc     # TypeScript
```

## Generate your protocol's types

Nothing in either package is protocol-specific. Your protocol's payload types,
method names, and capability tokens come from its committed artifacts:

```bash
lanok gen python     --schema schema/v1 --out lanok/_generated_myproto.py
lanok gen typescript --schema schema/v1 --out src/generatedMyproto.ts
```

Add `--check` in CI: a protocol change that does not regenerate its SDKs then
fails the build instead of shipping a mismatch.

## A serial server

The common case. It answers one request at a time and compiles no concurrency
model.

```python
from lanok import Server

Server("echo", "1.0").on_request(
    "echo", lambda params: {"text": params["text"].upper()}
).serve()
```

```ts
import { Server } from "@lanok/rpc";

await new Server("echo", "1.0")
  .onRequest("echo", (params) => ({ text: String(params.text).toUpperCase() }))
  .serve();
```

The `initialize` handshake is answered for you, so version and capability
reporting cannot drift between implementations of one protocol.

Streaming progress while a request is still open:

```python
def work(context, params):
    for step in (1, 2, 3):
        context.notify("progress", {"step": step, "of": 3})
    return {"done": True}
```

```ts
server.onRequest("work", (_params, context) => {
  for (const step of [1, 2, 3]) context.notify("progress", { step, of: 3 });
  return { done: true };
});
```

## Driving a server

```python
from lanok import Hello, Router, connect_child

router = Router().on_notification("progress", lambda p: print(p))
ours = Hello("my-host", "1.0", ["ui_ask"])

with connect_child(["./my-server"], router, serve_handshake=ours, request_timeout=30) as peer:
    server = peer.handshake(ours)
    print(peer.request("echo", {"text": "hi", "shout": True}))
```

```ts
import { Router, connectChild } from "@lanok/rpc";

const router = new Router().onNotification("progress", (p) => console.log(p));
const ours = { name: "my-host", protocolVersion: "1.0", capabilities: ["ui_ask"] };

const peer = connectChild(["./my-server"], router, { serveHandshake: ours, requestTimeoutMs: 30_000 });
try {
  const server = await peer.handshake(ours);
  console.log(await peer.request("echo", { text: "hi", shout: true }));
} finally {
  await peer.close();
}
```

`connect_child` / `connectChild` spawns the server and drains its stderr, which
is what stops a chatty server from deadlocking once its pipe buffer fills. Pass
`on_stderr` / `onStderr` to see those lines; when a server dies on startup they
are the only thing to go on.

## A server that asks questions back

A reverse request needs `Peer`, not `Server`: a serial loop cannot wait for a
reply while it is busy producing one.

```python
from lanok import Hello, Peer, Router, stdio

def echo(peer, params):
    peer.notify("echo/progress", {"step": 1, "of": 1})
    if peer.supports("ui_ask"):
        answer = peer.request("ui/ask", {"question": "shout?"})
        shout = answer["answer"] == "yes"
    else:
        shout = params.get("shout", False)
    return {"text": params["text"].upper() if shout else params["text"]}

peer = Peer(
    Router().on_request("echo", echo),
    serve_handshake=Hello("echo", "1.0", ["ui_ask"]),
).connect(stdio())
peer.wait_closed()
```

```ts
import { Peer, Router, stdio } from "@lanok/rpc";

async function echo(params, peer) {
  peer.notify("echo/progress", { step: 1, of: 1 });
  let shout = Boolean(params.shout);
  if (peer.supports("ui_ask")) {
    shout = (await peer.request("ui/ask", { question: "shout?" })).answer === "yes";
  }
  return { text: shout ? params.text.toUpperCase() : params.text };
}

const peer = new Peer(new Router().onRequest("echo", echo), {
  serveHandshake: { name: "echo", protocolVersion: "1.0", capabilities: ["ui_ask"] },
}).connect(stdio());
await peer.closed();
```

Always pass `serve_handshake` / `serveHandshake` on a peer that receives
connections. Capability state lives on the peer, so it is what makes
`supports(..)` true on the responding side, which is exactly what the gate above
needs.

## Handler signatures

Both runtimes accept a handler with or without the peer, so the common case
stays short:

| | params only | with the peer |
|---|---|---|
| Python | `lambda params: ...` | `lambda peer, params: ...` |
| TypeScript | `(params) => ...` | `(params, peer) => ...` |

## Errors

Raise or throw `RpcError`; the loop turns it into an error response. Anything
else becomes an internal error with the message attached, so a handler bug is
reported rather than silently closing the connection.

```python
raise RpcError("`text` must be a string", INVALID_PARAMS)
```

```ts
throw new RpcError("`text` must be a string", codes.invalidParams);
```

## Testing without a process

```python
from lanok import Peer, Router, duplex

a, b = duplex()
server = Peer(Router().on_request("echo", lambda p: p)).connect(b)
with Peer().connect(a) as client:
    assert client.request("echo", {"text": "hi"}) == {"text": "hi"}
```

```ts
const [a, b] = duplex();
const server = new Peer(new Router().onRequest("echo", (p) => p)).connect(b);
const client = new Peer().connect(a);
```

## Keep stdout clean

Only protocol JSON belongs on stdout. Logging goes to stderr. A host sees the
damage as a rising `skipped_lines` / `skippedLines` count, which is the fastest
way to diagnose "my server does nothing".

## Proof, not claim

`just matrix` runs every client against every server across all three
languages: nine combinations, each a full exchange including a reverse request.
It runs in CI.
