#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# The performance gate of the FerroPIX server (#506): ferrohealth-perf
# (FerroHEALTH/FerroSYS#249) in its A/B mode, the head of a change judged
# against its base measured beside it on the same runner, each the release
# build of the server over its own database in one PostgreSQL, with the stub
# OpenID Provider, each image at the testkit's pin in docs/VERSIONS.md.
#
#   scripts/perf/gate.sh build [BASE]
#       Builds the release server of the checkout, the head, and of its
#       base: the merge base of HEAD with BASE, a commit or ref, or with no
#       BASE the newest v* tag before HEAD, else HEAD's first parent. The
#       base is built from `git archive` of that commit under target/, with
#       its own toolchain pin, into the same target directory, so the
#       dependencies build once.
#   scripts/perf/gate.sh run
#       Starts PostgreSQL, the provider and both servers, the head on 8080
#       and MLLP 2575 with perf/ferropix.toml, the base on 8081 and 2576
#       with the base's own perf/ferropix.toml when it has one, seeds the
#       two patients the request set feeds into each, renders perf/plan.toml
#       with a Consumer's and the Source's token, and runs `ferrohealth-perf
#       ab`: the head's idle phase, then pairs of load phases in turn. Then
#       it plants an outbox poller that never waits, the FerroPIX #490 shape,
#       and runs the idle phase of perf/busy-loop.toml against the head
#       beside it, which must fail on idle transactions. Exit 0 when both
#       hold, 1 when the gate fails or the planted poller passes, 2 when the
#       run cannot be judged or set up. While the tool runs, the runner is
#       sampled into target/perf/runner.tsv (#511): a spool-shaped disk
#       sync, steal, I/O wait and the CPU clock. A breach on a runner whose
#       sync p99 is over SLOW_SYNC_MS is not judged and exits 2.
#   scripts/perf/gate.sh --self-test
#       Proves the pin reader, the plan rendering, the base resolution, the
#       agreement of the two plans' idle limits and the runner summary
#       offline.
#
# Needs Docker, curl, git, jq, openssl and ferrohealth-perf on PATH, on
# Linux, where the tool reads the servers' CPU and memory from /proc. No
# specification governs this: our own design.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
readonly PLAN=perf/plan.toml
readonly BUSY_PLAN=perf/busy-loop.toml
readonly CONFIG=perf/ferropix.toml
readonly WORK=target/perf-run
readonly BASE_TREE=target/perf-base
readonly HEAD_URL=http://127.0.0.1:8080
readonly BASE_URL=http://127.0.0.1:8081
readonly HEAD_MLLP=127.0.0.1:2575
readonly BASE_MLLP=127.0.0.1:2576
readonly ISSUER=http://localhost:8180/ferropix
readonly PG_NAME=ferropix-perf-postgres
readonly ISSUER_NAME=ferropix-perf-issuer
readonly PG_PASSWORD=perf
# The claim of the outbox sender (app/ferropix-server/src/store/outbox.rs),
# polled with no wait between two claims.
readonly POLL='SELECT o.id FROM pix.outbox o WHERE o.next_at <= now() ORDER BY o.next_at, o.id LIMIT 16 FOR UPDATE OF o SKIP LOCKED;'

# server_url DB and admin_url DB: the server's role and the superuser on DB.
server_url() { printf 'postgresql://ferropix:%s@127.0.0.1:5432/%s' "$PG_PASSWORD" "$1"; }
admin_url() { printf 'postgresql://postgres:%s@127.0.0.1:5432/%s' "$PG_PASSWORD" "$1"; }

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

# base_rev [BASE] [AT]: the commit the base is built from, as `build` names
# it, resolved from AT (HEAD by default).
base_rev() {
  local base="${1:-}" at="${2:-HEAD}" tag
  # The commit before the push that created a branch is all zeros.
  [[ "$base" =~ ^0+$ ]] && base=""
  if [[ -n "$base" ]]; then
    git -C "$root" merge-base "$at" "$base"
  elif tag="$(git -C "$root" describe --tags --abbrev=0 --match 'v*' "$at^" 2> /dev/null)"; then
    git -C "$root" rev-parse "$tag^{commit}"
  else
    git -C "$root" rev-parse "$at^1"
  fi
}

