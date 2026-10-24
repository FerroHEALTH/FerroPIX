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
# into the git-ignored .vendor-cache/ihe-iti-tf-vol2/ alone. The five HL7 v2
# transactions of PIX and PDQ (ITI-8, ITI-9, ITI-10, ITI-21 and ITI-22) are
# fetched into .vendor-cache/ihe-iti-tf-hl7v2/ alone, with their
# PROVENANCE.md in docs/specs/ihe-iti-tf-hl7v2/. Every page is taken as
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

# The Volume 2 transaction pages of the HL7 v2 face: Patient Identity Feed
# [ITI-8], PIX Query [ITI-9], PIX Update Notification [ITI-10], Patient
# Demographics Query [ITI-21] and Patient Demographics and Visit Query
# [ITI-22], all cache only. Each page reproduces the HL7 v2 segment tables of
# its messages, whose rights HL7 reserves (General Introduction §9.1.2).
hl7v2='IHE Technical Frameworks General Introduction §9.1.2: "Health Level Seven, Inc. has granted permission to IHE to reproduce tables from the HL7 standard. The HL7 tables in this document are copyrighted by Health Level Seven, Inc. All rights reserved." The page reproduces the HL7 v2 segment tables of its messages, so it is not redistributed.'

pin cache ITI-8.html "ITI TF Volume 2, the page states Revision 20.0, served on 2026-10-24" \
  https://profiles.ihe.net/ITI/TF/Volume2/ITI-8.html \
  64f5a636b8d5ed530b35a93491bea19e6746088bfd161c86cf1d3ef3b1bef693 "$hl7v2" \
  "Rendered HTML page whose bytes may change on a re-render. The page links one figure, media/Figure_3.8-1.png, which is not taken."
pin cache ITI-9.html "ITI TF Volume 2, the page states Revision 20.0, served on 2026-10-24" \
  https://profiles.ihe.net/ITI/TF/Volume2/ITI-9.html \
  db5bbdd715e04c9c909349b3744582729d08bee198258bf39d98537121e80372 "$hl7v2" \
  "Rendered HTML page whose bytes may change on a re-render. The page links one figure, media/Figure_3.9-1.png, which is not taken."
pin cache ITI-10.html "ITI TF Volume 2, the page states Revision 20.0, served on 2026-10-24" \
  https://profiles.ihe.net/ITI/TF/Volume2/ITI-10.html \
  ddac0c13917df03c8cf7c273e656bd167e7872175546a05761fdc266950dbd60 "$hl7v2" \
  "Rendered HTML page whose bytes may change on a re-render. The page links one figure, media/Figure_3.10-1.png, which is not taken."
pin cache ITI-21.html "ITI TF Volume 2, the page states Revision 20.2, served on 2026-10-24" \
  https://profiles.ihe.net/ITI/TF/Volume2/ITI-21.html \
  27dc792c15915aa7f5297ddcccffec44c08baacab357ff202cc5107c610fcbe4 "$hl7v2" \
  "Rendered HTML page whose bytes may change on a re-render. The page links one figure, media/Figure_3.21.4-1.png, which is not taken."
pin cache ITI-22.html "ITI TF Volume 2, the page states Revision 20.1, served on 2026-10-24" \
  https://profiles.ihe.net/ITI/TF/Volume2/ITI-22.html \
  b4f2736afde9aa9136751624069b37be8b05ea633f4846058c69fa26f2b47e27 "$hl7v2" \
  "Rendered HTML page whose bytes may change on a re-render. The page links one figure, media/Figure_3.22.4-1.png, which is not taken."
pinned_corpus ihe-iti-tf-hl7v2 "IHE ITI Technical Framework Volume 2 HL7 v2 pages" \
  "IHE ITI Technical Framework Volume 2, transactions ITI-8, ITI-9, ITI-10, ITI-21 and ITI-22" \
  "The HL7 v2 transactions of PIX and PDQ: Patient Identity Feed [ITI-8], PIX
Query [ITI-9], PIX Update Notification [ITI-10], Patient Demographics Query
[ITI-21] and Patient Demographics and Visit Query [ITI-22], whose sections
the HL7 v2 face cites and the IHE section citation guard
(\`scripts/checks/ihe-citations.sh\`) checks. Each page reproduces the HL7
v2 segment tables of its messages, whose rights HL7 reserves (General
Introduction §9.1.2, <https://profiles.ihe.net/GeneralIntro/ch-9.html>), so
all five are cache only: the guard checks their citations against the
cached pages when a local run has fetched them, and counts them otherwise." \
  "" "the IHE section citation guard, \`scripts/checks/ihe-citations.sh\`,
  over the ITI-8, ITI-9, ITI-10, ITI-21 and ITI-22 citations"

say "done"
