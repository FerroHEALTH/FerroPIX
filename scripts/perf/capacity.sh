#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# The capacity campaign of the FerroPIX server (#740): how far each
# transaction group goes over a store of a million persons, toward the
# targets of #712. The perf gate (scripts/perf/gate.sh) compares a change
# with its base at a fixed rate; this steps one group at a time up through
# the rates you give it, open loop, so a stalled server raises the latency of
# every request it held back.
#
#   scripts/perf/capacity.sh up
#       Starts PostgreSQL 19 on the volume ferropix-cap-pg, the stub OpenID
#       Provider and the terminology server, each at the test image pins of
#       docs/VERSIONS.md, and the release server of this checkout
#       (target/release/ferropix) on 8080 and MLLP 2575 with
#       perf/ferropix.toml, its pool sized by POOL (32).
#   scripts/perf/capacity.sh seed N
#       Feeds N synthetic patients over ITI-104, INFLIGHT (64) at once, in
#       chunks of 10000; FROM=k resumes at patient k.
#   scripts/perf/capacity.sh patients N
#       Writes the first N seeded patients, as the seed generated them, to
#       WORK/patients.jsonl, the data file the feed and mixed plans draw a
#       patient from for each arrival (ferrohealth-perf variables), so each
#       feed revises a record of its own; N is at most the patients seeded.
#   scripts/perf/capacity.sh snapshot | restore
#       Stops the server and PostgreSQL and exports the volume to SNAP
#       (target/capacity/pg.tar), or imports SNAP into a fresh volume.
#   scripts/perf/capacity.sh run GROUP RATE
#       Runs perf/capacity/GROUP.toml at RATE arrivals a second, its report
#       under target/capacity/run-GROUP-RATE/, the server's CPU and RSS
#       sampled with ps once a second beside it and PostgreSQL's with
#       `stats`, and the host's uptime before and after.
#   scripts/perf/capacity.sh down
#       Stops the server and removes every container and volume it made.
#   scripts/perf/capacity.sh --self-test
#       Proves the patient generator, the plan rendering and the pin reader
#       offline.
#
# Needs curl, jq, openssl, awk, ferrohealth-perf at the pin of
# docs/VERSIONS.md and a container engine (CONTAINER, podman by default; on
# macOS set DOCKER_HOST to its socket only for tools that need it). Every
# name, identifier, birth date and postal code it seeds is generated and
# synthetic: identifiers under the OID arc 2.999 (ITU-T X.660, examples),
# names built from syllables in the style of the testkit's synthetic names.
# No specification governs this: our own design.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
readonly CONTAINER="${CONTAINER:-podman}"
readonly WORK="${WORK:-$root/target/capacity}"
readonly SNAP="${SNAP:-$WORK/pg.tar}"
readonly PG_NAME=ferropix-cap-postgres
readonly ISSUER_NAME=ferropix-cap-issuer
readonly TX_NAME=ferropix-cap-terminology
readonly VOLUME=ferropix-cap-pg
readonly PG_PASSWORD=perf
readonly ISSUER=http://localhost:8180/ferropix
readonly URL=http://127.0.0.1:8080

die() {
  echo "capacity: $*" >&2
  exit 2
}

