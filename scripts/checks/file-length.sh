#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# File-length guard: a hand-written Rust file is at most $HARD lines, and a
# file over $SOFT lines is split into a module folder before it grows further.
#
# Generated files (a `@generated` banner on the first line) and vendored trees
# are outside the rule. A file listed in scripts/checks/file-length-allow.txt
# is a known breach with the issue that splits it: it passes while it does not
# grow past the line count recorded beside it. A file over $SOFT and under
# $HARD that is not listed is reported as a warning, never a failure.
#
# The list is a ratchet that only tightens (#392). A listed file that shrank,
# fell to $SOFT lines or under, or is gone passes with a one-line note, so a
# pull request that splits a file leaves the list alone and conflicts with no
# other one; the scheduled ratchet-tighten workflow runs --update on main and
# opens the one pull request that lowers or removes the entries.
#
# Usage:
#   scripts/checks/file-length.sh              check every tracked .rs file
#   scripts/checks/file-length.sh --files F…   check the named files
#   scripts/checks/file-length.sh --update     lower each entry to its file's
#                                              length and drop the entries
#                                              back within $SOFT or gone; it
#                                              never raises or adds one
#   scripts/checks/file-length.sh --record --issue N
#                                              --update, and add each new
#                                              breach with issue N
#   scripts/checks/file-length.sh --self-test  prove each refusal, each pass
#                                              and each rewrite against
#                                              fixture repositories
#   --root DIR                                 check the git tree at DIR
#                                              instead of this repository
#                                              (the self-test)
# Exit 1 on a hard breach or a listed file that grew, 2 on a usage error or
# a new breach --update may not add; 0 otherwise.
set -euo pipefail
self="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
root="$(dirname "$0")/../.."

SOFT=750
HARD=1000
ALLOW=scripts/checks/file-length-allow.txt

note() { printf '  %s\n' "$*"; }

