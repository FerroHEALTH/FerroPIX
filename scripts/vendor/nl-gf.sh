#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/nl-gf.sh
#
# Vendors the source of the Netherlands Generic Functions IG (package
# `fhir.nl.gf`)
# into docs/specs/nl-gf/: the narrative pages of GF-Localization, GF-Consent,
# GF-Addressing (the Care Services Directory), GF-Identification and
# GF-Authentication with its six transactions (GFI-001 to GFI-006), the FSH
# sources of the profiles, naming systems and capability statements those
# pages define, the examples of the localization record and of the LRZa and
# Query Directory content, the localization and access-token sequence
# diagrams, the IG's build configuration and the repository's licence.
#
# The clients of crates/nl-generic-functions read it: the NVI search of
# feature `nvi` is held to the Localization Service capability
# statement and the localization record profile, the LRZa reading of feature
# `lrza` to the Organization profiles and the LRZa examples, and the
# access token request of feature `nuts-auth` to GF-Authentication and
# its Request Access Token [GFI-004] and Authenticated Interaction [GFI-005]
# transactions.
#
# The IG is released as a git tag with no package on the FHIR package
# registry, so the "Netherlands Generic Functions IG source" row of
# docs/VERSIONS.md pins the tag and the commit it resolved to; the script
# fails when the tag has moved. The row also names the package version,
# which sushi-config.yaml must declare.
#
# Redistribution: the repository is licensed EUPL-1.2 (its LICENSE, vendored
# beside the text, and the `license` of sushi-config.yaml). The script fails if
# either stops saying so.
#
# Usage:
#   scripts/vendor/nl-gf.sh
#
# Requires: curl, tar, shasum, jq.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# shellcheck source=scripts/vendor/lib/corpus.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$root/scripts/vendor/lib/corpus.sh"

corpus_require curl tar shasum jq

dest="docs/specs/nl-gf"
paths=(
  LICENSE
  sushi-config.yaml
  input/pagecontent/index.md
  input/pagecontent/localization.md
  input/pagecontent/consent.md
  input/pagecontent/care-services.md
  input/pagecontent/identification.md
  input/pagecontent/authorization.md
  input/pagecontent/authentication.md
  input/pagecontent/GFI-001.md
  input/pagecontent/GFI-002.md
  input/pagecontent/GFI-003.md
  input/pagecontent/GFI-004.md
  input/pagecontent/GFI-005.md
  input/pagecontent/GFI-006.md
  input/fsh/aliases.fsh
  input/fsh/rulesets.fsh
  input/fsh/namingsystem.fsh
  input/fsh/codesystems.fsh
  input/fsh/valuesets.fsh
  input/fsh/searchparameters.fsh
  input/fsh/structuredefinitions.fsh
  input/fsh/nl_gf_localization_document_reference.fsh
  input/fsh/capabilitystatement-localization-repository.fsh
  input/fsh/capabilitystatement-localization-lmr.fsh
  input/fsh/capabilitystatement-querydirectory.fsh
  input/fsh/capabilitystatement-admindirectory-updateclient.fsh
  input/fsh/examples/gf-localization.fsh
  input/fsh/examples/admin-directory-lrza.fsh
  input/fsh/examples/query-directory.fsh
  input/images-source/localization-cardiologist-search.plantuml
  input/images-source/gfi-004.plantuml
  input/images-source/gfi-005.plantuml
)

pin="$(corpus_pin_cell "Netherlands Generic Functions IG source")"
repo="$(corpus_pin_repo "$pin")"
tag="$(corpus_pin_tag "$pin")"
want="$(corpus_pin_commit "$pin")"
version="$(awk '{ for (i = 1; i < NF; i++) if ($i == "version") { v = $(i + 1); gsub(/[,.;:]+$/, "", v); print v; exit } }' <<< "$pin")"
[ -n "$version" ] || die "the pin names no package version"

commit="$(corpus_resolve_tag "$repo" "$tag")"
[ "$commit" = "$want" ] || die "tag $tag of $repo resolves to $commit, the pin records $want"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

