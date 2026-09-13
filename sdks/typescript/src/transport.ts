/**
 * Transports: how a message gets from one peer to the other.
 *
 * Frame-oriented like the Rust trait, not byte-oriented: a transport yields one
 * whole line at a time and takes one whole line back, so framing is the
 * adapter's problem and the peer never sees a partial read.
 */

import { spawn, type ChildProcess } from "node:child_process";
import { createInterface } from "node:readline";
import { PassThrough, type Readable, type Writable } from "node:stream";

export interface Transport {
  /** Every line the peer sends, until end of stream. */
  lines(): AsyncIterable<string>;
  /** Write one line. Safe to call while `lines()` is being consumed. */
  send(line: string): void;
  /** Release the underlying resource. Idempotent. */
  close(): Promise<void> | void;
}

/** Newline-delimited JSON over an arbitrary reader and writer. */
export class StreamTransport implements Transport {
  private closed = false;

  constructor(
    private readonly reader: Readable,
    private readonly writer: Writable,
  ) {}

  async *lines(): AsyncIterable<string> {
    const reader = createInterface({ input: this.reader, crlfDelay: Infinity });
    for await (const line of reader) yield line;
  }

  send(line: string): void {
    if (this.closed) return;
    // Written with the newline in one call, so two concurrent senders cannot
    // interleave a message and its terminator.
    this.writer.write(`${line}\n`);
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.writer.end();
  }
}

/**
 * This process's own stdin and stdout.
 *
 * Keep stdout clean: only protocol JSON belongs there, logging on stderr.
 */
export function stdio(): StreamTransport {
  return new StreamTransport(process.stdin, process.stdout);
}

export interface ChildOptions {
  /** Called once per stderr line. */
  onStderr?: (line: string) => void;
  /** How long the child gets to exit on its own before being killed. */
  exitGraceMs?: number;
}

/**
 * A spawned child process speaking ndjson over its stdin and stdout.
 *
 * Its stderr is drained from the moment it starts, sink or no sink: an unread
 * pipe blocks a chatty server forever once the buffer fills, and the symptom is
 * a hang rather than an error.
 */
export class ChildTransport implements Transport {
  private readonly child: ChildProcess;
  private readonly inner: StreamTransport;
  private exited = false;

  constructor(
    command: string[],
    private readonly options: ChildOptions = {},
  ) {
    const [program, ...args] = command;
    if (program === undefined) throw new Error("no command given");
    this.child = spawn(program, args, { stdio: ["pipe", "pipe", "pipe"] });
    this.child.once("exit", () => {
      this.exited = true;
    });

    if (!this.child.stdout || !this.child.stdin || !this.child.stderr) {
      throw new Error("child process is missing a standard stream");
    }
    this.inner = new StreamTransport(this.child.stdout, this.child.stdin);

    const stderr = createInterface({ input: this.child.stderr, crlfDelay: Infinity });
    void (async () => {
      // Drained unconditionally: discarding still requires reading.
      for await (const line of stderr) options.onStderr?.(line);
    })();
  }

  get pid(): number | undefined {
    return this.child.pid;
  }

  lines(): AsyncIterable<string> {
    return this.inner.lines();
  }

  send(line: string): void {
    this.inner.send(line);
  }

  /**
   * Shut stdin, let the child exit on its own, kill only if it overstays.
   *
   * Killing first discards whatever stderr was still in the pipe, which is
   * exactly the output someone debugging a server that died on startup needs.
   */
  async close(): Promise<void> {
    this.inner.close();
    if (this.exited) return;

    const grace = this.options.exitGraceMs ?? 2000;
    const exited = new Promise<void>((resolve) => this.child.once("exit", () => resolve()));
    const timedOut = await Promise.race([
      exited.then(() => false),
      new Promise<boolean>((resolve) => setTimeout(() => resolve(true), grace)),
    ]);
    if (timedOut) {
      this.child.kill();
      await exited;
    }
  }
}

/**
 * An in-memory transport pair, for driving both sides in one test.
 *
 * The two halves are crossed: what one sends, the other receives.
 */
export function duplex(): [Transport, Transport] {
  const aToB = new PassThrough();
  const bToA = new PassThrough();
  return [new StreamTransport(bToA, aToB), new StreamTransport(aToB, bToA)];
}
