#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/nuts-rfc.sh
#
# Vendors the Nuts specifications GF-Authentication runs on into docs/specs/nuts-rfc/: RFC021, the VP Token Grant Type (the
# Verifiable Presentation as the authorization grant, the Presentation
# Definition endpoint and the access token request); RFC003, OAuth2
# Authorization (the authorization server and access token of the earlier
# Nuts flow, on the RFC 7523 JWT bearer grant the IG's GFI-004 also names);
# and RFC022, the Discovery Service (how a Nuts participant finds the
# authorization server of another).
#
# The access token request of crates/nl-generic-functions feature
# `nuts-auth` is held to RFC021.
#
# The specifications live on the default branch of their repository, whose
# last tag predates RFC021, so the "Nuts specifications" row of
# docs/VERSIONS.md pins a commit.
#
# Redistribution: the repository carries no licence file; each document
# states its own in its Copyright Notice, Creative Commons
# Attribution-ShareAlike 4.0 International. The script fails if a document
# stops saying so.
#
# Usage:
#   scripts/vendor/nuts-rfc.sh
#
# Requires: curl, tar, shasum, jq.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# shellcheck source=scripts/vendor/lib/corpus.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$root/scripts/vendor/lib/corpus.sh"

corpus_require curl tar shasum jq

dest="docs/specs/nuts-rfc"
paths=(
  rfc/rfc003-oauth2-authorization.md
  rfc/rfc021-vp_token-grant-type.md
  rfc/rfc022-discovery-service.md
)
statement='This document is released under the [Attribution-ShareAlike 4.0 International \(CC BY-SA 4.0\) license](https://creativecommons.org/licenses/by-sa/4.0/).'

pin="$(corpus_pin_cell "Nuts specifications")"
repo="$(corpus_pin_repo "$pin")"
commit="$(corpus_pin_commit "$pin")"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

say "fetching $repo at $commit"
tree_root="$(corpus_fetch "$repo" "$commit" "$tmp")"

rm -rf "$dest"
mkdir -p "$dest"
corpus_take "$tree_root" "$dest" "${paths[@]}"

for path in "${paths[@]}"; do
  grep -qF "$statement" "$dest/$path" \
    || die "$path no longer states the CC BY-SA 4.0 licence in its Copyright Notice"
done

rows=""
while IFS= read -r file; do
  path="${file#"$dest"/}"
  rows="$rows
| \`$path\` | \`$(corpus_sha256 "$file")\` | \`$(corpus_blob_id "$file")\` |"
done < <(find "$dest" -type f ! -name PROVENANCE.md | LC_ALL=C sort)

files="$(corpus_file_count "$dest")"
digest="$(corpus_tree_digest "$dest")"
fetched="$(corpus_fetched)"

cat > "$dest/PROVENANCE.md" << PROV
<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: the Nuts specifications

Vendored verbatim by \`scripts/vendor/nuts-rfc.sh\`. Never edit a file here:
change the pin in docs/VERSIONS.md and re-run the script.

- Source: <https://github.com/$repo>
- Pin: commit \`$commit\`
- Documents: *RFC003 OAuth2 Authorization*, *RFC021 VP Token Grant Type* and
  *RFC022 Discovery Service*, published by the Nuts foundation as Requests for
  Comments
- Fetched: $fetched
- Upstream licence: the repository has no licence file. Each document states
  its licence in its Copyright Notice, verbatim: "$statement"
  (<https://creativecommons.org/licenses/by-sa/4.0/legalcode>). The files are
  redistributed unmodified, with that notice, so no adaptation is made.
- Layout: the upstream paths, unchanged
- Files: $files
- Tree digest (sha256 over the sorted per-file \`sha256  path\` listing,
  \`PROVENANCE.md\` excluded): \`$digest\`
- Read by: the access token request of \`crates/nl-generic-functions\`
  feature \`nuts-auth\`, held to RFC021

## What is taken

The three documents GF-Authentication on the Nuts profile rests on: RFC021, the grant the client side sends; RFC003, the
authorization server and access token it replaces; and RFC022, the service
discovery a participant reaches another's authorization server through. The
other RFCs (the network, the registry, the credential formats of the earlier
Nuts node) and the repository's images serve no reader here and are not
taken.

| File | sha256 | git blob id |
|---|---|---|$rows
PROV

say "$files files, tree digest $digest"
say "done"
