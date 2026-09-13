# @lanok/rpc

Build JSON-RPC 2.0 protocols in TypeScript, on the same wire contract the Rust
core implements.

```ts
import { Server } from "@lanok/rpc";

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

A protocol's own wire types are generated, never hand-written:

```bash
lanok gen typescript --schema path/to/schema/v1 --out src/generatedMyproto.ts
```

Build and test: `npm test`.
