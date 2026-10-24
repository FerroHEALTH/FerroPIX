-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The MSH-10 of each HL7 v2 message received, with its sender, so a resent
-- message is acknowledged again and not applied twice (docs/architecture.md
-- section 7).

CREATE TABLE pix.v2_receipt (
    control_id text NOT NULL,
    sending_facility text NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now(),
    ack_code text NOT NULL,
    PRIMARY KEY (sending_facility, control_id),
    CONSTRAINT v2_receipt_ack_code_check CHECK (ack_code IN ('AA', 'AE', 'AR', 'CA', 'CE', 'CR'))
);
