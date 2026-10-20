<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Vendored inputs: fetched by a committed script, verbatim, with provenance

FerroPIX vendors the specifications it implements under `docs/specs/`: the
IHE PIXm, PDQm, PMIR and the other identity profiles' FHIR packages, the
Dutch Generic Functions source, and the ITI Technical Framework texts the
HL7 v2 transactions need, each tree with a `PROVENANCE.md`. The identity
corpora arrive with the move from FerroFED, fetched by FerroFED's scripts at
FerroFED's pins.

## Rules

- **Every vendored file is fetched by a committed script** under
  `scripts/vendor/` from the pin `docs/VERSIONS.md` records (a commit, an
  immutable publication or a checksum, never a moving tag or a `latest`
  URL), never copied by hand.
- **Vendored files are verbatim**: never edited. A hand-edited file, a moved
  pin or a stale provenance fails the pull request
  (`scripts/checks/versions.sh` holds every tree to a `PROVENANCE.md` and a
  row of the pin matrix).
- **Every vendored tree carries a `PROVENANCE.md`**: the source, the pinned
  version, the date fetched, the licence, and how to refresh it. Vendored
  material keeps its upstream terms (CC BY 4.0 for the IHE FHIR packages,
  IHE International's terms for the Technical Framework text, HL7's terms
  for the FHIR specification).
- **A text whose licence forbids redistribution is cited, never vendored**,
  and the citation names the edition.
- **A new corpus** is a row in `docs/VERSIONS.md`, a fetch by the script and
  the citation that needs it, in one change.
- **Bumping a pin** is a deliberate change: rerun the script, read what
  changed against the citations in the code, and record the new version in
  `PROVENANCE.md` and `docs/VERSIONS.md`.
## Moving the corpora with the crates (owner, 2026-10-20)

- **Every corpus `ihe-iti` and `nl-generic-functions` read moves here with
  its committed vendor script and its `PROVENANCE.md`**, at FerroFED's pin:
  `ihe-pixm`, `ihe-pdqm`, `ihe-pmir`, `ihe-mcsd`, `ihe-iua`, `ihe-iti-tf`
  and their page trees, the Swiss pins, `nl-gf`, `mitz`, `nuts-rfc`, and
  whatever else their tests read. The ITI TF-2 HL7 v2 texts are vendored
  new, by a new script.
- **The FerroFED swap pull request that follows each move removes every
  corpus copy nothing left in FerroFED reads.** FerroFED keeps only what its
  identity adapters and its citation guard (`scripts/checks/ihe-citations.sh`)
  still read, at the same pin as FerroPIX's. The pull request body lists the
  kept and the removed corpora with their readers, and each removal is proven
  by FerroFED's guards passing.

- **No patient data and no credential** ever enters the tree under any
  name.
