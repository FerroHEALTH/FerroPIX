-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The ITI-10 notifications to the HL7 v2 consumers go through the one outbox
-- every sender shares: a notification names the consumer it is for, so the
-- sender delivers a consumer's notifications in order, one at a time.

ALTER TABLE pix.outbox ADD COLUMN consumer text;

ALTER TABLE pix.outbox ADD CONSTRAINT outbox_consumer_check
    CHECK ((channel = 'hl7v2') = (consumer IS NOT NULL));

CREATE INDEX outbox_consumer_pending_idx ON pix.outbox (consumer, created_at, id)
    WHERE consumer IS NOT NULL AND delivered_at IS NULL AND abandoned_at IS NULL;
