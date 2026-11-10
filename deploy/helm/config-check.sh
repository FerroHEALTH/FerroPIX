#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# Holds the chart's rendered configuration to the configuration contract of
# the images it deploys (#585): for every value set under ci/, the defaults
# with only the required inputs among them, it renders the chart, then runs
# `ferropix config check` over the server's rendered `ferropix.toml` and
# `ferropix-console config check` over the console's `console.toml`, each in
# its image with the environment its container renders, the configuration
# mounted where the chart mounts it and stand-in secret files at the paths
# the chart names. Any refusal fails the check. A second render of each
# container with an unknown key proves the image refuses one, so a pass means
# the image read the file. No specification governs this: our own design.
#
#   deploy/helm/config-check.sh [--chart DIR] [--server IMAGE] [--console IMAGE]
#       Without --server or --console, each container runs the image the
#       rendered Deployment names: the chart's appVersion tag, or the digest
#       a release pins. CI passes the images it built from the tree. The
#       value sets are read from the ci/ directory beside DIR.
#   deploy/helm/config-check.sh --self-test
#       Proves the plumbing offline with a stand-in container engine: an
#       accepted chart passes, a key the image refuses fails naming its value
#       set, and an image that accepts an unknown key fails.
#
# Needs helm, yq and a container engine, `docker` unless CONTAINER_ENGINE
# names another (`podman`). Exit 2 on a usage error or a missing tool.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly UNKNOWN=chart_check_unknown_key

usage() {
  echo "usage: deploy/helm/config-check.sh [--chart DIR] [--server IMAGE] [--console IMAGE] | --self-test" >&2
  exit 2
}

# One container of a rendered value set: its config file key, the mount
# directories and the secret files it needs, by container name.
#   ferropix: ferropix.toml at /etc/ferropix, db-url and data-key under
#     /run/secrets/ferropix.
#   console: console.toml at /etc/ferropix-console, client-secret under
#     /run/secrets/ferropix-console.
spec() {
  case "$1" in
    ferropix) echo "ferropix.toml /etc/ferropix /run/secrets/ferropix" ;;
    console) echo "console.toml /etc/ferropix-console /run/secrets/ferropix-console" ;;
    *) echo "config-check: no container named $1" >&2; return 2 ;;
  esac
}

