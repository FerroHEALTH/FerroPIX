-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The outbox's retention: delivered and abandoned messages older than an age
-- are deleted in batches, and pending ones never are. The server's role has
-- no DELETE on the outbox; it calls this function, which runs as its owner,
-- so the only rows the server can remove are the ones the rule allows.

CREATE FUNCTION pix.prune_outbox(older_than interval, batch integer)
RETURNS integer
LANGUAGE sql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    WITH gone AS (
        DELETE FROM pix.outbox
        WHERE id IN (
            SELECT id FROM pix.outbox
            WHERE coalesce(delivered_at, abandoned_at) < now() - older_than
            ORDER BY id
            LIMIT batch
            FOR UPDATE SKIP LOCKED
        )
        RETURNING 1
    )
    SELECT count(*)::integer FROM gone;
$$;

REVOKE ALL ON FUNCTION pix.prune_outbox(interval, integer) FROM PUBLIC;
