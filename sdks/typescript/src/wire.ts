/**
 * JSON-RPC 2.0 framing.
 *
 * The TypeScript half of the same contract the Rust core implements, and the
 * same three rules: a message is classified by its fields rather than by which
 * pipe it arrived on, `jsonrpc: "2.0"` is written on everything outbound and
 * required on nothing inbound, and a line that does not parse is skipped rather
 * than fatal.
 */

export const JSONRPC_VERSION = "2.0" as const;

/** Reserved JSON-RPC codes, then lanok's, chosen outside the reserved range. */
export const codes = {
  parseError: -32700,
  invalidRequest: -32600,
  methodNotFound: -32601,
  invalidParams: -32602,
  internalError: -32603,
  requestCancelled: -32800,
  requestTimeout: -32801,
  capabilityUnsupported: -32802,
  versionIncompatible: -32803,
  transportClosed: -32804,
} as const;

export type Id = number | string;

export interface WireError {
  code: number;
  message: string;
  retryable?: boolean;
  data?: unknown;
}

/**
 * An error that can be returned to the peer. Throw it from a handler; the serve
 * loop turns it into an error response.
 */
export class RpcError extends Error {
  readonly code: number;
  data: unknown;
  isRetryable: boolean;

  constructor(
    message: string,
    code: number = codes.internalError,
    data?: unknown,
    retryable = false,
  ) {
    super(message);
    this.name = "RpcError";
    this.code = code;
    this.data = data;
    this.isRetryable = retryable;
  }

  /**
   * Hint that the failure is worth retrying: a rate limit, an overloaded
   * upstream, anything where the same call may succeed later.
   */
  retryable(): this {
    this.isRetryable = true;
    return this;
  }

  toWire(): WireError {
    const wire: WireError = { code: this.code, message: this.message };
    // Omitted when false, so an error that never sets it looks exactly as it
    // did before the field existed.
    if (this.isRetryable) wire.retryable = true;
    if (this.data !== undefined) wire.data = this.data;
    return wire;
  }
}

export type Message =
  | { kind: "request"; id: Id; method: string; params?: unknown }
  | { kind: "notification"; method: string; params?: unknown }
  | { kind: "response"; id: Id; result?: unknown; error?: RpcError }
  | { kind: "malformed"; line: string; reason: string };

/** Classify one line by its fields. Never by direction. */
export function classify(line: string): Message {
  let value: unknown;
  try {
    value = JSON.parse(line);
  } catch (e) {
    return { kind: "malformed", line, reason: `not valid JSON: ${String(e)}` };
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return { kind: "malformed", line, reason: "not a JSON object" };
  }

  const object = value as Record<string, unknown>;
  const method = object.method;
  // A null id means "could not determine the id" in JSON-RPC, so it is not a
  // correlation key: treat it as absent.
  const id = object.id === null ? undefined : (object.id as Id | undefined);

  if (method !== undefined) {
    if (typeof method !== "string") {
      return { kind: "malformed", line, reason: "`method` is not a string" };
    }
    return id === undefined
      ? { kind: "notification", method, params: object.params }
      : { kind: "request", id, method, params: object.params };
  }

  if (id !== undefined) {
    if ("error" in object) {
      const raw = object.error;
      if (typeof raw === "object" && raw !== null) {
        const e = raw as Record<string, unknown>;
        return {
          kind: "response",
          id,
          error: new RpcError(
            String(e.message ?? ""),
            typeof e.code === "number" ? e.code : codes.internalError,
            e.data,
            e.retryable === true,
          ),
        };
      }
      // A malformed error object still means failure; losing the outcome to a
      // parse error would be worse than losing the detail.
      return { kind: "response", id, error: new RpcError("peer sent a malformed error object") };
    }
    return { kind: "response", id, result: object.result };
  }

  return { kind: "malformed", line, reason: "neither a method nor an id" };
}

export function request(id: Id, method: string, params?: unknown): string {
  const message: Record<string, unknown> = { jsonrpc: JSONRPC_VERSION, id, method };
  if (params !== undefined) message.params = params;
  return JSON.stringify(message);
}

export function notification(method: string, params?: unknown): string {
  const message: Record<string, unknown> = { jsonrpc: JSONRPC_VERSION, method };
  if (params !== undefined) message.params = params;
  return JSON.stringify(message);
}

export function result(id: Id, value: unknown): string {
  // `result` is written even when null: JSON-RPC requires exactly one of
  // result/error, so omitting it makes a successful empty response
  // unclassifiable.
  return JSON.stringify({ jsonrpc: JSONRPC_VERSION, id, result: value ?? null });
}

export function errorResponse(id: Id, error: RpcError): string {
  return JSON.stringify({ jsonrpc: JSONRPC_VERSION, id, error: error.toWire() });
}
