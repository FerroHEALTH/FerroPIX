#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# Measures the operator console's release site bundle (app/ferropix-console)
# and fails when its brotli-compressed entry WebAssembly is over its byte
# budget (no specification governs this: our own design, after FerroBRIDGE's
# guard of the same name).
#
# It reads the bundle scripts/release/console-site.sh --release writes, prints
# the entry WebAssembly and its JavaScript glue raw, gzip-compressed (gzip -9)
# and brotli-compressed (brotli -q 11) as a Markdown table, with the lazy
# chunks a split build writes beside them (reported, never gated), and
# appends the table to $GITHUB_STEP_SUMMARY when that is set. The budget is
# read from the budget table of .claude/rules/leptos-ui.md §4, beside its
# reasoning, so the number lives in one place.
#
# Usage:
#   scripts/checks/console-bundle.sh [pkg-dir]   pkg-dir defaults to target/site/pkg
#   scripts/checks/console-bundle.sh --self-test prove the refusals and the
#                                                pass over fixture bundles
#
# Exit 0 = within the budget. Exit 1 = over the budget, a bundle file missing,
# a compressor missing, or the budget unreadable. Exit 2 = a usage error.
set -Eeuo pipefail

self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
rule="$root/.claude/rules/leptos-ui.md"
name="ferropix-console"

# The self-test writes one fixture tree per case, a rule file with a budget
# and a bundle, with a copy of this script under its scripts/checks/, and
# runs the copy there.
self_test() {
  local work failures=0 n=0
  work="$(mktemp -d)"
  # case_ NAME BUDGET WASM-BYTES WANT: a tree whose rule names BUDGET (or no
  # budget row when it is `-`) and whose bundle holds WASM-BYTES of random
  # bytes (no bundle when it is `-`); fails the self-test unless the check
  # exits WANT.
  case_() {
    local label="$1" budget="$2" bytes="$3" want="$4" tree status=0
    n=$((n + 1))
    tree="$work/$label"
    mkdir -p "$tree/scripts/checks" "$tree/.claude/rules" "$tree/target/site/pkg"
    cp "$self" "$tree/scripts/checks/console-bundle.sh"
    if [ "$budget" = "-" ]; then
      printf '| Measure | Budget |\n|---|---|\n' > "$tree/.claude/rules/leptos-ui.md"
    else
      printf '| Measure | Budget |\n|---|---|\n| Entry WebAssembly, brotli-compressed | %s bytes |\n' \
        "$budget" > "$tree/.claude/rules/leptos-ui.md"
    fi
    if [ "$bytes" != "-" ]; then
      head -c "$bytes" /dev/urandom > "$tree/target/site/pkg/$name.wasm"
      printf 'export default function init() {}\n' > "$tree/target/site/pkg/$name.js"
      head -c 512 /dev/urandom > "$tree/target/site/pkg/split_lazy.wasm"
    fi
    (cd "$tree" && GITHUB_STEP_SUMMARY='' bash scripts/checks/console-bundle.sh > out 2>&1) || status=$?
    if [ "$status" -ne "$want" ]; then
      echo "console-bundle: self-test failed: $label exited $status, wanted $want:" >&2
      cat "$tree/out" >&2
      failures=$((failures + 1))
    fi
  }

  case_ within 100000 4096 0
  case_ over 1024 4096 1
  case_ no-budget - 4096 1
  case_ no-bundle 100000 - 1

  find "$work" -type f -delete
  find "$work" -depth -type d -exec rmdir {} +
  if [ "$failures" -ne 0 ]; then
    echo "console-bundle: self-test: $failures of $n cases failed" >&2
    exit 1
  fi
  echo "console-bundle: self-test OK ($n cases)."
}

case "$#" in
  0) pkg="$root/target/site/pkg" ;;
  1)
    case "$1" in
      --self-test)
        self_test
        exit 0
        ;;
      -*)
        echo "usage: scripts/checks/console-bundle.sh [pkg-dir | --self-test]" >&2
        exit 2
        ;;
      *) pkg="$1" ;;
    esac
    ;;
  *)
    echo "usage: scripts/checks/console-bundle.sh [pkg-dir | --self-test]" >&2
    exit 2
    ;;
esac

for tool in gzip brotli; do
  if ! command -v "$tool" > /dev/null; then
    echo "console-bundle: $tool is not on PATH; the bundle cannot be measured." >&2
    exit 1
  fi
done

for file in "$name.wasm" "$name.js"; do
  if [ ! -s "$pkg/$file" ]; then
    echo "console-bundle: $pkg/$file is missing; run scripts/release/console-site.sh --release first." >&2
    exit 1
  fi
done

# The one row of the budget table whose first cell names the gating measure.
budget="$(awk -F'|' '
  $2 ~ /^[[:space:]]*Entry WebAssembly, brotli-compressed[[:space:]]*$/ {
    value = $3
    sub(/^[[:space:]]*/, "", value)
    sub(/[[:space:]]*bytes.*$/, "", value)
    print value
  }
' "$rule")"
if ! [[ "$budget" =~ ^[0-9]+$ ]]; then
  echo "console-bundle: $rule has no single \"| Entry WebAssembly, brotli-compressed | N bytes |\" budget row." >&2
  exit 1
fi

raw() { wc -c < "$1" | tr -d '[:space:]'; }
gz() { gzip -9 -c "$1" | wc -c | tr -d '[:space:]'; }
br() { brotli -q 11 -c "$1" | wc -c | tr -d '[:space:]'; }

wasm="$pkg/$name.wasm"
js="$pkg/$name.js"
wasm_br="$(br "$wasm")"
percent=$((wasm_br * 100 / budget))

# The chunks a split build writes beside the entry: each lazy island's own,
# loaded only on a page that draws it, reported and never gated.
chunks_raw=0
chunks_br=0
shopt -s nullglob
for chunk in "$pkg"/split_*.wasm "$pkg"/chunk_*.wasm; do
  chunks_raw=$((chunks_raw + $(raw "$chunk")))
  chunks_br=$((chunks_br + $(br "$chunk")))
done
shopt -u nullglob

table="$(
  echo "### Operator console bundle"
  echo
  echo "| File | Raw | gzip -9 | brotli -q 11 |"
  echo "|---|---:|---:|---:|"
  echo "| \`$name.wasm\` | $(raw "$wasm") | $(gz "$wasm") | $wasm_br |"
  echo "| \`$name.js\` | $(raw "$js") | $(gz "$js") | $(br "$js") |"
  echo "| lazy chunks | $chunks_raw | | $chunks_br |"
  echo
  echo "Sizes in bytes. Budget: $budget bytes of brotli-compressed entry WebAssembly; this build is $wasm_br bytes, $percent% of it."
)"

echo "$table"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  echo "$table" >> "$GITHUB_STEP_SUMMARY"
fi

if [ "$wasm_br" -gt "$budget" ]; then
  echo "::error file=.claude/rules/leptos-ui.md::The brotli-compressed entry WebAssembly is $wasm_br bytes, over the $budget-byte budget of .claude/rules/leptos-ui.md §4. Shrink the bundle, or raise the budget there with the reason." >&2
  exit 1
fi
echo "console-bundle: $wasm_br of $budget bytes, within the budget"
