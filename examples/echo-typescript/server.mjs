#!/usr/bin/env node
/**
 * The echo protocol, in TypeScript's runtime.
 *
 * As with the Python one, the point is what is absent: no JSON-RPC loop, no
 * framing, no handshake. The same conformance vectors that check the Rust and
 * Python servers check this one.
 */

import { RpcError, Server, codes } from "../../sdks/typescript/dist/index.js";
import { METHODS, PROTOCOL, VERSION, capability, method } from "../../sdks/typescript/dist/generatedEcho.js";

function echo(params, context) {
  if (typeof params !== "object" || params === null || typeof params.text !== "string") {
    throw new RpcError("`text` must be a string", codes.invalidParams);
  }

  for (const step of [1, 2, 3]) {
    context.notify(method.echoProgress, { step, of: 3 });
  }

  return { text: params.shout ? params.text.toUpperCase() : params.text };
}

if (PROTOCOL !== "echo" || !(method.uiAsk in METHODS)) {
  throw new Error("generated artifacts do not describe the echo protocol");
}

await new Server(PROTOCOL, VERSION, { capabilities: [capability.uiAsk] })
  .onRequest(method.echo, echo)
  .onRequest(method.ping, () => null)
  .serve();
