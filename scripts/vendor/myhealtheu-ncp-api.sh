#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/myhealtheu-ncp-api.sh
#
# Vendors the MyHealth@EU NCP API package (`myhealth.eu.fhir.ncp-api`), whose
# Patient-minimal profile the PDQm answers are validated against, and fetches
# the packages it and the IHE PDQm package depend on, which the validator
# (`ferrobridge-validator`) needs as a closed set.
#
#   scripts/vendor/myhealtheu-ncp-api.sh            vendor the package tarball
#                                                   under docs/specs/myhealtheu-ncp-api/
#   scripts/vendor/myhealtheu-ncp-api.sh --closure  fetch the dependency closure into
#                                                   the ignored docs/specs/myhealtheu-ncp-api/closure/
#
# The package is a continuous-integration build with no copy in the FHIR
# package registry, so the tarball is committed verbatim and the "MyHealth@EU
# NCP API package" row of docs/VERSIONS.md pins it by URL and sha256. Each
# package of the closure is pinned below by name, version and the sha1 the
# registry records for its tarball; a tarball already on disk with that sha1
# is kept. The closure is about 66 MB and is never committed.
#
# Requires: curl, shasum.
#
# No specification governs this file: our own design.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# shellcheck source=scripts/vendor/lib/corpus.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$root/scripts/vendor/lib/corpus.sh"

corpus_require curl shasum

dest="docs/specs/myhealtheu-ncp-api"
package="$dest/myhealth.eu.fhir.ncp-api-9.1.1-ci.tgz"

# The closure of myhealth.eu.fhir.ncp-api 9.1.1-ci and ihe.iti.pdqm 3.2.0: name,
# version and the registry's sha1 of the tarball, a patch range resolved to
# the highest version the registry published.
closure=(
  "fhir.dicom 2022.4.20221006 a918c40b57c84f462a87aff74fe6c8466e613b3c"
  "hl7.fhir.r4.core 4.0.1 0e4b8d99f7918587557682c8b47df605424547a5"
  "hl7.fhir.uv.extensions.r4 1.0.0 d4f95213cd55173fb5bce87c02eba3ecdace3cb2"
  "hl7.fhir.uv.extensions.r4 5.1.0 d0fdf5fa856701e73d531781bd96ecdee2abaef6"
  "hl7.fhir.uv.extensions.r4 5.2.0 2c7e21436dfa35ba7209696d63247538b851968a"
  "hl7.fhir.uv.extensions.r4 5.3.0 e03679fe759094ed670b637e6e376717659fa03b"
  "hl7.fhir.uv.ips 1.1.0 b034dcab833808c0528123e35f0b85e5cd501e16"
  "hl7.fhir.uv.xver-r5.r4 0.0.1-snapshot-2 cb0588d1bd75915f3e1a75839e97a5a2b77f9c62"
  "hl7.terminology.r4 5.0.0 44cb0600d9483d3a2411a02739edcfdbce8e92c1"
  "hl7.terminology.r4 5.3.0 4f1f0e0dfd6d95a24c69c67bfd32a7625c20f758"
  "hl7.terminology.r4 5.5.0 cb4fa252e5d3f0ebe1b5d3858a160a6954714a39"
  "hl7.terminology.r4 6.5.0 8a7a096866b9b6e96e288ce8d72bf99aa31714a3"
  "hl7.terminology.r4 7.1.0 19a681d8bd598b5c0da72e058616d78ae70940c0"
  "hl7.terminology.r4 7.3.0 2516ac47dfe833a6e75d754943346d004d27918a"
  "ihe.formatcode.fhir 1.2.0 a7a236abf5bf8745acfdab734fa02dacff2650ac"
  "ihe.iti.balp 1.1.4 8af15f01244ab5e3a48dd40e1afb0696d6905898"
  "ihe.iti.mhd 4.2.2 b57a093cf0c1dc6a15c15fc8eaafdc9312364203"
  "ihe.iti.pdqm 3.2.0 f728e969afa2a622c34b35e4f3f65e155a540a0c"
  "myhealth.eu.fhir.mvc-package 9.1.0 bbca87103ffc10240615f8776993d98d6005aa88"
)

fetch_closure() {
  local into="$dest/closure" entry name version want file got
  mkdir -p "$into"
  for entry in "${closure[@]}"; do
    read -r name version want <<< "$entry"
    file="$into/$name#$version.tgz"
    if [ -f "$file" ] && [ "$(shasum -a 1 "$file" | cut -d' ' -f1)" = "$want" ]; then
      continue
    fi
    corpus_download "https://packages.fhir.org/$name/$version" "$file.part" \
      || die "download of $name#$version failed"
    got="$(shasum -a 1 "$file.part" | cut -d' ' -f1)"
    [ "$got" = "$want" ] || die "$name#$version has sha1 $got, the pin records $want"
    mv "$file.part" "$file"
    say "fetched $name#$version"
  done
  say "the closure holds ${#closure[@]} packages under $into"
}

vendor_package() {
  local scratch
  scratch="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand $scratch now
  trap "rm -r -- '$scratch'" EXIT
  corpus_fetch_pinned "MyHealth@EU NCP API package" "$scratch/package.tgz"
  mkdir -p "$dest"
  cp "$scratch/package.tgz" "$package"
  say "vendored $package"
}

case "${1:-}" in
  "") vendor_package ;;
  --closure) fetch_closure ;;
  *) die "usage: myhealtheu-ncp-api.sh [--closure]" ;;
esac
