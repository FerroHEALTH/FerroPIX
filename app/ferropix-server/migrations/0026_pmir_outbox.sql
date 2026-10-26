-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The ITI-93 messages to subscribers go through the one outbox every sender
-- shares: a message names the subscription it is for, so the sender delivers
-- a subscription's messages in order and stops sending to a subscription
-- that is no longer active. The ITI-93 outbox of its own, never written, is
-- dropped.

ALTER TABLE pix.outbox ADD COLUMN subscription_id uuid REFERENCES pix.subscription (id);

ALTER TABLE pix.outbox DROP CONSTRAINT outbox_channel_check;
ALTER TABLE pix.outbox ADD CONSTRAINT outbox_channel_check
    CHECK (channel IN ('xcpd', 'hl7v2', 'pmir'));
ALTER TABLE pix.outbox ADD CONSTRAINT outbox_subscription_check
    CHECK ((channel = 'pmir') = (subscription_id IS NOT NULL));

CREATE INDEX outbox_subscription_pending_idx ON pix.outbox (subscription_id, created_at, id)
    WHERE subscription_id IS NOT NULL AND delivered_at IS NULL AND abandoned_at IS NULL;

DROP TABLE pix.feed_outbox;
