#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# docs-claims.sh: a claim the book makes that has a machine authority agrees
# with it (no specification governs this: our own design, after FerroEHR's
# docs-claims guard). A link check passes on a page whose every claim is
# false, so this guard checks the claims a file can answer:
#
#   features  every published crate under crates/ has a page under
#             website/book/src/crates/, listed on crates/index.md, and the
#             Features table of that page names exactly the features of the
#             crate's Cargo.toml: a feature added without its row, or a row
#             for a feature that does not exist, fails.
#   versions  no page types a published crate's current version; the book
#             writes {{version:<crate>}}, which scripts/site/assemble.sh
#             renders from the manifest, so the number cannot go stale.
#   planned   every section of a page (the text between two headings) that
#             calls something planned links its issue or milestone on a
#             FerroHEALTH tracker, so a reader can see where it stands.
#
# What is not checked, and why: prose claims of behaviour ("the client
# refuses an answer without its score") have no machine authority, and Rust
# paths in prose were measured in FerroEHR to cry wolf more than they find.
# Both are review's, under the same-pull-request rule of
# .claude/rules/docs-website.md.
#
# Usage:
#   scripts/checks/docs-claims.sh [--root DIR]   check this repository, or DIR
#   scripts/checks/docs-claims.sh --self-test    prove each refusal on fixtures
# Exit 1 naming each refusal; 2 on a usage error; 0 otherwise.

set -euo pipefail
export LC_ALL=C

readonly BOOK=website/book/src
readonly TRACKER='https://github\.com/FerroHEALTH/[A-Za-z]+/(issues|milestone)/[0-9]+'

# features MANIFEST: the feature names of MANIFEST's [features] table, sorted,
# `default` left out (it names other features and is no feature of its own).
features() {
  awk '/^\[features\]/ { p = 1; next } /^\[/ { p = 0 }
    p && /^[A-Za-z0-9_-]+[[:space:]]*=/ { name = $1; sub(/=.*/, "", name); if (name != "default") print name }' "$1" | sort -u
}

# documented PAGE: the feature names the first column of the table under the
# page's "## Features" heading names, sorted.
documented() {
  awk '/^## Features/ { p = 1; next } /^## / { p = 0 }
    p && /^\| `[^`]+` \|/ { name = $0; sub(/^\| `/, "", name); sub(/`.*/, "", name); print name }' "$1" | sort -u
}

# package_version MANIFEST: the version of MANIFEST's [package] table.
package_version() {
  awk '/^\[package\]/ { p = 1; next } /^\[/ { p = 0 }
    p && $1 == "version" { if (match($0, /"[^"]*"/)) { print substr($0, RSTART + 1, RLENGTH - 2); exit } }' "$1"
}

# planned_without_link PAGE: print "line: heading" for each section of PAGE
# that says "planned" and links no tracker issue or milestone.
planned_without_link() {
  awk -v tracker="$TRACKER" '
    function flush() { if (planned && !linked) print start ": " heading; planned = 0; linked = 0 }
    BEGIN { start = 1; heading = "(the page opening)" }
    /^```/ { fence = !fence; next }
    fence { next }
    /^#+ / { flush(); start = NR; heading = $0; next }
    { if (tolower($0) ~ /(^|[^a-z])planned([^a-z]|$)/) planned = 1
      if ($0 ~ tracker) linked = 1 }
    END { flush() }' "$1"
}

