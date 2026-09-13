---
type: Policy
title: Documentation Specification
description: Defines where lanok's documentation lives, which surface owns which fact, and the committed-SVG diagram convention.
---

# Documentation Specification

## Purpose

Lanok has five documentation surfaces and they drift unless each owns something
the others do not. This specification says who owns what, and fixes the diagram
convention so a second author does not invent a second visual language.

Conventions are shared with the sibling [everruns](https://github.com/everruns)
repositories, most notably that **diagrams are committed SVGs**, not raster
images.

## Where documentation lives

| Surface | Audience | Source of truth for | Format |
|---------|----------|---------------------|--------|
| `README.md` | first-time visitor | the pitch and a 60-second start | Markdown |
| `docs/` | users | guides and the readable wire reference | Markdown |
| rustdoc | API consumers | exact types, traits, signatures | `///` comments |
| `skills/lanok/` | coding agents | agent-facing overview and entry points | Markdown |
| `knowledge/` | maintainers | the design of record, the *why* | OKF Markdown |
| `CHANGELOG.md` | upgraders | what changed, per release | Keep a Changelog |

One fact has **one home**. Guides link to the reference rather than restating
it; the README links into `docs/` rather than duplicating a guide; knowledge
describes a design once and `docs/` describes its use. When a fact mirrors code
(a protocol version, an error code, a method name), the doc cites the code and
is updated in the same change: code is authoritative, docs track it.

`README.md` and `docs/` must not link into `knowledge/`. The bundle is internal
memory, and a public page that depends on it has leaked.

## The normative wire reference

The wire has two descriptions and they are not duplicates.
[`knowledge/specs/protocol-contract.md`](protocol-contract.md) is normative: it
binds every protocol built on lanok. [`docs/wire.md`](../../docs/wire.md) is the
readable one, and says so in its first paragraph. A rule added to one is added
to the other in the same change, or the readable one stops being true.

## Diagrams

Boxes-and-arrows diagrams are **hand-authored SVG, committed under
`docs/assets/`**.

- **SVG, not raster.** It diffs, scales, and stays crisp. Raster is only for a
  genuine screenshot, never for boxes and arrows.
- **Self-contained.** Inline styling, no external fonts, scripts, or image
  references. Use the system font stack
  (`-apple-system, Segoe UI, Helvetica, Arial, sans-serif`) so it renders the
  same everywhere, GitHub included.
- **Responsive.** Set a `viewBox`, not a fixed root `width`/`height`, and size
  at the embed site (`<img … width="720">`).
- **Legible on both themes.** GitHub renders the same file on a white and a
  near-black page, and a transparent SVG inherits whichever it gets. Every text
  element therefore sits on a fill this file draws: inside a box, or on an
  opaque band. Free-floating text in a mid-slate is unreadable on one of the
  two, which is how a diagram silently becomes useless for half its readers.
- **Restrained palette, reused.** Initiator blue (`#3b82f6` stroke, `#eff6ff`
  fill), responder green (`#22c55e`, `#f0fdf4`), forward messages blue
  (`#2563eb`), reverse messages amber (`#b45309`), structure and captions slate.
  A new diagram extends this palette rather than inventing one. Reverse traffic
  is always the amber one: it is the thing worth seeing at a glance.
- **Accessible.** Every embed carries `alt` text stating the relationship the
  diagram shows, not just its title.
- **Named by subject.** `docs/assets/<topic>.svg`, embedded from the README by
  absolute `raw.githubusercontent.com` URL (so it renders on crates.io and in
  package registries) and from `docs/` by relative path.
