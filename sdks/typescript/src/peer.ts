/**
 * The symmetric peer.
 *
 * The TypeScript counterpart of Rust's `Peer`, and the same idea: there is no
 * client type and no server type. One peer issues requests and answers them at
 * the same time, over one connection, and which methods flow in which direction
 * is a property of the protocol rather than of this code.
 *
 * Shape: one read loop owns the transport's input, resolving the promise that
 * registered each id and dispatching inbound requests without awaiting them, so
 * a slow handler never blocks the reader.
 */

import type { Transport } from "./transport.js";
import { ChildTransport } from "./transport.js";
import { accepts, formatVersion, parseVersion } from "./version.js";
import {
  RpcError,
  classify,
  codes,
  errorResponse,
  notification,
  request,
  result,
  type Id,
} from "./wire.js";

export const INITIALIZE = "initialize";
export const INITIALIZED = "initialized";

/**
 * Who a peer is and what it supports.
 *
 * `protocolVersion` is the one required field: a peer that will not say which
 * version it speaks cannot be negotiated with, and defaulting it turns that
 * into a mystery failure three methods later.
 */
export interface Hello {
  name: string;
  protocolVersion: string;
  capabilities?: string[];
  info?: unknown;
}

function helloToWire(hello: Hello): Record<string, unknown> {
  const wire: Record<string, unknown> = {
    name: hello.name,
    protocol_version: hello.protocolVersion,
    capabilities: [...(hello.capabilities ?? [])].sort(),
  };
  if (hello.info !== undefined) wire.info = hello.info;
  return wire;
}

function helloFromWire(value: unknown): Hello {
  const object = (typeof value === "object" && value !== null ? value : {}) as Record<
    string,
    unknown
  >;
  const version = object.protocol_version;
  if (typeof version !== "string") {
    throw new RpcError("peer did not state a protocol_version", codes.versionIncompatible);
  }
  return {
    name: typeof object.name === "string" ? object.name : "",
    protocolVersion: version,
    capabilities: Array.isArray(object.capabilities) ? (object.capabilities as string[]) : [],
    info: object.info,
  };
}

export type PeerRequestHandler = (params: unknown, peer: Peer) => unknown | Promise<unknown>;
export type PeerNotificationHandler = (params: unknown, peer: Peer) => void | Promise<void>;

/** A method table, for peers that dispatch by hand. */
export class Router {
  private readonly requests = new Map<string, PeerRequestHandler>();
  private readonly notifications = new Map<string, PeerNotificationHandler>();

  onRequest(method: string, handler: PeerRequestHandler): this {
    this.requests.set(method, handler);
    return this;
  }

  onNotification(method: string, handler: PeerNotificationHandler): this {
    this.notifications.set(method, handler);
    return this;
  }

  async request(peer: Peer, method: string, params: unknown): Promise<unknown> {
    const handler = this.requests.get(method);
    if (!handler) throw new RpcError(`method not found: ${method}`, codes.methodNotFound);
    return handler(params, peer);
  }

  async notification(peer: Peer, method: string, params: unknown): Promise<void> {
    await this.notifications.get(method)?.(params, peer);
  }
}

export interface PeerOptions {
  /** Answer `initialize` with this, rather than passing it to the handler. */
  serveHandshake?: Hello;
  /** Fail a request that has not been answered within this many milliseconds. */
  requestTimeoutMs?: number;
  /** The notification this protocol uses to abandon an in-flight request. */
  cancelNotification?: string;
}

interface Waiter {
  resolve: (value: unknown) => void;
  reject: (error: RpcError) => void;
  timer?: ReturnType<typeof setTimeout>;
}

/** A live connection to another peer. */
export class Peer {
  peerHello: Hello | undefined;
  skippedLines = 0;

  private transport: Transport | undefined;
  private nextId = 1;
  private pending: Map<Id, Waiter> | undefined = new Map();
  private closedResolve: (() => void) | undefined;
  private readonly closedPromise: Promise<void>;
  private closing = false;

  constructor(
    private readonly handler: Router = new Router(),
    private readonly options: PeerOptions = {},
  ) {
    this.closedPromise = new Promise((resolve) => {
      this.closedResolve = resolve;
    });
  }

  /** Start serving over `transport`. Returns this, so it chains. */
  connect(transport: Transport): this {
    this.transport = transport;
    void this.pump(transport);
    return this;
  }

  get isClosed(): boolean {
    return this.closing;
  }

  /** Resolves when the connection ends. A server's main loop. */
  closed(): Promise<void> {
    return this.closedPromise;
  }

  /**
   * End the connection and fail every pending request at once, rather than
   * letting each wait out its own timeout.
   */
  async close(): Promise<void> {
    if (this.closing) return;
    this.closing = true;
    await this.transport?.close();
    const pending = this.pending;
    this.pending = undefined;
    for (const waiter of pending?.values() ?? []) {
      if (waiter.timer) clearTimeout(waiter.timer);
      waiter.reject(new RpcError("the connection closed", codes.transportClosed));
    }
    this.closedResolve?.();
  }

