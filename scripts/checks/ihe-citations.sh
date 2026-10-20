#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# The IHE citation guard: every IHE section a tracked file under crates/,
# app/, tools/, docs/ (the vendored corpora aside), conformance/ or fuzz/
# cites exists in the vendored text of that profile and version (no
# specification governs this: our own design). A citation that names a
# section the pinned page does not hold fails, naming the file and line.
#
# The citation grammar, read from the citations the tree carries:
#
#   §V:S           a section of an IHE implementation guide, by volume and
#                  section (§2:3.83.4.2.2.1, §1:41.4.1, §3:5.7.5.4).
#   T §S           a Technical Framework section behind a designator T:
#                  `TF-1` (Volume 1), `TF-2` or `ITI-n` (Volume 2), so
#                  ITI TF-1 §27.4.1, ITI TF-2 §3.20.4.1.1 and ITI-72 §3.72.4.3.
#   §3.n.S, §c.S   a bare Volume 2 transaction section (n of 20 or more) and a
#                  bare Volume 1 chapter (c of 19 or more): numbers the
#                  federation specification, whose sections stop at 18, and
#                  the RFCs and AQL, which are refused by name, do not reach.
#
# A designator before the § is held to the page as well: `PIXm`, `PDQm`,
# `PMIR`, `mCSD` or `BALP`, with or without a version, must name the
# profile and the pinned version of the page that holds the section, and
# ITI-n must name the transaction the section belongs to.
#
# The sections are the numbered headings of every vendored IHE narrative text
# under docs/specs/ihe-*: the implementation guide pages, the Technical
# Framework pages, the IUA supplement and the RESTful ATNA supplement, the
# last read with pdftotext. A section is present when it is a heading or the
# parent of one. A text whose licence keeps it out of the repository is
# fetched into .vendor-cache/ihe-* by its vendor script, and is read from
# there when a local run has fetched it. A transaction no text at hand
# carries is listed in UNVENDORED with the reason; its citations are counted,
# never checked.
#
# Usage:
#   scripts/checks/ihe-citations.sh              check the tracked tree
#   scripts/checks/ihe-citations.sh --self-test  prove each refusal and pass
# Needs pdftotext (poppler) to read a vendored text that is a PDF. Without it,
# outside CI (CI unset), the guard prints a SKIP on stderr naming each PDF it
# could not read, and counts a citation no text it read holds as not checked,
# as it does for a cache-only text; a citation a text it read holds is still
# held to its designator. In CI, which installs poppler, a PDF left unread
# fails the run. Exit 1 naming each citation the vendored text does not hold;
# 0 otherwise.
set -euo pipefail
cd "$(dirname "$0")/../.."

# The cited sections no committed text carries, one per line: the section
# root, a tab, and why it is not checked. A root the cache holds is checked.
readonly UNVENDORED='2:3.55	ITI TF-2 §3.55 Cross Gateway Patient Discovery [ITI-55] is cache only (HL7 tables, all rights reserved); run scripts/vendor/ihe-iti-tf.sh to check it'

# corpus_files ROOT: the IHE narrative texts under ROOT, the vendored ones
# and the cached ones, one path per line, the FHIR packages and the
# provenance records left out.
corpus_files() {
  local dirs=("$1/docs/specs")
  [[ -d "$1/.vendor-cache" ]] && dirs+=("$1/.vendor-cache")
  find "${dirs[@]}" \( -path "$1/docs/specs/ihe-*" -o -path "$1/.vendor-cache/ihe-*" \) -type f \
    \( -name '*.html' -o -name '*.md' -o -name '*.pdf' \) \
    ! -path '*/package/*' ! -name 'PROVENANCE.md' ! -name 'LICENSE*' \
    | LC_ALL=C sort
}

# profile_of FILE: the "NAME VERSION" an implementation guide page directory
# records in the title of its PROVENANCE.md, or nothing for another text.
profile_of() {
  local provenance
  provenance="$(dirname "$1")/PROVENANCE.md"
  [[ -f "$provenance" ]] || return 0
  sed -n -E '1,8s/^# Provenance: the IHE ([A-Za-z]+) ([0-9]+\.[0-9]+\.[0-9]+) narrative pages.*$/\1 \2/p' "$provenance" | head -n 1
}

