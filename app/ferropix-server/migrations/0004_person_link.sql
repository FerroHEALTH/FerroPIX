-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- A person is the cross-reference, the Patient Master Identity of PMIR
-- 1.6.0 §1:49.1.1.1, active or merged into a survivor. A link joins a
-- source record to a person for a period; the temporal unique constraint
-- keeps a record in one person at any instant, so the history is the table
-- and a change closes a row and never deletes one.

CREATE TABLE pix.person (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    status text NOT NULL DEFAULT 'active',
    merged_into uuid REFERENCES pix.person (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT person_status_check CHECK (status IN ('active', 'merged')),
    CONSTRAINT person_merged_into_check
        CHECK ((status = 'merged') = (merged_into IS NOT NULL) AND merged_into IS DISTINCT FROM id)
);

CREATE TABLE pix.link (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    record_id uuid NOT NULL REFERENCES pix.source_record (id),
    person_id uuid NOT NULL REFERENCES pix.person (id),
    kind text NOT NULL,
    score double precision,
    valid tstzrange NOT NULL,
    made_by text NOT NULL,
    made_at timestamptz NOT NULL DEFAULT now(),
    reason text,
    ended_by_action uuid,
    CONSTRAINT link_one_person_at_a_time UNIQUE (record_id, valid WITHOUT OVERLAPS),
    CONSTRAINT link_kind_check
        CHECK (kind IN ('feed', 'deterministic', 'probabilistic', 'steward', 'source', 'merge')),
    CONSTRAINT link_valid_check CHECK (NOT isempty(valid) AND NOT lower_inf(valid)),
    CONSTRAINT link_ended_check CHECK (ended_by_action IS NULL OR NOT upper_inf(valid))
);

CREATE INDEX link_open_person_idx ON pix.link (person_id) WHERE upper_inf(valid);

CREATE TRIGGER person_kept BEFORE DELETE ON pix.person
    FOR EACH ROW EXECUTE FUNCTION pix.refuse_change();
CREATE TRIGGER link_kept BEFORE DELETE ON pix.link
    FOR EACH ROW EXECUTE FUNCTION pix.refuse_change();
