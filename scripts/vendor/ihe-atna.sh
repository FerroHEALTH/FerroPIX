#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/ihe-atna.sh
#
# Vendors the IHE ATNA text the audit records of crates/ihe-iti cite into
# docs/specs/ihe-atna/: the Record Audit Event [ITI-20] page of the IT
# Infrastructure Technical Framework Volume 2 with its one figure (the syslog
# interaction, its store-and-forward trigger, and the audit message format the
# ITI-55 audit message follows), and the RESTful ATNA Technical Framework
# Supplement (the ATX: FHIR Feed Option and the Send Audit Resource
# interaction of ITI-20 the BALP audit records travel over). FerroFED and
# FerroAUDIT vendor the same files at the same pins.
#
# Neither text is in a FHIR package or a tagged repository: the page is
# published as HTML at profiles.ihe.net and the supplement as a PDF at
# ihe.net, both at URLs that do not carry a revision. The "IHE ITI-20 Record
# Audit Event" and "IHE RESTful ATNA supplement" rows of docs/VERSIONS.md pin
# each file by its sha256, so a new revision fails the fetch instead of
# changing the tree, and the script checks the revision each document
# declares against the pin.
#
# Redistribution: the IHE Technical Frameworks General Introduction §9 grants
# every user a licence to reproduce and distribute IHE Technical Documents
# under IHE International's copyrights. Both texts refer to DICOM PS3.15 and
# HL7 FHIR by link and do not reproduce them.
#
# Usage:
#   scripts/vendor/ihe-atna.sh
#
# Requires: curl, shasum, pdftotext.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# shellcheck source=scripts/vendor/lib/corpus.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$root/scripts/vendor/lib/corpus.sh"

corpus_require curl shasum pdftotext

dest="docs/specs/ihe-atna"
page_url="https://profiles.ihe.net/ITI/TF/Volume2/ITI-20.html"
figure_url="https://profiles.ihe.net/ITI/TF/Volume2/media/Figure_3.20.4-1.png"
supplement_url="https://www.ihe.net/uploadedFiles/Documents/ITI/IHE_ITI_Suppl_RESTful-ATNA.pdf"

# The sha256 tokens of a pin cell, in the order the cell names them.
hashes() {
  awk '{ for (i = 1; i <= NF; i++) { t = $i; gsub(/[`,.;:]/, "", t); if (t ~ /^[0-9a-f]{64}$/) print t } }' <<< "$1"
}

page_pin="$(corpus_pin_cell "IHE ITI-20 Record Audit Event")"
supplement_pin="$(corpus_pin_cell "IHE RESTful ATNA supplement")"
page_revision="$(corpus_pin_field Revision "$page_pin" | tr -d ',')"
supplement_revision="$(corpus_pin_field Rev. "$supplement_pin" | tr -d ',')"
page_want="$(hashes "$page_pin" | sed -n 1p)"
figure_want="$(hashes "$page_pin" | sed -n 2p)"
supplement_want="$(hashes "$supplement_pin" | sed -n 1p)"
[ -n "$page_revision" ] || die "the ITI-20 pin names no Revision"
[ -n "$supplement_revision" ] || die "the RESTful ATNA pin names no Rev."
[ -n "$page_want" ] && [ -n "$figure_want" ] || die "the ITI-20 pin names no page and figure sha256"
[ -n "$supplement_want" ] || die "the RESTful ATNA pin names no sha256"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fetch() {
  local url="$1" into="$2" want="$3" got
  say "fetching $url"
  corpus_download "$url" "$into"
  got="$(corpus_sha256 "$into")"
  [ "$got" = "$want" ] || die "$url has sha256 $got, the pin records $want"
}

fetch "$page_url" "$tmp/ITI-20.html" "$page_want"
fetch "$figure_url" "$tmp/Figure_3.20.4-1.png" "$figure_want"
fetch "$supplement_url" "$tmp/IHE_ITI_Suppl_RESTful-ATNA.pdf" "$supplement_want"

grep -q "Revision $page_revision, " "$tmp/ITI-20.html" \
  || die "the ITI-20 page does not declare Revision $page_revision"
grep -q '3\.20 Record Audit Event \[ITI-20\]' "$tmp/ITI-20.html" \
  || die "the page is not the Record Audit Event [ITI-20] transaction"
pdftotext "$tmp/IHE_ITI_Suppl_RESTful-ATNA.pdf" "$tmp/supplement.txt"
grep -q "Rev\. $supplement_revision – Trial Implementation" "$tmp/supplement.txt" \
  || die "the supplement does not declare Rev. $supplement_revision Trial Implementation"
