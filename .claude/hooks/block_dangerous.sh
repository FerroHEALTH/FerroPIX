#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# .claude/hooks/block_dangerous.sh
#
# Claude Code PreToolUse hook (matcher: Bash). Blocks destructive commands:
#   - a recursive forced delete, unless every target is a directory the same
#     command created with `mktemp -d` or a path under /tmp or $TMPDIR, and
#     none lies inside the repository
#   - a force push whose target ref is main or master, and a force push that
#     names no conventional-type branch
#   - deletion of LICENSE or CLAUDE.md (the licence and the working discipline)
#
# Reads the tool-call JSON on stdin. Exit 2 blocks; exit 0 allows.
#
#   .claude/hooks/block_dangerous.sh --self-test   # run the built-in cases
#
# The force-push rule reads only an actual `git push` invocation: the command
# is split into simple commands at ; & | and newlines, and only one whose
# words are `git [global options] push …` is checked, by its own arguments.
# A protected name elsewhere (a sed string, an issue body) is never read.
#
# The delete rules read only an actual `rm` or `git rm` invocation. A perl
# tokenizer splits the command the way the shell does (quotes, `$( … )`,
# here-documents, comments and redirections), so the words inside an argument
# such as an issue body are never judged. The command a `trap`, `eval`,
# `bash -c`, `find -exec`, `xargs` or `sudo` runs is judged too.

set -euo pipefail

# The conventional branch types a force push may name.
readonly BRANCH_TYPES='feat|fix|chore|docs|refactor|perf|test|ci|build|release'

# Splits stdin into simple commands as the shell would, removing quotes and
# dropping redirections, comments and here-document bodies. Prints each
# command's words joined by 0x1f, the commands joined by 0x1e.
# shellcheck disable=SC2016 # perl source: perl expands its own variables
readonly TOKENIZER='
use strict; use warnings;
local $/; my $s = <STDIN>; $s = "" unless defined $s;
my $n = length $s; my $i = 0; my $SQ = chr 39;
my (@cmds, @words, @pending); my $w = ""; my $in = 0; my $skip = 0; my $doc = 0;
sub word_end {
  if ($in) {
    if ($doc) { push @pending, [$w, $doc == 2]; $doc = 0 }
    elsif ($skip) { $skip = 0 }
    else { push @words, $w }
  }
  $w = ""; $in = 0;
}
sub cmd_end { word_end(); push @cmds, join("\x1f", @words) if @words; @words = () }
sub subst_end {
  my ($j) = @_; my $d = 0;
  while ($j < $n) {
    my $c = substr($s, $j, 1);
    if ($c eq "\\") { $j += 2; next }
    if ($c eq $SQ) { my $k = index($s, $SQ, $j + 1); return $n if $k < 0; $j = $k + 1; next }
    if ($c eq "\"") {
      $j++;
      while ($j < $n && substr($s, $j, 1) ne "\"") { $j++ if substr($s, $j, 1) eq "\\"; $j++ }
      $j++; next;
    }
    if ($c eq "(") { $d++ } elsif ($c eq ")") { $d--; return $j + 1 if $d == 0 }
    $j++;
  }
  return $n;
}
while ($i < $n) {
  my $c = substr($s, $i, 1);
  if ($c eq "\\") {
    if (substr($s, $i + 1, 1) ne "\n") { $w .= substr($s, $i + 1, 1); $in = 1 }
    $i += 2; next;
  }
  if ($c eq $SQ) {
    my $k = index($s, $SQ, $i + 1); $k = $n if $k < 0;
    $w .= substr($s, $i + 1, $k - $i - 1); $in = 1; $i = $k + 1; next;
  }
  if ($c eq "\"") {
    $in = 1; $i++;
    while ($i < $n) {
      my $d = substr($s, $i, 1);
      last if $d eq "\"";
      if ($d eq "\\" && index("\$`\"\\\n", substr($s, $i + 1, 1)) >= 0) {
        $w .= substr($s, $i + 1, 1); $i += 2; next;
      }
      if ($d eq "\$" && substr($s, $i + 1, 1) eq "(") {
        my $e = subst_end($i); $w .= substr($s, $i, $e - $i); $i = $e; next;
      }
      $w .= $d; $i++;
    }
    $i++; next;
  }
  if ($c eq "\$" && substr($s, $i + 1, 1) eq "(") {
    my $e = subst_end($i); $w .= substr($s, $i, $e - $i); $in = 1; $i = $e; next;
  }
  if ($c eq "`") {
    my $k = index($s, "`", $i + 1); $k = $n - 1 if $k < 0;
    $w .= substr($s, $i, $k - $i + 1); $in = 1; $i = $k + 1; next;
  }
  if ($c eq "#" && !$in) { my $k = index($s, "\n", $i); $i = $k < 0 ? $n : $k; next }
  if ($c eq " " || $c eq "\t") { word_end(); $i++; next }
  if ($c eq "\n") {
    cmd_end(); $i++;
    for my $h (@pending) {
      my ($delim, $tabs) = @$h;
      while ($i < $n) {
        my $k = index($s, "\n", $i); $k = $n if $k < 0;
        my $line = substr($s, $i, $k - $i); $i = $k + 1;
        $line =~ s/^\t+// if $tabs;
        last if $line eq $delim;
      }
    }
    @pending = (); next;
  }
  if (index(";&|()", $c) >= 0) { cmd_end(); $i++; next }
  if ($c eq "<" || $c eq ">") {
    if ($in && $w =~ /^\d+$/) { $w = ""; $in = 0 } else { word_end() }
    my $k = $i; $k++ while $k < $n && index("<>&", substr($s, $k, 1)) >= 0;
    my $op = substr($s, $i, $k - $i);
    if ($op eq "<<") {
      my $tabs = substr($s, $k, 1) eq "-"; $k++ if $tabs; $doc = $tabs ? 2 : 1;
    } else { $skip = 1 }
    $i = $k; next;
  }
  $w .= $c; $in = 1; $i++;
}
cmd_end();
print join("\x1e", @cmds);
'

