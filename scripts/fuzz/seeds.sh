#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# scripts/fuzz/seeds.sh: writes the generated seed corpora of the fuzz targets
# under fuzz/seeds from the examples of the vendored IHE packages, never from
# hand-copied text (no specification governs this: our own design, after
# FerroFED's script of the same name):
#
#   pix_parameters  every PIXm Query Parameters In example, verbatim
#   pix_query       the same requests as the query of a GET
#   pix_feed        every PIXm Patient example that carries an identifier,
#                   behind the conditional update's identifier=system|value
#                   query and a NUL byte
#   pdq_search      the demographics of every PIXm and PDQm Patient example
#                   as an ITI-78 query, and as a domain-filtered :exact query
#   pdq_match       every PDQm Match Input example, Parameters and bare
#                   Patient, behind an empty query and a NUL byte, and once
#                   with _format in the query
#   format          the media types the IHE CapabilityStatements list, as a
#                   Content-Type, a _format and an Accept separated by NULs
#
# The script owns only the files named gen-*. A regression seed committed after
# a finding carries any other name and is never touched. With --check it
# regenerates into a scratch directory and fails when the committed gen-* files
# differ, so a re-vendored package cannot leave stale seeds behind.
#
# Usage:
#   scripts/fuzz/seeds.sh           # rewrite fuzz/seeds/*/gen-*
#   scripts/fuzz/seeds.sh --check   # exit 1 when they are out of date

set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
pixm="$root/docs/specs/ihe-pixm/package"
pdqm="$root/docs/specs/ihe-pdqm/package"
targets=(pix_parameters pix_query pix_feed pdq_search pdq_match format)

die() {
  echo "fuzz-seeds: $*" >&2
  exit 1
}

command -v jq >/dev/null 2>&1 || die "jq is required"
[[ -d "$pixm/example" ]] || die "no PIXm examples at $pixm (scripts/vendor/ihe-pixm.sh)"
[[ -d "$pdqm/example" ]] || die "no PDQm examples at $pdqm (scripts/vendor/ihe-pdqm.sh)"

# name FILE: the example's file name without its resource type and extension.
name() {
  local base
  base="$(basename "$1" .json)"
  printf '%s' "${base#*-}"
}

generate() {
  local out="$1" file query
  for target in "${targets[@]}"; do
    mkdir -p "$out/$target"
  done

  for file in "$pixm"/example/Parameters-pixm-request-*.json; do
    cp "$file" "$out/pix_parameters/gen-$(name "$file").json"
    jq -j '[.parameter[] | "\(.name)=\(.valueString | @uri)"] | join("&")' "$file" \
      >"$out/pix_query/gen-$(name "$file").txt"
  done

  for file in "$pixm"/example/Patient-*.json; do
    query="$(jq -j '.identifier[0] // empty | "identifier=\("\(.system)|\(.value)" | @uri)"' "$file")"
    [[ -n "$query" ]] || continue
    {
      printf '%s\0' "$query"
      cat "$file"
    } >"$out/pix_feed/gen-$(name "$file").bin"
  done

  local package seed
  for file in "$pixm"/example/Patient-*.json "$pdqm"/example/Patient-*.json; do
    package="${file%/package/example/*}"
    seed="$out/pdq_search/gen-${package##*/ihe-}-$(name "$file")"
    jq -j '[
        (.name[0].family // empty | "family=\(@uri)"),
        (.name[0].given[0] // empty | "given=\(@uri)"),
        (.birthDate // empty | "birthdate=\(@uri)"),
        (.gender // empty | "gender=\(@uri)"),
        (.identifier[0] // empty | "identifier=\("\(.system)|\(.value)" | @uri)"),
        (.telecom[0].value // empty | "telecom=\(@uri)"),
        (.address[0].state // empty | "address-state=\(@uri)")
      ] | join("&")' "$file" >"$seed.txt"
    jq -j '[
        (.name[0].family // empty | "family:exact=\(@uri)"),
        (.identifier[0].system // empty | "identifier=\("\(.)|" | @uri)"),
        "_count=10"
      ] | join("&")' "$file" >"$seed-domain.txt"
  done

  for file in "$pdqm"/example/Parameters-ex-match-input-*.json "$pdqm"/example/Patient-ex-match-input-patient.json; do
    {
      printf '\0'
      cat "$file"
    } >"$out/pdq_match/gen-$(name "$file").bin"
    {
      printf '_format=json\0'
      cat "$file"
    } >"$out/pdq_match/gen-$(name "$file")-format.bin"
  done

  local index=0 media
  while IFS= read -r media; do
    index=$((index + 1))
    printf '%s\0\0%s' "$media" "$media" >"$out/format/gen-$index-accept.bin"
    printf '%s; charset=utf-8\0%s\0*/*;q=0.1' "$media" "$media" >"$out/format/gen-$index-format.bin"
  done < <(jq -r '.format[]?' "$pixm"/CapabilityStatement-*.json "$pdqm"/CapabilityStatement-*.json | sort -u)
  [[ "$index" -gt 0 ]] || die "the CapabilityStatements list no format: the package layout moved"

  for target in "${targets[@]}"; do
    compgen -G "$out/$target/gen-*" >/dev/null || die "no seed for $target: the package layout moved"
  done
}

case "${1:-}" in
  '')
    for target in "${targets[@]}"; do
      mkdir -p "$root/fuzz/seeds/$target"
      find "$root/fuzz/seeds/$target" -maxdepth 1 -type f -name 'gen-*' -delete
    done
    generate "$root/fuzz/seeds"
    echo "fuzz-seeds: wrote fuzz/seeds/*/gen-*"
    ;;
  --check)
    scratch="$(mktemp -d)"
    trap 'find "$scratch" -type f -delete; find "$scratch" -depth -type d -exec rmdir {} +' EXIT
    generate "$scratch"
    stale=0
    for target in "${targets[@]}"; do
      want="$(cd "$scratch/$target" && find . -type f -name 'gen-*' | sort)"
      have="$(cd "$root/fuzz/seeds/$target" 2>/dev/null && find . -type f -name 'gen-*' | sort)" || have=""
      if [[ "$want" != "$have" ]]; then
        echo "fuzz-seeds: the gen-* files of $target differ from the packages" >&2
        stale=1
        continue
      fi
      while IFS= read -r seed; do
        [[ -n "$seed" ]] || continue
        cmp -s "$scratch/$target/$seed" "$root/fuzz/seeds/$target/$seed" || {
          echo "fuzz-seeds: $target/${seed#./} is out of date" >&2
          stale=1
        }
      done <<<"$want"
    done
    [[ "$stale" -eq 0 ]] || die "run scripts/fuzz/seeds.sh and commit the result"
    echo "fuzz-seeds: every generated seed is current"
    ;;
  *)
    die "usage: scripts/fuzz/seeds.sh [--check]"
    ;;
esac
