#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/ihe-pixm.sh
#
# Vendors the IHE PIXm 3.1.0 FHIR package artefacts the ITI-83 client of
# crates/ihe-iti (feature `pixm`) reads into docs/specs/ihe-pixm/: the `$ihe-pix`
# OperationDefinition, the Query Parameters In and Out profiles, the Consumer,
# Manager and Source capability statements, the ImplementationGuide, the IG's
# own ITI-83 request, response and error examples, which the client's tests
# decode, the ITI-104 Patient Identity Feed's Patient profiles and example
# Patients, which the PIX Manager's feed will be held to, and the ITI-83
# Consumer's BALP audit profile and example, which the ITI-83 audit record
# will be held to. The set is FerroFED's, at FerroFED's pin, so the tree is
# byte-identical to FerroFED's copy. The package manifest is
# read for its name, version and licence and left out of the tree (the
# dependency-manifest rule of scripts/vendor/lib/corpus.sh).
#
# The "IHE PIXm FHIR package" row of docs/VERSIONS.md pins the package by
# version and by the sha256 of the registry tarball, so a republished package
# under the same version fails the fetch instead of changing the tree.
#
# Usage:
#   scripts/vendor/ihe-pixm.sh
#
# Requires: curl, tar, shasum, jq.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# shellcheck source=scripts/vendor/lib/corpus.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$root/scripts/vendor/lib/corpus.sh"

corpus_require curl tar shasum jq

dest="docs/specs/ihe-pixm"
name="ihe.iti.pixm"

pin="$(corpus_pin_cell "IHE PIXm FHIR package")"
version="$(awk '{ for (i = 1; i < NF; i++) if ($i == "version") { v = $(i + 1); gsub(/[`,.;:]+$/, "", v); gsub(/`/, "", v); print v; exit } }' <<< "$pin")"
want="$(awk '{ for (i = 1; i <= NF; i++) { t = $i; gsub(/[`,.;:]/, "", t); if (t ~ /^[0-9a-f]{64}$/) { print t; exit } } }' <<< "$pin")"
[ -n "$version" ] || die "the pin names no package version"
[ -n "$want" ] || die "the pin names no package sha256"

# The artefacts of the ITI-83 Get Corresponding Identifiers and ITI-104 Patient
# Identity Feed FHIR transactions, at their upstream paths inside the package.
# The Consumer's audit (BALP) profile and example are taken; the Manager's and
# the ITI-104 audit profiles and examples serve no reader here and are not.
paths=(
  package/ImplementationGuide-ihe.iti.pixm.json
  package/OperationDefinition-IHE.PIXm.pix.json
  package/StructureDefinition-IHE.PIXm.Query.Parameters.In.json
  package/StructureDefinition-IHE.PIXm.Query.Parameters.Out.json
  package/CapabilityStatement-IHE.PIXm.Consumer.json
  package/CapabilityStatement-IHE.PIXm.Manager.json
  package/CapabilityStatement-IHE.PIXm.Source.json
  package/StructureDefinition-IHE.PIXm.Patient.json
  package/StructureDefinition-IHE.PIXm.Patient.BirthDateRequired.json
  package/example/Parameters-pixm-request-mohralice-red-all.json
  package/example/Parameters-pixm-request-mohralice-red-to-blue.json
  package/example/Parameters-pixm-response-mohralice-red-all.json
  package/example/Parameters-pixm-response-mohralice-red-to-blue.json
  package/example/OperationOutcome-pixm-response-error-not-found.json
  package/example/Patient-Patient-MaidenAlice-Red.json
  package/example/Patient-Patient-MohrAlice-Blue.json
  package/example/Patient-Patient-MohrAlice-Green.json
  package/example/Patient-Patient-MohrAlice-Red.json
  package/example/Patient-Patient-MohrAlice.json
  package/example/Patient-Patient-MohrAlissa-Red.json
  package/example/Patient-Patient-MohrMaidenResolvedByMohrMalice-Red.json
  package/example/Patient-ex-patient.json
  package/StructureDefinition-IHE.PIXm.Query.Audit.Consumer.json
  package/example/AuditEvent-ex-auditPixmQuery-consumer.json
)

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

rm -rf "$dest"
mkdir -p "$dest"
corpus_take "$tmp" "$dest" "${paths[@]}"

# shellcheck disable=SC2016 # the backticks are Markdown, not a command substitution
dropped="$(printf '| `%s` | `%s` |' package/package.json "$(corpus_sha256 "$tmp/package/package.json")")"

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

# Provenance: the IHE PIXm FHIR package

Vendored verbatim by \`scripts/vendor/ihe-pixm.sh\`. Never edit a file here:
change the pin in docs/VERSIONS.md and re-run the script.

- Source: <$url>, the FHIR package registry's copy of the IG published at
  <https://profiles.ihe.net/ITI/PIXm/3.1.0/>
- Pin: package \`$name\` version \`$version\`, tarball sha256 \`$want\`
- Fetched: $fetched
- Upstream licence: Creative Commons Attribution 4.0 International
  (\`$licence\`, the \`license\` of the package manifest, listed under What is
  left out;
  <https://creativecommons.org/licenses/by/4.0/>). The package ships no licence
  file of its own. Attribution: IHE International, IT Infrastructure Technical
  Committee, *Patient Identifier Cross-referencing for Mobile (PIXm)* $version.
- FHIR version: $fhir
- Layout: the upstream paths inside the package, unchanged
- Files: $files of the package's $total, listed below
- Tree digest (sha256 over the sorted per-file \`sha256  path\` listing,
  \`PROVENANCE.md\` excluded): \`$digest\`
- Read by: the ITI-83 client of \`crates/ihe-iti\` (feature \`pixm\`), whose
  tests decode the examples and hold the request and response to the
  OperationDefinition. The Patient profiles and the audit artefacts are kept
  for the PIX Manager and its audit, as FerroFED vendors them

## What is here

The artefacts of ITI-83, Get Corresponding Identifiers: the \`\$ihe-pix\`
OperationDefinition with its in and out parameters, the Query Parameters In
and Out profiles, the Consumer and Manager capability statements, and the
IG's examples of an ITI-83 request, a response and the not-found error. The
artefacts of ITI-104, Patient Identity Feed FHIR: the Source capability
statement, the Patient profile and its birth-date variant, and the IG's
example Patients. The audit record of ITI-83 as the Consumer records it
(§2:3.83.5.1.1): the PIXm Query Consumer audit profile, built on the BALP
Patient Query pattern, and its example. The package's other files serve no
reader here: the Manager's and the ITI-104 BALP audit profiles and examples,
the Schematron renderings, the OpenAPI renderings and the registry's
\`.index.db\`, a SQLite file. They are not taken.

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