# The roots of the repository and of its main checkout, and the temporary
# directory, set below or by the self-test; and the names the command being
# judged assigned a fresh `mktemp -d` directory.
repo_roots=()
tmp_root="${TMPDIR:-}"
mktemp_vars=" "

# Prints the simple commands of $1, as TOKENIZER describes.
tokenize() {
  printf '%s' "$1" | perl -e "$TOKENIZER"
}

# Records every word of the form NAME=$(mktemp -d …) among the arguments.
collect_mktemp() {
  # shellcheck disable=SC2016 # a regex matching a literal $( or backtick
  local word arg re='^([A-Za-z_][A-Za-z0-9_]*)=(\$\(|`)mktemp([^)`]*)(\)|`)$'
  local -a args
  for word in "$@"; do
    [[ "$word" =~ $re ]] || continue
    local name="${BASH_REMATCH[1]}"
    args=()
    read -r -a args <<< "${BASH_REMATCH[3]}" || true
    for arg in "${args[@]+"${args[@]}"}"; do
      case "$arg" in
        --directory) mktemp_vars+="$name " ;;
        --*) ;;
        -*d*) mktemp_vars+="$name " ;;
      esac
    done
  done
}

# Succeeds when $1, the part of a path under a temporary root, names
# something below that root: not empty, no . or .. step, no leading glob.
safe_rest() {
  [ -n "$1" ] || return 1
  [[ "/$1/" == */../* || "/$1/" == */./* ]] && return 1
  case "$1" in [*?[]*) return 1 ;; esac
  return 0
}

# Succeeds when the delete target $1 may be removed recursively.
allowed_target() {
  local t="$1" name rest root p
  # shellcheck disable=SC2016 # a regex matching a literal $NAME
  local re='^\$(\{([A-Za-z_][A-Za-z0-9_]*)\}|([A-Za-z_][A-Za-z0-9_]*))(/.*)?$'
  if [[ "$t" =~ $re ]]; then
    name="${BASH_REMATCH[2]}${BASH_REMATCH[3]}"
    rest="${BASH_REMATCH[4]}"
    if [[ "$mktemp_vars" == *" $name "* ]]; then
      [[ "$rest/" != */../* ]]
      return
    fi
    if [ "$name" = TMPDIR ]; then
      safe_rest "${rest#/}"
      return
    fi
    return 1
  fi
  case "$t" in /*) ;; *) return 1 ;; esac
  for root in "${repo_roots[@]+"${repo_roots[@]}"}"; do
    [[ "$t/" == "${root%/}"/* ]] && return 1
  done
  local -a prefixes=(/tmp /private/tmp)
  p="${tmp_root%/}"
  if [ -n "$p" ]; then
    prefixes+=("$p")
    [[ "$p" == /var/* ]] && prefixes+=("/private$p")
  fi
  for p in "${prefixes[@]}"; do
    if [[ "$t" == "$p"/* ]]; then
      safe_rest "${t#"$p"/}"
      return
    fi
  done
  return 1
}

# Prints the reason and returns 2 when a target among the arguments is
# LICENSE or CLAUDE.md.
check_protected() {
  local t base
  for t in "$@"; do
    case "$t" in -*) continue ;; esac
    base="${t%/}"
    base="${base##*/}"
    case "$base" in
      LICENSE | CLAUDE.md)
        echo "BLOCKED: LICENSE (the Business Source License 1.1, the project licence) and CLAUDE.md (the working discipline) must not be deleted."
        return 2
        ;;
    esac
  done
  return 0
}

