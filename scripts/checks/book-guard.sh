#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# The book guard (no specification governs it: our own design, the same-PR
# rule of .claude/rules/docs-website.md): a pull request that changes a
# surface a user of FerroPIX sees also changes the book under
# website/book/src, in the same pull request, as the changelog guard asks of
# CHANGELOG.md.
#
# The user-visible surfaces:
#   crates/*/src/**                   what a published crate does
#   crates/*/Cargo.toml [features]    the features a crate offers
#   app/**                            the server
#   docker/**, deploy/**, compose*.y*ml, Dockerfile*
#                                     how FerroPIX is deployed
#
# A crate version bump alone, a dependency move, a test or a vendored corpus
# is no surface: the book renders the versions and the pins at assembly.
#
#   book-guard.sh <base-ref> [head-ref]
#   book-guard.sh --self-test
#
# The change is what the head added since it forked from the base, read
# against the merge base of the two. Exit 0 when no surface changed or the
# book changed with it, 1 when a surface changed and the book did not, and 2
# on a usage error or a base and head with no merge base. The `no-book`
# pull-request label is the CI escape for a change that alters nothing the
# book says, such as a refactor; this script does not read labels.
set -euo pipefail

# features_at REV MANIFEST: the [features] table of MANIFEST at REV.
features_at() {
  git show "$1:$2" 2> /dev/null | awk '/^\[features\]/ { p = 1; next } /^\[/ { p = 0 } p' || true
}

self_test() {
  local script status
  work="$(mktemp -d)"
  trap 'chmod -R u+w "$work" && rm -r -- "$work"' EXIT
  script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
  mkdir -p "$work/scripts/checks"
  cp "$script" "$work/scripts/checks/book-guard.sh"

  stub_git() {
    git -C "$work" -c user.name=self-test -c user.email=self-test@example.org \
      -c commit.gpgsign=false -c core.hooksPath=/dev/null "$@"
  }
  commit() {
    stub_git add -A
    stub_git commit -q -m "$1"
  }
  branch() {
    stub_git checkout -q main
    stub_git checkout -q -b "$1"
  }
  expect() {
    local name=$1 want=$2 branch=$3
    status=0
    (cd "$work" && bash scripts/checks/book-guard.sh main "$branch") > "$work/out" 2>&1 || status=$?
    if [[ "$status" -ne "$want" ]]; then
      echo "book-guard: self-test failed: $name exited $status, wanted $want." >&2
      cat "$work/out" >&2
      exit 1
    fi
    echo "  ok: $name exits $want"
  }

  stub_git init -q -b main
  mkdir -p "$work/crates/demo/src" "$work/crates/demo/tests" "$work/website/book/src"
  printf '[package]\nname = "demo"\nversion = "0.1.0"\n\n[features]\nalpha = []\n' > "$work/crates/demo/Cargo.toml"
  echo 'pub fn f() {}' > "$work/crates/demo/src/lib.rs"
  echo 'test' > "$work/crates/demo/tests/it.rs"
  echo '# Book' > "$work/website/book/src/page.md"
  commit "the fork point"

  branch src-alone
  echo 'pub fn g() {}' >> "$work/crates/demo/src/lib.rs"
  commit "a crate change without the book"
  branch src-with-book
  echo 'pub fn g() {}' >> "$work/crates/demo/src/lib.rs"
  echo 'g, documented.' >> "$work/website/book/src/page.md"
  commit "a crate change with the book"
  branch feature-alone
  printf 'beta = []\n' >> "$work/crates/demo/Cargo.toml"
  commit "a feature without the book"
  branch version-bump
  perl -pi -e 's/0\.1\.0/0.1.1/' "$work/crates/demo/Cargo.toml"
  commit "a version bump"
  branch test-only
  echo 'more' >> "$work/crates/demo/tests/it.rs"
  commit "a test"
  branch server-alone
  mkdir -p "$work/app/server"
  echo 'fn main() {}' > "$work/app/server/main.rs"
  commit "the server without the book"
  branch compose-alone
  echo 'services: {}' > "$work/compose.yaml"
  commit "a compose file without the book"

  expect "a crate source change without the book" 1 src-alone
  grep -q "no-book" "$work/out" || {
    echo "book-guard: self-test failed: the refusal did not name the escape." >&2
    exit 1
  }
  expect "a crate source change with the book" 0 src-with-book
  expect "a new feature without the book" 1 feature-alone
  expect "a version bump alone" 0 version-bump
  expect "a test alone" 0 test-only
  expect "the server without the book" 1 server-alone
  expect "a compose file without the book" 1 compose-alone
  echo "book-guard: self-test passed."
}

if [[ "${1:-}" = "--self-test" && $# -eq 1 ]]; then
  self_test
  exit 0
fi

cd "$(dirname "$0")/../.."

if [[ $# -lt 1 || $# -gt 2 || -z "${1:-}" ]]; then
  echo "usage: book-guard.sh <base-ref> [head-ref] | --self-test" >&2
  exit 2
fi
base="$1"
head="${2:-HEAD}"

if ! fork="$(git merge-base "$base" "$head")"; then
  echo "book-guard: $base and $head have no merge base; check out the full history (fetch-depth: 0)." >&2
  exit 2
fi

surfaces=()
while IFS= read -r path; do
  case "$path" in
    crates/*/src/* | app/* | docker/* | deploy/* | compose*.yaml | compose*.yml | Dockerfile* | */Dockerfile*)
      surfaces+=("$path")
      ;;
    crates/*/Cargo.toml)
      if [[ "$(features_at "$fork" "$path")" != "$(features_at "$head" "$path")" ]]; then
        surfaces+=("$path [features]")
      fi
      ;;
    *) ;;
  esac
done < <(git diff --name-only "$fork" "$head")

if [[ "${#surfaces[@]}" -eq 0 ]]; then
  echo "book-guard: the change touches no user-visible surface."
  exit 0
fi
if git diff --name-only "$fork" "$head" -- website/book/src | grep -q .; then
  echo "book-guard: the change updates the book with its surfaces:"
  printf '  %s\n' "${surfaces[@]}"
  exit 0
fi
echo "book-guard: the change touches these user-visible surfaces:" >&2
printf '  %s\n' "${surfaces[@]}" >&2
echo "::error::the book is not updated: change the matching page under website/book/src in this pull request, or apply the 'no-book' label when the change alters nothing the book says." >&2
exit 1
