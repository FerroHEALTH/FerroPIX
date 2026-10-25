-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The outbound messages the server delivers with bounded retry: an XCPD
-- asynchronous answer, and the HL7 v2 notifications that follow. A message is
-- written before its request is acknowledged, so an acknowledged message
-- survives a restart. Its payload names a patient, so it is sealed under the
-- store's key with the row's id as the associated data, as an identifier is.

CREATE TABLE pix.outbox (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    channel text NOT NULL,
    destination text NOT NULL,
    media_type text NOT NULL,
    payload_sealed bytea NOT NULL,
    key_id bytea NOT NULL,
    attempts integer NOT NULL DEFAULT 0,
    max_attempts integer NOT NULL,
    next_at timestamptz NOT NULL DEFAULT now(),
    delivered_at timestamptz,
    abandoned_at timestamptz,
    last_failure text,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT outbox_channel_check CHECK (channel IN ('xcpd', 'hl7v2')),
    CONSTRAINT outbox_attempts_check CHECK (attempts >= 0 AND max_attempts > 0),
    CONSTRAINT outbox_settled_check CHECK (delivered_at IS NULL OR abandoned_at IS NULL)
);

CREATE INDEX outbox_due_idx ON pix.outbox (next_at)
    WHERE delivered_at IS NULL AND abandoned_at IS NULL;
