-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The two text blocking keys of section 5.2 move from every version to the
-- record's head (#724). A blocking read asks for the current versions that
-- carry a key, and an index over every version ever written made it read
-- and drop each historic version that once carried it, so its cost grew
-- with history. The keys of the head version are written on the record by
-- the statement that moves head_version, so a head never names a version
-- whose keys differ; the version rows keep no copy. The heads are filled a
-- thousand records at a time, so no statement reads the whole store at
-- once. No specification governs the blocking: our own design.

ALTER TABLE pix.source_record
    ADD COLUMN block_phonetic text,
    ADD COLUMN block_postal text;

DO $$
DECLARE
    after uuid := '00000000-0000-0000-0000-000000000000';
    batch uuid[];
BEGIN
    LOOP
        SELECT array_agg(id ORDER BY id) INTO batch
        FROM (SELECT id FROM pix.source_record WHERE id > after ORDER BY id LIMIT 1000) AS page;
        EXIT WHEN batch IS NULL;
        UPDATE pix.source_record r
        SET block_phonetic = v.block_phonetic, block_postal = v.block_postal
        FROM pix.source_record_version v
        WHERE r.id = ANY(batch) AND v.record_id = r.id AND v.version = r.head_version
          AND (v.block_phonetic IS NOT NULL OR v.block_postal IS NOT NULL);
        after := batch[array_upper(batch, 1)];
    END LOOP;
END
$$;

CREATE INDEX source_record_block_phonetic_idx
    ON pix.source_record (block_phonetic) WHERE block_phonetic IS NOT NULL;
CREATE INDEX source_record_block_postal_idx
    ON pix.source_record (block_postal) WHERE block_postal IS NOT NULL;

ALTER TABLE pix.source_record_version
    DROP COLUMN block_phonetic,
    DROP COLUMN block_postal;