# The self-test builds one fixture repository per case in a temporary
# directory and runs this guard over it: a Rust file over the limit fails, a
# listed file passes until it grows and passes with a note when it shrinks,
# --update lowers and drops entries and never raises or adds one, and the
# near misses pass.
self_test() {
  local work failed=0 n=0
  work="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand $work now: the local is gone at EXIT.
  trap "rm -rf '$work'" EXIT
  export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
  # fixture DIR PATH LINES [FIRST]: a tracked file of LINES lines at PATH.
  fixture() {
    local dir="$1" path="$2" count="$3" first="${4:-}"
    mkdir -p "$(dirname "$dir/$path")"
    {
      if [[ -n "$first" ]]; then
        printf '%s\n' "$first"
        count=$((count - 1))
      fi
      seq 1 "$count"
    } > "$dir/$path"
    git -C "$dir" add -- "$path"
  }
  # case_dir: a fresh git repository for one case, named in $d.
  case_dir() {
    n=$((n + 1))
    mkdir -p "$work/$n"
    git -C "$work/$n" init -q
    d="$work/$n"
  }
  # expect WANT NAME DIR [ARGS…]: the guard over DIR with ARGS exits WANT;
  # its output is left in $work/out.
  expect() {
    local want="$1" name="$2" dir="$3" status=0
    shift 3
    bash "$self" --root "$dir" "$@" > "$work/out" 2>&1 || status=$?
    if [[ "$status" -ne "$want" ]]; then
      echo "file-length: self-test failed: $name exited $status, wanted $want." >&2
      sed 's/^/    /' "$work/out" >&2
      failed=1
    fi
  }
  # same NAME GOT WANT: GOT is WANT, or the case NAME fails.
  same() {
    local name="$1" got="$2" want="$3"
    if [[ "$got" != "$want" ]]; then
      echo "file-length: self-test failed: $name (got '$got', wanted '$want')." >&2
      failed=1
    fi
  }
  # noted NAME: the last run printed its note, or the case NAME fails.
  noted() {
    local name="$1"
    if ! grep -q 'note:' "$work/out"; then
      echo "file-length: self-test failed: $name." >&2
      failed=1
    fi
  }
  # allow DIR [ENTRY…]: the allow-list of DIR with the given entries.
  allow() {
    local dir="$1"
    shift
    mkdir -p "$dir/scripts/checks"
    {
      printf '# header\n'
      if [[ $# -gt 0 ]]; then printf '%s\n' "$@"; fi
    } > "$dir/$ALLOW"
  }
  # entry DIR PATH: the count the allow-list of DIR records for PATH.
  entry() {
    local dir="$1" path="$2"
    awk -v p="$path" '$1 !~ /^#/ && $2 == p { print $1 }' "$dir/$ALLOW"
  }
  local d big=$((HARD + 1))

  case_dir; fixture "$d" crates/a/src/big.rs "$big"
  expect 1 "a Rust file over $HARD lines" "$d"

  case_dir; fixture "$d" app/a/src/big.rs "$big"
  allow "$d" "$big app/a/src/big.rs #1"
  expect 0 "a listed Rust file at its recorded count" "$d"

  case_dir; fixture "$d" crates/a/src/big.rs "$big"
  allow "$d" "$HARD crates/a/src/big.rs #1"
  expect 1 "a listed Rust file that grew" "$d"
  expect 1 "--update over a listed file that grew" "$d" --update
  same "--update raised an entry" "$(entry "$d" crates/a/src/big.rs)" "$HARD"
  expect 1 "--record over a listed file that grew" "$d" --record --issue 2
  same "--record raised an entry" "$(entry "$d" crates/a/src/big.rs)" "$HARD"

  case_dir; fixture "$d" crates/a/src/big.rs "$big"
  allow "$d" "HARD+1 crates/a/src/big.rs #1"
  expect 1 "an entry whose count is no number" "$d"

  case_dir; fixture "$d" crates/a/src/big.rs $((HARD - 50))
  allow "$d" "$big crates/a/src/big.rs #1"
  expect 0 "a listed Rust file that shrank" "$d"
  noted "a shrink passed with no note"
  same "a check wrote the list" "$(entry "$d" crates/a/src/big.rs)" "$big"
  expect 0 "--update over a listed file that shrank" "$d" --update
  same "--update did not lower the entry" "$(entry "$d" crates/a/src/big.rs)" $((HARD - 50))

  case_dir; fixture "$d" crates/a/src/big.rs "$SOFT"
  allow "$d" "$big crates/a/src/big.rs #1"
  expect 0 "a listed Rust file back within $SOFT" "$d"
  noted "a split passed with no note"
  expect 0 "--update over a file back within $SOFT" "$d" --update
  same "--update kept an entry within the limit" "$(entry "$d" crates/a/src/big.rs)" ""

  case_dir; fixture "$d" crates/a/src/small.rs 10
  allow "$d" "$big crates/a/src/gone.rs #1"
  expect 0 "an entry for a file that is gone" "$d"
  noted "a gone file passed with no note"
  expect 0 "--update over a gone file" "$d" --update
  same "--update kept the entry of a gone file" "$(entry "$d" crates/a/src/gone.rs)" ""

  case_dir; fixture "$d" crates/a/src/new.rs $((HARD + 100))
  allow "$d"
  expect 2 "--update over a new breach" "$d" --update
  same "--update added a breach" "$(entry "$d" crates/a/src/new.rs)" ""
  expect 2 "--record without --issue" "$d" --record
  expect 0 "--record --issue over a new breach" "$d" --record --issue 7
  same "--record did not add the breach" "$(entry "$d" crates/a/src/new.rs)" $((HARD + 100))
  expect 0 "the recorded breach" "$d"

  case_dir
  fixture "$d" crates/a/src/long.rs $((SOFT + 1))
  fixture "$d" docs/big.md "$big"
  fixture "$d" crates/a/vendor/big.rs "$big"
  fixture "$d" crates/a/src/gen.rs "$big" '// @generated by a fixture'
  expect 0 "the near misses (over $SOFT only, not Rust, vendored, generated)" "$d"
  if [[ "$failed" -ne 0 ]]; then
    exit 1
  fi
  echo "file-length: self-test OK (a growth, a shrink, a split, a gone file, --update and --record)."
}

mode=check
issue=""
files=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --files) mode=files; shift; while [[ $# -gt 0 ]] && [[ "${1#--}" = "$1" ]]; do files+=("$1"); shift; done ;;
    --update) mode=update; shift ;;
    --record) mode=record; shift ;;
    --issue) issue="${2:-}"; shift 2 ;;
    --root) root="$2"; shift 2 ;;
    --self-test) self_test; exit 0 ;;
    *) printf 'file-length: unknown argument %s\n' "$1" >&2; exit 2 ;;
  esac
done
if [[ "$mode" = "record" ]] && [[ ! "$issue" =~ ^[0-9]+$ ]]; then
  printf 'file-length: --record needs --issue N\n' >&2
  exit 2
fi
cd "$root"

if [[ "$mode" != "files" ]]; then
  while IFS= read -r f; do files+=("$f"); done < <(git ls-files -- '*.rs' ':(glob,exclude)**/vendor/**')
