# lanok

Build JSON-RPC 2.0 protocols in TypeScript, on the same wire contract the Rust
core implements. TypeScript can be **either end** of a connection.

## A serial server

One request at a time, no concurrency model to reason about. The right answer
for a small tool server.

```ts
import { Server } from "lanok";

await new Server("echo", "1.0")
  .onRequest("echo", (params) => ({ text: String(params.text).toUpperCase() }))
  .serve();
```

The handshake is answered for you, so version and capability reporting cannot
drift between implementations of one protocol. Keep stdout clean: only protocol
JSON belongs there, and logging belongs on stderr.

Streaming progress while a request is open uses the handler's context:

```ts
server.onRequest("work", (_params, context) => {
  for (const step of [1, 2, 3]) context.notify("progress", { step, of: 3 });
  return { done: true };
});
```

## Driving a server

```ts
import { Router, connectChild } from "lanok";

const ours = { name: "my-host", protocolVersion: "1.0", capabilities: ["ui_ask"] };
const router = new Router().onRequest("ui/ask", () => ({ answer: "yes" }));

const peer = connectChild(["./my-server"], router, { serveHandshake: ours });
await peer.handshake(ours);
console.log(await peer.request("echo", { text: "hi" }));
await peer.close();
```

## A server that asks questions back

A reverse request needs `Peer`, not `Server`: a serial loop cannot wait for a
reply while it is busy producing one.

```ts
import { Peer, Router, stdio } from "lanok";

async function echo(params, peer) {
  if (peer.supports("ui_ask")) {
    const answer = await peer.request("ui/ask", { question: "shout?" });
    // ...
  }
}

const peer = new Peer(new Router().onRequest("echo", echo), {
  serveHandshake: { name: "echo", protocolVersion: "1.0", capabilities: ["ui_ask"] },
}).connect(stdio());
await peer.closed();
```

## Generated types

A protocol's own wire types are generated, never hand-written:

```bash
lanok gen typescript --schema path/to/schema/v1 --out src/generatedMyproto.ts
```

Full guide: [docs/sdks.md](https://github.com/everruns/lanok/blob/main/docs/sdks.md).

Build and test: `npm test`.
