-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The grants again, last in the set (0029_grants.sql says why), over the
-- audit outbox 0042 adds: the forwarder deletes each record it delivered or
-- moved to the quarantine.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'ferropix')
        AND current_user <> 'ferropix' THEN
        GRANT USAGE ON SCHEMA pix TO ferropix;
        GRANT SELECT, INSERT, UPDATE ON ALL TABLES IN SCHEMA pix TO ferropix;
        REVOKE UPDATE ON pix.source_record_version, pix.merge_effect, pix.pseudonym,
            pix.steward_action, pix._sqlx_migrations FROM ferropix;
        REVOKE INSERT ON pix._sqlx_migrations FROM ferropix;
        GRANT DELETE ON pix.v2_receipt, pix.person_served, pix.person_search_key,
            pix.audit_outbox TO ferropix;
        GRANT EXECUTE ON FUNCTION pix.prune_outbox(interval, integer) TO ferropix;
    END IF;
END
$$;
