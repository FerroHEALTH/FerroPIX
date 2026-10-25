<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Architecture

FerroPIX is the FerroHEALTH family's Master Patient Index: the Patient
Identifier Cross-reference Manager of PIXm, the Patient Demographics
Supplier of PDQm and the Patient Identity Registry of PMIR, with the HL7 v2
PIX and PDQ transactions on the same store, and the minter of the
pseudonymous identifiers the clinical domain names a patient by. It runs as
one server over PostgreSQL, with an operator console beside it for the data
stewards who keep the cross-reference true.

This document is the design of record. It is the output of the design pass
of 2026-10-21 over the vendored specifications (`docs/specs/`), the three
sibling consoles (FerroFED, FerroEHR, FerroTERM) and the FerroUI component
library, under the owner's decisions of the same day:

- FerroPIX is a standalone server with its cross-reference in PostgreSQL.
- It ships two OCI images, the server and an operator console; the console is
  the fifth FerroUI console, beside FerroBRIDGE, FerroEHR, FerroFED and
  FerroTERM, and must be the best of the family.
- Both images are released at SLSA Build Level 3 through reusable release
  workflows, as FerroFED's are.
- Where FerroUI lacks a component or a pattern, the gap is filed and built in
  FerroUI, never drawn by hand in FerroPIX.
