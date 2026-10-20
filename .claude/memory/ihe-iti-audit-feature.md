---
name: ihe-iti-audit-feature
description: "Each IHE profile's own audit record (ITI-83/78/119/90/91/93/94 BALP AuditEvents, the ITI-55 DICOM message) stays in ihe-iti behind `balp` and `atna` on ihe-atna; the audit trail and the user model are ihe-atna's (owner 2026-10-20)"
metadata:
  type: project
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

When `ihe-iti` moved from FerroFED, its `atna` and `balp` features went to
FerroHEALTH/FerroAUDIT as the crate `ihe-atna`. What each transaction must
audit is defined by that profile's own specification, so the per-profile
audit writers stay with the profile in `ihe-iti`, behind `balp` (the BALP
records, `ihe-atna/balp`) and `atna` (the ITI-55 message of `xcpd`,
`ihe-atna/atna`, with no FHIR model), and every client keeps its
`.audited(recorder)`. The split came in 0.0.40 (owner, 2026-10-20, #32), so
FerroFED kept its feature names; 0.0.39 had one `audit` feature.

**Why:** the owner, 2026-10-20: the generic core lives in `ihe-atna`, the
profile's audit record with the profile, and no audit path is dropped.

**How to apply:** `ihe-iti` pins `ihe-atna` exactly. The user an exchange is
made for is `ihe_atna::user`, so the exchange and its audit record share one
type; `ihe-iti` has no user model of its own. Every feature, and each profile
with its audit feature, has its own CI lint and test row.
