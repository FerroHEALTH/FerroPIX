---
name: support-policy
description: "Only the newest release is supported, with no backports; Cadasto B.V. commits under the Cyber Resilience Act to publish security fixes for at least five years after each release, shipped in the newest release (family policy, FerroSMART #18)"
metadata:
  type: project
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

FerroPIX supports only its newest release. When a new release is published,
every earlier release stops receiving fixes: there are no maintenance
branches, no long-term-support line and no backports. Under the Cyber
Resilience Act, Cadasto B.V. commits to keep publishing security fixes for at
least five years after each release (Regulation (EU) 2024/2847 Art 13(8)) and
ships them only in the newest release (Art 13(10)).

**Why:** the policy is product-wide in the FerroHEALTH family, as FerroSMART
recorded it (FerroHEALTH/FerroSMART#18).

**How to apply:** `SECURITY.md` states it; never promise a backport or a
supported older line in a document, an issue or a release note.
