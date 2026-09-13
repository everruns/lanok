---
type: Experiment Report
title: Foreign Protocol Fit
description: Whether lanok's model describes protocols it was not designed for, tested against YEP, ACP, and MCP.
---

# Foreign Protocol Fit

## Why

Lanok was extracted from two protocols. Two is thin evidence for an
abstraction, and the usual failure of a kit like this is encoding its authors'
two examples as if they were the shape of the world. The cheapest available
falsification test is to declare protocols nobody designed with lanok in mind
and see what does not fit.

The experiment lives in `experiments/foreign-protocols/` and runs in CI, so the
findings cannot quietly stop being true.

## Method

Three protocols, declared from their real method surfaces:

| Protocol | Source | Relationship to lanok |
|---|---|---|
| YEP | `yolop/crates/yolop-yep/src/meta.rs` | a consumer; the control |
| ACP | `agent-client-protocol-schema` 1.7.0 | none |
| MCP | published JSON Schema, 2025-06-18 | none, and never will be: `rmcp` implements MCP |

Payloads are `serde_json::Value` throughout. The question is whether the
**method surface** is expressible: names, directions, request versus
notification, capability gating. Payload typing is a separate question each
protocol already answers for itself.

## Findings

### YEP and ACP fit

All 12 YEP methods and 23 of ACP's declare without contortion. Both use the
reverse direction for exactly what lanok's reverse direction is for: asking the
other side something while a request is still open. ACP's
`session/request_permission` is the same shape as yolop's `ui/ask`, in a
protocol with no connection to lanok at all. That is the single strongest piece
of evidence that the symmetric peer describes something real.

### MCP did not, and that produced `Direction::Either`

Three MCP methods are **bidirectional**: `ping`, `notifications/cancelled`, and
`notifications/progress` may be sent by either side.

Lanok's model was that a method has one direction, declared once, with
role-gated stubs so calling one the wrong way does not compile. Declaring such a
method twice under different Rust names is rejected by the validator as a
duplicate wire name, which is the right answer to the wrong question.

So the model grew a third direction. An `either` method:

* puts its stubs on a separate `SharedApi` trait, not on both role traits, so
  importing both roles cannot make a call ambiguous;
* appears on **both** handler traits and is routed by **both** dispatchers,
  because it can arrive from either side;
* counts for both sides in `sent_by` and for neither in `declared_by`, so a
  doctor report lists it once, in its own section.

It gives up the compile-time role gating, which is why it is a deliberate third
option and not the default. All 25 MCP methods now declare.

This is the experiment paying for itself. Lanok will never serve MCP, but a
protocol of ours could want a symmetric `ping`, and until MCP was written down
here it could not have one.

## Wire identity, against the real implementation

Declaring a method surface is a statement about the macro, not about the wire.
The obvious way to find out whether lanok's MCP declaration is *correct* is to
point it at an implementation that has never heard of lanok and let that
implementation judge.

`experiments/foreign-protocols/tests/rmcp_interop.rs` does exactly that.
`rmcp` is the official Rust MCP SDK from the modelcontextprotocol
organisation. It is a dev-dependency of the experiment, it runs in CI, and it
holds both ends of the contract: it owns the payload types and it decides what
is acceptable on the wire.

The two sides share one in-memory pipe, rmcp's transport on one end and
lanok's ndjson transport on the other, with nothing translating between them.
Every payload that crosses is an `rmcp::model` type serialised by rmcp's own
derives, so a byte lanok gets wrong is a byte rmcp rejects.

A full session passes:

| Step | What it proves |
|---|---|
| `initialize` with `InitializeRequestParams` / `InitializeResult` | a protocol's own handshake payloads, through `Peer::handshake_with` |
| `notifications/initialized` | the configurable handshake notification name |
| `ping` | `Direction::Either`, answered by a real MCP implementation |
| `tools/list`, `tools/call` | ordinary forward requests, gated on a capability the server advertised |
| `elicitation/create` | the **reverse channel**: rmcp's server asks, lanok's `InitiatorHandler` answers, inside an outstanding request |
| `notifications/progress` ×2 | an unsolicited inbound `either` notification arriving mid-request |

The tool's answer is built from what lanok replied to the elicitation, so the
assertion at the end (`"Vitayu, Kyiv!"`) can only hold if the reverse request
and its response both crossed intact.

A second test pins the local half: a method whose capability the server did not
advertise is refused with `CAPABILITY_UNSUPPORTED` before anything is written,
rather than making a round trip to be told `method not found`.

### What this still does not license

Lanok is **not** an MCP client and must not become one; `rmcp` is the answer
for anyone who wants one. What the test shows is narrower and is the thing
worth knowing: the peer, the transport, and the declaration are correct enough
that a third-party implementation of a protocol lanok did not design holds a
full session with them, reverse channel included.

Untested still: ACP against `agent-client-protocol`, and anything about MCP's
HTTP transports, which lanok does not have.

Wire identity is separately tested for the protocols lanok actually serves:
mira's adoption keeps its Python and TypeScript study SDKs, which are
independent implementations that know nothing about lanok, and CI drives the
Rust host against both.

## Two smaller gaps, both worked around

* **Nested capabilities.** ACP gates on a nested object
  (`clientCapabilities.fs.readTextFile`); MCP does the same. Lanok's
  `requires "token"` is flat. Flattening at the boundary works and is what the
  experiment does, but a real adoption would want either that flattening to be
  explicit or capability paths to be nested.
* **Per-method feature gating.** ACP's unstable surface is cargo-feature-gated
  per method. Lanok has no equivalent; mira solves the same problem by wrapping
  declarations in its own `protocol-unstable` feature, which works here too.

## What the experiment already changed

Declaring MCP is what showed that the handshake could not be its own shape:
MCP's `initialize` answers with `serverInfo` and a nested capabilities object,
and it completes the handshake with `notifications/initialized`, not
`initialized`. Both were hardcoded. Now [`Peer::handshake_with`] takes the
protocol's own payload types, [`Peer::record_peer`] lights up capability gating
afterwards, the handshake's two method names are configurable, and `protocol!`
no longer refuses to declare them.

ACP's `session/cancel` being a notification, where mira's `cancel` is an
acknowledged request, is the same divergence that turned lanok's cancellation
from a setting into a hook. Two independent protocols disagreeing about the
shape of one concern is the signal that it was never lanok's to decide.
