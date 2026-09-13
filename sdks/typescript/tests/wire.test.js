// The framing rules, which must match the Rust core exactly.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  RpcError,
  accepts,
  classify,
  codes,
  errorResponse,
  notification,
  parseVersion,
  request,
  result,
} from "../dist/index.js";

test("classifies by field, not by direction", () => {
  assert.equal(classify('{"id":1,"method":"a"}').kind, "request");
  assert.equal(classify('{"method":"a"}').kind, "notification");
  assert.equal(classify('{"id":1,"result":7}').kind, "response");
  assert.equal(classify('{"id":1,"error":{"code":-1,"message":"x"}}').kind, "response");
});

test("a null id reads as a notification", () => {
  // JSON-RPC uses a null id for "could not determine the id"; routing a
  // response to it would route to nothing.
  assert.equal(classify('{"method":"a","id":null}').kind, "notification");
});

test("the inbound jsonrpc field is optional", () => {
  const parsed = classify('{"id":4,"method":"run"}');
  assert.equal(parsed.kind, "request");
  assert.equal(parsed.method, "run");
});

test("every outbound message carries jsonrpc 2.0", () => {
  for (const line of [
    request(1, "a", {}),
    notification("a"),
    result(1, 7),
    errorResponse(1, new RpcError("x")),
  ]) {
    assert.equal(JSON.parse(line).jsonrpc, "2.0");
  }
});

test("a successful null result stays classifiable", () => {
  const line = result(1, null);
  assert.equal(JSON.parse(line).result, null);
  const parsed = classify(line);
  assert.equal(parsed.kind, "response");
  assert.equal(parsed.error, undefined);
});

test("string ids round trip", () => {
  const parsed = classify('{"id":"abc","method":"a"}');
  assert.equal(parsed.id, "abc");
});

test("a malformed error object still reads as failure", () => {
  const parsed = classify('{"id":1,"error":"just a string"}');
  assert.equal(parsed.kind, "response");
  assert.ok(parsed.error);
});

test("unclassifiable lines are reported, not thrown", () => {
  for (const line of ["{}", "[]", "nope", '{"id":1,"method":5}']) {
    assert.equal(classify(line).kind, "malformed");
  }
});

test("retryable rides in data and stays conformant", () => {
  const error = new RpcError("rate limited").retryable();
  assert.ok(error.isRetryable);
  assert.deepEqual(Object.keys(error.toWire()).sort(), ["code", "data", "message"]);
  assert.ok(classify(errorResponse(1, error)).error.isRetryable);
});

test("a bare error defaults to internal", () => {
  assert.equal(classify('{"id":1,"error":{"message":"boom"}}').error.code, codes.internalError);
});

test("rejects a malformed version", () => {
  for (const bad of ["1", "1.2.3", "", "x.y"]) {
    assert.throws(() => parseVersion(bad));
  }
});

test("negotiation matches the Rust contract", () => {
  const current = parseVersion("1.5");
  const min = parseVersion("1.2");
  assert.ok(accepts(current, min, parseVersion("1.2")));
  // A newer minor is fine: additions are ignorable by contract.
  assert.ok(accepts(current, min, parseVersion("1.9")));
  assert.ok(!accepts(current, min, parseVersion("1.1")));
  assert.ok(!accepts(current, min, parseVersion("2.0")));
});
