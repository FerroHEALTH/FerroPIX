-- SPDX-FileCopyrightText: Cadasto B.V.
-- SPDX-License-Identifier: BUSL-1.1

-- The demographics each person serves, and the values an ITI-78 search
-- matches them by, kept beside the cross-reference so a search reads an
-- index and never every person (#722).
--
-- pix.person_served is the row the answer shows a person by: among the
-- person's open links to active, unremoved records of unretired domains,
-- the head version, an authoritative record first, then the newest
-- received, then the record id. pix.person_search_key holds each value a
-- string or telecom parameter matches: the field, the value as written for
-- :exact and the telecom token, and the field with the value folded to its
-- base letters and lowercased for the default match, which an SP-GiST index
-- answers by prefix. pix.serve recomputes both for a set of persons and
-- writes only what changed; a statement trigger on each relation the choice
-- reads calls it at the end of every statement that changes one, in that
-- statement's transaction, so the copy is never out of step with what it
-- copies. The copy names persons and records by id with
-- no foreign key, since pix.serve alone writes it from the rows it reads
-- (a person and a record are never deleted). No specification governs the
-- copy: our own design.

-- A person names the record it is served by; the version is that record's
-- head, so a revision that changes neither the record, the gender nor the
-- birth date writes nothing here.
CREATE TABLE pix.person_served (
    person_id uuid PRIMARY KEY,
    record_id uuid NOT NULL,
    gender text,
    birth daterange
);

CREATE INDEX person_served_gender_idx ON pix.person_served (gender);
CREATE INDEX person_served_birth_idx ON pix.person_served USING gist (birth);
CREATE INDEX person_served_birth_lower_idx ON pix.person_served (lower(birth));
CREATE INDEX person_served_birth_upper_idx ON pix.person_served (upper(birth));

-- The field of a key: f family, g given, a any part of an address, c city,
-- n country, p postal code, s state, x an extension's valueString (its url
-- in system), t a telecom value (its ContactPoint.system in system).
CREATE TABLE pix.person_search_key (
    person_id uuid NOT NULL,
    field "char" NOT NULL,
    value text NOT NULL,
    system text,
    folded text NOT NULL,
    CONSTRAINT person_search_key_field_check
        CHECK (field IN ('f', 'g', 'a', 'c', 'n', 'p', 's', 'x', 't'))
);

CREATE UNIQUE INDEX person_search_key_idx
    ON pix.person_search_key (person_id, field, value, system) NULLS NOT DISTINCT;
CREATE INDEX person_search_key_value_idx ON pix.person_search_key (field, value);
CREATE INDEX person_search_key_folded_idx ON pix.person_search_key USING spgist (folded);

-- Every value of a demographics object a search matches, with its field: the
-- values pix.elements and pix.texts yield, read in place so the planner
-- inlines this function into the statement that calls it (no SET clause and
-- no function it cannot inline), and no call is made per person.
CREATE FUNCTION pix.search_keys(jsonb) RETURNS TABLE (field "char", value text, system text)
    LANGUAGE sql STABLE PARALLEL SAFE
AS $$
    SELECT k.field, k.value, k.system FROM (
        SELECT 'f'::"char", n->>'family', NULL::text FROM jsonb_array_elements(CASE WHEN jsonb_typeof($1->'name') = 'array' THEN $1->'name' ELSE '[]' END) n
        UNION ALL
        SELECT 'g', g, NULL FROM jsonb_array_elements(CASE WHEN jsonb_typeof($1->'name') = 'array' THEN $1->'name' ELSE '[]' END) n,
            jsonb_array_elements_text(
                CASE WHEN jsonb_typeof(n->'given') = 'array' THEN n->'given' ELSE '[]' END
            ) g
        UNION ALL
        SELECT 'a', part, NULL FROM jsonb_array_elements(CASE WHEN jsonb_typeof($1->'address') = 'array' THEN $1->'address' ELSE '[]' END) a,
            LATERAL (SELECT a->>'text' UNION ALL SELECT a->>'city'
                     UNION ALL SELECT a->>'district' UNION ALL SELECT a->>'state'
                     UNION ALL SELECT a->>'postalCode' UNION ALL SELECT a->>'country'
                     UNION ALL SELECT jsonb_array_elements_text(
                         CASE WHEN jsonb_typeof(a->'line') = 'array' THEN a->'line' ELSE '[]' END
                     )) AS lines(part)
        UNION ALL
        SELECT 'c', a->>'city', NULL FROM jsonb_array_elements(CASE WHEN jsonb_typeof($1->'address') = 'array' THEN $1->'address' ELSE '[]' END) a
        UNION ALL
        SELECT 'n', a->>'country', NULL FROM jsonb_array_elements(CASE WHEN jsonb_typeof($1->'address') = 'array' THEN $1->'address' ELSE '[]' END) a
        UNION ALL
        SELECT 'p', a->>'postalCode', NULL FROM jsonb_array_elements(CASE WHEN jsonb_typeof($1->'address') = 'array' THEN $1->'address' ELSE '[]' END) a
        UNION ALL
        SELECT 's', a->>'state', NULL FROM jsonb_array_elements(CASE WHEN jsonb_typeof($1->'address') = 'array' THEN $1->'address' ELSE '[]' END) a
        UNION ALL
        SELECT 'x', e->>'valueString', e->>'url' FROM jsonb_array_elements(CASE WHEN jsonb_typeof($1->'extension') = 'array' THEN $1->'extension' ELSE '[]' END) e
        UNION ALL
        SELECT 't', t->>'value', t->>'system' FROM jsonb_array_elements(CASE WHEN jsonb_typeof($1->'telecom') = 'array' THEN $1->'telecom' ELSE '[]' END) t
    ) AS k (field, value, system)
    WHERE k.value IS NOT NULL
