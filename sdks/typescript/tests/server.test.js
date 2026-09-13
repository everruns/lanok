// The serve loop, driven over in-memory streams.

import assert from "node:assert/strict";
import { Readable, Writable } from "node:stream";
import { test } from "node:test";

import { RpcError, Server, codes } from "../dist/index.js";
import { METHODS, PROTOCOL, VERSION, method } from "../dist/generatedEcho.js";

async function run(server, ...lines) {
  const input = Readable.from(lines.map((line) => `${line}\n`));
  const written = [];
  const output = new Writable({
    write(chunk, _encoding, done) {
      written.push(chunk.toString());
      done();
    },
  });
  await server.serve(input, output);
  return written
    .join("")
    .split("\n")
    .filter((line) => line.trim() !== "")
    .map((line) => JSON.parse(line));
}

function echoServer() {
  return new Server("echo", "1.1", { capabilities: ["uppercase"] }).onRequest("echo", (params) => {
    if (typeof params?.text !== "string") {
      throw new RpcError("`text` is required", codes.invalidParams);
    }
    return { text: params.text.toUpperCase() };
  });
}

test("answers the handshake without the author writing one", async () => {
  const [hello] = await run(
    echoServer(),
    JSON.stringify({ id: 1, method: "initialize", params: { name: "host", protocol_version: "1.0" } }),
  );
  assert.equal(hello.result.name, "echo");
  assert.equal(hello.result.protocol_version, "1.1");
  assert.deepEqual(hello.result.capabilities, ["uppercase"]);
});

test("dispatches registered methods", async () => {
  const [response] = await run(
    echoServer(),
    JSON.stringify({ id: 7, method: "echo", params: { text: "hi" } }),
  );
  assert.equal(response.result.text, "HI");
});

test("an unknown method is an error, not a silence", async () => {
  const [response] = await run(echoServer(), JSON.stringify({ id: 2, method: "nope" }));
  assert.equal(response.error.code, codes.methodNotFound);
});

test("a handler error becomes an error response", async () => {
  const [response] = await run(echoServer(), JSON.stringify({ id: 3, method: "echo", params: {} }));
  assert.equal(response.error.code, codes.invalidParams);
});

test("a handler bug is reported rather than fatal", async () => {
  const server = new Server("s", "1.0").onRequest("boom", () => {
    throw new Error("kaboom");
  });
  const [response] = await run(server, JSON.stringify({ id: 1, method: "boom" }));
  assert.match(response.error.message, /handler raised/);
});

test("progress notifications arrive before the response", async () => {
  const server = new Server("w", "1.0").onRequest("work", (_params, context) => {
    for (const step of [1, 2, 3]) context.notify("progress", { step });
    return "done";
  });
  const messages = await run(server, JSON.stringify({ id: 1, method: "work" }));
  assert.deepEqual(
    messages.map((m) => m.method ?? null),
    ["progress", "progress", "progress", null],
  );
});

test("handlers can see what the peer advertised", async () => {
  const server = new Server("s", "1.0").onRequest("check", (_params, context) => ({
    streams: context.peerSupports("streaming"),
    who: context.peerName,
  }));
  const messages = await run(
    server,
    JSON.stringify({
      id: 1,
      method: "initialize",
      params: { name: "host", protocol_version: "1.0", capabilities: ["streaming"] },
    }),
    JSON.stringify({ id: 2, method: "check" }),
  );
  assert.deepEqual(messages[1].result, { streams: true, who: "host" });
});

test("junk lines are skipped rather than fatal", async () => {
  const server = echoServer();
  const messages = await run(
    server,
    "not json at all",
    JSON.stringify({ id: 1, method: "echo", params: { text: "ok" } }),
  );
  assert.equal(messages[0].result.text, "OK");
  assert.equal(server.skippedLines, 1);
});

test("an unsolicited response is dropped, not answered", async () => {
  assert.deepEqual(await run(echoServer(), JSON.stringify({ id: 1, result: "unexpected" })), []);
});

test("notifications are observed and never answered", async () => {
  const seen = [];
  const server = new Server("s", "1.0").onNotification("tick", (params) => seen.push(params));
  assert.deepEqual(await run(server, JSON.stringify({ method: "tick", params: { n: 1 } })), []);
  assert.deepEqual(seen, [{ n: 1 }]);
});

test("the generated types describe the protocol they came from", () => {
  assert.equal(PROTOCOL, "echo");
  assert.equal(VERSION, "1.0");
  assert.equal(method.echoProgress, "echo/progress");
  assert.equal(METHODS["ui/ask"].direction, "responder");
  assert.equal(METHODS["ui/ask"].requires, "ui_ask");
  assert.equal(METHODS.echo.requires, null);
});