# stand_ins DIR CONTAINER: the stand-in secret files the container reads.
stand_ins() {
  local dir="$1" container="$2"
  mkdir -p "$dir"
  case "$container" in
    ferropix)
      printf 'postgresql://ferropix:%s@postgres:5432/ferropix' "$(openssl rand -hex 16)" > "$dir/db-url"
      openssl rand -base64 32 > "$dir/data-key"
      ;;
    console) openssl rand -hex 16 > "$dir/client-secret" ;;
    *) echo "config-check: no container named $container" >&2; return 2 ;;
  esac
  # The image runs as 65532, which owns none of these files.
  chmod 0755 "$dir"
  chmod 0644 "$dir"/*
}

# run_check RENDERED CONTAINER IMAGE WORK: runs CONTAINER's config check over
# the configuration RENDERED carries, in IMAGE; prints its output and returns
# its status.
run_check() {
  local rendered="$1" container="$2" image="$3" work="$4" file config secrets
  read -r file config secrets <<< "$(spec "$container")"
  mkdir -p "$work/config"
  chmod 0755 "$work" "$work/config"
  yq "select(.kind == \"ConfigMap\" and .data.\"${file}\" != null) | .data.\"${file}\"" <<< "$rendered" > "$work/config/$file"
  chmod 0644 "$work/config/$file"
  stand_ins "$work/secrets" "$container"
  local env=()
  while IFS= read -r pair; do
    [[ -n "$pair" ]] && env+=(--env "$pair")
  done < <(yq "select(.kind == \"Deployment\") | .spec.template.spec.containers[] | select(.name == \"${container}\") | .env[] | select(.value != null) | .name + \"=\" + .value" <<< "$rendered")
  local out status=0
  out="$("${CONTAINER_ENGINE:-docker}" run --rm --network none --read-only --user 65532:65532 \
    "${env[@]}" \
    --volume "$work/config:${config}:ro" \
    --volume "$work/secrets:${secrets}:ro" \
    "$image" config check 2>&1)" || status=$?
  # The tree the image prints, less the keys left at their defaults.
  grep -v '# default$' <<< "$out" || true
  return "$status"
}

# image_of RENDERED CONTAINER: the image the rendered Deployment runs.
image_of() {
  local rendered="$1" container="$2"
  yq "select(.kind == \"Deployment\") | .spec.template.spec.containers[] | select(.name == \"${container}\") | .image" <<< "$rendered"
}

# check CHART SERVER CONSOLE: every value set, then the unknown-key proof, in
# a subshell of its own, so the trap removes its directory.
check() (
  local chart="$1" server="$2" console="$3" values name rendered image container faults=0 work
  work="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand work now, so the trap removes this directory
  trap "rm -r '$work'" EXIT
  for values in "$(dirname "$chart")"/ci/*-values.yaml; do
    name="$(basename "$values")"
    rendered="$(helm template ferropix "$chart" --namespace ferropix -f "$values")" \
      || { echo "config-check: $name does not render"; faults=1; continue; }
    for container in ferropix console; do
      image="$(image_of "$rendered" "$container")"
      [[ -n "$image" ]] || continue
      [[ "$container" == ferropix && -n "$server" ]] && image="$server"
      [[ "$container" == console && -n "$console" ]] && image="$console"
      echo "config-check: $name, $container, in $image"
      if ! run_check "$rendered" "$container" "$image" "$work/$name-$container" > "$work/out" 2>&1; then
        cat "$work/out"
        echo "config-check: $image refuses the $container configuration $name renders"
        faults=1
      fi
      # The same render with one key the contract does not hold.
      local key="config.server.${UNKNOWN}=1"
      [[ "$container" == console ]] && key="console.config.server.${UNKNOWN}=1"
      if run_check "$(helm template ferropix "$chart" --namespace ferropix -f "$values" --set "$key")" \
        "$container" "$image" "$work/$name-$container-unknown" > "$work/out" 2>&1; then
        echo "config-check: $image accepts the unknown key ${key%=1}, so its config check proves nothing"
        faults=1
      elif ! grep -q "$UNKNOWN" "$work/out"; then
        cat "$work/out"
        echo "config-check: $image refuses the unknown key ${key%=1} without naming it"
        faults=1
      fi
    done
  done
  return "$faults"
)

self_test() {
  local work out
  work="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand work now, so the trap removes this directory
  trap "rm -r '$work'" EXIT
  # A stand-in engine: it reads the mounted configuration and refuses a line
  # that sets a key named in REFUSE, as the image's config check would.
  cat > "$work/engine" <<'ENGINE'
#!/usr/bin/env bash
set -euo pipefail
dir=""
for arg in "$@"; do
  case "$arg" in
    *:/etc/ferropix:ro | *:/etc/ferropix-console:ro) dir="${arg%%:*}" ;;
  esac
done
for file in "$dir"/*.toml; do
  for key in ${REFUSE}; do
    if grep -qE "^[[:space:]]*${key} = " "$file"; then
      echo "cannot start: unknown field \`${key}\`"
      exit 1
    fi
  done
done
echo "the configuration is accepted"
ENGINE
  chmod 0755 "$work/engine"
  mkdir -p "$work/tree"
  cp -R "$here/ferropix" "$here/ci" "$work/tree/"
  export CONTAINER_ENGINE="$work/engine"
  if ! REFUSE="$UNKNOWN" check "$work/tree/ferropix" "" "" > "$work/out" 2>&1; then
    cat "$work/out" >&2
    echo "config-check: self-test failed: the chart was refused" >&2
    exit 1
  fi
  yq -i '.config.server.retired_key = 1' "$work/tree/ci/default-values.yaml"
  if REFUSE="$UNKNOWN retired_key" check "$work/tree/ferropix" "" "" > "$work/out" 2>&1; then
    echo "config-check: self-test failed: a key the image refuses passed" >&2
    exit 1
  fi
  out="$(cat "$work/out")"
  grep -q 'refuses the ferropix configuration default-values.yaml renders' <<< "$out" \
    || { echo "config-check: self-test failed: the refusal does not name its value set" >&2; echo "$out" >&2; exit 1; }
  cp "$here/ci/default-values.yaml" "$work/tree/ci/default-values.yaml"
  if REFUSE="" check "$work/tree/ferropix" "" "" > "$work/out" 2>&1; then
    echo "config-check: self-test failed: an image that accepts an unknown key passed" >&2
    exit 1
  fi
  grep -q 'proves nothing' "$work/out" \
    || { echo "config-check: self-test failed: the refusal does not say why" >&2; cat "$work/out" >&2; exit 1; }
  echo "config-check: self-test passed."
}

if [[ "${1:-}" == "--self-test" ]]; then
  [[ $# -eq 1 ]] || usage
  for tool in helm yq openssl; do
    command -v "$tool" > /dev/null || { echo "config-check: $tool is not on PATH" >&2; exit 2; }
  done
  self_test
  exit 0
fi

chart="$here/ferropix"
server=""
console=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --chart) [[ $# -ge 2 ]] || usage; chart="$(cd "$2" && pwd)"; shift 2 ;;
    --server) [[ $# -ge 2 ]] || usage; server="$2"; shift 2 ;;
    --console) [[ $# -ge 2 ]] || usage; console="$2"; shift 2 ;;
    *) usage ;;
  esac
done
for tool in helm yq openssl "${CONTAINER_ENGINE:-docker}"; do
  command -v "$tool" > /dev/null || { echo "config-check: $tool is not on PATH" >&2; exit 2; }
done
check "$chart" "$server" "$console"
echo "config-check: every value set under $(basename "$(dirname "$chart")")/ci renders a configuration its images accept."
