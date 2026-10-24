-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- A merge: one person merged into another, by a steward or by the Source's
-- own statement (PIXm 3.1.0 §2:3.104.4.2.3, PMIR 1.6.0 §2:3.93.4.1.2.4),
-- with every link it moved, so the revert replays it backwards.

CREATE TABLE pix.merge (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    survivor_id uuid NOT NULL REFERENCES pix.person (id),
    merged_id uuid NOT NULL REFERENCES pix.person (id),
    kind text NOT NULL,
    made_by text NOT NULL,
    made_at timestamptz NOT NULL DEFAULT now(),
    reason text,
    reverted_at timestamptz,
    reverted_by text,
    revert_reason text,
    CONSTRAINT merge_kind_check CHECK (kind IN ('steward', 'source')),
    CONSTRAINT merge_persons_check CHECK (survivor_id <> merged_id),
    CONSTRAINT merge_reverted_check CHECK ((reverted_at IS NULL) = (reverted_by IS NULL))
);

CREATE INDEX merge_survivor_idx ON pix.merge (survivor_id, made_at DESC);

CREATE TABLE pix.merge_effect (
    merge_id uuid NOT NULL REFERENCES pix.merge (id),
    record_id uuid NOT NULL REFERENCES pix.source_record (id),
    from_person uuid NOT NULL REFERENCES pix.person (id),
    link_closed uuid NOT NULL REFERENCES pix.link (id),
    link_opened uuid NOT NULL REFERENCES pix.link (id),
    PRIMARY KEY (merge_id, record_id)
);

CREATE TRIGGER merge_kept BEFORE DELETE ON pix.merge
    FOR EACH ROW EXECUTE FUNCTION pix.refuse_change();
CREATE TRIGGER merge_effect_write_once BEFORE UPDATE OR DELETE ON pix.merge_effect
    FOR EACH ROW EXECUTE FUNCTION pix.refuse_change();
