#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# The conformance gate (#162; no specification governs it: our own design,
# the model of FerroBRIDGE's gate). It runs the corpus tests of ihe-iti, which
# read and write every example of the vendored PIXm, PDQm and PMIR packages,
# and, behind FERROPIX_E2E over PostgreSQL, the server's EHDS corpora (#786):
# the PDQm answers validated against MyHealth@EU Patient-minimal, the EHDS
# identifier types and the Xt-EHR D8.2 patient match assertions, after it
# fetches the validator's package closure,
# compares what they measured with the committed pass lists under
# conformance/, renders one shields.io endpoint badge per corpus
# (https://shields.io/badges/endpoint-badge) under conformance/badges/, and
# renders the README block between badges:begin and badges:end from them.
#
#   scripts/checks/conformance.sh             run and compare; warn on drift
#   scripts/checks/conformance.sh --update    run and rewrite the lists, the
#                                             badges and the README block
#   scripts/checks/conformance.sh --check     run and fail on any drift (CI)
#   scripts/checks/conformance.sh --self-test prove the comparison, the badges
#                                             and the block on fixtures
#
# A pass list holds one example per line: a passing example is its path,
# followed by `: <how>` when the pass is a refusal the specification
# requires, a failing one is `fail <path>: <reason>`, and the last line is
# the total. The corpus tests write target/conformance/<corpus>.txt in that
# form, and the pass lists themselves when FERROPIX_CONFORMANCE_UPDATE is 1,
# so a list comes from the same test the suite runs. An example the list records as
# passing that no longer passes fails every mode. Under --check, a passing
# example the list does not record, a moved total, a changed reason, a badge
# that disagrees with its list and a README block that disagrees with the
# badges fail too.
#
# The small caption under the README's conformance heading says once what the
# badges count (owner decision, 2026-11-25); a README without it fails. A
# badge's colour follows its count: green when all pass, yellow when some do,
# red when none do.
#
# Every badge, and every static badge in the README, carries a logo from
# scripts/checks/badge-logos/: one without fails every mode.
#
# A badge label never claims a certification: one holding "certified",
# "compliant" or "conformant" fails every mode, and a corpus with no pass list
# and no result is left out of the README.
#
# Needs cargo, cargo-nextest and Docker (the self-test needs none). Exit 0 when
# clean, 1 on a regression or (under --check) drift, 2 on a usage error or a
# missing result.
set -euo pipefail

# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$(dirname "$0")/../lib/guard-args.sh"
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$(dirname "$0")/badge-logos/logo.sh"

