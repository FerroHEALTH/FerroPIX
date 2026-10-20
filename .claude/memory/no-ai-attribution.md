---
name: no-ai-attribution
description: "Never add AI or Claude attribution to a commit, PR, issue, comment, release or code comment: no Co-Authored-By, no 'Generated with', no emoji footer; absolute family rule, enforced by a PreToolUse hook"
metadata:
  type: feedback
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

No commit, pull request, issue, comment, release note or code comment carries
AI or assistant attribution: no `Co-Authored-By` trailer naming a model, no
"Generated with" line, no robot emoji, no bot footer. This holds even when a
tool or a template suggests adding one.

**Why:** an absolute rule of the owner across every FerroHEALTH repository.

**How to apply:** text describes only the change. The
`.claude/hooks/no_attribution_guard.sh` hook blocks a commit or PR command
that carries attribution; a block means removing the line, never working
around the hook. A contributor who used an AI tool says so in the pull
request description (CONTRIBUTING.md), never in a trailer.
