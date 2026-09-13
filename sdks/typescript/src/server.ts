/**
 * The serve loop: read a line, answer it, write the answer.
 *
 * Serial, the TypeScript counterpart of Rust's `SimpleServer`, and for the same
 * reason: someone writing a small server to answer three methods should not
 * have to build a concurrency model first.
 *
 * The handshake is answered here rather than by the author, so version and
 * capability reporting cannot drift between implementations of one protocol.
 */

import { createInterface } from "node:readline";
import type { Readable, Writable } from "node:stream";

import { formatVersion, parseVersion, type Version } from "./version.js";
import {
  RpcError,
  classify,
  codes,
  errorResponse,
  notification,
  result,
} from "./wire.js";

export const INITIALIZE = "initialize";
export const INITIALIZED = "initialized";

/** What a handler can do besides returning a value. */
export interface Context {
  /**
   * Emit a notification now, before the request's own response. This is how a
   * serial server streams progress: the caller sees these while the request is
   * still open.
   */
  notify(method: string, params?: unknown): void;
  peerSupports(token: string): boolean;
  readonly peerName: string;
}

export type RequestHandler = (params: unknown, context: Context) => unknown | Promise<unknown>;
export type NotificationHandler = (params: unknown, context: Context) => void | Promise<void>;

export interface ServerOptions {
  capabilities?: string[];
  info?: unknown;
}

export class Server {
  readonly name: string;
  readonly version: Version;
  capabilities: string[];
  readonly info: unknown;
  peerCapabilities: string[] = [];
  peerName = "";
  skippedLines = 0;

  private readonly requests = new Map<string, RequestHandler>();
  private readonly notifications = new Map<string, NotificationHandler>();

  constructor(name: string, version: string, options: ServerOptions = {}) {
    this.name = name;
    this.version = parseVersion(version);
    this.capabilities = [...new Set(options.capabilities ?? [])].sort();
    this.info = options.info;
  }

  capability(token: string): this {
    if (!this.capabilities.includes(token)) {
      this.capabilities = [...this.capabilities, token].sort();
    }
    return this;
  }

  onRequest(method: string, handler: RequestHandler): this {
    this.requests.set(method, handler);
    return this;
  }

  onNotification(method: string, handler: NotificationHandler): this {
    this.notifications.set(method, handler);
    return this;
  }

  get methods(): string[] {
    return [...this.requests.keys()].sort();
  }

  /** Serve until end of input. */
  async serve(input: Readable = process.stdin, output: Writable = process.stdout): Promise<void> {
    const write = (line: string) => {
      output.write(`${line}\n`);
    };
    const context: Context = {
      notify: (method, params) => write(notification(method, params)),
      peerSupports: (token) => this.peerCapabilities.includes(token),
      get peerName() {
        return "";
      },
    };
    // `peerName` has to read through to the server, not capture an empty string
    // at construction time.
    Object.defineProperty(context, "peerName", { get: () => this.peerName });

    const lines = createInterface({ input, crlfDelay: Infinity });

    for await (const line of lines) {
      if (line.trim() === "") continue;
      const message = classify(line);

      switch (message.kind) {
        case "malformed":
          // A peer writing one bad line should not take the connection down.
          this.skippedLines += 1;
          continue;

        // A serial server never issues requests, so a response is unsolicited.
        case "response":
          continue;

        case "notification": {
          if (message.method === INITIALIZED) continue;
          const handler = this.notifications.get(message.method);
          if (handler) await handler(message.params, context);
          continue;
        }

        case "request": {
          try {
            write(result(message.id, await this.answer(message.method, message.params, context)));
          } catch (e) {
            const error =
              e instanceof RpcError ? e : new RpcError(`handler raised: ${String(e)}`);
            write(errorResponse(message.id, error));
          }
          continue;
        }
      }
    }
  }

  private async answer(method: string, params: unknown, context: Context): Promise<unknown> {
    if (method === INITIALIZE) {
      const theirs = (typeof params === "object" && params !== null ? params : {}) as Record<
        string,
        unknown
      >;
      this.peerName = typeof theirs.name === "string" ? theirs.name : "";
      this.peerCapabilities = Array.isArray(theirs.capabilities)
        ? (theirs.capabilities as string[])
        : [];
      const hello: Record<string, unknown> = {
        name: this.name,
        protocol_version: formatVersion(this.version),
        capabilities: this.capabilities,
      };
      if (this.info !== undefined) hello.info = this.info;
      return hello;
    }

    const handler = this.requests.get(method);
    if (!handler) {
      throw new RpcError(`method not found: ${method}`, codes.methodNotFound);
    }
    return handler(params, context);
  }
}
