/**
 * Lanok: build JSON-RPC 2.0 protocols in TypeScript.
 *
 * The framing and the serve loop live here, written once. A protocol's own wire
 * types are generated from its committed artifacts with `lanok gen typescript`,
 * so nothing in this package is protocol-specific and no protocol
 * re-implements a JSON-RPC loop.
 *
 * ```ts
 * import { Server } from "@lanok/rpc";
 *
 * await new Server("echo", "1.0")
 *   .onRequest("echo", (params) => ({ text: String((params as any).text).toUpperCase() }))
 *   .serve();
 * ```
 *
 * Keep stdout clean: only protocol JSON belongs there, and logging belongs on
 * stderr.
 */

export { INITIALIZE, INITIALIZED, Server } from "./server.js";
export type { Context, NotificationHandler, RequestHandler, ServerOptions } from "./server.js";
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
