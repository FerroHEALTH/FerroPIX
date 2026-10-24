-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- What a steward action names beyond the person, the record, the candidate
-- and the merge: the identifier domain an administrator wrote or whose
-- identifier a steward revealed, and the pseudonymisation domain whose
-- pseudonym a steward revealed (docs/architecture.md section 12.4). The
-- action names the domain, never the value.

ALTER TABLE pix.steward_action
    ADD COLUMN domain_id uuid REFERENCES pix.identifier_domain (id),
    ADD COLUMN pseudonym_domain_id uuid REFERENCES pix.pseudonym_domain (id);

CREATE INDEX steward_action_domain_idx ON pix.steward_action (domain_id, at DESC)
    WHERE domain_id IS NOT NULL;
