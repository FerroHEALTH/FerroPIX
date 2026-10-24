#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# Stray-literal guard: a string a reader sees belongs in a catalog under
# locales/, never in the code (no specification governs this: our own
# design).
#
# It reads the hand-written .rs files under each ROOT, up to the file's
# `#[cfg(test)]` module, and reports every string literal that reads as
# words a reader sees:
#
#   1. the value of an attribute a reader sees or hears (`aria-label`,
#      `title`, `placeholder`, `alt`, `data-label`, `aria-description`,
#      `aria-roledescription`, `aria-valuetext`, `label`) holding a letter;
#   2. a text node of a `view!`: a literal between two tags (`>"Close"<`)
#      or alone on its line, holding a letter;
#   3. any other literal holding words: an upper-case letter, or a space
#      next to a letter, outside its `{}` placeables, and not a class list.
#
# A single lower-case word in code (`=> "fatal"`) reads as a key and is
# passed over, so a word drawn that way is review's to catch. Lines that
# speak to a developer are passed over: `expect(`, log macros (an event
# spread over several lines too, up to its closing `);`), `write!(f`,
# `f.write_str(`, `debug_struct(`, `.field(`, a lint's `reason`, assertions
# and attributes. A literal the check reads wrong is allowed by a line of the
# allow file, and a file the check does not cover is skipped there:
#
#   allow <extended regular expression matched against the literal> # the reason
#   skip <path or directory prefix, from the repository root> # the reason
#
# The baseline file holds `<count> <path>` lines for a tree whose strings
# are still moving to the catalogs: a file may hold up to its count and no
# more, and a file not named may hold none. With --update the script
# rewrites the baseline to the counts it finds, which only a change that
# moves strings to the catalogs should need.
#
# Usage:
#   scripts/checks/stray-literals.sh [--update] [ROOT...]
#   scripts/checks/stray-literals.sh --self-test
#
# With no ROOT it reads app/ferropix-console/src, the tree CI reads. The
# self-test runs a copy of the check over fixture trees and proves each
# refusal and each pass.
#
# The allow file is scripts/checks/stray-literals.allow and the baseline
# scripts/checks/stray-literals.baseline, both optional.
#
# Exit 0 = clean, 1 = a stray literal (listed as file:line: literal), 2 =
# a usage error.
set -Eeuo pipefail

self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# The self-test writes one fixture tree per case in a temporary directory,
# with a copy of this script under its scripts/checks/, and runs the copy
# there, so the allow file and the baseline it reads are the fixture's own.
self_test() {
  local work failures=0 n=0
  work="$(mktemp -d)"
  # case NAME WANT: runs the check in the fixture tree $work/NAME over its
  # src/ and fails the self-test unless it exits WANT.
  case_() {
    local name="$1" want="$2" status=0
    n=$((n + 1))
    mkdir -p "$work/$name/scripts/checks"
    cp "$self" "$work/$name/scripts/checks/stray-literals.sh"
    (cd "$work/$name" && bash scripts/checks/stray-literals.sh src > out 2>&1) || status=$?
    if [ "$status" -ne "$want" ]; then
      echo "stray-literals: self-test failed: $name exited $status, wanted $want:" >&2
      cat "$work/$name/out" >&2
      failures=$((failures + 1))
    fi
  }
  # fixture NAME FILE TEXT: writes TEXT into FILE of the fixture tree NAME.
  fixture() {
    mkdir -p "$(dirname "$work/$1/$2")"
    printf '%s\n' "$3" > "$work/$1/$2"
  }

  fixture text-node src/page.rs '    view! { <h1>"Patients"</h1> }'
  case_ text-node 1
  fixture attribute src/page.rs '    view! { <input aria-label="search"/> }'
  case_ attribute 1
  fixture sentence src/page.rs '    let words = format!("the server did not answer");'
  case_ sentence 1
  fixture lone src/page.rs '        "Try again"'
  case_ lone 1
  fixture key src/page.rs '    let key = "fatal";'
  case_ key 0
  fixture classes src/page.rs '    view! { <div class="fui-card fui-card-flat"></div> }'
  case_ classes 0
  fixture lookup src/page.rs '    view! { <h1>{tr(Msg::Patients)}</h1> }'
  case_ lookup 0
  fixture event src/page.rs "$(printf '%s\n' '    tracing::warn!(' '        method = %request.method(),' '        "a request was refused"' '    );')"
  case_ event 0
  fixture after-event src/page.rs "$(printf '%s\n' '    tracing::warn!(' '        "a request was refused"' '    );' '    view! { <p>"Refused"</p> }')"
  case_ after-event 1
  fixture tests src/page.rs "$(printf '%s\n' '#[cfg(test)]' 'mod tests {' '    const WORDS: &str = "Page not found";' '}')"
  case_ tests 0
  fixture allowed src/page.rs '    let line = "GET /operator/overview";'
  fixture allowed scripts/checks/stray-literals.allow 'allow ^GET /   # an HTTP request line'
  case_ allowed 0
  fixture skipped src/terminal.rs '    eprintln!("ferropix-console: cannot start");'
  fixture skipped scripts/checks/stray-literals.allow 'skip src/terminal.rs   # the terminal'
  case_ skipped 0
  fixture unreadable src/page.rs '    let key = "fatal";'
  fixture unreadable scripts/checks/stray-literals.allow 'permit everything'
  case_ unreadable 2
  fixture within src/page.rs '    view! { <h1>"Patients"</h1> }'
  fixture within scripts/checks/stray-literals.baseline '1 src/page.rs'
  case_ within 0
  fixture over src/page.rs "$(printf '%s\n' '    view! { <h1>"Patients"</h1> }' '    view! { <h2>"Sources"</h2> }')"
  fixture over scripts/checks/stray-literals.baseline '1 src/page.rs'
  case_ over 1

  find "$work" -type f -delete
  find "$work" -depth -type d -exec rmdir {} +
  if [ "$failures" -ne 0 ]; then
    echo "stray-literals: self-test: $failures of $n cases failed" >&2
    exit 1
  fi
  echo "stray-literals: self-test OK ($n cases)."
}

