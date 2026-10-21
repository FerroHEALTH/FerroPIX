#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# writing-style.sh: the mechanical half of .claude/rules/writing-style.md over
# the prose FerroPIX publishes (no specification governs this: our own
# design, the FerroHEALTH family book's guard).
#
#   scripts/checks/writing-style.sh [FILE...]
#   scripts/checks/writing-style.sh --self-test
#
# With no argument it checks the published prose: the site and the book under
# website/, the brand README, and the root and crate markdown a reader opens
# on GitHub or crates.io. With arguments it checks those files, which is what
# the PostToolUse hook passes after an edit. The vendored specifications keep
# their upstream words and are never checked.
#
# It catches what a regular expression can catch: em dashes, the buzzword
# list, and AI attribution. The tells that need judgement (the "not X but Y"
# setup, decorative triads, the TED-talk tone) are review-enforced, so a clean
# run here is not a pass on the rule. Exit 1 naming each hit; 0 otherwise.

set -uo pipefail

readonly BUZZWORDS='delve|robust|elevate|testament|landscape|leverage|tapestry|underscore|foster|realm|seamless|seamlessly|empower|unlock|cutting-edge|state-of-the-art|game-changing|holistic|synergy|streamline|harness|pivotal'
readonly ATTRIBUTION='co-authored-by:[[:space:]]*.*(claude|anthropic|\[bot\])|generated[[:space:]]+with[[:space:]]+.*claude|generated[[:space:]]+by[[:space:]]+.*claude|assisted-by:[[:space:]]*.*claude'

# One rule over one file. Prints every hit and returns non-zero on a hit, with
# no pipeline in between, because a `while` after a pipe runs in a subshell
# and would lose the result.
check() {
  local file="$1" pattern="$2" advice="$3" hits
  hits="$(grep -nEi -- "$pattern" "$file" || true)"
  [[ -n "$hits" ]] || return 0
  while IFS= read -r hit; do
    echo "writing-style: $file:$hit" >&2
    echo "  $advice" >&2
  done <<< "$hits"
  return 1
}

# check_files FILE...: every rule over every FILE; returns 1 on any hit.
check_files() {
  local file fail=0 checked=0
  for file in "$@"; do
    [[ -f "$file" ]] || continue
    case "$file" in
      docs/specs/* | */docs/specs/* | .claude/rules/writing-style.md | */.claude/rules/writing-style.md | scripts/checks/writing-style.sh | */scripts/checks/writing-style.sh) continue ;;
      *) ;;
    esac
    checked=$((checked + 1))
    check "$file" '—' 'em dash: use a comma, a period, parentheses, or a colon.' || fail=1
    check "$file" "(^|[^[:alnum:]_-])($BUZZWORDS)([^[:alnum:]_-]|$)" 'buzzword: use the plain word.' || fail=1
    check "$file" "$ATTRIBUTION" 'AI attribution: text describes only the change.' || fail=1
  done
  if [[ "$fail" -ne 0 ]]; then
    echo "writing-style: FAILED (.claude/rules/writing-style.md)" >&2
    return 1
  fi
  echo "writing-style: no mechanical tells in $checked files."
}

self_test() {
  local work failed=0 case
  work="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand $work now: the local is gone at EXIT.
  trap "rm -rf '$work'" EXIT
  printf 'The client refuses an answer that departs from the profile.\nThe lint is let_underscore_drop.\n' > "$work/sound.md"
  printf 'The client \342\200\224 a fast one \342\200\224 answers.\n' > "$work/dash.md"
  printf 'A robust index.\n' > "$work/buzzword.md"
  printf 'Co-Authored-By: Claude <noreply@anthropic.com>\n' > "$work/attribution.md"
  if check_files "$work/sound.md" > /dev/null 2>&1; then
    echo "  ok: plain prose passes, an identifier with an underscore included"
  else
    echo "  FAIL: plain prose was refused" >&2
    failed=1
  fi
  for case in dash buzzword attribution; do
    if check_files "$work/$case.md" > /dev/null 2>&1; then
      echo "  FAIL: the $case fixture passed" >&2
      failed=1
    else
      echo "  ok: the $case fixture fails"
    fi
  done
  [[ "$failed" -eq 0 ]] && echo "writing-style self-test: OK"
  return "$failed"
}

if [[ "${1:-}" == "--self-test" ]]; then
  self_test
  exit
fi

cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

files=()
if [[ "$#" -gt 0 ]]; then
  files=("$@")
else
  while IFS= read -r tracked; do
    files+=("$tracked")
  done < <(git ls-files 'website/*.md' 'website/*.html' 'website/*.css' 'website/*.toml' 'assets/brand/README.md' '*.md' ':!docs/**' ':!.claude/**')
fi

check_files "${files[@]}"
