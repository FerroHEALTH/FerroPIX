#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/ihe-balp.sh
#
# Vendors the IHE Basic Audit Log Patterns (BALP) 1.1.4 FHIR package into
# docs/specs/ihe-balp/, every file of it but the package manifest: each
# pattern an audit record of crates/ihe-iti (feature `audit`) or an
# access record can name, among them Query, Patient Query, Read, Create,
# Update and Delete and their Patient variants, the Audit Creator and Audit
# Record Repository capability statements, the code systems and value sets,
# and the IG's examples. The PIXm, mCSD, PDQm and PMIR audit profiles are
# built on these patterns. The package manifest is read for its name, version
# and licence and left out of the tree (the dependency-manifest rule of
# scripts/vendor/lib/corpus.sh).
#
# The "IHE BALP FHIR package" row of docs/VERSIONS.md pins the package by
# version and by the sha256 of the registry tarball, so a republished package
# under the same version fails the fetch instead of changing the tree.
#
# Usage:
#   scripts/vendor/ihe-balp.sh
#
# Requires: curl, tar, shasum, jq.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# shellcheck source=scripts/vendor/lib/corpus.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$root/scripts/vendor/lib/corpus.sh"

corpus_require curl tar shasum jq

dest="docs/specs/ihe-balp"
name="ihe.iti.balp"

pin="$(corpus_pin_cell "IHE BALP FHIR package")"
version="$(awk '{ for (i = 1; i < NF; i++) if ($i == "version") { v = $(i + 1); gsub(/[`,.;:]+$/, "", v); gsub(/`/, "", v); print v; exit } }' <<< "$pin")"
want="$(awk '{ for (i = 1; i <= NF; i++) { t = $i; gsub(/[`,.;:]/, "", t); if (t ~ /^[0-9a-f]{64}$/) { print t; exit } } }' <<< "$pin")"
[ -n "$version" ] || die "the pin names no package version"
[ -n "$want" ] || die "the pin names no package sha256"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

url="https://packages.fhir.org/$name/$version"
say "fetching $url"
corpus_download "$url" "$tmp/package.tgz"
got="$(corpus_sha256 "$tmp/package.tgz")"
[ "$got" = "$want" ] || die "the $name $version tarball has sha256 $got, the pin records $want"
tar -xzf "$tmp/package.tgz" -C "$tmp"

manifest_name="$(jq -r .name "$tmp/package/package.json")"
manifest_version="$(jq -r .version "$tmp/package/package.json")"
licence="$(jq -r .license "$tmp/package/package.json")"
fhir="$(jq -r '.fhirVersions | join(", ")' "$tmp/package/package.json")"
[ "$manifest_name" = "$name" ] || die "the package names itself $manifest_name, not $name"
[ "$manifest_version" = "$version" ] || die "the package declares version $manifest_version, not $version"
[ "$licence" = "CC-BY-4.0" ] || die "the package declares licence $licence, not CC-BY-4.0"

# The manifest is accounted for by its sha256 and left out; every other file
# of the package is taken at its upstream path.
dropped="$(corpus_drop_manifests "$tmp")"
[ -n "$dropped" ] || die "the package carries no package.json"

rm -rf "$dest"
mkdir -p "$dest"
corpus_take "$tmp" "$dest" package

# Each pattern the audit records and the access records name by canonical
# URL must be here.
for pattern in Query PatientQuery Read PatientRead Create PatientCreate Update PatientUpdate Delete PatientDelete; do
  [ -f "$dest/package/StructureDefinition-IHE.BasicAudit.$pattern.json" ] \
    || die "the package has no IHE.BasicAudit.$pattern pattern"
done

rows=""
while IFS= read -r file; do
  path="${file#"$dest"/}"
  rows="$rows
| \`$path\` | \`$(corpus_sha256 "$file")\` |"
done < <(find "$dest" -type f ! -name PROVENANCE.md | LC_ALL=C sort)

total="$(tar -tzf "$tmp/package.tgz" | grep -cv '/$')"
files="$(corpus_file_count "$dest")"
digest="$(corpus_tree_digest "$dest")"
fetched="$(corpus_fetched)"

cat > "$dest/PROVENANCE.md" << PROV
<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: the IHE BALP FHIR package

Vendored verbatim by \`scripts/vendor/ihe-balp.sh\`. Never edit a file here:
change the pin in docs/VERSIONS.md and re-run the script.

- Source: <$url>, the FHIR package registry's copy of the IG published at
  <https://profiles.ihe.net/ITI/BALP/$version/>
- Pin: package \`$name\` version \`$version\`, tarball sha256 \`$want\`
- Fetched: $fetched
- Upstream licence: Creative Commons Attribution 4.0 International
  (\`$licence\`, the \`license\` of the package manifest, listed under What is
  left out;
  <https://creativecommons.org/licenses/by/4.0/>). The package ships no licence
  file of its own. Attribution: IHE International, IT Infrastructure Technical
  Committee, *Basic Audit Log Patterns (BALP)* $version.
- FHIR version: $fhir
- Layout: the upstream paths inside the package, unchanged
- Files: $files of the package's $total, listed below
- Tree digest (sha256 over the sorted per-file \`sha256  path\` listing,
  \`PROVENANCE.md\` excluded): \`$digest\`
- Read by: the audit records of \`crates/ihe-iti\` (feature \`audit\`), whose
  tests hold each record to the pattern its transaction's audit profile
  derives from and a search to the client-side example. The tree is the one
  FerroFED and FerroAUDIT vendor at the same pin.

## What is here

The whole package but its manifest: every audit pattern (the RESTful Query,
Read, Create, Update and Delete patterns and their Patient variants, the
OAuth and SAML token-use, consent and privacy disclosure patterns), the Audit
Creator, Audit Consumer and Audit Record Repository capability statements,
the code systems and value sets, the IG's examples, the OpenAPI and
Schematron renderings and the registry's index and validation output.

| File | sha256 |
|---|---|$rows

## What is left out

The package manifest, \`package.json\`. The script reads its name, version
and licence from the tarball and checks them against the pin. A vendored copy
would make this repository's dependency graph claim an npm package that
depends on \`hl7.fhir.r4.core\`, a FHIR registry package whose name the GitHub
advisory database flags as a malicious npm package; nothing here installs
either.

| File | sha256 |
|---|---|
$dropped
PROV

say "$files files, tree digest $digest"
say "done"