# headings FILE: the section numbers of FILE's headings, one per line, as
# written: V:S in an implementation guide page, S in a Technical Framework
# text, V:S in a Markdown text under its `# Volume V` heading.
headings() {
  local file="$1"
  case "$file" in
    *.html)
      grep -o -E '<(h[1-6]|span class="heading[0-9]+")[^>]*>[^<]*' "$file" | sed -E 's/^<[^>]*>//' \
        | awk '{
            if (match($0, /[0-9]+:[0-9]+(\.[0-9]+)*/)) { print substr($0, RSTART, RLENGTH) }
            else if (match($0, /^[ \t]*[0-9]+(\.[0-9]+)*/)) { s = substr($0, RSTART, RLENGTH); gsub(/[ \t]/, "", s); print s }
          }'
      ;;
    *.md)
      awk '
        /^#+[ \t]+Volume[ \t]+[0-9]/ { v = $0; sub(/^#+[ \t]+Volume[ \t]+/, "", v); sub(/[^0-9].*$/, "", v); next }
        v != "" && /^#+[ \t]+[0-9]+(\.[0-9]+)*[ \t]/ { s = $2; print v ":" s }
      ' "$file"
      ;;
    *.pdf)
      # check names a PDF it cannot read and leaves its citations unchecked.
      command -v pdftotext > /dev/null 2>&1 || return 0
      pdftotext -layout "$file" - | layout_headings
      ;;
  esac
}

# layout_headings: the section numbers of the headings in pdftotext -layout
# text on stdin. A supplement printed with line numbers puts the number in
# the margin before a heading (`920   3.81.4.1.2.1 Date Search Parameters`);
# the margin number is read past.
layout_headings() {
  grep -o -E '^[[:space:]]*([0-9]+[[:space:]]{2,})?[0-9]+(\.[0-9]+)+[[:space:]]+[A-Z]' \
    | sed -E 's/^[[:space:]]*([0-9]+[[:space:]]{2,})?//; s/[[:space:]]+[A-Z]$//'
}

# build_index ROOT: one line per present section, `V:S<TAB>FILE<TAB>PROFILE`,
# each heading with its parents down to the transaction or the chapter. A
# bare Technical Framework number is in Volume 2 when its chapter is 3 (the
# transactions) and in Volume 1 otherwise.
build_index() {
  local root="$1" file profile
  while IFS= read -r file; do
    profile="$(profile_of "$file")"
    headings "$file" | awk -v file="${file#"$root"/}" -v profile="$profile" '
      {
        s = $0
        if (index(s, ":") == 0) { s = ((s ~ /^3\./) ? "2:" : "1:") s }
        while (s ~ /[:.]/ && s !~ /^2:[0-9]+$/) {
          print s "\t" file "\t" profile
          if (s !~ /\./) { break }
          sub(/\.[0-9]+$/, "", s)
        }
      }'
  done < <(corpus_files "$root")
}

