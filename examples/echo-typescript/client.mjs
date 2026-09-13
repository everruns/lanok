#!/usr/bin/env node
/**
 * Drive the echo protocol from TypeScript's runtime.
 *
 * The other half of the story: TypeScript is not only something you can
 * implement a server in, it is something you can drive one from. This spawns
 * whatever server it is pointed at, runs the handshake, calls forward, and
 * answers the reverse `ui/ask` request the server sends back.
 *
 *   node examples/echo-typescript/client.mjs ./target/debug/echo-server --async
 */

import { Router, connectChild } from "../../sdks/typescript/dist/index.js";
import { PROTOCOL, VERSION, capability, method } from "../../sdks/typescript/dist/generatedEcho.js";

let progressSeen = 0;

const router = new Router()
  .onRequest(method.uiAsk, (params) => {
    // The reverse request: the server is asking us.
    console.log(`server asks: ${params.question}`);
    return { answer: "yes" };
  })
  .onNotification(method.echoProgress, (params) => {
    progressSeen += 1;
    console.log(`progress ${params.step}/${params.of}`);
  });

const command = process.argv.slice(2);
const ours = {
  name: "echo-client-typescript",
  protocolVersion: VERSION,
  capabilities: [capability.uiAsk],
};

const peer = connectChild(command.length > 0 ? command : ["./target/debug/echo-server", "--async"], router, {
  serveHandshake: ours,
  requestTimeoutMs: 30_000,
  onStderr: (line) => console.error(`[server] ${line}`),
});

try {
  const server = await peer.handshake(ours);
  console.log(
    `connected to ${server.name} speaking ${server.protocolVersion} ` +
      `(${server.capabilities?.length ?? 0} capabilities)`,
  );

  await peer.request(method.ping);
  console.log("ping ok");

  const result = await peer.request(method.echo, { text: "hello from typescript", shout: true });
  console.log(`echo -> ${result.text}`);

  if (result.text !== "HELLO FROM TYPESCRIPT") throw new Error(`unexpected result: ${result.text}`);
  if (progressSeen !== 3) throw new Error(`expected 3 progress notifications, saw ${progressSeen}`);
  if (PROTOCOL !== "echo") throw new Error("generated artifacts do not describe the echo protocol");
} finally {
  await peer.close();
}

console.log(
  `ok: typescript client, ${progressSeen} progress notifications, reverse channel exercised`,
);
