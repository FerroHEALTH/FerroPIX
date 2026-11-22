-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The BALP records of the writes (#742). A write inserts its record here in
-- its own transaction, before COMMIT, so the change and its record commit
-- together or not at all. The forwarder sends the oldest records as one
-- batch Bundle (ITI TF-2 §3.20.4.4) and deletes each one the repository
-- took; a record it refused moves to audit_quarantine for an operator. A
-- record names a patient, so it is compressed and sealed under the store's
-- key, its key id beside it for db reseal. No specification governs the
-- storage: our own design.

CREATE TABLE pix.audit_outbox (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    record_sealed bytea NOT NULL,
    key_id bytea NOT NULL
);

CREATE TABLE pix.audit_quarantine (
    id bigint PRIMARY KEY,
    record_sealed bytea NOT NULL,
    key_id bytea NOT NULL,
    quarantined_at timestamptz NOT NULL DEFAULT now()
);
