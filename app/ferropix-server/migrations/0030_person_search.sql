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
-- writes only what changed; a constraint trigger on each relation the choice
-- reads calls it before the transaction commits, so the copy is never out of
-- step with what it copies. The copy names persons and records by id with
-- no foreign key, since pix.serve alone writes it from the rows it reads
-- (a person and a record are never deleted). No specification governs the
-- copy: our own design.

CREATE TABLE pix.person_served (
    person_id uuid PRIMARY KEY,
    record_id uuid NOT NULL,
    version integer NOT NULL,
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

-- Every value of a demographics object a search matches, with its field.
CREATE FUNCTION pix.search_keys(jsonb) RETURNS TABLE (field "char", value text, system text)
    LANGUAGE sql STABLE PARALLEL SAFE
    SET search_path = pg_catalog
AS $$
    SELECT k.field, k.value, k.system FROM (
        SELECT 'f'::"char", n->>'family', NULL::text FROM pix.elements($1->'name') n
        UNION ALL
        SELECT 'g', g, NULL FROM pix.elements($1->'name') n, pix.texts(n->'given') g
        UNION ALL
        SELECT 'a', part, NULL FROM pix.elements($1->'address') a,
            LATERAL (SELECT a->>'text' UNION ALL SELECT a->>'city'
                     UNION ALL SELECT a->>'district' UNION ALL SELECT a->>'state'
                     UNION ALL SELECT a->>'postalCode' UNION ALL SELECT a->>'country'
                     UNION ALL SELECT pix.texts(a->'line')) AS parts(part)
        UNION ALL
        SELECT 'c', a->>'city', NULL FROM pix.elements($1->'address') a
        UNION ALL
        SELECT 'n', a->>'country', NULL FROM pix.elements($1->'address') a
        UNION ALL
        SELECT 'p', a->>'postalCode', NULL FROM pix.elements($1->'address') a
        UNION ALL
        SELECT 's', a->>'state', NULL FROM pix.elements($1->'address') a
        UNION ALL
        SELECT 'x', e->>'valueString', e->>'url' FROM pix.elements($1->'extension') e
        UNION ALL
        SELECT 't', t->>'value', t->>'system' FROM pix.elements($1->'telecom') t
    ) AS k (field, value, system)
    WHERE k.value IS NOT NULL
$$;

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
        SELECT DISTINCT ON (l.person_id) l.person_id, r.id AS record_id, v.version, v.demographics
        FROM pix.link l
        JOIN pix.source_record r ON r.id = l.record_id
        JOIN pix.identifier_domain d ON d.id = r.domain_id
        JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version
        WHERE l.person_id = ANY(persons) AND upper_inf(l.valid)
          AND r.active AND r.removed_at IS NULL AND d.retired_at IS NULL
        ORDER BY l.person_id, (d.kind = 'authoritative') DESC, v.received_at DESC, r.id
    ),
    gone AS (
        DELETE FROM pix.person_served s
        WHERE s.person_id = ANY(persons)
          AND NOT EXISTS (SELECT 1 FROM chosen c WHERE c.person_id = s.person_id)
    ),
    served AS (
        INSERT INTO pix.person_served AS s (person_id, record_id, version, gender, birth)
        SELECT c.person_id, c.record_id, c.version, c.demographics->>'gender',
               pix.birth_range(c.demographics->>'birthDate')
        FROM chosen c
        ON CONFLICT (person_id) DO UPDATE
            SET record_id = excluded.record_id, version = excluded.version,
                gender = excluded.gender, birth = excluded.birth
            WHERE (s.record_id, s.version) IS DISTINCT FROM (excluded.record_id, excluded.version)
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

-- A link opened, closed or moved: both persons it named.
CREATE FUNCTION pix.serve_link() RETURNS trigger
    LANGUAGE plpgsql
    SET search_path = pg_catalog
AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND OLD.person_id IS DISTINCT FROM NEW.person_id THEN
        PERFORM pix.serve(ARRAY[OLD.person_id, NEW.person_id]);
    ELSE
        PERFORM pix.serve(ARRAY[NEW.person_id]);
    END IF;
    RETURN NULL;
END
$$;

-- A record's head version, activity or removal: the person of its open link.
CREATE FUNCTION pix.serve_record() RETURNS trigger
    LANGUAGE plpgsql
    SET search_path = pg_catalog
AS $$
BEGIN
    PERFORM pix.serve(ARRAY(
        SELECT l.person_id FROM pix.link l
        WHERE l.record_id = NEW.id AND upper_inf(l.valid)
    ));
    RETURN NULL;
END
$$;

-- A domain retired, restored or of another kind: every person one of its
-- records is open in.
CREATE FUNCTION pix.serve_domain() RETURNS trigger
    LANGUAGE plpgsql
    SET search_path = pg_catalog
AS $$
BEGIN
    PERFORM pix.serve(ARRAY(
        SELECT DISTINCT l.person_id FROM pix.source_record r
        JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
        WHERE r.domain_id = NEW.id
    ));
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER link_served AFTER INSERT OR UPDATE ON pix.link
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION pix.serve_link();

CREATE CONSTRAINT TRIGGER source_record_served
    AFTER UPDATE OF head_version, active, removed_at ON pix.source_record
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION pix.serve_record();

CREATE CONSTRAINT TRIGGER identifier_domain_served
    AFTER UPDATE OF retired_at, kind ON pix.identifier_domain
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION pix.serve_domain();

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
