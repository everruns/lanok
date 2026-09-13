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
//! **All three fit, and two of them did not when this was written.**
//!
//! YEP and ACP declared cleanly from the start: every method has one direction,
//! requests and notifications are distinguishable, and the optional parts gate
//! on capabilities.
//!
//! MCP did not. Three of its methods are *bidirectional*: `ping`,
//! `notifications/cancelled` and `notifications/progress` may be sent by either
//! side, and lanok's model said a method has one direction, declared once. The
//! gap produced [`lanok::Direction::Either`], and all 25 MCP methods now
//! declare.
//!
//! That is the experiment paying for itself. Lanok will never serve MCP, but a
//! protocol of ours could want a symmetric `ping`, and until MCP was written
//! down here it could not have one.
//!
//! What remains unproven is **wire identity**. These declarations use
//! `serde_json::Value` payloads and are never connected to a real MCP or ACP
//! peer, so they show the method surface is expressible and nothing more. See
//! `knowledge/specs/foreign-protocols.md` for what that does and does not
//! license anyone to claim.

pub mod acp;
pub mod mcp;
pub mod yep;
