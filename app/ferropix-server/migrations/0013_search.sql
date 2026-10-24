-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- What the ITI-78 search reads the stored demographics with (PDQm 3.2.0
-- §2:3.78.4.1.2.1, §2:3.78.4.1.2.2). unaccent, installed into pix as
-- btree_gist is, folds both sides of a string search to their base letters,
-- lowercased, the case- and accent-insensitive match FHIR R4 gives a string
-- parameter; pix.elements reads a member FHIR JSON writes as an array, and
-- pix.birth_range reads a FHIR date at its precision as the range of days it
-- covers (FHIR R4 search, date). No specification governs the functions:
-- our own design (docs/architecture.md section 6.2).

CREATE EXTENSION IF NOT EXISTS unaccent WITH SCHEMA pix;

CREATE FUNCTION pix.fold(text) RETURNS text
    LANGUAGE sql STABLE PARALLEL SAFE STRICT
    RETURN lower(pix.unaccent('pix.unaccent'::regdictionary, $1));

CREATE FUNCTION pix.elements(jsonb) RETURNS SETOF jsonb
    LANGUAGE sql IMMUTABLE PARALLEL SAFE
    SET search_path = pg_catalog
AS $$
    SELECT element
    FROM jsonb_array_elements(CASE WHEN jsonb_typeof($1) = 'array' THEN $1 ELSE '[]'::jsonb END)
        AS element
$$;

CREATE FUNCTION pix.texts(jsonb) RETURNS SETOF text
    LANGUAGE sql IMMUTABLE PARALLEL SAFE
    SET search_path = pg_catalog
AS $$
    SELECT element
    FROM jsonb_array_elements_text(CASE WHEN jsonb_typeof($1) = 'array' THEN $1 ELSE '[]'::jsonb END)
        AS element
$$;

CREATE FUNCTION pix.birth_range(text) RETURNS daterange
    LANGUAGE sql STABLE PARALLEL SAFE STRICT
    SET search_path = pg_catalog
AS $$
    SELECT CASE
        WHEN $1 ~ '^[0-9]{4}$' AND pg_input_is_valid($1 || '-01-01', 'date')
            THEN daterange(($1 || '-01-01')::date, (($1 || '-01-01')::date + interval '1 year')::date)
        WHEN $1 ~ '^[0-9]{4}-[0-9]{2}$' AND pg_input_is_valid($1 || '-01', 'date')
            THEN daterange(($1 || '-01')::date, (($1 || '-01')::date + interval '1 month')::date)
        WHEN $1 ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$' AND pg_input_is_valid($1, 'date')
            THEN daterange($1::date, $1::date + 1)
    END
$$;
