#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# Stale pending-work guard. A `TODO(#N):` comment promises that issue N
# carries the missing work; when N closes without it, the pointer lies. This
# guard collects every such number from the tracked tree and fails when the
# tracker reports one closed or missing (no specification governs this: our
# own design).
#
#   scripts/checks/todo-issues.sh
#   scripts/checks/todo-issues.sh --self-test
#
# It needs a `gh` that can read the repository's issues: locally your login,
# in CI the job's GITHUB_TOKEN with `issues: read`. When `gh` is absent,
# unauthenticated or offline, the guard says so and exits 0; CI is where it
# gates.
set -euo pipefail
# The self-test builds a stub repository with a copy of this script and a gh
# on PATH that answers #1 open and #2 closed, and runs the guard there.
self_test() {
  local script work status
  script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
  work="$(mktemp -d)"
  mkdir -p "$work/repo/scripts/checks" "$work/bin"
  cp "$script" "$work/repo/scripts/checks/todo-issues.sh"
  cat > "$work/bin/gh" <<'STUB'
#!/usr/bin/env bash
case "$*" in
  "repo view --json name") echo '{"name":"Example"}' ;;
  "issue view 1 --json state --jq .state") echo OPEN ;;
  "issue view 2 --json state --jq .state") echo CLOSED ;;
  *) exit 1 ;;
esac
STUB
  chmod 0755 "$work/bin/gh"
  git -C "$work/repo" init -q
  # expect NAME WANT LINE: the tree holding LINE exits WANT.
  expect() {
    local name="$1" want="$2" line="$3"
    printf '%s\n' "$line" > "$work/repo/lib.rs"
    git -C "$work/repo" add lib.rs
    status=0
    PATH="$work/bin:$PATH" bash "$work/repo/scripts/checks/todo-issues.sh" > /dev/null 2>&1 || status=$?
    if [ "$status" -ne "$want" ]; then
      echo "todo-issues: self-test failed: $name exited $status, wanted $want." >&2
      exit 1
    fi
  }
  expect "a pointer at an open issue" 0 '// TODO(#1): the PDQ v2 query'
  expect "a pointer at a closed issue" 1 '// TODO(#2): the PIX feed'
  expect "a pointer at a missing issue" 1 '// TODO(#3): the merge'
  expect "a tree without pointers" 0 'fn main() {}'
  rm -r "$work"
  echo "todo-issues: self-test OK."
}

if [ "${1:-}" = "--self-test" ]; then
  self_test
  exit 0
fi

cd "$(dirname "$0")/../.."

# One "number<TAB>file:line" record per pointer.
pointers() {
  # git grep exits 1 when nothing matches, which is the clean tree. The
  # two guards whose self-tests carry pointers as fixture text are skipped.
  { git grep -nE 'TODO\(#[0-9]+\)' -- . ':!scripts/checks/todo-issues.sh' ':!scripts/checks/comment-style.sh' || true; } |
    while IFS= read -r hit; do
      local location=${hit%%:*}
      local rest=${hit#*:}
      location="$location:${rest%%:*}"
      grep -oE 'TODO\(#[0-9]+\)' <<< "$hit" | tr -dc '0-9\n' |
        while IFS= read -r number; do
          printf '%s\t%s\n' "$number" "$location"
        done
    done
}

if ! command -v gh > /dev/null 2>&1; then
  echo "todo-issues: skipped: gh is not installed, so the tracker cannot be read."
  exit 0
fi
if ! gh repo view --json name > /dev/null 2>&1; then
  echo "todo-issues: skipped: gh cannot read the repository (unauthenticated or offline)."
  exit 0
fi

records=$(pointers | sort -t $'\t' -k1,1n -k2,2)
if [ -z "$records" ]; then
  echo "todo-issues: no pending-work pointers in the tree."
  exit 0
fi

fail=0
checked=0
while IFS= read -r number; do
  checked=$((checked + 1))
  if ! state=$(gh issue view "$number" --json state --jq .state 2> /dev/null < /dev/null); then
    state=UNKNOWN
  fi
  [ "$state" = OPEN ] && continue
  fail=1
  while IFS=$'\t' read -r n location; do
    [ "$n" = "$number" ] || continue
    echo "::error file=${location%%:*},line=${location##*:}::$location names #$number, which the tracker does not answer as an open issue ($state)."
  done <<< "$records"
done < <(cut -f1 <<< "$records" | sort -un)

if [ "$fail" -eq 0 ]; then
  echo "todo-issues: $checked issue(s) named by pending-work pointers, all open."
fi
exit "$fail"
