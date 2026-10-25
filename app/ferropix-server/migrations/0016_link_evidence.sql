-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The evidence an automatic probabilistic link was made on, kept on the link
-- beside its score, so a steward reads why the scorer joined the records.
-- Only a link of kind probabilistic carries it.

ALTER TABLE pix.link ADD COLUMN evidence jsonb;

ALTER TABLE pix.link
    ADD CONSTRAINT link_evidence_check CHECK (evidence IS NULL OR kind = 'probabilistic');
