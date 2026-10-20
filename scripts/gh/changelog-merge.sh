#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/gh/changelog-merge.sh: merges a branch's CHANGELOG.md onto main's
# by entry, so the branch's new entries stay under [Unreleased].
#
# A line merge cannot do this. When a release moves [Unreleased] into a
# version section after the branch was cut, git sees the branch's new bullet
# next to lines that now sit under that version, and a clean merge files the
# bullet there (#903 landed under [0.0.9] this way). This program reads the
# [Unreleased] section of the merge base, of the branch and of main as
# subsections of bullets: the bullets the branch added are placed in main's
# [Unreleased] under their own heading, after the bullet that precedes them on
# the branch; the bullets it removed are removed. Main's released sections
# and link references are kept byte for byte.
#
# It refuses (exit 1, nothing written) a branch that changed anything outside
# [Unreleased], such as a release bump, and an [Unreleased] section whose text
# lines it cannot read back in order, naming the first such line. Blank lines
# are layout: one between bullets is kept, and a heading gets one above it.
# No specification governs this: our own design over the Keep a Changelog
# 1.1.0 layout.
#
# Usage:
#   scripts/gh/changelog-merge.sh BASE BRANCH MAIN   the merged file on stdout
#   scripts/gh/changelog-merge.sh --released FILE    the released part of FILE
#   scripts/gh/changelog-merge.sh --self-test        the #902/#903 order and
#                                                    the refusals, on fixtures
set -euo pipefail

die() {
  echo "changelog-merge: $*" >&2
  exit 1
}

