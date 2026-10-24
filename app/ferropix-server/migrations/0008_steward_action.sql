-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- A steward action: one row of the console's audit trail, written once.
-- A closed link names the action that ended it.

CREATE TABLE pix.steward_action (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    kind text NOT NULL,
    actor text NOT NULL,
    at timestamptz NOT NULL DEFAULT now(),
    person_id uuid REFERENCES pix.person (id),
    record_id uuid REFERENCES pix.source_record (id),
    candidate_id uuid REFERENCES pix.candidate (id),
    merge_id uuid REFERENCES pix.merge (id),
    reason text,
    request_id text
);

CREATE INDEX steward_action_at_idx ON pix.steward_action (at DESC);
CREATE INDEX steward_action_person_idx ON pix.steward_action (person_id, at DESC);

ALTER TABLE pix.link
    ADD CONSTRAINT link_ended_by_action_fkey FOREIGN KEY (ended_by_action)
        REFERENCES pix.steward_action (id);

CREATE TRIGGER steward_action_write_once BEFORE UPDATE OR DELETE ON pix.steward_action
    FOR EACH ROW EXECUTE FUNCTION pix.refuse_change();
CREATE TRIGGER steward_action_no_truncate BEFORE TRUNCATE ON pix.steward_action
    FOR EACH STATEMENT EXECUTE FUNCTION pix.refuse_change();
