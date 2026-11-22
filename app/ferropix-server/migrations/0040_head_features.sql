-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- Each head keeps its scoring features beside its blocking keys (#747): the
-- normalised demographics the scorer compares, as compact JSON, with the
-- version of the normalisation they were written under. The statement that
-- moves head_version writes them with the keys, so a candidate is scored
-- from them and its version's demographics are neither read nor parsed.
-- The normalisation lives in the server, so the server fills the column:
-- after the migrations it rewrites every head whose features_version is
-- not its own, keys included, a thousand at a time, and a server that does
-- not migrate refuses to start while one is left. No specification governs
-- the matching: our own design.

ALTER TABLE pix.record_block
    ADD COLUMN features text,
    ADD COLUMN features_version smallint;
