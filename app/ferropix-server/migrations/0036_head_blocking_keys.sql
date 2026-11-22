-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The two text blocking keys of section 5.2 move from every version to the
-- record's head (#724). A blocking read asks for the current versions that
-- carry a key, and an index over every version ever written made it read
-- and drop each historic version that once carried it, so its cost grew
-- with history. pix.record_block holds one row per record with the keys of
-- its head version, written by the statement that moves head_version (the
-- insert of a new record, the update of a revision), so a head never names
-- a version whose keys differ; the version rows keep no copy. The keys sit
-- beside the record rather than on it, so the record row every lookup reads
-- stays as narrow as it was. The row names its record by id with no foreign
-- key, since only the statement that writes the record writes it (a record
-- is never deleted). The heads are filled a thousand records at a time, so
-- no statement reads the whole store at once. No specification governs the
-- blocking: our own design.

CREATE TABLE pix.record_block (
    record_id uuid PRIMARY KEY,
    block_phonetic text,
    block_postal text
);

DO $$
DECLARE
    after uuid := '00000000-0000-0000-0000-000000000000';
    batch uuid[];
BEGIN
    LOOP
        SELECT array_agg(id ORDER BY id) INTO batch
        FROM (SELECT id FROM pix.source_record WHERE id > after ORDER BY id LIMIT 1000) AS page;
        EXIT WHEN batch IS NULL;
        INSERT INTO pix.record_block (record_id, block_phonetic, block_postal)
        SELECT r.id, v.block_phonetic, v.block_postal
        FROM pix.source_record r
        JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version
        WHERE r.id = ANY(batch);
        after := batch[array_upper(batch, 1)];
    END LOOP;
END
$$;

CREATE INDEX record_block_phonetic_idx
    ON pix.record_block (block_phonetic) WHERE block_phonetic IS NOT NULL;
CREATE INDEX record_block_postal_idx
    ON pix.record_block (block_postal) WHERE block_postal IS NOT NULL;

CREATE TRIGGER record_block_kept BEFORE DELETE ON pix.record_block
    FOR EACH ROW EXECUTE FUNCTION pix.refuse_change();
CREATE TRIGGER record_block_no_truncate BEFORE TRUNCATE ON pix.record_block
    FOR EACH STATEMENT EXECUTE FUNCTION pix.refuse_change();

ALTER TABLE pix.source_record_version
    DROP COLUMN block_phonetic,
    DROP COLUMN block_postal;
