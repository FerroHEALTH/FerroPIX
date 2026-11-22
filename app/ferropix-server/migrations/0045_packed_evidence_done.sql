-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The server has packed every candidate's and link's evidence into the
-- layout of 0044 (#752), so the JSON column goes, and a link's evidence
-- opens with the layout's version byte, 1, which JSON (`{`) never does: a
-- link the server did not pack fails this file, so the set stops before
-- it. No specification governs the stored form: our own design.

ALTER TABLE pix.candidate DROP COLUMN evidence_json;

ALTER TABLE pix.link
    ADD CONSTRAINT link_evidence_packed CHECK (evidence IS NULL OR get_byte(evidence, 0) = 1);
