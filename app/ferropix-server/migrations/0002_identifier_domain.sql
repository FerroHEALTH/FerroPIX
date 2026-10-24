-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- An identifier domain: an assigning authority, the system of a FHIR
-- identifier, fed by exactly one Patient Identity Source (PIXm 3.1.0
-- §2:3.104.4.1.3), named by the client id of the token that feeds it.
-- Retired, never deleted.

CREATE TABLE pix.identifier_domain (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    system text NOT NULL,
    name text NOT NULL,
    kind text NOT NULL,
    source_client_id text NOT NULL,
    sensitive boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now(),
    retired_at timestamptz,
    CONSTRAINT identifier_domain_system_key UNIQUE (system),
    CONSTRAINT identifier_domain_kind_check CHECK (kind IN ('local', 'authoritative', 'master')),
    CONSTRAINT identifier_domain_text_check CHECK (system <> '' AND name <> '' AND source_client_id <> '')
);

CREATE TRIGGER identifier_domain_kept BEFORE DELETE ON pix.identifier_domain
    FOR EACH ROW EXECUTE FUNCTION pix.refuse_change();
