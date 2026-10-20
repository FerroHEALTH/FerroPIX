#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/dif-pe.sh
#
# Vendors DIF Presentation Exchange 2.0.0 into docs/specs/dif-pe/: the
# specification text, its JSON Schemas and the repository's licence, with
# the claim format designation schemas of the DIF Claim Format Registry its
# submission schema references, under claim-format-registry/.
#
# Nuts RFC021 §3 sends a Presentation Submission with every access token
# request and §5 serves a Presentation Definition per scope, both as
# Presentation Exchange defines them. The access token request of
# crates/nl-generic-functions feature `nuts-auth` reads the definition
# and writes the submission, and its tests hold both to these schemas.
#
# The repository has no release tags, so the "DIF Presentation Exchange
# 2.0.0" row of docs/VERSIONS.md pins a commit; the v2.0.0 text and schemas
# sit under their own versioned directories in it.
#
# Redistribution: both repositories are licensed Apache-2.0 (each LICENSE,
# vendored beside its files). The script fails if one stops saying so.
#
# Usage:
#   scripts/vendor/dif-pe.sh
#
# Requires: curl, tar, shasum, jq.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# shellcheck source=scripts/vendor/lib/corpus.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$root/scripts/vendor/lib/corpus.sh"

corpus_require curl tar shasum jq

dest="docs/specs/dif-pe"
paths=(
  LICENSE
  spec/v2.0.0/spec.md
  schemas/v2.0.0/input-descriptor.json
  schemas/v2.0.0/presentation-definition.json
  schemas/v2.0.0/presentation-definition-envelope.json
  schemas/v2.0.0/presentation-submission.json
  schemas/v2.0.0/submission-requirement.json
  schemas/v2.0.0/submission-requirements.json
)

pin="$(corpus_pin_cell "DIF Presentation Exchange 2.0.0")"
repo="$(corpus_pin_repo "$pin")"
commit="$(corpus_pin_commit "$pin")"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

say "fetching $repo at $commit"
tree_root="$(corpus_fetch "$repo" "$commit" "$tmp")"

rm -rf "$dest"
mkdir -p "$dest"
corpus_take "$tree_root" "$dest" "${paths[@]}"

registry_pin="$(corpus_pin_cell "DIF Claim Format Registry")"
registry_repo="$(corpus_pin_repo "$registry_pin")"
registry_commit="$(corpus_pin_commit "$registry_pin")"
say "fetching $registry_repo at $registry_commit"
mkdir -p "$tmp/registry"
registry_root="$(corpus_fetch "$registry_repo" "$registry_commit" "$tmp/registry")"
mkdir -p "$dest/claim-format-registry"
corpus_take "$registry_root" "$dest/claim-format-registry" \
  LICENSE \
  schemas/presentation-definition-claim-format-designations.json \
  schemas/presentation-submission-claim-format-designations.json

for licence in "$dest/LICENSE" "$dest/claim-format-registry/LICENSE"; do
  if ! grep -qF "Apache License" "$licence" || ! grep -qF "Version 2.0, January 2004" "$licence"; then
    die "$licence is not the Apache License 2.0"
  fi
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

# Provenance: DIF Presentation Exchange 2.0.0

Vendored verbatim by \`scripts/vendor/dif-pe.sh\`. Never edit a file here:
change the pin in docs/VERSIONS.md and re-run the script.

- Source: <https://github.com/$repo>, rendered at
  <https://identity.foundation/presentation-exchange/spec/v2.0.0/>
- Pin: commit \`$commit\`; the claim format registry,
  <https://github.com/$registry_repo>, at commit \`$registry_commit\`, under
  \`claim-format-registry/\`
- Document: *Presentation Exchange 2.0.0*, Decentralized Identity Foundation
- Fetched: $fetched
- Upstream licence: the Apache License 2.0 (each repository's \`LICENSE\`,
  vendored beside its files)
- Layout: the upstream paths, unchanged, the registry's under
  \`claim-format-registry/\`
- Files: $files
- Tree digest (sha256 over the sorted per-file \`sha256  path\` listing,
  \`PROVENANCE.md\` excluded): \`$digest\`
- Read by: the Presentation Definition the access token request of
  \`crates/nl-generic-functions\` feature \`nuts-auth\` reads and the
  Presentation Submission it writes, held to the schemas

## What is taken

The v2.0.0 specification text, the version Nuts RFC021 cites, and its JSON
Schemas, and the claim format designations the submission schema
references for its \`format\`. The other versions, the playground, the sample implementation and
the test vectors serve no reader here and are not taken.

| File | sha256 | git blob id |
|---|---|---|$rows
PROV

say "$files files, tree digest $digest"
say "done"
