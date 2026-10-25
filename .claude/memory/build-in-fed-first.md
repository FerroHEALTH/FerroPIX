---
name: build-in-fed-first
description: "History: the identity protocols were built in FerroFED first, shaped to move here (owner 2026-10-01); the move was v0.0.2 (FerroFED#367), released; the rule is retired, since identity and the IHE profiles are FerroPIX's lane alone (owner 2026-10-25)"
metadata:
  type: project
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

On 2026-10-01 the owner decided that FerroFED builds every identity
capability it needs itself, shaped to move to FerroPIX later: "maybe we
should build anything what we need to have first in the FED and then we can
always move code to PIX right otherwise we are stuck with first making PIX
before making FED". The rule was for FerroPIX alone, because nothing of it
was built (FerroFED's `.claude/memory/build-in-fed-first.md`).

**What was built there to move:** `crates/ihe-iti` (PIXm, PDQm, PMIR, mCSD,
XCPD, IUA, and the `atna` and `balp` audit features), `crates/nl-generic-functions`,
the vendored IHE and Dutch corpora with their `scripts/vendor/*.sh`, and the
harness identity services (the PIX Manager and the PDQm Supplier seed the
server here).

**The move, called on 2026-10-20** (FerroHEALTH/FerroFED#367, the owner's
decisions in its last comment): re-vendor the corpora here with the same
scripts and pins (FerroFED keeps its copies for its citation guard); copy
each crate with its history, `ihe-iti` without `atna` and `balp`;
publish from here above FerroFED's versions; then FerroFED swaps each path
dependency for the published version in one PR per crate. The harness fakes
in FerroFED stay until a pinned FerroPIX image passes its track 2 and #735,
and FerroEHR#3380 wires to FerroPIX last.

**The rule is retired.** The move was milestone v0.0.2, released, one step
per crate with history. Under the family architecture of 2026-10-25
([[family-architecture]]) identity and the IHE profiles are FerroPIX's lane:
FerroFED reaches FerroPIX over its published protocols as a client and
builds no identity capability of its own.

**How to apply:** an identity capability FerroFED or another product needs
is filed and built here, never built elsewhere first to move later.
