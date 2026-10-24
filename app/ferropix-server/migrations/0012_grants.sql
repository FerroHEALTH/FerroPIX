-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The grants, last in the set. When the migrations run as a role other than
-- the runtime role ferropix, ferropix gets the schema and the rows it
-- writes, and the write-once relations take no update or delete from it.
-- When ferropix runs them itself, it owns the schema and needs no grant.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'ferropix')
        AND current_user <> 'ferropix' THEN
        GRANT USAGE ON SCHEMA pix TO ferropix;
        GRANT SELECT, INSERT, UPDATE ON ALL TABLES IN SCHEMA pix TO ferropix;
        REVOKE UPDATE ON pix.source_record_version, pix.merge_effect, pix.pseudonym,
            pix.steward_action, pix._sqlx_migrations FROM ferropix;
        REVOKE INSERT ON pix._sqlx_migrations FROM ferropix;
        GRANT DELETE ON pix.feed_outbox, pix.v2_receipt TO ferropix;
    END IF;
END
$$;
