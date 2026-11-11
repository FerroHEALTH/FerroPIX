#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# The bash 3.2 guard: every shell program of the repository runs under the
# bash macOS ships, 3.2, or says which bash it needs and stops with a clear
# message (no specification governs this: our own design). CI runs bash 5,
# so a construct bash 3.2 lacks would pass every lane and fail first on a
# contributor's Mac.
#
# It reads every tracked *.sh under scripts/ and the session hooks, with each
# full-line comment blanked, and refuses two kinds of line:
#
#   syntax      a construct a later bash introduced: associative arrays,
#               mapfile and readarray, case modification (${v,,}, ${v^^}),
#               the |& pipe, the ;& and ;;& case terminators, namerefs, the
#               ${v@Q} transformations, coproc, wait -n, &>>, declare -g,
#               declare -l and -u, the bash-4 shopt options, the bash-4
#               variables, printf %(...)T, the -v test, {fd} redirections and
#               a negative substring length.
#   empty array an array that can be empty (assigned `name=()`, or filled by
#               `read -a name`) expanded as "${name[@]}" or "${name[*]}":
#               under `set -u`, bash before 4.4 calls the empty expansion an
#               unbound variable. Spell it ${name[@]+"${name[@]}"}.
#
# A script that needs a later bash declares it with a BASH_VERSINFO check
# that exits with a message, and the guard then skips it, for example:
#
#   if ((BASH_VERSINFO[0] < 4)); then
#     echo "this script needs bash 4 or later" >&2; exit 1
#   fi
#
# Usage:
#   scripts/checks/bash-compat.sh              check every tracked script
#   scripts/checks/bash-compat.sh --self-test  prove each refusal and pass
# Exit 1 naming each file, line and construct; 0 otherwise.
set -euo pipefail
cd "$(dirname "$0")/../.."

