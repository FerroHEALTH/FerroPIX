-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- What the feed transaction needs beyond the record (docs/architecture.md
-- sections 4.3, 5.1 and 5.2). Each version keeps its two text blocking keys
-- as indexed columns, and every identifier it carries in a known domain as
-- a row of carried_identifier, found by its HMAC and held sealed in a
-- sensitive domain, like the record's own, so db reseal rewrites it too;
-- the demographics keep no identifier. A merge the system makes on an
-- authoritative identifier or an automatic link is kept with its kind.

ALTER TABLE pix.source_record_version
    ADD COLUMN block_phonetic text,
    ADD COLUMN block_postal text;

CREATE INDEX source_record_version_block_phonetic_idx
    ON pix.source_record_version (block_phonetic) WHERE block_phonetic IS NOT NULL;
CREATE INDEX source_record_version_block_postal_idx
    ON pix.source_record_version (block_postal) WHERE block_postal IS NOT NULL;

CREATE TABLE pix.carried_identifier (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    record_id uuid NOT NULL,
    version integer NOT NULL,
    domain_id uuid NOT NULL REFERENCES pix.identifier_domain (id),
    identifier_hmac bytea NOT NULL,
    identifier_sealed bytea,
    identifier_value text,
    key_id bytea NOT NULL,
    CONSTRAINT carried_identifier_version_fkey FOREIGN KEY (record_id, version)
        REFERENCES pix.source_record_version (record_id, version),
    CONSTRAINT carried_identifier_key UNIQUE (record_id, version, domain_id, identifier_hmac),
    CONSTRAINT carried_identifier_hmac_check CHECK (octet_length(identifier_hmac) = 32),
    CONSTRAINT carried_identifier_value_check
        CHECK ((identifier_sealed IS NULL) <> (identifier_value IS NULL)),
    CONSTRAINT carried_identifier_key_id_check CHECK (octet_length(key_id) = 16)
);

CREATE INDEX carried_identifier_lookup_idx
    ON pix.carried_identifier (domain_id, identifier_hmac);
CREATE INDEX carried_identifier_key_id_idx ON pix.carried_identifier (key_id);

CREATE TRIGGER carried_identifier_kept BEFORE DELETE ON pix.carried_identifier
    FOR EACH ROW EXECUTE FUNCTION pix.refuse_change();
CREATE TRIGGER carried_identifier_no_truncate BEFORE TRUNCATE ON pix.carried_identifier
    FOR EACH STATEMENT EXECUTE FUNCTION pix.refuse_change();

ALTER TABLE pix.merge DROP CONSTRAINT merge_kind_check;
ALTER TABLE pix.merge ADD CONSTRAINT merge_kind_check
    CHECK (kind IN ('steward', 'source', 'deterministic', 'probabilistic'));