readonly GRAMMAR="[--update | --check | --self-test]"
readonly CORPORA=(pixm pdqm pmir audit)
# The server's corpora, the README's EHDS row.
readonly EHDS_CORPORA=(ncp-patient-minimal ehds-identifier-types xtehr-patient-match)
readonly BADGES=conformance/badges
# The shields.io endpoint prefix every conformance badge file is read through.
readonly ENDPOINT='https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2FFerroHEALTH%2FFerroPIX%2Fmain%2Fconformance%2Fbadges%2F'
# The project and quality badges, the first two rows of the README block.
readonly PROJECT_BADGES=(
  '[![Release](https://img.shields.io/github/v/release/FerroHEALTH/FerroPIX?sort=semver&logo=github&label=release)](https://github.com/FerroHEALTH/FerroPIX/releases/latest)'
  '[![CI](https://img.shields.io/github/actions/workflow/status/FerroHEALTH/FerroPIX/ci.yml?branch=main&logo=githubactions&logoColor=white&label=CI)](https://github.com/FerroHEALTH/FerroPIX/actions/workflows/ci.yml)'
  '[![License: BUSL-1.1](https://img.shields.io/badge/License-BUSL--1.1-blue?logo=data%3Aimage%2Fsvg%2Bxml%3Bbase64%2CPHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCI%2BPHBhdGggZmlsbD0ibm9uZSIgc3Ryb2tlPSIjZmZmIiBzdHJva2Utd2lkdGg9IjIiIHN0cm9rZS1saW5lY2FwPSJyb3VuZCIgc3Ryb2tlLWxpbmVqb2luPSJyb3VuZCIgZD0iTTEyIDN2MThNNyAyMWgxME00IDdoMTZNNCA3TDEgMTRhMyAzIDAgMCAwIDYgMFpNMjAgN2wtMyA3YTMgMyAwIDAgMCA2IDBaIi8%2BPC9zdmc%2B)](LICENSE)'
  '[![Docs](https://img.shields.io/badge/docs-ferropix.eu-6B21A8?logo=mdbook)](https://ferropix.eu/docs/)'
  '[![GHCR](https://img.shields.io/badge/ghcr.io-ferropix-2496ED?logo=docker&logoColor=white)](https://github.com/FerroHEALTH/FerroPIX/pkgs/container/ferropix)'
  '[![Image pulls](https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Fghcr-badge.elias.eu.org%2Fapi%2FFerroHEALTH%2FFerroPIX%2Fferropix&query=downloadCount&label=image%20pulls&logo=github)](https://github.com/FerroHEALTH/FerroPIX/pkgs/container/ferropix)'
)
readonly QUALITY_BADGES=(
  '[![OpenSSF Scorecard](https://img.shields.io/ossf-scorecard/github.com/FerroHEALTH/FerroPIX?label=openssf%20scorecard&logo=data%3Aimage%2Fsvg%2Bxml%3Bbase64%2CPHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCI%2BPHBhdGggZmlsbD0ibm9uZSIgc3Ryb2tlPSIjZmZmIiBzdHJva2Utd2lkdGg9IjIiIHN0cm9rZS1saW5lY2FwPSJyb3VuZCIgc3Ryb2tlLWxpbmVqb2luPSJyb3VuZCIgZD0iTTEyIDJsOCAzdjZjMCA1LTMuNSA5LjUtOCAxMS00LjUtMS41LTgtNi04LTExVjVaTTguNSAxMmwyLjUgMi41IDQuNS01Ii8%2BPC9zdmc%2B)](https://scorecard.dev/viewer/?uri=github.com/FerroHEALTH/FerroPIX)'
  '[![Quality gate](https://img.shields.io/sonar/quality_gate/FerroHEALTH_FerroPIX?server=https%3A%2F%2Fsonarcloud.io&logo=sonarqubecloud&label=quality%20gate)](https://sonarcloud.io/summary/overall?id=FerroHEALTH_FerroPIX)'
  '[![Coverage](https://img.shields.io/sonar/coverage/FerroHEALTH_FerroPIX?server=https%3A%2F%2Fsonarcloud.io&logo=sonarqubecloud&label=coverage)](https://sonarcloud.io/summary/new_code?id=FerroHEALTH_FerroPIX)'
  '[![CodeQL](https://img.shields.io/github/actions/workflow/status/FerroHEALTH/FerroPIX/codeql.yml?branch=main&logo=github&label=CodeQL)](https://github.com/FerroHEALTH/FerroPIX/actions/workflows/codeql.yml)'
  '[![Signed releases](https://img.shields.io/badge/releases-signed-brightgreen?logo=github)](https://ferropix.eu/docs/operate/deployment.html#verifying-an-image)'
  '[![SLSA Build L3](https://slsa.dev/images/gh-badge-level3.svg)](docs/release.md#the-build-legs)'
  '[![Post-quantum safe uses](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2FFerroHEALTH%2FFerroPIX%2Fmain%2Fconformance%2Fbadges%2Fpost-quantum.json)](docs/post-quantum.md)'
)

