#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/ihe-xcpd-examples.sh
#
# Vendors IHE's example messages of Cross Gateway Patient Discovery [ITI-55]
# into docs/specs/ihe-xcpd-examples/: the synchronous request and its
# answers, the Deferred Response Option's messages, and the revoke and
# patient location query messages. ITI TF-2 Appendix W names IHE's
# implementation material, examples included, and points to the IHE Google
# Drive, where the files sit under ITI/examples/XCPD. The XCPD tests of
# crates/ihe-iti and app/ferropix-server read them.
#
# Each file is pinned by its Drive id, through the download URL, and the
# sha256 of its bytes.
#
# Usage:
#   scripts/vendor/ihe-xcpd-examples.sh
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

ihe='IHE Technical Frameworks General Introduction §9: IHE International grants "to any other user of these documents, an irrevocable, worldwide, perpetual, royalty-free, nontransferable, nonexclusive, non-sublicensable license under its copyrights in any IHE profiles and Technical Framework documents [...] to reproduce and distribute" them; ITI TF-2 Appendix W names these examples as the non-normative implementation material of the Technical Framework'
drive='https://drive.usercontent.google.com/download?export=download&confirm=t&id='
served='IHE Google Drive, ITI/examples/XCPD, as served on 2026-10-26'

# example FILE DRIVE-ID SHA256: one example file, committed.
example() {
  pin commit "$1" "$served" "$drive$2" "$3" "$ihe"
}

example XCPDCrossGatewayPatientDiscoveryRequest.xml 1n0NvrSEahACZXXiWkra48eERel9yHWUD 7cbe6e57b97fe5211542d63282eeef2c46e09346180584d400a56d05f19b5784
example XCPDCrossGatewayPatientDiscoveryResponse.xml 1n43ENRTPCm98U7v6s2sfye18KYfOG2pw e8fb6d80d570367580f8e50e60caf990e957f0e94a2b3cb9ed08a50012685ebb
example XCPDCrossGatewayPatientDiscoveryResponseAEError.xml 1nLLPF8ULeYsuQ7bKvYS-BC4s9h12zcYN e9f3e9a2b416f31c9f8f74c0730064a550c1814880912de31c8611888b36a16d
example XCPDCrossGatewayPatientDiscoveryResponseForMoreDemographics.xml 1nPPG9XSO5KoVrR7Wpg2xYU47I0xS2ETe 604162b819c5a32b09112300a92337e02a5a90f9500e7323b93a7837508aac67
example XCPDCrossGatewayPatientDiscoveryRevoke.xml 1nSqoj1hV8nu3jgf6FQZvJ67JkWWGE3Qu 3745a27c73d35baa8c01b301684d18a73b4b64e08f7d097a71433ef0ecf61fad
example XCPDCrossGatewayPatientDiscovery_Deferred_Request.xml 1nZiGAMrRDAtj3g3szo5dRWUzOi1U40Qg 880192b17826d58ea4a0d245bb5ecab39d53367d9c0908bb2654e7384b2660d4
example XCPDCrossGatewayPatientDiscovery_Deferred_RequestAck.xml 1n_DElac29R6GZzn4J60Bmkl3oVqdetn_ 1c7e08f2dca2ab43256b17460776947ab18455a0daaef123040e218be849ce92
example XCPDCrossGatewayPatientDiscovery_Deferred_RequestNack.xml 1ndgy0neUjN1inXWTGLjxonfjy00K0FOf b4536cda3814b5082f2a1b8ee8ccc42c449e4e878819c836436198907088c4ff
example XCPDCrossGatewayPatientDiscovery_Deferred_Response.xml 1nmTpRuOxSAFFwXfaLt6HJO31IxiaIotX 3f011b0dd2faaedc7a63cd8f60f8bfeb84c9df0aaedea3c04652327625bc7d48
example XCPDCrossGatewayPatientDiscovery_Deferred_ResponseAck.xml 1nw7dxMnsyASBu1GvUfluIbyJQJ2KiBsE a0ece108e629583ba2fb440ed6544bd79bbef73ca9c7a692a75ef8900b3c7913
example XCPDPatientLocationQueryRequest.xml 1nx6Fx8rxIRdSTQCksi502EcQxMyBLdkh 0057b999eb1e690c0ce993b5d3b3fba6f3c3e12c8285dfc9f20d50a1e8fc7281
example XCPDPatientLocationQueryResponse.xml 1o4Ny46asAd6zRtHRgpakVKHi24ZmN4Bw e883680ed648bd114e9d9c30a2c79ccffcab145d6eefde69a0d09e62c5eace94
pinned_corpus ihe-xcpd-examples "IHE XCPD example messages" \
  "IHE example messages of Cross Gateway Patient Discovery [ITI-55]" \
  "IHE's example messages of ITI-55, from the implementation material ITI TF-2
Appendix W names (<https://profiles.ihe.net/ITI/TF/Volume2/ch-W.html>). They
are IHE material, which the General Introduction §9
(<https://profiles.ihe.net/GeneralIntro/ch-9.html>) licenses for
reproduction and distribution, and reproduce no HL7 table. The patients they
name are IHE's fictional test patients." \
  "" "the XCPD tests of crates/ihe-iti and app/ferropix-server"
