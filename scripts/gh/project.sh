#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/gh/project.sh: FerroPIX has no roadmap board.
#
# FerroPIX keeps no GitHub Project (v2) board, and this helper never creates
# one. The tracker is the issue list, milestones are the release spine, and
# the milestone page is the public view (.claude/rules/project-board.md). The
# helper stays so the shared skills and habits that call it get a clear
# answer instead of a GraphQL failure.
#
# Policy: no specification governs it: our own design.
#
# Usage:
#   scripts/gh/project.sh <any command> [args...]
#       Prints that this repository has no board and exits 0, touching
#       nothing on GitHub.
#   scripts/gh/project.sh --self-test
#       Proves the stub answers every command without calling gh.
#   No argument or --help prints this usage and exits 2.

set -euo pipefail

usage() {
  sed -n '/^# Usage:/,/^$/p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

self_test() {
  local work rc command
  work="$(mktemp -d)"
  mkdir -p "$work/bin"
  # A gh on PATH that fails the test the moment it is called.
  printf '#!/usr/bin/env bash\necho called >> "%s/calls"\nexit 90\n' "$work" > "$work/bin/gh"
  chmod 0755 "$work/bin/gh"
  for command in status add show board url update updates sync-dates; do
    rc=0
    PATH="$work/bin:$PATH" bash "$0" "$command" 1 in-progress > /dev/null 2> "$work/err" || rc=$?
    if [[ "$rc" -ne 0 ]] || ! grep -qF "FerroPIX has no roadmap board" "$work/err"; then
      echo "gh-project: self-test failed: '$command' exited $rc." >&2
      exit 1
    fi
  done
  for command in "" --help -h; do
    rc=0
    PATH="$work/bin:$PATH" bash "$0" ${command:+"$command"} > /dev/null 2>&1 || rc=$?
    if [[ "$rc" -ne 2 ]]; then
      echo "gh-project: self-test failed: '${command:-<none>}' exited $rc, wanted 2." >&2
      exit 1
    fi
  done
  if [[ -e "$work/calls" ]]; then
    echo "gh-project: self-test failed: the stub called gh." >&2
    exit 1
  fi
  rm -r "$work"
  echo "gh-project: self-test OK."
}

case "${1:-}" in
  '' | -h | --help) usage ;;
  --self-test) self_test ;;
  *)
    echo "gh-project: FerroPIX has no roadmap board; nothing to do (.claude/rules/project-board.md)" >&2
    ;;
esac