# build [BASE]: the base's release server into target/perf-base/ferropix,
# then the head's into target/release/ferropix.
build() {
  local rev
  cd "$root"
  rev="$(base_rev "${1:-}")" || die "the base of ${1:-the previous release} cannot be resolved"
  echo "perf-gate: the base is $rev"
  rm -rf "$BASE_TREE"
  mkdir -p "$BASE_TREE/src"
  git archive "$rev" | tar -x -C "$BASE_TREE/src"
  printf '%s\n' "$rev" > "$BASE_TREE/rev"
  # From inside the base's tree, so its own toolchain pin builds it.
  (cd "$BASE_TREE/src" && cargo build --release --locked -p ferropix-server --target-dir "$root/target")
  cp target/release/ferropix "$BASE_TREE/ferropix"
  cargo build --release --locked -p ferropix-server
}

# The p99 of the runner's spool-shaped sync, in ms, above which the gate
# cannot judge the server (#511): every audited request waits for such a
# sync, so a disk this slow stalls every route at once. A runner that kept
# the gate steady measured 8 ms, one that stalled it 270 ms.
readonly SLOW_SYNC_MS=50

# sample OUT: every quarter second until killed, appends one line to OUT of
# what the runner did since the line before (#511): the Unix time; the
# milliseconds a 16 KiB record took to reach the disk the way the audit
# spool stores one (a file synced, renamed, its directory synced), on the
# spool's file system; the share of CPU time stolen by the hypervisor and
# spent waiting on I/O; and the mean clock of the CPUs in MHz.
sample() {
  local out="$1" probe="$WORK/probe" t0 t1 cpu0 cpu1
  mkdir -p "$probe"
  printf 'time\tsync_ms\tsteal_pct\tiowait_pct\tmhz\n' > "$out"
  cpu0="$(cpu_ticks)"
  while :; do
    t0="$EPOCHREALTIME"
    dd if=/dev/zero of="$probe/record.partial" bs=16k count=1 conv=fsync status=none
    mv -f "$probe/record.partial" "$probe/record"
    sync "$probe"
    t1="$EPOCHREALTIME"
    sleep 0.25
    cpu1="$(cpu_ticks)"
    awk -v t0="$t0" -v t1="$t1" -v b="$cpu0" -v a="$cpu1" -v mhz="$(cpu_mhz)" 'BEGIN {
      split(b, x, " "); split(a, y, " "); total = y[1] - x[1]
      printf "%.3f\t%.3f\t%.2f\t%.2f\t%.0f\n", t0, (t1 - t0) * 1000,
        (total > 0 ? 100 * (y[2] - x[2]) / total : 0), (total > 0 ? 100 * (y[3] - x[3]) / total : 0), mhz
    }' >> "$out"
    cpu0="$cpu1"
  done
}

# cpu_ticks: the total, steal and iowait ticks of all CPUs (proc(5)).
cpu_ticks() {
  awk '$1 == "cpu" { t = 0; for (i = 2; i <= NF; i++) t += $i; print t, $9, $6; exit }' /proc/stat
}

# cpu_mhz: the mean clock of the CPUs (proc(5) cpuinfo).
cpu_mhz() {
  awk -F': ' '/^cpu MHz/ { s += $2; n++ } END { if (n) printf "%.0f", s / n; else print 0 }' /proc/cpuinfo
}

# sync_p99 OUT: the p99 of the sync times sample wrote to OUT, in whole ms,
# or nothing when it wrote none.
sync_p99() {
  tail -n +2 "$1" | cut -f 2 | sort -g | awk '{ s[++n] = $1 }
    END { if (!n) exit; k = int(n * 0.99 + 0.5); if (k < 1) k = 1; if (k > n) k = n; printf "%d", s[k] }'
}

# judged CODE P99: the gate's exit code once the runner's sync p99 is
# known: a breach (1) on a runner whose disk syncs slower than SLOW_SYNC_MS
# could not be judged (2), since every request waited on that disk; any
# other code stands.
judged() {
  if [[ "$1" -eq 1 && -n "$2" && "$2" -gt "$SLOW_SYNC_MS" ]]; then
    echo 2
  else
    echo "$1"
  fi
}

