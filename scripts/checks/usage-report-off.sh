#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# Test launchers switch the usage report off.
#
# `ferropix serve` sends the installation report by default, so every server
# a test lane starts would report from a throwaway runner and count as an
# installation. A launcher is a tracked shell script or workflow that starts
# the server: the binary itself (`target/release/ferropix`, `"$bin/ferropix"`,
# `/usr/local/bin/ferropix`, `cargo run -p ferropix-server`), `docker compose
# ... up`, `helm install` / `helm upgrade`, or `docker run` of the server
# image. Each one carries the switch (`FERROPIX__USAGE_REPORT__ENABLED=false`,
# the same key in a workflow `env:` map, or `usageReport.enabled=false` for a
# chart), or is only ever sourced by scripts that carry it. A `docker run` of
# the server image carries it on its own command line (`--env
# FERROPIX__USAGE_REPORT__ENABLED=false`): a container inherits nothing from
# the shell. A Rust test that starts the binary with `serve` sets
# `.env("FERROPIX__USAGE_REPORT__ENABLED", "false")` on that `Command`. A
# one-shot job (`db`, `config`, `healthcheck`, `--help`, `--version`) sends no
# report and is not a launch, and neither is the console binary
# (`ferropix-console`). The server also stays silent when `GITHUB_ACTIONS` or
# `CI` is set; this guard covers what that misses, a container that does not
# inherit the runner's environment. No specification governs this: our own
# design, after FerroBRIDGE's guard of the same name.
#
# Exempt: this file, which reads the commands as data.
#
#   scripts/checks/usage-report-off.sh [--self-test]
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

