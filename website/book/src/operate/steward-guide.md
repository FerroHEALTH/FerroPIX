<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The steward's guide

A data steward decides what the matcher could not: whether two records
describe one patient. This page walks the work in the order you do it, in
the operator console ([The operator console](console.md)). Every decision,
merge and revert is one steward action in the audit trail with your name
and your reason, and one BALP record on the server's audit trail. A
decision deletes nothing: a record stays a record, and a merge can be
reverted.

## The review queue

The matcher links a pair it is certain of by itself, and leaves a pair
below its possible threshold alone ([Matching](matching.md)). Every pair in
between waits in the review queue at `/queue`: the probable pairs first,
then by score, then the longest wait. Each row shows both leading names,
their birth dates and domains, the score and the grade.

- `j` and `k` move the focused row, and Enter opens the pair.
- At a glance opens the pair in a side panel, with Open to decide and
  Defer.
- The filters narrow the queue by grade, domain system, days waiting, or
  to the pairs deferred until a later day.

## Compare and decide

The pair's page puts the two records side by side, the rows that differ
first, and explains the score: each field's weight, the total, and where
the score falls between the thresholds. Below them are the two patients
the records are in, with their identifiers masked.

You decide in the bar at the foot of the page, always with a reason:

- **Link:** the records are one patient. When they sit in two
  patients, linking merges the second into the first, and the console asks
  first in a dialog that names what moves.
- **Not a match:** the records are two patients. The pair stays out
  of the queue until a record's demographics change.
- **Defer:** decide later. Give the day the pair comes back.

Previous and Next lead to the neighbouring pairs. After a decision the next
pair opens. If another steward decided the
pair first, or a source changed a record after you opened it, the server
refuses with `409` and nothing is written: the bar says so and offers the
next pair or a reload.

## Merge

A merge joins two patients when you know they are one, outside the queue.
On the surviving patient's page choose Merge into…, and name the other
patient by the id FerroPIX gave it, from a search or a pair. The preview
shows, before anything is written:

- both patients, the older surviving unless you swap them;
- the identifiers that move to the survivor;
- the pseudonyms that then resolve to the survivor, whose values never
  change ([Pseudonyms](../integrate/pseudonyms.md));
- the PMIR subscriptions that are told ([The PMIR Registry](../integrate/pmir-registry.md));
- the candidates between the two patients that close as linked.

Merge… asks in a dialog with your reason. The merge carries the versions the
preview read, so a patient that changed in the meantime is refused with
`409` and the preview is read again.

## Unmerge

A merge can be reverted while it is the survivor's latest. On the
survivor's page choose Unmerge…: the page shows the merge with the reason
it was made for and what the revert moves back. When a later merge of the
survivor stands, the page names it, and that merge is reverted first, so
reverts unwind in order (the local unmerge policy PMIR 1.6.0 §2:3.93.4.1.3
admits). The revert, too, asks in a dialog with your reason.

The patient's History tab shows the merge and its revert, and the audit
trail lists both with their reasons ([Security and audit](security-and-audit.md)).
