-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The id of the key the identifier columns are written under, so a server
-- started with another key refuses to serve instead of finding nothing, and
-- the key a running db reseal moves them to.

CREATE TABLE pix.seal_key (
    singleton boolean PRIMARY KEY DEFAULT true,
    key_id bytea NOT NULL,
    pending_key_id bytea,
    set_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT seal_key_singleton_check CHECK (singleton),
    CONSTRAINT seal_key_key_id_check
        CHECK (octet_length(key_id) = 16 AND (pending_key_id IS NULL OR octet_length(pending_key_id) = 16))
);