check() {
  local fail=0 manifest crate page missing extra version hits section
  for manifest in crates/*/Cargo.toml; do
    [[ -f "$manifest" ]] || continue
    crate="$(basename "$(dirname "$manifest")")"
    page="$BOOK/crates/$crate.md"
    if [[ ! -f "$page" ]]; then
      echo "::error file=$manifest::docs-claims: $crate has no book page; add $page with its Features table" >&2
      fail=1
      continue
    fi
    if ! grep -q "(${crate}\.md)" "$BOOK/crates/index.md" 2> /dev/null; then
      echo "::error file=$BOOK/crates/index.md::docs-claims: the crates page does not list $crate" >&2
      fail=1
    fi
    missing="$(comm -23 <(features "$manifest") <(documented "$page"))"
    extra="$(comm -13 <(features "$manifest") <(documented "$page"))"
    if [[ -n "$missing" ]]; then
      echo "::error file=$page::docs-claims: $page has no Features row for $(tr '\n' ' ' <<< "$missing")(the features of $manifest)" >&2
      fail=1
    fi
    if [[ -n "$extra" ]]; then
      echo "::error file=$page::docs-claims: $page documents $(tr '\n' ' ' <<< "$extra")which $manifest does not define" >&2
      fail=1
    fi
    version="$(package_version "$manifest")"
    if [[ -n "$version" ]]; then
      hits="$(grep -rnF -- "$version" "$BOOK" | grep -E "(^|[^0-9.])${version//./\\.}([^0-9]|$)" || true)"
      if [[ -n "$hits" ]]; then
        while IFS= read -r hit; do
          echo "::error::docs-claims: $hit types $crate's version $version; write {{version:$crate}}" >&2
        done <<< "$hits"
        fail=1
      fi
    fi
  done
  while IFS= read -r page; do
    while IFS= read -r section; do
      [[ -n "$section" ]] || continue
      echo "::error file=$page::docs-claims: $page:$section says planned and links no tracker issue or milestone" >&2
      fail=1
    done < <(planned_without_link "$page")
  done < <(find "$BOOK" -name '*.md' | sort)
  if [[ "$fail" -ne 0 ]]; then
    echo "docs-claims: FAILED (.claude/rules/docs-website.md)" >&2
    return 1
  fi
  echo "docs-claims: OK (crate pages, features, versions and planned work agree)"
}

# The fixtures are markdown, whose backticks are literal.
# shellcheck disable=SC2016
self_test() {
  local work failed=0 name want status
  work="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand $work now: the local is gone at EXIT.
  trap "rm -rf '$work'" EXIT
  script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"

  # fixture DIR: a passing repository in DIR.
  fixture() {
    local d=$1
    mkdir -p "$d/crates/demo" "$d/$BOOK/crates" "$d/$BOOK/operate"
    printf '[package]\nname = "demo"\nversion = "0.4.7"\n\n[features]\ndefault = []\nalpha = ["dep:x"]\nbeta = [\n  "alpha",\n]\n\n[dependencies]\n' > "$d/crates/demo/Cargo.toml"
    printf '# Crates\n\n- [demo](demo.md)\n' > "$d/$BOOK/crates/index.md"
    printf '# demo\n\nVersion {{version:demo}}.\n\n## Features\n\n| Feature | What |\n|---|---|\n| `alpha` | a |\n| `beta` | b |\n\n## Next\n' > "$d/$BOOK/crates/demo.md"
    printf '# Deployment\n\nThe image is planned in the [milestone](https://github.com/FerroHEALTH/FerroPIX/milestone/3).\n\n## Today\n\nThe crates.\n\n```sh\n# planned inside a fence is code\n```\n' > "$d/$BOOK/operate/deployment.md"
  }
  # expect NAME WANT: the guard over $work/NAME exits WANT.
  expect() {
    name=$1
    want=$2
    status=0
    bash "$script" --root "$work/$name" > "$work/$name.out" 2>&1 || status=$?
    if [[ "$status" -eq "$want" ]]; then
      echo "  ok: $name exits $want"
    else
      echo "  FAIL: $name exited $status, wanted $want" >&2
      sed 's/^/    /' "$work/$name.out" >&2
      failed=1
    fi
  }

  fixture "$work/sound"
  expect sound 0

  fixture "$work/missing-row"
  printf 'gamma = []\n' >> "$work/missing-row/crates/demo/Cargo.toml"
  perl -0pi -e 's/\[dependencies\]\n//; s/\ngamma = \[\]\n/\ngamma = []\n\n[dependencies]\n/' "$work/missing-row/crates/demo/Cargo.toml"
  expect missing-row 1

  fixture "$work/extra-row"
  perl -0pi -e 's/(\| `beta` \| b \|\n)/$1| `delta` | d |\n/' "$work/extra-row/$BOOK/crates/demo.md"
  expect extra-row 1

  fixture "$work/no-page"
  rm "$work/no-page/$BOOK/crates/demo.md"
  expect no-page 1

  fixture "$work/unlisted"
  printf '# Crates\n' > "$work/unlisted/$BOOK/crates/index.md"
  expect unlisted 1

  fixture "$work/typed-version"
  printf '\nAdd `demo = "=0.4.7"`.\n' >> "$work/typed-version/$BOOK/crates/demo.md"
  expect typed-version 1

  fixture "$work/planned-unlinked"
  printf '\n## Later\n\nThe server is planned.\n' >> "$work/planned-unlinked/$BOOK/operate/deployment.md"
  expect planned-unlinked 1

  [[ "$failed" -eq 0 ]] && echo "docs-claims self-test: OK"
  return "$failed"
}

root="$(dirname "${BASH_SOURCE[0]}")/../.."
case "${1:-}" in
  --self-test)
    self_test
    exit
    ;;
  --root)
    root="${2:?usage: docs-claims.sh --root DIR}"
    ;;
  "") ;;
  *)
    echo "usage: docs-claims.sh [--root DIR] | --self-test" >&2
    exit 2
    ;;
esac
cd "$root"
check
