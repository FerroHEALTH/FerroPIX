#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# The changelog guard (no specification governs it: our own design): a pull
# request records its user-visible effect in CHANGELOG.md, under
# [Unreleased], in the Keep a Changelog 1.1.0 layout. It passes when the
# change adds at least one bullet to the [Unreleased] section, cuts a
# release (adds a `## [X.Y.Z]` section the fork did not have), or adds a
# bullet to the newest `## [X.Y.Z]` section while no `vX.Y.Z` tag exists: a
# fix that lands between the release pull request and the tag records itself
# in the section the tag's notes are taken from.
#
#   changelog-guard.sh <base-ref> [head-ref]
#   changelog-guard.sh --self-test
#
# The change is what the head added since it forked from the base, read
# against the merge base of the two, so an entry that reached the base after
# the fork is never counted as the change's own.
#
# Exit 0 when the change carries an entry, 1 when it carries none, and 2 on a
# usage error or a base and head with no merge base. The `no-changelog`
# pull-request label is the CI escape for a change with no user-visible
# effect; this script does not read labels.
set -euo pipefail

# The version headings of the CHANGELOG.md read on stdin.
versions() {
  sed -nE 's/^## \[([0-9]+\.[0-9]+\.[0-9]+)\].*/\1/p'
}

# The bullets of the [Unreleased] section of the CHANGELOG.md read on stdin.
unreleased() {
  awk '/^## \[Unreleased\]/ { p = 1; next } p && /^## \[/ { exit } p && /^- / { print }'
}

# The bullets of the `## [$1]` section of the CHANGELOG.md read on stdin.
section() {
  awk -v v="$1" 'index($0, "## [" v "]") == 1 { p = 1; next } p && /^## \[/ { exit } p && /^- / { print }'
}

# The self-test builds a stub repository in a temporary directory, with a copy
# of this script in it, and runs the guard there as CI does: the base is
# main's tip and the head is the branch.
self_test() {
  local script status
  # Global, so the EXIT trap still sees it after the function returns.
  work="$(mktemp -d)"
  # git writes its objects read-only, so the tree is made writable before it
  # is removed.
  trap 'chmod -R u+w "$work" && rm -r -- "$work"' EXIT
  script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
  mkdir -p "$work/scripts/checks"
  cp "$script" "$work/scripts/checks/changelog-guard.sh"

  # A repository of its own, so no global hook, signing key or identity of the
  # caller's reaches it.
  stub_git() {
    git -C "$work" -c user.name=self-test -c user.email=self-test@example.org \
      -c commit.gpgsign=false -c tag.gpgsign=false -c core.hooksPath=/dev/null "$@"
  }
  commit() {
    stub_git add -A
    stub_git commit -q -m "$1"
  }
  branch() {
    stub_git checkout -q main
    stub_git checkout -q -b "$1"
  }
  # expect NAME WANT BRANCH: the guard over BRANCH against main exits WANT.
  expect() {
    local name=$1 want=$2 branch=$3
    status=0
    (cd "$work" && bash scripts/checks/changelog-guard.sh main "$branch") > "$work/out" 2>&1 || status=$?
    if [[ "$status" -ne "$want" ]]; then
      echo "changelog-guard: self-test failed: $name exited $status, wanted $want." >&2
      cat "$work/out" >&2
      exit 1
    fi
  }

  stub_git init -q -b main
  printf '# Changelog\n\n## [Unreleased]\n\n## [0.0.1] - 2026-10-20\n\n### Added\n\n- Released.\n' > "$work/CHANGELOG.md"
  echo 'code' > "$work/code.txt"
  commit "the fork point"
  stub_git tag v0.0.1

  branch entry
  echo 'more code' >> "$work/code.txt"
  perl -0pi -e 's/## \[Unreleased\]\n/## [Unreleased]\n\n### Added\n\n- An entry.\n/' "$work/CHANGELOG.md"
  commit "a change with an entry"
  branch released
  perl -0pi -e 's/- Released\./- Released, reworded./' "$work/CHANGELOG.md"
  commit "a change that edits a released section only"
  branch release
  perl -0pi -e 's/## \[Unreleased\]\n/## [Unreleased]\n\n## [0.0.2] - 2026-10-21\n\n### Added\n\n- Cut.\n/' "$work/CHANGELOG.md"
  commit "a release cut"
  branch none
  echo 'more code' >> "$work/code.txt"
  commit "a change with no entry"

  stub_git checkout -q main
  perl -0pi -e 's/## \[Unreleased\]\n/## [Unreleased]\n\n### Fixed\n\n- From main.\n/' "$work/CHANGELOG.md"
  commit "main gains an entry after every branch forked"

  # main cuts 0.0.2 and leaves it untagged; a fix records itself in the cut.
  perl -0pi -e 's/## \[Unreleased\]\n\n### Fixed\n\n- From main\.\n/## [Unreleased]\n\n## [0.0.2] - 2026-10-21\n\n### Fixed\n\n- From main.\n/' "$work/CHANGELOG.md"
  commit "main cuts 0.0.2"
  branch late-fix
  echo 'a late fix' >> "$work/code.txt"
  perl -0pi -e 's/- From main\.\n/- From main.\n- A late fix.\n/' "$work/CHANGELOG.md"
  commit "a fix recorded in the untagged cut section"
  branch older
  perl -0pi -e 's/- Released\.\n/- Released.\n- An addition to a tagged release.\n/' "$work/CHANGELOG.md"
  commit "a bullet added to an older, tagged section"

  expect "a change that adds an entry" 0 entry
  expect "a change that edits a released section only" 1 released
  expect "a release cut" 0 release
  expect "a change with no entry, behind a main that gained one" 1 none
  grep -q 'no changelog entry' "$work/out" || {
    echo "changelog-guard: self-test failed: the refusal did not say why." >&2
    cat "$work/out" >&2
    exit 1
  }
  expect "a bullet added to the untagged cut section" 0 late-fix
  grep -q 'untagged release 0.0.2' "$work/out" || {
    echo "changelog-guard: self-test failed: the pass did not name the untagged section." >&2
    cat "$work/out" >&2
    exit 1
  }
  expect "a bullet added to an older, tagged section" 1 older
  stub_git tag v0.0.2 main
  expect "a bullet added to the cut section once it is tagged" 1 late-fix
  echo "changelog-guard: self-test passed."
}