# The badge label of a corpus.
label_of() {
  case "$1" in
    pixm) echo "PIXm 3.1.0 examples" ;;
    pdqm) echo "PDQm 3.2.0 examples" ;;
    pmir) echo "PMIR 1.6.0 examples" ;;
    audit) echo "Audit record examples" ;;
    ncp-patient-minimal) echo "MyHealth@EU NCP API 9.1.1-ci Patient-minimal valid" ;;
    ehds-identifier-types) echo "EHDS identifier types (EEHRxF draft Annex X)" ;;
    xtehr-patient-match) echo "Xt-EHR D8.2 2026-05-19 Patient Match assertions" ;;
    *) return 1 ;;
  esac
}

# The badge logo of a corpus: the EU emblem for an EU act, else a pictogram
# of the FerroHEALTH badge set.
logo_of() {
  case "$1" in
    pixm | pmir) echo id-card ;;
    pdqm) echo magnifier ;;
    audit) echo log ;;
    ncp-patient-minimal | ehds-identifier-types | xtehr-patient-match) echo eu ;;
    *) return 1 ;;
  esac
}

# Fails when the label $1 claims a certification.
label_claims() {
  if grep -qiE 'certified|compliant|conformant' <<<"$1"; then
    echo "conformance: the badge label '$1' claims a certification; name the version and the count" >&2
    return 1
  fi
}

# The badge colour of $1 passing out of $2, by the shared rule.
colour_of() {
  badge_count_color "$1" "$2"
}

# The passing examples of the pass list $1, sorted.
passes_of() {
  local list="$1"
  grep -v -e '^#' -e '^fail ' -e '^total ' -e '^$' "$list" | LC_ALL=C sort || true
}

# The total the pass list $1 records, or nothing.
total_of() {
  local list="$1"
  sed -n 's/^total \([0-9][0-9]*\)$/\1/p' "$list"
}

# The badge JSON of corpus $1 that the pass list $2 implies.
badge_of() {
  local corpus="$1" list="$2" passed total logo
  passed="$(passes_of "$list" | sed '/^$/d' | wc -l | tr -d ' ')"
  total="$(total_of "$list")"
  [[ -n "$total" ]] || { echo "conformance: $list has no total line" >&2; return 2; }
  logo="$(badge_logo_fields "$(logo_of "$corpus")")"
  jq -cn --arg label "$(label_of "$corpus")" --arg message "$passed / $total" \
    --arg color "$(colour_of "$passed" "$total")" --argjson logo "$logo" \
    '{schemaVersion: 1, label: $label, message: $message, color: $color} + $logo'
}

# Compares the result $2 of corpus $1 with its pass list $3 under mode $4 and
# prints what differs; status 1 for a regression or, under check, any drift.
compare_corpus() {
  local corpus="$1" result="$2" list="$3" mode="$4" status=0 regressed new
  regressed="$(LC_ALL=C comm -23 <(passes_of "$list") <(passes_of "$result"))"
  new="$(LC_ALL=C comm -13 <(passes_of "$list") <(passes_of "$result"))"
  echo "conformance: $corpus $(passes_of "$result" | sed '/^$/d' | wc -l | tr -d ' ') / $(total_of "$result")"
  if [[ -n "$regressed" ]]; then
    echo "conformance: $corpus regressed:"
    printf '  %s\n' "$regressed"
    status=1
  fi
  if [[ -n "$new" ]]; then
    echo "conformance: $corpus passes examples its list does not record; run $0 --update:"
    printf '  %s\n' "$new"
    if [[ "$mode" = check ]]; then status=1; fi
  fi
  if [[ -z "$regressed" && -z "$new" ]] && ! cmp -s "$result" "$list"; then
    echo "conformance: $corpus differs from $list in its total or a reason; run $0 --update"
    if [[ "$mode" = check ]]; then status=1; fi
  fi
  return "$status"
}

# The markdown of one corpus badge.
badge_link() {
  local corpus="$1"
  echo "[![$(label_of "$corpus")](${ENDPOINT}${corpus}.json)](conformance/$corpus/pass-list.txt)"
}