say "$repo tag $tag resolves to $commit"
tree_root="$(corpus_fetch "$repo" "$commit" "$tmp")"

[ ! -f "$dest/PROVENANCE.md" ] || cp "$dest/PROVENANCE.md" "$tmp/PROVENANCE.old"
rm -rf "$dest"
mkdir -p "$dest"
corpus_take "$tree_root" "$dest" "${paths[@]}"

id="$(sed -nE 's/^id:[[:space:]]*([^[:space:]#]+).*/\1/p' "$dest/sushi-config.yaml" | head -n1)"
declared="$(sed -nE 's/^version:[[:space:]]*([^[:space:]#]+).*/\1/p' "$dest/sushi-config.yaml" | head -n1)"
licence="$(sed -nE 's/^license:[[:space:]]*([^[:space:]#]+).*/\1/p' "$dest/sushi-config.yaml" | head -n1)"
fhir="$(sed -nE 's/^fhirVersion:[[:space:]]*([^[:space:]#]+).*/\1/p' "$dest/sushi-config.yaml" | head -n1)"
[ "$id" = "fhir.nl.gf" ] || die "sushi-config.yaml names the IG $id, not fhir.nl.gf"
[ "$declared" = "$version" ] || die "sushi-config.yaml declares version $declared, docs/VERSIONS.md pins $version"
[ "$licence" = "EUPL-1.2" ] || die "sushi-config.yaml declares licence $licence, not EUPL-1.2"
grep -q 'EUROPEAN UNION PUBLIC LICENCE v. 1.2' "$dest/LICENSE" \
  || die "the repository LICENSE is not the European Union Public Licence 1.2"

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

# Provenance: the Netherlands Generic Functions IG source

Vendored verbatim by \`scripts/vendor/nl-gf.sh\`. Never edit a file here:
change the pin in docs/VERSIONS.md and re-run the script.

- Source: <https://github.com/$repo>, rendered at
  <https://build.fhir.org/ig/nuts-foundation/nl-generic-functions-ig/>
- Pin: tag \`$tag\`, which resolves to commit \`$commit\`
- Document: *Netherlands - Generic Functions for data exchange
  Implementation Guide*, package \`$id\` version $declared, published by
  Stichting Nuts
- Fetched: $fetched
- Upstream licence: the European Union Public Licence 1.2 (\`$licence\`, the
  \`license\` of \`sushi-config.yaml\` and the repository's \`LICENSE\`,
  vendored beside this file)
- FHIR version: $fhir
- Layout: the upstream paths, unchanged
- Files: $files
- Tree digest (sha256 over the sorted per-file \`sha256  path\` listing,
  \`PROVENANCE.md\` excluded): \`$digest\`
- Read by: the NVI search of \`crates/nl-generic-functions\` feature
  \`nvi\`, held to the Localization Service capability statement and the
  localization record profile, the LRZa reading of feature \`lrza\`, held
  to the Organization profiles and the LRZa examples, and the access token
  request of feature \`nuts-auth\`, held to GF-Authentication and its
  GFI-004 and GFI-005 transactions

## What is taken

The IG has no package on the FHIR package registry; its release is the git
tag, so the source is taken. The narrative pages of the functions FerroFED's
Federation Tier specification binds (localization, consent, the care
services directory, identification, authentication with its six GFI
transactions, and authorization), the FSH sources of the profiles, naming
systems, code systems, value sets, search parameters and capability
statements those pages define, the examples of the localization record and of
the LRZa Administration Directory and the Query Directory, the localization
sequence diagram and the sequence diagrams of Request Access Token (GFI-004)
and Authenticated Interaction (GFI-005), and \`sushi-config.yaml\`, which
names the package, its version and its licence. The pages and examples of
routing, care teams and workflow, the rendered images and the build scripts
serve no reader here and are not taken.

| File | sha256 | git blob id |
|---|---|---|$rows
PROV
corpus_keep_fetched "$tmp/PROVENANCE.old" "$dest/PROVENANCE.md"

say "$files files, tree digest $digest"
say "done"