if [ "${1:-}" = "--self-test" ]; then
  self_test
  exit 0
fi

update=0
if [ "${1:-}" = "--update" ]; then
  update=1
  shift
fi
if [ "$#" -eq 0 ]; then
  set -- app/ferropix-console/src
fi
for dir in "$@"; do
  if [ ! -d "$dir" ]; then
    echo "stray-literals: $dir is no directory" >&2
    exit 2
  fi
done

allow="scripts/checks/stray-literals.allow"
baseline="scripts/checks/stray-literals.baseline"

files=()
while IFS= read -r file; do
  files+=("$file")
done < <(find "$@" -type f -name '*.rs' | LC_ALL=C sort)
if [ "${#files[@]}" -eq 0 ]; then
  echo "stray-literals: no .rs file under $*"
  exit 0
fi

found="$(
  awk -v allowfile="$allow" '
    function wordy(t,    bare, n, i, toks, plain) {
      bare = t
      gsub(/\{[^{}]*\}/, "", bare)
      if (bare !~ /[A-Za-z]/) return 0
      n = split(bare, toks, /[ \t]+/)
      plain = 1
      for (i = 1; i <= n; i++) if (toks[i] != "" && toks[i] !~ /^fui-[a-z0-9-]+$/) plain = 0
      if (plain) return 0
      if (bare ~ /[A-Z]/) return 1
      if (bare ~ /[A-Za-z][ \t]/ || bare ~ /[ \t][A-Za-z]/) return 1
      return 0
    }
    function classes(t,    n, i, toks) {
      n = split(t, toks, /[ \t]+/)
      for (i = 1; i <= n; i++) if (toks[i] != "" && toks[i] !~ /^fui-[a-z0-9-]+$/) return 0
      return 1
    }
    BEGIN {
      nallow = 0
      nskip = 0
      while ((getline line < allowfile) > 0) {
        sub(/[ \t]+#.*$/, "", line)
        if (line ~ /^[ \t]*(#|$)/) continue
        if (line ~ /^allow[ \t]+/) {
          sub(/^allow[ \t]+/, "", line)
          sub(/[ \t]+$/, "", line)
          allow[++nallow] = line
        } else if (line ~ /^skip[ \t]+/) {
          sub(/^skip[ \t]+/, "", line)
          sub(/[ \t]+$/, "", line)
          skip[++nskip] = line
        } else {
          print "stray-literals: " allowfile ": unreadable line: " line > "/dev/stderr"
          exit 2
        }
      }
      visible = "(^|[^A-Za-z-])(aria-label|title|placeholder|alt|data-label|aria-description|aria-roledescription|aria-valuetext|label)[ \t]*=[ \t]*$"
    }
    FNR == 1 {
      off = 0
      inlog = 0
      for (i = 1; i <= nskip; i++) if (index(FILENAME, skip[i]) == 1) off = 1
      if ($0 ~ /@generated/) off = 1
    }
    off { next }
    /^#\[cfg\(test\)\]/ { off = 1; next }
    inlog { if ($0 ~ /^[ \t]*\);/) inlog = 0; next }
    /(warn|error|debug|info|trace|log)!\([ \t]*$/ { inlog = 1; next }
    /^[ \t]*\/\// { next }
    /^[ \t]*#!?\[/ { next }
    /reason = "|expect\(|logging::|warn!\(|error!\(|debug!\(|info!\(|trace!\(|log!\(|write!\(f|write_str\(|debug_struct\(|\.field\(|assert|panic!\(|unreachable!\(|include_str!\(/ { next }
    {
      code = $0
      while ((s = index(code, "r#\"")) > 0) {
        rest = substr(code, s + 3)
        e = index(rest, "\"#")
        if (e == 0) { code = substr(code, 1, s - 1); break }
        code = substr(code, 1, s - 1) substr(rest, e + 2)
      }
      n = length(code)
      i = 1
      while (i <= n) {
        c = substr(code, i, 1)
        if (c == "'"'"'" && substr(code, i + 2, 1) == "'"'"'") { i += 3; continue }
        if (c != "\"") { i++; continue }
        start = i
        lit = ""
        i++
        while (i <= n) {
          c = substr(code, i, 1)
          if (c == "\\") { lit = lit substr(code, i, 2); i += 2; continue }
          if (c == "\"") break
          lit = lit c
          i++
        }
        pre = substr(code, 1, start - 1)
        after = substr(code, i + 1)
        i++
        hit = 0
        if (pre ~ visible) {
          hit = lit ~ /[A-Za-z]/
        } else if (pre ~ /[A-Za-z:-]=$/) {
          hit = 0
        } else if (pre ~ />[ \t]*$/ && after ~ /^[ \t]*</) {
          hit = lit ~ /[A-Za-z]/
        } else if (pre ~ /^[ \t]*$/ && after ~ /^[ \t]*$/) {
          hit = lit ~ /[A-Za-z]/ && !classes(lit)
        } else {
          hit = wordy(lit)
        }
        if (!hit) continue
        allowed = 0
        for (a = 1; a <= nallow; a++) if (lit ~ allow[a]) allowed = 1
        if (!allowed) print FILENAME ":" FNR ": \"" lit "\""
      }
    }
  ' ${files[@]+"${files[@]}"}
)"

counts="$(printf '%s\n' "$found" | sed -n 's/^\([^:]*\):[0-9]*:.*/\1/p' | LC_ALL=C sort | uniq -c | awk '{print $1, $2}')"

if [ "$update" -eq 1 ]; then
  {
    printf '# %s: %s\n' "SPDX-FileCopyrightText" "Cadasto B.V."
    printf '# %s: %s\n' "SPDX-License-Identifier" "BUSL-1.1"
    echo "# The stray literals each file may still hold while its strings move to"
    echo "# the catalogs; rewritten by scripts/checks/stray-literals.sh --update."
    if [ -n "$counts" ]; then printf '%s\n' "$counts"; fi
  } > "$baseline"
  echo "stray-literals: wrote $baseline"
  exit 0
fi

status=0
while IFS= read -r entry; do
  [ -n "$entry" ] || continue
  count="${entry%% *}"
  file="${entry#* }"
  allowed=0
  if [ -f "$baseline" ]; then
    allowed="$(awk -v f="$file" '$2 == f { print $1 }' "$baseline")"
    allowed="${allowed:-0}"
  fi
  if [ "$count" -gt "$allowed" ]; then
    status=1
    echo "stray-literals: $file holds $count stray literal(s), $allowed allowed:" >&2
    printf '%s\n' "$found" | awk -v f="$file:" 'index($0, f) == 1' >&2
  fi
done <<< "$counts"

if [ "$status" -ne 0 ]; then
  echo "stray-literals: move each string into the catalog under locales/ and look it up, or allow a literal that is no reader's text in $allow with the reason." >&2
  exit 1
fi
echo "stray-literals: no stray literal under $*"
