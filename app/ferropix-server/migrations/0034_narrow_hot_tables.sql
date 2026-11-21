-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The rows every feed writes carry no key nothing reads (#730). A version is
-- named by (record_id, version), which its unique constraint holds and the
-- foreign keys name, so its uuid id column and primary key go. A carried
-- identifier keeps its id, which the operator API names an identifier to
-- reveal by and reads only among the person's own records, so its primary
-- key goes and the column stays. The rotation walks the records and the carried identifiers by their
-- keys with a cursor, so the key_id indexes go too. Of steward_action's
-- indexes, the one by domain has no reader, and the one by person serves the
-- person's history alone, which no feed's row (it names no person) is part
-- of. No specification governs the storage: our own design.

ALTER TABLE pix.source_record_version DROP CONSTRAINT source_record_version_pkey;
ALTER TABLE pix.source_record_version DROP COLUMN id;
ALTER TABLE pix.carried_identifier DROP CONSTRAINT carried_identifier_pkey;
DROP INDEX pix.source_record_key_id_idx;
DROP INDEX pix.carried_identifier_key_id_idx;
DROP INDEX pix.steward_action_domain_idx;
DROP INDEX pix.steward_action_person_idx;
CREATE INDEX steward_action_person_idx ON pix.steward_action (person_id, at DESC)
    WHERE person_id IS NOT NULL;
