---
name: build-in-fed-first
description: "The identity protocols were built in FerroFED first, shaped to move here, because FerroPIX had nothing built (owner 2026-10-01); the owner called the move on 2026-10-20 (FerroFED#367) and it is v0.0.2, copied with history, never piecemeal"
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

**How to apply:** the move is milestone v0.0.2 and happens in one step per
crate, with history; nothing moves piecemeal, and nothing moves in v0.0.1.
A separate analysis scopes it; follow its issues.