# The bash-4-and-later syntax, one rule per line: the bash release that
# introduced it, a tab, what it is, a tab, and an extended regular expression
# over a comment-blanked line.
# shellcheck disable=SC2016 # the rules are regular expressions, never expanded
readonly RULES='4.0	an associative array	(^|[;&|({[:space:]])(declare|local|typeset|readonly)[[:space:]]+-[a-zA-Z]*A
4.0	mapfile or readarray	(^|[;&|({[:space:]])(mapfile|readarray)([[:space:]]|$)
4.0	case modification of a parameter	\$\{[#!]?[A-Za-z_][A-Za-z0-9_]*(\[[^]]*\])?(\^|,)
4.0	the |& pipe	[[:space:]]\|&([[:space:]]|$)
4.0	a ;& or ;;& case terminator	(^|[^;&]);;?&[[:space:]]*$
4.0	the &>> redirection	&>>
4.0	coproc	(^|[;&|({[:space:]])coproc([[:space:]]|$)
4.0	declare -l or -u	(^|[;&|({[:space:]])(declare|local|typeset)[[:space:]]+-[a-zA-Z]*[lu]
4.0	a bash-4 shopt option	shopt[[:space:]]+-[su][[:space:]].*(globstar|lastpipe|inherit_errexit|autocd|checkjobs|dirspell|direxpand|globasciiranges|localvar_inherit|localvar_unset|assoc_expand_once|progcomp_alias|patsub_replacement|varredir_close|noexpand_translation|compat4)
4.0	a bash-4 variable	\$\{?(BASHPID|EPOCHSECONDS|EPOCHREALTIME|BASH_ARGV0|SRANDOM|READLINE_MARK)([^A-Za-z0-9_]|$)
4.1	a {fd} redirection	\{[A-Za-z_][A-Za-z0-9_]*\}[<>]
4.2	declare -g	(^|[;&|({[:space:]])(declare|local|typeset)[[:space:]]+-[a-zA-Z]*g
4.2	printf %(...)T	%[-0-9]*\([^)]*\)T
4.2	the -v test	(\[\[?|test)[[:space:]]+(![[:space:]]+)?-v[[:space:]]
4.2	a negative substring length	\$\{[A-Za-z_][A-Za-z0-9_]*:[^}:]*:[[:space:]]*-[0-9]
4.3	a nameref	(^|[;&|({[:space:]])(declare|local|typeset)[[:space:]]+-[a-zA-Z]*n
4.3	wait -n	(^|[;&|({[:space:]])wait[[:space:]]+-[a-zA-Z]*n
4.4	a ${v@...} transformation	\$\{[A-Za-z_][A-Za-z0-9_]*(\[[^]]*\])?@[QEPAaKkUuL]\}'

# blank FILE: FILE with every full-line comment emptied, so line numbers hold.
blank() {
  local file="$1"
  sed -E 's/^[[:space:]]*#.*$//' "$file"
}

# declares_version FILE: whether FILE checks BASH_VERSINFO on a code line.
# The text is read whole first: under pipefail, grep -q ending a pipe early
# fails the writer with SIGPIPE on a long file, and a declared script would
# then be checked.
declares_version() {
  local file="$1" text
  text="$(blank "$file")"
  grep -q -E 'BASH_VERSINFO' <<< "$text"
}

# maybe_empty FILE: the arrays FILE assigns empty or fills with read -a.
maybe_empty() {
  local text
  text="$(blank "$1")"
  {
    grep -o -E '(^|[^A-Za-z0-9_])[A-Za-z_][A-Za-z0-9_]*=\(\)' <<< "$text" \
      | sed -E 's/^[^A-Za-z_]//; s/=\(\)$//' || true
    grep -o -E '(^|[;&|({[:space:]])read([[:space:]]+-[A-Za-z]+)*[[:space:]]+-[A-Za-z]*a[[:space:]]+[A-Za-z_][A-Za-z0-9_]*' <<< "$text" \
      | sed -E 's/.*[[:space:]]//' || true
  } | LC_ALL=C sort -u
}

# check_file PATH FILE: reports each bash-3.2 hazard of FILE under PATH.
check_file() {
  local path="$1" file="$2" fail=0 since what regex hit name all
  if declares_version "$file"; then
    return 0
  fi
  while IFS=$'\t' read -r since what regex; do
    while IFS= read -r hit; do
      echo "::error file=$path,line=${hit%%:*}::$path:${hit%%:*} uses $what, which needs bash $since; macOS ships bash 3.2. Rewrite it, or declare the bash it needs with a BASH_VERSINFO check that exits with a message." >&2
      fail=1
    done < <(blank "$file" | grep -n -E -- "$regex" || true)
  done <<< "$RULES"
  while IFS= read -r name; do
    [[ -n "$name" ]] || continue
    # The guarded form ${name[@]+"${name[@]}"} is the one sanctioned spelling,
    # so its inner expansion is masked before the search.
    all="\${${name}[@]+\"\${${name}[@]}\"}"
    while IFS= read -r hit; do
      echo "::error file=$path,line=${hit%%:*}::$path:${hit%%:*} expands the array $name, which can be empty, unguarded; under set -u, bash 3.2 calls the empty expansion unbound. Write ${all}." >&2
      fail=1
    done < <(blank "$file" \
      | sed -E "s/\\$\\{$name\\[[@*]\\]\\+\"\\$\\{$name\\[[@*]\\]\\}\"\\}/GUARDED/g" \
      | grep -n -E "\\$\\{!?$name\\[[@*]\\]\\}" \
      | grep -v -E "\\$\\{!$name\\[@\\]\\}" || true)
  done < <(maybe_empty "$file")
  return "$fail"
}

check_tree() {
  local fail=0 count=0 path
  while IFS= read -r path; do
    count=$((count + 1))
    check_file "$path" "$path" || fail=1
  done < <(git ls-files -- 'scripts/*.sh' 'scripts/**/*.sh' '.claude/hooks/*.sh' | LC_ALL=C sort -u)
  if [[ "$fail" -eq 0 ]]; then
    echo "bash-compat: $count shell programs, every one runs under bash 3.2."
  fi
  return "$fail"
}

# The self-test drives check_file over fixtures in a temporary directory:
# each refused construct fails, each near miss passes.
self_test() {
  local work failed=0
  work="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand $work now: the local is gone at EXIT.
  trap "rm -rf '$work'" EXIT
  local n=0
  # expect WANT NAME LINE...: a script of the LINEs exits WANT.
  expect() {
    local want="$1" name="$2" status=0 file
    shift 2
    n=$((n + 1))
    file="$work/case-$n.sh"
    {
      printf '#!/usr/bin/env bash\nset -euo pipefail\n'
      printf '%s\n' "$@"
    } > "$file"
    check_file "$name" "$file" 2> /dev/null || status=$?
    if [[ "$status" -ne "$want" ]]; then
      echo "bash-compat: self-test failed: $name exited $status, wanted $want." >&2
      failed=1
    fi
  }
  local d='$' empty='args=()'
  expect 1 'declare -A' 'declare -A seen=()'
  expect 1 'local -A' 'f() {' '  local -A map' '}'
  expect 1 'mapfile' 'mapfile -t lines < file'
  expect 1 'readarray' 'x=1; readarray lines < file'
  expect 1 'lower case' "echo \"${d}{name,,}\""
  expect 1 'upper case' "echo \"${d}{name^^}\""
  expect 1 'first upper' "echo \"${d}{name^}\""
  expect 1 '|& pipe' 'make |& tee log'
  expect 1 ';& terminator' 'case x in' '  a) echo a ;&' '  b) echo b ;;' 'esac'
  expect 1 ';;& terminator' 'case x in' '  a) echo a ;;&' 'esac'
  expect 1 '&>>' 'echo x &>> log'
  expect 1 'coproc' 'coproc cat'
  expect 1 'declare -l' 'declare -l lower=X'
  expect 1 'shopt globstar' 'shopt -s globstar'
  expect 1 'shopt inherit_errexit' 'shopt -s nullglob inherit_errexit'
  expect 1 'EPOCHSECONDS' "echo ${d}EPOCHSECONDS"
  expect 1 'BASHPID' "echo ${d}{BASHPID}"
  expect 1 '{fd} redirection' 'exec {fd}> log'
  expect 1 'declare -g' 'f() { declare -g x=1; }'
  expect 1 'printf %()T' "printf '%(%F)T' -1"
  expect 1 '[[ -v ]]' 'if [[ -v name ]]; then :; fi'
  expect 1 'test -v' 'test -v name'
  expect 1 'negative length' "echo \"${d}{name:0:-1}\""
  expect 1 'nameref' "f() { local -n ref=${d}1; }"
  expect 1 'wait -n' 'wait -n'
  expect 1 '@Q' "echo \"${d}{name@Q}\""
  expect 1 'empty array expanded' "$empty" "cmd \"${d}{args[@]}\""
  expect 1 'empty local array expanded' 'f() {' '  local -a args=()' "  cmd \"${d}{args[*]}\"" '}'
  expect 1 'read -a array expanded' "IFS=, read -r -a names <<< \"${d}1\"" "for n in \"${d}{names[@]}\"; do :; done"
  expect 1 'unquoted empty array' "$empty" "cmd ${d}{args[@]}"
  expect 0 'guarded empty array' "$empty" "cmd ${d}{args[@]+\"${d}{args[@]}\"}"
  expect 0 'empty array length' "$empty" "echo \"${d}{#args[@]}\""
  expect 0 'empty array keys' "$empty" "for i in \"${d}{!args[@]}\"; do :; done"
  expect 0 'literal array' "paths=(a b c)" "cmd \"${d}{paths[@]}\""
  expect 0 'positional parameters' "cmd \"${d}@\""
  expect 0 'indexed array' 'declare -a list' 'local -r x=1' 'readonly y=2'
  expect 0 'a comment' '# mapfile, declare -A and |& in a comment'
  expect 0 'bracket |& in a regex' "grep -E '[^;|&]+' file"
  expect 0 'case ;;' 'case x in' '  a) echo a ;;' 'esac'
  expect 0 'default value' "echo \"${d}{name:-30}\" \"${d}{name: -1}\" \"${d}{name:1:2}\""
  expect 0 'pattern removal' "echo \"${d}{name#,}\" \"${d}{name%^}\""
  expect 0 'arithmetic' "x=${d}((a < 4))"
  expect 0 'grep -v' "grep -v -E 'x' file"
  expect 0 'declared version' 'if ((BASH_VERSINFO[0] < 4)); then' '  echo "needs bash 4" >&2; exit 1' 'fi' 'declare -A seen=()' 'mapfile -t lines < file'
  if [[ "$failed" -ne 0 ]]; then
    exit 1
  fi
  echo "bash-compat: self-test OK ($n cases)."
}

case "${1:-}" in
  --self-test) self_test ;;
  '') check_tree ;;
  *)
    echo "usage: $0 [--self-test]" >&2
    exit 2
    ;;
esac
