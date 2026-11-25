#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# The post-quantum inventory gate (#810; no specification governs it: our own
# design, over FerroSYS's inventory format). conformance/post-quantum.tsv
# lists every cryptographic use, five tab-separated columns: use, algorithm,
# safe (yes or no), pq_standard and location, under its `#` comment lines. The gate holds the file's
# shape, every repository path a location names to the tree, the table of
# docs/post-quantum.md to the file, and the shields.io endpoint badge
# conformance/badges/post-quantum.json to the count of safe rows.
#
#   scripts/checks/post-quantum.sh             check; warn on drift
#   scripts/checks/post-quantum.sh --update    rewrite the table and the badge
#   scripts/checks/post-quantum.sh --check     fail on any drift (CI)
#   scripts/checks/post-quantum.sh --self-test prove the rules on fixtures
#
# A row counts as safe when its safe column is yes and its pq_standard names
# a standard; a `no standard yet` row counts as not safe. The badge colour
# follows the shared rule: green when every row is safe, yellow when some
# are, red when none are. Its label never claims a certification.
#
# Needs jq. Exit 0 when clean, 1 on a defect or (under --check) drift, 2 on a
# usage error.
set -euo pipefail

# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$(dirname "$0")/../lib/guard-args.sh"
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$(dirname "$0")/badge-logos/logo.sh"

readonly GRAMMAR="[--update | --check | --self-test]"
readonly INVENTORY=conformance/post-quantum.tsv
readonly PAGE=docs/post-quantum.md
readonly BADGE=conformance/badges/post-quantum.json
readonly HEADER=$'use\talgorithm\tsafe\tpq_standard\tlocation'
readonly LABEL="post-quantum safe uses"
readonly NO_STANDARD="no standard yet"

