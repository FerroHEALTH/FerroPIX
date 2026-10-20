---
name: product-scope
description: "What FerroPIX is (owner, 2026-10-20): the family's Master Patient Index over IHE PIX, PIXm, PDQ and PDQm, owning ihe-iti and nl-generic-functions, serving FHIR and HL7 v2 together; v0.0.1 setup, v0.0.2 the move, v0.0.3 the server"
metadata:
  type: project
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

The owner's statement of 2026-10-20: FerroPIX is the FerroHEALTH family's
Master Patient Index over IHE PIX, PIXm, PDQ and PDQm. It holds the patient
identity cross-reference and mints the pseudonymous identifiers the clinical
domain uses. FerroEHR resolves `ehr_id` through it (FerroHEALTH/FerroEHR#3380:
PIXm `$ihe-pix` ITI-83 and PDQm ITI-78), and FerroFED's identity binding
points at it.

**The scope the owner settled on 2026-10-20** (FerroHEALTH/FerroFED#367):

- FerroPIX owns and publishes `ihe-iti` (the identity profiles PIXm, PDQm,
  PMIR, mCSD, XCPD and IUA, a feature each, with the server sides added as
  features) and `nl-generic-functions` (BSN and the BSNk pseudonyms, NVI
  localization, and Mitz and Nuts, which travel with the crate). FerroFED
  pins both by exact version.
- The ATNA and BALP audit senders are not FerroPIX's: they go to the shared
  FerroHEALTH/FerroAUDIT repository as the crate `ihe-atna`, set up by
  another worker. FerroPIX audits the Manager side through it.
- `oauth-server-metadata` is retired and never published; the metadata
  module of `ferrohealth-smart` replaces it.
- FerroPIX serves FHIR and HL7 v2 together, on FerroBRIDGE's v2 parsing
  (`hl7v2-types` and its MLLP codec and parser), never a second parser.

**The milestones:**

- **v0.0.1:** the repository setup. No code moves.
- **v0.0.2:** the move. `ihe-iti` without the `atna` and `balp`
  features, and `nl-generic-functions`, each copied with its history and
  its corpora re-vendored by FerroFED's scripts at FerroFED's pins, published
  above FerroFED's versions (`ihe-iti` 0.0.39 or later,
  `nl-generic-functions` 0.0.25 or later).
- **v0.0.3:** the server. The PIXm Manager (ITI-83 by GET and POST with
  several `targetSystem`, and ITI-104), the PDQm Supplier (ITI-78,
  ITI-119), the PMIR Registry (ITI-93, ITI-94), the persisted
  cross-reference (identifier domains, one Source per domain, linking and
  merge), pseudonym minting (a UUID per pseudonymisation domain, for
  FerroEHR's `subject_namespaces`), and the HL7 v2 PIX and PDQ transactions
  (ITI-8, 9, 21 and 22, with 44, 45, 46 and 47 as the ITI TF-2 texts decide,
  after those texts are vendored), Manager-side BALP auditing through
  `ihe-atna`, and a published container image pinned by digest.

**How to apply:** make no technical claim beyond this statement, the
milestone descriptions or a cited finding. Release milestones step by 0.0.x,
never a v0.1.0 opener, as in every family repository.
