-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The open link of a record, found by record_id under upper_inf(valid) by
-- every PIX query, PDQm read and feed, through a partial B-tree index; the
-- GiST index behind link_one_person_at_a_time keeps the temporal constraint.
-- A link is written only when a record is linked, moved or closed, so the
-- entry is rare beside the reads it serves.

CREATE INDEX link_open_record_idx ON pix.link (record_id) WHERE upper_inf(valid);
