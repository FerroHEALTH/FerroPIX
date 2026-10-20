#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/ihe-iti-pages.sh
#
# Vendors the narrative pages of the IHE ITI profiles the code cites by
# section number (PIXm `§2:3.83.4.2.2.1`, PMIR `§1:49`, BALP `§3:5.7.5.4`),
# one corpus per profile beside its FHIR package: the Volume 1 page and each
# transaction page of PIXm, PDQm, PMIR and mCSD, and the Volume 1 and Volume
# 3 pages of BALP, as profiles.ihe.net renders them at the version the
# package corpus pins. The FHIR packages carry the profiles and examples, not
# the text the section numbers name. IUA has no FHIR package; its supplement
# text is vendored by scripts/vendor/ihe-iua.sh.
#
# Each page is pinned by its versioned URL and sha256
# (scripts/vendor/lib/pinned.sh), and each corpus row of docs/VERSIONS.md
# carries its pin-set digest. The pages are the published implementation
# guides, licensed CC-BY-4.0 by each ImplementationGuide resource, with the
# page footer's licence link to IHE General Introduction §9.
#
# Usage:
#   scripts/vendor/ihe-iti-pages.sh
#
# Requires: curl, shasum, awk.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# shellcheck source=scripts/vendor/lib/corpus.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$root/scripts/vendor/lib/corpus.sh"
# shellcheck source=scripts/vendor/lib/pinned.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$root/scripts/vendor/lib/pinned.sh"

corpus_require curl shasum awk
pinned_begin

ig=https://profiles.ihe.net/ITI
read_by='the section citations of crates/ihe-iti, held by scripts/checks/ihe-citations.sh'

# licence PACKAGE VERSION: the licence statement of one IG's pages.
licence() {
  printf 'CC-BY-4.0: the license of the ImplementationGuide resource of %s %s; the page footer links IHE General Introduction §9 (https://profiles.ihe.net/GeneralIntro/ch-9.html)' "$1" "$2"
}

pixm="$(licence ihe.iti.pixm 3.1.0)"
pin commit volume-1.html "PIXm 3.1.0, 1:41" "$ig/PIXm/3.1.0/volume-1.html" \
  c4038f25b92d56a36b4f55fdf4850759b79f8035dc9fdb29c213c7899f95cda5 "$pixm"
pin commit ITI-83.html "PIXm 3.1.0, 2:3.83" "$ig/PIXm/3.1.0/ITI-83.html" \
  2e4b9a17a35d5199bfd5b1479c7cde9d1ea40b719115f848115cf1cd33847cd3 "$pixm"
pin commit ITI-104.html "PIXm 3.1.0, 2:3.104" "$ig/PIXm/3.1.0/ITI-104.html" \
  30dc0324b288f59f1d3b26958b828cef658629fe3343c2b56d0870b28a6a6d32 "$pixm"
pinned_corpus ihe-pixm-pages "IHE PIXm narrative pages" \
  "the IHE PIXm 3.1.0 narrative pages" \
  "The Volume 1 page (1:41) and the Mobile Patient Identifier
Cross-reference Query [ITI-83] and Patient Identity Feed FHIR [ITI-104]
pages of the PIXm 3.1.0 implementation guide, the text of the package
vendored in docs/specs/ihe-pixm/, committed under CC-BY-4.0. Attribution:
IHE International, IT Infrastructure Technical Committee, *Patient Identifier
Cross-referencing for mobile (PIXm)* 3.1.0." "" "$read_by"

pdqm="$(licence ihe.iti.pdqm 3.2.0)"
pin commit volume-1.html "PDQm 3.2.0, 1:38" "$ig/PDQm/3.2.0/volume-1.html" \
  86261d703309e3def28ba155acad1dedb6162361b0eae3db7d80a2ad9775ae29 "$pdqm"
pin commit ITI-78.html "PDQm 3.2.0, 2:3.78" "$ig/PDQm/3.2.0/ITI-78.html" \
  0d7bae88d408e820f5e3ea0884b04dc77f66ee83eaf87ac64daf9842a6ab636c "$pdqm"
pin commit ITI-119.html "PDQm 3.2.0, 2:3.119" "$ig/PDQm/3.2.0/ITI-119.html" \
  ce466c7492a565ef5c8e428b41c28e62bef31bd072a2dd362ae4956cfdd8b593 "$pdqm"
pinned_corpus ihe-pdqm-pages "IHE PDQm narrative pages" \
  "the IHE PDQm 3.2.0 narrative pages" \
  "The Volume 1 page (1:38) and the Mobile Patient Demographics Query
