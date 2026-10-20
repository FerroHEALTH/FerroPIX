---
name: license-busl
description: "FerroPIX is under the Business Source License 1.1 on the FerroHEALTH terms, Cadasto B.V. the Licensor, Apache 2.0 the Change License four years after each version; every first-party file carries the SPDX header"
metadata:
  type: project
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

FerroPIX's own code and text are under the Business Source License 1.1, the
terms every FerroHEALTH product uses. Cadasto B.V. is the Licensor and
handles the business side (info@cadasto.com); the maintainer handles the
technical side. The crates FerroPIX publishes (`ihe-iti`,
`nl-generic-functions`) are hand-written code that implements a
specification, so they are BUSL-1.1 too; only generator-emitted code is
Apache-2.0 in this family.

**How to apply:**

- Every first-party file carries `SPDX-FileCopyrightText: Cadasto B.V.` and
  `SPDX-License-Identifier: BUSL-1.1` in its header;
  `scripts/checks/versions.sh` fails on a missing header or another claim.
- `LICENSE` is the one file naming Apache 2.0 as a licence of its own, as the
  Change License.
- Vendored material keeps its upstream terms, recorded in each
  `PROVENANCE.md`; `deny.toml` admits the dependency licences.
- A contribution is licensed under the same terms (CONTRIBUTING.md §
  Licensing of contributions); the pull request box records it.
