#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/gh/labels.sh: bootstrap the FerroPIX issue-label taxonomy.
#
# Creates (idempotently) the labels the tracker workflow assumes:
#   * the WORK-KIND labels a Task carries to pick its commit type
#     (documentation/chore/refactor/perf/test/ci); a Bug and a Feature carry
#     none, because the native issue type already says fix or feat;
#   * the DOMAIN labels, one per conformance surface;
#   * a few workflow labels.
# The type of an issue (Bug, Feature, Task) and its priority (Urgent, High,
# Medium, Low) are NOT labels since 2026-10-08: they are GitHub's native issue
# type and the organisation's Priority issue field, set with
# scripts/gh/fields.sh. The six labels that carried them before (bug,
# enhancement, P0 to P3) are deleted here, with `accessibility`, a label for a
# user interface FerroPIX does not have, so a re-run converges a repository
# that still has them.
#
# `gh label create --force` updates an existing label in place, so re-running
# this is safe and converges the colours and descriptions to the values below.
#
# Taxonomy policy: no specification governs it: our own design.
# Official docs: https://cli.github.com/manual/gh_label_create
#
# Usage:
#   scripts/gh/labels.sh
#   scripts/gh/labels.sh --self-test
#       Drives this program against a stub gh on PATH: every label is created
#       with --force, a retired name is deleted only when the repository
#       carries it, a refused gh call fails the run, and an unknown argument
#       touches nothing.
#   Any other argument, --help included, prints this usage and exits 2
#   before any gh call.

set -euo pipefail

die() {
  echo "gh-labels: $*" >&2
  exit 1
}

usage() {
  sed -n '/^# Usage:/,/^$/p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

# The self-test stands before the preflight below, because it answers every
# gh call from a stub on PATH and the real `gh repo view` refuses a runner
# that holds no token for this repository.
self_test() {
  command -v jq >/dev/null 2>&1 || die "jq is not installed"
  local work stub calls
  work="$(mktemp -d)"
  stub="$work/bin"
  calls="$work/calls"
  mkdir -p "$stub"
  cat > "$stub/gh" <<'STUB'
#!/usr/bin/env bash
# The gh a self-test run of scripts/gh/labels.sh speaks to. It writes every
# call where the test reads it, applies a --jq filter when one is passed, and
# lists the labels the run is told the repository carries.
set -euo pipefail
filter=""
previous=""
for argument in "$@"; do
  if [[ "$previous" = "--jq" ]]; then
    filter="$argument"
  fi
  previous="$argument"
done
emit() {
  if [[ -n "$filter" ]]; then
    printf '%s' "$1" | jq -r "$filter"
  else
    printf '%s\n' "$1"
  fi
}
printf '%s\n' "$*" >> "$GH_STUB_CALLS"
case "${1:-} ${2:-}" in
  "repo view")
    emit '{"nameWithOwner":"Example-Org/Example"}'
    ;;
  "label list")
    emit "$GH_STUB_LABELS"
    ;;
  "label create")
    if [[ "${GH_STUB_CREATE:-ok}" != "ok" ]]; then
      echo "stub: gh label create refused" >&2
      exit 1
    fi
    ;;
  "label delete") ;;
  *)
    echo "stub: an unexpected call: $*" >&2
    exit 90
    ;;
esac
STUB
  chmod 0755 "$stub/gh"

  local labels create rc out err
  # run NAME CODE [ARG...]: this program through the stub with ARGs, landing
  # on CODE.
  run() {
    local name=$1 code=$2
    shift 2
    rc=0
    : > "$calls"
    PATH="$stub:$PATH" GH_STUB_CALLS="$calls" \
      GH_STUB_LABELS="$labels" GH_STUB_CREATE="$create" \
      bash "$0" "$@" > "$work/out" 2> "$work/err" || rc=$?
    out="$work/out"
    err="$work/err"
    if [[ "$rc" -ne "$code" ]]; then
      echo "gh-labels: self-test failed: $name exited $rc, wanted $code." >&2
      cat "$work/out" "$work/err" >&2
      exit 1
    fi
  }
  # said NAME FILE NEEDLE: the case left that sentence where it belongs.
  said() {
    local name=$1 file=$2 needle=$3
    if ! grep -qF -- "$needle" "$file"; then
      echo "gh-labels: self-test failed: $name did not say '$needle'." >&2
      cat "$work/out" "$work/err" "$calls" >&2
      exit 1
    fi
  }
  # never NAME FILE NEEDLE: and never that one.
  never() {
    local name=$1 file=$2 needle=$3
    if grep -qF -- "$needle" "$file"; then
      echo "gh-labels: self-test failed: $name said '$needle'." >&2
      cat "$work/out" "$work/err" "$calls" >&2
      exit 1
    fi
  }

  # untouched NAME: the case made no gh call at all.
  untouched() {
    if [[ -s "$calls" ]]; then
      echo "gh-labels: self-test failed: $1 called gh." >&2
      cat "$work/out" "$work/err" "$calls" >&2
      exit 1
    fi
  }

  # A repository that carries none of the retired names: nothing is deleted,
  # and every label of the taxonomy is created in place with --force.
  labels='[{"name":"documentation"}]'
  create=ok
  local clean_case="a repository without the retired labels"
  run "$clean_case" 0
  said "$clean_case" "$out" "ok: documentation"
  said "$clean_case" "$out" "ok: spec:PIXm"
  said "$clean_case" "$out" "ok: upstream-report"
  said "$clean_case" "$out" "ok: no-crate-bump"
  said "$clean_case" "$calls" "label create spec:HL7v2 --color 0e8a16"
  said "$clean_case" "$calls" "label create documentation --color 0075ca"
  said "$clean_case" "$calls" "--force"
  never "$clean_case" "$out" "retired:"
  never "$clean_case" "$calls" "label delete"
  never "$clean_case" "$calls" "label create bug"
  never "$clean_case" "$calls" "label create P1"

  # The same repository with two of the six the native issue type and the
  # Priority field replaced on 2026-10-08: both go, and no other name does.
  labels='[{"name":"enhancement"},{"name":"P1"},{"name":"documentation"}]'
  local retired_case="a repository that still carries two retired labels"
  run "$retired_case" 0
  said "$retired_case" "$out" "retired: enhancement"
  said "$retired_case" "$out" "retired: P1"
  said "$retired_case" "$calls" "label delete enhancement --yes"
  said "$retired_case" "$calls" "label delete P1 --yes"
  never "$retired_case" "$out" "retired: P0"
  never "$retired_case" "$calls" "label delete documentation"

  # A gh that refuses the first create stops the run rather than reporting a
  # taxonomy it did not write.
  create=fail
  run "a refused gh label create" 1
  never "a refused gh label create" "$out" "done."

  # An argument the program does not know, --help among them, prints the
  # usage and exits 2 before a single gh call.
  create=ok
  for argument in --help -h --bogus help; do
    run "the argument $argument" 2 "$argument"
    said "the argument $argument" "$err" "Usage:"
    never "the argument $argument" "$out" "ok: "
    untouched "the argument $argument"
  done
  run "a second argument after --self-test" 2 --self-test --bogus
  untouched "a second argument after --self-test"

  rm -r "$work"
  echo "gh-labels: self-test OK."
}

