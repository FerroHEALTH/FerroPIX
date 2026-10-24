-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- A candidate: a pair of records the scorer found worth a human look, held
-- against the versions it was computed on, with its score, its grade and
-- its evidence. A not-a-match pair is not queued again until either record
-- changes (docs/architecture.md section 5.3).

CREATE TABLE pix.candidate (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    record_a uuid NOT NULL,
    record_b uuid NOT NULL,
    a_version integer NOT NULL,
    b_version integer NOT NULL,
    score double precision NOT NULL,
    grade text NOT NULL,
    evidence jsonb NOT NULL,
    status text NOT NULL DEFAULT 'open',
    created_at timestamptz NOT NULL DEFAULT now(),
    decided_by text,
    decided_at timestamptz,
    reason text,
    deferred_until timestamptz,
    CONSTRAINT candidate_a_fkey FOREIGN KEY (record_a, a_version)
        REFERENCES pix.source_record_version (record_id, version),
    CONSTRAINT candidate_b_fkey FOREIGN KEY (record_b, b_version)
        REFERENCES pix.source_record_version (record_id, version),
    CONSTRAINT candidate_pair_check CHECK (record_a < record_b),
    CONSTRAINT candidate_grade_check CHECK (grade IN ('certain', 'probable', 'possible')),
    CONSTRAINT candidate_status_check CHECK (status IN ('open', 'linked', 'not-a-match', 'deferred')),
    CONSTRAINT candidate_decided_check
        CHECK ((status = 'open') = (decided_at IS NULL) AND (decided_at IS NULL) = (decided_by IS NULL)),
    CONSTRAINT candidate_deferred_check CHECK ((status = 'deferred') = (deferred_until IS NOT NULL))
);

CREATE UNIQUE INDEX candidate_one_open_per_pair ON pix.candidate (record_a, record_b)
    WHERE status IN ('open', 'deferred');
CREATE INDEX candidate_queue_idx ON pix.candidate (status, score DESC);