# The README block with its markers: the project and quality rows, then the
# conformance row.
render_block() {
  local corpus
  echo '<!-- badges:begin -->'
  echo '**Project**<br>'
  printf '%s\n' "${PROJECT_BADGES[@]}"
  echo
  echo '**Quality and security**<br>'
  printf '%s\n' "${QUALITY_BADGES[@]}"
  echo
  echo '**Conformance**<br>'
  # shellcheck disable=SC2154 # badge-logos/logo.sh sets it
  echo "$badge_caption<br>"
  for corpus in "${CORPORA[@]}"; do
    badge_link "$corpus"
  done
  local ehds=()
  for corpus in "${EHDS_CORPORA[@]}"; do
    [[ -f "conformance/$corpus/pass-list.txt" ]] && ehds+=("$corpus")
  done
  if [[ "${#ehds[@]}" -gt 0 ]]; then
    echo
    echo '**EHDS**<br>'
    for corpus in ${ehds[@]+"${ehds[@]}"}; do
      badge_link "$corpus"
    done
  fi
  echo '<!-- badges:end -->'
}

# Writes badge file $1 as $2 under update, else compares; status 1 under
# check when they disagree.
emit_badge() {
  local badge="$1" json="$2" mode="$3"
  if [[ "$mode" = update ]]; then
    mkdir -p "$(dirname "$badge")"
    printf '%s\n' "$json" >"$badge"
  elif [[ ! -f "$badge" ]] || [[ "$(cat "$badge")" != "$json" ]]; then
    echo "conformance: $badge disagrees with its pass list; run $0 --update"
    if [[ "$mode" = check ]]; then return 1; fi
  fi
  return 0
}

# Rewrites the README block of file $1 under update, else compares; status 1
# under check when it disagrees or the markers are missing.
emit_block() {
  local readme="$1" mode="$2" block rendered
  block="$(render_block)"
  if ! grep -qx '<!-- badges:begin -->' "$readme" || ! grep -qx '<!-- badges:end -->' "$readme"; then
    echo "conformance: $readme has no badges:begin and badges:end markers"
    return 1
  fi
  if [[ "$mode" = update ]]; then
    rendered="$(mktemp)"
    printf '%s\n' "$block" >"$rendered"
    awk -v block="$rendered" '
      $0 == "<!-- badges:begin -->" { while ((getline line < block) > 0) print line; skipping = 1; next }
      $0 == "<!-- badges:end -->" { skipping = 0; next }
      !skipping { print }
    ' "$readme" >"$rendered.readme"
    cat "$rendered.readme" >"$readme"
    rm -f "$rendered" "$rendered.readme"
  elif [[ "$(sed -n '/^<!-- badges:begin -->$/,/^<!-- badges:end -->$/p' "$readme")" != "$block" ]]; then
    echo "conformance: the $readme badge block disagrees with the badge files; run $0 --update"
    if [[ "$mode" = check ]]; then return 1; fi
  fi
  return 0
}