# summarize OUT: one line over the samples sample wrote to OUT: the sync
# time's p50, p99 and maximum, the samples over 50 ms, the mean and largest
# steal and I/O wait, and the clock's range.
summarize() {
  sort -t "$(printf '\t')" -k2,2g <(tail -n +2 "$1") | awk -F'\t' '
    { n++; s[n] = $2; if ($2 > 50) slow++
      st += $3; if ($3 > stmax) stmax = $3; io += $4; if ($4 > iomax) iomax = $4
      if (n == 1 || $5 < lo) lo = $5; if ($5 > hi) hi = $5 }
    function at(q, k) { k = int(n * q + 0.5); if (k < 1) k = 1; if (k > n) k = n; return s[k] }
    END {
      if (!n) { print "no samples"; exit }
      printf "%d samples; spool-shaped sync p50 %.1f ms, p99 %.1f ms, max %.1f ms, %d over 50 ms; steal mean %.2f %%, max %.2f %%; I/O wait mean %.2f %%, max %.2f %%; clock %d to %d MHz\n",
        n, at(0.50), at(0.99), s[n], slow + 0, st / n, stmax, io / n, iomax, lo, hi
    }'
}

head_pid=""
base_pid=""
poller_pid=""
sampler_pid=""
cleanup() {
  [[ -n "$sampler_pid" ]] && kill "$sampler_pid" 2> /dev/null || true
  [[ -n "$poller_pid" ]] && kill "$poller_pid" 2> /dev/null || true
  local pid
  for pid in "$head_pid" "$base_pid"; do
    [[ -n "$pid" ]] && kill "$pid" 2> /dev/null || true
    [[ -n "$pid" ]] && wait "$pid" 2> /dev/null || true
  done
  docker rm --force "$PG_NAME" "$ISSUER_NAME" > /dev/null 2>&1 || true
}

# serve NAME BINARY CONFIG DB PORT MLLP: starts one server in the
# background, its secrets and spool under $WORK/NAME, and prints its pid.
serve() {
  local binary="$2" config="$3" db="$4" port="$5" mllp="$6" dir="$WORK/$1"
  mkdir -p "$dir/audit"
  chmod 0700 "$dir" "$dir/audit"
  server_url "$db" > "$dir/db-url"
  openssl rand -base64 32 > "$dir/data-key"
  FERROPIX__USAGE_REPORT__ENABLED=false \
    FERROPIX__DB__URL_FILE="$dir/db-url" \
    FERROPIX__DB__DATA_KEY_FILE="$dir/data-key" \
    FERROPIX__AUDIT__SPOOL_DIR="$dir/audit" \
    FERROPIX__SERVER__LISTEN="127.0.0.1:$port" \
    FERROPIX__HL7V2__LISTEN="$mllp" \
    "$binary" serve --config "$config" > "$dir/server.log" 2>&1 &
  echo "$!"
}

start() {
  local pg_image issuer_image base_config=$CONFIG db
  pg_image="$(pin "PostgreSQL test image")"
  issuer_image="$(pin "Stub OpenID Provider image")"
  [[ -n "$pg_image" && -n "$issuer_image" ]] || die "docs/VERSIONS.md pins no test image"
  [[ -x target/release/ferropix ]] || die "target/release/ferropix is not built"
  [[ -x "$BASE_TREE/ferropix" ]] || die "$BASE_TREE/ferropix is not built: run gate.sh build first"
  [[ -f "$BASE_TREE/src/$CONFIG" ]] && base_config="$BASE_TREE/src/$CONFIG"
  rm -rf "$WORK"
  mkdir -p "$WORK"
  chmod 0700 "$WORK"

  docker run --detach --name "$PG_NAME" --publish 127.0.0.1:5432:5432 \
    --env POSTGRES_PASSWORD="$PG_PASSWORD" --env POSTGRES_DB=ferropix "$pg_image" > /dev/null
  wait_for PostgreSQL 60 docker exec "$PG_NAME" pg_isready --username postgres --dbname ferropix --host 127.0.0.1
  # The quickstart's role (docker/postgres/10-ferropix-role.sh): the server
  # refuses a superuser at boot, and the gate reads the backends as one.
  # Each server has a database of its own, so each arm counts its own
  # transactions.
  docker exec "$PG_NAME" psql --quiet -v ON_ERROR_STOP=1 --username postgres --dbname ferropix \
    --command "CREATE ROLE ferropix LOGIN PASSWORD '${PG_PASSWORD}'" \
    --command "CREATE DATABASE ferropix_base" > /dev/null
  for db in ferropix ferropix_base; do
    docker exec "$PG_NAME" psql --quiet -v ON_ERROR_STOP=1 --username postgres --dbname ferropix \
      --command "GRANT CREATE ON DATABASE ${db} TO ferropix" > /dev/null
  done

  # The client credentials mapping of the issuer service of compose.yaml.
  docker run --detach --name "$ISSUER_NAME" --publish 127.0.0.1:8180:8180 \
    --env SERVER_PORT=8180 \
    --env JSON_CONFIG='{"interactiveLogin":false,"tokenCallbacks":[{"issuerId":"ferropix","requestMappings":[{"requestParam":"grant_type","match":"client_credentials","claims":{"sub":"${clientId}","client_id":"${clientId}","aud":["http://localhost:8080/fhir"],"scope":"${scope}"}}]}]}' \
    "$issuer_image" > /dev/null
  wait_for "the OpenID Provider" 60 curl --fail --silent "$ISSUER/.well-known/openid-configuration"

  head_pid="$(serve head target/release/ferropix "$CONFIG" ferropix 8080 "$HEAD_MLLP")"
  base_pid="$(serve base "$BASE_TREE/ferropix" "$base_config" ferropix_base 8081 "$BASE_MLLP")"
  wait_for "the head server" 60 curl --fail --silent "$HEAD_URL/health/readiness"
  wait_for "the base server" 60 curl --fail --silent "$BASE_URL/health/readiness"
}