# citations: reads `PATH:LINE:TEXT` lines and prints one line per IHE
# citation, `PATH:LINE<TAB>V:S<TAB>NAME<TAB>VERSION<TAB>TRANSACTION<TAB>AS-WRITTEN`.
citations() {
  LC_ALL=C awk '
    function emit(where, canon, name, ver, txn, written) {
      print where "\t" canon "\t" name "\t" ver "\t" txn "\t" written
    }
    {
      p = index($0, ":"); path = substr($0, 1, p - 1); rest = substr($0, p + 1)
      q = index(rest, ":"); line = substr(rest, 1, q - 1); text = substr(rest, q + 1)
      before = ""
      while (match(text, /§ ?[0-9]+(:[0-9]+)?(\.[0-9]+)*/)) {
        pre = before substr(text, 1, RSTART - 1)
        tok = substr(text, RSTART, RLENGTH)
        before = pre tok
        text = substr(text, RSTART + RLENGTH)
        num = tok; sub(/^§ ?/, "", num)
        tail = pre; sub(/[ \t]+$/, "", tail)
        vol = ""; txn = ""; name = ""; ver = ""; foreign = 0
        if (tail ~ /TF-1$/) { vol = 1 }
        else if (tail ~ /TF-2$/) { vol = 2 }
        else if (match(tail, /ITI-[0-9]+$/)) { vol = 2; txn = substr(tail, RSTART + 4) }
        else if (match(tail, /(PIXm|PDQm|PMIR|mCSD|BALP)( [0-9]+\.[0-9]+\.[0-9]+)?$/)) {
          d = substr(tail, RSTART); sp = index(d, " ")
          if (sp > 0) { name = substr(d, 1, sp - 1); ver = substr(d, sp + 1) } else { name = d }
        }
        else if (tail ~ /(RFC[ -]?[0-9]+|AQL( [0-9.]+)?|ITS-REST|FHIR( R[0-9]+)?)$/) { foreign = 1 }
        if (index(num, ":") > 0) {
          if (vol != "" && substr(num, 1, index(num, ":") - 1) != vol) {
            emit(path ":" line, "MISMATCH:" vol, name, ver, txn, tok)
            continue
          }
          emit(path ":" line, num, name, ver, txn, tok)
          continue
        }
        if (vol != "") { emit(path ":" line, vol ":" num, name, ver, txn, tok); continue }
        if (foreign || index(num, ".") == 0) { continue }
        split(num, c, /\./)
        if (c[1] == 3 && c[2] + 0 >= 20) { emit(path ":" line, "2:" num, name, ver, txn, tok) }
        else if (c[1] + 0 >= 19) { emit(path ":" line, "1:" num, name, ver, txn, tok) }
      }
    }'
}

# judge UNVENDORED INDEX CITATIONS UNREAD: prints a finding per citation INDEX
# does not hold, outside the UNVENDORED roots INDEX does not hold either, and
# exits 1 when there is one. UNREAD names the PDFs left out of INDEX for want
# of pdftotext; a citation INDEX does not hold may be in one of them, even
# inside a transaction another text holds, so it is counted as not checked.
judge() {
  awk -F '\t' -v unread="$4" '
    FNR == 1 { part++ }
    part == 1 { skip[$1] = $2; order[++roots] = $1; next }
    part == 2 {
      present[$1] = 1
      if (!(($1 SUBSEP $2) in seenfile)) { seenfile[$1, $2] = 1; files[$1] = files[$1] (files[$1] == "" ? "" : ", ") $2 }
      if ($3 != "" && !(($1 SUBSEP $3) in seenprofile)) {
        seenprofile[$1, $3] = 1; profiles[$1] = profiles[$1] "|" $3 "|"
        shown[$1] = shown[$1] (shown[$1] == "" ? "" : ", ") $3
      }
      next
    }
    {
      where = $1; canon = $2; name = $3; ver = $4; txn = $5; written = $6
      if (canon ~ /^MISMATCH:/) {
        printf "::error::%s cites %s behind a Volume %s designator\n", where, written, substr(canon, 10)
        bad++; checked++; next
      }
      root = canon; under = ""
      while (1) {
        if (root in skip) { under = root; break }
        if (root !~ /\./) { break }
        sub(/\.[0-9]+$/, "", root)
      }
      if (under != "" && !(under in present)) { unchecked[under]++; next }
      if (unread != "" && !(canon in present)) { unreadable++; next }
      checked++
      if (!(canon in present)) {
        parent = canon
        while (parent ~ /\./ && !(parent in present)) { sub(/\.[0-9]+$/, "", parent) }
        if (parent in present) {
          printf "::error::%s cites %s, which %s does not hold (section %s)\n", where, written, files[parent], canon
        } else {
          printf "::error::%s cites %s, and no vendored IHE text holds section %s or a parent of it\n", where, written, canon
        }
        bad++; next
      }
      if (name != "") {
        want = (ver == "") ? name " " : name " " ver "|"
        if (index(tolower(profiles[canon]), "|" tolower(want)) == 0) {
          printf "::error::%s cites %s %s, but section %s is in %s, pinned as %s\n", where, (ver == "" ? name : name " " ver), written, canon, files[canon], (shown[canon] == "" ? "no profile" : shown[canon])
          bad++; next
        }
      }
      if (txn != "" && canon !~ ("^2:3\\." txn "(\\.|$)")) {
        printf "::error::%s cites ITI-%s %s, a section outside transaction %s\n", where, txn, written, txn
        bad++; next
      }
    }
    END {
      for (i = 1; i <= roots; i++) {
        if (unchecked[order[i]] > 0) { printf "ihe-citations: %d citation(s) under %s not checked: %s\n", unchecked[order[i]], order[i], skip[order[i]] }
      }
      if (unreadable > 0) {
        printf "ihe-citations: %d citation(s) not checked: no text read holds their section, and pdftotext is not on PATH to read %s; install poppler (brew install poppler, or apt-get install poppler-utils) to check them\n", unreadable, unread
      }
      if (bad > 0) { printf "ihe-citations: %d of %d citation(s) name a section the vendored text does not hold\n", bad, checked; exit 1 }
      printf "ihe-citations: %d IHE citation(s), every one in the vendored text\n", checked
    }' "$1" "$2" "$3"
}

