/**
 * Lanok: build JSON-RPC 2.0 protocols in TypeScript.
 *
 * The framing, the serve loop, and the symmetric peer live here, written once.
 * A protocol's own wire types are generated from its committed artifacts with
 * `lanok gen typescript`, so nothing in this package is protocol-specific and
 * no protocol re-implements a JSON-RPC loop.
 *
 * Two shapes, matching the Rust kit. `Server` is serial: it answers one request
 * at a time and cannot send one, which is all a small tool server needs.
 *
 * ```ts
 * import { Server } from "@lanok/rpc";
 *
 * await new Server("echo", "1.0")
 *   .onRequest("echo", (params) => ({ text: String((params as any).text).toUpperCase() }))
 *   .serve();
 * ```
 *
 * `Peer` is the symmetric one. It issues requests and answers them at the same
 * time, so it can be either end of a connection, including the reverse
 * direction.
 *
 * ```ts
 * import { Router, connectChild } from "@lanok/rpc";
 *
 * const peer = connectChild(["./my-server"], new Router().onRequest("ui/ask", answer));
 * await peer.handshake({ name: "my-host", protocolVersion: "1.0", capabilities: ["ui_ask"] });
 * await peer.request("echo", { text: "hi" });
 * ```
 *
 * Keep stdout clean: only protocol JSON belongs there, and logging belongs on
 * stderr.
 */

export { INITIALIZE, INITIALIZED, Server } from "./server.js";
export type { Context, NotificationHandler, RequestHandler, ServerOptions } from "./server.js";
export { Peer, Router, connectChild } from "./peer.js";
export type { Hello, PeerNotificationHandler, PeerOptions, PeerRequestHandler } from "./peer.js";
export { ChildTransport, StreamTransport, duplex, stdio } from "./transport.js";
export type { ChildOptions, Transport } from "./transport.js";
export { accepts, formatVersion, parseVersion } from "./version.js";
export type { Version } from "./version.js";
export {
  JSONRPC_VERSION,
  RpcError,
  classify,
  codes,
  errorResponse,
  notification,
  request,
  result,
} from "./wire.js";
export type { Id, Message, WireError } from "./wire.js";
