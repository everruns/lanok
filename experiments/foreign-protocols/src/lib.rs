//! Can `lanok::protocol!` express protocols it was not designed for?
//!
//! Lanok was extracted from two protocols, mira's and yolop's. Two is thin
//! evidence for an abstraction, and the usual failure of a kit like this is
//! that it encodes its authors' two examples as if they were the shape of the
//! world. This crate is the cheapest available falsification test: declare
//! three real protocols, from their real method surfaces, and see what does not
//! fit.
//!
//! * [`yep`], the yolop extension protocol. A consumer, so it should fit.
//! * [`mcp`], the Model Context Protocol, from its published JSON Schema.
//!   Not a consumer, and never will be: lanok does not implement MCP, `rmcp`
//!   does. It is here because it is a real, independently designed protocol.
//! * [`acp`], the Agent Client Protocol, from the `agent-client-protocol-schema`
//!   crate. Same reasoning.
//!
//! Payload types are `serde_json::Value` throughout. The experiment is about
//! whether the **method surface** is expressible: names, directions, request
//! versus notification, and capability gating. Payload typing is a separate
//! question that each protocol already answers for itself.
//!
//! # Findings
//!
//! **Two of three fit exactly.** YEP and ACP declare cleanly: every method has
//! one direction, requests and notifications are distinguishable, and the
//! optional parts gate on capabilities.
//!
//! **MCP does not, and the reason is structural.** Three of its methods are
//! *bidirectional*: `ping`, `notifications/cancelled`, and
//! `notifications/progress` may be sent by either side. Lanok's model says a
//! method has one direction, declared once. That is not a missing feature, it
//! is a different model, and the workaround (declaring each twice under
//! different Rust names) is ugly enough to be evidence rather than a fix. See
//! [`mcp`] for the detail.
//!
//! This is worth knowing even though lanok will never serve MCP, because a
//! future protocol of ours could want a symmetric `ping`, and today it could
//! not have one.

pub mod acp;
pub mod mcp;
pub mod yep;