case "$#:${1:-}" in
  0:) ;;
  1:--self-test)
    self_test
    exit 0
    ;;
  *) usage ;;
esac

command -v gh >/dev/null 2>&1 || die "the GitHub CLI (gh) is not installed"
command -v jq >/dev/null 2>&1 || die "jq is not installed"
gh repo view --json nameWithOwner --jq .nameWithOwner >/dev/null 2>&1 ||
  die "could not resolve the current repository (run inside a gh-authenticated clone)"

# The labels the repository carries today, read once, so no gh call writes
# into a pipe grep has already stopped reading under `set -o pipefail`.
EXISTING="$(gh label list --limit 200 --json name --jq '.[].name')"

# label <name> <hex-color> <description>
label() {
  local name="$1" color="$2" description="$3"
  gh label create "$name" --color "$color" --description "$description" --force >/dev/null
  echo "ok: $name"
}

# retire <name>: delete a label the taxonomy no longer carries, if it exists.
retire() {
  local name="$1"
  if grep -qx -- "$name" <<<"$EXISTING"; then
    gh label delete "$name" --yes >/dev/null
    echo "retired: $name"
  fi
  return 0
}

echo "== retired labels (the native issue type and the Priority field carry these now) =="
for l in bug enhancement P0 P1 P2 P3 accessibility; do retire "$l"; done

echo "== work-kind labels (on a Task only; picks the conventional-commit type) =="
label documentation 0075ca "Docs-only work. Maps to a docs/ branch and a docs: commit."
label chore         fef2c0 "Maintenance with no product change. chore/ and chore:."
label refactor      cfd3d7 "Behaviour-preserving code change. refactor/ and refactor:."
label perf          fbca04 "Performance work. perf/ and perf:."
label test          c2e0c6 "Test-only work. test/ and test:."
label ci            ededed "CI, build, and tooling. ci/ and ci:."

echo "== domain labels (one per conformance surface) =="
label spec:IHE         1d76db "The IHE ITI Technical Framework: profiles, actors and transactions."
label spec:PIXm        5319e7 "IHE PIXm: the FHIR identifier cross-reference query (ITI-83, ITI-104)."
label spec:PDQm        006b75 "IHE PDQm: the FHIR patient demographics query and match (ITI-78, ITI-119)."
label spec:FHIR        b60205 "The HL7 FHIR R4 wire: resource shapes and REST interactions."
label spec:HL7v2       0e8a16 "The HL7 v2 PIX and PDQ transactions (ITI-8, ITI-9, ITI-10, ITI-21)."
label cross-reference  fbca04 "The patient identity cross-reference and its linking."
label pseudonym        d93f0b "The pseudonymous identifiers minted for the clinical domain."

echo "== workflow and meta labels =="
label research         c5def5 "A design-phase investigation; the deliverable is cited evidence."
label dependencies     0366d6 "Dependency updates (used by Dependabot)."
label security         ee0701 "Security fix or hardening."
label blocked-upstream 6f42c1 "Waiting on an upstream specification or tool release."
label upstream-report  990000 "An outbound report of a defect in a published specification."
label no-changelog     bfdadc "PR escape hatch: the change has no user-visible effect."
label no-crate-bump    bfdadc "PR escape hatch: the diff provably does not alter packaged crate bytes."

echo "done."
