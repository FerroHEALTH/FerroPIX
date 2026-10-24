-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The foundation of schema pix: btree_gist, which the temporal link
-- constraint needs for its uuid equality part, installed into pix so the
-- runtime role reads no other schema, and the trigger function that keeps a
-- write-once table write-once. The migrator creates the schema itself, since
-- its bookkeeping table lives in it. No specification governs the store: our
-- own design (docs/architecture.md section 4.2).

CREATE EXTENSION IF NOT EXISTS btree_gist WITH SCHEMA pix;

CREATE FUNCTION pix.refuse_change() RETURNS trigger
    LANGUAGE plpgsql
    SET search_path = pg_catalog
AS $$
BEGIN
    RAISE EXCEPTION 'pix.%: a % is refused, the relation is write-once', TG_TABLE_NAME, lower(TG_OP)
        USING ERRCODE = 'restrict_violation';
END;
$$;

REVOKE ALL ON SCHEMA pix FROM PUBLIC;
