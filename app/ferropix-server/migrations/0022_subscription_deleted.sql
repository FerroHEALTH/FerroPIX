-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- A subscription deleted through ITI-94 (PMIR 1.6.0 §2:3.94.4.5) keeps its
-- row, so the trail of what it was sent stays, and is marked deleted: it is
-- read as gone and sent nothing.

ALTER TABLE pix.subscription ADD COLUMN deleted_at timestamptz;