# Prints each defect of inventory $1 under the tree $2; status 1 when any.
shape_of() {
  local inventory="$1" tree="$2" failed=0 line=0 use algorithm safe standard location extra item path rest
  if [[ "$(grep -v '^#' "$inventory" | head -n 1)" != "$HEADER" ]]; then
    echo "post-quantum: $inventory does not open with the header line" >&2
    failed=1
  fi
  while IFS=$'\t' read -r use algorithm safe standard location extra; do
    line=$((line + 1))
    [[ "$line" -eq 1 ]] && continue
    if [[ -z "$use" || -z "$algorithm" || -z "$standard" || -z "$location" || -n "$extra" ]]; then
      echo "post-quantum: $inventory line $line does not hold five filled columns" >&2
      failed=1
      continue
    fi
    if [[ "$safe" != yes && "$safe" != no ]]; then
      echo "post-quantum: $inventory line $line says safe '$safe'; it is yes or no" >&2
      failed=1
    fi
    rest="$location"
    while [[ -n "$rest" ]]; do
      item="${rest%%, *}"
      if [[ "$rest" == *", "* ]]; then rest="${rest#*, }"; else rest=""; fi
      path="${item%% (*}"
      if [[ "$path" == */* ]] && [[ ! -e "$tree/$path" ]]; then
        echo "post-quantum: $inventory line $line names $path, which the tree does not hold" >&2
        failed=1
      fi
    done
  done < <(grep -v '^#' "$inventory")
  if [[ "$line" -lt 2 ]]; then
    echo "post-quantum: $inventory lists no use" >&2
    failed=1
  fi
  return "$failed"
}

# The rows of inventory $1 that count as safe, and all its rows: "N M".
count_of() {
  awk -F'\t' -v none="$NO_STANDARD" '
    /^#/ { next }
    seen++ { total++; if ($3 == "yes" && $4 != none) safe++ }
    END { printf "%d %d\n", safe, total }
  ' "$1"
}

# The badge JSON inventory $1 implies.
badge_of() {
  local safe total
  read -r safe total < <(count_of "$1")
  jq -cn --arg label "$LABEL" --arg message "$safe / $total" \
    --arg color "$(badge_count_color "$safe" "$total")" --argjson logo "$(badge_logo_fields key)" \
    '{schemaVersion: 1, label: $label, message: $message, color: $color} + $logo'
}

# The markdown table inventory $1 implies.
table_of() {
  echo '| Use | Algorithm | Safe | Post-quantum standard | Location |'
  echo '|---|---|---|---|---|'
  awk -F'\t' '/^#/ { next } seen++ { printf "| %s | %s | %s | %s | `%s` |\n", $1, $2, $3, $4, $5 }' "$1"
}

# The table of page $1: the lines from its header row to the first line that
# is no table row.
page_table() {
  awk '/^\| Use \| Algorithm \|/ { inside = 1 } inside && !/^\|/ { exit } inside { print }' "$1"
}

# Fails when the label $1 claims a certification.
label_claims() {
  if grep -qiE 'certified|compliant|conformant' <<<"$1"; then
    echo "post-quantum: the badge label '$1' claims a certification" >&2
    return 1
  fi
}

# Rewrites the table of page $1 from inventory $2 under update, else
# compares; status 1 under check when they disagree.
emit_table() {
  local page="$1" inventory="$2" mode="$3" table rendered
  table="$(table_of "$inventory")"
  if [[ -z "$(page_table "$page")" ]]; then
    echo "post-quantum: $page has no inventory table" >&2
    return 1
  fi
  if [[ "$mode" = update ]]; then
    rendered="$(mktemp)"
    printf '%s\n' "$table" >"$rendered"
    awk -v table="$rendered" '
      /^\| Use \| Algorithm \|/ && !done { while ((getline line < table) > 0) print line; skipping = 1; done = 1; next }
      skipping && /^\|/ { next }
      { skipping = 0; print }
    ' "$page" >"$rendered.page"
    cat "$rendered.page" >"$page"
    rm -f "$rendered" "$rendered.page"
  elif [[ "$(page_table "$page")" != "$table" ]]; then
    echo "post-quantum: the $page table disagrees with $inventory; run $0 --update"
    if [[ "$mode" = check ]]; then return 1; fi
  fi
  return 0
}

# Writes badge file $1 from inventory $2 under update, else compares; status
# 1 under check when they disagree.
emit_badge() {
  local badge="$1" inventory="$2" mode="$3" json
  json="$(badge_of "$inventory")"
  if [[ "$mode" = update ]]; then
    mkdir -p "$(dirname "$badge")"
    printf '%s\n' "$json" >"$badge"
  elif [[ ! -f "$badge" ]] || [[ "$(cat "$badge")" != "$json" ]]; then
    echo "post-quantum: $badge disagrees with $inventory; run $0 --update"
    if [[ "$mode" = check ]]; then return 1; fi
  fi
  return 0
}

self_test() {
  local work failed=0
  work="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand $work now: the local is gone at EXIT.
  trap "rm -rf '$work'" EXIT
  # expect WANT NAME COMMAND...: COMMAND exits WANT.
  expect() {
    local want="$1" name="$2" status=0
    shift 2
    "$@" >/dev/null 2>&1 || status=$?
    if [[ "$status" -ne "$want" ]]; then
      echo "post-quantum: self-test failed: $name exited $status, wanted $want" >&2
      failed=1
    fi
  }
  mkdir -p "$work/tree/src"
  : >"$work/tree/src/kx.rs"
  {
    printf '# SPDX-FileCopyrightText: Cadasto B.V.\n'
    printf '%s\n' "$HEADER"
    printf 'Key exchange\tX25519MLKEM768\tyes\tRFC 10024\tsrc/kx.rs (provider), a-crate tls\n'
    printf 'Record protection\tAES-256-GCM\tyes\tNIST IR 8547 4.1.3\ta-crate tls\n'
    printf 'Certificates\tECDSA\tyes\t%s\ta-crate tls\n' "$NO_STANDARD"
    printf 'Tokens\tES256\tno\tRFC 9964\ta-crate resource\n'
  } >"$work/inventory.tsv"
  expect 0 "a well formed inventory" shape_of "$work/inventory.tsv" "$work/tree"
  sed 's#src/kx.rs#src/gone.rs#' "$work/inventory.tsv" >"$work/missing.tsv"
  expect 1 "a location the tree does not hold" shape_of "$work/missing.tsv" "$work/tree"
  sed $'s#\tno\t#\tmaybe\t#' "$work/inventory.tsv" >"$work/maybe.tsv"
  expect 1 "a safe column that is neither yes nor no" shape_of "$work/maybe.tsv" "$work/tree"
  sed $'$s#\ta-crate resource$##' "$work/inventory.tsv" >"$work/short.tsv"
  expect 1 "a row of four columns" shape_of "$work/short.tsv" "$work/tree"
  sed '2s#^use#purpose#' "$work/inventory.tsv" >"$work/header.tsv"
  expect 1 "a file without the header" shape_of "$work/header.tsv" "$work/tree"
  if [[ "$(count_of "$work/inventory.tsv")" != "2 4" ]]; then
    echo "post-quantum: self-test failed: a 'no standard yet' row counted as safe" >&2
    failed=1
  fi
  local badge
  badge="$(badge_of "$work/inventory.tsv")"
  if [[ "$(jq -c 'del(.logoSvg)' <<<"$badge")" != "{\"schemaVersion\":1,\"label\":\"$LABEL\",\"message\":\"2 / 4\",\"color\":\"yellow\"}" ]]; then
    echo "post-quantum: self-test failed: the badge of two safe rows of four is $badge" >&2
    failed=1
  fi
  printf '%s\n' "$badge" >"$work/badge.json"
  expect 0 "the badge carries a logo" badge_logo_check "$work/badge.json"
  expect 0 "the badge colour follows its count" badge_color_check "$work/badge.json"
  printf '%s\n' "$HEADER" >"$work/none.tsv"
  awk -F'\t' '$3 == "no"' "$work/inventory.tsv" >>"$work/none.tsv"
  if [[ "$(badge_of "$work/none.tsv" | jq -r .color)" != red ]]; then
    echo "post-quantum: self-test failed: an inventory with no safe row is not red" >&2
    failed=1
  fi
  printf '%s\n' "$HEADER" >"$work/all.tsv"
  sed -n '3p' "$work/inventory.tsv" >>"$work/all.tsv"
  if [[ "$(badge_of "$work/all.tsv" | jq -r .color)" != brightgreen ]]; then
    echo "post-quantum: self-test failed: an inventory of safe rows only is not green" >&2
    failed=1
  fi
  expect 0 "the label" label_claims "$LABEL"
  expect 1 "a label that claims a certification" label_claims "post-quantum certified"
  expect 1 "a missing badge under check" emit_badge "$work/badges/pq.json" "$work/inventory.tsv" check
  expect 0 "a badge written" emit_badge "$work/badges/pq.json" "$work/inventory.tsv" update
  expect 0 "a written badge under check" emit_badge "$work/badges/pq.json" "$work/inventory.tsv" check
  printf '# Page\n\nintro\n\n| Use | Algorithm | Safe | Post-quantum standard | Location |\n|---|---|---|---|---|\n| stale | x | no | y | z |\n\nrest\n' >"$work/page.md"
  expect 1 "a stale table under check" emit_table "$work/page.md" "$work/inventory.tsv" check
  expect 0 "a stale table under compare" emit_table "$work/page.md" "$work/inventory.tsv" compare
  expect 0 "the table rewritten" emit_table "$work/page.md" "$work/inventory.tsv" update
  expect 0 "a rewritten table under check" emit_table "$work/page.md" "$work/inventory.tsv" check
  if [[ "$(head -n 3 "$work/page.md" | tail -n 1)" != intro || "$(tail -n 1 "$work/page.md")" != rest ]]; then
    echo "post-quantum: self-test failed: the rewrite touched text outside the table" >&2
    failed=1
  fi
  printf '# Page\n' >"$work/bare.md"
  expect 1 "a page without a table" emit_table "$work/bare.md" "$work/inventory.tsv" check
  if [[ "$failed" -eq 0 ]]; then
    echo "post-quantum: self-test OK"
  fi
  return "$failed"
}

mode=compare
case "${1-}" in
  "") ;;
  --update) mode=update ;;
  --check) mode=check ;;
  --self-test)
    [[ "$#" -eq 1 ]] || guard_usage "$GRAMMAR"
    self_test
    exit $?
    ;;
  *) guard_usage "$GRAMMAR" ;;
esac
[[ "$#" -le 1 ]] || guard_usage "$GRAMMAR"
cd "$(dirname "$0")/../.."

failed=0
label_claims "$LABEL" || failed=1
shape_of "$INVENTORY" . || failed=1
emit_table "$PAGE" "$INVENTORY" "$mode" || failed=1
emit_badge "$BADGE" "$INVENTORY" "$mode" || failed=1
if [[ "$failed" -eq 0 ]]; then
  read -r safe total < <(count_of "$INVENTORY")
  echo "post-quantum: $safe of $total uses safe"
fi
exit "$failed"