  /** Issue a request and wait for its response. */
  request(method: string, params?: unknown, timeoutMs?: number): Promise<unknown> {
    if (!this.transport || !this.pending) {
      return Promise.reject(new RpcError("peer is not connected", codes.transportClosed));
    }
    const id = this.nextId++;
    const limit = timeoutMs ?? this.options.requestTimeoutMs;

    return new Promise<unknown>((resolve, reject) => {
      const waiter: Waiter = { resolve, reject };
      if (limit !== undefined) {
        waiter.timer = setTimeout(() => {
          // The caller gave up, so free the slot and, when the protocol says
          // how, tell the peer to stop working.
          this.pending?.delete(id);
          if (this.options.cancelNotification) {
            this.notify(this.options.cancelNotification, { id });
          }
          reject(new RpcError(`no response to \`${method}\` within ${limit}ms`, codes.requestTimeout));
        }, limit);
      }
      this.pending?.set(id, waiter);
      this.transport?.send(request(id, method, params));
    });
  }

  /** Send a notification. Fire and forget, by definition. */
  notify(method: string, params?: unknown): void {
    if (!this.closing) this.transport?.send(notification(method, params));
  }

  /**
   * Send `initialize`, check the reply's version, record what the peer can do,
   * then send `initialized`.
   *
   * After this resolves, `supports()` is populated, which is what a reverse
   * request needs to know.
   */
  async handshake(ours: Hello, minimum?: string): Promise<Hello> {
    const theirs = helloFromWire(await this.request(INITIALIZE, helloToWire(ours)));

    const current = parseVersion(ours.protocolVersion);
    const floor = minimum ? parseVersion(minimum) : { major: current.major, minor: 0 };
    if (!accepts(current, floor, parseVersion(theirs.protocolVersion))) {
      throw new RpcError(
        `peer speaks ${theirs.protocolVersion} but this build speaks ` +
          `${ours.protocolVersion} (min ${formatVersion(floor)})`,
        codes.versionIncompatible,
      );
    }

    this.peerHello = theirs;
    // Only after accepting: a peer told the connection is live before the
    // version check has been told something we then hang up on.
    this.notify(INITIALIZED);
    return theirs;
  }

  /** Whether the peer advertised `token` during the handshake. */
  supports(token: string): boolean {
    return this.peerHello?.capabilities?.includes(token) ?? false;
  }

  private async pump(transport: Transport): Promise<void> {
    try {
      for await (const line of transport.lines()) {
        if (line.trim() === "") continue;
        const message = classify(line);

        switch (message.kind) {
          case "malformed":
            // One bad line should not take down a healthy connection.
            this.skippedLines += 1;
            break;
          case "response": {
            const waiter = this.pending?.get(message.id);
            // No waiter means the caller already gave up. Dropping the response
            // is correct: a best-effort cancel races exactly this way.
            if (!waiter) break;
            this.pending?.delete(message.id);
            if (waiter.timer) clearTimeout(waiter.timer);
            if (message.error) waiter.reject(message.error);
            else waiter.resolve(message.result);
            break;
          }
          case "notification":
            if (message.method !== INITIALIZED) {
              // Not awaited: a slow handler must not block the reader.
              void this.handler.notification(this, message.method, message.params).catch(() => {
                // A notification has nobody to report a failure to.
              });
            }
            break;
          case "request":
            void this.answer(message.id, message.method, message.params);
            break;
        }
      }
    } finally {
      await this.close();
    }
  }

  private async answer(id: Id, method: string, params: unknown): Promise<void> {
    try {
      this.transport?.send(result(id, await this.dispatch(method, params)));
    } catch (e) {
      const error = e instanceof RpcError ? e : new RpcError(`handler raised: ${String(e)}`);
      this.transport?.send(errorResponse(id, error));
    }
  }

  private async dispatch(method: string, params: unknown): Promise<unknown> {
    // The peer, not the handler, answers the handshake when configured to.
    // Capability state lives here, so answering here is what makes supports()
    // true on the responding side.
    if (method === INITIALIZE && this.options.serveHandshake) {
      try {
        this.peerHello = helloFromWire(params);
      } catch {
        // A peer that will not state its version still gets our reply; the
        // version check is the initiator's job.
      }
      return helloToWire(this.options.serveHandshake);
    }
    return this.handler.request(this, method, params);
  }
}

/** Spawn a server and connect a peer to it, in one call. */
export function connectChild(
  command: string[],
  handler?: Router,
  options: PeerOptions & { onStderr?: (line: string) => void } = {},
): Peer {
  const { onStderr, ...peerOptions } = options;
  return new Peer(handler, peerOptions).connect(new ChildTransport(command, { onStderr }));
}