$$;

-- The part of a demographics object the served row and the search keys are
-- made of: two objects with equal parts serve and search alike.
CREATE FUNCTION pix.searched(jsonb) RETURNS jsonb
    LANGUAGE sql IMMUTABLE PARALLEL SAFE
    RETURN jsonb_build_object(
        'name', $1->'name', 'address', $1->'address', 'telecom', $1->'telecom',
        'extension', $1->'extension', 'gender', $1->'gender', 'birthDate', $1->'birthDate'
    );

-- The key a folded value is searched by: the field and the folded value.
CREATE FUNCTION pix.folded_key("char", text) RETURNS text
    LANGUAGE sql STABLE PARALLEL SAFE STRICT
    RETURN $1::text || ':' || pix.fold($2);

-- Recomputes the served row and the search keys of each of persons, and
-- writes only what changed.
CREATE FUNCTION pix.serve(persons uuid[]) RETURNS void
    LANGUAGE sql
    SET search_path = pg_catalog
AS $$
    WITH chosen AS (
        SELECT DISTINCT ON (l.person_id) l.person_id, r.id AS record_id, v.demographics
        FROM unnest(persons) AS asked (id)
        JOIN pix.link l ON l.person_id = asked.id AND upper_inf(l.valid)
        JOIN pix.source_record r ON r.id = l.record_id
        JOIN pix.identifier_domain d ON d.id = r.domain_id
        -- The head version of each record, read through its key whatever the
        -- planner guesses of the array's size.
        CROSS JOIN LATERAL (
            SELECT h.demographics, h.received_at
            FROM pix.source_record_version h
            WHERE h.record_id = r.id AND h.version = r.head_version
        ) AS v
        WHERE r.active AND r.removed_at IS NULL AND d.retired_at IS NULL
        ORDER BY l.person_id, (d.kind = 'authoritative') DESC, v.received_at DESC, r.id
    ),
    gone AS (
        DELETE FROM pix.person_served s
        WHERE s.person_id = ANY(persons)
          AND NOT EXISTS (SELECT 1 FROM chosen c WHERE c.person_id = s.person_id)
    ),
    served AS (
        INSERT INTO pix.person_served AS s (person_id, record_id, gender, birth)
        SELECT c.person_id, c.record_id, c.demographics->>'gender',
               pix.birth_range(c.demographics->>'birthDate')
        FROM chosen c
        ON CONFLICT (person_id) DO UPDATE
            SET record_id = excluded.record_id, gender = excluded.gender, birth = excluded.birth
            WHERE (s.record_id, s.gender, s.birth)
                  IS DISTINCT FROM (excluded.record_id, excluded.gender, excluded.birth)
    ),
    keys AS (
        SELECT DISTINCT c.person_id, k.field, k.value, k.system
        FROM chosen c, pix.search_keys(c.demographics) k
    ),
    dropped AS (
        DELETE FROM pix.person_search_key o
        WHERE o.person_id = ANY(persons)
          AND NOT EXISTS (
              SELECT 1 FROM keys k
              WHERE k.person_id = o.person_id AND k.field = o.field AND k.value = o.value
                AND k.system IS NOT DISTINCT FROM o.system
          )
    )
    INSERT INTO pix.person_search_key (person_id, field, value, system, folded)
    SELECT k.person_id, k.field, k.value, k.system, pix.folded_key(k.field, k.value)
    FROM keys k
    ON CONFLICT DO NOTHING
$$;

