-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- A score's evidence moves from JSON into the server's versioned binary
-- layout (#752, src/store/evidence.rs). The JSON repeated every name in
-- every field entry and held each masked value in three-byte bullets, about
-- 1,800 bytes a candidate, 94% of the candidate row. The candidates' JSON
-- column keeps its rows under a new name until the server has moved them:
-- after the migrations it packs a thousand rows at a time and empties
-- evidence_json as it goes, and a server that does not migrate refuses to
-- start until pix.evidence_refill records the refill done, so no start
-- reads the table to learn it. No specification governs the stored form:
-- our own design.

ALTER TABLE pix.candidate RENAME COLUMN evidence TO evidence_json;
ALTER TABLE pix.candidate
    ALTER COLUMN evidence_json DROP NOT NULL,
    ADD COLUMN evidence bytea;
ALTER TABLE pix.candidate
    ADD CONSTRAINT candidate_evidence_kept
        CHECK (evidence IS NOT NULL OR evidence_json IS NOT NULL) NOT VALID;

-- A link's evidence changes type where it stands, its JSON kept as the
-- bytes of its text until the server packs it. pix.link is small beside
-- pix.candidate, so its rewrite is short, and it keeps its width, which
-- the plans of the lookups that read it depend on.
ALTER TABLE pix.link ALTER COLUMN evidence TYPE bytea USING convert_to(evidence::text, 'UTF8');

CREATE TABLE pix.evidence_refill (
    single boolean PRIMARY KEY DEFAULT true CHECK (single),
    done_at timestamptz
);
INSERT INTO pix.evidence_refill DEFAULT VALUES;
