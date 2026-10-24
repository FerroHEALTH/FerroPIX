-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- A source record: what one domain said about one patient, one row per
-- (domain, identifier). The identifier is found by its HMAC and held sealed
-- in a sensitive domain, plain in another; key_id names the key both were
-- written under. replaced_by is the Source's own duplicate resolution, a
-- record of the same domain (PIXm 3.1.0 §2:3.104.4.2.3). Each version is
-- written once and carries the demographics of the PDQm Patient profile the
-- Manager accepts (PIXm 3.1.0 §2:3.104.4.1.2.1).

CREATE TABLE pix.source_record (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    domain_id uuid NOT NULL REFERENCES pix.identifier_domain (id),
    identifier_hmac bytea NOT NULL,
    identifier_sealed bytea,
    identifier_value text,
    key_id bytea NOT NULL,
    head_version integer NOT NULL DEFAULT 1,
    active boolean NOT NULL DEFAULT true,
    replaced_by uuid,
    removed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT source_record_identifier_key UNIQUE (domain_id, identifier_hmac),
    CONSTRAINT source_record_domain_id_key UNIQUE (domain_id, id),
    CONSTRAINT source_record_replaced_by_fkey FOREIGN KEY (domain_id, replaced_by)
        REFERENCES pix.source_record (domain_id, id),
    CONSTRAINT source_record_hmac_check CHECK (octet_length(identifier_hmac) = 32),
    CONSTRAINT source_record_identifier_check
        CHECK ((identifier_sealed IS NULL) <> (identifier_value IS NULL)),
    CONSTRAINT source_record_key_id_check CHECK (octet_length(key_id) = 16),
    CONSTRAINT source_record_replaced_by_check CHECK (replaced_by IS NULL OR replaced_by <> id),
    CONSTRAINT source_record_head_version_check CHECK (head_version >= 1)
);

CREATE INDEX source_record_key_id_idx ON pix.source_record (key_id);

CREATE TABLE pix.source_record_version (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    record_id uuid NOT NULL REFERENCES pix.source_record (id),
    version integer NOT NULL,
    demographics jsonb NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now(),
    channel text NOT NULL,
    request_id text,
    CONSTRAINT source_record_version_key UNIQUE (record_id, version),
    CONSTRAINT source_record_version_version_check CHECK (version >= 1),
    CONSTRAINT source_record_version_demographics_check CHECK (jsonb_typeof(demographics) = 'object'),
    CONSTRAINT source_record_version_channel_check CHECK (channel IN ('fhir', 'v2', 'pmir'))
);

ALTER TABLE pix.source_record
    ADD CONSTRAINT source_record_head_version_fkey FOREIGN KEY (id, head_version)
        REFERENCES pix.source_record_version (record_id, version)
        DEFERRABLE INITIALLY DEFERRED;

CREATE TRIGGER source_record_kept BEFORE DELETE ON pix.source_record
    FOR EACH ROW EXECUTE FUNCTION pix.refuse_change();
CREATE TRIGGER source_record_version_write_once BEFORE UPDATE OR DELETE ON pix.source_record_version
    FOR EACH ROW EXECUTE FUNCTION pix.refuse_change();
CREATE TRIGGER source_record_version_no_truncate BEFORE TRUNCATE ON pix.source_record_version
    FOR EACH STATEMENT EXECUTE FUNCTION pix.refuse_change();
