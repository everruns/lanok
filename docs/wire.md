# The wire

What goes over the connection, and the compatibility rules your protocol
inherits by building on lanok. The normative version is
[`knowledge/specs/protocol-contract.md`](../knowledge/specs/protocol-contract.md); this is the
readable one.

## One message per line

Newline-delimited JSON over stdio. One object per line, no embedded newlines.

```json
{"jsonrpc":"2.0","id":1,"method":"echo","params":{"text":"hi"}}
{"jsonrpc":"2.0","method":"echo/progress","params":{"step":1,"of":3}}
{"jsonrpc":"2.0","id":1,"result":{"text":"hi"}}
```

## Classified by field, never by direction

| `method` | `id` | it is a |
|----------|------|---------|
| yes      | yes  | request |
| yes      | no   | notification |
| no       | yes  | response |

Nothing in that table names a pipe, which is why a reverse request needs no new
framing. A `null` id counts as absent: JSON-RPC uses it for "could not determine
the id", so it is not a correlation key.

**Each direction owns its own id space.** A server's `id: 1` and a client's
`id: 1` are unrelated requests.

## JSON-RPC 2.0, honestly

`jsonrpc: "2.0"` goes out on every message and is required on none coming in.
So an off-the-shelf JSON-RPC client in any language can drive a lanok protocol,
and a peer that predates the field still reads.

Ids may be numbers or strings inbound; lanok emits numbers.

## Errors

```json
{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"`text` must be a string"}}
```

`code`, `message`, optional `data`, nothing else. A "retryable" hint lives
*inside* `data`, because JSON-RPC enumerates the members of an error object.

| Code | Meaning |
|------|---------|
| -32700 … -32603 | the reserved JSON-RPC set (parse, invalid request, method not found, invalid params, internal) |
| -32800 | request cancelled |
| -32801 | request timed out |
| -32802 | capability unsupported |
| -32803 | version incompatible |
| -32804 | transport closed |

## Handshake

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"name":"my-host","protocol_version":"1.0","capabilities":["ui_ask"]}}
{"jsonrpc":"2.0","id":1,"result":{"name":"echo","protocol_version":"1.0","capabilities":["ui_ask"]}}
{"jsonrpc":"2.0","method":"initialized"}
```

Both sides send the same shape. `initialized` follows the version check, so a
peer that receives it has been told the truth. Protocols cannot redeclare either
method.

## Versions

`MAJOR.MINOR`. Same major can talk; different majors cannot and the handshake
refuses. Minors are additive: a new method, a new optional field, a new
capability token. A build also states the oldest peer it accepts, so dropping an
old minor is deliberate.

A **newer** peer is always accepted. Everything you understand is still there,
and what you do not understand you ignore.

## The rules that make minors safe

Your payload types have to hold up their end:

- never `deny_unknown_fields`
- every added field is `#[serde(default)]`
- never repurpose an existing field (that is a major change even if the type is
  unchanged)
- never remove a capability token

## Capabilities

A bare string advertised in the handshake, and the unit of optionality. A method
that `requires` one is refused **locally** by the generated stub until the peer
says the token is there, so discovering that an old peer cannot stream costs no
round trip.

Gated notifications are dropped rather than refused: no id, nobody to tell.

## Robustness

- A line that does not parse is **skipped, not fatal**. One bad line should not
  take down a healthy connection. Skips are counted, and a non-zero count means
  a server is printing non-protocol output to stdout.
- A malformed error object still reads as a failure. Losing the detail beats
  losing the outcome.
- When a connection ends, every pending request fails at once rather than
  waiting out its individual timeout.