-- The persons whose links a statement opened.
CREATE FUNCTION pix.serve_links_opened() RETURNS trigger
    LANGUAGE plpgsql
    SET search_path = pg_catalog
AS $$
BEGIN
    PERFORM pix.serve(ARRAY(SELECT DISTINCT person_id FROM opened));
    RETURN NULL;
END
$$;

-- The persons whose links a statement closed or moved, before and after.
CREATE FUNCTION pix.serve_links_changed() RETURNS trigger
    LANGUAGE plpgsql
    SET search_path = pg_catalog
AS $$
BEGIN
    PERFORM pix.serve(ARRAY(
        SELECT person_id FROM before_change UNION SELECT person_id FROM after_change
    ));
    RETURN NULL;
END
$$;

-- The persons of the open links of the records a statement gave another
-- head version, activity or removal, unless the record already served its
-- person, stays active and unremoved, and its new head carries the same
-- searched part as the old: a revision that changes nothing a search or the
-- served row reads recomputes nothing, in one statement.
CREATE FUNCTION pix.serve_records_changed() RETURNS trigger
    LANGUAGE plpgsql
    SET search_path = pg_catalog
AS $$
DECLARE
    persons uuid[];
BEGIN
    SELECT ARRAY(
        SELECT DISTINCT l.person_id
        FROM after_change n
        JOIN before_change o ON o.id = n.id
        JOIN pix.link l ON l.record_id = n.id AND upper_inf(l.valid)
        LEFT JOIN pix.person_served s ON s.person_id = l.person_id
        LEFT JOIN pix.source_record_version ov
            ON ov.record_id = o.id AND ov.version = o.head_version
        LEFT JOIN pix.source_record_version nv
            ON nv.record_id = n.id AND nv.version = n.head_version
        WHERE (o.head_version, o.active, o.removed_at)
              IS DISTINCT FROM (n.head_version, n.active, n.removed_at)
          AND NOT coalesce(
              s.record_id = n.id
              AND o.active = n.active AND o.removed_at IS NOT DISTINCT FROM n.removed_at
              AND pix.searched(ov.demographics) = pix.searched(nv.demographics),
              false
          )
    ) INTO persons;
    IF cardinality(persons) > 0 THEN
        PERFORM pix.serve(persons);
    END IF;
    RETURN NULL;
END
$$;

-- Every person one of whose records is open in a domain a statement
-- retired, restored or gave another kind.
CREATE FUNCTION pix.serve_domains_changed() RETURNS trigger
    LANGUAGE plpgsql
    SET search_path = pg_catalog
AS $$
BEGIN
    PERFORM pix.serve(ARRAY(
        SELECT DISTINCT l.person_id
        FROM after_change n
        JOIN before_change o ON o.id = n.id
        JOIN pix.source_record r ON r.domain_id = n.id
        JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
        WHERE (o.retired_at, o.kind) IS DISTINCT FROM (n.retired_at, n.kind)
    ));
    RETURN NULL;
END
$$;

-- Each runs once per statement, over every row the statement changed, so a
-- statement that writes many rows recomputes each person once, and the
-- statements after it in the transaction read the copy they left.
CREATE TRIGGER link_opened_served AFTER INSERT ON pix.link
    REFERENCING NEW TABLE AS opened
    FOR EACH STATEMENT EXECUTE FUNCTION pix.serve_links_opened();

CREATE TRIGGER link_changed_served AFTER UPDATE ON pix.link
    REFERENCING OLD TABLE AS before_change NEW TABLE AS after_change
    FOR EACH STATEMENT EXECUTE FUNCTION pix.serve_links_changed();

CREATE TRIGGER source_record_served AFTER UPDATE ON pix.source_record
    REFERENCING OLD TABLE AS before_change NEW TABLE AS after_change
    FOR EACH STATEMENT EXECUTE FUNCTION pix.serve_records_changed();

CREATE TRIGGER identifier_domain_served AFTER UPDATE ON pix.identifier_domain
    REFERENCING OLD TABLE AS before_change NEW TABLE AS after_change
    FOR EACH STATEMENT EXECUTE FUNCTION pix.serve_domains_changed();

-- Every person the store holds, a thousand at a time, so no statement reads
-- the whole store at once.
DO $$
DECLARE
    after uuid := '00000000-0000-0000-0000-000000000000';
    batch uuid[];
BEGIN
    LOOP
        SELECT array_agg(id ORDER BY id) INTO batch
        FROM (SELECT id FROM pix.person WHERE id > after ORDER BY id LIMIT 1000) AS page;
        EXIT WHEN batch IS NULL;
        PERFORM pix.serve(batch);
        after := batch[array_upper(batch, 1)];
    END LOOP;
END
$$;
