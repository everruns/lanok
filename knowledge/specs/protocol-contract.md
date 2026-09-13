---
type: Policy
title: Protocol Contract Specification
description: Defines the framing, versioning, capability, and forward-compatibility rules every protocol built on lanok inherits.
---

# The protocol contract

Every protocol built on lanok inherits these rules. They are what let two
independently written peers interoperate without coordinating a release.

Lanok itself is pre-1.0 and may break its Rust API. That never licenses
breaking a downstream *wire*: a protocol's compatibility promise runs from its
own 1.0, not from lanok's.

## Framing

One JSON object per message. Over stdio, one per line, newline-delimited, no
embedded newlines.

Classification is by **field**, never by direction:

| `method` | `id` | meaning |
|----------|------|---------|
| yes      | yes  | request |
| yes      | no   | notification |
| no       | yes  | response |

An object with neither is not a message. A `null` id counts as absent: JSON-RPC
uses it for "could not determine the id", so it is not a correlation key.

## JSON-RPC 2.0

`jsonrpc: "2.0"` is written on every outbound message and required on no
inbound one. Emitting it makes the wire literally JSON-RPC 2.0, so off-the-shelf
clients in any language can drive a lanok protocol. Not requiring it keeps peers
that predate the field readable.

Ids may be a number or a string inbound; lanok emits only numbers. **Each
direction owns its own id space**: a responder's `id: 1` and an initiator's
`id: 1` are unrelated, because a response is routed by the pending map of
whichever side sent the request.

## Errors

The error object carries `code`, `message`, and optional `data`, and nothing
else. The `retryable` hint lives inside `data` rather than beside `code`,
because JSON-RPC enumerates the members of an error object and an extra
top-level field is what makes a wire "JSON-RPC shaped" instead of JSON-RPC.

Reserved codes are JSON-RPC's. Lanok's are outside the reserved range so they
cannot collide with a future assignment: `-32800` cancelled, `-32801` timeout,
`-32802` capability unsupported, `-32803` version incompatible, `-32804`
transport closed.

## Versioning

`MAJOR.MINOR`.

- **Major** changes only on a breaking wire change. Different majors cannot
  talk, and the handshake refuses.
- **Minor** increments for backwards-compatible additions: a new method, a new
  optional field, a new capability token.

A build also declares the oldest peer it accepts, so dropping support for an
ancient minor is an explicit act rather than a silent regression. A *newer*
minor is always accepted: by the additive rule, everything this build
understands is still there, and what it does not understand it ignores.

## Forward compatibility

These are obligations on payload types, and they are what make the minor rule
true rather than aspirational:

- **Never `deny_unknown_fields`.** An older peer must ignore what it does not
  know.
- **Every addition is `#[serde(default)]`.** A newer peer must tolerate its
  absence.
- **Never repurpose a field.** Changing the meaning of an existing field is a
  major change even when the type is unchanged.
- **Never remove a capability token.** Removing one is a major change; adding
  one is a minor.

The drift guard checks the artifacts, not the leniency. Reviewers check the
leniency.

## The handshake

Two method names are conventional rather than configurable, because a
convention is what lets a generic tool talk to a protocol it has never seen:

- `initialize`, a request. Both directions send the same shape: name,
  `protocol_version`, `capabilities`, and an open `info` field for whatever
  else the protocol wants.
- `initialized`, a notification the initiator sends once it has accepted the
  reply. It comes after the version check, so a peer that receives it has been
  told the truth.

A protocol may not declare either: `protocol!` rejects it, because shadowing
them would shadow version and capability negotiation.

## Capabilities

A capability is a bare string advertised in the handshake. It is the unit of
optionality: a method that needs one is unavailable until the peer says the
token is there, and a generated stub refuses **locally**, before the wire. That
keeps "this peer is older and cannot stream" a typed local answer rather than a
round trip ending in `method not found`.

A notification gated on a capability is dropped rather than refused, because a
notification carries no id and so has nobody to report a refusal to.

## Reverse requests

A protocol may declare methods sent by the responder. Nothing about the framing
changes: the id space is already per-direction and classification already
ignores direction. A peer that receives a reverse request it does not handle
answers `method not found`, so adding one is safe against older peers that were
built before it existed.

A serial server (`SimpleServer`, or the Python and TypeScript SDK servers)
cannot serve reverse requests, because it cannot wait for a reply while
producing one. Protocols that need them require a concurrent peer on that side.

## Conformance

A protocol's behaviour is written down once as vectors and replayed against
every implementation. Expectations are partial by design: a case asserts what
matters and ignores the rest, so adding an optional field does not invalidate
the suite. That is the same forward-compatibility rule, applied to the tests.
