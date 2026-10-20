#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/ch.sh
#
# Vendors the Swiss pins of the IHE identity profiles, one corpus each: the
# IHE PIXm, PDQm and IUA revisions EPDV-EDI Annex 5 pins
# (docs/specs/ihe-pixm-ch/, docs/specs/ihe-pdqm-ch/, docs/specs/ihe-iua-ch/),
# which differ from the revisions crates/ihe-iti binds. FerroFED's ch.sh, of
# which this is the identity part, fetches the same three at the same pins.
#
# Each artefact is pinned by URL and sha256 (scripts/vendor/lib/pinned.sh),
# and each corpus row of docs/VERSIONS.md carries its pin-set digest.
#
# Usage:
#   scripts/vendor/ch.sh
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

pin commit ihe.iti.pixm-3.0.4.tgz "3.0.4" \
  https://packages.fhir.org/ihe.iti.pixm/3.0.4 \
  49d5964f1e9bc7d413ab87a2d6487613ef276f53f02a004a38e207b35cc39fd2 \
  'CC-BY-4.0: the license of the package manifest. Attribution: IHE International, IT Infrastructure Technical Committee, Patient Identifier Cross-referencing for Mobile (PIXm) 3.0.4' \
  'The registry tarball as served, package manifest included; the archive is committed whole so its sha256 stays the pin.'
pinned_corpus ihe-pixm-ch "IHE PIXm FHIR package, Swiss pin" \
  "IHE PIXm 3.0.4, the revision Swiss Annex 5 pins" \
  "EPDV-EDI Annex 5 pins IHE PIXm 3.0.4; \`crates/ihe-iti\` binds PIXm 3.1.0
(\`docs/specs/ihe-pixm/\`). The whole registry tarball is kept, so the two
revisions can be compared."

pin commit ihe.iti.pdqm-3.1.0.tgz "3.1.0" \
  https://packages.fhir.org/ihe.iti.pdqm/3.1.0 \
  3952220c822bc8dfec86d5635677a994f5bc64b1bd0d72b8054ac064fdd4aa6d \
  'CC-BY-4.0: the license of the package manifest. Attribution: IHE International, IT Infrastructure Technical Committee, Patient Demographics Query for Mobile (PDQm) 3.1.0' \
  'The registry tarball as served, package manifest included; the archive is committed whole so its sha256 stays the pin.'
pinned_corpus ihe-pdqm-ch "IHE PDQm FHIR package, Swiss pin" \
  "IHE PDQm 3.1.0, the revision Swiss Annex 5 pins" \
  "EPDV-EDI Annex 5 pins IHE PDQm 3.1.0; \`crates/ihe-iti\` binds PDQm 3.2.0
(\`docs/specs/ihe-pdqm/\`). The whole registry tarball is kept, so the two
revisions can be compared."

pin commit ITI.IUA-2.3-7c70ff7.tar.gz "tag 2.3, commit 7c70ff7b2b18d392baf083d9eee4b17a3f1a7c52" \
  https://codeload.github.com/IHE/ITI.IUA/tar.gz/7c70ff7b2b18d392baf083d9eee4b17a3f1a7c52 \
  7a4f88dc7268f2716515daacb6bac951397771205a9085bb479b08f749473c0b \
  'CC-BY-4.0: the repository LICENSE inside the archive, "Attribution 4.0 International"; the IHE Technical Frameworks General Introduction §9 also grants every user a licence to reproduce and distribute IHE Technical Documents' \
  'Fetched by commit, so the archive'"'"'s top directory names the commit; GitHub does not promise byte-stable archives, so a re-generated archive fails the pin loudly.'
pinned_corpus ihe-iua-ch "IHE IUA supplement, Swiss pin" \
  "IHE IUA Revision 2.3, the revision Swiss Annex 5 pins" \
  "EPDV-EDI Annex 5 pins IHE IUA Revision 2.3; \`crates/ihe-iti\` binds
Revision 2.5 (\`docs/specs/ihe-iua/\`). The repository archive at the commit
tag \`2.3\` resolves to is kept whole, so the two revisions can be compared."

say "done"