# seed URL SOURCE: feeds the two patients of the testkit's ITI-104 fixtures
# to the server at URL.
seed() {
  local fixture system value status
  for fixture in hospital:urn%3Aoid%3A2.999.1.1:H-100001 laboratory:urn%3Aoid%3A2.999.1.2:L-200001; do
    IFS=: read -r fixture system value <<< "$fixture"
    status="$(curl --silent --output "$WORK/seed.json" --write-out '%{http_code}' -X PUT \
      -H "Authorization: Bearer $2" -H 'Content-Type: application/fhir+json' \
      --data-binary "@tools/ferropix-testkit/fixtures/fhir/iti-104/patient-${fixture}.json" \
      "$1/fhir/Patient?identifier=${system}%7C${value}")"
    [[ "$status" = 200 || "$status" = 201 ]] || die "seeding the ${fixture} patient into $1 was answered $status"
  done
}

# busy_loop: the planted poller must fail the head's idle phase on its
# transactions.
busy_loop() {
  local code=0 summary="$WORK/busy-loop.md"
  yes "$POLL" | docker exec --interactive --env PGAPPNAME=perf-busy-loop "$PG_NAME" \
    psql --quiet --no-psqlrc --username ferropix --dbname ferropix > /dev/null 2>&1 &
  poller_pid=$!
  GITHUB_STEP_SUMMARY='' ferrohealth-perf run --plan "$BUSY_PLAN" --pid "$head_pid" \
    --base-url "$HEAD_URL" --database-url "$(admin_url ferropix)" --baseline "$WORK/no-baseline.toml" \
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
  [[ $# -eq 0 ]] || die "usage: gate.sh run"
  cd "$root"
  command -v ferrohealth-perf > /dev/null || die "ferrohealth-perf is not on PATH"
  trap cleanup EXIT
  start
  local source consumer code=0 server
  source="$(token perf-source system/Patient.cud)"
  consumer="$(token perf-consumer system/Patient.rs)"
  seed "$HEAD_URL" "$source"
  seed "$BASE_URL" "$source"
  render "$PLAN" "$WORK/plan.toml" "$consumer" "$source" || exit 2
  if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
    printf "The base is \`%s\`.\n\n" "$(< "$BASE_TREE/rev")" >> "$GITHUB_STEP_SUMMARY"
  fi
  mkdir -p target/perf
  sample target/perf/runner.tsv &
  sampler_pid=$!
  ferrohealth-perf ab --plan "$WORK/plan.toml" \
    --base-url "$BASE_URL" --base-pid "$base_pid" --base-mllp-address "$BASE_MLLP" \
    --base-database-url "$(admin_url ferropix_base)" \
    --head-url "$HEAD_URL" --head-pid "$head_pid" --head-mllp-address "$HEAD_MLLP" \
    --head-database-url "$(admin_url ferropix)" \
    --out-dir target/perf || code=$?
  kill "$sampler_pid" 2> /dev/null || true
  wait "$sampler_pid" 2> /dev/null || true
  sampler_pid=""
  local runner
  runner="$(summarize target/perf/runner.tsv 2> /dev/null)" || runner="the runner was not sampled"
  echo "perf-gate: the runner: $runner" >&2
  local p99 verdict
  p99="$(sync_p99 target/perf/runner.tsv 2> /dev/null)" || p99=""
  verdict="$(judged "$code" "$p99")"
  if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
    printf "### The runner\n\n%s (\`target/perf/runner.tsv\`, #511).\n\n" "$runner" >> "$GITHUB_STEP_SUMMARY"
    if [[ "$verdict" != "$code" ]]; then
      printf "The breach is not judged: the runner's disk synced at p99 %s ms, over %s ms, and every audited request waits on it.\n\n" "$p99" "$SLOW_SYNC_MS" >> "$GITHUB_STEP_SUMMARY"
    fi
  fi
  if [[ "$verdict" != "$code" ]]; then
    echo "perf-gate: the breach is not judged: the runner's disk synced at p99 ${p99} ms, over ${SLOW_SYNC_MS} ms" >&2
  fi
  code="$verdict"
  busy_loop || { [[ "$code" -ne 0 ]] || code=1; }
  if [[ "$code" -ne 0 ]]; then
    for server in head base; do
      echo "perf-gate: the $server server's log follows" >&2
      tail -n 50 "$WORK/$server/server.log" >&2
    done
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
  {
    printf 'time\tsync_ms\tsteal_pct\tiowait_pct\tmhz\n'
    printf '1.0\t2.0\t0.00\t1.00\t2400\n'
    printf '2.0\t180.0\t0.50\t9.00\t2450\n'
    printf '3.0\t4.0\t0.00\t2.00\t2400\n'
  } > "$work/runner.tsv"
  check "the runner's samples are summarized" \
    "3 samples; spool-shaped sync p50 4.0 ms, p99 180.0 ms, max 180.0 ms, 1 over 50 ms; steal mean 0.17 %, max 0.50 %; I/O wait mean 4.00 %, max 9.00 %; clock 2400 to 2450 MHz" \
    "$(summarize "$work/runner.tsv")"
  check "the sync p99 is read in whole ms" 180 "$(sync_p99 "$work/runner.tsv")"
  printf 'time\tsync_ms\tsteal_pct\tiowait_pct\tmhz\n' > "$work/empty.tsv"
  check "a runner with no samples says so" "no samples" "$(summarize "$work/empty.tsv")"
  check "a runner with no samples has no sync p99" "" "$(sync_p99 "$work/empty.tsv")"
  check "a breach on a slow disk is not judged" 2 "$(judged 1 180)"
  check "a breach on a fast disk stands" 1 "$(judged 1 8)"
  check "a pass on a slow disk stands" 0 "$(judged 0 180)"
  check "a breach with no samples stands" 1 "$(judged 1 "")"
  local repo="$work/repo" saved_root="$root"
  git init --quiet "$repo"
  commit() { git -C "$repo" -c user.name=t -c user.email=t@t -c commit.gpgsign=false -c tag.gpgsign=false commit --quiet --allow-empty -m "$1"; }
  commit one
  commit two
  git -C "$repo" -c tag.gpgsign=false tag v0.0.1
  commit three
  git -C "$repo" branch side
  commit four
  git -C "$repo" checkout --quiet side
  commit side-only
  git -C "$repo" checkout --quiet -
  root="$repo"
  check "with no BASE the base is the newest tag before HEAD" \
    "$(git -C "$repo" rev-parse v0.0.1)" "$(base_rev "")"
  check "a BASE names its merge base with HEAD" \
    "$(git -C "$repo" rev-parse HEAD~1)" "$(base_rev side)"
  check "an all-zero BASE takes the previous release" \
    "$(git -C "$repo" rev-parse v0.0.1)" "$(base_rev 0000000000000000000000000000000000000000)"
  check "with no tag before it the base is the first parent" \
    "$(git -C "$repo" rev-parse v0.0.1~1)" "$(base_rev "" v0.0.1)"
  root="$saved_root"
  [[ "$failed" -eq 0 ]] || exit 1
  echo "perf-gate: self-test OK ($n checks)."
}

case "${1:-}" in
  build)
    shift
    build "$@"
    ;;
  run)
    shift
    run "$@"
    ;;
  --self-test) self_test ;;
  *) die "usage: gate.sh build [BASE] | run | --self-test" ;;
esac
