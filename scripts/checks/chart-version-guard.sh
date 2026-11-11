#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# A change to the packaged chart moves the chart version (#632).
#
# A published chart version is immutable: build-chart.yml refuses to publish
# a version the registry holds with other files. The packaged content is
# everything under deploy/helm/ferropix/ (Chart.yaml with its appVersion,
# values.yaml, the templates), so a change there moves `version` in
# Chart.yaml, or the release's chart leg fails after everything else has
# shipped, as v0.0.28's did. The value sets, the goldens and the validator
# one level up ship in nothing and need no bump. With NO_CHART_BUMP=true (the
# `no-chart-bump` pull-request label, for a diff that provably changes no
# packaged byte) the rule is waived. No specification governs this: our own
# design, after FerroEHR's chart-version-guard.
#
#   chart-version-guard.sh <base-ref> <head-ref>
#   chart-version-guard.sh --self-test
#
# The diff runs from the merge base of the two refs. Exit 0 when the rule
# holds, 1 when it does not, 2 on a usage error.
set -euo pipefail

# shellcheck source=scripts/lib/guard-args.sh
. "$(dirname "${BASH_SOURCE[0]}")/../lib/guard-args.sh"

readonly CHART=deploy/helm/ferropix

# The `version` of Chart.yaml at the ref $1, empty when it has none.
chart_version() {
  git show "$1:$CHART/Chart.yaml" 2>/dev/null | sed -nE 's/^version:[[:space:]]*"?([^"[:space:]]+)"?.*$/\1/p' | head -n 1
}

# Judges the change from the merge base of $1 and $2 to $2.
guard() {
  local base="$1" head="$2" fork old new
  fork="$(git merge-base "$base" "$head")"
  if ! git diff --name-only "$fork" "$head" -- "$CHART/" | grep -q .; then
    echo "chart-version-guard: no packaged chart content changed."
    return 0
  fi
  if [[ "${NO_CHART_BUMP:-false}" == true ]]; then
    echo "chart-version-guard: ok: no-chart-bump, the packaged chart changes without a version bump."
    return 0
  fi
  old="$(chart_version "$fork")"
  new="$(chart_version "$head")"
  if [[ -n "$old" && "$old" == "$new" ]]; then
    echo "::error::This change touches the packaged chart ($CHART/) but leaves its Chart.yaml version at $new, a version the registry may already hold with other files. Raise the chart version, or apply 'no-chart-bump' when the diff provably changes no packaged byte." >&2
    return 1
  fi
  echo "chart-version-guard: the chart moves from ${old:-none} to $new."
}

# Runs the guard over a scratch repository for each case it must judge.
self_test() {
  local dir failures=0
  dir="$(mktemp -d)"
  trap 'rm -rf "$dir"' RETURN
  (
    cd "$dir"
    git init -q -b main
    git config user.email test@example.invalid
    git config user.name test
    git config commit.gpgsign false
    mkdir -p "$CHART" deploy/helm/ci
    printf 'version: 0.3.20\nappVersion: "0.0.27"\n' > "$CHART/Chart.yaml"
    printf 'image: 0.0.27\n' > "$CHART/values.yaml"
    printf 'set: 1\n' > deploy/helm/ci/set.yaml
    git add -A && git commit -qm base
  )
  # case NAME WANT [NO_CHART_BUMP] -- the edit, run in the scratch tree
  case_of() {
    local name="$1" want="$2" waive="$3" got=0 head
    shift 3
    (cd "$dir" && git checkout -q -B "case" main && "$@" && git add -A && git commit -qm "$name" --allow-empty)
    head="$(git -C "$dir" rev-parse HEAD)"
    (cd "$dir" && NO_CHART_BUMP="$waive" guard main "$head") > /dev/null 2>&1 || got=$?
    if [[ "$got" != "$want" ]]; then
      echo "self-test: $name gave $got, wanted $want" >&2
      failures=$((failures + 1))
    fi
  }
  app_version() { sed -i.bak 's/0.0.27/0.0.28/' "$CHART/Chart.yaml" && rm "$CHART/Chart.yaml.bak"; }
  bumped() { app_version && sed -i.bak 's/0.3.20/0.3.21/' "$CHART/Chart.yaml" && rm "$CHART/Chart.yaml.bak"; }
  values() { printf 'image: 0.0.28\n' > "$CHART/values.yaml"; }
  outside() { printf 'set: 2\n' > deploy/helm/ci/set.yaml; }
  case_of "an appVersion move without a bump is refused" 1 false app_version
  case_of "a values.yaml change without a bump is refused" 1 false values
  case_of "a move with a bump passes" 0 false bumped
  case_of "a change outside the packaged chart passes" 0 false outside
  case_of "no-chart-bump waives the rule" 0 true values
  if ((failures > 0)); then
    echo "chart-version-guard: self-test failed ($failures case(s))" >&2
    return 1
  fi
  echo "chart-version-guard: self-test OK (an appVersion move and a values change refused, a bump, an outside change and the label allowed)."
}

case "${1:-}" in
  --self-test)
    [[ "$#" -eq 1 ]] || guard_usage "<base-ref> <head-ref> | --self-test"
    self_test
    ;;
  *)
    [[ "$#" -eq 2 ]] || guard_usage "<base-ref> <head-ref> | --self-test"
    guard "$1" "$2"
    ;;
esac
