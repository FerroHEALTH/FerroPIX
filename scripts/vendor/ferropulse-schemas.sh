#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/ferropulse-schemas.sh
#
# Vendors FerroPULSE's report envelope v2, the machine-readable half of the
# usage report contract: the JSON Schema and its valid and invalid examples
# (.claude/rules/vendored-inputs.md). It reads the pin from the "FerroPULSE
# report envelope v2" row of docs/VERSIONS.md (a commit and the sha256 of the
# schema), copies the files verbatim into
# app/ferropix-server/tests/fixtures/ferropulse/, checks the schema against
# the pinned digest, and writes a PROVENANCE.md beside them. The server's
# integration tests validate every report it builds against the schema.
# FerroPULSE owns the contract; never hand-edit the copy, move the pin and
# re-run.
#
# The repository is private, so the fetch goes through the authenticated
# GitHub API (`gh api`, raw media type).
#
# Usage: scripts/vendor/ferropulse-schemas.sh
# Requires: gh (authenticated with read access to FerroHEALTH/FerroPULSE),
# shasum, awk.
#
# No specification governs this file; it is FerroPIX's own design, after
# FerroBRIDGE's script of the same name.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# shellcheck source=scripts/vendor/lib/corpus.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$root/scripts/vendor/lib/corpus.sh"

corpus_require gh shasum awk

item="FerroPULSE report envelope v2"
dest="app/ferropix-server/tests/fixtures/ferropulse"
schema="schemas/report-envelope.v2.json"

cell="$(corpus_pin_cell "$item")"
repo="$(corpus_pin_repo "$cell")"
sha="$(corpus_pin_commit "$cell")"
want="$(grep -oE '\b[0-9a-f]{64}\b' <<< "$cell" | head -n1 || true)"
[ -n "$want" ] || die "the '$item' row names no sha256 of $schema"

fetch() {
  local path="$1"
  mkdir -p "$(dirname "$dest/$path")"
  gh api -H 'Accept: application/vnd.github.raw' "repos/$repo/contents/$path?ref=$sha" > "$dest/$path"
}

# The files of one directory at the pin, by name.
list() {
  gh api "repos/$repo/contents/$1?ref=$sha" --jq '.[] | select(.type == "file") | .name'
}

say "$repo at $sha"
old="$(mktemp)"
trap 'rm -f "$old"' EXIT
[ ! -f "$dest/PROVENANCE.md" ] || cp "$dest/PROVENANCE.md" "$old"
# A file the pin no longer carries must not linger in the copy.
[ ! -d "$dest" ] || find "$dest" -type f -delete
fetch "$schema"
got="$(shasum -a 256 "$dest/$schema" | awk '{ print $1 }')"
[ "$got" = "$want" ] || die "$schema at $sha has sha256 $got, and docs/VERSIONS.md pins $want"
for validity in valid invalid; do
  dir="schemas/examples/report-envelope.v2/$validity"
  while IFS= read -r name; do
    fetch "$dir/$name"
  done < <(list "$dir")
done
count="$(find "$dest/schemas" -type f | wc -l | tr -d ' ')"

cat > "$dest/PROVENANCE.md" << PROV
<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Provenance: the FerroPULSE report envelope v2

- Source: <https://github.com/$repo> (private; Cadasto B.V., the Licensor
  of this repository too)
- Commit: $sha
- Schema sha256: $want
- Fetched: $(date -u +%Y-%m-%d) by \`scripts/vendor/ferropulse-schemas.sh\`
- Licence: upstream declares \`LicenseRef-Proprietary\` in its \`Cargo.toml\`
  and ships no LICENSE file; Cadasto B.V. holds the copyright of both
  repositories and publishes this copy under this repository's licence
- Contents: \`$schema\` (JSON Schema 2020-12) and
  \`schemas/examples/report-envelope.v2/\` (the contract's own valid and
  invalid examples), $count files, copied verbatim

The usage report tests in \`app/ferropix-server/tests/it/usage_report.rs\`
check the validator against the examples, then validate every report the
server builds against the schema. Never hand-edit these files; change the pin
in \`docs/VERSIONS.md\` and re-run the script.
PROV
corpus_keep_fetched "$old" "$dest/PROVENANCE.md"
say "vendored $count files under $dest"
