#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# The two version lines (.claude/rules/crates-publishing.md; no specification
# governs them: our own design, the model FerroBRIDGE uses).
#
# The product line is the `[workspace.package]` version, which is the release
# tag: it never moves backwards, and a change that moves it adds its
# CHANGELOG.md section in the same change.
#
# The crate line is each published crate's own `version` in
# crates/<name>/Cargo.toml (the CRATES array of
# scripts/release/publish-crates.sh). It never adopts the product version, so
# a listed crate that inherits it fails. It never moves backwards, Cargo.lock
# records it, and a change that alters the crate's packaged content (what its
# `include` ships: `src/**`, `README.md`, `LICENSE`, `Cargo.toml`, or a root
# `[workspace.dependencies]` entry it consumes, which `cargo package` renders)
# moves it, because a published version is immutable. A crate new at the head
# and a 0.0.0 name reservation are outside the bump rule.
#
#   crate-version-guard.sh <base-ref> [head-ref|WORKTREE]
#   crate-version-guard.sh --self-test
#
# The crate diff runs from the merge base of the base ref and the head, so a
# change the base gained after the head forked is not counted against the
# head. The head ref may be the literal WORKTREE, which reads the files as
# they stand on disk (what scripts/gh/watch-prs.sh passes mid-merge). With
# NO_CRATE_BUMP=true (the `no-crate-bump` pull-request label, for a diff that
# provably alters no packaged bytes) the bump rule is waived; every other
# check still runs. Exit 0 when the rules hold, 1 when they do not, 2 on a
# usage error.
set -euo pipefail

# The `version` key of `[workspace.package]` in a Cargo.toml read on stdin.
workspace_version() {
  awk '/^\[workspace\.package\]/{p=1; next} /^\[/{p=0}
       p && /^version[[:space:]]*=/{gsub(/.*=[[:space:]]*"|".*/, ""); print; exit}'
}

# The `version` of `[package]` in a Cargo.toml read on stdin: the quoted
# value, `workspace` for `version.workspace = true`, or nothing.
package_version() {
  awk '/^\[package\]/{p=1; next} /^\[/{p=0}
       p && /^version\.workspace[[:space:]]*=[[:space:]]*true/{print "workspace"; exit}
       p && /^version[[:space:]]*=/{gsub(/.*=[[:space:]]*"|".*/, ""); print; exit}'
}

# The version of the package $1 in a Cargo.lock read on stdin.
locked_version() {
  awk -v name="$1" '$0 == "name = \"" name "\"" {p=1; next}
       p && /^version = /{gsub(/version = "|"/, ""); print; exit}'
}

# The names the `[workspace.dependencies]` table of a Cargo.toml on stdin
# declares, one a line.
workspace_dependencies() {
  awk '/^\[workspace\.dependencies\]/{p=1; next} /^\[/{p=0}
       p && /^[A-Za-z0-9_-]+[[:space:]]*=/{sub(/[[:space:]]*=.*/, ""); print}'
}

