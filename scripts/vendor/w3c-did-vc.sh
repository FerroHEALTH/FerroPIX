#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/w3c-did-vc.sh
#
# Vendors the identifier and credential specifications the GF-Authentication
# pages of the Netherlands Generic Functions IG cite into
# docs/specs/w3c-did-vc/: the Verifiable Credentials Data Model 1.1 (the
# credential and presentation, and their JWT encoding of §6.3.1), Decentralized
# Identifiers 1.0, DID Resolution, Bitstring Status List 1.0 and the did:web
# Method Specification.
#
# The access token request of crates/nl-generic-functions feature
# `nuts-auth` builds its Verifiable Presentation to VC Data Model 1.1
# §4.10 and §6.3.1, names its holder by a did:web identifier (DID 1.0 §3,
# the did:web method) and signs it with a key a DID URL names.
#
# The W3C documents are dated publications, pinned by URL and sha256 in their
# docs/VERSIONS.md rows; the did:web method is a Credentials Community Group
# report kept in a repository, pinned by commit.
#
# Redistribution: the W3C documents are published under the W3C Software and
# Document License, which grants permission to copy and distribute them in
# any medium for any purpose, and the did:web repository licenses its reports
# under the same licence (its LICENSE.md, vendored beside the text). The
# script fails if a document stops naming that licence.
#
# Usage:
#   scripts/vendor/w3c-did-vc.sh
#
# Requires: curl, tar, shasum, jq.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# shellcheck source=scripts/vendor/lib/corpus.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$root/scripts/vendor/lib/corpus.sh"

corpus_require curl tar shasum jq

dest="docs/specs/w3c-did-vc"
licence_2015="https://www.w3.org/Consortium/Legal/2015/copyright-software-and-document"
licence_2023="https://www.w3.org/copyright/software-license-2023/"

# Each W3C document: its docs/VERSIONS.md row and the file it is saved as.
documents=(
  "W3C Verifiable Credentials Data Model 1.1|vc-data-model-1.1.html"
  "W3C Decentralized Identifiers 1.0|did-core-1.0.html"
  "W3C DID Resolution 1.0|did-resolution-1.0.html"
  "W3C Bitstring Status List 1.0|vc-bitstring-status-list-1.0.html"
)

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/out"
pins=""
for entry in "${documents[@]}"; do
  item="${entry%%|*}"
  file="${entry#*|}"
  say "fetching $item"
  corpus_fetch_pinned "$item" "$tmp/out/$file"
  grep -qF -e "$licence_2015" -e "$licence_2023" "$tmp/out/$file" \
    || die "$item no longer names the W3C Software and Document License"
  pin="$(corpus_pin_cell "$item")"
  pins="$pins
- $item: <$(corpus_pin_url "$pin")>, sha256 \`$(corpus_pin_sha256 "$pin")\`"
done

method="$(corpus_pin_cell "did:web Method Specification")"
repo="$(corpus_pin_repo "$method")"
commit="$(corpus_pin_commit "$method")"
say "fetching $repo at $commit"
tree_root="$(corpus_fetch "$repo" "$commit" "$tmp")"
grep -qF "W3C Software and Document" "$tree_root/LICENSE.md" \
  || die "the did:web repository's LICENSE.md no longer names the W3C Software and Document License"
mkdir -p "$tmp/out/did-method-web"
cp "$tree_root/index.html" "$tmp/out/did-method-web/index.html"
cp "$tree_root/LICENSE.md" "$tmp/out/did-method-web/LICENSE.md"

rm -rf "$dest"
mkdir -p "$dest"
cp -R "$tmp/out/." "$dest/"

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

# Provenance: the W3C identifier and credential specifications

Vendored verbatim by \`scripts/vendor/w3c-did-vc.sh\`. Never edit a file here:
change the pins in docs/VERSIONS.md and re-run the script.

- Source: the W3C Technical Reports, <https://www.w3.org/TR/>, each at its
  dated URL, and the Credentials Community Group repository
  <https://github.com/$repo>
- Pins, one row of docs/VERSIONS.md each:$pins
- did:web Method Specification: commit \`$commit\` of \`$repo\`, its
  \`index.html\` (the report's source) and \`LICENSE.md\`
- Fetched: $fetched
- Upstream licence: the W3C documents carry the W3C copyright notice and
  name the W3C Software and Document License, the 2015 text
  (<$licence_2015>) for the two 2022 Recommendations and the 2023 text
  (<$licence_2023>) for the later documents; both permit
  copying and distributing the documents in any medium for any purpose,
  provided the URL of the original, the copyright notice and the status of the
  document are kept; each file is the original, unmodified, with all three.
  The did:web repository's \`LICENSE.md\`, vendored in
  \`did-method-web/\`, licenses its reports under the same licence.
- Layout: one file per W3C document, named for it; \`did-method-web/\` at the
  repository's own paths
- Files: $files
- Tree digest (sha256 over the sorted per-file \`sha256  path\` listing,
  \`PROVENANCE.md\` excluded): \`$digest\`
- Read by: the Verifiable Presentation of the access token request of
  \`crates/nl-generic-functions\` feature \`nuts-auth\`: VC Data Model 1.1
  §4.10 and §6.3.1, and the did:web holder identifier

## What is taken

The documents the GF-Authentication pages of the Netherlands Generic
Functions IG cite for the entity identifier (DID 1.0, the did:web method and
DID Resolution, GFI-001), the credential and its presentation (VC Data Model
1.1, GFI-002 and GFI-004) and revocation (Bitstring Status List 1.0,
GFI-003). DID Resolution and Bitstring Status List are the verifier's side of
the exchange; they are taken because the IG binds them, so the record of
what the track requires is complete.

| File | sha256 |
|---|---|$rows
PROV

say "$files files, tree digest $digest"
say "done"
