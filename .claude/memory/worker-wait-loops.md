---
name: worker-wait-loops
description: "Never wait with a pgrep -f whose pattern the loop's own command line contains; run long commands in the background, poll at most every 60 seconds, one wait per output; push a finished branch rather than holding it for local reruns (FerroBRIDGE, 2026-10-16)"
metadata:
  type: feedback
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

In FerroBRIDGE, workers left shells polling for hours with `until ! pgrep -f
"<command>"`: the pattern matches the loop's own command line, so it never
ends. One loop ran 4.5 hours after its worker was gone.

**How to apply:**

- Run a long command with `run_in_background` and wait for its completion
  notification. Never poll with `pgrep -f` on a string the loop itself
  contains, and keep one wait per output.
- A polling loop (CI checks, a merge queue) sleeps at least 60 seconds
  between reads.
- A finished branch waits for nobody: push once your own tests and the
  local gates pass, and let CI run the rest (owner, 2026-10-16).
- Stop a stuck process by its PID, never with a broad `pkill -f`.