# Prints the reason and returns 2 when the arguments of an `rm` delete a
# protected file, or delete recursively and by force a target not allowed.
judge_rm() {
  local recursive=0 force=0 options=1 t
  local -a targets=()
  for t in "$@"; do
    if [ "$options" -eq 1 ]; then
      case "$t" in
        --) options=0 && continue ;;
        --recursive) recursive=1 && continue ;;
        --force) force=1 && continue ;;
        --*) continue ;;
        -?*)
          [[ "$t" == *[rR]* ]] && recursive=1
          [[ "$t" == *f* ]] && force=1
          continue
          ;;
      esac
    fi
    targets+=("$t")
  done
  check_protected "${targets[@]+"${targets[@]}"}" || return $?
  if [ "$recursive" -eq 0 ] || [ "$force" -eq 0 ]; then
    return 0
  fi
  if [ "${#targets[@]}" -eq 0 ]; then
    echo "BLOCKED: a recursive forced delete must name its targets (block_dangerous hook)."
    return 2
  fi
  for t in "${targets[@]}"; do
    if ! allowed_target "$t"; then
      echo "BLOCKED: a recursive forced delete of '$t' is not allowed (block_dangerous hook). Delete specific files with 'git rm' or a plain 'rm <file>', or delete a directory the same command created with 'mktemp -d' or one under \$TMPDIR."
      return 2
    fi
  done
  return 0
}

