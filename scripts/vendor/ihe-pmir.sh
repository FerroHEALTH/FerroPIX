#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/ihe-pmir.sh
#
# Vendors the IHE PMIR 1.6.0 FHIR package artefacts the ITI-94 subscriber and
# the ITI-93 feed reader of crates/ihe-iti (feature `pmir`) read into
# docs/specs/ihe-pmir/: the four actors' capability statements, the ITI-93
# MessageDefinition and its response, the message Bundle, history Bundle,
# MessageHeader, MessageHeader response and merged Patient profiles, the
# Subscription and Subscription request profiles of ITI-94, the
# ImplementationGuide, and the IG's own message Bundle, response MessageHeader
# and Subscription examples, which the tests decode, and the Consumer's ITI-93
# and the Subscriber's ITI-94 audit (BALP) profiles and examples, which the
# audit records will be held to. The
# Update audit profile and the related-person artefacts serve no reader here
# and are not taken. The package
# manifest is read for its name, version and licence and left out of the tree
# (the dependency-manifest rule of scripts/vendor/lib/corpus.sh).
#
# The "IHE PMIR FHIR package" row of docs/VERSIONS.md pins the package by
# version and by the sha256 of the registry tarball, so a republished package
# under the same version fails the fetch instead of changing the tree.
#
# Usage:
#   scripts/vendor/ihe-pmir.sh
#
# Requires: curl, tar, shasum, jq.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# shellcheck source=scripts/vendor/lib/corpus.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$root/scripts/vendor/lib/corpus.sh"

corpus_require curl tar shasum jq

dest="docs/specs/ihe-pmir"
name="ihe.iti.pmir"

pin="$(corpus_pin_cell "IHE PMIR FHIR package")"
version="$(awk '{ for (i = 1; i < NF; i++) if ($i == "version") { v = $(i + 1); gsub(/[`,.;:]+$/, "", v); gsub(/`/, "", v); print v; exit } }' <<< "$pin")"
want="$(awk '{ for (i = 1; i <= NF; i++) { t = $i; gsub(/[`,.;:]/, "", t); if (t ~ /^[0-9a-f]{64}$/) { print t; exit } } }' <<< "$pin")"
[ -n "$version" ] || die "the pin names no package version"
[ -n "$want" ] || die "the pin names no package sha256"

# The artefacts of the ITI-93 Mobile Patient Identity Feed and ITI-94
# Subscribe to Patient Updates transactions, at their upstream paths inside
# the package, with the Feed and Subscription Create, Read and Delete audit
# (BALP) profiles and the Consumer's and the Subscriber's examples of them. The
# Subscription Update audit profile, the other audit examples, the
# related-person profiles and examples, the standalone history Bundle and
# Patient examples
# (each is inlined in a message Bundle example taken here), the OpenAPI and
# XML renderings and the registry's validation output serve no reader here and
# are not taken.
paths=(
  package/ImplementationGuide-ihe.iti.pmir.json
  package/CapabilityStatement-IHE.PMIR.PatientIdentityConsumer.json
  package/CapabilityStatement-IHE.PMIR.PatientIdentityRegistry.json
  package/CapabilityStatement-IHE.PMIR.PatientIdentitySource.json
  package/CapabilityStatement-IHE.PMIR.PatientIdentitySubscriber.json
  package/MessageDefinition-IHE.PMIR.MessageDefinition.json
  package/MessageDefinition-IHE.PMIR.MessageDefinition.Response.json
  package/StructureDefinition-IHE.PMIR.Bundle.json
  package/StructureDefinition-IHE.PMIR.Bundle.History.json
  package/StructureDefinition-IHE.PMIR.MessageHeader.json
  package/StructureDefinition-IHE.PMIR.MessageHeader.Response.json
  package/StructureDefinition-IHE.PMIR.Patient.Merge.json
  package/StructureDefinition-IHE.PMIR.Subscription.json
  package/StructureDefinition-IHE.PMIR.Subscription.Request.json
  package/example/Bundle-ex-PMIRBundleCreate.json
  package/example/Bundle-ex-PMIRBundleDelete.json
  package/example/Bundle-ex-PMIRBundleMerge.json
  package/example/Bundle-ex-PMIRBundleUpdate.json
  package/example/MessageHeader-ex-messageheader-create-response.json
  package/example/Subscription-ex-subscription-request.json
  package/example/Subscription-ex-subscription.json
  package/StructureDefinition-IHE.PMIR.Feed.Audit.json
  package/StructureDefinition-IHE.PMIR.Audit.Subscription.Create.json
  package/StructureDefinition-IHE.PMIR.Audit.Subscription.Read.json
  package/StructureDefinition-IHE.PMIR.Audit.Subscription.Delete.json
  package/example/AuditEvent-ex-auditPmirFeed-consumer.json
  package/example/AuditEvent-ex-auditPmirSubscription-subscriber-create.json
  package/example/AuditEvent-ex-auditPmirSubscription-subscriber-read.json
  package/example/AuditEvent-ex-auditPmirSubscription-subscriber-delete.json
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

# Provenance: the IHE PMIR FHIR package

Vendored verbatim by \`scripts/vendor/ihe-pmir.sh\`. Never edit a file here:
change the pin in docs/VERSIONS.md and re-run the script.

- Source: <$url>, the FHIR package registry's copy of the IG published at
  <https://profiles.ihe.net/ITI/PMIR/$version/>
- Pin: package \`$name\` version \`$version\`, tarball sha256 \`$want\`
- Fetched: $fetched
- Upstream licence: Creative Commons Attribution 4.0 International
  (\`$licence\`, the \`license\` of the package manifest, listed under What is
  left out;
  <https://creativecommons.org/licenses/by/4.0/>). The package ships no licence
  file of its own. Attribution: IHE International, IT Infrastructure Technical
  Committee, *Patient Master Identity Registry (PMIR)* $version.
- FHIR version: $fhir
- Layout: the upstream paths inside the package, unchanged
- Files: $files of the package's $total, listed below
- Tree digest (sha256 over the sorted per-file \`sha256  path\` listing,
  \`PROVENANCE.md\` excluded): \`$digest\`
- Read by: the ITI-94 subscriber and the ITI-93 feed reader of
  \`crates/ihe-iti\` (feature \`pmir\`), whose tests decode the example
  message Bundles and hold the subscription and the feed response to the
  profiles. The audit artefacts are kept as FerroFED vendors them

## What is here

The artefacts of ITI-93, Mobile Patient Identity Feed, and ITI-94, Subscribe
to Patient Updates: the Patient Identity Consumer, Registry, Source and
Subscriber capability statements, the feed MessageDefinition and its response,
the message Bundle, history Bundle, MessageHeader, MessageHeader response and
merged Patient profiles, the Subscription and Subscription request profiles,
the ImplementationGuide, and the IG's examples of the create, update, delete
and merge message Bundles, a response MessageHeader and the two
Subscriptions, with the audit records of the two transactions (§2:3.93.5.1,
§2:3.94.5.1): the Feed audit profile and the Consumer's example, and the
Subscription Create, Read and Delete audit profiles and the Subscriber's
examples. The package's other files serve no reader here: the Subscription
Update audit profile and the other BALP audit examples, the related-person
profiles and examples, the
standalone history Bundle and Patient examples, which the message Bundle
examples taken here inline, the OpenAPI and XML renderings, and the registry's
validation output. They are not taken.

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