# The self-test: the comparison, the badges and the README block over fixture
# lists in a temporary directory, with no cargo.
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
      echo "conformance: self-test failed: $name exited $status, wanted $want" >&2
      failed=1
    fi
  }
  printf '# head\nexample/a.json\nexample/b.json\nfail example/c.json: why\ntotal 3\n' >"$work/list"
  cp "$work/list" "$work/same"
  printf '# head\nexample/a.json\nfail example/b.json: broke\nfail example/c.json: why\ntotal 3\n' >"$work/regressed"
  printf '# head\nexample/a.json\nexample/b.json\nexample/c.json\ntotal 3\n' >"$work/new"
  printf '# head\nexample/a.json\nexample/b.json\nfail example/c.json: another reason\ntotal 3\n' >"$work/reason"
  expect 0 "an unchanged result under check" compare_corpus pixm "$work/same" "$work/list" check
  expect 1 "a regression under compare" compare_corpus pixm "$work/regressed" "$work/list" compare
  expect 1 "a regression under check" compare_corpus pixm "$work/regressed" "$work/list" check
  expect 0 "a new pass under compare" compare_corpus pixm "$work/new" "$work/list" compare
  expect 1 "a new pass under check" compare_corpus pixm "$work/new" "$work/list" check
  expect 1 "a changed reason under check" compare_corpus pixm "$work/reason" "$work/list" check
  local badge
  badge="$(badge_of pixm "$work/list")"
  if [[ "$(jq -c 'del(.logoSvg)' <<<"$badge")" != '{"schemaVersion":1,"label":"PIXm 3.1.0 examples","message":"2 / 3","color":"yellow"}' ]]; then
    echo "conformance: self-test failed: the badge of two passes of three is $badge" >&2
    failed=1
  fi
  if [[ "$(jq -r .logoSvg <<<"$badge")" != "<svg "* ]]; then
    echo "conformance: self-test failed: the PIXm badge carries no pictogram" >&2
    failed=1
  fi
  if [[ "$(badge_of xtehr-patient-match "$work/list" | jq -r '.namedLogo + .labelColor')" != 'europeanunion#003399' ]]; then
    echo "conformance: self-test failed: the Xt-EHR badge carries no EU emblem on EU blue" >&2
    failed=1
  fi
  mkdir -p "$work/logos"
  printf '%s\n' "$badge" >"$work/logos/pixm.json"
  expect 0 "a badge with a logo" badge_logo_check "$work/logos/pixm.json"
  jq -c 'del(.logoSvg)' <<<"$badge" >"$work/logos/bare.json"
  expect 1 "a badge without a logo" badge_logo_check "$work/logos/bare.json"
  printf '[![x](https://img.shields.io/badge/x-y-blue?logo=github)](x)\n' >"$work/logos/README.md"
  expect 0 "a static badge with a logo" badge_logo_readme "$work/logos/README.md"
  printf '[![x](https://img.shields.io/badge/x-y-blue)](x)\n' >>"$work/logos/README.md"
  expect 1 "a static badge without a logo" badge_logo_readme "$work/logos/README.md"
  printf '**Conformance**<br>\n%s<br>\n' "$badge_caption" >"$work/logos/caption.md"
  expect 0 "a README with the conformance caption" badge_caption_check "$work/logos/caption.md"
  printf '**Conformance**<br>\n' >"$work/logos/caption.md"
  expect 1 "a README without the conformance caption" badge_caption_check "$work/logos/caption.md"
  expect 0 "a counted badge in its colour" badge_color_check "$work/logos/pixm.json"
  jq -c '.color = "blue"' "$work/logos/pixm.json" >"$work/logos/blue.json"
  expect 1 "a counted badge in blue" badge_color_check "$work/logos/blue.json"
  jq -c '.message = "0 / 3" | .color = "red"' "$work/logos/pixm.json" >"$work/logos/red.json"
  expect 0 "a badge with nothing passing in red" badge_color_check "$work/logos/red.json"
  jq -c '.message = "3 / 3"' "$work/logos/pixm.json" >"$work/logos/green.json"
  expect 1 "a badge with every case passing in yellow" badge_color_check "$work/logos/green.json"
  # The colours of the owner decision of 2026-11-25: all, some or none passing.
  for pair in "3 3 brightgreen" "3 4 yellow" "1 2 yellow" "1 4 yellow" "0 5 red" "0 0 red"; do
    read -r passed total colour <<<"$pair"
    if [[ "$(colour_of "$passed" "$total")" != "$colour" ]]; then
      echo "conformance: self-test failed: $passed of $total is not $colour" >&2
      failed=1
    fi
  done
  printf 'intro\n' >"$work/no-markers.md"
  expect 1 "a README without markers" emit_block "$work/no-markers.md" check
  printf 'intro\n<!-- badges:begin -->\nstale\n<!-- badges:end -->\nrest\n' >"$work/README.md"
  expect 1 "a stale block under check" emit_block "$work/README.md" check
  expect 0 "a stale block under compare" emit_block "$work/README.md" compare
  expect 0 "the block rewritten" emit_block "$work/README.md" update
  expect 0 "a rewritten block under check" emit_block "$work/README.md" check
  if [[ "$(head -n 1 "$work/README.md")" != intro || "$(tail -n 1 "$work/README.md")" != rest ]]; then
    echo "conformance: self-test failed: the rewrite touched text outside the block" >&2
    failed=1
  fi
  expect 1 "a label that claims a certification" label_claims "PDQm 3.2.0 certified"
  expect 1 "a label that claims compliance" label_claims "EHDS Compliant"
  expect 1 "a label that claims conformance" label_claims "conformant to PIXm"
  for corpus in "${CORPORA[@]}" "${EHDS_CORPORA[@]}"; do
    expect 0 "the label of $corpus" label_claims "$(label_of "$corpus")"
  done
  expect 1 "a missing badge under check" emit_badge "$work/badges/pixm.json" "$badge" check
  expect 0 "a badge written" emit_badge "$work/badges/pixm.json" "$badge" update
  expect 0 "a written badge under check" emit_badge "$work/badges/pixm.json" "$badge" check
  if [[ "$failed" -eq 0 ]]; then
    echo "conformance: self-test OK"
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