# Prints the reason and returns 2 when the simple command whose words are the
# arguments, or a command it runs, deletes what the delete rules refuse.
judge_command() {
  local -a w=("$@")
  local n=$# i=0 j word base
  while [ "$i" -lt "$n" ]; do
    word="${w[$i]}"
    case "$word" in
      '!' | '{' | '}' | then | do | else | elif | if | while | until | time | command | builtin | exec | nohup)
        i=$((i + 1))
        ;;
      sudo | doas | env | nice | xargs | timeout)
        i=$((i + 1))
        while [ "$i" -lt "$n" ] && [[ "${w[$i]}" == -* || "${w[$i]}" =~ ^[A-Za-z_][A-Za-z0-9_]*= ]]; do
          case "${w[$i]}" in
            -n | -P | -I | -L | -s | -d | -E | -u | -g | -C | -a | -k) i=$((i + 2)) ;;
            *) i=$((i + 1)) ;;
          esac
        done
        [ "$word" = timeout ] && i=$((i + 1))
        ;;
      *)
        [[ "$word" =~ ^[A-Za-z_][A-Za-z0-9_]*= ]] || break
        i=$((i + 1))
        ;;
    esac
  done
  [ "$i" -lt "$n" ] || return 0
  word="${w[$i]#\\}"
  base="${word##*/}"
  j=$((i + 1))
  case "$base" in
    rm)
      judge_rm "${w[@]:$j}" || return $?
      ;;
    git)
      while [ "$j" -lt "$n" ] && [[ "${w[$j]}" == -* ]]; do
        case "${w[$j]}" in
          -C | -c) j=$((j + 2)) ;;
          *) j=$((j + 1)) ;;
        esac
      done
      if [ "$j" -lt "$n" ] && [ "${w[$j]}" = rm ]; then
        check_protected "${w[@]:$((j + 1))}" || return $?
      fi
      ;;
    trap)
      if [ "$j" -lt "$n" ]; then
        judge_string "${w[$j]}" || return $?
      fi
      ;;
    eval)
      judge_string "${w[*]:$j}" || return $?
      ;;
    bash | sh | zsh | dash | ksh)
      while [ "$j" -lt "$n" ]; do
        if [[ "${w[$j]}" =~ ^-[a-zA-Z]*c[a-zA-Z]*$ ]] && [ $((j + 1)) -lt "$n" ]; then
          judge_string "${w[$((j + 1))]}" || return $?
          break
        fi
        j=$((j + 1))
      done
      ;;
    find)
      while [ "$j" -lt "$n" ]; do
        case "${w[$j]}" in
          -exec | -execdir | -ok | -okdir)
            local start=$((j + 1))
            j=$start
            while [ "$j" -lt "$n" ] && [ "${w[$j]}" != ';' ] && [ "${w[$j]}" != '+' ]; do
              j=$((j + 1))
            done
            judge_command "${w[@]:$start:$((j - start))}" || return $?
            ;;
        esac
        j=$((j + 1))
      done
      ;;
  esac
  return 0
}

# Prints the reason and returns 2 when a simple command of the shell text $1
# deletes what the delete rules refuse.
judge_string() {
  local rec
  local -a cmds=() words
  IFS=$'\x1e' read -r -d '' -a cmds < <(tokenize "$1") || true
  for rec in "${cmds[@]+"${cmds[@]}"}"; do
    words=()
    IFS=$'\x1f' read -r -d '' -a words < <(printf '%s' "$rec") || true
    collect_mktemp "${words[@]+"${words[@]}"}"
  done
  for rec in "${cmds[@]+"${cmds[@]}"}"; do
    words=()
    IFS=$'\x1f' read -r -d '' -a words < <(printf '%s' "$rec") || true
    judge_command "${words[@]+"${words[@]}"}" || return $?
  done
  return 0
}

