#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# assemble.sh: build the site the way GitHub Pages serves it, from the landing
# page and the book, into one directory (no specification governs this: our
# own design).
#
#   scripts/site/assemble.sh OUT
#
# The landing page lands at the site root and the book under /docs/, which is
# what book.toml's site-url declares. Four things are rendered on the way:
#
#   * the book's version markers, from the manifests, so a page never states
#     a version the tree has moved past: {{version:<crate>}} is the version
#     of crates/<crate>/Cargo.toml, {{version:ferropix}} the product version
#     of the root Cargo.toml, and {{dep:<name>}} the requirement the root
#     [workspace.dependencies] gives <name>. A marker that names nothing
#     fails the assembly.
#   * the favicon: mdBook reads it from the book's theme directory, so the
#     book is built from a staged copy that adds favicon.svg and
#     favicon-32.png from assets/brand/, and the repository keeps one copy
#     of the artwork.
#   * each page of the book gets its own hashed Content-Security-Policy
#     (scripts/site/book-csp.sh).
#   * the landing page's roadmap block (between `<!-- roadmap:begin -->` and
#     `<!-- roadmap:end -->`), from the tracker's open milestones through the
#     GitHub API when `gh` is authenticated. Without a token the block stays
#     empty and the page keeps its link to the milestones page, so an
#     assembly on a laptop never fails on the network. SITE_ROADMAP=off
#     leaves it empty without asking, which is how the offline link check
#     (scripts/checks/site-links.sh) assembles the site.
#
# The script leaves the source tree untouched: the book is staged and built
# in temporary directories.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

readonly OUT="${1:?usage: $0 OUT}"
readonly LANDING=website/landing
readonly REPO=FerroHEALTH/FerroPIX

# shellcheck source=scripts/site/toolchain.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. scripts/site/toolchain.sh
if ! command -v mdbook > /dev/null 2>&1; then
  echo "assemble: mdbook not found; install mdbook $MDBOOK_VERSION (docs/VERSIONS.md)" >&2
  exit 1
fi
mdbook_seen="$(mdbook --version)"
if [[ "$mdbook_seen" != "mdbook v$MDBOOK_VERSION" ]]; then
  echo "assemble: found $mdbook_seen, but the book is pinned to mdbook v$MDBOOK_VERSION (docs/VERSIONS.md)" >&2
  exit 1
fi

# package_version MANIFEST: the version of the [package] or
# [workspace.package] table of MANIFEST.
package_version() {
  awk '/^\[(workspace\.)?package\]/ { p = 1; next } /^\[/ { p = 0 }
    p && $1 == "version" { if (match($0, /"[^"]*"/)) { print substr($0, RSTART + 1, RLENGTH - 2); exit } }' "$1"
}

# workspace_requirement NAME: the version requirement the root manifest's
# [workspace.dependencies] gives NAME.
workspace_requirement() {
  awk -v name="$1" '/^\[workspace\.dependencies\]/ { p = 1; next } /^\[/ { p = 0 }
    p && $1 == name { if (match($0, /version = "[^"]*"/)) { print substr($0, RSTART + 11, RLENGTH - 12); exit }
                      else if (match($0, /"[^"]*"/)) { print substr($0, RSTART + 1, RLENGTH - 2); exit } }' Cargo.toml
}

# render FILE: replace every version marker of FILE in place, and fail
# naming a marker no manifest answers.
render() {
  local file=$1 marker kind name value
  while IFS= read -r marker; do
    kind="${marker#\{\{}"
    kind="${kind%%:*}"
    name="${marker#*:}"
    name="${name%\}\}}"
    value=""
    case "$kind" in
      version)
        if [[ "$name" == ferropix ]]; then
          value="$(package_version Cargo.toml)"
        elif [[ -f "crates/$name/Cargo.toml" ]]; then
          value="$(package_version "crates/$name/Cargo.toml")"
        fi
        ;;
      dep) value="$(workspace_requirement "$name")" ;;
      *) ;;
    esac
    if [[ -z "$value" ]]; then
      echo "assemble: $file: $marker names no manifest entry" >&2
      return 1
    fi
    MARKER="$marker" VALUE="$value" perl -pi -e 's/\Q$ENV{MARKER}\E/$ENV{VALUE}/g' "$file"
  done < <(grep -o -E '\{\{(version|dep):[A-Za-z0-9_-]+\}\}' "$file" | sort -u)
}

# The open milestones as list items, oldest due date first: the name and a
# link to the milestone, never its issue counts, which go stale the moment an
# issue moves and the site is only redeployed from main. Every field that
# reaches the page is HTML-escaped, because a milestone title is text somebody
# typed.
roadmap() {
  gh api "repos/$REPO/milestones?state=open&sort=due_on&direction=asc" --jq '
    def esc: gsub("&"; "&amp;") | gsub("<"; "&lt;") | gsub(">"; "&gt;");
    ["<ul class=\"roadmap\">"]
    + map("  <li><a href=\"" + .html_url + "\" rel=\"noopener\">" + (.title | esc) + "</a>"
          + (if .description == null or .description == "" then "" else "<span class=\"theme\">" + (.description | split(":")[0] | esc) + "</span>" end)
          + "</li>")
    + ["</ul>"] | .[]'
}

stage="$(mktemp -d)"
book="$(mktemp -d)"
block="$(mktemp)"
trap 'rm -rf "$stage" "$book"; rm -f "$block"' EXIT

# The staged tree mirrors the repository's layout, so the book's
# {{#include}} of CHANGELOG.md and docs/VERSIONS.md resolves as it does in
# the checkout.
mkdir -p "$stage/website" "$stage/docs"
cp -R website/book "$stage/website/book"
rm -rf "$stage/website/book/book"
cp CHANGELOG.md "$stage/"
cp docs/VERSIONS.md "$stage/docs/"
mkdir -p "$stage/website/book/theme"
cp assets/brand/favicon.svg "$stage/website/book/theme/favicon.svg"
cp assets/brand/favicon-32.png "$stage/website/book/theme/favicon.png"
while IFS= read -r page; do
  render "$page"
done < <(find "$stage/website/book/src" -name '*.md')

mdbook build "$stage/website/book" --dest-dir "$book"
scripts/site/book-csp.sh "$book"

rm -rf "${OUT:?}"
mkdir -p "$OUT/docs"
cp -R "$LANDING"/. "$OUT/"
cp -R "$book"/. "$OUT/docs/"

# The brand directory holds the favicons and the marks the landing page and
# the book's stylesheet address from the site root.
mkdir -p "$OUT/assets"
cp -R assets/brand "$OUT/assets/"

if [[ "${SITE_ROADMAP:-on}" == off ]]; then
  echo "assemble: SITE_ROADMAP=off, so the roadmap block stays empty."
elif roadmap > "$block" 2> /dev/null && [[ -s "$block" ]]; then
  awk -v blockfile="$block" '
    /<!-- roadmap:begin -->/ { print; while ((getline line < blockfile) > 0) print line; skip = 1; next }
    /<!-- roadmap:end -->/ { skip = 0 }
    !skip { print }
  ' "$LANDING/index.html" > "$OUT/index.html"
  echo "assemble: roadmap rendered from the open milestones."
else
  echo "assemble: no GitHub API access, so the roadmap block stays empty." >&2
fi

echo "assemble: site at $OUT"