fi

# A template under a generator's src/templates/ is the text of one generated
# file, embedded verbatim, so it is generated output in source form.
hand_written() {
  local file="$1"
  case "$file" in
  */vendor/* | */src/templates/*) return 1 ;;
  *) ;;
  esac
  ! head -n1 "$file" | grep -q '@generated'
}


# The allow-list entry of a path, as "<lines> <tag>", or nothing (bash 3.2 has
# no associative arrays, so the list is read per lookup).
listed() {
  local path="$1"
  [[ -f "$ALLOW" ]] || return 0
  awk -v p="$path" '$1 !~ /^#/ && $2 == p { print $1, $3 }' "$ALLOW"
}

fail=0
grew=0
warn=0
fell=0
breaches=()
# A listed path that no longer exists was split away: a fall.
if [[ "$mode" != "files" ]] && [[ -f "$ALLOW" ]]; then
  while IFS=' ' read -r first path _; do
    case "$first" in
    '' | \#*) continue ;;
    *) ;;
    esac
    [[ -f "$path" ]] && continue
    fell=$((fell + 1))
  done < "$ALLOW"
fi
for f in ${files[@]+"${files[@]}"}; do
  [[ -f "$f" ]] || continue
  hand_written "$f" || continue
  n=$(wc -l < "$f" | tr -d " ")
  if [[ "$n" -gt "$SOFT" ]]; then
    breaches+=("$n $f")
  fi
  entry=$(listed "$f")
  if [[ -n "$entry" ]]; then
    recorded=${entry%% *}
    tag=${entry#* }
    # [[ -gt ]] evaluates its operands as arithmetic, so the count read from
    # the allow-list is held to digits before it is compared.
    if [[ ! "$recorded" =~ ^[0-9]+$ ]]; then
      note "FAIL $f: its allow-list entry records '$recorded', not a line count"
      fail=1
      grew=1
    elif [[ "$n" -gt "$recorded" ]]; then
      note "FAIL $f: $n lines, listed at $recorded ($tag); a listed file may not grow, split it"
      fail=1
      grew=1
    elif [[ "$n" -lt "$recorded" ]]; then
      fell=$((fell + 1))
    fi
    continue
  fi
  if [[ "$n" -gt "$HARD" ]]; then
    note "FAIL $f: $n lines, over the hard limit of $HARD; split it into a module folder"
    fail=1
  elif [[ "$n" -gt "$SOFT" ]]; then
    note "warn $f: $n lines, over $SOFT; split it before it grows"
    warn=$((warn + 1))
  fi
done

if [[ "$mode" = "update" ]] || [[ "$mode" = "record" ]]; then
  if [[ "$grew" -ne 0 ]]; then
    note "the list never rises: split the file that grew"
    exit 1
  fi
  out=()
  for b in ${breaches[@]+"${breaches[@]}"}; do
    n=${b%% *}; p=${b#* }
    entry=$(listed "$p")
    if [[ -n "$entry" ]]; then
      out+=("$n $p ${entry#* }")
    elif [[ "$mode" = "record" ]]; then
      out+=("$n $p #$issue")
    elif [[ "$n" -gt "$HARD" ]]; then
      printf 'file-length: %s is a new breach; split it, or record it with --record --issue N\n' "$p" >&2
      exit 2
    fi
  done
  # With no list and nothing to list, there is nothing to write.
  if [[ ! -f "$ALLOW" ]] && [[ "${#out[@]}" -eq 0 ]]; then
    note "no breach to record and no $ALLOW"
    exit 0
  fi
  {
    printf '# Known file-length breaches: "<lines> <path> #<issue>". A listed file passes\n'
    printf '# while it does not grow past its recorded count; --update lowers it.\n'
    if [[ "${#out[@]}" -gt 0 ]]; then printf '%s\n' ${out[@]+"${out[@]}"} | sort -k2; fi
  } > "$ALLOW.tmp" && mv "$ALLOW.tmp" "$ALLOW"
  note "recorded ${#out[@]} breaches in $ALLOW"
  exit 0
fi

if [[ "$fail" -eq 0 ]]; then
  note "OK: no hand-written .rs file over $HARD lines outside the allow-list ($warn over $SOFT unlisted)"
  if [[ "$fell" -gt 0 ]]; then
    note "note: $fell listed file(s) shrank, split or went; the scheduled ratchet-tighten workflow lowers $ALLOW."
  fi
fi
exit "$fail"