# Prints the reason and returns 2 when the simple command $1 is a force push
# that targets main or master or names no conventional-type branch.
check_push() {
  local -a words
  local i=0 word force=0 named=0 ref dst
  read -r -a words <<< "$1" || true
  # Leading environment assignments and a subshell parenthesis.
  while [ "$i" -lt "${#words[@]}" ] && [[ "${words[$i]}" =~ ^[A-Za-z_][A-Za-z0-9_]*= ]]; do
    i=$((i + 1))
  done
  [ "$i" -lt "${#words[@]}" ] || return 0
  word="${words[$i]#(}"
  [ "$word" = "git" ] || return 0
  i=$((i + 1))
  # Global options before the subcommand; -C and -c take a value.
  while [ "$i" -lt "${#words[@]}" ] && [[ "${words[$i]}" == -* ]]; do
    case "${words[$i]}" in
      -C | -c) i=$((i + 2)) ;;
      *) i=$((i + 1)) ;;
    esac
  done
  [ "$i" -lt "${#words[@]}" ] && [ "${words[$i]}" = "push" ] || return 0
  i=$((i + 1))

  local -a refs=()
  for word in "${words[@]:$i}"; do
    word="${word//\"/}"
    word="${word//\'/}"
    word="${word%)}"
    case "$word" in
      --force | -f | --force-if-includes) force=1 ;;
      --force-with-lease) force=1 ;;
      --force-with-lease=*)
        force=1
        ref="${word#--force-with-lease=}"
        refs+=("${ref%%:*}")
        ;;
      -*) ;;
      +*)
        force=1
        refs+=("${word#+}")
        ;;
      *) refs+=("$word") ;;
    esac
  done
  [ "$force" -eq 1 ] || return 0

  for ref in "${refs[@]+"${refs[@]}"}"; do
    dst="${ref##*:}"
    dst="${dst#refs/heads/}"
    case "$dst" in
      main | master)
        echo "BLOCKED: force-push touching main or master is forbidden (CLAUDE.md hard rule)."
        return 2
        ;;
    esac
    [[ "$dst" =~ ^($BRANCH_TYPES)/ ]] && named=1
  done
  if [ "$named" -eq 0 ]; then
    echo "BLOCKED: bare force-push refused. Force-push (prefer --force-with-lease) only an explicit conventional-type branch (feat/, fix/, chore/, docs/, refactor/, perf/, test/, ci/, build/, release/)."
    return 2
  fi
  return 0
}

# Prints the reason and returns 2 when the command $1 must be blocked.
verdict() {
  local cmd="$1" segment status
  mktemp_vars=" "
  judge_string "$cmd" || return $?

  while IFS= read -r segment; do
    status=0
    check_push "$segment" || status=$?
    [ "$status" -eq 0 ] || return "$status"
  done < <(printf '%s\n' "$cmd" | tr ';&|' '\n')
  return 0
}

