---
name: corpora-move-with-crates
description: "Owner 2026-10-20: every corpus ihe-iti and nl-generic-functions read moves to FerroPIX with its vendor script and PROVENANCE at FerroFED's pin; the ITI TF-2 v2 texts are vendored new; each FerroFED swap PR removes the copies nothing there reads, listing kept and removed corpora with their readers"
metadata:
  type: feedback
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

The owner's direction for the move (v0.0.2), 2026-10-20: the vendored
corpora (documents, laws, specifications) go where their readers go.

**How to apply:**

- Every corpus `ihe-iti` and `nl-generic-functions` read comes to FerroPIX
  with its committed `scripts/vendor/*.sh` and its `PROVENANCE.md`, at
  FerroFED's pin: `ihe-pixm`, `ihe-pdqm`, `ihe-pmir`, `ihe-mcsd`, `ihe-iua`,
  `ihe-iti-tf` and their page trees, the Swiss pins, `nl-gf`, `mitz`,
  `nuts-rfc`, and whatever else their tests read. Each is a row of
  `docs/VERSIONS.md`.
- The ITI TF-2 HL7 v2 texts are vendored new, before the v2 transactions
  (v0.0.3).
- In the FerroFED swap pull request that follows each move, remove every
  corpus copy nothing left in FerroFED reads. FerroFED keeps only what its
  identity adapters and its citation guard (`scripts/checks/ihe-citations.sh`)
  still read, at the same pin as FerroPIX's. The pull request body lists the
  kept and removed corpora with their readers, and FerroFED's guards passing
  prove each removal.

The rule is in `.claude/rules/vendored-inputs.md`. See [[build-in-fed-first]].
