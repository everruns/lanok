#!/usr/bin/env node
/**
 * The echo protocol in TypeScript's runtime, on the symmetric peer.
 *
 * The difference from `server.mjs` is the last thing a non-Rust implementation
 * was missing: this one asks the caller a question mid-request. `ui/ask` is a
 * reverse request, sent by the responder, and a serial server cannot do it
 * because it cannot wait for a reply while producing one.
 *
 *   cargo run -p echo-protocol --bin echo-client -- \
 *       --server node examples/echo-typescript/peerServer.mjs
 */

import { Peer, Router, RpcError, codes, stdio } from "../../sdks/typescript/dist/index.js";
import { PROTOCOL, VERSION, capability, method } from "../../sdks/typescript/dist/generatedEcho.js";

async function echo(params, peer) {
  if (typeof params !== "object" || params === null || typeof params.text !== "string") {
    throw new RpcError("`text` must be a string", codes.invalidParams);
  }

  for (const step of [1, 2, 3]) {
    peer.notify(method.echoProgress, { step, of: 3 });
  }

  // The reverse request. Gated on the caller having advertised it, so an older
  // host that cannot answer is not left hanging.
  let shout = Boolean(params.shout);
  if (peer.supports(capability.uiAsk)) {
    const answer = await peer.request(method.uiAsk, { question: "shout?" });
    shout = answer?.answer === "yes";
  }

  return { text: shout ? params.text.toUpperCase() : params.text };
}

const router = new Router().onRequest(method.echo, echo).onRequest(method.ping, () => null);

const peer = new Peer(router, {
  serveHandshake: {
    name: PROTOCOL,
    protocolVersion: VERSION,
    capabilities: [capability.uiAsk],
  },
}).connect(stdio());

// Serve until the caller closes our stdin.
await peer.closed();
