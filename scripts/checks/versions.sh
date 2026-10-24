#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# The pin and licence guard (no specification governs this: our own design).
#
#   1. Toolchain: the toolchain, MSRV and edition rows of docs/VERSIONS.md
#      agree with rust-toolchain.toml, the root Cargo.toml and rustfmt.toml,
#      each checked once the file exists.
#   2. Corpora: every tree under docs/specs/ carries a PROVENANCE.md and is
#      named in a row of the corpus table of docs/VERSIONS.md.
#   3. Tool pins: each row REPEATED_ROWS names is carried word for word by
#      every file its "Repeated in" cell names.
#   4. Licence: every tracked first-party file names Cadasto B.V. in an
#      `SPDX-FileCopyrightText` line within its first 25 lines, and no
#      header there claims a licence other than BUSL-1.1. LICENSE, NOTICE, Cargo.lock, JSON (which has no comment
#      syntax), the raster images (PNG and ICO, which have none either) and
#      test fixtures carry no header.
#
# Usage:
#   scripts/checks/versions.sh [--root DIR]   check this repository, or DIR
#   scripts/checks/versions.sh --self-test    prove each refusal on fixtures
#
# Exit 0 when everything agrees, 1 on drift, 2 on a usage error.
set -euo pipefail
self="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
root="$(dirname "$0")/../.."

