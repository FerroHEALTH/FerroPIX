-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- A PMIR subscription (ITI-94) with its delivery state, and the outbox of
-- ITI-93 messages to subscribers, written inside the transaction that made
-- the change so a committed change is never unannounced.

CREATE TABLE pix.subscription (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    resource jsonb NOT NULL,
    status text NOT NULL,
    criteria text NOT NULL,
    endpoint text NOT NULL,
    error text,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT subscription_status_check CHECK (status IN ('requested', 'active', 'error', 'off'))
);

CREATE TABLE pix.feed_outbox (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    subscription_id uuid NOT NULL REFERENCES pix.subscription (id),
    bundle jsonb NOT NULL,
    attempts integer NOT NULL DEFAULT 0,
    next_at timestamptz NOT NULL DEFAULT now(),
    delivered_at timestamptz,
    CONSTRAINT feed_outbox_attempts_check CHECK (attempts >= 0)
);

CREATE INDEX feed_outbox_due_idx ON pix.feed_outbox (next_at) WHERE delivered_at IS NULL;
