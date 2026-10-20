#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/gh/watch-prs.sh: watches pull requests until each is merged.
#
# Usage: scripts/gh/watch-prs.sh <pr> [<pr>...]   (run it in the background)
#
# Run a long watch from a copy outside the worktree: bash reads a script as
# it runs, so a branch switch or rebase mid-watch mixes two versions of it.
#
# - BEHIND: updates the branch.
# - DIRTY in CHANGELOG.md or only in version lines: merges origin/main
#   (the branch's changelog entries by scripts/gh/changelog-merge.sh, the
#   higher of the two versions on each version line, a fresh Cargo.lock, the
#   crate bump guard passing), pushes, and keeps watching.
# - A clean merge that would file the branch's changelog entries under a
#   released version (a release cut after the branch): merges the same way.
# - Open and not queued: arms auto-merge again, since a push to the branch
#   (an update or a resolved merge) drops the arming.
# - DIRTY elsewhere, or a red required check with nothing pending: stops
#   (exit 2 or 3) so the session fixes it. Advisory analyzers never stop it.
#   Only the newest run of each workflow on the head is judged, so a run a
#   later push superseded or cancelled never stops it.
#
# scripts/gh/watch-prs.sh --self-test   judges fixture checks and merges
# fixture version conflicts, with no gh call.
set -u
# Resolves, in place, each conflict hunk of file $1 whose two sides differ
# only in their x.y.z version numbers: the hunk becomes one side with the
# higher version at each position. Any other hunk keeps its markers. Every
# capture is copied before the next match, which would overwrite it.
merge_versions() {
  perl -0pi -e '
    sub newer { my @a = split /\./, $_[0]; my @b = split /\./, $_[1];
      for my $i (0 .. 2) { my $c = $a[$i] <=> $b[$i]; return $c if $c } 0 }
    sub pick { my ($ours, $theirs, $hunk) = @_;
      my @ov = $ours =~ /(\d+\.\d+\.\d+)/g;
      my @tv = $theirs =~ /(\d+\.\d+\.\d+)/g;
      (my $on = $ours) =~ s/\d+\.\d+\.\d+/V/g;
      (my $tn = $theirs) =~ s/\d+\.\d+\.\d+/V/g;
      return $hunk if $on ne $tn;
      my $i = 0;
      $ours =~ s{\d+\.\d+\.\d+}{ my ($o, $t) = ($ov[$i], $tv[$i]); $i++;
        newer($o, $t) >= 0 ? $o : $t }ge;
      $ours }
    s{<<<<<<< HEAD\n(.*?)=======\n(.*?)>>>>>>> origin/main\n}{pick($1, $2, $&)}gse' "$1"
}
# The checks of the newest run of each workflow, from `gh pr checks --json
# name,bucket,link,workflow`; a check whose link names no Actions run is kept.
# Prints the pending count, a tab, and the red required checks.
judge() {
  jq -r '
    def run: ((.link // "") | capture("/actions/runs/(?<r>[0-9]+)") | .r | tonumber) // 0;
    def advisory: .name == "conclusion" or .name == "analyze" or .name == "CodeQL"
      or .name == "Analyze (rust)" or .name == "SonarCloud Code Analysis";
    map(. + {run: run})
    | group_by(.workflow)
    | map((map(.run) | max) as $newest | map(select(.run == $newest)))
    | flatten
    | [([.[] | select(.bucket == "pending")] | length | tostring),
       ([.[] | select(.bucket == "fail" and (advisory | not)) | .name] | join(" "))]
    | join("\t")'
}
self_test() {
  local failures=0 got
  # A cancelled run's failed jobs, superseded by a newer green run of CI.
  got=$(judge <<'EOF'
[{"name":"clippy (${{ matrix.packages }})","bucket":"fail","workflow":"CI","link":"https://github.com/o/r/actions/runs/100/job/1"},
 {"name":"clippy (a b)","bucket":"pass","workflow":"CI","link":"https://github.com/o/r/actions/runs/101/job/2"},
 {"name":"build","bucket":"pass","workflow":"Docs","link":"https://github.com/o/r/actions/runs/99/job/3"}]
EOF
  )
  if [ "$got" != "0	" ]; then
    echo "watch-prs: self-test: a superseded run was judged: [$got]" >&2
    failures=$((failures + 1))
  fi
  # The newest run is red, and an advisory analyzer never counts.
  got=$(judge <<'EOF'
[{"name":"clippy (a b)","bucket":"pass","workflow":"CI","link":"https://github.com/o/r/actions/runs/100/job/1"},
 {"name":"test (a b)","bucket":"fail","workflow":"CI","link":"https://github.com/o/r/actions/runs/101/job/2"},
 {"name":"analyze","bucket":"fail","workflow":"SonarQube Cloud","link":"https://github.com/o/r/actions/runs/102/job/3"}]
EOF
  )
  if [ "$got" != "0	test (a b)" ]; then
    echo "watch-prs: self-test: the newest red run was not judged: [$got]" >&2
    failures=$((failures + 1))
  fi
  # A pending job of the newest run holds the verdict.
  got=$(judge <<'EOF'
[{"name":"test (a b)","bucket":"fail","workflow":"CI","link":"https://github.com/o/r/actions/runs/101/job/2"},
 {"name":"e2e","bucket":"pending","workflow":"CI","link":"https://github.com/o/r/actions/runs/101/job/4"},
 {"name":"status","bucket":"fail","workflow":"","link":"https://example.org/status"}]
EOF
  )
  if [ "$got" != "1	status test (a b)" ]; then
    echo "watch-prs: self-test: a pending job or a check outside Actions was misread: [$got]" >&2
    failures=$((failures + 1))
  fi
  local dir
  dir=$(mktemp -d)
  # Both sides bumped one requirement, main higher: main's line, whole.
  printf '%s\n' 'a = 1' '<<<<<<< HEAD' 'x = { path = "x", version = "0.1.157" }' \
    '=======' 'x = { path = "x", version = "0.1.161" }' '>>>>>>> origin/main' \
    '# The next comment.' >"$dir/main-higher.toml"
  merge_versions "$dir/main-higher.toml"
  if [ "$(cat "$dir/main-higher.toml")" != "$(printf '%s\n' 'a = 1' \
    'x = { path = "x", version = "0.1.161" }' '# The next comment.')" ]; then
    echo "watch-prs: self-test: a both-sides bump kept the lower version or broke the line:" >&2
    cat "$dir/main-higher.toml" >&2
    failures=$((failures + 1))
  fi
  # The branch higher, at each position of a two-line hunk.
  printf '%s\n' '<<<<<<< HEAD' 'version = "0.2.0"' 'y = "1.10.0"' '=======' \
    'version = "0.1.9"' 'y = "1.9.3"' '>>>>>>> origin/main' >"$dir/branch-higher.toml"
  merge_versions "$dir/branch-higher.toml"
  if [ "$(cat "$dir/branch-higher.toml")" != "$(printf '%s\n' 'version = "0.2.0"' 'y = "1.10.0"')" ]; then
    echo "watch-prs: self-test: the branch's higher versions were not kept:" >&2
    cat "$dir/branch-higher.toml" >&2
    failures=$((failures + 1))
  fi
  # Sides that differ beyond a version keep their markers, so the merge stops.
  printf '%s\n' '<<<<<<< HEAD' 'x = { version = "0.1.157" }' '=======' \
    'x = { path = "x", version = "0.1.161" }' '>>>>>>> origin/main' >"$dir/other.toml"
  cp "$dir/other.toml" "$dir/other.expected"
  merge_versions "$dir/other.toml"
  if ! cmp -s "$dir/other.toml" "$dir/other.expected"; then
    echo "watch-prs: self-test: a hunk differing beyond its versions was rewritten:" >&2
    cat "$dir/other.toml" >&2
    failures=$((failures + 1))
  fi
  rm -f "$dir/main-higher.toml" "$dir/branch-higher.toml" "$dir/other.toml" "$dir/other.expected"
  rmdir "$dir"
  if [ "$failures" != 0 ]; then
    echo "watch-prs: self-test: $failures check(s) failed" >&2
    exit 1
  fi
  echo "watch-prs: self-test passed"
}
if [ "${1:-}" = --self-test ]; then
  self_test
  exit 0
fi
if [ $# -eq 0 ]; then
  echo "usage: scripts/gh/watch-prs.sh <pr> [<pr>...]" >&2
  exit 64
fi
repo=$(git rev-parse --show-toplevel) || exit 1
merge_changelog="$(cd "$(dirname "$0")" && pwd)/changelog-merge.sh"
# A conflict this script resolves on its own: CHANGELOG.md takes the branch's
# entries into main's [Unreleased] whatever main released meanwhile, and a
# version line (a crate manifest, the root manifest, the pin tables)
# keeps the higher version when the two sides differ only in their version
# numbers. Every manifest must still parse, Cargo.lock is regenerated, and
# the crate bump guard must pass.
resolve_merge() {
  local p=$1 b tmp
  b=$(gh pr view "$p" --json headRefName --jq .headRefName)
  tmp=$(mktemp -d)
  git -C "$repo" fetch -q origin || return 1
  git -C "$repo" worktree add -q "$tmp" "origin/$b" 2>/dev/null || return 1
  (
    cd "$tmp" || exit 1
    git -c merge.conflictStyle=merge merge --no-commit --no-ff origin/main >/dev/null 2>&1
    mb=$(git merge-base HEAD origin/main) || { git merge --abort; exit 1; }
    if ! git diff --quiet "$mb" HEAD -- CHANGELOG.md; then
      "$merge_changelog" <(git show "$mb:CHANGELOG.md") <(git show HEAD:CHANGELOG.md) \
        <(git show origin/main:CHANGELOG.md) >CHANGELOG.md.merged ||
        { rm -f CHANGELOG.md.merged; git merge --abort 2>/dev/null; exit 1; }
      mv CHANGELOG.md.merged CHANGELOG.md
      git add CHANGELOG.md
    fi
    for f in $(git diff --name-only --diff-filter=U); do
      case $f in
        Cargo.lock)
          git checkout --theirs Cargo.lock ;;
        Cargo.toml | crates/*/Cargo.toml | app/*/Cargo.toml | docs/VERSIONS.md | docs/architecture.md)
          merge_versions "$f" ;;
        *) git merge --abort; exit 1 ;;
      esac
      if grep -q '^<<<<<<<\|^>>>>>>>' "$f"; then git merge --abort; exit 1; fi
    done
    # The lock exists once the workspace has a member; an empty one has none.
    if [ -f Cargo.lock ]; then
      cargo metadata --no-deps --format-version 1 >/dev/null 2>&1 || { git merge --abort; exit 1; }
      cargo update -w -q || { git merge --abort; exit 1; }
    fi
    git add -A
    scripts/checks/crate-version-guard.sh origin/main WORKTREE >/dev/null 2>&1 || { git merge --abort; exit 1; }
    git commit -q --no-edit && git push -q origin "HEAD:$b"
  )
  local rc=$?
  git -C "$repo" worktree remove --force "$tmp" >/dev/null 2>&1
  return $rc
}
# Whether a clean merge with main would change main's released sections: the
# branch's changelog entries would land under a version cut after it.
changelog_leaks() {
  local p=$1 b tree
  gh pr view "$p" --json files --jq '.files[].path' | grep -qx CHANGELOG.md || return 1
  b=$(gh pr view "$p" --json headRefName --jq .headRefName)
  git -C "$repo" fetch -q origin main "$b" || return 1
  # A branch that already holds main has nothing left to merge.
  git -C "$repo" merge-base --is-ancestor origin/main "origin/$b" && return 1
  tree=$(git -C "$repo" merge-tree --write-tree origin/main "origin/$b" 2>/dev/null | head -n1) || return 1
  ! cmp -s <("$merge_changelog" --released <(git -C "$repo" show "$tree:CHANGELOG.md")) \
    <("$merge_changelog" --released <(git -C "$repo" show origin/main:CHANGELOG.md))
}
left=("$@")
# A run started by a fresh push takes a moment to register; until it does,
# gh pr checks still shows the previous run.
sleep 90
while [ ${#left[@]} -gt 0 ]; do
  next=()
  for p in "${left[@]}"; do
    s=$(gh pr view "$p" --json state,mergeStateStatus --jq '.state + " " + .mergeStateStatus')
    st=${s% *}
    ms=${s#* }
    if [ "$st" = MERGED ]; then echo "MERGED #$p"; continue; fi
    if [ "$st" = CLOSED ]; then echo "CLOSED #$p"; continue; fi
    # A push drops the auto-merge arming; re-arming a queued PR is refused
    # harmlessly, so this runs on every pass.
    gh pr merge "$p" --auto >/dev/null 2>&1 || true
    case $ms in
      BEHIND) gh pr update-branch "$p" >/dev/null 2>&1 && echo "updated #$p" ;;
      DIRTY)
        if resolve_merge "$p"; then echo "resolved merge #$p"; next+=("$p"); continue
        else echo "CONFLICT #$p"; exit 2; fi ;;
      *)
        if changelog_leaks "$p"; then
          if resolve_merge "$p"; then echo "kept the changelog entries of #$p unreleased"; next+=("$p"); continue
          else echo "CONFLICT #$p: its changelog entries would land under a released version"; exit 2; fi
        fi ;;
    esac
    # gh exits non-zero while a check is pending or red and still prints them.
    verdict=$(gh pr checks "$p" --json name,bucket,link,workflow 2>/dev/null | judge) || verdict="1	"
    pend=${verdict%%	*}
    fails=${verdict#*	}
    if [ "$pend" = 0 ] && [ -n "$fails" ]; then echo "RED #$p: $fails"; exit 3; fi
    next+=("$p")
  done
  left=("${next[@]+"${next[@]}"}")
  [ ${#left[@]} -gt 0 ] && sleep 60
done
echo "ALL MERGED"
