#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/ihe-pdqm.sh
#
# Vendors the IHE PDQm 3.2.0 FHIR package artefacts the ITI-78 client of
# crates/ihe-iti (feature `pdqm`) reads into docs/specs/ihe-pdqm/: the
# Patient Demographics Consumer (Query) and Supplier capability statements,
# whose Patient search parameters the client's query is held to, the Query
# Patient Resource Response Message Bundle profile and the PDQm Patient profile,
# the ImplementationGuide, and the IG's own response Bundle and Patient
# examples, which the client's tests decode, and the Consumer's ITI-78 BALP
# audit profile and example, which the ITI-78 audit record will be held to,
# and the ITI-119 `$match` artefacts the match client of crates/ihe-iti
# reads: the OperationDefinition, the
# input and output profiles, the Consumer and Supplier capability statements,
# the Consumer's ITI-119 audit profile, and their examples. The package
# manifest is read for its name, version and licence and left out of the tree
# (the dependency-manifest rule of scripts/vendor/lib/corpus.sh).
#
# The "IHE PDQm FHIR package" row of docs/VERSIONS.md pins the package by
# version and by the sha256 of the registry tarball, so a republished package
# under the same version fails the fetch instead of changing the tree.
#
# Usage:
#   scripts/vendor/ihe-pdqm.sh
#
# Requires: curl, tar, shasum, jq.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# shellcheck source=scripts/vendor/lib/corpus.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$root/scripts/vendor/lib/corpus.sh"

corpus_require curl tar shasum jq

dest="docs/specs/ihe-pdqm"
name="ihe.iti.pdqm"

pin="$(corpus_pin_cell "IHE PDQm FHIR package")"
version="$(awk '{ for (i = 1; i < NF; i++) if ($i == "version") { v = $(i + 1); gsub(/[`,.;:]+$/, "", v); gsub(/`/, "", v); print v; exit } }' <<< "$pin")"
want="$(awk '{ for (i = 1; i <= NF; i++) { t = $i; gsub(/[`,.;:]/, "", t); if (t ~ /^[0-9a-f]{64}$/) { print t; exit } } }' <<< "$pin")"
[ -n "$version" ] || die "the pin names no package version"
[ -n "$want" ] || die "the pin names no package sha256"

# The artefacts of the ITI-78 Mobile Patient Demographics Query transaction, at
# their upstream paths inside the package, with the Consumer's ITI-78 audit
# (BALP) profile and example, and the artefacts of the ITI-119 Patient
# Demographics Match with the Consumer's ITI-119 audit profile and example.
# The Supplier audit profiles and their examples, the XML renderings and the
# OperationOutcome examples serve no reader here and are not taken.
paths=(
  package/ImplementationGuide-ihe.iti.pdqm.json
  package/CapabilityStatement-IHE.PDQm.PatientDemographicsConsumerQuery.json
  package/CapabilityStatement-IHE.PDQm.PatientDemographicsSupplier.json
  package/StructureDefinition-IHE.PDQm.Patient.json
  package/StructureDefinition-IHE.PDQm.QueryPatientResourceResponseMessage.json
  package/example/Bundle-ex-QueryPatientResourceResponseMessage.json
  package/example/Patient-ex-patient.json
  package/example/Patient-ex-patient-mothers-maiden-name.json
  package/StructureDefinition-IHE.PDQm.Query.Audit.Consumer.json
  package/example/AuditEvent-ex-auditPdqmQuery-consumer.json
  package/OperationDefinition-PDQmMatch.json
  package/CapabilityStatement-IHE.PDQm.PatientDemographicsConsumerMatch.json
  package/CapabilityStatement-IHE.PDQm.PatientDemographicsSupplierMatch.json
  package/StructureDefinition-IHE.PDQm.MatchInputPatient.json
  package/StructureDefinition-IHE.PDQm.MatchParametersIn.json
  package/StructureDefinition-IHE.PDQm.MatchParametersOut.json
  package/example/Patient-ex-match-input-patient.json
  package/example/Parameters-ex-match-input-patient-only.json
  package/example/Parameters-ex-match-input-onlyCertainMatches.json
  package/example/Bundle-ex-match-output.json
  package/example/Bundle-ex-match-output-multiple.json
  package/example/Bundle-ex-match-output-empty.json
  package/example/Bundle-ex-match-output-warning.json
  package/example/Bundle-ex-match-output-error.json
  package/StructureDefinition-IHE.PDQm.Match.Audit.Consumer.json
  package/example/AuditEvent-ex-auditPdqmMatch-consumer.json
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

# Provenance: the IHE PDQm FHIR package

Vendored verbatim by \`scripts/vendor/ihe-pdqm.sh\`. Never edit a file here:
change the pin in docs/VERSIONS.md and re-run the script.

- Source: <$url>, the FHIR package registry's copy of the IG published at
  <https://profiles.ihe.net/ITI/PDQm/3.2.0/>
- Pin: package \`$name\` version \`$version\`, tarball sha256 \`$want\`
- Fetched: $fetched
- Upstream licence: Creative Commons Attribution 4.0 International
  (\`$licence\`, the \`license\` of the package manifest, listed under What is
  left out;
  <https://creativecommons.org/licenses/by/4.0/>). The package ships no licence
  file of its own. Attribution: IHE International, IT Infrastructure Technical
  Committee, *Patient Demographics Query for Mobile (PDQm)* $version.
- FHIR version: $fhir
- Layout: the upstream paths inside the package, unchanged
- Files: $files of the package's $total, listed below
- Tree digest (sha256 over the sorted per-file \`sha256  path\` listing,
  \`PROVENANCE.md\` excluded): \`$digest\`
- Read by: the ITI-78 client of \`crates/ihe-iti\` (feature \`pdqm\`), whose
  tests hold the query to the Supplier's Patient search parameters and decode
  the example response Bundle and Patients, and its ITI-119 \`\$match\`
  client, held to the operation, its input and output profiles and their
  examples. The audit artefacts are kept as FerroFED vendors them

## What is here

The artefacts of ITI-78, Mobile Patient Demographics Query: the Patient
Demographics Consumer (Query) and Supplier capability statements, which list
the Patient search parameters a Supplier processes, the Query Patient Resource
Response Message profile of the \`searchset\` Bundle, the PDQm Patient profile,
the ImplementationGuide, and the IG's examples of a response Bundle and two
Patients, with the Consumer's ITI-78 audit profile, built on the BALP Patient
Query pattern, and its example. With them, the artefacts of ITI-119, Patient
Demographics Match: the \`\$match\` OperationDefinition, the Consumer and
Supplier match capability statements, the input Parameters, input Patient and
output Bundle profiles, the IG's input and output examples, and the
Consumer's ITI-119 audit profile and its example. The package's other files
serve no reader here: the Supplier audit profiles and their examples, the
OperationOutcome examples, the XML renderings, and the registry's validation
output. They are not taken.

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
