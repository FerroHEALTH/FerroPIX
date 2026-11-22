-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- A score's evidence moves from JSON into the server's versioned binary
-- layout (#752, src/store/evidence.rs). The JSON repeated every name in
-- every field entry and held each masked value in three-byte bullets, about
-- 1,800 bytes a candidate, 94% of the candidate row. The layout lives in
-- the server, so the server packs the rows: this file makes room, the
-- server packs every row a thousand at a time once it is applied, and
-- 0045 drops the JSON and holds every row to the layout. No specification
-- governs the stored form: our own design.

ALTER TABLE pix.candidate RENAME COLUMN evidence TO evidence_json;
ALTER TABLE pix.candidate
    ALTER COLUMN evidence_json DROP NOT NULL,
    ADD COLUMN evidence bytea;

-- A link's evidence changes type where it stands, its JSON kept as the
-- bytes of its text until the server packs it. pix.link is small beside
-- pix.candidate, so its rewrite is short, and it keeps its width, which
-- the plans of the lookups that read it depend on.
ALTER TABLE pix.link ALTER COLUMN evidence TYPE bytea USING convert_to(evidence::text, 'UTF8');