[ITI-78] and Patient Demographics Match [ITI-119] pages of the PDQm 3.2.0
implementation guide, the text of the package vendored in
docs/specs/ihe-pdqm/, committed under CC-BY-4.0. Attribution: IHE
International, IT Infrastructure Technical Committee, *Patient Demographics
Query for Mobile (PDQm)* 3.2.0." "" "$read_by"

pmir="$(licence ihe.iti.pmir 1.6.0)"
pin commit volume-1.html "PMIR 1.6.0, 1:49" "$ig/PMIR/1.6.0/volume-1.html" \
  7f03afa490b73442edbff12fea1b1fadc5e64df75028027ba6568dd1fcc931d0 "$pmir"
pin commit ITI-93.html "PMIR 1.6.0, 2:3.93" "$ig/PMIR/1.6.0/ITI-93.html" \
  845f847703c51c66afc4726859f8d1b516336060682a6ed5e5f526e235423ce3 "$pmir"
pin commit ITI-94.html "PMIR 1.6.0, 2:3.94" "$ig/PMIR/1.6.0/ITI-94.html" \
  2f1bcd5a1b19a56b8bf0e0f9b4554a32c22557b745d31dfbf0f7d1e2b22e3f1b "$pmir"
pinned_corpus ihe-pmir-pages "IHE PMIR narrative pages" \
  "the IHE PMIR 1.6.0 narrative pages" \
  "The Volume 1 page (1:49) and the Mobile Patient Identity Feed [ITI-93]
and Subscribe to Patient Updates [ITI-94] pages of the PMIR 1.6.0
implementation guide, the text of the package vendored in
docs/specs/ihe-pmir/, committed under CC-BY-4.0. Attribution: IHE
International, IT Infrastructure Technical Committee, *Patient Master
Identity Registry (PMIR)* 1.6.0." "" "$read_by"

mcsd="$(licence ihe.iti.mcsd 4.0.0)"
pin commit volume-1.html "mCSD 4.0.0, 1:46" "$ig/mCSD/4.0.0/volume-1.html" \
  b658946f453feee4caa64490ef3990d6c0299c208c81adf2a174ab19414be227 "$mcsd"
pin commit ITI-90.html "mCSD 4.0.0, 2:3.90" "$ig/mCSD/4.0.0/ITI-90.html" \
  e85e922e5dc7ef645f85e4bd39e2750f451ba51d6f70bcc35301063db0d3a9cf "$mcsd"
pin commit ITI-91.html "mCSD 4.0.0, 2:3.91" "$ig/mCSD/4.0.0/ITI-91.html" \
  10712a41062ccd724b98247d20c76a3a646061fc77ded0310da0069e6355d43f "$mcsd"
pinned_corpus ihe-mcsd-pages "IHE mCSD narrative pages" \
  "the IHE mCSD 4.0.0 narrative pages" \
  "The Volume 1 page (1:46) and the Find Matching Care Services [ITI-90]
and Request Care Services Updates [ITI-91] pages of the mCSD 4.0.0
implementation guide, the text of the package vendored in
docs/specs/ihe-mcsd/, committed under CC-BY-4.0. Attribution: IHE
International, IT Infrastructure Technical Committee, *Mobile Care Services
Discovery (mCSD)* 4.0.0." "" "$read_by"

balp="$(licence ihe.iti.balp 1.1.4)"
pin commit volume-1.html "BALP 1.1.4, 1:52" "$ig/BALP/1.1.4/volume-1.html" \
  e2503b35872f1ea70f420ad1c5274e100f82f6282b558cc5071ca66fcc59e8c8 "$balp"
pin commit content.html "BALP 1.1.4, 3:5.7" "$ig/BALP/1.1.4/content.html" \
  a3c220e25086f97941544d9919ea521a63126023d66c27bfcd5e2e7ed17f9d09 "$balp"
pinned_corpus ihe-balp-pages "IHE BALP narrative pages" \
  "the IHE BALP 1.1.4 narrative pages" \
  "The Volume 1 page (1:52) and the Volume 3 content page (3:5.7) of the
Basic Audit Log Patterns 1.1.4 implementation guide, the text of the
package vendored in docs/specs/ihe-balp/, committed under CC-BY-4.0.
Attribution: IHE International, IT Infrastructure Technical Committee,
*Basic Audit Log Patterns (BALP)* 1.1.4." "" "$read_by"

say "done"