# The pin of the docs/VERSIONS.md row whose first cell is $1, backticks off.
pin() {
  awk -F'|' -v item="$1" '{ gsub(/^[[:space:]]+|[[:space:]]+$/, "", $2) }
    $2 == item { v = $3; gsub(/[[:space:]`]/, "", v); print v; exit }' docs/VERSIONS.md
}

# The rows whose pin each file their "Repeated in" cell names must carry
# word for word: the build and test tools a workflow or a script installs.
readonly REPEATED_ROWS=(
  "cargo-leptos"
  "wasm-bindgen"
  "Binaryen"
  "thirtyfour"
  "Chrome for Testing (Chrome and chromedriver)"
  "Chrome for Testing, \`chrome-linux64.zip\` SHA-256"
  "Chrome for Testing, \`chrome-mac-arm64.zip\` SHA-256"
  "Chrome for Testing, \`chromedriver-linux64.zip\` SHA-256"
  "Chrome for Testing, \`chromedriver-mac-arm64.zip\` SHA-256"
)

# Every backticked path of the "Repeated in" cell of the row whose first
# cell is $1, one per line.
repeats() {
  awk -F'|' -v item="$1" '{ gsub(/^[[:space:]]+|[[:space:]]+$/, "", $2) }
    $2 == item { n = split($4, part, "`"); for (i = 2; i <= n; i += 2) print part[i]; exit }' docs/VERSIONS.md
}

# The value of key $2 in the TOML file $1, read as a quoted string.
toml_value() {
  awk -v key="$2" '$1 == key { if (match($0, /"[^"]*"/)) { print substr($0, RSTART + 1, RLENGTH - 2); exit } }' "$1"
}

# The self-test builds one fixture repository per case and runs the guard
# over it.
self_test() {
  local work n=0 failed=0 d
  work="$(mktemp -d)"
  # case_dir: a fresh, passing fixture repository in $d.
  case_dir() {
    n=$((n + 1))
    d="$work/$n"
    mkdir -p "$d/docs"
    git -C "$d" init -q
    # shellcheck disable=SC2016 # the backticks are Markdown in the fixture
    printf '<!-- SPDX-FileCopyrightText: Cadasto B.V. -->\n| Item | Pin |\n|---|---|\n| Rust toolchain | `1.98.1` |\n| Minimum supported Rust version | `1.97` |\n| Edition | `2024` |\n' > "$d/docs/VERSIONS.md"
    printf '# SPDX-FileCopyrightText: Cadasto B.V.\n# SPDX-License-Identifier: BUSL-1.1\n[toolchain]\nchannel = "1.98.1"\n' > "$d/rust-toolchain.toml"
    printf '# SPDX-FileCopyrightText: Cadasto B.V.\n# SPDX-License-Identifier: BUSL-1.1\n[workspace.package]\nedition = "2024"\nrust-version = "1.97"\n' > "$d/Cargo.toml"
    printf 'MIT text\n' > "$d/LICENSE"
    git -C "$d" add -A
  }
  # expect WANT NAME: the guard over $d exits WANT.
  expect() {
    local status=0
    git -C "$d" add -A
    bash "$self" --root "$d" > /dev/null 2>&1 || status=$?
    if [[ "$status" -ne "$1" ]]; then
      echo "versions: self-test failed: $2 exited $status, wanted $1." >&2
      failed=1
    fi
  }
  case_dir
  expect 0 "a repository that agrees"
  case_dir
  sed -i.bak 's/1.98.1/1.98.0/' "$d/rust-toolchain.toml" && rm "$d/rust-toolchain.toml.bak"
  expect 1 "a toolchain behind its pin"
  case_dir
  sed -i.bak 's/"1.97"/"1.96"/' "$d/Cargo.toml" && rm "$d/Cargo.toml.bak"
  expect 1 "an MSRV off its pin"
  case_dir
  printf 'edition = "2021"\n' > "$d/rustfmt.toml"
  expect 1 "a rustfmt edition off its pin"
  case_dir
  printf 'echo hi\n' > "$d/run.sh"
  expect 1 "a first-party file without the header"
  case_dir
  printf '# SPDX-FileCopyrightText: Cadasto B.V.\n# SPDX-License-Identifier: MIT\n' > "$d/run.sh"
  expect 1 "a first-party file claiming another licence"
  case_dir
  mkdir -p "$d/docs/specs/ihe-pixm"
  printf 'upstream text\nSPDX-License-Identifier: CC-BY-4.0\n' > "$d/docs/specs/ihe-pixm/page.md"
  expect 1 "a vendored tree without a provenance"
  printf 'provenance\n' > "$d/docs/specs/ihe-pixm/PROVENANCE.md"
  expect 1 "a vendored tree without a row"
  # shellcheck disable=SC2016 # the backticks are Markdown in the fixture
  printf '| IHE PIXm package | 3.1.0 | `docs/specs/ihe-pixm/PROVENANCE.md` |\n' >> "$d/docs/VERSIONS.md"
  expect 0 "a vendored tree with its provenance and its row"
  case_dir
  mkdir -p "$d/scripts"
  # shellcheck disable=SC2016 # the backticks are Markdown in the fixture
  printf '| Binaryen | version_133 | `scripts/tools.sh` |\n' >> "$d/docs/VERSIONS.md"
  printf '# SPDX-FileCopyrightText: Cadasto B.V.\n# SPDX-License-Identifier: BUSL-1.1\nversion="version_133"\n' > "$d/scripts/tools.sh"
  expect 0 "a tool pin its script repeats"
  sed -i.bak 's/version_133/version_132/' "$d/scripts/tools.sh" && rm "$d/scripts/tools.sh.bak"
  expect 1 "a tool pin its script does not repeat"
  rm -r "$work"
  if [[ "$failed" -ne 0 ]]; then
    exit 1
  fi
  echo "versions: self-test OK ($n fixtures)."
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --self-test)
      self_test
      exit 0
      ;;
    --root)
      root="${2:?usage: versions.sh --root DIR}"
      shift 2
      ;;
    *)
      echo "usage: versions.sh [--root DIR] | --self-test" >&2
      exit 2
      ;;
  esac
done
cd "$root"

fail=0
drift() {
  echo "  DRIFT: $*" >&2
  fail=1
}

[[ -f docs/VERSIONS.md ]] || {
  echo "versions: docs/VERSIONS.md is missing" >&2
  exit 1
}

# 1. Toolchain.
if [[ -f rust-toolchain.toml ]]; then
  want="$(pin "Rust toolchain")"
  got="$(toml_value rust-toolchain.toml channel)"
  [[ "$got" == "$want" ]] || drift "rust-toolchain.toml pins $got, docs/VERSIONS.md $want"
fi
if [[ -f Cargo.toml ]]; then
  want="$(pin "Minimum supported Rust version")"
  got="$(toml_value Cargo.toml rust-version)"
  [[ "$got" == "$want" ]] || drift "Cargo.toml rust-version is ${got:-unset}, docs/VERSIONS.md $want"
  want="$(pin Edition)"
  got="$(toml_value Cargo.toml edition)"
  [[ "$got" == "$want" ]] || drift "Cargo.toml edition is ${got:-unset}, docs/VERSIONS.md $want"
fi
if [[ -f rustfmt.toml ]]; then
  want="$(pin Edition)"
  got="$(toml_value rustfmt.toml edition)"
  [[ "$got" == "$want" ]] || drift "rustfmt.toml edition is ${got:-unset}, docs/VERSIONS.md $want"
fi

# 2. Corpora.
if [[ -d docs/specs ]]; then
  for tree in docs/specs/*/; do
    tree="${tree%/}"
    [[ -d "$tree" ]] || continue
    [[ -f "$tree/PROVENANCE.md" ]] || drift "$tree has no PROVENANCE.md"
    grep -qF "\`$tree/" docs/VERSIONS.md || drift "$tree is named in no row of docs/VERSIONS.md"
  done
fi

# 3. The tool pins, each in every file its row names.
for item in "${REPEATED_ROWS[@]}"; do
  want="$(pin "$item")"
  [[ -n "$want" ]] || continue
  while IFS= read -r file; do
    [[ -f "$file" ]] || continue
    grep -qF -- "$want" "$file" || drift "$file does not carry the $item pin $want of docs/VERSIONS.md"
  done < <(repeats "$item")
done

# 4. Licence.
while IFS= read -r file; do
  case "$file" in
    LICENSE | */LICENSE | NOTICE | Cargo.lock | *.json | *.png | *.ico | docs/specs/* | */testdata/* | */fixtures/*) continue ;;
    *) ;;
  esac
  [[ -f "$file" ]] || continue
  head -n 25 "$file" | grep -q 'SPDX-FileCopyrightText: Cadasto B.V.' ||
    drift "$file carries no SPDX-FileCopyrightText: Cadasto B.V. header"
  if head -n 25 "$file" | grep 'SPDX-License-Identifier:' | grep -qv 'BUSL-1.1'; then
    drift "$file claims a licence other than BUSL-1.1"
  fi
done < <(git ls-files)

if [[ "$fail" -ne 0 ]]; then
  echo "versions: drift found; docs/VERSIONS.md is the source of truth" >&2
  exit 1
fi
echo "versions: OK (toolchain, corpora, tool pins and licence headers agree)"
