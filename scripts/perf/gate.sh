#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# The performance gate of the FerroPIX server (#506): ferrohealth-perf
# (FerroHEALTH/FerroSYS#249) against the release build of the server, its
# PostgreSQL and the stub OpenID Provider, each image at the testkit's pin
# in docs/VERSIONS.md, as the e2e lane starts them.
#
#   scripts/perf/gate.sh run [--update]
#       Starts PostgreSQL, the provider and target/release/ferropix with
#       perf/ferropix.toml, seeds the two patients the request set feeds,
#       renders perf/plan.toml with a Consumer's and the Source's token, and
#       runs the gate against perf/baseline.toml. Then it plants an outbox
#       poller that never waits, the FerroPIX #490 shape, and runs the idle
#       phase of perf/busy-loop.toml beside it, which must fail on idle
#       transactions. --update writes perf/baseline.toml from a run that
#       passes. Exit 0 when both hold, 1 when the gate fails or the planted
#       poller passes, 2 when the run cannot be judged or set up.
#   scripts/perf/gate.sh build
#       Builds the release server the gate measures.
#   scripts/perf/gate.sh --self-test
#       Proves the pin reader, the plan rendering and the agreement of the
#       two plans' idle limits offline.
#
# Needs Docker, curl, jq, openssl and ferrohealth-perf on PATH, on Linux,
# where the tool reads the server's CPU and memory from /proc. No
# specification governs this: our own design.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
readonly PLAN=perf/plan.toml
readonly BUSY_PLAN=perf/busy-loop.toml
readonly CONFIG=perf/ferropix.toml
readonly WORK=target/perf-run
readonly BASE=http://127.0.0.1:8080
readonly ISSUER=http://localhost:8180/ferropix
readonly PG_NAME=ferropix-perf-postgres
readonly ISSUER_NAME=ferropix-perf-issuer
readonly PG_PASSWORD=perf
readonly SERVER_URL="postgresql://ferropix:${PG_PASSWORD}@127.0.0.1:5432/ferropix"
readonly ADMIN_URL="postgresql://postgres:${PG_PASSWORD}@127.0.0.1:5432/ferropix"
# The claim of the outbox sender (app/ferropix-server/src/store/outbox.rs),
# polled with no wait between two claims.
readonly POLL='SELECT o.id FROM pix.outbox o WHERE o.next_at <= now() ORDER BY o.next_at, o.id LIMIT 16 FOR UPDATE OF o SKIP LOCKED;'

die() {
  echo "perf-gate: $*" >&2
  exit 2
}