# newer A B: whether version B sorts after version A, a pre-release before its
# release.
newer() {
  local a=$1 b=$2 newest
  [[ "$a" != "$b" ]] || return 1
  if [[ "${a%%-*}" == "${b%%-*}" ]]; then
    [[ "$a" == *-* && "$b" != *-* ]] && return 0
    [[ "$b" == *-* && "$a" != *-* ]] && return 1
  fi
  newest="$(printf '%s\n%s\n' "$a" "$b" | sort -V | tail -1)"
  [[ "$newest" == "$b" ]]
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
  # manifest PRODUCT [DEPENDENCY]: the root manifest at PRODUCT, its
  # dependency table declaring DEPENDENCY.
  manifest() {
    printf '[workspace]\nmembers = ["crates/*"]\n\n[workspace.package]\nversion = "%s"\n\n[workspace.dependencies]\n%s\n' \
      "$1" "${2:-}" > "$work/Cargo.toml"
  }
  published() {
    printf 'readonly CRATES=(\n' > "$work/scripts/release/publish-crates.sh"
    if [[ -n "$1" ]]; then printf '  %s\n' "$1" >> "$work/scripts/release/publish-crates.sh"; fi
    printf ')\n' >> "$work/scripts/release/publish-crates.sh"
  }
  # crate VERSION [LOCKED]: crates/pix-a at VERSION, consuming the workspace
  # `serde`, locked at LOCKED (VERSION when not given).
  crate() {
    mkdir -p "$work/crates/pix-a/src"
    if [[ "$1" == workspace ]]; then
      printf '[package]\nname = "pix-a"\nversion.workspace = true\n\n[dependencies]\nserde.workspace = true\n' > "$work/crates/pix-a/Cargo.toml"
    else
      printf '[package]\nname = "pix-a"\nversion = "%s"\n\n[dependencies]\nserde.workspace = true\n' "$1" > "$work/crates/pix-a/Cargo.toml"
    fi
    printf '[[package]]\nname = "pix-a"\nversion = "%s"\n' "${2:-$1}" > "$work/Cargo.lock"
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
  # expect NAME WANT BRANCH [ENV…]: the guard over BRANCH against main exits
  # WANT.
  expect() {
    local name=$1 want=$2 branch=$3
    shift 3
    stub_git checkout -q "$branch"
    status=0
    (cd "$work" && env "$@" bash scripts/checks/crate-version-guard.sh main "$branch") > "$work/.git/guard-out" 2>&1 || status=$?
    if [[ "$status" -ne "$want" ]]; then
      echo "crate-version-guard: self-test failed: $name exited $status, wanted $want." >&2
      cat "$work/.git/guard-out" >&2
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

  # The product line.
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

  expect "the workspace's first version" 0 introduce
  expect "a change that moves nothing" 0 untouched
  expect "a release with its section" 0 release
  expect "a version move without a section" 1 nosection
  expect "a version that moves backwards" 1 backwards

  # A crate arrives on its own line, above the product version.
  branch arrive
  published "pix-a"
  manifest 0.0.0 'serde = "1"'
  crate 0.0.39
  echo 'pub fn a() {}' > "$work/crates/pix-a/src/lib.rs"
  commit "a crate arrives at 0.0.39"
  branch inherit
  published "pix-a"
  manifest 0.0.0 'serde = "1"'
  crate workspace 0.0.0
  commit "a crate that inherits the product version"
  branch stale
  published "pix-a"
  manifest 0.0.0 'serde = "1"'
  crate 0.0.39 0.0.38
  commit "a crate locked behind its version"
  expect "a crate arriving on its own line" 0 arrive
  expect "a crate inheriting the product version" 1 inherit
  grep -q 'inherits the workspace version' "$work/.git/guard-out" || {
    echo "crate-version-guard: self-test failed: the inheriting crate was not named." >&2
    exit 1
  }
  expect "a crate locked behind its version" 1 stale
  grep -q 'Cargo.lock holds pix-a 0.0.38' "$work/.git/guard-out" || {
    echo "crate-version-guard: self-test failed: the stale lock was not named." >&2
    exit 1
  }
  stub_git checkout -q main
  stub_git merge -q --ff-only arrive

  # The bump rule over the crate line.
  branch fork
  echo 'notes' >> "$work/NOTES.md"
  commit "a branch that forks before main moves"
  stub_git checkout -q main
  echo 'pub fn b() {}' >> "$work/crates/pix-a/src/lib.rs"
  crate 0.0.40
  commit "main changes the crate with its bump"
  branch unbumped
  echo 'pub fn c() {}' >> "$work/crates/pix-a/src/lib.rs"
  commit "a crate change without a bump"
  branch bumped
  echo 'pub fn c() {}' >> "$work/crates/pix-a/src/lib.rs"
  crate 0.0.41
  commit "a crate change with its bump"
  branch dependency
  manifest 0.0.0 'serde = "1.1"'
  commit "a workspace dependency the crate consumes moves"
  branch lower
  echo 'pub fn c() {}' >> "$work/crates/pix-a/src/lib.rs"
  crate 0.0.39
  commit "a crate version that moves backwards"
  branch together
  manifest 0.0.1
  section 0.0.1
  echo 'pub fn c() {}' >> "$work/crates/pix-a/src/lib.rs"
  crate 0.0.41
  commit "a release beside a crate bump"
  expect "a crate change main gained after the fork" 0 fork
  expect "a crate change without a bump" 1 unbumped
  grep -q 'pix-a changed but its version is still 0.0.40' "$work/.git/guard-out" || {
    echo "crate-version-guard: self-test failed: the unbumped crate was not named." >&2
    exit 1
  }
  expect "a crate change without a bump, waived" 0 unbumped NO_CRATE_BUMP=true
  expect "a crate change with its bump" 0 bumped
  expect "a consumed workspace dependency without a bump" 1 dependency
  expect "a crate version that moves backwards" 1 lower
  expect "a release beside a crate bump" 0 together
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
base_ref="$1"
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

# During an uncommitted merge the worktree already holds MERGE_HEAD, so the
# merge base is taken against both parents.
head_commits=("$head")
if [[ "$head" == WORKTREE ]]; then
  head_commits=(HEAD)
  if merge_head="$(git rev-parse -q --verify MERGE_HEAD)"; then head_commits+=("$merge_head"); fi
fi
fork="$(git merge-base "$base_ref" "${head_commits[@]}")" || {
  echo "crate-version-guard: no merge base between $base_ref and ${head_commits[*]}" >&2
  exit 1
}

failed=0

# The product line.
want="$(read_head Cargo.toml | workspace_version)"
was="$(git show "$base_ref:Cargo.toml" 2> /dev/null | workspace_version || true)"
if [[ -z "$want" ]]; then
  echo "crate-version-guard: Cargo.toml declares no [workspace.package] version" >&2
  exit 1
fi
if [[ -n "$was" && "$was" != "$want" ]]; then
  if ! newer "$was" "$want"; then
    echo "crate-version-guard: the product version moves backwards, $was to $want" >&2
    failed=1
  elif ! read_head CHANGELOG.md | grep -qF "## [$want]"; then
    echo "crate-version-guard: the product version moves to $want, but CHANGELOG.md has no section for it" >&2
    failed=1
  fi
fi

# The crate line.
if [[ "$head" == WORKTREE ]]; then
  changed="$(git diff --name-only "$fork" --)"
  root_diff="$(git diff "$fork" -- Cargo.toml)"
else
  changed="$(git diff --name-only "$fork" "$head" --)"
  root_diff="$(git diff "$fork" "$head" -- Cargo.toml)"
fi
# The workspace dependency names this change touched, restricted to names the
# table declares on either side.
touched=""
if [[ -n "$root_diff" ]]; then
  declared="$( { read_head Cargo.toml; git show "$fork:Cargo.toml" 2> /dev/null || true; } | workspace_dependencies)"
  while IFS= read -r name; do
    [[ -n "$name" ]] || continue
    if grep -qxF "$name" <<< "$declared"; then touched="$touched $name"; fi
  done < <(grep -E '^[+-][A-Za-z0-9_-]+[[:space:]]*=' <<< "$root_diff" | sed -E 's/^[+-]//; s/[[:space:]]*=.*//' | sort -u)
fi

crates="$(read_head scripts/release/publish-crates.sh | awk '/^readonly CRATES=\(/ { p = 1; next } p && /^\)/ { exit } p { gsub(/[[:space:]]/, ""); if ($0 != "" && $0 !~ /^#/) print }')"
lock="$(read_head Cargo.lock)"
summary=""
while IFS= read -r crate; do
  [[ -n "$crate" ]] || continue
  manifest="crates/$crate/Cargo.toml"
  new_ver="$(read_head "$manifest" | package_version)"
  if [[ -z "$new_ver" ]]; then
    echo "crate-version-guard: $crate is listed for publishing, but $manifest declares no version" >&2
    failed=1
    continue
  fi
  if [[ "$new_ver" == workspace ]]; then
    echo "crate-version-guard: $crate inherits the workspace version; a published crate keeps its own line in $manifest" >&2
    failed=1
    continue
  fi
  locked="$(locked_version "$crate" <<< "$lock")"
  if [[ "$locked" != "$new_ver" ]]; then
    echo "crate-version-guard: Cargo.lock holds $crate ${locked:-<none>}, its manifest $new_ver; run cargo update -w" >&2
    failed=1
  fi
  old_ver="$(git show "$fork:$manifest" 2> /dev/null | package_version || true)"
  if [[ -z "$old_ver" || "$old_ver" == workspace ]]; then
    summary="$summary $crate@$new_ver(new)"
    continue
  fi
  if [[ "$old_ver" != "$new_ver" ]] && ! newer "$old_ver" "$new_ver"; then
    echo "crate-version-guard: $crate moves backwards, $old_ver to $new_ver" >&2
    failed=1
    continue
  fi
  packaged=0
  if grep -qE "^crates/$crate/(src/|README\.md$|LICENSE$|Cargo\.toml$)" <<< "$changed"; then
    packaged=1
  fi
  for dep in $touched; do
    if read_head "$manifest" | grep -qE "^${dep}(\.workspace)?[[:space:]]*="; then packaged=1; fi
  done
  if [[ "$packaged" -eq 1 && "$old_ver" == "$new_ver" && "$new_ver" != 0.0.0 ]]; then
    if [[ "${NO_CRATE_BUMP:-false}" == true ]]; then
      echo "crate-version-guard: $crate changed at $new_ver; the bump rule is waived (no-crate-bump)"
    else
      echo "crate-version-guard: the packaged content of $crate changed but its version is still $new_ver; bump $manifest and run cargo update -w, or label the pull request no-crate-bump when no packaged byte changed" >&2
      failed=1
    fi
    continue
  fi
  summary="$summary $crate@$new_ver"
done <<< "$crates"

[[ "$failed" -eq 0 ]] || exit 1
echo "crate-version-guard: product $want agrees with the changelog; crates:${summary:- none listed}"