# check ROOT: builds the index under ROOT and judges the citations read from
# stdin as `PATH:LINE:TEXT`.
check() {
  local root="$1" work status=0 file unread=""
  if ! command -v pdftotext > /dev/null 2>&1; then
    while IFS= read -r file; do
      if [[ -n "${CI:-}" ]]; then
        echo "::error::ihe-citations: pdftotext is not on PATH, so ${file#"$root"/} is not read, and CI installs it." >&2
      else
        echo "SKIP: ihe-citations: pdftotext is not on PATH, so ${file#"$root"/} is not read; install poppler (brew install poppler, or apt-get install poppler-utils)." >&2
      fi
      unread="$unread${unread:+, }${file#"$root"/}"
    done < <(corpus_files "$root" | grep '\.pdf$' || true)
    if [[ -n "$unread" ]] && [[ -n "${CI:-}" ]]; then
      return 1
    fi
  fi
  work="$(mktemp -d)"
  build_index "$root" > "$work/index.tsv" || status=$?
  if [[ "$status" -eq 0 ]] && [[ ! -s "$work/index.tsv" ]]; then
    echo "ihe-citations: no vendored IHE text under $root/docs/specs/ihe-*" >&2
    status=1
  fi
  if [[ "$status" -ne 0 ]]; then
    rm -rf "$work"
    return "$status"
  fi
  citations > "$work/citations.tsv"
  printf '%s\n' "$UNVENDORED" > "$work/unvendored.tsv"
  judge "$work/unvendored.tsv" "$work/index.tsv" "$work/citations.tsv" "$unread" || status=$?
  rm -rf "$work"
  return "$status"
}

check_tree() {
  git grep -n -I '§' -- crates app tools docs conformance fuzz \
    ':(exclude)docs/specs/**' ':(glob,exclude)**/vendor/**' | check "$PWD"
}

