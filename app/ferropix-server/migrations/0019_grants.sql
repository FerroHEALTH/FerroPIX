-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The grants again, last in the set. A released migration is never edited
-- or renumbered, since a store that applied it would refuse the set, so a
-- migration added after a release is followed by this file, which grants
-- what 0015_grants granted over every relation the set now holds.

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
