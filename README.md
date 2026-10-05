<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->
# <img src="assets/brand/ferropix-icon.svg" alt="" width="40" height="40" align="top"> FerroPIX

[![License: BUSL-1.1](https://img.shields.io/badge/License-BUSL--1.1-blue.svg)](LICENSE)

A Master Patient Index for the openEHR platform, in pure Rust: who the patient is.

Every organisation issues its own identifiers for the same person, and records move. FerroPIX holds the cross-reference between those identifiers and the location of each record, and answers the question every other component starts with: who is this patient, and where is their record. It exposes that over the IHE profiles integrators already expect, PIX and PDQ, in their FHIR forms PIXm and PDQm, and it keeps identifying data outside the clinical record, which is where the openEHR Platform Service Model puts the EHR Index.

FerroPIX is one of the [FerroHEALTH](https://ferrohealth.eu/) family. The family
page shows where it sits among the eight and what calls what, and this
repository is where the design and the build happen; the tracker is the
record of both. Its site will be <https://ferropix.eu/>.

## Licence

FerroPIX is source-available under the Business Source License 1.1. The
parameters that apply, the Licensor, the Licensed Work, the Additional Use
Grant and the Change Date, are in [LICENSE](LICENSE): free for non-commercial
production use, a commercial licence for any other production use, and Apache
2.0 four years after each version is published. The maintainer named in
[MAINTAINERS.md](MAINTAINERS.md) is the contact for a commercial licence.

The brand assets under `assets/brand/` are part of the Licensed Work.

Contributions carry the terms in
[CONTRIBUTING.md](CONTRIBUTING.md#licensing-of-contributions): you keep your
copyright, and you grant the Licensor the relicensing right that keeps the work
one work under one licensor. There is no separate agreement to sign.
