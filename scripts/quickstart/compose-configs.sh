#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# compose.yaml is self-contained: a reader downloads it alone from the latest
# release, so every file it used to mount from a checkout is inlined as a
# top-level Compose config with `content:` (Compose specification, configs;
# Docker Compose 2.23.1 or newer). The repository keeps each file as the one
# source the tests and the chart read, and this guard holds the inlined copy
# to it (no specification governs this: our own design; #619).
#
# An inlined config names its source on the comment line right above its
# `content: |`:
#
#   configs:
#     ferropix-toml:
#       # source: app/ferropix-server/quickstart.toml
#       content: |
#         ...
#
# The content is the source indented six spaces, every `$` written `$$` so
# Compose interpolates none of it.
#
#   scripts/quickstart/compose-configs.sh [--check] [FILE]   refuse a stale copy
#   scripts/quickstart/compose-configs.sh --write [FILE]     rewrite the copies
#   scripts/quickstart/compose-configs.sh --self-test        prove both
#
# FILE is compose.yaml by default. Exit 0 when every copy matches its source
# (or was rewritten), 1 naming each stale copy or missing source, 2 on a usage
# error.
set -euo pipefail

# render FILE: FILE with every inlined config rewritten from its source, the
# sources read relative to the directory of FILE.
render() {
  local file=$1
  ROOT="$(cd "$(dirname "$file")" && pwd)" perl -e '
    use strict; use warnings;
    my ($file) = @ARGV;
    open my $in, "<", $file or die "compose-configs: cannot read $file\n";
    my @lines = <$in>;
    my @out; my $i = 0; my $count = 0;
    while ($i < @lines) {
      my $line = $lines[$i];
      my ($path) = $line =~ /^    # source: (\S+)\s*$/;
      if (defined $path && $i + 1 < @lines && $lines[$i + 1] =~ /^    content: \|\s*$/) {
        open my $src, "<", "$ENV{ROOT}/$path" or die "compose-configs: $file inlines $path, which does not exist\n";
        my @body = <$src>;
        push @out, $line, $lines[$i + 1];
        $i += 2;
        $i++ while $i < @lines && ($lines[$i] =~ /^      / || $lines[$i] =~ /^\s*$/ && $i + 1 < @lines && $lines[$i + 1] =~ /^      /);
        for my $b (@body) {
          (my $c = $b) =~ s/\$/\$\$/g;
          $c =~ s/\s+$//;
          push @out, ($c eq "" ? "\n" : "      $c\n");
        }
        $count++;
        next;
      }
      push @out, $line;
      $i++;
    }
    die "compose-configs: $file inlines no config\n" unless $count;
    print @out;
  ' "$file"
}

mode=check
file=compose.yaml
case "${1:-}" in
  --self-test)
    work="$(mktemp -d)"
    trap 'rm -rf "$work"' EXIT
    mkdir -p "$work/conf"
    printf '[a]\nkey = "%sx"\n\n[b]\n' '$' > "$work/conf/a.toml"
    printf 'configs:\n  a:\n    # source: conf/a.toml\n    content: |\n      [a]\n      key = "%sx"\n\n      [b]\nvolumes:\n  v:\n' '$$' > "$work/compose.yaml"
    fails=0
    bash "$0" --check "$work/compose.yaml" > /dev/null 2>&1 || { echo "compose-configs: self-test failed: a current copy was refused." >&2; fails=1; }
    printf '[a]\nkey = "%sy"\n' '$' > "$work/conf/a.toml"
    if bash "$0" --check "$work/compose.yaml" > /dev/null 2>&1; then
      echo "compose-configs: self-test failed: a stale copy passed." >&2
      fails=1
    fi
    bash "$0" --write "$work/compose.yaml" > /dev/null
    grep -qxF "      key = \"\$\$y\"" "$work/compose.yaml" || { echo "compose-configs: self-test failed: --write did not rewrite the copy." >&2; fails=1; }
    grep -qx 'volumes:' "$work/compose.yaml" || { echo "compose-configs: self-test failed: --write lost what follows the copy." >&2; fails=1; }
    rm "$work/conf/a.toml"
    if bash "$0" --check "$work/compose.yaml" > /dev/null 2>&1; then
      echo "compose-configs: self-test failed: a missing source passed." >&2
      fails=1
    fi
    [[ "$fails" == 0 ]] || exit 1
    echo "compose-configs: self-test OK."
    exit 0
    ;;
  --write) mode="write"; shift ;;
  --check) shift ;;
  -*) echo "usage: $0 [--check|--write] [FILE] | --self-test" >&2; exit 2 ;;
  *) ;;
esac
[[ $# -le 1 ]] || { echo "usage: $0 [--check|--write] [FILE] | --self-test" >&2; exit 2; }
file="${1:-$file}"
if [[ "$file" == compose.yaml ]]; then
  cd "$(dirname "${BASH_SOURCE[0]}")/../.."
fi
rendered="$(render "$file")"
if [[ "$mode" == write ]]; then
  printf '%s\n' "$rendered" > "$file"
  echo "compose-configs: rewrote the inlined configs of $file."
elif [[ "$rendered" != "$(cat "$file")" ]]; then
  echo "compose-configs: an inlined config of $file differs from its source; run scripts/quickstart/compose-configs.sh --write" >&2
  diff <(printf '%s\n' "$rendered") "$file" >&2 || true
  exit 1
else
  echo "compose-configs: every inlined config of $file matches its source."
fi