# The engine. Arguments: a mode (merge or released) and the files.
engine() {
  perl -e '
use strict;
use warnings;

sub slurp {
  my ($path) = @_;
  open(my $fh, "<", $path) or die "cannot read $path: $!\n";
  local $/;
  my $text = <$fh>;
  close($fh);
  return $text;
}

# (preamble through the [Unreleased] heading line, its body, the rest).
sub split_file {
  my ($text) = @_;
  $text =~ /\A(.*?^## \[Unreleased\][^\n]*\n)(.*?)(^## \[|^\[[^\]\n]+\]: |\z)/ms
    or die "no ## [Unreleased] heading\n";
  my ($pre, $body) = ($1, $2);
  my $rest = substr($text, length($pre) + length($body));
  return ($pre, $body, $rest);
}

# The body as ([heading, [entry, ...]], ...), an entry being one bullet
# with its continuation lines and the blank lines inside and after it.
sub parse_body {
  my ($body) = @_;
  my @sections;
  my $entry;
  my @lines = split(/\n/, $body, -1);
  pop(@lines) if @lines && $lines[-1] eq "";
  for (my $i = 0; $i < @lines; $i++) {
    my $line = $lines[$i];
    if ($line =~ /^### /) {
      push(@sections, [$line, []]);
      $entry = undef;
    } elsif ($line =~ /^- /) {
      die "a bullet outside a ### subsection\n" unless @sections;
      $entry = [$line];
      push(@{ $sections[-1][1] }, $entry);
    } elsif ($line eq "") {
      my $next = $i + 1 < @lines ? $lines[$i + 1] : "";
      push(@$entry, $line) if $entry && $next =~ /^(  |- )/;
    } elsif ($line =~ /^  / && $entry) {
      push(@$entry, $line);
    } else {
      die "a line this program cannot place: $line\n";
    }
  }
  return \@sections;
}

sub render_body {
  my ($sections) = @_;
  my $out = "";
  for my $s (@$sections) {
    my @entries = @{ $s->[1] };
    next unless @entries;
    $out .= "\n$s->[0]\n\n";
    for my $i (0 .. $#entries) {
      my @lines = @{ $entries[$i] };
      if ($i == $#entries) {
        pop(@lines) while @lines > 1 && $lines[-1] eq "";
      }
      $out .= join("\n", @lines) . "\n";
    }
  }
  return $out . "\n";
}

# An entry by its text, the blank lines after it left out.
sub key_of {
  my @lines = @{ $_[0] };
  pop(@lines) while @lines > 1 && $lines[-1] eq "";
  return join("\n", @lines);
}

# The first text line a reading loses, reorders or invents, or the empty
# string when the two carry the same text lines in the same order. Blank
# lines are layout, which the rendering normalizes.
sub first_difference {
  my ($want, $got) = @_;
  my @w = grep { $_ ne "" } split(/\n/, $want);
  my @g = grep { $_ ne "" } split(/\n/, $got);
  for my $i (0 .. ($#w > $#g ? $#w : $#g)) {
    my $a = $i <= $#w ? $w[$i] : "(end)";
    my $b = $i <= $#g ? $g[$i] : "(end)";
    return sprintf("text line %d reads \"%s\", rendered as \"%s\"", $i + 1, $a, $b) if $a ne $b;
  }
  return "";
}

sub entry_set {
  my ($sections) = @_;
  my %set;
  $set{ key_of($_) }++ for map { @{ $_->[1] } } @$sections;
  return \%set;
}

my $mode = shift(@ARGV);
if ($mode eq "released") {
  my (undef, undef, $rest) = split_file(slurp($ARGV[0]));
  print $rest;
  exit 0;
}

my ($base_pre, $base_body, $base_rest) = split_file(slurp($ARGV[0]));
my ($ours_pre, $ours_body, $ours_rest) = split_file(slurp($ARGV[1]));
my ($main_pre, $main_body, $main_rest) = split_file(slurp($ARGV[2]));
die "the branch changed the text above [Unreleased]\n" if $ours_pre ne $base_pre;
die "the branch changed a released section or a link reference\n" if $ours_rest ne $base_rest;

my $base = parse_body($base_body);
my $ours = parse_body($ours_body);
my $main = parse_body($main_body);
for my $side (["main", $main, $main_body], ["the branch", $ours, $ours_body], ["the merge base", $base, $base_body]) {
  my ($name, $parsed, $body) = @$side;
  my $lost = first_difference($body, render_body($parsed));
  die "$name" . "'"'"'s [Unreleased] does not read back: $lost\n" if $lost ne "";
}

my $base_set = entry_set($base);
my $ours_set = entry_set($ours);

# The bullets the branch removed or reworded leave main too.
for my $s (@$main) {
  $s->[1] = [ grep { !exists $base_set->{ key_of($_) } || exists $ours_set->{ key_of($_) } } @{ $s->[1] } ];
}

# The bullets the branch added, each after its predecessor on the branch.
# The subsections of main keep their order; a heading main lacks goes before
# the first subsection of main that the Keep a Changelog order puts after it.
my @order = ("### Added", "### Changed", "### Deprecated", "### Removed", "### Fixed", "### Security");
my %rank;
@rank{@order} = (0 .. $#order);
sub rank_of { return exists $rank{ $_[0] } ? $rank{ $_[0] } : scalar(@order); }
for my $s (@$ours) {
  my ($heading, $entries) = @$s;
  my ($target) = grep { $_->[0] eq $heading } @$main;
  unless ($target) {
    $target = [$heading, []];
    my $at = scalar(@$main);
    for my $i (reverse 0 .. $#{$main}) {
      $at = $i if rank_of($main->[$i][0]) > rank_of($heading);
    }
    splice(@$main, $at, 0, $target);
  }
  my $previous;
  for my $e (@$entries) {
    my $key = key_of($e);
    if (exists $base_set->{$key}) {
      $previous = $key;
      next;
    }
    my $at = 0;
    if (defined $previous) {
      for my $i (0 .. $#{ $target->[1] }) {
        $at = $i + 1 if key_of($target->[1][$i]) eq $previous;
      }
    }
    splice(@{ $target->[1] }, $at, 0, $e);
    $previous = $key;
  }
}
print $main_pre, render_body($main), $main_rest;
' "$@"
}

self_test() {
  local work failures=0
  work="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand $work now, while it is set
  trap "rm -rf '$work'" EXIT
  head_text='# Changelog

## [Unreleased]
'
  links='[Unreleased]: https://example.org/compare/v0.0.9...HEAD
[0.0.9]: https://example.org/compare/v0.0.8...v0.0.9
'
  e1='- The Demographic API (#894). One entry
  that wraps.'
  e2='- Sub-contexts (#864).'
  new='- Party contexts compile (#898). A context
  rooted at a PERSON is a party context.'
  printf '%s\n### Added\n\n%s\n%s\n\n## [0.0.8] - 2026-10-18\n\n### Added\n\n- Old.\n\n%s' \
    "$head_text" "$e1" "$e2" "$links" >"$work/base"
  printf '%s\n## [0.0.9] - 2026-10-19\n\n### Added\n\n%s\n%s\n\n## [0.0.8] - 2026-10-18\n\n### Added\n\n- Old.\n\n%s' \
    "$head_text" "$e1" "$e2" "$links" >"$work/main"
  printf '%s\n### Added\n\n%s\n%s\n%s\n\n## [0.0.8] - 2026-10-18\n\n### Added\n\n- Old.\n\n%s' \
    "$head_text" "$new" "$e1" "$e2" "$links" >"$work/ours"
  printf '%s\n### Added\n\n%s\n\n## [0.0.9] - 2026-10-19\n\n### Added\n\n%s\n%s\n\n## [0.0.8] - 2026-10-18\n\n### Added\n\n- Old.\n\n%s' \
    "$head_text" "$new" "$e1" "$e2" "$links" >"$work/want"

  # The defect, reproduced: git'"'"'s line merge files the entry under [0.0.9].
  cp "$work/ours" "$work/line"
  git merge-file -q "$work/line" "$work/base" "$work/main" || true
  if ! awk '/^## \[0.0.9\]/{r=1} r && /Party contexts/{f=1} END{exit !f}' "$work/line"; then
    echo "self-test: the line merge no longer reproduces the #902/#903 order" >&2
    failures=$((failures + 1))
  fi

  # The fix: the entry stays under [Unreleased].
  if ! engine merge "$work/base" "$work/ours" "$work/main" >"$work/got" ||
    ! cmp -s "$work/got" "$work/want"; then
    echo "self-test: the #902/#903 order did not merge as expected:" >&2
    diff "$work/want" "$work/got" >&2 || true
    failures=$((failures + 1))
  fi

  # Main gained an entry under another heading; the branch reworded e2 and
  # added one after e1. Both land, the reworded one in place of the old.
  printf '%s\n### Added\n\n%s\n%s\n\n## [0.0.8] - 2026-10-18\n\n### Added\n\n- Old.\n\n%s' \
    "$head_text" "$e1" "$e2" "$links" >"$work/base2"
  printf '%s\n### Added\n\n%s\n%s\n\n### Fixed\n\n- A fix on main.\n\n## [0.0.8] - 2026-10-18\n\n### Added\n\n- Old.\n\n%s' \
    "$head_text" "$e1" "$e2" "$links" >"$work/main2"
  printf '%s\n### Added\n\n%s\n- After e1.\n- Sub-contexts (#864), reworded.\n\n## [0.0.8] - 2026-10-18\n\n### Added\n\n- Old.\n\n%s' \
    "$head_text" "$e1" "$links" >"$work/ours2"
  printf '%s\n### Added\n\n%s\n- After e1.\n- Sub-contexts (#864), reworded.\n\n### Fixed\n\n- A fix on main.\n\n## [0.0.8] - 2026-10-18\n\n### Added\n\n- Old.\n\n%s' \
    "$head_text" "$e1" "$links" >"$work/want2"
  if ! engine merge "$work/base2" "$work/ours2" "$work/main2" >"$work/got2" ||
    ! cmp -s "$work/got2" "$work/want2"; then
    echo "self-test: an entry beside main's new heading did not merge as expected:" >&2
    diff "$work/want2" "$work/got2" >&2 || true
    failures=$((failures + 1))
  fi

  # A release bump changes a released section: refused.
  if engine merge "$work/base" "$work/main" "$work/base" >/dev/null 2>&1; then
    echo "self-test: a branch that cuts a release was merged instead of refused" >&2
    failures=$((failures + 1))
  fi

  # A line that is no bullet, heading or continuation: refused.
  printf '%s\nA stray paragraph.\n\n%s' "$head_text" "$links" >"$work/odd"
  if engine merge "$work/base" "$work/base" "$work/odd" >/dev/null 2>&1; then
    echo "self-test: an [Unreleased] it cannot read was merged instead of refused" >&2
    failures=$((failures + 1))
  fi

  # #942, from its real merge base, head and main: main's [Unreleased] keeps
  # a blank line between two bullets, which a reading that dropped it refused.
  local data
  data="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/testdata/changelog-merge"
  if ! engine merge "$data/942-base.md" "$data/942-branch.md" "$data/942-main.md" >"$work/got942" ||
    ! cmp -s "$work/got942" "$data/942-merged.md"; then
    echo "self-test: the #942 merge did not give 942-merged.md" >&2
    diff "$data/942-merged.md" "$work/got942" >&2 || true
    failures=$((failures + 1))
  fi

  # A heading with no blank line above it reads, and renders with one.
  printf '%s\n### Added\n\n%s\n### Fixed\n\n- A fix.\n\n## [0.0.8] - 2026-10-18\n\n### Added\n\n- Old.\n\n%s' \
    "$head_text" "$e1" "$links" >"$work/tight"
  printf '%s\n### Added\n\n%s\n\n### Fixed\n\n- A fix.\n\n## [0.0.8] - 2026-10-18\n\n### Added\n\n- Old.\n\n%s' \
    "$head_text" "$e1" "$links" >"$work/tight-want"
  if ! engine merge "$work/tight" "$work/tight" "$work/tight" >"$work/tight-got" ||
    ! cmp -s "$work/tight-got" "$work/tight-want"; then
    echo "self-test: a heading with no blank line above it did not read" >&2
    failures=$((failures + 1))
  fi

  # A heading with no bullet would be lost: refused, naming the line.
  printf '%s\n### Removed\n\n%s' "$head_text" "$links" >"$work/empty"
  if engine merge "$work/base" "$work/base" "$work/empty" >/dev/null 2>"$work/empty-err" ||
    ! grep -q 'reads "### Removed"' "$work/empty-err"; then
    echo "self-test: an empty subsection was not refused by name" >&2
    failures=$((failures + 1))
  fi

  # The released part starts at the first version heading.
  if [ "$(engine released "$work/main" | head -n1)" != "## [0.0.9] - 2026-10-19" ]; then
    echo "self-test: --released does not start at the first version" >&2
    failures=$((failures + 1))
  fi

  [ "$failures" = 0 ] || die "self-test: $failures check(s) failed"
  echo "changelog-merge: self-test passed"
}

case "${1:-}" in
  --self-test) self_test ;;
  --released)
    [ $# -eq 2 ] || die "usage: --released FILE"
    engine released "$2"
    ;;
  -*) die "unknown option $1" ;;
  *)
    [ $# -eq 3 ] || die "usage: changelog-merge.sh BASE BRANCH MAIN"
    engine merge "$1" "$2" "$3"
    ;;
esac
