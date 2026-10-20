---
name: ihe-iti-audit-feature
description: "Each IHE profile's own audit record (ITI-83/78/119/90/91/93/94 BALP AuditEvents, the ITI-55 DICOM message) stays in ihe-iti behind the `audit` feature on ihe-atna; the audit trail and the user model are ihe-atna's (owner 2026-10-20)"
metadata:
  type: project
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

When `ihe-iti` moved from FerroFED, its `atna` and `balp` features went to
FerroHEALTH/FerroAUDIT as the crate `ihe-atna`. What each transaction must
audit is defined by that profile's own specification, so the per-profile
audit writers stay with the profile in `ihe-iti`, behind the `audit` feature
(`ihe-atna/balp`, which carries `atna` for the ITI-55 message), and every
client keeps its `.audited(recorder)`.

**Why:** the owner, 2026-10-20: the generic core lives in `ihe-atna`, the
profile's audit record with the profile, and no audit path is dropped.

**How to apply:** `ihe-iti` pins `ihe-atna` exactly. The user an exchange is
made for is `ihe_atna::user`, so the exchange and its audit record share one
type; `ihe-iti` has no user model of its own. Every feature, and each profile
with `audit`, has its own CI lint and test row.