# The self-test builds a small vendored corpus in a temporary directory and
# judges one citation per case: each refused form fails, each near miss and
# each form the grammar leaves to another specification passes.
self_test() {
  local work failed=0 n=0
  work="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand $work now: the local is gone at EXIT.
  trap "rm -rf '$work'" EXIT
  mkdir -p "$work/docs/specs/ihe-pixm-pages" "$work/docs/specs/ihe-iti-tf" "$work/docs/specs/ihe-iua"
  printf '# Provenance: the IHE PIXm 3.1.0 narrative pages\n' > "$work/docs/specs/ihe-pixm-pages/PROVENANCE.md"
  printf '<h2>2:3.83 Query [ITI-83]</h2>\n<h3 id="a">2:3.83.4 Messages</h3><h6 id="b">2:3.83.4.2.2.1 Success</h6>\n' \
    > "$work/docs/specs/ihe-pixm-pages/ITI-83.html"
  printf '<h2>Volume 1:41</h2>\n<h3 id="c">1:41.4.1 Concepts</h3>\n' > "$work/docs/specs/ihe-pixm-pages/volume-1.html"
  printf '# Provenance: IHE ITI TF Volume 1\n' > "$work/docs/specs/ihe-iti-tf/PROVENANCE.md"
  printf '<h1 id="27">27 XCPD</h1>\n<h3 id="27.4.1">27.4.1 Options</h3>\n<h2 id="3.20">3.20 Record Audit Event</h2>\n<h4 id="3.20.4.1">3.20.4.1 Syslog</h4>\n' \
    > "$work/docs/specs/ihe-iti-tf/ch-27.html"
  printf '## 9 Copyright\n# Volume 1 - Profiles\n## 34.1 Actors\n# Volume 2 - Transactions\n#### 3.72.4.3 Expected Actions\n' \
    > "$work/docs/specs/ihe-iua/IUA.md"

  # expect WANT LINE: a source line LINE exits WANT, the tools found on
  # $search, with CI unset.
  local search="$PATH"
  expect() {
    local want="$1" status=0
    n=$((n + 1))
    printf 'crates/x/src/lib.rs:%d:%s\n' "$n" "$2" | CI="" PATH="$search" check "$work" > "$work/out" 2>&1 || status=$?
    if [[ "$status" -ne "$want" ]]; then
      echo "ihe-citations: self-test failed: '$2' exited $status, wanted $want." >&2
      sed 's/^/  /' "$work/out" >&2
      failed=1
    elif [[ "$want" -ne 0 ]] && ! grep -q -F "::error::crates/x/src/lib.rs:$n " "$work/out"; then
      echo "ihe-citations: self-test failed: the refusal of '$2' does not name its file and line." >&2
      failed=1
    fi
    if [[ -n "${IHE_CITATIONS_VERBOSE:-}" ]]; then
      sed 's/^/  /' "$work/out"
    fi
  }
  expect 0 '/// PIXm §2:3.83.4.2.2.1 lists the identifiers.'
  expect 0 '/// PIXm 3.1.0 §2:3.83.4.2.2.1 and §2:3.83.4.'
  expect 0 '// NOTE: PIXm §1:41.4.1: the concepts.'
  expect 0 '/// (ITI-83 §2:3.83.4, PIXm §2:3.83)'
  expect 0 '/// IUA ITI-72 §3.72.4.3: the bearer token.'
  expect 0 '/// ITI TF-1 §27.4.1 and Table 27.1.3-1, §27.4.1.'
  expect 0 '/// the syslog interaction (ITI TF-2 §3.20.4.1).'
  expect 0 '/// the syslog interaction (§3.20.4.1).'
  expect 0 '/// TF-1 §1:41 is the profile.'
  expect 0 '/// the five cases of §3.55.4.2.3 (not vendored).'
  expect 0 '/// the federation specification §5.4, N33, §12.7 and §3.2.4.2.'
  expect 0 '/// RFC 8725 §3.20 and AQL 1.1.0 §3.9.1.5 and RFC 6749 §27.1.'
  expect 0 '/// no section here.'
  expect 1 '/// PIXm §2:3.83.4.2.2.9 lists the identifiers.'
  expect 1 '/// PIXm §1:41.9 the concepts.'
  expect 1 '/// PIXm 3.0.4 §2:3.83.4 the messages.'
  expect 1 '/// PDQm §2:3.83.4 the messages.'
  expect 1 '/// ITI-72 §2:3.83.4 the messages.'
  expect 1 '/// IUA ITI-72 §3.72.4.4: the bearer token.'
  expect 1 '/// ITI TF-1 §27.9.'
  expect 1 '/// §3.20.4.7 the syslog interaction.'
  expect 1 '/// ITI TF-2 §1:41.4.1.'
  expect 1 '/// PMIR §2:3.93.4 the feed.'
  expect 1 '/// ITI TF-1 §9.2 the ATNA chapter.'
  expect 1 '/// the query of ITI TF-2 §3.38.4.1, which no text carries.'
  # A cache-only text, once fetched, holds its sections to it.
  mkdir -p "$work/.vendor-cache/ihe-iti-tf-vol2"
  printf '<h2 id="3.55">3.55 Cross Gateway Patient Discovery [ITI-55]</h2>\n<h6 id="x">3.55.4.2.3 Expected Actions</h6>\n' \
    > "$work/.vendor-cache/ihe-iti-tf-vol2/ITI-55.html"
  expect 0 '/// the five cases of §3.55.4.2.3, from the cache.'
  expect 0 '/// ITI-55 §3.55.4.2.3 and ITI TF-2 §3.55.'
  expect 1 '/// ITI TF-2 §3.55.4.2.9, which the cached page does not hold.'
  expect 1 '/// ITI-38 §3.55.4.2.3 names another transaction.'
  # A PDF heading is read with and without a margin line number before it,
  # and a numbered body line is no heading.
  local layout
  layout="$(printf '%s\n' '      3.81.4.1.2 Message Semantics' \
    '920   3.81.4.1.2.1 Date Search Parameters' \
    '905   AuditEvent Resources (see the search).' \
    '930   For example, 3.5 days.' | layout_headings | tr '\n' ' ')"
  n=$((n + 1))
  if [[ "$layout" != "3.81.4.1.2 3.81.4.1.2.1 " ]]; then
    echo "ihe-citations: self-test failed: the layout headings read '$layout'." >&2
    failed=1
  fi
  # Without pdftotext a PDF is named as not read, a citation only it could
  # hold is not checked, and a citation another text holds is still judged.
  local tool
  mkdir -p "$work/bin" "$work/docs/specs/ihe-atna"
  for tool in awk dirname find grep head mktemp rm sed sort; do
    ln -s "$(command -v "$tool")" "$work/bin/$tool"
  done
  printf 'not read\n' > "$work/docs/specs/ihe-atna/Suppl.pdf"
  search="$work/bin"
  expect 0 '/// ITI-81 §3.81.4.1.2.1 the date search.'
  if ! grep -q -F 'SKIP: ihe-citations: pdftotext is not on PATH, so docs/specs/ihe-atna/Suppl.pdf is not read; install poppler' "$work/out" ||
    ! grep -q -F '1 citation(s) not checked' "$work/out"; then
    echo "ihe-citations: self-test failed: without pdftotext the PDF is not named, or the citation is not counted as not checked." >&2
    sed 's/^/  /' "$work/out" >&2
    failed=1
  fi
  expect 0 '/// PIXm §2:3.83.4.2.2.1 lists the identifiers.'
  expect 1 '/// PDQm §2:3.83.4 the messages.'
  expect 1 '/// ITI-38 §3.55.4.2.3 names another transaction.'
  # In CI, which installs poppler, a PDF left unread fails the run.
  local status=0
  n=$((n + 1))
  printf 'crates/x/src/lib.rs:%d:%s\n' "$n" '/// PIXm §2:3.83.4.2.2.1 lists the identifiers.' |
    CI=true PATH="$search" check "$work" > "$work/out" 2>&1 || status=$?
  if [[ "$status" -ne 1 ]] ||
    ! grep -q -F '::error::ihe-citations: pdftotext is not on PATH, so docs/specs/ihe-atna/Suppl.pdf is not read, and CI installs it.' "$work/out"; then
    echo "ihe-citations: self-test failed: in CI without pdftotext the run exited $status, wanted 1 naming the PDF." >&2
    sed 's/^/  /' "$work/out" >&2
    failed=1
  fi
  if [[ "$failed" -ne 0 ]]; then
    exit 1
  fi
  echo "ihe-citations: self-test OK ($n cases)."
}

case "${1:-}" in
  --self-test) self_test ;;
  '') check_tree ;;
  *)
    echo "usage: $0 [--self-test]" >&2
    exit 2
    ;;
esac
