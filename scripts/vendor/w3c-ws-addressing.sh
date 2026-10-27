#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/w3c-ws-addressing.sh
#
# Vendors the W3C Web Services Addressing 1.0 SOAP Binding Recommendation
# into docs/specs/w3c-ws-addressing/. The XCPD Responding Gateway of the
# server cites its §5.2.2 for the asynchronous exchange: a response to a
# request whose reply endpoint is not the anonymous address is not the
# outbound message of the same exchange.
#
# The document is a dated publication, pinned by URL and sha256 in its
# docs/VERSIONS.md row.
#
# Redistribution: the Recommendation carries the W3C copyright notice and
# links the W3C Document License, which permits copying and distributing the
# document in any medium for any purpose, unmodified, with its notice. The
# script fails if the document stops linking that licence.
#
# Usage:
#   scripts/vendor/w3c-ws-addressing.sh
#
# Requires: curl, shasum.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# shellcheck source=scripts/vendor/lib/corpus.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$root/scripts/vendor/lib/corpus.sh"

corpus_require curl shasum

dest="docs/specs/w3c-ws-addressing"
item="W3C Web Services Addressing 1.0 SOAP Binding"
file="ws-addr-soap-1.0.html"
licence="https://www.w3.org/Consortium/Legal/copyright-documents"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

say "fetching $item"
corpus_fetch_pinned "$item" "$tmp/$file"
grep -qF "$licence" "$tmp/$file" \
  || die "$item no longer links the W3C Document License"
pin="$(corpus_pin_cell "$item")"

[ ! -f "$dest/PROVENANCE.md" ] || cp "$dest/PROVENANCE.md" "$tmp/PROVENANCE.old"
rm -rf "$dest"
mkdir -p "$dest"
cp "$tmp/$file" "$dest/$file"

files="$(corpus_file_count "$dest")"
digest="$(corpus_tree_digest "$dest")"
fetched="$(corpus_fetched)"

cat > "$dest/PROVENANCE.md" << PROV
<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: the W3C Web Services Addressing 1.0 SOAP Binding

Vendored verbatim by \`scripts/vendor/w3c-ws-addressing.sh\`. Never edit a
file here: change the pin in docs/VERSIONS.md and re-run the script.

- Source: the W3C Technical Reports, <https://www.w3.org/TR/>, at the dated
  URL of the Recommendation of 2006-05-09
- Pin, the docs/VERSIONS.md row: $item, <$(corpus_pin_url "$pin")>, sha256
  \`$(corpus_pin_sha256 "$pin")\`
- Fetched: $fetched
- Upstream licence: the document carries the W3C copyright notice and links
  the W3C Document License (<$licence>), which permits copying and
  distributing it in any medium for any purpose, provided the URL of the
  original, the copyright notice and the status of the document are kept;
  the file is the original, unmodified, with all three.
- Layout: one file, named for the document
- Files: $files
- Tree digest (sha256 over the sorted per-file \`sha256  path\` listing,
  \`PROVENANCE.md\` excluded): \`$digest\`
- Read by: the XCPD Responding Gateway of \`app/ferropix-server\`, which cites
  §5.2.2 for the asynchronous exchange of ITI TF-2 Appendix V.5

| File | sha256 |
|---|---|
| \`$file\` | \`$(corpus_sha256 "$dest/$file")\` |
PROV
corpus_keep_fetched "$tmp/PROVENANCE.old" "$dest/PROVENANCE.md"

say "$files files, tree digest $digest"
say "done"