- An identifier lookup uses `POST`, so a BSN never sits in a URL (#52).

Every decision names its ground: a section of a vendored IHE text (the
implementation guide pages are cited by volume and section, `PIXm 3.1.0
§2:3.83.4.1.2`), a section of HL7 FHIR R4, an RFC, or a W3C recommendation.
Where no specification governs a decision (the store, the matching, the
process model, the configuration, the console), the text says so: "no
specification governs this: our own design". The ITI TF-2 texts of the HL7
v2 transactions are not vendored yet; section 7 names them by transaction
and the issue that vendors them, and cites no section of them until then.

Section 15 is the decision register. Section 14 is the build order, which
the tracker's milestones carry.

## 1. The problem and the specification pins

A patient is known to every system by that system's own identifier: a
hospital number, a laboratory number, a national number. A system that
holds one of them and needs another, or that knows only a name and a birth
date, asks the Master Patient Index. The index holds the identifiers of each
identifier domain, the demographics the matching needs, and the links that
say which identifiers name one person. It holds no clinical data.

Three IHE profiles define the FHIR face of that index, and the ITI Technical
Framework defines the HL7 v2 face:

| Profile | Actor FerroPIX is | Transactions | What they do |
|---|---|---|---|
| PIXm 3.1.0 | Patient Identifier Cross-reference Manager | ITI-83, ITI-104 | the cross-reference query, and the feed that adds, revises, resolves a duplicate and removes a patient identity (PIXm 3.1.0 §1:41.1.1.3, §1:41.4.1) |
| PDQm 3.2.0 | Patient Demographics Supplier, with the Match Operation Option | ITI-78, ITI-119 | the demographics search and the graded match (PDQm 3.2.0 §1:38.1.1.2, §1:38.2.2) |
| PMIR 1.6.0 | Patient Identity Registry | ITI-93, ITI-94 | the master identity feed and the subscription to it (PMIR 1.6.0 §1:49.1.1.1) |
| ITI TF, PIX and PDQ | Patient Identifier Cross-reference Manager, Patient Demographics Supplier | ITI-8, ITI-9, ITI-10, ITI-21, ITI-22 | the HL7 v2 feed, query, update notification and demographics query (section 7) |

PIXm leaves the cross-referencing itself open: "the cross-referencing
process (algorithm, human decisions, etc.) is performed within the Patient
Identifier Cross-reference Manager, but its specification is beyond the
scope of IHE" (PIXm 3.1.0 §2:3.104.4.1.3), and "it may also provide other
triggers (e.g., manual linking or unlinking in case when the rules and
algorithms go wrong)" (PIXm 3.1.0 §1:41.4.1). Sections 4 and 5 and the
console are that open part, and they are FerroPIX's own design.

**Pinned versions.** The pins live in `docs/VERSIONS.md`, held by
`scripts/checks/versions.sh`. This table records the ground of each.

| Component | Pin | Ground |
|---|---|---|
| IHE PIXm | 3.1.0 (`ihe.iti.pixm`) | the profile the crate `ihe-iti` implements and the server answers; its `CapabilityStatement-IHE.PIXm.Manager` is the server's declared shape |
| IHE PDQm | 3.2.0 (`ihe.iti.pdqm`) | the search and the `$match` the server answers; `CapabilityStatement-IHE.PDQm.PatientDemographicsSupplier` and `…SupplierMatch` |
| IHE PMIR | 1.6.0 (`ihe.iti.pmir`) | the master identity feed and subscription; `CapabilityStatement-IHE.PMIR.PatientIdentityRegistry` |
| IHE BALP | 1.1.4 (`ihe.iti.balp`) | the audit record of every transaction and of every steward action (section 9) |
| IHE ITI TF-2, the PIX and PDQ transactions | ITI-8, ITI-9 and ITI-10 at Revision 20.0, ITI-21 at Revision 20.2, ITI-22 at Revision 20.1, each page by sha256 (`docs/VERSIONS.md`) | the HL7 v2 face (section 7); vendored cache only, since each page reproduces HL7 tables |
| HL7 FHIR | R4 (4.0.1) | the resources, search and REST interactions the profiles constrain |
| HL7 v2 | the version each transaction names in ITI TF-2 | the segments and fields of the v2 messages |
| `hl7v2-types`, `ferrobridge-hl7v2` | FerroBRIDGE's published line, pinned exactly | the v2 tables, the MLLP codec, the parser and the acknowledgment; FerroPIX writes no second parser (`CLAUDE.md` §Scope) |
| `ihe-atna` | the FerroAUDIT line, pinned exactly | the BALP `AuditEvent` builder and the audit spool |
| `ferrohealth-smart` | the FerroAUTH line, pinned exactly | the resource-server side of SMART App Launch and OAuth 2.0: token checks, issuer metadata, Basic users |
| `ferrohealth-ui` | the FerroUI line, pinned exactly | every component the console draws (`.claude/memory/ferroui-lockstep.md`) |
| PostgreSQL | 18 | the one store (section 4); `uuidv7()` for every database-minted id, `UNIQUE … WITHOUT OVERLAPS` for the temporal link table |
| Netherlands Generic Functions | `fhir.nl.gf` 0.3.0 | the BSN, the pseudonymised BSN and the NVI (section 8) |

## 2. The shape

The family's shape, which FerroFED fixed as its decision A55 and FerroBRIDGE
and FerroTERM followed: a server that answers an API and holds no browser
session, and a console that is its own binary and image, a Leptos
application rendered on the server and hydrated in the browser, with a
backend-for-frontend that holds the operator's session and calls the server
with the operator's own token. No specification governs the process model:
our own design, after the siblings.

```text
   PIXm Source / PMIR Source          PIXm Consumer / PDQm Consumer
   (ITI-104 feed, ITI-93 feed)        (ITI-83, ITI-78, ITI-119)
             │                                     │
             ▼                                     ▼
   ┌──────────────────────────────────────────────────────────┐
   │ ferropix (the server)                                     │
   │   /fhir       the PIXm Manager, PDQm Supplier, PMIR       │
   │               Registry                                    │
   │   :2575       the MLLP listener (ITI-8, ITI-9, ITI-21)    │
   │   /operator   the steward's API (the console's only road) │
   │   /health     liveness and readiness                      │
   └───────────────┬──────────────────────────┬───────────────┘
                   │                          │
                   ▼                          ▼
          PostgreSQL 18 (schema pix)     ATNA repository (BALP)
                   ▲
                   │  /operator, with the steward's token
   ┌───────────────┴──────────────────────────────────────────┐
   │ ferropix-console (the console): server-rendered Leptos     │
   │   OIDC sign-in, session on the console's server           │
   └──────────────────────────────────────────────────────────┘
```

- **`app/ferropix-server`**, binary `ferropix`, image
  `ghcr.io/ferrohealth/ferropix`. One process: the FHIR face under
  `{base}/fhir`, the MLLP listener on its own port, the operator API under
  `{base}/operator`, the health family, the audit sender and the PMIR feed
  sender.
- **`app/ferropix-console`**, binary `ferropix-console`, image
  `ghcr.io/ferrohealth/ferropix-console`. A pure HTTP client of the operator
  API. It links `ferropix-operator-api` (the serde types of every operator
  document, so a field renamed on one side fails the other's build, the
  shape FerroBRIDGE #777 fixed) and `ferrohealth-ui`, and no other workspace
  crate; a dependency-closure test holds it to that.
- **`crates/ihe-iti`** gains the server sides as features (`pixm-server`,
  `pdqm-server`, `pmir-server`): the typed request readers and response
  writers of each transaction, the operation and search parameter decoding,
  and the `OperationOutcome` of each refusal, with no store and no HTTP
  framework, so FerroFED can take them too. The store, the matching and the
  router stay in `app/`.

## 3. The server process and its configuration

No specification governs the process model or the configuration: our own
design, in the family's shape (FerroFED's `app/ferrofed-server`,
FerroBRIDGE's `app/ferrobridge-server`).

**Subcommands.** `serve` runs the server; `config check` loads and validates
the configuration and exits; `healthcheck` asks `GET {base}/health/readiness`
of the process beside it with a short timeout and exits 0 or 1, the
container `HEALTHCHECK`; `db migrate` applies the pending migrations,
checks the sealing key and applies the `[[domain]]` seed, and exits (`serve`
does the same at start, the migrations unless `[db] migrate = false`); `db
reseal --old-key-file <path>` rotates the sealing key (section 4.2).

**Configuration.** One TOML file named by `FERROPIX_CONFIG` or `--config`,
overridden by environment variables `FERROPIX__<SECTION>__<KEY>` (two
underscores between levels), every table `deny_unknown_fields`, so a
misspelled key refuses the start. Every secret is a `Secret` type that
renders as `***` through `Debug`, `Display` and `Serialize`, read from the
value or from a `<key>_file` sibling (the file's text trimmed, then zeroed),
so no consumer ever holds a bare string. A URL that may carry a password
(`[db] url`) is a `SecretUrl`, rendered with its userinfo and query replaced.
`profile = "production"` is the default and refuses every development
setting: the `[dev]` table, Basic users, a secret set inline instead of
through its `_file` sibling, a plain `http` public base URL off loopback and
a plain `http` audit repository. `profile = "development"` admits them for a
loopback run with Basic users and no TLS, and `[dev] loopback` (on by
default) refuses a listener on any address but a loopback one. An error
names the key and never the value, and a parse error keeps the parser's
message and position without quoting the line.

| Section | Holds |
|---|---|
| `[server]` | `listen`, `base_path`, `public_base_url` (the `[base]` the `CapabilityStatement` and every `targetId` name), `body_limit_bytes`, `request_timeout_ms`, `drain_timeout_ms` (the drain window on `SIGTERM`) |
| `[telemetry]` | the `tracing` filter and format, the OTLP endpoint |
| `[db]` | `url` or `url_file`, the pool size, `migrate`, `data_key_file` (the sealing key of section 4) |
| `[auth]` | the `ferrohealth-smart` resource-server settings: `[[auth.issuer]]` entries with their RFC 8414 metadata, each listing the `clients` it may vouch for as a Source, the audience, `[[auth.basic.users]]` under the development profile only, and the two product scopes, `steward_scope` and `admin_scope` |
| `[[domain]]` | the identifier domains to seed at start, each `system`, `name`, `kind` and `source_client_id`; applied as insert-if-absent, never an update, so the database stays the record (section 4) |
| `[matching]` | the thresholds, the field weights, the blocking keys, `auto_link` |
| `[pixm]`, `[pdqm]`, `[pmir]` | each profile's switch and options: `[pixm] remove_patient` (PIXm 3.1.0 §1:41.2.1), `[pdqm] deprecated` (which of the policy choices of PDQm 3.2.0 §2:3.78.4.1.3 case 6 the supplier takes), `[pmir] feed` (the outbound ITI-93 sender's retry bounds) |
| `[hl7v2]` | `listen`, `default_charset`, `frame_limit_bytes`, the timeouts, `[[hl7v2.senders]]` (section 7) |
| `[audit]` | the ATNA repository the BALP records go to (`url`, a FHIR base for the FHIR Feed, BALP 1.1.4 §1:52.1.1.1), the spool directory and its bounds |
| `[operator]` | the operator API switch (`enabled`, off by default; on requires `[auth]`) |
| `[usage_report]` | the FerroPULSE report (section 9) |

A `[[domain]]` entry whose `source_client_id` no `[[auth.issuer]]` lists in
its `clients` (or, under the development profile, no Basic user is named) is
refused at load, as is a seed that names one system twice.

## 4. The cross-reference store

No specification governs the store: our own design. PIXm fixes what the
store must answer (PIXm 3.1.0 §2:3.83.4.1.3: the identifiers that match a
source identifier, by target domain) and what the feed does to it (§2:3.104.4.1.3,
§2:3.104.4.2.3, §2:3.104.4.3.3), and the design follows FerroEHR's storage
discipline: write-once history rows, one mutable head row per object, and a
temporal link table under PostgreSQL 18's `UNIQUE … WITHOUT OVERLAPS`, so a
change closes a row and never deletes one.

### 4.1 The model

Four nouns, and the history of each:

- **An identifier domain** is an assigning authority: the `system` of a
  FHIR identifier (`urn:oid:…` or a URL), the PID-3.4 assigning authority of
  a v2 message. Exactly one Patient Identity Source feeds a domain (PIXm
  3.1.0 §2:3.104.4.1.3: "shall only recognize a single Patient Identity
  Source per domain"), named by the client id of the token that feeds it.
  A domain has a `kind`: `local` (a system's own numbers), `authoritative`
  (a national number such as the BSN, which the deterministic match may
  link on), or `master` (FerroPIX's own, the PMIR Master Patient
  Identification Domain, whose identifiers are the person ids below).
- **A source record** is what one domain said about one patient: its
  identifier in that domain and the demographics the feed carried (the
  elements of the PDQm Patient profile, which the Manager must accept, PIXm
  3.1.0 §2:3.104.4.1.2.1). One record per `(domain, identifier)`. A feed
  writes a new version; the record's head points at the current one.
- **A person** is the cross-reference: the set of source records FerroPIX
  holds to be one patient. It is the Patient Master Identity of PMIR (PMIR
  1.6.0 §1:49.1.1.1) and the `Patient` resource the FHIR face serves
  (section 6). A person is `active`, or `merged` into a survivor.
- **A link** joins a source record to a person for a period. Its `kind`
  says why: `feed` (the record's first person, made when the record arrived
  alone), `deterministic` (an authoritative identifier agreed),
  `probabilistic` (the scorer crossed the auto-link threshold),
  `steward` (a reviewer decided), `source` (the Source resolved a duplicate
  or merged, ITI-104 or ITI-93), `merge` (moved by a person merge). A
  closed link keeps its `ended_at` and the action that ended it.

Beside them: **a candidate** is a pair the scorer found worth a human look,
with its score and evidence (section 5); **a merge** records one person
merged into another with every effect, so it can be reverted; **a
pseudonymisation domain** names a consumer of pseudonyms (FerroEHR's
`subject_namespaces`), and **a pseudonym** is the UUID a person carries in
it (section 8); **a steward action** is one row of the console's audit
trail; **a subscription** is a PMIR ITI-94 subscription with its delivery
state.

### 4.2 The schema

One schema, `pix`, one migration set under `app/ferropix-server/migrations/`
(`sqlx migrate`, one concern per file, the grants in the last file of the
set, which a unit test holds; a released migration is never edited or
renumbered, so a migration added after a release is followed by the grants
again), embedded in the binary and applied in order by `db migrate` or at the
start of `serve`, under the migrator's advisory lock, with its bookkeeping in
`pix._sqlx_migrations`; the migrator creates the schema, and `btree_gist`
is installed into `pix`, so the runtime role needs no other schema. Every
database-minted id is `uuidv7()`, so ids sort by time and the history of a
row reads in order without a sequence.

| Relation | Columns (the load-bearing ones) | Notes |
|---|---|---|
| `identifier_domain` | `id`, `system` (unique), `name`, `kind`, `source_client_id`, `sensitive`, `created_at`, `retired_at` | retired, never deleted; a retired domain answers `targetSystem not found` (section 6) |
| `source_record` | `id`, `domain_id`, `identifier_hmac` (unique with `domain_id`), `identifier_sealed` or `identifier_value` (exactly one), `key_id`, `head_version`, `active`, `replaced_by` (a `source_record.id` of the same domain), `removed_at` | the one mutable row per record; `replaced_by` is the Source's own duplicate resolution (PIXm 3.1.0 §2:3.104.4.2.3) |
| `source_record_version` | `id`, `record_id`, `version`, `demographics` (jsonb), `received_at`, `channel` (`fhir`, `v2`, `pmir`), `request_id`, `block_phonetic`, `block_postal` | write-once; every feed inserts one; the demographics hold no identifier (the fed `Patient` without `identifier`, `link`, `contained`, `text`, `id` and `meta`), and the two text blocking keys of section 5.2 are indexed columns |
| `carried_identifier` | `id`, `record_id`, `version`, `domain_id`, `identifier_hmac`, `identifier_sealed` or `identifier_value`, `key_id` | every identifier a version carries in a domain the store knows, the record's own included, found and sealed as a record's own; kept, never deleted, rewritten only by `db reseal` |
| `person` | `id`, `status` (`active`, `merged`), `merged_into`, `created_at` | the Patient Master Identity; `id` is the FHIR `Patient.id` |
| `link` | `id`, `record_id`, `person_id`, `kind`, `score`, `evidence` (jsonb, a `probabilistic` link's alone), `valid` (`tstzrange`), `made_by` (an actor), `made_at`, `reason`, `ended_by_action` | `UNIQUE (record_id, valid WITHOUT OVERLAPS)` over `btree_gist`, so a record is in one person at any instant and the history is the table |
| `candidate` | `id`, `record_a`, `record_b`, `a_version`, `b_version`, `score`, `grade`, `evidence` (jsonb), `status` (`open`, `linked`, `not-a-match`, `deferred`), `decided_by`, `decided_at`, `reason`, `deferred_until` | the review queue; a `not-a-match` pair is not queued again until either record changes |
| `merge` | `id`, `survivor_id`, `merged_id`, `kind` (`steward`, `source`, `deterministic`, `probabilistic`), `made_by`, `made_at`, `reason`, `reverted_at`, `reverted_by`, `revert_reason` | one row per merge |
| `merge_effect` | `merge_id`, `record_id`, `from_person`, `link_closed`, `link_opened` | what the merge moved, which the revert replays backwards |
| `pseudonym_domain` | `id`, `namespace` (unique), `name`, `created_at`, `retired_at` | |
| `pseudonym` | `person_id`, `domain_id`, `value` (uuid, unique), `minted_at` | immutable; resolved through the merge chain (section 8) |
| `steward_action` | `id`, `kind`, `actor`, `at`, `person_id`, `record_id`, `candidate_id`, `merge_id`, `reason`, `request_id` | the console's audit trail; every row also goes out as a BALP record (section 9) |
| `subscription` | `id`, `resource` (jsonb), `status`, `criteria`, `endpoint`, `error`, `created_at` | ITI-94 |
| `feed_outbox` | `id`, `subscription_id`, `bundle` (jsonb), `attempts`, `next_at`, `delivered_at` | ITI-93 messages to subscribers, delivered with bounded retry |
| `v2_receipt` | `control_id`, `sending_facility`, `received_at`, `ack_code` | the MSH-10 of each v2 message, so a resent message is acknowledged again and not applied twice (section 7) |
| `seal_key` | `key_id`, `pending_key_id`, `set_at` | one row: the id of the key the identifier columns are written under, and the key an unfinished `db reseal` moves them to |

**Sealing.** An identifier value is personal data and, in a domain marked
`sensitive` (the BSN, by default every domain), it is stored sealed:
`identifier_sealed` is a format byte, a random 96-bit nonce and the value
under AES-256-GCM with the key `[db] data_key_file` names (the base64 of 32
random bytes), the domain id as associated data, so a sealed value moved to
another domain does not open. In a domain not marked `sensitive` the value
is `identifier_value`, plain. Either way `identifier_hmac` is HMAC-SHA256 of
the domain id and the value, as the feed normalised it (section 5.1), under
a second key derived from the first with HKDF-SHA256 (RFC 5869), which is
what the equality lookups of ITI-83, ITI-104 and the `identifier` search
parameter use; the domain id in it keeps one value in two domains from
showing as one. A database dump therefore shows no sealed identifier, and a
lookup is one probe of the `(domain_id, identifier_hmac)` index. The
demographics are not sealed: the search of ITI-78 needs prefix and
`:exact` string matching (PDQm 3.2.0 §2:3.78.4.1.2.2) and the scorer needs
the values, so they stay plain in a database whose roles and disk the
operator protects, as FerroEHR keeps its party records plain and seals only
`national_identifier`. Every row carries the `key_id` it was written under,
16 bytes derived from the key, and `seal_key` names the store's key: the
first start records it, and a start under another key is refused, since it
would find nothing and answer every identifier as unknown. The key is
rotated by `db reseal`, the one subcommand that reads the old key: it marks
the configured key pending in `seal_key`, which refuses every start until it
finishes, rewrites the HMAC of every record and of every carried
identifier, and the sealed value of every sealed one, in batches of 500, one transaction each, so an interrupted run
resumes, and records the new key once no row is left under the old one. No
specification governs sealing: our own design.

**Roles.** The server connects as `ferropix`, which owns schema `pix` and
nothing else, and runs the migrations itself unless `[db] migrate = false`.
A deployment that migrates with another role runs `db migrate` as that role,
and the grants file gives `ferropix` the schema and its rows, without
`UPDATE` or `DELETE` on the write-once relations, which a trigger refuses to
any role. A boot gate refuses to serve when the connected role can read any
schema beside `pix`: a superuser, a member of `pg_read_all_data`, or a role
with `SELECT` on a table, view, materialized view, foreign table or sequence
in another schema, the system catalogs aside; `db migrate` is not gated,
since the role it runs as may be wider. After FerroEHR's
`verify_domain_isolation`.

### 4.3 Transactions and concurrency

Every write is one PostgreSQL transaction at `READ COMMITTED` with explicit
locks, and every steward decision carries the versions it was made against
(optimistic concurrency), so a decision made on a stale screen is refused
with `409` and the console re-reads. No specification governs the
boundaries: our own design.

- **The lock order.** A transaction first takes an advisory lock on every
  identifier it will lock or match on (its record's own, the survivor of a
  duplicate, every `authoritative` identifier the version carries), keyed
  by the first eight bytes of the HMAC and taken in ascending key order;
  then the record rows; then an advisory lock on each person, keyed by the
  random tail of its `uuidv7` and taken in ascending order. Two feeds that
  carry one BSN therefore run one after the other, and the second sees the
  first's commit. A person found merged once its lock is held, or a
  deadlock the database breaks, retries the whole transaction, at most
  three times.
- **A feed** (ITI-104 add or revise, ITI-93 create or update, ITI-8): lock
  the `source_record` row by `(domain_id, identifier_hmac)` with `SELECT …
  FOR UPDATE`, or insert it; insert the version and the identifiers it
  carries (a revision of a removed identifier brings it back; a revision of
  a subsumed one is refused); run the deterministic match
  (section 5.1) over the authoritative identifiers the version carries; take
  an advisory lock on each person the match names, in `uuid` order, so two
  feeds that join the same persons never deadlock; open or move the link;
  compute the probabilistic candidates (section 5.2) from the blocking keys
  and insert the open ones; insert the `steward_action` row the system
  action produces; commit. The BALP record is written to the audit spool
  after the commit, by the request handler, and the ITI-93 message to each
  matching subscription is inserted into `feed_outbox` inside the
  transaction, so a committed change is never unannounced (#104).
- **A Source's duplicate resolution** (ITI-104 with `replaced-by`,
  §2:3.104.4.2.3, or an ITI-93 merge, PMIR 1.6.0 §2:3.93.4.1.2.4): both
  records of the domain locked, the subsumed one inserted when the store
  lacks it and the survivor refused when the store lacks it or holds it
  removed or subsumed; the subsumed record takes the message's version; the subsumed record gets `active = false`
  and `replaced_by`; when the two records sit in different persons, the
  persons are merged with `kind = source` (the Source's statement is the
  ground), recorded in `merge` and `merge_effect` as a steward merge would
  be; then the cross-referencing runs again over the survivor (§2:3.104.4.2.3:
  "shall reapply its internal cross-referencing logic"). The subsumed
  record's link closes: a PIX query of its identifier follows `replaced_by`
  to the survivor and answers the survivor's set, which never names the
  subsumed identifier.
- **A removal** (ITI-104 `DELETE`, the Remove Patient Option, PIXm 3.1.0
  §2:3.104.4.3.3): the record locked, `removed_at` set, its link closed, and
  the identifier is answered as not found from then on. The versions stay,
  because the audit trail must still say what was held and when.
- **A steward link, not-a-match or deferral** (section 5.3): lock the
  candidate; refuse `409` when it is no longer `open` or when either
  record's head version moved past the version the candidate was computed
  on; a link of two records that sit in two persons is a merge of the
  persons, the older surviving, and takes the merge path below; a link of
  two records already in one person settles the candidate; record the
  action with the steward's reason; commit. A `not-a-match` holds while
  both records' current versions state the demographics the decided
  versions stated: a feed that changes only what the scorer does not
  compare leaves the pair out of the queue.
- **A merge** (section 5.3): advisory locks on both persons in `uuid`
  order; refuse `409` when either is no longer `active` or its version
  moved (a person's version is the time of its latest change: a link opened
  or closed, or a version fed to one of its records); the merged person
  gets `status = merged` and `merged_into`; every open link of the merged
  person is closed and reopened on the survivor, each move a `merge_effect`
  row; the candidates between the two persons' records are closed as
  `linked`; a PMIR merge message goes to `feed_outbox` (section 6.3); the
  action is recorded; commit.
- **An unmerge**: only the latest merge whose survivor is this person can
  be reverted, so reverts unwind in order; a revert replays
  `merge_effect` backwards, reactivates the merged person, reopens the
  links on it, marks the merge `reverted_at`, and records the action. PMIR
  says unmerge "is not supported by this transaction" and a feed that
  attempts one is answered `405` "unless local policy allows unmerge"
  (PMIR 1.6.0 §2:3.93.4.1.3): the local policy is that a steward may
  unmerge through the console and a Source may not through the feed.

Reads take no lock. A PIX query (section 6.1) is one statement: the record
by `(domain, hmac)`, its open link's person, that person resolved through
`merged_into` to the active survivor, and the open links of the survivor
joined to their records and domains. A PDQm search reads persons and their
current versions. Nothing a read returns is written back.

## 5. Matching

No specification governs the matching: PIXm places it outside IHE (PIXm
3.1.0 §2:3.104.4.1.3) and PDQm requires only that a match carry a score and
a grade (PDQm 3.2.0 §2:3.119.4.2.2.4). Our own design, in three tiers, with
the human in the middle.

### 5.1 Deterministic, on identifiers

Two records are the same patient when their current versions carry the
same identifier in the same `authoritative` domain, with the value
normalised by its domain's rule, and no hard conflict in demographics (a
birth date that differs in year, or an administrative gender that differs
where both are stated). The rule of a BSN system keeps the nine digits
(spaces, dots and dashes dropped, a leading zero dropped from an
eight-digit number restored) and requires the eleven-proof; every other
domain trims the value and keeps its case, as FHIR R4 compares an
`Identifier.value`. A value its domain's rule refuses refuses the feed.
The record joins the oldest of the persons its identifiers name, and every
other is merged into it with `kind = deterministic`. A hard conflict turns
the deterministic link into a candidate for review, its evidence carrying
the conflicts (`conflicts`, which the console draws first), graded at
least `possible`. A `local` domain never links
deterministically across domains: two hospitals that both number a patient
`12345` say nothing. The feed's own `replaced-by` is deterministic by
definition (section 4.3).

### 5.2 Probabilistic, on demographics, with explainable scores

The scorer is Fellegi-Sunter: for each compared field, an agreement weight
`log2(m / u)` and a disagreement weight `log2((1 - m) / (1 - u))`, where
`m` is the probability that the field agrees for a true pair and `u` the
probability that it agrees for a random pair. The configured defaults are
conservative and `[matching.fields]` overrides each; the `u` values can be
estimated from the store with `ferropix matching estimate`, which reads
field frequencies and nothing else. The comparators:

| Field | Comparator | Partial credit |
|---|---|---|
| family name | Jaro-Winkler over the normalised name | agreement above 0.92 counts whole, 0.85 to 0.92 counts half |
| given names | Jaro-Winkler over the first given name, and whether any given name matches | initials agree: half |
| birth date | exact | day and month transposed, or one digit off: half |
| administrative gender | exact | absent on either side: no weight |
| postal code | exact, then the prefix | the first four characters agree: half |
| address line, city | token overlap | |
| telephone, email | exact after normalisation | |
| mother's maiden name | Jaro-Winkler | |
| an identifier in a `local` domain shared by both records' domains | exact | a shared local identifier is evidence, never proof |

A partial agreement adds half the agreement weight and a field absent on
either side adds nothing; where a field holds several values (names,
addresses, telecom) the best pair counts, so every comparator is symmetric.
Names and addresses are compared folded (accents removed, lowercased,
punctuation a space), a telephone number on its last nine digits. The
defaults are `m` 0.95 and `u` 0.01 for the family name, 0.90 and 0.02 for
the given names, 0.97 and 0.003 for the birth date, 0.98 and 0.5 for the
gender, 0.85 and 0.001 for the postal code, 0.80 and 0.001 for the address,
0.60 and 0.0001 for telephone and email, 0.90 and 0.01 for the mother's
maiden name, and 0.50 and 0.00001 for a shared local identifier; the book's
Matching page lists them with their weights, and the scorer's corpus test
reads them from there.

The total is the sum of the weights in bits, mapped to `[0, 1]` by the
logistic `1 / (1 + 2^(10 - total))`: a total of 10 bits scores 0.5, the
point where the evidence outweighs prior odds of one in 1,024 that two
blocked records are one patient. The midpoint is fixed, never derived from
a deployment's thresholds or weights, so `search.score` is comparable across
deployments. The grade follows the thresholds:

| Grade | When | Used for |
|---|---|---|
| `certain` | score at or above `link_at` (default 0.95) | ITI-119 `onlyCertainMatches`; an automatic link when `auto_link = true` |
| `probable` | at or above `review_at` (default 0.80) | a candidate in the review queue |
| `possible` | at or above `possible_at` (default 0.60) | a candidate in the review queue, below the probable ones |
| `certainly-not` | below | not queued; answered by ITI-119 only without `onlyCertainMatches` and within `count` |

**Blocking.** A pair is scored only when it shares a blocking key: `(birth
year, the phonetic key of the family name)`, `(postal code, birth date)`, or
an identifier value shared across domains. The phonetic key is American
Soundex over the last word of the folded family name, so a Dutch prefix
(`van`, `de`, `van der`) does not decide the block. The keys are columns of
`source_record_version`, indexed, so a feed scores tens of pairs and not the
table.

**Evidence.** Every score is kept with its evidence: one entry per field
with the comparator, the two values as compared (every value masked to its
last three characters and at most half its length, section 12.5), the
agreement, the comparator's reading, and the weight it added, plus the
blocking key that produced the pair. The console draws
the breakdown from that, and the ITI-119 answer carries the score and the
grade and nothing of the evidence.

**Automatic linking is off by default.** `[matching] auto_link = false`
means a `certain` pair is still a candidate at the top of the queue: a
deployment decides, with its own data, when the scorer has earned the
right to link alone. With `auto_link = true` a `certain` pair merges the
two persons, the younger into the older, with `kind = probabilistic`, and
each moved record's link is of kind `probabilistic` and keeps the score
and the evidence; the merge is an `auto-link` system action. The
deterministic tier links alone always, because an authoritative identifier
is the Source's statement. A feed refreshes the open candidate of a pair it
scores again with the new versions, score and evidence, and leaves a
deferred one as the steward left it.

### 5.3 Human review

A candidate waits in the queue until a steward decides: **link** (the
records are one patient; a merge when they sit in two persons), **not a
match** (remembered, so the pair is not queued again until either record
changes), or **defer** (with a date and a note, back in the queue on the
date). Every decision carries the steward, the time and a reason, and is one
`steward_action` row and one BALP record. A decision never deletes
anything: a link closes and opens links, a merge is reversible, and a
not-a-match is a row. The queue's order is grade, then score, then age, so
the most likely duplicates with the oldest wait come first. The console is
the only client of the decisions, through the operator API (section 12).

## 6. The FHIR face

Mounted under `{base}/fhir`, FHIR R4 JSON and XML (every IHE transaction
requires both on the server side: PDQm 3.2.0 §2:3.78.4.1.2.4, PIXm 3.1.0
§2:3.104.4.1.2, PMIR 1.6.0 §2:3.93.4.1.2). `GET {base}/fhir/metadata` serves
the `CapabilityStatement` built from the three vendored actor statements,
with `conditionalUpdate` and, when `[pixm] remove_patient` is on,
`conditionalDelete` declared on `Patient` (PIXm 3.1.0 §2:3.104.4.1.3,
§2:3.104.4.3.3), the `$ihe-pix` and `$match` operations, and a
`supportedProfile` for the PIXm and PDQm Patient profiles. Every refusal is
an `OperationOutcome` with the issue the profile prescribes.

**The Patient resource FerroPIX serves is the person.** `Patient/{id}` is
the person's `uuid`; its `identifier[]` is the person's id in the Master
Patient Identification Domain (`[server] public_base_url`, PMIR 1.6.0
§2:3.93.4.1.3), then every identifier a PIX query answers for the person
(section 6.1), each with its domain's `system`; its demographics are the
current version of one open link's record, an `authoritative` domain's
first, then the most recently received (decision D21); a merged person
reads `active = false` with a `link` of type `replaced-by` to its active
survivor and no demographics, since its records moved to the survivor
(FHIR R4 Patient, "Patient records that have been merged"; PMIR 1.6.0
§2:3.93.4.1.3). This one choice makes `targetId` of ITI-83, the entries of
ITI-78 and ITI-119, and the Patient Master Identity of PMIR the same
resource. No specification fixes what a Manager's `Patient` is (PIXm
3.1.0 §2:3.104.4.1.3: "not required to maintain a Patient resource"): our
own design.

### 6.1 PIXm

**ITI-83, by `GET` and by `POST`.** `GET {base}/fhir/Patient/$ihe-pix?sourceIdentifier=…{&targetSystem=…}`
is the form PIXm prescribes (PIXm 3.1.0 §2:3.83.4.1.2), and `POST
{base}/fhir/Patient/$ihe-pix` with a `Parameters` body carrying the same
`sourceIdentifier` and `targetSystem` parameters is the form FHIR R4 gives
every operation (FHIR R4 Operations, "Executing an Operation": an operation
is invoked by `POST` of a `Parameters` resource). The two answer identically,
tested over both forms (#52). FerroHEALTH consumers use `POST`
(`ihe_iti::pixm::Invocation::Post`), so a BSN never appears in a URL, a
browser history, a proxy log or a trace; the request log records the
matched route and never a query string (section 9).

`sourceIdentifier` is exactly one `system|value` token (§2:3.83.4.1.2.1);
`targetSystem` repeats (§2:3.83.4.1.2.2) and the answer holds, for each
requested domain the server knows a value in, one `targetIdentifier`, and
with no `targetSystem` every identifier of the person except the one asked
(§2:3.83.4.2.2.1: "shall not be included in the returned Response"), plus one
`targetId` naming `Patient/{person id}`. The answers:

| Case | Status | `OperationOutcome` issue | Section |
|---|---|---|---|
| found | `200`, `Parameters` | | PIXm 3.1.0 §2:3.83.4.2.2.1 |
| the domain is known, the identifier is not, or was removed | `404` | `error`, `not-found`, "sourceIdentifier Patient Identifier not found" | §2:3.83.4.2.2.2, §2:3.104.4.3.3 |
| the source domain is unknown or retired | `400` | `error`, `code-invalid`, "sourceIdentifier Assigning Authority not found" | §2:3.83.4.2.2.3 |
| a target domain is unknown or retired | `403` | `error`, `code-invalid`, "targetSystem not found" | §2:3.83.4.2.2.4 |
| the identifier's record is subsumed, or its person merged | `200`, the survivor's identifiers | | §2:3.83.4.2.2.5 leaves the choice to policy; FerroPIX follows §2:3.104.4.2.3, which replaces the subsumed identifier's references with the survivor's, so the consumer gets the answer the cross-reference now holds |
| the caller's token lacks the read scope | `401` or `403` | `security` | section 9 |
| the identifier has no record of its own and the records that carry it are in two persons | `409` | `error`, `multiple-matches` | no specification governs this: our own design (#167), so two persons' identifiers are never mixed |

An identifier in an `authoritative` domain that no Source feeds, such as the
BSN on a hospital's `Patient`, is found through the current versions that
carry it and answered among the person's identifiers, when no record of its
own exists (#167; our own design). The one status table of the face, with
every row above and the ITI-104 ones below, is `ferropix_server::fhir::status`.

**ITI-104.** `PUT {base}/fhir/Patient?identifier=system|value` with a
`Patient` body (PIXm 3.1.0 §2:3.104.4.1.2, FHIR R4 conditional update): no
match creates the record (`201`), one match revises it (`200`), and the
Manager answers `412` when the condition matches more than one, which the
store's uniqueness makes impossible. The body must carry the domain's
identifier (§2:3.104.4.1.2) and is accepted up to the PDQm Patient profile
(§2:3.104.4.1.2.1). A body whose `link` carries `replaced-by` is the Resolve
Duplicate message (§2:3.104.4.2.2), applied as section 4.3 says. `DELETE
{base}/fhir/Patient?identifier=…` is the Remove Patient message, served
only with `[pixm] remove_patient = true` (§1:41.2.1, §2:3.104.4.3). The
caller must be the domain's one Source: a token whose client id is not the
domain's `source_client_id` is refused `403` with a `forbidden` issue
(§2:3.104.5: "should check that the Patient Identity Source is only making
changes to Patient Identifier Domain for which it is authorized"). The new
cross-reference is visible to PIX queries at the commit (§2:3.104.4.1.3). A refused feed answers `400` for
a message ITI-104 does not admit (as `ihe-iti` reads it) and for an unknown or
retired domain (`code-invalid`, after §2:3.83.4.2.2.3), and `422`
`business-rule` for a value its domain's rule refuses, a revision of a
subsumed identifier, or a survivor that names no active record (FHIR R4
http.html §3.1.0.4.2); a success answers an `OperationOutcome` with one
`information` issue and a `Location` naming the person, and a removal of an
identifier the store does not hold answers `200` (§3.1.0.7). These choices
are our own design where PIXm points at FHIR R4 for the response
(§2:3.104.4.1.4).

### 6.2 PDQm

**ITI-78.** `GET {base}/fhir/Patient?…` and `POST {base}/fhir/Patient/_search`
(the Supplier shall support both, PDQm 3.2.0 §2:3.78.4.1.2) with the
parameters of §2:3.78.4.1.2.1 (`_id`, `active`, `family`, `given`,
`identifier`, `telecom`, `birthdate`, `address` and its parts, `gender`,
`mothersMaidenName`), AND and OR between values, the `:exact` modifier on
every string parameter (§2:3.78.4.1.2.2), the two required combinations
(§2:3.78.4.1.2.1.1), the domain filter `identifier=system|` which keeps only
the identifiers of the named domains in the answer (§2:3.78.4.1.2.3), and
`_count` with `Bundle.link` paging. A parameter the Supplier does not
support is dropped and the `self` link says what was respected
(§2:3.78.4.1.2.1.1). A string search is a case- and accent-insensitive
prefix match (FHIR R4 Search, string). The six cases of §2:3.78.4.1.3 map
to: `200` with the `total`; `200` with identifiers filtered; `200` with
`total` 0; `404` with a `warning` `not-found` issue "targetSystem not found"
for an unknown domain (the preferred response of case 4); `406` for a
`_format` the server cannot produce; and, for a merged person, the
`[pdqm] deprecated` policy: `inactive` (the person with `active = false`
and its `replaced-by` link, the survivor included in the Bundle as an
`include` entry) by default, or `omit`. `GET {base}/fhir/Patient/{id}` is
the retrieve (§2:3.78.4.3): a merged person answers `200` with
`active = false` under `inactive` and `404` under `omit` (case 3), and a
format the server cannot write `400` (§2:3.78.4.4.2).

The search is one statement over the persons and the demographics each
`Patient` shows, read in one read-only snapshot with the page's Patients.
A string parameter compares both sides folded by PostgreSQL's `unaccent`,
installed into `pix` as `btree_gist` is, and lowercased; an `identifier`
is found by the HMAC of its normalised value, in the records' own
identifiers and in the authoritative identifiers they carry with no record
of their own, as a PIX query finds it, so no sealed value is opened to
search; a `birthdate` compares the ranges of days the value and the stored
date cover. A merged person holds no records, so only `_id` and `active`
find it. A page holds `_count` persons, 20 by default and at most 100, in
id order after the cursor the `next` link names, so a person created or
merged while a client pages moves no other person (decision D22). The
domain filter recognises every unretired domain the store holds and the
Master Patient Identification Domain. `_lastUpdated`, which the vendored
Supplier statement lists and §2:3.78.4.1.2.1 does not, is not read: the
person has no version of its own to date.

**ITI-119.** `POST {base}/fhir/Patient/$match` with the PDQm Match input
`Parameters` (`resource`, `onlyCertainMatches`, `count`), or a bare `Patient`
body when neither parameter is used (PDQm 3.2.0 §2:3.119.4.1.2). The answer
is the PDQm Match Output Bundle: one entry per person found, ordered from
most to least likely, each with `search.score` in `[0, 1]` and the
`match-grade` extension (§2:3.119.4.1.3, §2:3.119.4.2.2.4), computed by the
scorer of section 5.2 against the input as if it were a record: the
records sharing one of the configured blocking keys with the input are
scored, the input's `local` identifiers compared by their HMAC, and each
person takes the best score of its records; a person graded
`certainly-not` is no match and is left out (decision D23). With
`onlyCertainMatches = true` only `certain` entries are returned, and when the
certain entries are more than one and not one person, the Bundle is empty
(case 5). `count` cuts the ordered list. A merged person holds no records,
so the match finds its survivor, the second option of case 8 under either
`[pdqm] deprecated` policy. An input stating fewer than three of the
scorer's fields rides with an `OperationOutcome` entry of severity
`warning` and code `incomplete` (case 10).

Every search, read and match, answered or refused, writes the PDQm
Supplier BALP record with its raw request, one per person in the answer
or one naming no patient, before the answer is sent; a record that cannot
be stored withholds the answer with `503` (decision D24).

### 6.3 PMIR

**ITI-93, received.** `POST {base}/fhir` with a PMIR message Bundle: a
`MessageHeader` as `entry[0]` and a history Bundle as `entry[1]` (PMIR 1.6.0
§2:3.93.4.1.2.2, §2:3.93.4.1.2.3). The Registry creates, updates and deletes
persons in the Master Patient Identification Domain, which is `[server]
public_base_url` (§2:3.93.4.1.3), and merges when an entry carries
`replaced-by` (§2:3.93.4.1.2.4): the deprecated person gets `active = false`
and the `replaced-by` link, as section 4.3's merge path does. An attempt to
unmerge through the feed is answered with `entry.response.code` `405`
(§2:3.93.4.1.3). The response is a message Bundle with one `MessageHeader`
carrying the outcome (§2:3.93.4.2.2).

**ITI-93, sent.** Every committed change to a person is an ITI-93 message to
each active subscription whose criteria match (section 4.3 inserts it into
`feed_outbox`). A sender delivers in order per subscription, with the retry
bounds of `[pmir] feed`, and sets the subscription's `status` to `error`
with the error text when the bounds are spent (PMIR 1.6.0 §2:3.94.4.1.3).
The subscriber side is `ihe_iti::pmir`, already built.

**ITI-94.** `POST {base}/fhir/Subscription` creates one (`201`,
`Location`), with the criteria the Registry must accept: `Patient`,
`Patient?_id=`, `Patient?organization=`, `Patient?identifier=` (PMIR 1.6.0
§2:3.94.4.1.2.1.1); `GET`, the enable and disable `PUT`, and `DELETE` of
`Subscription/{id}` are §2:3.94.4.3, §2:3.94.4.4 and §2:3.94.4.5. A
subscription's `channel.endpoint` must be `https`; a loopback `http`
endpoint is admitted under `[dev]` only.

### 6.4 After a merge or a removal

The PIXm and PMIR texts each leave the answer for a deprecated patient to
policy and list the admissible forms (PIXm 3.1.0 §2:3.83.4.2.2.5, PMIR
1.6.0 §2:3.93.4.1.3.1, PDQm 3.2.0 §2:3.78.4.1.3 case 6). FerroPIX's policy,
in one place so every face agrees: a **merged** identifier resolves to the
survivor's identifiers in ITI-83 and ITI-9 (the cross-reference's purpose
is to answer with what it now holds), a merged person reads `inactive` with
its `replaced-by` link in a search and a read, and a **removed** identifier
is `not-found` everywhere (PIXm 3.1.0 §2:3.104.4.3.3: "shall not return the
removed identifier").

## 7. The HL7 v2 face

ITI TF-1 defines PIX and PDQ with the HL7 v2 transactions ITI-8 Patient
Identity Feed, ITI-9 PIX Query, ITI-10 PIX Update Notification, ITI-21
Patient Demographics Query and ITI-22 Patient Demographics and Visit Query.
Their ITI TF-2 texts are vendored cache only, as the ITI-55 text is: each
reproduces HL7 v2 segment tables whose rights HL7 reserves, so
`scripts/vendor/ihe-iti-tf.sh` fetches them by checksum into the ignored
`.vendor-cache/ihe-iti-tf-hl7v2/` (`docs/VERSIONS.md`). This section names
the transaction sections that govern each part; the issue that builds a
transaction reads its text in full.

- **Transport.** One MLLP Release 1 listener on `[hl7v2] listen` (default
  `0.0.0.0:2575`), the frame codec, the idle and frame timeouts, the frame
  ceiling, the MSH-18 character sets and the positional parser all from
  `ferrobridge-hl7v2` and the tables from `hl7v2-types`, pinned exactly
  (`CLAUDE.md` §Scope: never a second parser). TLS on the listener is the
  deployment's: a sidecar or the network, as FerroBRIDGE's face does. The
  listener runs under the same drain as the HTTP server.
- **Senders.** `[[hl7v2.senders]]` names each accepted sending application
  and facility (MSH-3, MSH-4) and the identifier domain it feeds, which is
  the v2 counterpart of a Source's client id: a message from an unlisted
  sender is acknowledged `AR` and not applied. One sender feeds a domain
  (ITI TF-2 §3.8.4.1.3), and a sender that names no domain may query and
  not feed.
- **Assigning authorities.** A universal id of type `ISO` names the domain
  `urn:oid:<oid>`, one of type `URI` the domain of that system, and a
  namespace id the domain `[hl7v2.namespaces]` maps it to; an answer writes
  all three subcomponents (ITI TF-2 §3.9.4.2.2.5). No specification governs
  the mapping to a FHIR system: our own design.
- **One feed path.** An ITI-8 message is read as the ITI-104 message of the
  same patient (PID through the HL7 v2-to-FHIR maps, an A40 as the Resolve
  Duplicate message) and applied by the same feed transaction, and ITI-21
  is read as the ITI-78 search it names, so the two faces cannot drift.
- **ITI-8** (ADT A01, A04, A05, A08 add and revise, ITI TF-2 §3.8.4.1.1; A40
  merge, ITI TF-2 §3.8.4.2.1) writes a source record version from PID, with
  the domain from PID-3.4 checked against the sender's domain, and is
  acknowledged in original mode (`AA`, `AE`, `AR`) by
  `ferrobridge_hl7v2::ack`, MSA-2 echoing MSH-10. A message whose MSH-10 was
  already acknowledged by this sender is acknowledged again with the same
  code and not applied (`v2_receipt`).
- **ITI-9** (QBP^Q23, RSP^K23, ITI TF-2 §3.9.4) answers the same
  cross-reference as ITI-83, with the same policy for merged and removed
  identifiers (section 6.4), and the domain filter from QPD-4
  (ITI TF-2 §3.9.4.1.2.2).
- **ITI-21** (QBP^Q22, RSP^K22, ITI TF-2 §3.21.4) answers the same search
  as ITI-78 over the QPD-3 parameters (ITI TF-2 §3.21.4.1.2.3), with
  continuation (DSC, ITI TF-2 §3.21.4.1.3.3) over a server-side cursor
  bounded in time.
- **ITI-22** (QBP^ZV1, ITI TF-2 §3.22) is refused: FerroPIX does not act
  as its Patient Demographics Supplier. Every patient its RSP^ZV2 returns
  is a PID-PV1 group with PV1-2 Patient Class required
  (ITI TF-2 §3.22.4.2.2.7, Table 3.22-9; Cases 1 and 2 of
  §3.22.4.2.2.11), and the Supplier shall support at least one of the PV1
  query fields (Table 3.22-4, §3.22.4.1.3.2). FerroPIX keeps no visit, so
  it could only answer a PV1 it made up and a visit field it cannot search.
  A QBP^ZV1 is acknowledged `AR` with ERR code `200`, unsupported message
  type (HL7 table 0357), and the text names ITI-21, the demographics query
  over the same search (#199).
- **ITI-10** (ITI TF-2 §3.10.4.1) is an outbound notification to configured
  consumers after a cross-reference change; it is the v2 counterpart of the
  ITI-93 sender and shares its outbox.

Every v2 message is logged by its MSH-10, sender and acknowledgment code,
never by its content; the BALP record of each transaction is the one the
ITI TF-2 text prescribes.

### 7.1 The XCPD Responding Gateway

ITI-55 (ITI TF-2 §3.55) is answered synchronously at `POST {base}/xcpd`
behind `[xcpd]`, read and written by `ihe-iti`'s `xcpd::responding`
(D32). The ITI-55 text is vendored cache only, as section 7 says.

- **The match.** An identifier of the query in a domain the store holds is
  looked up in the cross-reference; with none found, the demographics are
  scored as an ITI-119 input. `certain` and `probable` persons are Case 1
  or 2, `possible` ones alone Case 3 asking for the scored attributes the
  query left out, none Case 4 (§3.55.4.2.3). IHE leaves matching to the
  gateway (§3.55.4.1.3.1).
- **The answer.** A person's identifiers in `[xcpd] domains`, each a
  `urn:oid:` system because an `II` root is an OID (ITI TF-2 Appendix E);
  the first held is `Patient.id`, the rest `OtherIDs` (Table
  3.55.4.2.2.2-1), and a person holding none is left out.
- **Refusals.** A Deferred request is `NS250` (§3.55.4.1.3) until the
  Deferred Response Option lands (#232); an unlisted initiating community
  and an ambiguous identifier are Case 5 `AnswerNotAvailable`, another
  target community Case 5 (§3.55.4.1.3.2), a store fault Case 5
  `InternalError`.
- **Transport and audit.** The endpoint takes no bearer token: the mutual
  TLS of an ATNA Secure Node is the deployment's, as for the MLLP listener.
  The audit message of §3.55.5.1.2 is written as a BALP `AuditEvent` the
  way section 7's v2 records are, and stored before the answer leaves.

## 8. Pseudonyms and the Dutch functions

**Pseudonyms.** FerroEHR names a patient by a value that identifies no one
on its own: a UUID per patient and `subject_namespace`. FerroPIX mints it.
A pseudonymisation domain is a namespace; `POST {base}/operator/pseudonyms`
(and the console) resolves a person, through the merge chain to the active
survivor, to its pseudonym in a domain, minting one when the survivor has
none. A pseudonym row is immutable and attached to the person it was minted
for: after a merge, both persons' pseudonyms resolve to the survivor,
through `merged_into`; after an unmerge, each resolves to its own person
again. The reverse resolution, from a pseudonym to a person, is a steward
action with a reason, audited as a disclosure (section 9). No specification
governs the pseudonym: our own design, after FerroEHR's linkage domain.

**The BSN.** Patients are identified by BSN in the Netherlands, the
authentic source being the BRP (NL Generic Functions, the Identification
page); the naming systems are `nl_generic_functions::identification::BSN_SYSTEMS`.
A BSN domain is `authoritative` and `sensitive`: it links deterministically
and is sealed at rest. The BSN's eleven-proof is the domain's normalisation
rule.

**The pseudonymised BSN and BSNk.** The Generic Functions name a patient
toward the national index by a pseudonymised BSN
(`http://fhir.nl/fhir/NamingSystem/pseudo-bsn`, the Identification page) and
leave the Pseudonymization Service, BSNk-pp, outside the guide's current
version (the Localization page: "this version of the IG will not go into
the details"). FerroPIX treats a pseudo-BSN as an identifier in its own
domain (`kind = authoritative`), fed by the system that obtained it, and
calls no BSNk service; the BSNk transformation is filed as its own work
when the guide specifies it.

**The NVI.** FerroPIX registers nothing at the NVI and answers no
localization: `nl_generic_functions::nvi` is the client FerroFED runs. The
MPI's part is to hold the pseudo-BSN beside the local identifiers, so a
data holder can derive the key the NVI wants from the patient it knows.

## 9. Authorization, audit, health, the request log and the usage report

**Authorization** is `ferrohealth-smart`, the family's resource-server
crate: every request to `{base}/fhir` and `{base}/operator` carries a bearer
token one of the configured issuers signed, checked against the issuer's
RFC 8414 metadata and key set, with the audience FerroPIX names. The
scopes:

| Caller | Scope | Admits |
|---|---|---|
| a PIXm or PDQm Consumer | `system/Patient.rs` (SMART App Launch 2.2.0 scopes) | ITI-83, ITI-78, ITI-119, ITI-9, ITI-21 |
| a Patient Identity Source | `system/Patient.cud`, and the token's client id equal to the domain's `source_client_id` | ITI-104 and ITI-93 for that domain |
| a PMIR subscriber | `system/Subscription.cruds` | ITI-94 |
| a data steward | `[auth] steward_scope` (`__ferropix.steward`) | the operator API: search, the record, the queue and its decisions, merge and unmerge, pseudonym resolution, the audit trail |
| an administrator | `[auth] admin_scope` (`__ferropix.admin`) | the domains and the pseudonymisation domains, the matching thresholds in effect |

The product scopes take the extension form SMART App Launch 2.2.0 gives
(a string starting with `__`), as FerroBRIDGE's operator scope does. A
token is never forwarded: the ATNA repository and the PMIR subscribers are
called with FerroPIX's own client-credentials token.

The permission table (`app/ferropix-server/src/auth/table.rs`) holds one
row per route of section 6 and section 12.4, and the router mounts exactly
its rows, so no route is reachable without one. Beside the SMART scopes,
each IHE transaction admits the scope its profile defines for an IUA
grouping (PIXm 3.1.0 §2:3.83.5.2, §2:3.104.5.2; PDQm 3.2.0 §2:3.78.5.2,
§2:3.119.5.2). The rest is our own design, no specification governs it:

- An operation or a search needs `s` and the read of `Patient/{id}` `r`;
  ITI-104's conditional update needs `u`, which SMART App Launch 2.2.0
  admits for an update that creates, and its delete `d`.
- A `patient/` scope is refused behind the gate as a missing permission:
  it reaches one patient's compartment, and a Master Patient Index has no
  use for one.
- A Source's `client_id` counts only when the issuer that signed the token
  lists it in `clients`, so two authorization servers cannot each vouch
  for a client of the same name; a Basic user of the development profile
  counts by its user-id. Every domain an ITI-104 request's `identifier`
  names must be the caller's; a domain FerroPIX does not know is left to
  the transaction's handler.
- A path under `{base}/fhir` or `{base}/operator` that no row names needs
  a credential before the router answers `404`, so an anonymous probe
  learns nothing of the router.
- A refusal is an `OperationOutcome` with a `security` issue (`401`,
  `403`), or a `forbidden` issue for a Source outside its domain, with a
  fixed diagnostics text.
- The discovery document and the `CapabilityStatement` name the first
  issuer whose metadata names a `token_endpoint`; issuers none of which
  names one refuse the start, and a development surface of Basic users
  alone serves no discovery document.
- Basic users need the `[dev]` table beside `profile = "development"`.

**Audit** is BALP through `ihe-atna`. Every transaction writes the
`AuditEvent` its profile prescribes for the Manager, the Supplier or the
Registry (PIXm 3.1.0 §2:3.83.5.1.2, §2:3.104.5.1.2; PDQm 3.2.0 §2:3.78.5.1.2,
§2:3.119.5.1.2; PMIR 1.6.0 §2:3.93.5.1), with the patient as the subject
entity (BALP 1.1.4 §1:52.4.1.6) and, for a search, the raw query base64 in
`entity.query` (BALP 1.1.4 §1:52.4.1.4), which is why the audit record and
only the audit record carries an identifier. Every steward action is a
BALP record too: a decision is a RESTful activity on the person (BALP
1.1.4 §3:5.7.3), and a reveal or a reverse pseudonym resolution is a
disclosure with the steward as the agent and the reason in `purposeOfEvent`.
The records go to the ATNA repository in `[audit]` through the ITI-20
store-and-forward spool `ihe-atna` provides (one `0600` file per record in
a `0700` directory, drained in order), and a transaction whose record
cannot be spooled fails closed with `503`, as FerroFED's does. The console's
audit trail view reads `steward_action`, the server's own copy, because the
repository is the deployment's and may not be readable by the console.

How the trail is built (`app/ferropix-server/src/audit/`), our own design
where no specification governs it: a layer in front of every audited row of
the permission table (every FHIR transaction and every steward action that
reads or changes a person) takes the record's place in the spool, 16 KiB,
before the handler runs, so a full spool answers `503` with a `transient`
issue before any change; the handler fills the place once it knows the
outcome, and a place left unfilled is given back. A search writes one record
per person found and one with no patient when none was (BALP 1.1.4
§1:52.4.1.6 recommends one per patient); a steward's view is BALP Patient
Read and a decision Patient Update. A token whose subject is its own client
is recorded as the system acting for itself, any other with its user. With
no `[audit] url` the records stay in the spool until one is named. The
readiness indicator `audit_spool` is down under a tenth of either bound.

**Health.** `GET {base}/health` is liveness; `GET {base}/health/readiness`
runs the indicators (the database, the audit spool's headroom, the MLLP
listener's socket, the feed sender's backlog) and answers `503` with the
`ferropix_operator_api::health::Readiness` document naming each one that is
down. `GET {base}/operator/dependencies` is the same report with the
ATNA repository's and each subscriber's last outcome, behind the steward
scope, so an anonymous reader learns no endpoint, the ruling FerroFED made
as its decision A83. The container `HEALTHCHECK` is `ferropix healthcheck`.

The registry holds the indicators of the subsystems that exist, each added
with its subsystem; each check is bounded to two seconds and a check past
the bound is down, so a wedged subsystem cannot hang the probe, and the
probe runs the checks on the request instead of reading a cache a
background loop fills (our own design, after FerroBRIDGE's registry).

**The request log** records, per request, the request id, the matched route
template, the method, the status, the latency, the caller's client id or
subject, and the domain a feed wrote to. It never records a query string,
a body, a header value or an identifier: the ITI-83 `GET` form carries the
identifier in the query, and the log records `/fhir/Patient/$ihe-pix` and
nothing after it, which the test of #52 proves over a BSN sent by `GET`.
`tracing` spans carry the same fields and no more. The line is written
under the target `ferropix_server::request` after routing, so it reads the
matched template; a request no route matched is logged as `<unmatched>`
with no part of its path. The guard leaves the caller (the token's
`client_id`, or its subject when it names none) and the domains a
successful feed wrote to on the response, where the log reads them; a
request the gate refused names no caller (our own design).

**The usage report.** FerroEHR, FerroTERM and FerroBRIDGE send an
installation report to FerroPULSE on its envelope v2; FerroPIX does the
same under `[usage_report]` (`app/ferropix-server/src/usage_report/`): on
by default with a notice at every start, `enabled = false`, an unset
`state_dir` or a CI environment (`GITHUB_ACTIONS`, `CI`) keeps it off, one
`start` and one `daily` report with the version, the deployment kind the
artifact states (`compose`, `helm`, `binary`), the instance id under
`state_dir`, latency and counts by route group from the request log, and
never an identifier, a domain name or a host name. Its `status` and
`counters` keys are listed in FerroPULSE's `docs/report-v2.md` §FerroPIX,
every report is validated against the vendored schema, and
`scripts/checks/usage-report-off.sh` holds every launcher of the server in
a test lane to the switch. The
console sends none: FerroTERM dropped its console from the report (#736),
and the server's report counts the operator API's routes.

## 10. The two images and the release

No specification governs packaging: our own design, after FerroFED.

**The images.** `docker/Dockerfile` and `docker/console/Dockerfile`, each one
stage on `gcr.io/distroless/static-debian13:nonroot` pinned by digest,
copying the static binary the release lane stages under
`dist/<os>_<arch>/` (and, for the console, the site bundle under
`dist/console/site/`), compiling nothing, running as `65532:65532`, with the
`HEALTHCHECK` on the binary's own `healthcheck` subcommand, the OCI labels
(title, description, url, documentation, source, vendor, authors, the
manufacturer, licence `BUSL-1.1`, base name). The server image exposes
`8080/tcp` and `2575/tcp`; the console image `3000/tcp`. The server's audit
spool directory is created in the image, `0700`, owned by the runtime user.

**The quickstart.** `compose.yaml` at the root runs the published server
image with the hardening set (`read_only`, `cap_drop: [ALL]`,
`no-new-privileges`, `tmpfs` for `/tmp`), the official `postgres:18` image
pinned by digest with its data on a volume, every secret as a file secret
(`FERROPIX__DB__URL_FILE`), the `console` profile that runs the console
beside the server, and a `dev` issuer for the sign-in: the testkit's stub
provider, NAV's `mock-oauth2-server`, which owns the network namespace the
server joins, because the server reads its issuer's metadata at start and
admits plain `http` only on loopback (RFC 8414 §2). FerroBRIDGE's
`compose.yaml` is the model; the `cdm` profiles are not needed here.

**Kubernetes.** FerroFED ships plain manifests under `deploy/kubernetes`
and FerroEHR a Helm chart under `deploy/helm`; FerroPIX ships a chart,
after FerroEHR's, once the images exist (section 14, v0.0.4). Nothing is
scaffolded before its issue.

**The release.** `release.yml` is dormant until a `v*` tag is pushed. Its
`plan` job refuses a tag whose milestone is missing or holds an open item;
`github-release` creates the draft; `build-binaries` calls the reusable
`release-build.yml` per target (`x86_64` and `aarch64`, `musl`, `cargo
auditable`), which attests each tarball; `build-image` calls
`release-image.yml`, which verifies the tarballs against `release-build.yml`'s
signer identity before it extracts them, pushes the multi-platform index,
and attests the index and each manifest with SLSA provenance and an SPDX
SBOM as OCI referrers; `build-console` calls `release-console.yml`, whose
`binary` and `site` jobs attest the console's binary and bundle and whose
`image` job packs and attests `ferropix-console`; `finalize-release`
publishes; `crates` publishes `ihe-iti` and `nl-generic-functions` when they
moved; `milestone` closes the milestone. A reusable workflow runs on a VM a
caller cannot add steps to, which is what keeps the signing identity out of
reach of the build steps: SLSA v1.2 Build Level 3 (SLSA, Build
requirements; GitHub, "Using artifact attestations and reusable workflows
to achieve SLSA v1 Build Level 3"). A consumer verifies with `gh attestation
verify oci://ghcr.io/ferrohealth/ferropix:<version> --repo
FerroHEALTH/FerroPIX --signer-workflow
FerroHEALTH/FerroPIX/.github/workflows/release-image.yml`, and the same
for the console against `release-console.yml`.

## 11. The CI lanes

`ci.yml` keeps its workflow tier and its Rust tier under the single
required `conclusion` check. It runs on `opened`, `synchronize` and
`reopened` only, so a commit gets one run; `pr-guards.yml` re-runs the guard
a changed label (`no-changelog`, `no-crate-bump`, `no-book`) or description
affects in the head commit's run once that run completes. Beside FerroFED,
FerroPIX lacks these lanes, each added as its own issue (section 14):

| Lane | What it does | Why FerroPIX needs it |
|---|---|---|
| `sonar.yml` | SonarQube Cloud over the Rust tier and the shell, YAML and JSON the Rust gates never read; advisory, gates no merge | the second opinion the family runs; `sonar-project.properties` scopes it |
| `fuzz.yml` | `cargo fuzz` for a bounded time, weekly and on a pull request that touches a target's input | the server parses untrusted input on every request: the `Parameters` and `Patient` bodies, the search parameters, the MLLP frame and the v2 message; a panic is a defect, an `Err` is the parser's job |
| `pin-freshness.yml` | reads the newest upstream release of every pin no Dependabot ecosystem watches (the FHIR packages, the IHE pages' digests, the container images, the distroless digest, Chrome for Testing) and opens one issue when one is behind | the corpora and the images are pinned by digest and nothing else watches them |
| `contribution-licence.yml` | the licence box of the pull request body as a required check of its own, re-run on `edited`, skipped for a bot | `pr-guards.yml` re-runs the `ci.yml` job on an edited body, inside `conclusion`; a workflow of its own reports the box without waiting for the CI run |
| `console` (in `ci.yml`) | clippy on `wasm32-unknown-unknown`, the site bundle against its byte budget | the console's browser half |
| `journeys` (in `ci.yml`) | the console's browser journeys and the painted-contrast battery behind `FERROPIX_JOURNEYS` | section 12.5 |

## 12. The operator console

No specification governs a patient index's user interface: our own design.
What the console reads is spec-bound through the server. The console is
the fifth FerroUI console, and the owner's bar for it is the best user
interface and experience in the family. This section is the heart of the
design: a data steward's tool, built around the one job the specifications
leave to people, deciding whether two records are one patient (PIXm 3.1.0
§1:41.4.1).

### 12.1 Who it is for, and what it is not

The reader is a **data steward**: someone who works a queue of possible
duplicates for hours, resolves the cases a system cannot, answers "which
identifiers does this patient have" for a colleague, and is accountable for
every link and merge. The console is shaped for that work: the queue first,
the keyboard everywhere, identifiers masked until a reveal is needed and
recorded, every decision with a reason, and every destructive step shown
in full before it is confirmed.

It is not a CRUD page over the tables, and it is not a patient portal. It
links no workspace crate beside `ferropix-operator-api` and draws every
element with `ferrohealth-ui` in the library's FerroPIX brand, Plum & Iron
(`Brand::Pix`, section 12.7 gap 7); a component the library lacks is
filed and built there (section 12.7). It holds no token in the browser: the steward
signs in at the console's server (OpenID Connect Core 1.0 §3.1,
authorization code with PKCE), the session lives on the console's server
behind one `HttpOnly` cookie, and the console calls the operator API with
the steward's own token, so the server records the steward as the actor of
every action. Every server function is a `POST` and checks the session
first. Nothing patient-related is kept in browser storage, a URL query or a
cache (`Cache-Control: no-store` on every page and answer).

**The one identifier a URL may carry is a person id**, the `uuidv7`
FerroPIX minted, which names no one outside FerroPIX, as FerroEHR's console
carries an `ehr_id` in `/ehrs/{ehr_id}`. A business identifier (a BSN, a
hospital number), a name or a birth date never travels in a URL: the search
posts, the results live in the page, and the listings keep their page, size
and sort in the session (`StateSource::Session`). A page that shows a
person records a `steward_action` of kind `view`, so the trail says who
looked.

### 12.2 Information architecture

The shell is the library's: a top bar with the global search field, the
server's status pill and the steward's menu, which holds the theme (the top
bar carries no theme toggle, FerroUI family rule 5); a grouped navigation
beside the page from 64 rem, a drawer below it; the footer with the product,
its version and the manufacturer. The groups, as the reviewed design in
FerroUI's `design/consoles/ferropix/` draws them:

| Group | Entry | Route | For |
|---|---|---|---|
| Work | Dashboard | `/` | the day's state: queue depth, match rates, feed activity, the listeners |
| Work | Review queue | `/queue`, `/queue/{candidate}` | the possible duplicates, oldest and likeliest first, and the comparison of one pair |
| Patients | Search | `/search` | by identifier (posted) or by demographics |
| Patients | Patient | `/patients/{person}` | every identifier per domain, the source records side by side, the history, the pseudonyms; the merge and unmerge from here |
| Registry | Sources | `/sources`, `/sources/{source}` | every system allowed to feed, its channel and how it is admitted (#120) |
| Registry | Identifier domains | `/domains`, `/domains/{domain}` | the assigning authorities, each with its one Source and its feed activity |
| Registry | Pseudonymisation domains | `/pseudonym-domains` | the namespaces and their counts |
| Trail | Audit trail | `/audit` | every steward action, filtered by actor, kind, person and time |
| Console | Configuration | `/configuration` | the thresholds, weights, blocking keys and rules in effect (#121) |
| Console | About | `/about` | the server's `CapabilityStatement`, versions, the manufacturer |
| | Your preferences | `/preferences` | behind the steward's name: FerroUI's three tabs, with the reveal duration and hide on leave in Appearance |
| | Sign in | `/sign-in` | the provider's action starts the sign-in at `GET /login` |

The global search field takes one thing: an identifier, a name, or a birth
date. The console reads its shape (an eleven-proof number is a BSN, two
words are a name, a date is a date), posts it, and lands on the search
results with the field pre-filled, so "find this patient" is one keystroke
(`/`) and one Enter from anywhere.

### 12.3 The core workflows and their screens

Each screen below names what it reads (the operator API), the FerroUI
components it draws with, its empty, loading and error states, its keyboard
path and how it reflows on a phone. The wireframes are in section 12.6.

**Sign-in** (`/login`). The library's `sign_in` card with the provider
button and, under `[dev]`, the Basic form; the notice names a refused
sign-in by its cause (`401`) or an unreachable provider (`502`), as
FerroFED's does. No patient data is drawn before a session exists.

**Dashboard** (`/`). Reads `GET /operator/overview`. Four stat cards
(`stat_card`): the open queue (with the number waiting more than seven
days), the auto-link rate over the last 24 hours, the records fed today,
and the subscriptions in error. Below them: a `line_chart` of queue depth
over 30 days and an `activity_chart` of feeds per hour by channel (FHIR,
v2, PMIR), both in role tones; the listeners card (`definitions`): the HTTP
face, the MLLP listener (`status` pill, last message, senders seen) and the
feed sender's backlog; and the "Oldest in the queue" `table_card` with a
`card_action` to the queue. Empty: a quiet empty state on each card ("No
candidates waiting"). Loading: `skeleton` lines in each card. Error: a
`refusal` in place of the card that failed and the others drawn, so one
down indicator never blanks the page. Phone: one column, the stat cards as
compact tiles.

**Search** (`/search`). A `segmented_form` with two segments: *By
identifier* (the domain as a `combobox`, the value as a masked
`password`-style field that reveals on hold, posted to `POST
/operator/search/identifier`) and *By demographics* (family, given, birth
date, gender, postal code; posted to `POST /operator/search/demographics`,
which runs the scorer and returns graded candidates). The results are a
`data_table` with the person's leading name, birth date, gender, the
identifier chips per domain (masked, section 12.5), the score and grade
for a demographics search, and the row opens the patient. The table's
state is in the session. Empty: "No patient matches" with the two things
to try (fewer fields, `:exact` off). Loading: the table's skeleton. Error:
the `refusal` with the server's `OperationOutcome` issues. Keyboard: `/`
focuses the field, Tab between segments, Enter posts, `j`/`k` walk the
rows, Enter opens. Phone: the form stacks, the table stacks to cards.

**Patient** (`/patients/{person}`). Reads `GET /operator/persons/{id}`.
The page header carries the leading name, birth date and gender, the
person id as a `chip` with a `copy_button`, and the actions: *Merge into…*,
*Unmerge* (when the latest merge is revertible), *Resolve pseudonym*. Three
`keyed_tabs`:

- *Identifiers*: one row per domain (`data_table`): the domain's name, the
  identifier masked with its reveal, the record's state (`active`,
  `subsumed`, `removed`), when it was last fed, and the link's kind and
  who made it.
- *Records*: the source records side by side in the **record compare**
  (section 12.7, gap 1): one column per domain, one row per demographic
  field, each cell marked agree, differ or absent against the leading
  record, so a steward sees at a glance where the domains disagree.
- *History*: the **timeline** (gap 3) of every version, link, merge and
  steward action on this person, newest first, each entry with its actor,
  time, kind and reason, and a `disclosure` that opens the version's fields.
- *Pseudonyms*: the pseudonymisation domains and whether this person has a
  pseudonym in each; the value is shown masked and revealed as an
  identifier is.

Empty: a person with one record shows the compare with one column and a
sentence. Loading: the header draws first, the tabs skeleton. Error: a
`404` is the library's not-found page; a `403` the refusal naming the scope.
Keyboard: `1` to `4` select the tabs, `m` opens the merge, `?` the legend.
Phone: the compare becomes one record at a time with a segmented switch of
domains and the agree and differ marks kept.

**Review queue** (`/queue`). Reads `GET /operator/candidates` with the
filters (grade, domain, age, deferred). The **work-list** (gap 6): a dense
`data_table` in the compact density by default, one row per candidate: the
two leading names, birth dates, the domains, the score as a short bar with
its number, the grade pill, and the age; `j`/`k` move the focused row,
Enter opens, `o` opens in place as a side panel, `d` defers. The range
label says "1-50 of 1,204" and the filter is the library's `table_filter`.
Empty: "The queue is empty" with the last decision's time and a link to the
dashboard. Loading: skeleton rows. Error: the refusal. Phone: cards, one
per candidate, with the score bar and the grade, and the row actions in a
menu.

**Compare and decide** (`/queue/{candidate}`). Reads `GET
/operator/candidates/{id}`. The screen is the **record compare** (gap 1)
of the two records with every field in a row, each row marked agree,
differ or absent, in words and tone, the differing rows first; beside it,
the **score breakdown** (gap 2): one bar per field with the weight it
added or removed, the comparator's reading ("Jaro-Winkler 0.94"), the
total, and the thresholds drawn on the scale so the grade is visible; above
both, the two persons' other identifiers (masked) and the blocking key that
produced the pair. The decision bar is sticky at the foot: *Link* (`l`),
*Not a match* (`n`), *Defer* (`d`), each opening the reason field (`r`),
and *Next* (`]`) and *Previous* (`[`) through the queue in its order. A
decision posts `POST /operator/candidates/{id}/decide` with the versions
the screen read; a `409` (someone decided first, or a record changed) is a
`notice` with *Reload* and the decision is not repeated. A link that would
merge two persons says so in the confirmation: "Link these records? The two
patients become one; 3 identifiers move to the surviving patient", with
the effect listed, the library's `danger_dialog`. Empty: no candidate is a
`404`. Loading: the compare's skeleton. Error: the refusal. Phone: the
compare stacks to one record at a time with the marks; the breakdown
below; the decision bar stays sticky with the three actions.

**Merge and unmerge** (`/patients/{person}/merge`,
`/patients/{person}/unmerge`). The merge screen takes the other person
(from a search or a candidate), draws the **preview of the effect** before
anything is posted: `GET /operator/merges/preview` answers the full effect
and the screen shows it as a `change_table` (which identifiers move, which
pseudonyms now resolve to the survivor, which subscriptions will be told,
which candidates close) with the survivor chosen by a `choice_group`
(default: the older person) and the reason required. The confirm is the
library's `danger_dialog`, naming the count of identifiers that move;
`POST /operator/merges` carries the versions of both persons. The unmerge
screen shows the latest merge of this person with its effects and the
reason it was made, and the preview of the revert; a merge that is not the
latest says which must be reverted first. Every merge and unmerge is a
`steward_action` and a BALP record. Keyboard: Tab through the choice and
the reason, Enter confirms inside the dialog, Escape cancels and returns the
focus.

**Identifier domains** (`/domains`, `/domains/{domain}`). Reads `GET
/operator/domains`. A `data_table` of domains: name, system (as `<code>`,
wrapped), kind, sensitive, the Source's client id, records, last fed,
state. The domain page: its definitions, its feed activity chart, its last
20 feeds (time, channel, outcome, never a value), and, behind the admin
scope, the editor to retire it or change its Source, each change a
`steward_action`. A new domain is the library's editor form (`field`,
`check_field`, `help_line` behind the help switch). Empty: "No identifier
domains" with the seed instruction. Phone: cards.

**Pseudonymisation domains** (`/pseudonym-domains`). The same table shape
with the namespace and the count of pseudonyms; the editor behind the
admin scope; *Resolve a pseudonym* opens a form that takes a pseudonym and
a reason and answers the person, audited as a disclosure.

**Audit trail** (`/audit`). Reads `GET /operator/actions` with filters
(actor, kind, person, from, to). A `data_table` in the session, one row per
action: time (`timestamp`), actor, kind as a `badge`, the person (a link),
the reason, the request id. A row's `disclosure` shows the action's detail
(the candidate, the merge, the fields revealed). Export is a `POST` that
answers CSV for the filter, audited itself. Empty, loading and error as the
queue. Phone: cards.

**Your preferences** (`/preferences`, `/preferences/language`,
`/preferences/keyboard`). FerroUI's three tabs (Appearance, Language and
region, Keyboard), with the console's own rows, the reveal duration (10, 15
or 30 seconds, the server capping every reveal at 30) and hide on leave,
inside Appearance; each preference its own cookie, a plain form post drawn
on the server so the first paint is right (FerroUI family rule 9).

### 12.4 The operator API the console reads

`{base}/operator`, behind `[operator] enabled` and the steward scope, read
and decided through `ferropix-operator-api` types. Every search and every
lookup that carries an identifier is a `POST`; every list keeps its paging
in the body or the session; every answer is `no-store`.

| Route | Does |
|---|---|
| `GET /operator/overview` | the dashboard document |
| `GET /operator/dependencies` | the dependency report (section 9) |
| `POST /operator/search/identifier`, `POST /operator/search/demographics` | the two searches |
| `GET /operator/persons/{id}` | the person with its records, links, history and pseudonyms |
| `POST /operator/persons/{id}/reveal` | one identifier or pseudonym unmasked, with the reason; audited as a disclosure |
| `GET /operator/candidates`, `GET /operator/candidates/{id}` | the queue and one candidate with its evidence |
| `POST /operator/candidates/{id}/decide` | link, not-a-match or defer, with the versions and the reason |
| `GET /operator/merges/preview`, `POST /operator/merges`, `POST /operator/merges/{id}/revert` | the merge preview, the merge, the unmerge |
| `GET`, `POST`, `PATCH /operator/domains…` | the identifier domains (writes behind the admin scope) |
| `GET`, `POST /operator/pseudonym-domains…`, `POST /operator/pseudonyms/resolve` | the pseudonymisation domains and the resolutions |
| `GET /operator/sources`, `GET /operator/sources/{id}` | the systems allowed to feed, read from the configuration, with their domains and their last messages |
| `GET /operator/matching` | the thresholds and weights in effect |
| `GET /operator/configuration` | the matching configuration with each setting's origin, file mode |
| `GET /operator/actions`, `POST /operator/actions/export` | the audit trail and its export |

FerroPIX keeps no configuration store, so the console runs in file mode
(FerroUI 0.3.7 `design/COMPONENTS.md` rule 13): Configuration and every
Settings tab are read-only views of the file, each setting marked when the
environment set it or nothing did. A source's admission is configuration
(an issuer's `clients`, a Basic user, an `[[hl7v2.senders]]` entry), so the
sources are read from it too and are added or retired in the file, never
written through the API; the domains that name a source are read from the
domains. No specification governs this: our own design (#120, #121).

### 12.5 Design quality, stated

- **Every screen has its empty, loading and error state**, each named in
  section 12.3: an empty state says what the absence means and what to do;
  a loading state is the library's skeleton in the shape of the content;
  an error is the library's `refusal` with the server's issues, never a
  blank, and a `409` is a notice with a reload, never a silent retry.
- **Accessibility is WCAG 2.2 Level AA** in light and dark, the library's
  bar: 4.5:1 for text and 3:1 for control edges and the focus ring, held by
  `tests/it/contrast.rs` over the brand theme and by the painted battery of
  the journeys; no meaning rides on colour alone (agree and differ are
  words and marks, a grade is a word); every control is keyboard operable
  with a visible focus ring; the focus order follows the reading order and
  a dialog returns the focus to its trigger; every table has its caption,
  headers and `aria-sort`; every identifier chip, mask and reveal has its
  accessible name; the page reflows at 320 px with no sideways scroll (SC
  1.4.10); the sticky bars never cover the focus (SC 2.4.11); a live region
  announces a decision's outcome and a reveal's expiry (`role="status"`).
- **Keyboard-driven, with a legend.** The shortcuts of section 12.3 are
  single keys that act only when no field has the focus; `?` opens the
  **shortcut legend** (gap 5), a dialog listing every shortcut of the
  current screen with its `<kbd>` keys; every shortcut has a visible
  control that does the same thing.
- **Responsive at 1440 and 390 px**, light and dark: the compare and the
  breakdown sit side by side from 64 rem and stack below; the queue is a
  table above and cards below; the forms reflow to one column; each screen
  is captured at both widths in both themes by the journeys and the
  captures are read before a merge, the family's rule.
- **Density for a steward who works a queue all day:** the queue and the
  compare default to the compact density; the stat cards, the forms and
  the audit trail default to comfortable; the switch is in Your preferences and
  the choice is a cookie drawn on the server.
- **Identifiers are masked by default**, shown as the domain, a mask and
  the last three characters (`BSN ••••••123`), with a *Reveal* control that
  posts `POST /operator/persons/{id}/reveal` with a reason, shows the
  value for the configured duration with a visible countdown, and is
  recorded as a disclosure in the audit trail and in BALP; the reveal is a
  server round trip, never a client-side unmask, so a value the steward did
  not reveal never reaches the page. The search field's value is masked as
  typed and sent by `POST`. A copy copies the revealed value only.
- **Plain language, ready for i18n.** Every string the console draws is a
  message of `app/ferropix-console/locales/en/ferropix-console.ftl` (Fluent),
  written in plain words ("These two records look like the same patient",
  never "candidate pair grade probable"); a pseudo-locale journey stretches
  every string to catch truncation; a Dutch catalog is one file with no
  code change, and the library's own words follow the page's locale.
- **One thing is loudest per screen** (the lesson of FerroTERM #515):
  the compare on the decision screen, the queue on the queue, the
  identifiers on the patient. Help sits behind the help switch, never
  under every field; absence is a mark with a `title`, never a sentence
  in every cell; a name leads and an identifier follows it, quieter.

### 12.6 Wireframes

Low fidelity, the shell omitted after the first. `[ ]` is a control, `▒` a
masked identifier, `✓` agree, `≠` differ, `–` absent.

**Dashboard, 1440 px**

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ ◆ FerroPIX   [ / Find a patient: identifier, name or birth date… ]  ● Serving │
│                                                      ☾ theme   Sam Q ▾       │
├────────────┬─────────────────────────────────────────────────────────────────┤
│ WORK       │ Dashboard                                                        │
│ ▸ Dashboard│                                                                  │
│   Queue 38 │ ┌───────────┐ ┌───────────┐ ┌───────────┐ ┌───────────┐          │
│ PATIENTS   │ │ 38 waiting│ │ 92 % auto │ │ 1,204 fed │ │ 0 in error│          │
│   Search   │ │ 5 > 7 days│ │ linked 24h│ │ today     │ │ subscript.│          │
│ REGISTRY   │ └───────────┘ └───────────┘ └───────────┘ └───────────┘          │
│   Domains  │ ┌ Queue depth, 30 days ───────────┐ ┌ Feeds per hour ──────────┐ │
│   Pseudonym│ │        ╱╲    ╱╲                 │ │ ▁▂▃▅▇▅▃▂▁▂▃▅▇▆▃▂ FHIR    │ │
│   domains  │ │  ╱╲  ╱  ╲╱╲╱  ╲╱╲               │ │ ▁▁▂▂▃▃▂▂▁▁▂▂▃▃▂▂ v2      │ │
│   Matching │ └─────────────────────────────────┘ └──────────────────────────┘ │
│ TRAIL      │ ┌ Listeners ──────────────┐ ┌ Oldest in the queue ──── All ▸ ─┐ │
│   Audit    │ │ HTTP  ● up  2 ms p50    │ │ Mohr, Alice  1958  probable 12d │ │
│ CONSOLE    │ │ MLLP  ● up  last 09:41  │ │ Maiden, A.   1958  possible 11d │ │
│   Settings │ │ Feeds ● 0 queued        │ │ Jansen, P.   1971  probable  9d │ │
│   About    │ └─────────────────────────┘ └─────────────────────────────────┘ │
└────────────┴─────────────────────────────────────────────────────────────────┘
```

**Dashboard, 390 px**

```text
┌──────────────────────────┐
│ ☰  ◆ FerroPIX     Sam Q ▾│
│ [ / Find a patient…     ]│
├──────────────────────────┤
│ Dashboard                │
│ ▣ 38 waiting  5 > 7 days │
│ ▣ 92 % auto-linked, 24 h │
│ ▣ 1,204 fed today        │
│ ▣ 0 subscriptions in err │
│ ┌ Queue depth, 30 days ┐ │
│ │  ╱╲  ╱╲╱╲            │ │
│ └──────────────────────┘ │
│ ┌ Listeners ───────────┐ │
│ │ HTTP ● up            │ │
│ │ MLLP ● up  09:41     │ │
│ └──────────────────────┘ │
│ ┌ Oldest in queue  All ▸│ │
│ │ Mohr, Alice   12 d   │ │
│ └──────────────────────┘ │
└──────────────────────────┘
```

**Search, 1440 px**

```text
│ Search                                                                       │
│ ( By identifier ) ( By demographics )                                        │
│ Domain [ BSN ▾ ]   Identifier [ ▒▒▒▒▒▒▒▒▒ ]  [ Search ]                      │
│                                                                              │
│ 3 patients                                                     1-3 of 3      │
│ Name            Born        Identifiers                      Score  Grade    │
│ Mohr, Alice     1958-01-30  BSN ▒▒▒▒▒▒123  MRN ▒▒▒▒994        –      –       │
│ Mohr, Alissa    1958-01-30  MRN ▒▒▒▒m94                       –      –       │
│ Maiden, Alice   1958-01-30  LAB ▒▒▒▒▒550                      –      –       │
```

**Patient, 1440 px**

```text
│ ‹ Search                                                                     │
│ Mohr, Alice   1958-01-30   female            person 0199-…-7c3e [copy]       │
│                                   [ Merge into… ] [ Unmerge ] [ Pseudonym ]  │
│ ( Identifiers ) ( Records ) ( History ) ( Pseudonyms )                       │
│ ┌ Records, side by side ─────────────────────────────────────────────────┐   │
│ │ Field        │ BSN (leading)    │ Hospital MRN     │ Laboratory         │   │
│ │ Identifier   │ ▒▒▒▒▒▒123 Reveal │ ▒▒▒▒994 Reveal   │ ▒▒▒▒▒550 Reveal    │   │
│ │ Family name  │ Mohr             │ Mohr          ✓  │ Maiden          ≠  │   │
│ │ Given        │ Alice            │ Alice         ✓  │ Alice           ✓  │   │
│ │ Birth date   │ 1958-01-30       │ 1958-01-30    ✓  │ 1958-01-30      ✓  │   │
│ │ Gender       │ female           │ female        ✓  │ –               –  │   │
│ │ Postal code  │ 9999 XX          │ 9999 XX       ✓  │ 9998 XY         ≠  │   │
│ │ Fed          │ 2026-10-20 09:41 │ 2026-10-19       │ 2026-09-02         │   │
│ │ Linked by    │ feed             │ deterministic    │ steward: S. Q.     │   │
│ └────────────────────────────────────────────────────────────────────────┘   │
```

**Patient, 390 px (Records tab)**

```text
│ ‹ Mohr, Alice  1958  ♀   │
│ [Merge][Unmerge][Pseud.] │
│ (Ids)(Records)(Hist)(Ps) │
│ Compare with: [ BSN ▾ ]  │
│ Showing: ( MRN )( LAB )  │
│ ┌ Laboratory ──────────┐ │
│ │ Identifier ▒▒▒550 [R]│ │
│ │ Family   Maiden    ≠ │ │
│ │ Given    Alice     ✓ │ │
│ │ Born     1958-01-30✓ │ │
│ │ Gender   –         – │ │
│ │ Postal   9998 XY   ≠ │ │
│ └──────────────────────┘ │
```

**Review queue, 1440 px**

```text
│ Review queue                      [ grade ▾ ] [ domain ▾ ] [ filter rows… ] │
│ 38 candidates                                              1-50 of 38       │
│   A                 B                 Domains    Score      Grade     Age   │
│ ▸ Mohr, Alice 1958  Maiden, Alice 58  BSN · LAB  ████▌ 0.91 probable  12 d  │
│   Jansen, Piet 1971 Janssen, P. 1971  MRN · MRN  ███▌  0.84 probable   9 d  │
│   De Vries, A. 1990 Vries, Anna 1990  MRN · LAB  ██▌   0.67 possible   3 d  │
│ j/k move · Enter open · o side panel · d defer · ? shortcuts                │
```

**Compare and decide, 1440 px**

```text
│ ‹ Queue  ·  candidate 3 of 38                              [ ‹ ] [ › ]      │
│ Mohr, Alice (BSN)  vs  Maiden, Alice (Laboratory)        blocked on 1958+MR │
│ ┌ Fields ────────────────────────────────┐ ┌ Score 0.91, probable ───────┐  │
│ │ Family name  Mohr        Maiden      ≠ │ │ Family   JW 0.71    ▬▬ -3.1 │  │
│ │ Postal code  9999 XX     9998 XY     ≠ │ │ Postal   prefix     ▬  -0.8 │  │
│ │ Given        Alice       Alice       ✓ │ │ Given    exact    ▬▬▬▬ +4.2 │  │
│ │ Birth date   1958-01-30  1958-01-30  ✓ │ │ Born     exact   ▬▬▬▬▬ +6.0 │  │
│ │ Gender       female      –           – │ │ Gender   absent          0  │  │
│ │ Telephone    …4421       …4421       ✓ │ │ Phone    exact    ▬▬▬▬ +3.9 │  │
│ │ Identifier   ▒▒▒123 [R]  ▒▒▒550 [R]    │ │ ───────────────────────────│  │
│ └────────────────────────────────────────┘ │ total +10.2 → 0.91          │  │
│                                            │ ├──possible──┼─probable─┼─certain│  │
│                                            └─────────────────────────────┘  │
├─────────────────────────────────────────────────────────────────────────────┤
│ Reason [ Maiden name; same phone and address history                     ]  │
│ [ Link (l) ]  [ Not a match (n) ]  [ Defer (d) ]                   Next (]) │
```

**Compare and decide, 390 px**

```text
│ ‹ 3 of 38          [‹][›]│
│ Mohr, Alice vs Maiden, A.│
│ ( A )( B )               │
│ ┌ Maiden, Alice (LAB) ─┐ │
│ │ Family  Maiden     ≠ │ │
│ │ Postal  9998 XY    ≠ │ │
│ │ Given   Alice      ✓ │ │
│ │ Born    1958-01-30 ✓ │ │
│ └──────────────────────┘ │
│ Score 0.91 · probable    │
│ Born    ▬▬▬▬▬ +6.0       │
│ Given   ▬▬▬▬  +4.2       │
│ Family  ▬▬    -3.1       │
│ …                        │
├──────────────────────────┤
│ Reason [              ]  │
│ [Link] [Not match] [Defer]│
```

**Merge preview, 1440 px**

```text
│ Merge Maiden, Alice into Mohr, Alice                                        │
│ Survivor  (•) Mohr, Alice, older, 3 identifiers  ( ) Maiden, Alice          │
│ ┌ What changes ─────────────────────────────────────────────────────────┐   │
│ │ Setting                  Before                    After              │   │
│ │ LAB ▒▒▒▒▒550             Maiden, Alice             Mohr, Alice        │   │
│ │ Pseudonym ferroehr-nl    person 0199-…-9a1f        resolves to …7c3e  │   │
│ │ Subscriptions told       –                         2 (ITI-93 merge)   │   │
│ │ Candidates closed        –                         1                  │   │
│ └───────────────────────────────────────────────────────────────────────┘   │
│ Reason [                                                                 ]  │
│ [ Cancel ]                                        [ Merge 1 identifier ▲ ]  │
│          ┌ Merge these patients? ───────────────────────────┐               │
│          │ ⚠ 1 identifier moves to Mohr, Alice. Reversible   │               │
│          │   from the patient's History until the next merge.│               │
│          │                     [ Cancel ]  [ Merge ]         │               │
│          └───────────────────────────────────────────────────┘               │
```

**Identifier domains, 1440 px**

```text
│ Identifier domains                                        [ New domain ]    │
│ Name           System                       Kind      Source        Fed     │
│ BSN            http://fhir.nl/…/bsn         authorit. brp-feed      09:41   │
│ Hospital MRN   urn:oid:1.3.6.1.4.1.21367…   local     his-prod      09:40   │
│ Laboratory     urn:oid:1.3.6.1.4.1.21367…   local     lab-lis       Sep 2   │
```

**Audit trail, 1440 px**

```text
│ Audit trail   [ actor ▾ ] [ kind ▾ ] [ from ] [ to ]            [ Export ]  │
│ Time              Actor    Kind       Patient         Reason                │
│ 10-21 09:42:10    S. Q.    link       Mohr, Alice     Maiden name; same …   │
│ 10-21 09:41:55    S. Q.    reveal     Mohr, Alice     Phone call with lab   │
│ 10-21 09:40:02    system   auto-link  Jansen, Piet    BSN agreed            │
```

### 12.7 FerroUI component inventory and gaps

Checked against `ferrohealth-ui` 0.2.1 on `main` (2026-10-21): every public
view and widget, the stylesheet's classes, and the gallery.

**What each screen draws with (existing components):**

| Screen | FerroUI components |
|---|---|
| Shell | `view::shell::{shell, topbar_with, lockup, drawer_button, footer_about, skip_link}`, `widget::nav::sidebar`, `view::search::search_box`, `view::status::pill`, `widget::theme::theme_toggle`, `view::menu::operator_menu`, `widget::toast::{provide_toasts, toast_region}` |
| Sign-in | `view::sign_in::{sign_in, Provider, Basic, Notice}`, `view::shell::centred_about` |
| Dashboard | `view::surface::{stat_card, stat_grid, card, table_card, card_action, definitions}`, `view::chart::{figures, activity_chart}`, `widget::chart::line_chart`, `view::status::{status, pill}`, `view::empty::{empty_quiet, skeleton}`, `view::refusal::refusal` |
| Search | `view::segmented::segmented_form`, `view::form::{Field, field, keyed_field, fieldset}`, `widget::combobox::combobox`, `table::view::{data_table, Column}`, `widget::pager::session_pager`, `view::surface::chip`, `view::empty::empty_state` |
| Patient | `view::surface::{page_heading, Header, Crumb, chip, definitions, disclosure}`, `widget::copy::copy_button`, `view::tabs::keyed_tabs`, `table::view::data_table`, `view::time::timestamp`, `view::button::{label, Variant}`, `view::dialog::{confirm_trigger, danger_dialog}` |
| Review queue | `table::view::{data_table, Column, Listing}`, `table::model::{Paging, Sort, StateSource}`, `widget::filter::table_filter`, `widget::pager::session_pager`, `view::status::pill`, `widget::side_panel::{side_panel, side_panel_trigger}`, `view::chart::proportion` (the short score bar) |
| Compare and decide | `view::layout::{main_aside, action_bar}`, `view::form::{Field, field}`, `view::notice::notice`, `view::dialog::danger_dialog`, `widget::dialog::modal_dialog`, `view::button::label` |
| Merge and unmerge | `view::change::{change_table, Change}`, `view::segmented::choice_group`, `view::form::field`, `view::dialog::danger_dialog`, `view::surface::well` |
| Domains, pseudonymisation domains | `table::view::data_table`, `view::surface::{page_heading, editor_heading, Mode, definitions, card}`, `view::form::{field, check_field, fieldset}`, `widget::help::{provide_help, help_switch, help_line}`, `view::chart::activity_chart` |
| Audit trail | `table::view::data_table`, `widget::pager::session_pager`, `view::status::badge`, `view::time::timestamp`, `view::surface::disclosure`, `view::form::field` (the filters) |
| Your preferences | `view::preferences::{preferences_page, preference_row, select_preference, switch_preference}` |
| About | `view::surface::{definitions, card}`, `view::code::code_panel` (the `CapabilityStatement`) |
| Every screen | `view::empty::{empty_state, skeleton, busy}`, `view::refusal::{refusal, fault}`, `i18n::{Locale, Message, provide_locale}` |

**Not a gap, after checking.** A confirm-with-preview dialog is composed
from what exists: `widget::dialog::modal_dialog` with `Width::Wide` and
`danger: true`, a `change_table` as its body, and the console's reason
field and confirming button as its actions; the record compare (gap 1) can
stand in the body for a merge preview. A queue's "next after deciding" is
the console's own navigation. A sticky decision bar is `action_bar` with
the console's sticky placement, which the stylesheet's layer already
allows for a card footer.

**The gaps**, each to be filed on FerroHEALTH/FerroUI and built there,
generic, in the gallery for every brand, light and dark, with its states
captured and its roles in the contrast test. The API shapes follow the
library's rules: `view` functions are plain functions returning a typed
view with no reactive props; a `widget` takes `Signal<T>` and reports
through a callback; no component writes a `style` attribute or a `<style>`
element (strict CSP, `style-src 'self'`); every word is a message of the
library's catalog or taken from the console as a string.

1. **Record compare** (`view::compare`). Several records side by side with
   per-field agree, differ and absent marks against a leading record.
   ```rust
   pub struct CompareField { pub key: String, pub label: Cow<'static, str> }
   pub struct CompareRecord { pub key: String, pub title: String, pub values: Vec<Option<AnyView>> }
   pub enum Verdict { Agree, Differ, Absent, Unmarked }
   pub struct Compare<'a> { pub caption: Cow<'static, str>, pub fields: &'a [CompareField],
       pub leading: &'a CompareRecord, pub others: &'a [CompareRecord],
       pub verdict: &'a dyn Fn(&str, usize) -> Verdict, pub differ_first: bool }
   pub fn compare(table: Compare<'_>) -> AnyView;
   pub fn compare_stacked(table: Compare<'_>, shown: usize) -> AnyView;
   ```
   Classes `fui-compare`, `fui-compare-leading`, `fui-compare-agree`,
   `fui-compare-differ`, `fui-compare-absent`; a `<table>` with the field
   as the row header, the mark as a word in visually hidden text and a
   glyph beside the value (`✓`, `≠`, `–`), the differ rows first when
   asked, the leading column sticky from 64 rem, and the stacked form one
   record at a time under a segmented switch below it. States: two to six
   records; a record with no value in a field; every field agreeing (the
   caption says so). Accessibility: `aria-sort` is not used (not a sort);
   each mark has `title` and hidden text; the tones are `--ok`, `--warn`
   and `--muted`, never colour alone.
2. **Score breakdown** (`view::score`). The contributions of the fields of
   a score, the total, and the thresholds on one scale.
   ```rust
   pub struct Contribution { pub label: String, pub reading: Option<String>, pub weight: f64 }
   pub struct Threshold { pub label: String, pub at: f64 }
   pub struct Score { pub caption: Cow<'static, str>, pub total: f64, pub score: f64,
       pub grade: String, pub contributions: Vec<Contribution>, pub thresholds: Vec<Threshold> }
   pub fn score_breakdown(score: &Score) -> AnyView;
   pub fn score_bar(score: f64, grade: &str) -> AnyView;
   ```
   Classes `fui-score`, `fui-score-row`, `fui-score-bar-add`,
   `fui-score-bar-remove`, `fui-score-scale`, `fui-score-mark`; the bars
   are SVG drawn in Rust in the `--ok` and `--bad` tones with the number
   beside each, the scale shows the thresholds as labelled marks and the
   score as a point, the compact `score_bar` is the queue's cell. States:
   a positive and a negative weight, a zero, a score at a threshold.
   Accessibility: the SVG carries `role="img"` and a sentence as its
   accessible name ("Score 0.91, probable: birth date +6.0, …"); numbers
   are in the `fui-num` face.
3. **Timeline** (`view::timeline`). Events in time order with an actor, a
   kind, a time and an optional body.
   ```rust
   pub struct Event { pub key: String, pub at: String, pub kind: String, pub tone: Tone,
       pub actor: Option<String>, pub title: String, pub body: Option<AnyView> }
   pub fn timeline(label: impl Into<Cow<'static, str>>, events: &[Event]) -> AnyView;
   ```
   Classes `fui-timeline`, `fui-timeline-event`, `fui-timeline-dot`,
   `fui-timeline-rail`; an `<ol>` with a rail, each event's dot in its
   tone with the kind as a `badge`, the time through `timestamp`, the body
   in a `disclosure`. States: one event; an event with no actor; fifty
   events (no cap in height). Accessibility: the list order is the reading
   order, the dot is decorative, the kind is a word.
4. **Masked identifier with reveal** (`widget::identifier`). A domain
   label, a masked value with its last characters, and a reveal control
   whose unmasking is the console's server round trip.
   ```rust
   pub struct Identifier { pub id: String, pub domain: String, pub masked: String,
       pub revealed: Signal<Option<String>>, pub expires_in_s: Signal<Option<u32>> }
   pub fn identifier(identifier: Identifier, on_reveal: Callback<()>) -> AnyView;
   pub fn identifier_static(domain: &str, masked: &str) -> AnyView;
   ```
   Classes `fui-identifier`, `fui-identifier-domain`, `fui-identifier-mask`,
   `fui-identifier-value`, `fui-identifier-reveal`, `fui-identifier-countdown`;
   the domain as a chip, the masked value in the code face with the mask
   glyphs, a small secondary *Reveal* button with the eye glyph, the
   revealed value with a countdown and a `copy_button` that is enabled
   only while revealed. The widget never holds the value before the
   console's callback supplies it, so a page source carries no identifier.
   States: masked; revealing (busy); revealed with the countdown; refused;
   the static form for a listing. Accessibility: the mask is announced as
   "masked, ends in 123"; the reveal is a toggle with `aria-pressed`; the
   countdown is a polite live region at 10 and 0 seconds.
5. **Keyboard shortcut legend** (`widget::shortcuts`, `view::kbd`). The
   `<kbd>` element and the legend dialog listing a screen's shortcuts.
   ```rust
   pub fn kbd(keys: &str) -> AnyView;
   pub struct Shortcut { pub keys: String, pub does: String }
   pub struct ShortcutGroup { pub title: String, pub shortcuts: Vec<Shortcut> }
   pub fn shortcut_legend(groups: Vec<ShortcutGroup>, open: Signal<bool>, on_dismiss: Callback<()>) -> AnyView;
   pub fn shortcut_hint(shortcuts: &[Shortcut]) -> AnyView;
   ```
   Classes `fui-kbd`, `fui-shortcuts`, `fui-shortcuts-group`,
   `fui-shortcut-hint`; the legend is a `modal_dialog` with a
   `definitions`-like grid of keys and words, opened by `?` through the
   console's listener, and the hint is the one-line foot of a listing. The
   library binds no key itself (the console owns its listeners); it draws
   the keys. States: open and closed; a group of one. Accessibility: `<kbd>`
   with the key name, the dialog returns the focus.
6. **Work-list** (`widget::worklist`). A dense listing with a focused row
   moved by the keyboard, a selection, and a sticky foot with the actions.
   ```rust
   pub struct WorkList<'a> { pub table: DataTable<'a>, pub focused: RwSignal<Option<String>>,
       pub on_open: Callback<String>, pub foot: AnyView }
   pub fn worklist(list: WorkList<'_>) -> AnyView;
   ```
   Classes `fui-worklist`, `fui-worklist-focused`, `fui-worklist-foot`; the
   table is `data_table` with a `data-key` per row, the focused row carries
   `aria-current="true"` and the sunken ground, `j`/`k` and the arrow keys
   move it while the table has the focus, Enter runs `on_open`, the foot is
   sticky below the rows inside the card and never covers the focused row.
   States: focus on the first, the last, a row scrolled out of view; the
   compact density. Accessibility: the table is `role="grid"` with the
   roving tabindex pattern (WAI-ARIA Authoring Practices, grid), one tab
   stop for the whole list.
7. **The FerroPIX brand** (`themes/ferropix.css`, `Brand::Pix`). The
   library's `Brand` enum and `themes/` hold four brands (Bridge, Fed, Ehr,
   Term) and no FerroPIX. The Plum & Iron theme sets the five custom
   properties of a brand file in its three blocks: light `--accent:
   #6B21A8` (`--ferropix-plum` of `assets/brand/tokens.css`), dark
   `--accent: #C084FC` (`--ferropix-plum-light`), with `--accent-hover`,
   `--accent-ink`, `--accent-tint` and `--focus` derived as the other
   brands derive theirs; `Brand::Pix` joins the enum, `Brand::theme` and
   the gallery's brand switch, every role pairing of the brand joins
   `tests/it/contrast.rs` at WCAG 2.2 AA, and the gallery's screenshots
   and state captures are taken in the brand, light and dark. The console
   then loads the library's theme as its second stylesheet and keeps no
   theme file of its own.

A FerroPIX console issue that draws one of these is blocked by its FerroUI
issue, written as a sentence in the FerroPIX issue body, since a cross-
repository edge is no native relationship.

## 13. Workspace layout

| Member | Holds | May depend on (workspace) |
|---|---|---|
| `crates/ihe-iti` | the identity profiles, client and server sides by feature | |
| `crates/nl-generic-functions` | the Dutch functions | `ihe-iti` is not needed |
| `app/ferropix-operator-api` | the serde types of the health and operator documents | |
| `app/ferropix-server` | the server: configuration, store, matching, the three faces, audit, health, the operator API | `ihe-iti`, `nl-generic-functions`, `ferropix-operator-api` |
| `app/ferropix-console` | the console | `ferropix-operator-api` only |
| `tools/ferropix-testkit` | the PostgreSQL and stub-provider containers behind `FERROPIX_E2E`, the MLLP sender, the synthetic fixtures (no real identifier, ever), the browser journeys behind `FERROPIX_JOURNEYS` and the screenshots behind `FERROPIX_SCREENSHOTS` | path-only dev-dependency |
| `fuzz/` | the fuzz targets of section 11 | |

`ferropix-*` members are `publish = false`; `ihe-iti` and
`nl-generic-functions` keep their own version lines
(`.claude/rules/crates-publishing.md`).

## 14. Build order

The milestones carry the order; every issue cites the section it builds.

- **v0.0.3, the server on PostgreSQL with PIXm and PDQm:** the workspace
  members of section 13 (the server, the operator-api crate, the testkit),
  the configuration tree (section 3), the store and its migrations
  (section 4), the deterministic match and the scorer (sections 5.1, 5.2,
  with the candidates recorded and no review surface yet), the PIXm
  Manager by `GET` and `POST` with ITI-104 (#52, section 6.1), the PDQm
  Supplier (section 6.2), authorization, audit, health, the request log
  and the usage report (section 9), the server image, the compose
  quickstart, the SLSA release lanes (section 10) and the five CI lanes
  (section 11).
- **v0.0.4, HL7 v2 and the console:** the ITI TF-2 texts vendored, the
  MLLP listener with ITI-8, ITI-9 and ITI-21 (section 7), the console with
  its sign-in, shell, dashboard, search, patient record, domains, audit
  trail and settings (section 12), the FerroUI components it needs
  (section 12.7), its image and release lane, the console and journeys CI
  lanes, and the Helm chart.
- **v0.0.5, PMIR, review and pseudonyms:** the PMIR Registry with the feed
  sender and ITI-94 (section 6.3), the review queue, the compare and decide
  screen, merge and unmerge with their previews and the steward decisions
  behind them (sections 4.3, 5.3, 12.3), `auto_link`, the pseudonymisation
  domains and the pseudonyms (section 8), ITI-10.

## 15. Decision register

| # | Decision | Chosen | Ground |
|---|---|---|---|
| D1 | The store | PostgreSQL 18, one schema `pix`, write-once versions, a temporal link table | owner 2026-10-21; FerroEHR's storage discipline; no specification governs it |
| D2 | What a `Patient` is | the person (the cross-reference), with every linked identifier | PIXm 3.1.0 §2:3.104.4.1.3 leaves it open; one resource serves ITI-83 `targetId`, ITI-78, ITI-119 and PMIR |
| D3 | A merged identifier in ITI-83 | answered with the survivor's identifiers | PIXm 3.1.0 §2:3.83.4.2.2.5 defers to policy; §2:3.104.4.2.3 replaces the subsumed identifier's references |
| D4 | A removed identifier | `not-found` everywhere; the versions stay | PIXm 3.1.0 §2:3.104.4.3.3; the audit trail needs the history |
| D5 | Unmerge | a steward may, through the console; a feed may not (`405`) | PMIR 1.6.0 §2:3.93.4.1.3 admits a local policy |
| D6 | Automatic probabilistic linking | off by default; deterministic linking always on | no specification governs it; a wrong link is a clinical risk, a queued pair is not |
| D7 | Identifier values at rest | sealed (AES-256-GCM) with an HMAC for equality; demographics plain | no specification governs it; ITI-78 string search needs the demographics, equality is all the identifier lookups need |
| D8 | Where the console's audit trail reads | `steward_action`, the server's own table; BALP sent as well | the ATNA repository is the deployment's and may be unreadable by the console |
| D9 | A person id in a console URL | allowed; a business identifier never | FerroEHR's `ehr_id` precedent; FerroUI's rule names patient identifiers, and a `uuidv7` surrogate identifies no one outside FerroPIX |
| D10 | Reveal | a server round trip with a reason, audited as a disclosure, timed | no specification governs it; BALP 1.1.4 §1:52.4.1.6 wants the patient as the subject of a disclosure record |
| D11 | The v2 parser | FerroBRIDGE's crates, pinned exactly; the TF-2 texts vendored before the first v2 issue | `CLAUDE.md` §Scope; `.claude/rules/spec-adherence.md` |
| D12 | Helm | a chart after the images, as FerroEHR ships one | owner 2026-10-21: Helm only if a sibling ships one |
| D13 | FerroUI gaps | six components and the FerroPIX brand theme, filed on FerroUI, built there; the confirm-with-preview composed from existing parts | owner 2026-10-21; `.claude/memory/ferroui-lockstep.md` |
| D14 | The identifier search field | masked as typed, posted, never in a URL | #52; owner 2026-10-21 |
| D15 | A start under another sealing key | refused, with the key id of every row and of the store recorded; `db reseal` resumable and refusing every start until done | no specification governs it; a wrong key would answer every identifier as unknown, a silent wrong answer |
| D16 | The store's errors | the SQLSTATE, the constraint and the message of a refusal; never PostgreSQL's detail, which quotes the row | no specification governs it; PostgreSQL fills the detail of a refusal with the values of the offending row |
| D17 | The score's midpoint | a fixed 10 bits (prior odds of one in 1,024), never derived from a deployment's thresholds or weights | no specification governs it; PDQm 3.2.0 §2:3.119.4.2.2.4 asks for a score in `[0, 1]`, and a fixed midpoint keeps a score comparable across deployments |
| D18 | A duplicate resolution naming a survivor the store lacks | refused; a subsumed identifier the store lacks is inserted and subsumed | no specification governs it; PIXm 3.1.0 §2:3.104.4.2.3 replaces references to the subsumed identifier with the survivor's, which needs a survivor to point at |
| D19 | An identifier value before it is sealed | normalised by its domain's rule: a BSN to nine digits passing the eleven-proof, any other trimmed with its case kept; a refused value refuses the feed | no specification governs it; section 5.1 links on the same normalised value, and FHIR R4 compares `Identifier.value` as written |
| D20 | The identifiers a version carries | rows of `carried_identifier`, found by HMAC and sealed as a record's own; never in the demographics | no specification governs it; a BSN in plain `jsonb` would defeat the sealing of section 4.2 |
| D21 | The demographics a `Patient` shows | the current version of an `authoritative` domain's record first, then the most recently received; no ranking key | no specification governs it; the national number's Source is the best-vetted, and a rule without a key cannot be misconfigured |
| D22 | ITI-78 paging | a cursor of the last person id in the `next` link, 20 a page by default, at most 100 | FHIR R4 leaves paging links to the server (http.html, paging); a cursor keeps every person on exactly one page while the set changes, an offset does not |
| D23 | Who ITI-119 returns | each person once, at the best score of its records, `certainly-not` left out | PDQm 3.2.0 §2:3.119.4.1.3 leaves the matching to the Supplier; a pair below `possible_at` is no match by the scorer's own grades |
| D24 | The PDQm audit record | written before the answer, for refusals too; `503` when it cannot be stored | BALP 1.1.4 §1:52.4.1.6 wants one record per patient; an answer whose disclosure is not recorded is withheld, as the spool already refuses an unaudited transaction |
| D25 | The operator API's paging | the domains by a cursor of the last system, the trail by a cursor of the last action id, the two searches by an offset; at most 200, 200 and 50 a page, an export at most 10,000 actions with the cursor of the rest in a header | no specification governs it; a search's persons are ranked by score, so an offset is their only stable order, and the other lists keep a cursor as ITI-78 does (D22) |
| D26 | What an operator read records | a person view a `view` steward action and a BALP Patient Read record; a search one BALP Patient Query record per person found, or a Query record, and no steward action; an export an `export` steward action and an Export record; the dashboard, the domains, the matching configuration and the trail nothing; each written before the answer, a request refused before it runs writing none | no specification governs it; BALP 1.1.4 §1:52.4.1.6 wants a record per patient for an access, and a steward action per search would bury the trail the console reads (D8) |
| D27 | An identifier search over an identifier two persons carry | every person listed, each masked | no specification governs it; ITI-83 answers the ambiguity alone, and the steward is the one who resolves it |
| D28 | The dashboard's counts | a record linked in 24 hours is one created in that time that the scorer linked alone, with a `probabilistic` link opened in that time (#112); "today" is the UTC day; the queue depth is the open candidates at the end of each UTC day | no specification governs it; the console draws a rate and two series from counts the store holds without a log |
| D29 | The trail export | `text/csv` (RFC 4180), a cell a spreadsheet would read as a formula prefixed with `'` | no specification governs it; a steward's reason is free text and the export is opened in a spreadsheet |
| D30 | The reveal's record | the `reveal` steward action and the BALP Privacy Disclosure record in one transaction committed only once the record is stored, the value answered after the commit; the action and the record name the domain, never the value; the steward's reason a `reason` detail of the disclosed entity, `purposeOfEvent` left to the token's coded purposes; 15 seconds shown by default, 30 at most | BALP 1.1.4 §3:5.7.7 and its `PrivacyDisclosure.Source` profile fix the event, the agents and the patient; where a free-text reason goes, and the order of the writes, no specification governs: our own design (D10) |
| D31 | The domain writes | an administrator adds a `local` or `authoritative` domain whose Source an issuer lists, renames it, gives it another Source or retires it; a retired domain stays retired; the guard takes the store's Sources at the commit | PIXm 3.1.0 §2:3.104.5 has the Manager check the Source of every change; the master domain is the server's own, and an unretire would revive identifiers PIXm already answers as unknown (§2:3.83.4.2.2.3) |
| D32 | The XCPD Responding Gateway | synchronous ITI-55 over the cross-reference and the scorer; the answering domains and the initiating communities listed in `[xcpd]`; no bearer token on the SOAP endpoint | ITI TF-2 §3.55.4.1.3.1 leaves matching to the gateway; ITI TF-1 Table 27.1.3-1 groups the gateways with ATNA, whose mutual TLS the deployment terminates; which domains answer and which communities are served no specification governs: our own design |
