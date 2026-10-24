-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- A pseudonymisation domain names a consumer of pseudonyms, and a pseudonym
-- is the UUID a person carries in it: immutable, attached to the person it
-- was minted for, resolved through the merge chain (docs/architecture.md
-- section 8).

CREATE TABLE pix.pseudonym_domain (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    namespace text NOT NULL,
    name text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    retired_at timestamptz,
    CONSTRAINT pseudonym_domain_namespace_key UNIQUE (namespace)
);

CREATE TABLE pix.pseudonym (
    person_id uuid NOT NULL REFERENCES pix.person (id),
    domain_id uuid NOT NULL REFERENCES pix.pseudonym_domain (id),
    value uuid NOT NULL,
    minted_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (person_id, domain_id),
    CONSTRAINT pseudonym_value_key UNIQUE (value)
);

CREATE TRIGGER pseudonym_domain_kept BEFORE DELETE ON pix.pseudonym_domain
    FOR EACH ROW EXECUTE FUNCTION pix.refuse_change();
CREATE TRIGGER pseudonym_immutable BEFORE UPDATE OR DELETE ON pix.pseudonym
    FOR EACH ROW EXECUTE FUNCTION pix.refuse_change();
CREATE TRIGGER pseudonym_no_truncate BEFORE TRUNCATE ON pix.pseudonym
    FOR EACH STATEMENT EXECUTE FUNCTION pix.refuse_change();