# Runs every case: a command and the exit status the hook must give it. A
# literal \n in a case is a newline.
self_test() {
  local failed=0 count=0 expected cmd got
  repo_roots=(/tmp/ferro-repo)
  tmp_root=/var/folders/ab/T/
  while IFS=$'\t' read -r expected cmd; do
    [ -n "$expected" ] || continue
    count=$((count + 1))
    cmd="${cmd//\\n/$'\n'}"
    got=0
    verdict "$cmd" > /dev/null || got=$?
    if [ "$got" != "$expected" ]; then
      echo "block_dangerous: FAIL: expected $expected, got $got: $cmd" >&2
      failed=1
    fi
  done << 'CASES'
2	git push --force origin main
2	git push --force-with-lease origin main
2	git push -f origin master
2	git push origin +main
2	git push --force-with-lease origin feat/x:main
2	git push --force-with-lease origin refs/heads/main
2	cd /repo && git push --force-with-lease origin main
2	git -C /repo push --force origin HEAD:main
2	git push --force-with-lease=main origin feat/x:main
2	git push --force
2	git push --force-with-lease origin some-branch
2	rm -rf build
2	rm -r -f build
2	rm -fr build
2	rm -Rf build
2	rm --recursive --force build
2	rm -rf -- build
2	/bin/rm -rf build
2	\rm -rf build
2	echo ok; rm -rf build # cleanup
2	rm -rf
2	rm -rf "$HOME/scratch"
2	rm -rf ~/scratch
2	rm -rf "$dir"
2	d=$(mktemp); rm -rf "$d"
2	d=$(mktemp -d); rm -rf "$d" build
2	d=$(mktemp -d); rm -rf "$d/../build"
2	rm -rf "$TMPDIR"
2	rm -rf "$TMPDIR"/*
2	rm -rf "$TMPDIR/../build"
2	rm -rf /tmp
2	rm -rf /tmp/
2	rm -rf /tmp/../Users/me
2	rm -rf /tmp/ferro-repo/target
2	rm -rf /tmp/ferro-repo
2	rm -rf /
2	sudo rm -rf /
2	bash -c 'rm -rf build'
2	sh -ec "rm -rf build"
2	eval "rm -rf build"
2	trap 'rm -rf build' EXIT
2	find . -name target -exec rm -rf {} +
2	find . -name target -exec rm -rf {} \;
2	ls | xargs rm -rf
2	ls | xargs -n 1 rm -rf
2	(cd /x && rm -rf build)
2	if true; then rm -rf build; fi
2	git rm LICENSE
2	git -C /repo rm -f crates/x/LICENSE
2	rm CLAUDE.md
2	rm -f ./CLAUDE.md
0	git push --force-with-lease origin feat/maintenance
0	git push --force-with-lease origin fix/main-menu
0	git push --force-with-lease origin feat/x && sed -i 's/main/trunk/' notes.txt
0	gh issue create --title "force push" --body "never git push --force-with-lease origin main"
0	sed -n '/--force-with-lease/p' notes.md; echo main
0	git push origin main
0	git push -u origin feat/x
0	git log --oneline main
0	rm -rf /tmp/scratch
0	rm -rf /private/tmp/scratch
0	rm -rf /var/folders/ab/T/scratch
0	rm -rf /private/var/folders/ab/T/scratch
0	rm -rf "$TMPDIR/scratch"
0	rm -rf "${TMPDIR}/scratch" "$TMPDIR"/other
0	rm notes.txt
0	rm -f notes.txt
0	d=$(mktemp -d); touch "$d/a"; rm -rf "$d"
0	d="$(mktemp -d -t ferro)" && rm -rf "$d"
0	tmp=`mktemp -d`; rm -rf "${tmp}"
0	tmp=$(mktemp -d "$TMPDIR/x.XXXX"); trap 'rm -rf "$tmp"' EXIT; echo hi
0	local tmp; tmp=$(mktemp -d)\nrm -rf "$tmp" 2>/dev/null
0	d=$(mktemp -d) && rm -rf "$d"/*
0	gh issue create --title "rm -rf" --body "never rm -rf build or rm CLAUDE.md"
0	gh issue create --body "a; rm -rf build && git rm LICENSE"
0	gh issue comment 1 --body 'it refused rm -rf "$d" | xargs rm -rf'
0	cat > notes.md <<'EOF'\nrm -rf build\nrm LICENSE\nEOF\necho done
0	echo "rm -rf build" # rm -rf build
0	grep -n "rm -rf" .claude/hooks/block_dangerous.sh
0	git commit -m "fix: refuse rm -rf build"
0	rm -r build
CASES
  if [ "$failed" -ne 0 ]; then
    exit 1
  fi
  echo "block_dangerous: self-test OK ($count cases)."
}

if [ "${1:-}" = "--self-test" ]; then
  self_test
  exit 0
fi

payload="$(cat)"

if command -v jq > /dev/null 2>&1; then
  cmd="$(printf '%s' "$payload" | jq -r '.tool_input.command // empty' 2> /dev/null || true)"
else
  cmd="$payload"
fi
[ -n "${cmd:-}" ] || exit 0

for dir in "${CLAUDE_PROJECT_DIR:-}" "$PWD"; do
  [ -n "$dir" ] || continue
  while IFS= read -r root; do
    [ -n "$root" ] && repo_roots+=("$root")
  done < <(git -C "$dir" rev-parse --path-format=absolute --show-toplevel --git-common-dir 2> /dev/null | sed 's#/\.git$##' || true)
done

status=0
reason="$(verdict "$cmd")" || status=$?
if [ "$status" -ne 0 ]; then
  echo "$reason" >&2
  exit 2
fi
exit 0