readonly SELF='scripts/checks/usage-report-off.sh'
readonly SWITCH='FERROPIX__USAGE_REPORT__ENABLED(=|:[[:space:]]*["'\'']?)["'\'']?false|usageReport\.enabled=false'
# shellcheck disable=SC2016 # a regular expression, never expanded
readonly BINARY='(target/(release|debug)/|/usr/local/bin/|\$bin/|\$\{bin\}/)ferropix(["'\''[:space:]]|$)|cargo[[:space:]]+run[[:space:]].*(-p|--package)[[:space:]=]+ferropix-server|cargo[[:space:]]+run[[:space:]].*--bin[[:space:]=]+ferropix([[:space:]]|$)'
readonly COMPOSE_UP='docker[ -]compose([[:space:]].*)?[[:space:]]up([[:space:]]|$)'
readonly HELM_INSTALL='(^|[;&|(`[:space:]])helm[[:space:]]+(upgrade|install)[[:space:]]'
readonly DOCKER_RUN='docker[[:space:]]+run[[:space:]].*(ghcr\.io/ferrohealth/ferropix[:@]|SERVER_IMAGE|FERROPIX_IMAGE)'
# The switch handed to a container on its own `docker run` command line.
readonly RUN_SWITCH='(-e|--env)[[:space:]=]+["'\'']?FERROPIX__USAGE_REPORT__ENABLED=false'
# A Rust test that starts the binary as a server, and the switch it carries.
readonly RUST_BINARY='CARGO_BIN_EXE_ferropix")'
readonly RUST_LAUNCH='\.arg\( ?"serve" ?\)'
readonly RUST_SWITCH='\.env\( ?"FERROPIX__USAGE_REPORT__ENABLED", ?"false",? ?\)'
# A one-shot job that starts no server.
readonly ONE_SHOT='[[:space:]"'\'',](db|config|healthcheck|--help|--version)(["'\''[:space:],]|$)'

# Prints the file's content without comment lines, one logical line per
# backslash-continued command.
logical_lines() {
  awk '/^[[:space:]]*#/ { next }
       { l = $0; if (sub(/\\$/, "", l)) { buf = buf l " "; next } print buf l; buf = "" }
       END { if (buf != "") print buf }' "$1"
}

# The predicates grep a captured copy, never a pipe: `grep -q` exits at the
# first match, and under pipefail the writer's SIGPIPE would read as no match.
is_launcher() {
  local lines starts
  lines="$(logical_lines "$1")"
  grep -qE "$COMPOSE_UP|$HELM_INSTALL" <<< "$lines" && return 0
  starts="$(grep -E "$BINARY|$DOCKER_RUN" <<< "$lines" || true)"
  [[ -n "$starts" ]] && grep -qvE "$ONE_SHOT" <<< "$starts"
}

# Whether $1 starts the server with a `docker run` that does not hand the
# container the switch itself.
bare_docker_run() {
  local lines runs
  lines="$(logical_lines "$1")"
  runs="$(grep -E "$DOCKER_RUN" <<< "$lines" | grep -vE "$ONE_SHOT" || true)"
  [[ -n "$runs" ]] && grep -qvE "$RUN_SWITCH" <<< "$runs"
}

carries_switch() {
  local lines
  lines="$(logical_lines "$1")"
  grep -qE "$SWITCH" <<< "$lines"
}

# Lists the candidate files: shell scripts and workflow and action YAML.
candidates() {
  local f
  while IFS= read -r f; do
    case "$f" in
      "$SELF") continue ;;
      .github/*.yml | .github/*.yaml | *.sh) printf '%s\n' "$f" ;;
      scripts/* | docker/*)
        head -n 1 "$f" 2> /dev/null | grep -qE '^#!.*(ba)?sh' && printf '%s\n' "$f"
        ;;
      *) ;;
    esac
  done < <("$@" | grep -E '^(scripts|docker|\.github/(workflows|actions))/' | sort)
}

# Prints every candidate that sources `$1` (`. path` or `source path`).
sourcers() {
  local target="$1" list="$2" f
  while IFS= read -r f; do
    [[ "$f" = "$target" ]] && continue
    grep -qE "^[[:space:]]*(\.|source)[[:space:]]+[\"']?([^[:space:]]*/)?${target//./\\.}([\"'[:space:]]|$)" "$f" &&
      printf '%s\n' "$f"
  done <<< "$list"
}

# Prints one finding per launcher that neither carries the switch nor is
# covered by every script that sources it. $@ = the file-listing command.
findings() {
  local list f carriers c covered
  list="$(candidates "$@")"
  while IFS= read -r f; do
    [[ -n "$f" ]] || continue
    is_launcher "$f" || continue
    if bare_docker_run "$f"; then
      printf '%s\n' "$f"
      continue
    fi
    carries_switch "$f" && continue
    carriers="$(sourcers "$f" "$list")"
    covered=0
    if [[ -n "$carriers" ]]; then
      covered=1
      while IFS= read -r c; do
        carries_switch "$c" || covered=0
      done <<< "$carriers"
    fi
    [[ "$covered" -eq 1 ]] || printf '%s\n' "$f"
  done <<< "$list"
  rust_findings "$@"
}

# Prints every tracked Rust source that starts the binary with `serve`
# without the switch. The source is read with its line breaks folded, so a
# call that rustfmt wraps still matches.
rust_findings() {
  local f text
  while IFS= read -r f; do
    [[ -n "$f" ]] || continue
    grep -q "$RUST_BINARY" "$f" || continue
    text="$(tr '\n' ' ' < "$f" | tr -s '[:space:]' ' ')"
    grep -qE "$RUST_LAUNCH" <<< "$text" || continue
    grep -qE "$RUST_SWITCH" <<< "$text" || printf '%s\n' "$f"
  done < <("$@" | grep -E '^(app|tools|crates)/.*\.rs$' | sort)
}

self_test() {
  local tmp found failures=0 name
  tmp="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand $tmp now, while it is still in scope
  trap "rm -r -- '$tmp'" RETURN
  mkdir -p "$tmp/scripts/fam" "$tmp/scripts/checks" "$tmp/docker" "$tmp/.github/workflows" "$tmp/app/server/tests"
  printf '#!/usr/bin/env bash\nFERROPIX__USAGE_REPORT__ENABLED=false target/release/ferropix serve &\n' > "$tmp/scripts/off.sh"
  printf '#!/usr/bin/env bash\nFERROPIX__SERVER__LISTEN=127.0.0.1:1 target/release/ferropix serve > log 2>&1 &\n' > "$tmp/scripts/on.sh"
  # shellcheck disable=SC2016 # the fixture is literal shell text, never expanded here
  printf '#!/usr/bin/env bash\nFERROPIX_CONFIG=x \\\n  "$bin/ferropix" serve >log &\n' > "$tmp/scripts/bin-on.sh"
  # shellcheck disable=SC2016 # the fixture is literal shell text, never expanded here
  printf '#!/usr/bin/env bash\nexport FERROPIX__USAGE_REPORT__ENABLED=false\n"$bin/ferropix" serve &\n' > "$tmp/scripts/bin-off.sh"
  printf '#!/usr/bin/env bash\ncargo run --release -p ferropix-server -- serve\n' > "$tmp/scripts/cargo-on.sh"
  printf '#!/usr/bin/env bash\ndocker compose -f compose.yaml up -d\n' > "$tmp/scripts/compose-on.sh"
  # A launch early in a long file: a predicate that piped into grep -q would miss it.
  {
    printf '#!/usr/bin/env bash\ndocker compose up -d\n'
    seq 1 20000 | sed 's/^/echo /'
  } > "$tmp/scripts/long-on.sh"
  printf '#!/usr/bin/env bash\nhelm upgrade --install r chart \\\n  --set image.tag=x\n' > "$tmp/scripts/helm-on.sh"
  printf '#!/usr/bin/env bash\nhelm upgrade --install r chart \\\n  --set usageReport.enabled=false\n' > "$tmp/scripts/helm-off.sh"
  printf '#!/usr/bin/env bash\n# target/release/ferropix serve &\necho done\n' > "$tmp/scripts/commented.sh"
  printf '#!/usr/bin/env bash\ntarget/release/ferropix-testkit\ntarget/release/ferropix db migrate\ntarget/release/ferropix-console serve &\n' > "$tmp/scripts/jobs.sh"
  printf '#!/usr/bin/env bash\n/usr/local/bin/ferropix config check\n' > "$tmp/scripts/check.sh"
  printf 'target/release/ferropix serve &\n' > "$tmp/scripts/fam/covered.sh"
  printf '#!/usr/bin/env bash\nexport FERROPIX__USAGE_REPORT__ENABLED=false\n. scripts/fam/covered.sh\n' > "$tmp/scripts/carrier.sh"
  printf 'target/release/ferropix serve &\n' > "$tmp/scripts/fam/orphan.sh"
  printf '#!/usr/bin/env bash\n. scripts/fam/orphan.sh\n' > "$tmp/scripts/bare-carrier.sh"
  printf 'jobs:\n  a:\n    steps:\n      - run: |\n          docker run -d ghcr.io/ferrohealth/ferropix:main\n' > "$tmp/.github/workflows/run-on.yml"
  # shellcheck disable=SC2016 # the fixture is literal shell text, never expanded here
  printf 'env:\n  FERROPIX__USAGE_REPORT__ENABLED: "false"\njobs:\n  a:\n    steps:\n      - run: docker run -d "$SERVER_IMAGE"\n' > "$tmp/.github/workflows/run-job-env.yml"
  # shellcheck disable=SC2016 # the fixture is literal shell text, never expanded here
  printf 'jobs:\n  a:\n    steps:\n      - run: docker run -d --env FERROPIX__USAGE_REPORT__ENABLED=false "$SERVER_IMAGE"\n' > "$tmp/.github/workflows/run-off.yml"
  printf 'env:\n  FERROPIX__USAGE_REPORT__ENABLED: "false"\njobs:\n  a:\n    steps:\n      - run: target/release/ferropix serve &\n' > "$tmp/.github/workflows/bin-off.yml"
  printf 'jobs:\n  a:\n    steps:\n      - run: target/release/ferropix serve &\n' > "$tmp/.github/workflows/bin-on.yml"
  printf '#!/usr/bin/env bash\ngrep "target/release/ferropix serve &" x\n' > "$tmp/$SELF"
  printf '#!/usr/bin/env bash\ndocker run --rm ghcr.io/ferrohealth/ferropix:main --version\n' > "$tmp/docker/one-shot.sh"
  printf 'Command::new(env!("CARGO_BIN_EXE_ferropix"))\n    .arg("serve")\n    .output()\n' > "$tmp/app/server/tests/on.rs"
  printf 'Command::new(env!("CARGO_BIN_EXE_ferropix"))\n    .arg("serve")\n    .env(\n        "FERROPIX__USAGE_REPORT__ENABLED",\n        "false",\n    )\n' > "$tmp/app/server/tests/off.rs"
  printf 'Command::new(env!("CARGO_BIN_EXE_ferropix"))\n    .args(["db", "migrate"])\n' > "$tmp/app/server/tests/job.rs"
  printf 'Command::new(env!("CARGO_BIN_EXE_ferropix-console"))\n    .arg("serve")\n' > "$tmp/app/server/tests/console.rs"
  printf '#!/usr/bin/env bash\ndocker run -d ghcr.io/ferrohealth/ferropix-console:main\n' > "$tmp/docker/console.sh"

  found="$(cd "$tmp" && findings find scripts docker .github app -type f)"
  for name in scripts/on.sh scripts/bin-on.sh scripts/cargo-on.sh scripts/compose-on.sh scripts/long-on.sh \
    scripts/helm-on.sh scripts/fam/orphan.sh .github/workflows/run-on.yml .github/workflows/run-job-env.yml \
    .github/workflows/bin-on.yml app/server/tests/on.rs; do
    grep -qxF "$name" <<< "$found" || {
      echo "self-test: $name was not caught" >&2
      failures=1
    }
  done
  for name in scripts/off.sh scripts/bin-off.sh scripts/helm-off.sh scripts/commented.sh scripts/jobs.sh \
    scripts/check.sh scripts/fam/covered.sh scripts/carrier.sh scripts/bare-carrier.sh .github/workflows/run-off.yml \
    .github/workflows/bin-off.yml "$SELF" docker/one-shot.sh docker/console.sh app/server/tests/off.rs app/server/tests/job.rs \
    app/server/tests/console.rs; do
    if grep -qxF "$name" <<< "$found"; then
      echo "self-test: $name was wrongly caught" >&2
      failures=1
    fi
  done
  [[ "$failures" -eq 0 ]] || return 1
  echo "usage-report-off: self-test OK (eleven unswitched launchers caught, seventeen clean or exempt files allowed)."
}

report() {
  local found="$1"
  if [[ -z "$found" ]]; then
    echo "usage-report-off: OK (every launcher switches the usage report off)."
    return 0
  fi
  echo "error: a launcher starts a FerroPIX server with the usage report on" >&2
  local f
  while IFS= read -r f; do printf '  %s\n' "$f" >&2; done <<< "$found"
  echo "Set FERROPIX__USAGE_REPORT__ENABLED=false in the launcher (or in every script" >&2
  echo "that sources it, or in the workflow env); a docker run of the server passes" >&2
  echo "--env FERROPIX__USAGE_REPORT__ENABLED=false on its own command line." >&2
  return 1
}

case "${1:-}" in
  --self-test) self_test ;;
  "") report "$(findings git ls-files)" ;;
  *)
    echo "usage: scripts/checks/usage-report-off.sh [--self-test]" >&2
    exit 2
    ;;
esac
