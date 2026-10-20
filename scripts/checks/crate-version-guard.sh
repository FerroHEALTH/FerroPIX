#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# The crate version rule (.claude/rules/crates-publishing.md): the workspace
# version is the version of every published crate and the release tag, so it
# never moves backwards, Cargo.lock carries it for each published crate, and
# a change that moves it is a release change that adds its CHANGELOG.md
# section in the same change (no specification governs this: our own
# design). The published crates are the CRATES array of
# scripts/release/publish-crates.sh; while it holds none, the lock is not read.
#
#   crate-version-guard.sh <base-ref> [head-ref|WORKTREE]
#   crate-version-guard.sh --self-test
#
# The head ref may be the literal WORKTREE, which reads the files as they
# stand on disk (what scripts/gh/watch-prs.sh passes mid-merge). Exit 0 when
# the rule holds, 1 when it does not, 2 on a usage error.
set -euo pipefail

# The `version` key of `[workspace.package]` in a Cargo.toml read on stdin.
workspace_version() {
  awk '/^\[workspace\.package\]/{p=1; next} /^\[/{p=0}
       p && /^version[[:space:]]*=/{gsub(/.*=[[:space:]]*"|".*/, ""); print; exit}'
}

# The version of the package $1 in a Cargo.lock read on stdin.
locked_version() {
  awk -v name="$1" '$0 == "name = \"" name "\"" {p=1; next}
       p && /^version = /{gsub(/version = "|"/, ""); print; exit}'
}

# The self-test builds a stub repository in a temporary directory, with a copy
# of this script and a stub publish list, and runs the guard on branches.
self_test() {
  local script status
  # Global, so the EXIT trap still sees it after the function returns.
  work="$(mktemp -d)"
  trap 'chmod -R u+w "$work" && rm -r -- "$work"' EXIT
  script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
  mkdir -p "$work/scripts/checks" "$work/scripts/release"
  cp "$script" "$work/scripts/checks/crate-version-guard.sh"
  stub_git() {
    git -C "$work" -c user.name=self-test -c user.email=self-test@example.org \
      -c commit.gpgsign=false -c core.hooksPath=/dev/null "$@"
  }
  manifest() {
    printf '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "%s"\n' "$1" > "$work/Cargo.toml"
  }
  published() {
    printf 'readonly CRATES=(\n' > "$work/scripts/release/publish-crates.sh"
    if [[ -n "$1" ]]; then printf '  %s\n' "$1" >> "$work/scripts/release/publish-crates.sh"; fi
    printf ')\n' >> "$work/scripts/release/publish-crates.sh"
  }
  section() {
    perl -0pi -e "s/## \\[Unreleased\\]\\n/## [Unreleased]\\n\\n## [$1] - 2026-10-21\\n/" "$work/CHANGELOG.md"
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
    stub_git checkout -q "$branch"
    status=0
    (cd "$work" && bash scripts/checks/crate-version-guard.sh main "$branch") > "$work/out" 2>&1 || status=$?
    if [[ "$status" -ne "$want" ]]; then
      echo "crate-version-guard: self-test failed: $name exited $status, wanted $want." >&2
      cat "$work/out" >&2
      exit 1
    fi
  }

  stub_git init -q -b main
  printf '# Changelog\n\n## [Unreleased]\n' > "$work/CHANGELOG.md"
  echo notes > "$work/NOTES.md"
  published ""
  commit "before the workspace"

  branch introduce
  manifest 0.0.0
  commit "the workspace at 0.0.0"
  stub_git checkout -q main
  stub_git merge -q --ff-only introduce

  branch untouched
  echo more >> "$work/NOTES.md"
  commit "a change that moves nothing"
  branch release
  manifest 0.0.1
  section 0.0.1
  commit "a release with its section"
  branch nosection
  manifest 0.0.1
  commit "a version move without a section"
  branch backwards
  manifest 0.0.0-rc.1
  commit "a version that moves backwards"
  branch locked
  published "pix-a"
  manifest 0.0.1
  section 0.0.1
  printf '[[package]]\nname = "pix-a"\nversion = "0.0.1"\n' > "$work/Cargo.lock"
  commit "a published crate locked at the version"
  branch stale
  published "pix-a"
  manifest 0.0.1
  section 0.0.1
  printf '[[package]]\nname = "pix-a"\nversion = "0.0.0"\n' > "$work/Cargo.lock"
  commit "a published crate locked behind the version"

  expect "the workspace's first version" 0 introduce
  expect "a change that moves nothing" 0 untouched
  expect "a release with its section" 0 release
  expect "a version move without a section" 1 nosection
  expect "a version that moves backwards" 1 backwards
  expect "a published crate locked at the version" 0 locked
  expect "a published crate locked behind the version" 1 stale
  grep -q 'Cargo.lock holds pix-a 0.0.0' "$work/out" || {
    echo "crate-version-guard: self-test failed: the stale lock was not named." >&2
    exit 1
  }
  echo "crate-version-guard: self-test passed."
}

if [[ "${1:-}" == "--self-test" && $# -eq 1 ]]; then
  self_test
  exit 0
fi

cd "$(dirname "$0")/../.."
if [[ $# -lt 1 || $# -gt 2 || -z "${1:-}" ]]; then
  echo "usage: crate-version-guard.sh <base-ref> [head-ref|WORKTREE] | --self-test" >&2
  exit 2
fi
base="$1"
head="${2:-HEAD}"

# read_head PATH: the file at the head, or on disk for WORKTREE.
read_head() {
  if [[ "$head" == WORKTREE ]]; then
    cat "$1" 2> /dev/null || true
  else
    git show "$head:$1" 2> /dev/null || true
  fi
}

if [[ -z "$(read_head Cargo.toml)" ]]; then
  echo "crate-version-guard: no workspace yet, nothing to check"
  exit 0
fi
want="$(read_head Cargo.toml | workspace_version)"
was="$(git show "$base:Cargo.toml" 2> /dev/null | workspace_version || true)"
if [[ -z "$want" ]]; then
  echo "crate-version-guard: Cargo.toml declares no [workspace.package] version" >&2
  exit 1
fi

failed=0
if [[ -n "$was" && "$was" != "$want" ]]; then
  newest="$(printf '%s\n%s\n' "$was" "$want" | sort -V | tail -1)"
  if [[ "$newest" != "$want" || "$want" == *-* && "$was" != *-* && "${want%%-*}" == "$was" ]]; then
    echo "crate-version-guard: the version moves backwards, $was to $want" >&2
    failed=1
  elif ! read_head CHANGELOG.md | grep -qF "## [$want]"; then
    echo "crate-version-guard: the version moves to $want, but CHANGELOG.md has no section for it" >&2
    failed=1
  fi
fi

# The CRATES array of the publish script, one name per line.
crates="$(read_head scripts/release/publish-crates.sh | awk '/^readonly CRATES=\(/ { p = 1; next } p && /^\)/ { exit } p { gsub(/[[:space:]]/, ""); if ($0 != "" && $0 !~ /^#/) print }')"
lock="$(read_head Cargo.lock)"
while IFS= read -r crate; do
  [[ -n "$crate" ]] || continue
  locked="$(locked_version "$crate" <<< "$lock")"
  if [[ "$locked" != "$want" ]]; then
    echo "crate-version-guard: Cargo.lock holds $crate ${locked:-<none>}, the workspace $want; run cargo update -w" >&2
    failed=1
  fi
done <<< "$crates"

[[ "$failed" -eq 0 ]] || exit 1
echo "crate-version-guard: version $want agrees with the lock and the changelog"