# pin ITEM [MATRIX]: the pin of the docs/VERSIONS.md row whose first cell is
# ITEM, backticks off.
pin() {
  local item="$1" matrix="${2:-$root/docs/VERSIONS.md}"
  awk -F'|' -v item="$item" '{ gsub(/^[[:space:]]+|[[:space:]]+$/, "", $2) }
    $2 == item { v = $3; gsub(/[[:space:]`]/, "", v); print v; exit }' "$matrix"
}

# token CLIENT SCOPE: a client credentials token of the stub provider.
token() {
  curl --fail --silent --show-error -d grant_type=client_credentials \
    -d "client_id=$1" -d client_secret=perf -d "scope=$2" "$ISSUER/token" | jq -er .access_token
}

wait_for() {
  local what="$1" tries="$2"
  shift 2
  for _ in $(seq 1 "$tries"); do
    "$@" > /dev/null 2>&1 && return 0
    sleep 1
  done
  die "$what did not come up"
}

serve() {
  mkdir -p "$WORK/audit"
  chmod 0700 "$WORK" "$WORK/audit"
  printf 'postgresql://ferropix:%s@127.0.0.1:5432/ferropix' "$PG_PASSWORD" > "$WORK/db-url"
  [[ -f "$WORK/data-key" ]] || openssl rand -base64 32 > "$WORK/data-key"
  [[ -x "$root/target/release/ferropix" ]] || die "target/release/ferropix is not built"
  env FERROPIX__TERMINOLOGY__ENDPOINT__URL=http://127.0.0.1:8280/r4 \
    FERROPIX__USAGE_REPORT__ENABLED=false \
    FERROPIX__DB__URL_FILE="$WORK/db-url" FERROPIX__DB__DATA_KEY_FILE="$WORK/data-key" \
    FERROPIX__DB__POOL_SIZE="${POOL:-32}" \
    FERROPIX__AUDIT__SPOOL_WRITE_TIMEOUT="${SPOOL_TIMEOUT:-2s}" \
    FERROPIX__AUDIT__SPOOL_MAX_SIZE="${SPOOL_MAX_SIZE:-1GiB}" FERROPIX__AUDIT__SPOOL_MAX_EVENTS="${SPOOL_MAX_EVENTS:-1000000}" \
    FERROPIX__AUDIT__SPOOL_DIR="$WORK/audit" \
    "$root/target/release/ferropix" serve --config "$root/perf/ferropix.toml" \
    > "$WORK/server.log" 2>&1 &
  echo "$!" > "$WORK/server.pid"
  wait_for "the server" 300 curl --fail --silent "$URL/health/readiness"
}

# packages DIR: the terminology server's packages, as the perf gate fetches
# them (its PACKAGES list and pins), each checked against its sha256.
packages() {
  local dir="$1" list name url sha file
  list="$(sed -n '/^readonly FERROBRIDGE_VALUES=/p' "$root/scripts/perf/gate.sh")"
  eval "$list"
  list="$(awk '/^readonly PACKAGES="/ { on = 1; sub(/^readonly PACKAGES="/, "") }
    on { line = $0; if (sub(/"$/, "", line)) { print line; exit } print line }' "$root/scripts/perf/gate.sh")"
  while read -r name url sha; do
    url="${url//\$\{FERROBRIDGE_VALUES\}/$FERROBRIDGE_VALUES}"
    file="$(mktemp)"
    curl --fail --silent --show-error --location --output "$file" "$url" || die "$url could not be fetched"
    [[ "$(openssl dgst -sha256 -r "$file" | cut -d' ' -f1)" == "$sha" ]] || die "$url is not the pinned bytes"
    mkdir -p "$dir/$name"
    case "$url" in
      *.json) mv "$file" "$dir/$name/${url##*/}" ;;
      *) tar -xzf "$file" -C "$dir/$name" && rm -- "$file" ;;
    esac
  done <<< "$list"
  chmod -R a+rX "$dir"
}

up() {
  local pg issuer tx
  mkdir -p "$WORK"
  chmod 0700 "$WORK"
  pg="$(pin "PostgreSQL test image")"
  issuer="$(pin "Stub OpenID Provider image")"
  tx="$(pin "Quickstart terminology server image")"
  [[ -n "$pg" && -n "$issuer" && -n "$tx" ]] || die "docs/VERSIONS.md pins no test image"
  "$CONTAINER" volume inspect "$VOLUME" > /dev/null 2>&1 || "$CONTAINER" volume create "$VOLUME" > /dev/null
  "$CONTAINER" run --detach --name "$PG_NAME" --publish 127.0.0.1:5432:5432 \
    --volume "$VOLUME:/var/lib/postgresql" \
    --env POSTGRES_PASSWORD="$PG_PASSWORD" --env POSTGRES_DB=ferropix "$pg" \
    -c shared_buffers=2GB -c max_connections=200 \
    -c shared_preload_libraries=pg_stat_statements > /dev/null
  wait_for PostgreSQL 90 "$CONTAINER" exec "$PG_NAME" pg_isready --username postgres --dbname ferropix --host 127.0.0.1
  "$CONTAINER" exec "$PG_NAME" psql --quiet --username postgres --dbname ferropix \
    --command "DO \$\$ BEGIN CREATE ROLE ferropix LOGIN PASSWORD '${PG_PASSWORD}'; EXCEPTION WHEN duplicate_object THEN NULL; END \$\$" \
    --command "GRANT CREATE ON DATABASE ferropix TO ferropix" > /dev/null
  # The client credentials mapping of the issuer service of compose.yaml.
  # shellcheck disable=SC2016 # ${clientId} and ${scope} are the provider's own templates
  "$CONTAINER" run --detach --name "$ISSUER_NAME" --publish 127.0.0.1:8180:8180 --env SERVER_PORT=8180 \
    --env JSON_CONFIG='{"interactiveLogin":false,"tokenCallbacks":[{"issuerId":"ferropix","requestMappings":[{"requestParam":"grant_type","match":"client_credentials","claims":{"sub":"${clientId}","client_id":"${clientId}","aud":["http://localhost:8080/fhir"],"scope":"${scope}"}}]}]}' \
    "$issuer" > /dev/null
  wait_for "the OpenID Provider" 60 curl --fail --silent "$ISSUER/.well-known/openid-configuration"
  [[ -d "$WORK/terminology" ]] || packages "$WORK/terminology"
  "$CONTAINER" run --detach --name "$TX_NAME" --publish 127.0.0.1:8280:8280 \
    --volume "$WORK/terminology:/data/packages:ro" \
    --env FERROTERM__SERVER__LISTEN=0.0.0.0:8280 \
    --env FERROTERM__TERMINOLOGY__CODE_SYSTEMS=/data/packages/hl7.terminology/package,/data/packages/hl7.fhir.uv.v2mappings/package,/data/packages/ferrobridge-hl7v2-values \
    --env FERROTERM__USAGE_REPORT__ENABLED=false "$tx" > /dev/null
  wait_for "the terminology server" 180 curl --fail --silent http://127.0.0.1:8280/health/readiness
  serve
}

# The synthetic patient of number i, as awk: patient(i) sets P_ID, P_FAMILY,
# P_GIVEN, P_GENDER, P_BIRTH, P_LINE and P_POSTAL: a name from 16 x 16 x 8
# syllables and 26 given names, a birth date over 90 years, a postal code of
# 9000 numbers and 441 letter pairs. The seed and the capacity plans' data
# file both take it from here, so a feed revises a record with the
# demographics it already holds.
readonly PATIENT_AWK='
function init() {
  split("Proef Test Voor Fict Ver Steek Model Schets Oefen Proto Monster Staal Bedacht Gefingeerd Verzon Droom", F, " ")
  split("sma erink beeldt huis zinga proef ma ink stra veld berg hof dijk kamp laar wijk", M, " ")
  split("- en er s ing a o e", L, " ")
  split("Ada Bram Cato Daan Elin Floor Gijs Hanna Ilse Joost Kiki Lars Mira Noud Olga Piet Quinta Ruud Saar Teun Uma Vera Wout Xena Yara Zeno", G, " ")
  split("A B C D E F G H J K L M N P R S T V W X Z", P, " ")
  split("an el ik om ur is ad ef og uk ber dor fen gal hum jor", S, " ")
}
function patient(i,    h, k, last, year, month, day) {
  # Two multiplicative hashes of i, so each attribute varies on its own.
  h = (i * 2654435761) % 4294967296
  k = (i * 40503 + 7919) % 1048576
  last = L[int(h / 4096) % 8 + 1]; if (last == "-") last = ""
  P_ID = sprintf("C-%07d", i)
  P_FAMILY = F[h % 16 + 1] S[int(h / 16) % 16 + 1] M[int(h / 256) % 16 + 1] last
  P_GIVEN = G[int(h / 32768) % 26 + 1]
  year = 1930 + int(h / 851968) % 90; month = 1 + k % 12; day = 1 + int(k / 12) % 28
  P_BIRTH = sprintf("%04d-%02d-%02d", year, month, day)
  P_POSTAL = sprintf("%04d %s%s", 1000 + int(k / 336) % 9000, P[int(h / 65536) % 21 + 1], P[int(k / 64) % 21 + 1])
  P_GENDER = (i % 2 == 0) ? "female" : "male"
  P_LINE = sprintf("Synthetische straat %d", i % 300)
}'

# chunk START END TOKEN BASE: the curl config of the ITI-104 feeds of the
# synthetic patients START to END-1 at BASE.
chunk() {
  awk -v start="$1" -v end="$2" -v token="$3" -v url="$4" "$PATIENT_AWK"'
  BEGIN {
    init()
    for (i = start; i < end; i++) {
      patient(i)
      body = sprintf("{\\\"resourceType\\\":\\\"Patient\\\",\\\"identifier\\\":[{\\\"system\\\":\\\"urn:oid:2.999.1.1\\\",\\\"value\\\":\\\"%s\\\"}],\\\"active\\\":true,\\\"name\\\":[{\\\"use\\\":\\\"official\\\",\\\"family\\\":\\\"%s\\\",\\\"given\\\":[\\\"%s\\\"]}],\\\"gender\\\":\\\"%s\\\",\\\"birthDate\\\":\\\"%s\\\",\\\"address\\\":[{\\\"use\\\":\\\"home\\\",\\\"line\\\":[\\\"%s\\\"],\\\"city\\\":\\\"Teststad\\\",\\\"postalCode\\\":\\\"%s\\\",\\\"country\\\":\\\"NL\\\"}]}", P_ID, P_FAMILY, P_GIVEN, P_GENDER, P_BIRTH, P_LINE, P_POSTAL)
      if (i > start) printf "next\n"
      printf "url = \"%s/fhir/Patient?identifier=urn%%3Aoid%%3A2.999.1.1%%7C%s\"\n", url, P_ID
      printf "request = PUT\nheader = \"Authorization: Bearer %s\"\nheader = \"Content-Type: application/fhir+json\"\n", token
      printf "data = \"%s\"\noutput = /dev/null\nwrite-out = \"%%{http_code}\\n\"\n", body
    }
  }'
}

# rows N: the JSON Lines rows of the synthetic patients 0 to N-1, one object
# a patient, the data file of the capacity plans' `patient` variable.
rows() {
  awk -v n="$1" "$PATIENT_AWK"'
  BEGIN {
    init()
    for (i = 0; i < n; i++) {
      patient(i)
      printf "{\"id\":\"%s\",\"family\":\"%s\",\"given\":\"%s\",\"gender\":\"%s\",\"birthDate\":\"%s\",\"line\":\"%s\",\"postalCode\":\"%s\"}\n", P_ID, P_FAMILY, P_GIVEN, P_GENDER, P_BIRTH, P_LINE, P_POSTAL
    }
  }'
}

# patients N: writes the data file of the first N seeded patients.
patients() {
  mkdir -p "$WORK"
  rows "$1" > "$WORK/patients.jsonl"
  echo "patients: $WORK/patients.jsonl ($1 rows)"
}

seed() {
  local n="$1" from="${FROM:-0}" step=10000 source start end bad
  mkdir -p "$WORK/seed"
  for ((start = from; start < n; start += step)); do
    end=$((start + step < n ? start + step : n))
    # The stub provider's tokens live an hour; each chunk takes a fresh one.
    source="$(token perf-source system/Patient.cud)"
    chunk "$start" "$end" "$source" "$URL" > "$WORK/seed/chunk.cfg"
    curl --silent --parallel --parallel-max "${INFLIGHT:-64}" --config "$WORK/seed/chunk.cfg" > "$WORK/seed/codes" || true
    bad="$(grep -cvE '^(200|201)$' "$WORK/seed/codes" || true)"
    [[ "$bad" == 0 ]] || die "chunk $start..$end: $bad feeds not answered 200 or 201 (resume with FROM=$start)"
    echo "seeded $end of $n at $(date +%T), load $(uptime | sed 's/.*load averages*: //')"
  done
}

snapshot() {
  [[ -f "$WORK/server.pid" ]] && kill "$(cat "$WORK/server.pid")" 2> /dev/null || true
  "$CONTAINER" stop "$PG_NAME" > /dev/null
  "$CONTAINER" volume export "$VOLUME" --output "$SNAP"
  echo "snapshot: $SNAP ($(du -h "$SNAP" | cut -f1))"
}

restore() {
  "$CONTAINER" rm --force "$PG_NAME" > /dev/null 2>&1 || true
  "$CONTAINER" volume rm --force "$VOLUME" > /dev/null 2>&1 || true
  "$CONTAINER" volume create "$VOLUME" > /dev/null
  "$CONTAINER" volume import "$VOLUME" "$SNAP"
}

# render PLAN OUT RATE CONSUMER SOURCE PATIENTS: PLAN at RATE with its
# tokens and its patients data file filled, refused when a placeholder is
# left. A {{name}} placeholder is ferrohealth-perf's own and stays.
render() {
  local text
  text="$(< "$1")"
  text="${text//\{consumer_token\}/$4}"
  text="${text//\{source_token\}/$5}"
  text="${text//\{patients_file\}/$6}"
  # shellcheck disable=SC2001 # the rate line is matched as a pattern, which parameter expansion cannot anchor
  text="$(sed "s/^rate = .*/rate = $3.0/" <<< "$text")"
  if grep -qE '(^|[^{])\{[a-z_]+_(token|file)\}' <<< "$text"; then
    echo "capacity: $1 holds a placeholder the script does not fill" >&2
    return 1
  fi
  printf '%s\n' "$text" > "$2"
}

run() {
  local group="$1" rate="$2" out pid sampler
  out="$WORK/run-$group-$rate"
  mkdir -p "$out"
  if grep -q '{patients_file}' "$root/perf/capacity/$group.toml"; then
    [[ -s "$WORK/patients.jsonl" ]] || die "no $WORK/patients.jsonl: run \`capacity.sh patients N\` with N at most the patients seeded"
  fi
  render "$root/perf/capacity/$group.toml" "$out/plan.toml" "$rate" \
    "$(token perf-consumer system/Patient.rs)" "$(token perf-source system/Patient.cud)" \
    "$WORK/patients.jsonl"
  pid="$(cat "$WORK/server.pid")"
  uptime > "$out/uptime-before"
  (while kill -0 "$pid" 2> /dev/null; do ps -o %cpu=,rss= -p "$pid"; sleep 1; done) > "$out/server-ps" &
  sampler=$!
  (cd "$root" && ferrohealth-perf run --plan "$out/plan.toml" --baseline "$out/none.toml" \
    --pid "$pid" --base-url "$URL" --mllp-address 127.0.0.1:2575 \
    --database-url "postgresql://postgres:${PG_PASSWORD}@127.0.0.1:5432/ferropix" \
    --out-dir "$out" --summary "$out/summary.md" > "$out/perf.log" 2>&1) || true
  kill "$sampler" 2> /dev/null || true
  uptime > "$out/uptime-after"
  "$CONTAINER" stats --no-stream --format '{{.Name}} {{.CPUPerc}} {{.MemUsage}}' "$PG_NAME" > "$out/pg-stats" 2>&1 || true
  tail -n 40 "$out/perf.log"
}

down() {
  [[ -f "$WORK/server.pid" ]] && kill "$(cat "$WORK/server.pid")" 2> /dev/null || true
  "$CONTAINER" rm --force --volumes "$PG_NAME" "$ISSUER_NAME" "$TX_NAME" > /dev/null 2>&1 || true
  "$CONTAINER" volume rm --force "$VOLUME" > /dev/null 2>&1 || true
}

self_test() {
  local work n=0 cfg
  work="$(mktemp -d)"
  trap 'rm -rf "$work"' RETURN
  check() {
    n=$((n + 1))
    [[ "$2" == "$3" ]] || { echo "capacity: self-test failed: $1: wanted '$2', got '$3'" >&2; exit 1; }
  }
  cfg="$(chunk 500000 500002 TOKEN http://x)"
  check "two feeds are two transfers" 1 "$(grep -c '^next$' <<< "$cfg")"
  check "the identifier is in the URL" 1 "$(grep -c 'C-0500000"$' <<< "$cfg")"
  check "the body is the synthetic patient" "Proefikink Xena female 1987-04-10 2070 CN" \
    "$(sed -n 's/^data = "\(.*\)"$/\1/p' <<< "$cfg" | sed -n 1p | sed 's/\\"/"/g' \
      | jq -r '[.name[0].family, .name[0].given[0], .gender, .birthDate, .address[0].postalCode] | join(" ")')"
  check "each body is JSON once unescaped" 2 \
    "$(sed -n 's/^data = "\(.*\)"$/\1/p' <<< "$cfg" | sed 's/\\"/"/g' | jq -c .resourceType | grep -c Patient)"
  check "a data file row is the seeded patient" "C-0500000 Proefikink Xena female 1987-04-10 2070 CN" \
    "$(rows 500001 | sed -n '$p' | jq -r '[.id, .family, .given, .gender, .birthDate, .postalCode] | join(" ")')"
  check "the data file holds a row a patient" 3 "$(rows 3 | jq -c .id | wc -l | tr -d ' ')"
  printf 'rate = 1.0\npath = "{consumer_token}"\nbody = "{source_token}"\nfile = "{patients_file}"\nx = "{{patient.id}}"\n' > "$work/plan.toml"
  render "$work/plan.toml" "$work/out.toml" 250 C S /p.jsonl
  check "the rate is set" "rate = 250.0" "$(sed -n 1p "$work/out.toml")"
  check "the tokens are filled" 'path = "C" body = "S"' "$(sed -n '2,3p' "$work/out.toml" | tr '\n' ' ' | sed 's/ $//')"
  check "the data file is filled" 'file = "/p.jsonl"' "$(sed -n 4p "$work/out.toml")"
  check "a variable of the plan stays" 'x = "{{patient.id}}"' "$(sed -n 5p "$work/out.toml")"
  printf 'rate = 1.0\nx = "{other_token}"\n' > "$work/left.toml"
  check "a placeholder left is refused" 1 "$(render "$work/left.toml" "$work/x.toml" 1 C S P 2> /dev/null; echo $?)"
  # shellcheck disable=SC2016 # the backticks are the matrix's own text
  printf '| Item | Pin |\n|---|---|\n| PostgreSQL test image | `postgres:1@sha256:00` |\n' > "$work/matrix.md"
  check "a pin is read without its backticks" "postgres:1@sha256:00" "$(pin "PostgreSQL test image" "$work/matrix.md")"
  echo "capacity: self-test OK ($n cases)."
}

case "${1:-}" in
  up) up ;;
  seed) seed "${2:?the number of patients}" ;;
  patients) patients "${2:?the number of patients}" ;;
  snapshot) snapshot ;;
  restore) restore ;;
  run) run "${2:?a group of perf/capacity}" "${3:?a rate}" ;;
  down) down ;;
  --self-test) self_test ;;
  *)
    sed -n '5,40p' "$0" >&2
    exit 2
    ;;
esac