if [[ "${1:-}" = "--self-test" && $# -eq 1 ]]; then
  self_test
  exit 0
fi

cd "$(dirname "$0")/../.."

if [[ $# -lt 1 || $# -gt 2 || -z "${1:-}" ]]; then
  echo "usage: changelog-guard.sh <base-ref> [head-ref] | --self-test" >&2
  exit 2
fi
base="$1"
head="${2:-HEAD}"

if ! fork="$(git merge-base "$base" "$head")"; then
  echo "changelog-guard: $base and $head have no merge base; check out the full history (fetch-depth: 0)." >&2
  exit 2
fi

before="$(git show "$fork:CHANGELOG.md" 2> /dev/null | unreleased || true)"
after="$(git show "$head:CHANGELOG.md" 2> /dev/null | unreleased || true)"
added="$(comm -13 <(sort <<< "$before") <(sort <<< "$after") | sed '/^$/d')"
if [[ -n "$added" ]]; then
  echo "changelog-guard: the change adds to [Unreleased]:"
  while IFS= read -r line; do echo "  $line"; done <<< "$added"
  exit 0
fi
newest="$(git show "$head:CHANGELOG.md" 2> /dev/null | versions | head -n 1 || true)"
if [[ -n "$newest" ]] && ! git rev-parse -q --verify "refs/tags/v$newest" > /dev/null; then
  before="$(git show "$fork:CHANGELOG.md" 2> /dev/null | section "$newest" || true)"
  after="$(git show "$head:CHANGELOG.md" 2> /dev/null | section "$newest" || true)"
  added="$(comm -13 <(sort <<< "$before") <(sort <<< "$after") | sed '/^$/d')"
  if [[ -n "$added" ]]; then
    echo "changelog-guard: the change adds to the untagged release $newest:"
    while IFS= read -r line; do echo "  $line"; done <<< "$added"
    exit 0
  fi
fi
cut="$(comm -13 <(git show "$fork:CHANGELOG.md" 2> /dev/null | versions | sort) <(git show "$head:CHANGELOG.md" 2> /dev/null | versions | sort) | sed '/^$/d')"
if [[ -n "$cut" ]]; then
  echo "changelog-guard: the change cuts the release(s): $(tr '\n' ' ' <<< "$cut")"
  exit 0
fi
echo "::error::no changelog entry: add a bullet under [Unreleased] in CHANGELOG.md, in its Keep a Changelog section, or apply the 'no-changelog' label when the change has no user-visible effect." >&2
exit 1
