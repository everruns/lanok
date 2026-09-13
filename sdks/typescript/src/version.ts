/** MAJOR.MINOR negotiation, matching the Rust contract exactly. */

export interface Version {
  major: number;
  minor: number;
}

export function parseVersion(text: string): Version {
  const parts = text.split(".");
  const invalid = () => new Error(`\`${text}\` is not a MAJOR.MINOR protocol version`);
  if (parts.length !== 2) throw invalid();
  const major = Number(parts[0]);
  const minor = Number(parts[1]);
  if (!Number.isInteger(major) || !Number.isInteger(minor) || major < 0 || minor < 0) {
    throw invalid();
  }
  return { major, minor };
}

export function formatVersion(version: Version): string {
  return `${version.major}.${version.minor}`;
}

/**
 * Whether a build speaking `current`, supporting back to `minimum`, can talk to
 * a peer speaking `peer`.
 *
 * A newer minor is always accepted: by the additive contract everything this
 * build understands is still there, and what it does not understand it ignores.
 */
export function accepts(current: Version, minimum: Version, peer: Version): boolean {
  if (peer.major !== current.major) return false;
  return peer.major > minimum.major || (peer.major === minimum.major && peer.minor >= minimum.minor);
}