grep -q '3\.20\.4\.2 Send Audit Resource Request Message – FHIR Feed Interaction' "$tmp/supplement.txt" \
  || die "the supplement defines no Send Audit Resource Request (3.20.4.2)"
page_date="$(sed -nE 's/.*Revision [0-9.]+, ([A-Za-z]+ [0-9]+, [0-9]+) - Final Text.*/\1/p' "$tmp/ITI-20.html" | head -n1)"
# pdftotext writes the cover's "Date:" label and its value as separate lines.
supplement_date="$(awk '/^Date:/ { seen = 1; next } seen && NF { print; exit }' "$tmp/supplement.txt")"
[ -n "$page_date" ] && [ -n "$supplement_date" ] || die "a document declares no date"

rm -rf "$dest"
mkdir -p "$dest/Volume2/media"
cp "$tmp/ITI-20.html" "$dest/Volume2/ITI-20.html"
cp "$tmp/Figure_3.20.4-1.png" "$dest/Volume2/media/Figure_3.20.4-1.png"
cp "$tmp/IHE_ITI_Suppl_RESTful-ATNA.pdf" "$dest/IHE_ITI_Suppl_RESTful-ATNA.pdf"

rows=""
while IFS= read -r file; do
  path="${file#"$dest"/}"
  rows="$rows
| \`$path\` | \`$(corpus_sha256 "$file")\` |"
done < <(find "$dest" -type f ! -name PROVENANCE.md | LC_ALL=C sort)

files="$(corpus_file_count "$dest")"
digest="$(corpus_tree_digest "$dest")"
fetched="$(corpus_fetched)"

cat > "$dest/PROVENANCE.md" << PROV
<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: the IHE ATNA audit text

Vendored verbatim by \`scripts/vendor/ihe-atna.sh\`. Never edit a file here:
change the pins in docs/VERSIONS.md and re-run the script.

- Sources:
  - <$page_url> and the figure it shows,
    <$figure_url>: IHE IT Infrastructure Technical Framework Volume 2, §3.20 Record Audit Event
    [ITI-20], Revision $page_revision, $page_date, Final Text
  - <$supplement_url>: IHE IT Infrastructure Technical Framework
    Supplement *Add RESTful ATNA (Query and Feed)*, Rev. $supplement_revision,
    $supplement_date, Trial Implementation
- Pins: each file by its sha256, listed below; the URLs carry no revision
- Fetched: $fetched
- Upstream licence: copyright IHE International, Inc. The IHE Technical
  Frameworks General Introduction §9
  (<https://profiles.ihe.net/GeneralIntro/ch-9.html>) grants it, verbatim:
  "IHE International hereby grants to each Member Organization, and to any
  other user of these documents, an irrevocable, worldwide, perpetual,
  royalty-free, nontransferable, nonexclusive, non-sublicensable license
  under its copyrights in any IHE profiles and Technical Framework documents,
  as well as any additional copyrighted materials that will be owned by IHE
  International and will be made available for use by Member Organizations,
  to reproduce and distribute (in any and all print, electronic or other
  means of reproduction, storage or transmission) such IHE Technical
  Documents." Both texts refer to DICOM PS3.15 Annex A.5 and HL7 FHIR R4 by
  link; neither base standard is vendored here.
- Layout: the page under \`Volume2/\` with its figure at the relative path the
  page links (\`media/\`), the supplement at the top
- Files: $files
- Tree digest (sha256 over the sorted per-file \`sha256  path\` listing,
  \`PROVENANCE.md\` excluded): \`$digest\`
- Read by: the ITI-20 citations of \`crates/ihe-iti\` (feature \`audit\`),
  held to these files by \`scripts/checks/ihe-citations.sh\`

## What is here

\`Volume2/ITI-20.html\` is the whole Record Audit Event transaction: the
Send Audit Event syslog interaction (§3.20.4.1) with its trigger events and
the store-and-forward a sender that cannot reach its repository keeps
(§3.20.4.1.1), the syslog message semantics and transports (§3.20.4.1.2), and
the IHE Audit Trail Message Format (§3.20.7). The page's stylesheets, scripts
and logo serve no reader here and are not taken. The supplement adds the
ATX: FHIR Feed Option (ITI TF-1 §9.2.7.1) and the FHIR Feed interactions of
ITI-20: Send Audit Resource Request (§3.20.4.2), its mapping from the DICOM
audit message to the FHIR \`AuditEvent\` (Table 3.20.4.2.2.1-1) and the
Send Audit Resource Response (§3.20.4.3). The status is the document's own:
a Trial Implementation supplement may be amended before it is incorporated
into the Technical Framework.

| File | sha256 |
|---|---|$rows
PROV

say "$files files, tree digest $digest"
say "done"
