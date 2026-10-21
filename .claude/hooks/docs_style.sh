#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# .claude/hooks/docs_style.sh
#
# Claude Code PostToolUse hook (matcher: Write|Edit). For an edited file of
# the published prose (the site and book under website/, a root or crate
# README, the changelog, the brand README), run the mechanical half of the
# writing style (scripts/checks/writing-style.sh) and block with exit 2 on a
# hit, so the finding returns as a correction while the file is open. Any
# other file is a quiet exit 0.

set -uo pipefail

payload="$(cat)" || true

if command -v jq > /dev/null 2>&1; then
  file_path="$(printf '%s' "$payload" | jq -r '.tool_input.file_path // empty' 2> /dev/null)" || true
else
  file_path="$(printf '%s' "$payload" | sed -n 's/.*"file_path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)"
fi

repo_root="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "$0")/../.." && pwd)}"
[[ -n "${file_path:-}" && -f "$file_path" ]] || exit 0

case "$file_path" in
  */website/*.md | */website/*.html | */website/*.css | */README.md | */CHANGELOG.md | */CONTRIBUTING.md | */SECURITY.md | */SUPPORT.md) ;;
  *) exit 0 ;;
esac
case "$file_path" in
  */docs/specs/*) exit 0 ;;
  *) ;;
esac

if ! out="$(bash "$repo_root/scripts/checks/writing-style.sh" "$file_path" 2>&1)"; then
  printf '%s\n' "$out" >&2
  exit 2
fi
exit 0