# pin ITEM [MATRIX]: the pin of the docs/VERSIONS.md row whose first cell is
# ITEM, backticks off.
pin() {
  local item="$1" matrix="${2:-$root/docs/VERSIONS.md}"
  awk -F'|' -v item="$item" '{ gsub(/^[[:space:]]+|[[:space:]]+$/, "", $2) }
    $2 == item { v = $3; gsub(/[[:space:]`]/, "", v); print v; exit }' "$matrix"
}

# render PLAN OUT CONSUMER SOURCE: PLAN with its token placeholders
# replaced, refused when a placeholder is left.
render() {
  local text
  text="$(< "$1")"
  text="${text//\{consumer_token\}/$3}"
  text="${text//\{source_token\}/$4}"
  if grep -qE '\{[a-z_]+_token\}' <<< "$text"; then
    echo "perf-gate: $1 holds a placeholder the gate does not fill" >&2
    return 1
  fi
  printf '%s\n' "$text" > "$2"
}

# idle_table PLAN: the [idle] table of PLAN, comments and blank lines off.
idle_table() {
  awk '/^\[/ { inside = ($0 == "[idle]") ; next }
       inside && !/^[[:space:]]*(#|$)/ { print }' "$1"
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

server_pid=""
poller_pid=""
cleanup() {
  [[ -n "$poller_pid" ]] && kill "$poller_pid" 2> /dev/null || true
  [[ -n "$server_pid" ]] && kill "$server_pid" 2> /dev/null || true
  [[ -n "$server_pid" ]] && wait "$server_pid" 2> /dev/null || true
  docker rm --force "$PG_NAME" "$ISSUER_NAME" > /dev/null 2>&1 || true
}

start() {
  local pg_image issuer_image
  pg_image="$(pin "PostgreSQL test image")"
  issuer_image="$(pin "Stub OpenID Provider image")"
  [[ -n "$pg_image" && -n "$issuer_image" ]] || die "docs/VERSIONS.md pins no test image"
  [[ -x target/release/ferropix ]] || die "target/release/ferropix is not built"
  rm -rf "$WORK"
  mkdir -p "$WORK/audit"
  chmod 0700 "$WORK" "$WORK/audit"

  docker run --detach --name "$PG_NAME" --publish 127.0.0.1:5432:5432 \
    --env POSTGRES_PASSWORD="$PG_PASSWORD" --env POSTGRES_DB=ferropix "$pg_image" > /dev/null
  wait_for PostgreSQL 60 docker exec "$PG_NAME" pg_isready --username postgres --dbname ferropix --host 127.0.0.1
  # The quickstart's role (docker/postgres/10-ferropix-role.sh): the server
  # refuses a superuser at boot, and the gate reads the backends as one.
  docker exec "$PG_NAME" psql --quiet -v ON_ERROR_STOP=1 --username postgres --dbname ferropix \
    --command "CREATE ROLE ferropix LOGIN PASSWORD '${PG_PASSWORD}'" \
    --command "GRANT CREATE ON DATABASE ferropix TO ferropix" > /dev/null

  # The client credentials mapping of the issuer service of compose.yaml.
  docker run --detach --name "$ISSUER_NAME" --publish 127.0.0.1:8180:8180 \
    --env SERVER_PORT=8180 \
    --env JSON_CONFIG='{"interactiveLogin":false,"tokenCallbacks":[{"issuerId":"ferropix","requestMappings":[{"requestParam":"grant_type","match":"client_credentials","claims":{"sub":"${clientId}","client_id":"${clientId}","aud":["http://localhost:8080/fhir"],"scope":"${scope}"}}]}]}' \
    "$issuer_image" > /dev/null
  wait_for "the OpenID Provider" 60 curl --fail --silent "$ISSUER/.well-known/openid-configuration"

  printf '%s' "$SERVER_URL" > "$WORK/db-url"
  openssl rand -base64 32 > "$WORK/data-key"
  FERROPIX__USAGE_REPORT__ENABLED=false \
    FERROPIX__DB__URL_FILE="$WORK/db-url" \
    FERROPIX__DB__DATA_KEY_FILE="$WORK/data-key" \
    FERROPIX__AUDIT__SPOOL_DIR="$WORK/audit" \
    target/release/ferropix serve --config "$CONFIG" > "$WORK/server.log" 2>&1 &
  server_pid=$!
  wait_for "the server" 60 curl --fail --silent "$BASE/health/readiness"
}

# seed SOURCE: feeds the two patients of the testkit's ITI-104 fixtures.
seed() {
  local fixture system value status
  for fixture in hospital:urn%3Aoid%3A2.999.1.1:H-100001 laboratory:urn%3Aoid%3A2.999.1.2:L-200001; do
    IFS=: read -r fixture system value <<< "$fixture"
    status="$(curl --silent --output "$WORK/seed.json" --write-out '%{http_code}' -X PUT \
      -H "Authorization: Bearer $1" -H 'Content-Type: application/fhir+json' \
      --data-binary "@tools/ferropix-testkit/fixtures/fhir/iti-104/patient-${fixture}.json" \
      "$BASE/fhir/Patient?identifier=${system}%7C${value}")"
    [[ "$status" = 200 || "$status" = 201 ]] || die "seeding the ${fixture} patient was answered $status"
  done
}

# busy_loop: the planted poller must fail the idle phase on its
# transactions.
busy_loop() {
  local code=0 summary="$WORK/busy-loop.md"
  yes "$POLL" | docker exec --interactive --env PGAPPNAME=perf-busy-loop "$PG_NAME" \
    psql --quiet --no-psqlrc --username ferropix --dbname ferropix > /dev/null 2>&1 &
  poller_pid=$!
  GITHUB_STEP_SUMMARY='' ferrohealth-perf run --plan "$BUSY_PLAN" --pid "$server_pid" \
    --base-url "$BASE" --database-url "$ADMIN_URL" --baseline "$WORK/no-baseline.toml" \
    --out-dir target/perf/busy-loop --summary "$summary" > /dev/null || code=$?
  kill "$poller_pid" 2> /dev/null || true
  poller_pid=""
  docker exec "$PG_NAME" psql --quiet --username postgres --dbname ferropix --command \
    "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name = 'perf-busy-loop'" > /dev/null
  local breaches
  breaches="$(awk '/^### Breaches/ { on = 1; next } /^### / { on = 0 } on' "$summary")"
  if [[ "$code" -eq 1 ]] && grep -qF 'idle.transactions_per_second' <<< "$breaches"; then
    echo "perf-gate: the planted outbox poller fails the idle phase on its transactions"
    report_busy_loop "failed the gate, as it must" "$summary"
    return 0
  fi
  echo "perf-gate: the planted outbox poller was not failed on idle transactions (exit $code)" >&2
  cat "$summary" >&2
  report_busy_loop "was NOT failed on idle transactions (exit $code)" "$summary"
  return 1
}

report_busy_loop() {
  [[ -n "${GITHUB_STEP_SUMMARY:-}" ]] || return 0
  {
    printf '## The planted idle busy-loop\n\nAn outbox poller that never waits %s.\n\n' "$1"
    awk '/^### Breaches/ { on = 1 } /^### Checks/ { on = 0 } on' "$2"
  } >> "$GITHUB_STEP_SUMMARY"
}

run() {
  local update=()
  case "${1:-}" in
    --update) update=(--update) ;;
    "") ;;
    *) die "usage: gate.sh run [--update]" ;;
  esac
  cd "$root"
  command -v ferrohealth-perf > /dev/null || die "ferrohealth-perf is not on PATH"
  trap cleanup EXIT
  start
  local source consumer code=0
  source="$(token perf-source system/Patient.cud)"
  consumer="$(token perf-consumer system/Patient.rs)"
  seed "$source"
  render "$PLAN" "$WORK/plan.toml" "$consumer" "$source" || exit 2
  ferrohealth-perf run --plan "$WORK/plan.toml" --pid "$server_pid" --base-url "$BASE" \
    --database-url "$ADMIN_URL" --baseline perf/baseline.toml --out-dir target/perf \
    "${update[@]}" || code=$?
  busy_loop || { [[ "$code" -ne 0 ]] || code=1; }
  if [[ "$code" -ne 0 ]]; then
    echo "perf-gate: the server log follows" >&2
    tail -n 50 "$WORK/server.log" >&2
  fi
  exit "$code"
}

self_test() {
  local work n=0 failed=0
  work="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand $work now
  trap "rm -r -- '$work'" EXIT
  check() {
    n=$((n + 1))
    if [[ "$2" != "$3" ]]; then
      echo "  FAIL: $1: expected [$2], got [$3]" >&2
      failed=1
    fi
  }
  # shellcheck disable=SC2016 # the backticks are the row's, never expanded
  printf '| Item | Pin | Repeated in |\n|---|---|---|\n| PostgreSQL test image | `postgres:1@sha256:00` | x |\n' > "$work/matrix.md"
  check "a pin is read without its backticks" "postgres:1@sha256:00" "$(pin "PostgreSQL test image" "$work/matrix.md")"
  check "a missing row reads empty" "" "$(pin "Nothing" "$work/matrix.md")"
  printf 'a = "Bearer {consumer_token}"\nb = "Bearer {source_token}"\n' > "$work/plan.toml"
  render "$work/plan.toml" "$work/out.toml" c1 s1
  check "both tokens are filled" $'a = "Bearer c1"\nb = "Bearer s1"' "$(< "$work/out.toml")"
  printf 'a = "Bearer {other_token}"\n' > "$work/plan.toml"
  check "a placeholder left is refused" 1 "$(render "$work/plan.toml" "$work/out.toml" c s 2> /dev/null && echo 0 || echo 1)"
  printf '[idle]\n# a note\nmax = 1.0\n\n[load]\nrate = 2.0\n' > "$work/plan.toml"
  check "the idle table is read alone" "max = 1.0" "$(idle_table "$work/plan.toml")"
  check "the gate and the busy-loop plan hold the same idle limits" \
    "$(idle_table "$root/$PLAN")" "$(idle_table "$root/$BUSY_PLAN")"
  [[ -n "$(idle_table "$root/$PLAN")" ]] || {
    echo "  FAIL: $PLAN holds no [idle] table" >&2
    failed=1
  }
  [[ "$failed" -eq 0 ]] || exit 1
  echo "perf-gate: self-test OK ($n checks)."
}

case "${1:-}" in
  build)
    cd "$root"
    cargo build --release --locked -p ferropix-server
    ;;
  run)
    shift
    run "$@"
    ;;
  --self-test) self_test ;;
  *) die "usage: gate.sh build | run [--update] | --self-test" ;;
esac