# A missing cargo would otherwise read as every corpus test failing.
if ! command -v cargo >/dev/null 2>&1; then
  echo "conformance: cargo not found on PATH" >&2
  exit 1
fi
cd "$(dirname "$0")/../.."

readonly OUT=target/conformance
rm -rf "$OUT"
export FERROPIX_CONFORMANCE_OUT="$PWD/$OUT"
if [[ "$mode" = update ]]; then
  export FERROPIX_CONFORMANCE_UPDATE=1
else
  export FERROPIX_CONFORMANCE_UPDATE=0
fi

failed=0
for corpus in "${CORPORA[@]}" "${EHDS_CORPORA[@]}"; do
  label_claims "$(label_of "$corpus")" || failed=1
done
if ! cargo nextest run --locked -p ihe-iti --all-features --no-tests=fail --no-fail-fast \
  -E 'test(/^conformance::/)'; then
  echo "conformance: a corpus test failed; an example its list records no longer passes"
  failed=1
fi
bash scripts/vendor/myhealtheu-ncp-api.sh --closure
if ! FERROPIX_E2E=1 cargo nextest run --locked -p ferropix-server --no-tests=fail --no-fail-fast \
  -E 'test(/^conformance::/)'; then
  echo "conformance: a server corpus test failed; a case its list records no longer passes"
  failed=1
fi

for corpus in "${CORPORA[@]}" "${EHDS_CORPORA[@]}"; do
  result="$OUT/$corpus.txt"
  list="conformance/$corpus/pass-list.txt"
  if [[ ! -f "$result" && ! -f "$list" ]] && [[ " ${EHDS_CORPORA[*]} " = *" $corpus "* ]]; then
    continue
  fi
  if [[ ! -f "$result" ]]; then
    echo "conformance: no test reported $corpus ($result is missing)" >&2
    exit 2
  fi
  if [[ ! -f "$list" ]]; then
    echo "conformance: $list is missing; run $0 --update"
    failed=1
    continue
  fi
  compare_corpus "$corpus" "$result" "$list" "$mode" || failed=1
  emit_badge "$BADGES/$corpus.json" "$(badge_of "$corpus" "$list")" "$mode" || failed=1
done
emit_block README.md "$mode" || failed=1
badge_logo_check "$BADGES"/*.json || failed=1
badge_logo_readme README.md || failed=1
badge_caption_check README.md || failed=1
badge_color_check "$BADGES"/*.json || failed=1

exit "$failed"
