-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The type of a domain's identifiers, every Patient.identifier.type FerroPIX
-- writes: the HL7 Identifier Type extended with the EHDS codes of the draft
-- EEHRxF Annexes, Table 26 EHDSPatient.identifier.type. A domain added
-- before has PI, or NI when it is authoritative.

ALTER TABLE pix.identifier_domain ADD COLUMN identifier_type text;

UPDATE pix.identifier_domain
SET identifier_type = CASE kind WHEN 'authoritative' THEN 'NI' ELSE 'PI' END;

ALTER TABLE pix.identifier_domain
    ALTER COLUMN identifier_type SET NOT NULL,
    ADD CONSTRAINT identifier_domain_identifier_type_check CHECK (identifier_type IN (
        'NI', 'SS', 'PI', 'MR', 'PPN', 'NH', 'JHN', 'HC', 'HI', 'HIN', 'REGPID', 'REGION'
    ));
