-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- A pseudonymisation domain names the one client its pseudonyms are minted
-- for, as an identifier domain names its Source: an ITI-83 query from that
-- client that names the domain mints the person's pseudonym when it has
-- none, and any other caller reads only the pseudonyms already minted.

ALTER TABLE pix.pseudonym_domain ADD COLUMN consumer_client_id text;
ALTER TABLE pix.pseudonym_domain ADD CONSTRAINT pseudonym_domain_consumer_check
    CHECK (consumer_client_id IS NULL OR consumer_client_id <> '');
