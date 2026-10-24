<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Matching

FerroPIX decides that two records are one patient in three tiers. An
identifier in an `authoritative` domain links two records on its own. The
probabilistic scorer on this page compares demographics and grades a pair.
A data steward decides the pairs the scorer is unsure of. No IHE profile
governs how a Manager cross-references: PIXm leaves it to the Manager, and
PDQm asks only that a match carry a score and a grade. The rules below are
FerroPIX's own.

The scorer is built. Recording its candidates when a feed arrives is
planned in [#60](https://github.com/FerroHEALTH/FerroPIX/issues/60),
answering ITI-119 with its score and grade in
[#73](https://github.com/FerroHEALTH/FerroPIX/issues/73), and the review
queue in [#107](https://github.com/FerroHEALTH/FerroPIX/issues/107).

## How a pair is scored

The scorer follows Fellegi and Sunter. For each field you configure two
probabilities: `m`, that the field agrees when the two records are one
patient, and `u`, that it agrees for two records picked at random. A field
that agrees adds `log2(m / u)` bits to the total, a field that differs adds
`log2((1 - m) / (1 - u))` (a negative number), a partial agreement adds half
the agreement weight, and a field missing on either side adds nothing.

The total maps to a score between 0 and 1:

```text
score = 1 / (1 + 2^(10 - total))
```

A total of 10 bits scores 0.5: the evidence for the pair has to outweigh
odds of one in 1,024 that two records sharing a blocking key are one
patient. The mapping is the same in every deployment, so a score means the
same thing everywhere.

## The fields and their defaults

| Field | `m` | `u` | Agree adds | Differ adds | How it compares |
|---|---|---|---|---|---|
| `family_name` | 0.95 | 0.01 | 6.57 | -4.31 | Jaro-Winkler over the folded name: 0.92 or more agrees, 0.85 to 0.92 agrees half |
| `given_names` | 0.90 | 0.02 | 5.49 | -3.29 | Jaro-Winkler over the first given name, or any given name equal; agreeing initials count half |
| `birth_date` | 0.97 | 0.003 | 8.34 | -5.05 | equal to the day; the day and month swapped, or one digit off, counts half; a date without a day is missing |
| `gender` | 0.98 | 0.5 | 0.97 | -4.64 | equal; `unknown` counts as missing |
| `postal_code` | 0.85 | 0.001 | 9.73 | -2.74 | equal; the first four characters equal counts half |
| `address` | 0.80 | 0.001 | 9.64 | -2.32 | the words of the lines and the city: agrees when the other address holds half the words of the shorter one |
| `telephone` | 0.60 | 0.0001 | 12.55 | -1.32 | the last nine digits, equal |
| `email` | 0.60 | 0.0001 | 12.55 | -1.32 | equal after trimming and lowercasing |
| `mothers_maiden_name` | 0.90 | 0.01 | 6.49 | -3.31 | Jaro-Winkler, 0.92 or more agrees |
| `local_identifier` | 0.50 | 0.00001 | 15.61 | -1.00 | the same value in a `local` domain both records carry an identifier of; evidence, never proof |

Names and addresses are folded before they are compared: accents removed,
lowercased, punctuation read as a space. Where a field holds several values
(several names, addresses or numbers), the best pair counts. Override a
field under `[matching.fields.<field>]` in the
[configuration](configuration.md#matching); each override must hold
`0 < u < m < 1`.

## Grades

| Grade | When | What happens |
|---|---|---|
| `certain` | the score is at or above `link_at` (0.95) | a candidate at the top of the queue; linked without a steward only with `auto_link = true` |
| `probable` | at or above `review_at` (0.80) | a candidate in the review queue |
| `possible` | at or above `possible_at` (0.60) | a candidate, below the probable ones |
| `certainly-not` | below `possible_at` | not queued |

Change a threshold in `[matching]` and the grade of every later score moves
with it; no code changes.

## Blocking

A pair is scored only when the two records share a blocking key, so a feed
scores the few records that could be the same patient and never the whole
table:

- `birth_year_family_phonetic`: the birth year and the Soundex code of the
  last word of the family name, so a prefix such as `van der` does not
  decide the block;
- `postal_code_birth_date`: the postal code and the full birth date;
- `shared_identifier`: an identifier both records carry.

## Evidence

Every score keeps its evidence: one entry per field with the comparator,
the two values as compared, masked to their last three characters (at most
half the value), how the field agreed, the comparator's reading (such as the
Jaro-Winkler similarity) and the weight it added, plus the blocking key
that produced the pair. The operator console draws its score breakdown from
it. No log line and no error shows a compared value.
