// The symmetric peer, driven over an in-memory duplex.
//
// Mirrors crates/lanok-peer/tests/peer.rs and sdks/python/tests/test_peer.py
// case for case, so a divergence shows up as a failure in one language only.

import assert from "node:assert/strict";
import { test } from "node:test";

import { Peer, Router, RpcError, codes, duplex } from "../dist/index.js";

function pair(clientRouter, serverRouter, options = {}, serverOptions = {}) {
  const [a, b] = duplex();
  const server = new Peer(serverRouter ?? new Router(), serverOptions).connect(b);
  const client = new Peer(clientRouter ?? new Router(), options).connect(a);
  return { client, server };
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

test("a request gets its response", async () => {
  const { client, server } = pair(null, new Router().onRequest("echo", (p) => p));
  assert.deepEqual(await client.request("echo", { text: "hi" }), { text: "hi" });
  await client.close();
  await server.close();
});

test("both directions carry requests at once", async () => {
  // The whole point of the symmetric peer: the server calls back into the
  // client while it is answering the client's own request.
  const [a, b] = duplex();
  const client = new Peer(
    new Router().onRequest("ui/ask", () => ({ answer: "blue" })),
  ).connect(a);
  const server = new Peer(
    new Router().onRequest("tool/call", async (_p, peer) => ({
      used: (await peer.request("ui/ask", { q: "colour?" })).answer,
    })),
  ).connect(b);

  assert.deepEqual(await client.request("tool/call", {}), { used: "blue" });
  await client.close();
  await server.close();
});

test("slow handlers do not block other requests", async () => {
  const { client, server } = pair(
    null,
    new Router()
      .onRequest("slow", async () => {
        await sleep(300);
        return "slow";
      })
      .onRequest("fast", () => "fast"),
  );

  const slow = client.request("slow");
  // Issued second, must come back first.
  assert.equal(await client.request("fast"), "fast");
  assert.equal(await slow, "slow");
  await client.close();
  await server.close();
});

test("an unknown method is an error, not a hang", async () => {
  const { client, server } = pair();
  await assert.rejects(client.request("nope"), (e) => e.code === codes.methodNotFound);
  await client.close();
  await server.close();
});

test("a handler error reaches the caller intact", async () => {
  const { client, server } = pair(
    null,
    new Router().onRequest("fail", () => {
      throw new RpcError("upstream is busy", -32050).retryable();
    }),
  );
  await assert.rejects(client.request("fail"), (e) => {
    assert.equal(e.code, -32050);
    assert.equal(e.message, "upstream is busy");
    assert.ok(e.isRetryable, "the retryable hint must survive the wire");
    return true;
  });
  await client.close();
  await server.close();
});

test("a handler bug is reported rather than fatal", async () => {
  const { client, server } = pair(
    null,
    new Router().onRequest("boom", () => {
      throw new Error("kaboom");
    }),
  );
  await assert.rejects(client.request("boom"), /handler raised/);
  await client.close();
  await server.close();
});

test("closing the connection fails every pending request at once", async () => {
  const { client, server } = pair(
    null,
    new Router().onRequest("never", () => sleep(30_000)),
  );
  const waiting = client.request("never");
  await sleep(50);
  await server.close();

  // Without the drain this would hang until the request's own timeout, which
  // is the bug the drain exists to prevent.
  await assert.rejects(waiting, (e) => e.code === codes.transportClosed);
  await client.close();
});

test("a request past its timeout fails locally", async () => {
  const { client, server } = pair(
    null,
    new Router().onRequest("slow", () => sleep(30_000)),
    { requestTimeoutMs: 100 },
  );
  await assert.rejects(client.request("slow"), (e) => e.code === codes.requestTimeout);
  await client.close();
  await server.close();
});

test("abandoning a request cancels it on the peer", async () => {
  const cancels = [];
  const { client, server } = pair(
    null,
    new Router()
      .onRequest("slow", () => sleep(30_000))
      .onNotification("$/cancel", (p) => cancels.push(p)),
    { requestTimeoutMs: 100, cancelNotification: "$/cancel" },
  );

  await assert.rejects(client.request("slow"));
  for (let i = 0; i < 100 && cancels.length === 0; i += 1) await sleep(10);
  assert.equal(cancels.length, 1, "a caller that gave up must tell the peer to stop working");
  await client.close();
  await server.close();
});

test("the handshake records version and capabilities", async () => {
  const [a, b] = duplex();
  const server = new Peer(new Router(), {
    serveHandshake: {
      name: "test-server",
      protocolVersion: "1.2",
      capabilities: ["streaming", "tools"],
    },
  }).connect(b);
  const client = new Peer().connect(a);

  const theirs = await client.handshake({ name: "test-client", protocolVersion: "1.0" });
  assert.equal(theirs.name, "test-server");
  assert.equal(theirs.protocolVersion, "1.2");
  assert.ok(client.supports("tools"));
  assert.ok(!client.supports("ui_ask"));
  // The responding side learns about the caller too, which is what a reverse
  // request needs to know.
  assert.equal(server.peerHello?.name, "test-client");

  await client.close();
  await server.close();
});

test("an incompatible major is refused", async () => {
  const [a, b] = duplex();
  const server = new Peer(new Router(), {
    serveHandshake: { name: "future", protocolVersion: "2.0" },
  }).connect(b);
  const client = new Peer().connect(a);

  await assert.rejects(
    client.handshake({ name: "client", protocolVersion: "1.0" }),
    (e) => e.code === codes.versionIncompatible,
  );
  // Nothing was recorded, so a stub cannot be fooled into thinking the peer
  // supports something on a connection that was refused.
  assert.ok(!client.supports("anything"));

  await client.close();
  await server.close();
});

test("notifications flow without a response", async () => {
  const seen = [];
  const { client, server } = pair(null, new Router().onNotification("tick", (p) => seen.push(p)));
  for (let i = 0; i < 3; i += 1) client.notify("tick", {});
  for (let i = 0; i < 100 && seen.length < 3; i += 1) await sleep(10);
  assert.equal(seen.length, 3);
  await client.close();
  await server.close();
});

test("ids are per direction", async () => {
  // Both sides number from 1 independently. If responses were keyed by id alone
  // across directions, these would collide.
  const [a, b] = duplex();
  const client = new Peer(new Router().onRequest("from_b", () => "a answered")).connect(a);
  const server = new Peer(
    new Router().onRequest("from_a", async (_p, peer) => ({
      nested: await peer.request("from_b"),
    })),
  ).connect(b);

  assert.deepEqual(await client.request("from_a"), { nested: "a answered" });
  await client.close();
  await server.close();
});

test("junk lines are skipped rather than fatal", async () => {
  const [a, b] = duplex();
  const server = new Peer(new Router().onRequest("echo", (p) => p)).connect(b);
  const client = new Peer().connect(a);

  a.send("not json at all");
  assert.deepEqual(await client.request("echo", { ok: true }), { ok: true });
  for (let i = 0; i < 100 && server.skippedLines === 0; i += 1) await sleep(10);
  assert.equal(server.skippedLines, 1);

  await client.close();
  await server.close();
});
