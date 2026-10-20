#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/ihe-iti-tf.sh
#
# Vendors the three IHE ITI Technical Framework Volume 1 chapters FerroFED's
# country research cited into docs/specs/ihe-iti-tf/: XUA (ch. 13), XCA
# (ch. 18) and XCPD (ch. 27), and the two Volume 2 transactions the XCPD code
# cites by section into docs/specs/ihe-iti-tf-vol2/: Cross Gateway Query
# [ITI-38], committed, and Cross Gateway Patient Discovery [ITI-55], fetched
# into the git-ignored .vendor-cache/ihe-iti-tf-vol2/ alone, each as
# profiles.ihe.net renders it.
#
# Each page is pinned by URL and sha256 (scripts/vendor/lib/pinned.sh), and
# each corpus row of docs/VERSIONS.md carries its pin-set digest.
#
# Usage:
#   scripts/vendor/ihe-iti-tf.sh
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

ihe='IHE Technical Frameworks General Introduction §9: IHE International grants "to any other user of these documents, an irrevocable, worldwide, perpetual, royalty-free, nontransferable, nonexclusive, non-sublicensable license under its copyrights in any IHE profiles and Technical Framework documents [...] to reproduce and distribute" them (page footer "© 2000 — IHE International")'
rendered='Rendered HTML page whose bytes may change on a re-render; re-fetched on 2026-10-04 with the same sha256.'

pin commit ihe-iti-xua-ch13.html "ITI TF Revision 20.2, page as served on 2026-10-04" \
  https://profiles.ihe.net/ITI/TF/Volume1/ch-13.html \
  e9624b8007480f653103a641a0db3c8a2a3d6efdb44c33396e5fb70a10851108 "$ihe" "$rendered"
pin commit ihe-iti-xca-ch18.html "ITI TF Revision 20.2, page as served on 2026-10-04" \
  https://profiles.ihe.net/ITI/TF/Volume1/ch-18.html \
  2bd1b6569e15923f6ded425be7abe045ee159ec2f874abd9e1afcb40e79ce6d1 "$ihe" "$rendered"
pin commit ihe-iti-xcpd-ch27.html "ITI TF Revision 20.2, page as served on 2026-10-04" \
  https://profiles.ihe.net/ITI/TF/Volume1/ch-27.html \
  fc88613b05a3fb08d40980f36ead00bc38e962ae811378225ac7c016c61966ad "$ihe" "$rendered"
pinned_corpus ihe-iti-tf "IHE ITI Technical Framework Volume 1 pages" \
  "IHE ITI Technical Framework Volume 1, chapters 13, 18 and 27" \
  "The profile overviews of Cross-Enterprise User Assertion (XUA),
Cross-Community Access (XCA) and Cross-Community Patient Discovery (XCPD),
which MyHealth@EU and the Norwegian and Swiss record networks build on. They
are IHE's own text, which the General Introduction §9
(<https://profiles.ihe.net/GeneralIntro/ch-9.html>) licenses for
reproduction and distribution; that licence does not reach base-standard
material, and these Volume 1 chapters reproduce no HL7 table. The Volume 2
transaction text of ITI-38 and ITI-55 is the corpus
\`docs/specs/ihe-iti-tf-vol2/\`, beside this one."

# The Volume 2 transaction pages the identity code cites by section: Cross
# Gateway Query [ITI-38], committed, and Cross Gateway Patient Discovery
# [ITI-55], cache only. General Introduction §9.1.2 reserves every right in
# the HL7 tables IHE reproduces by permission, and the ITI-55 page carries the
# HL7 Version 3 message information model tables of its two messages.
hl7='IHE Technical Frameworks General Introduction §9.1.2: "Health Level Seven, Inc. has granted permission to IHE to reproduce tables from the HL7 standard. The HL7 tables in this document are copyrighted by Health Level Seven, Inc. All rights reserved." The page reproduces the HL7 Version 3 message information model tables of its query and its response, so it is not redistributed.'

pin commit ITI-38.html "ITI TF Revision 20.2, page as served on 2026-10-06" \
  https://profiles.ihe.net/ITI/TF/Volume2/ITI-38.html \
  49be7b6b435ff05153c2da655369dfb8eac2c6685489ec237285d05af394a192 "$ihe" \
  "Rendered HTML page whose bytes may change on a re-render. The page links one figure, media/Figure_3.38.4-1.png, which is not taken."
pin cache ITI-55.html "ITI TF Volume 2, the page states Revision 20.1, served on 2026-10-06" \
  https://profiles.ihe.net/ITI/TF/Volume2/ITI-55.html \
  67dcce3339dd2fae07db50422c02f36e8d11053b5740aceef847361e33bfb801 "$hl7" \
  "Rendered HTML page whose bytes may change on a re-render."
pinned_corpus ihe-iti-tf-vol2 "IHE ITI Technical Framework Volume 2 pages" \
  "IHE ITI Technical Framework Volume 2, transactions ITI-38 and ITI-55" \
  "The Cross Gateway Query [ITI-38] and Cross Gateway Patient Discovery
[ITI-55] transactions, whose sections the XCPD code cites and the IHE
section citation guard (\`scripts/checks/ihe-citations.sh\`) checks. ITI-38 is IHE's own text, which the General Introduction §9
(<https://profiles.ihe.net/GeneralIntro/ch-9.html>) licenses for
reproduction and distribution; it names the OASIS ebXML registry standards
and reproduces none of their text. ITI-55 reproduces HL7 Version 3 tables,
whose rights HL7 reserves (General Introduction §9.1.2), so it is cache
only: the guard checks the ITI-55 citations against the cached page when a
local run has fetched it, and counts them otherwise." \
  "" "the IHE section citation guard, \`scripts/checks/ihe-citations.sh\`,
  over the ITI-55 citations of crates/ihe-iti"

say "done"
