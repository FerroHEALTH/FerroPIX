#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# The crates.io upload of the published members, and its read-back, as one
# implementation shared by the `crates` leg of release.yml (a `v*` tag,
# approval-gated) and publish-crates.yml (a manual dispatch: the dry-run and
# recovery lane). The two lanes cannot share a reusable workflow: crates.io
# Trusted Publishing matches the OIDC `workflow_ref` claim, and for a job
# inside a reusable workflow that claim names the calling workflow
# (<https://crates.io/docs/trusted-publishing>), so the shared thing is this
# script and each lane keeps its own job and identity.
#
# One crate at a time, "already exists" counted as done, makes the lane
# resumable and idempotent; the registry is read back before success is
# reported (no specification governs this: our own design).
#
# Usage:
#   publish-crates.sh publish          # upload each crate in order
#   publish-crates.sh dry-run          # package every crate, as one set
#   publish-crates.sh package DIR      # package every crate as one set into
#                                      # DIR, each .crate with its .sha256sum
#   publish-crates.sh verify           # read the registry back, with retries
#   publish-crates.sh version CRATE    # print one crate's version
#   publish-crates.sh list             # print the published members, one a line
#
# Requires cargo, curl, and jq; `publish` also needs CARGO_REGISTRY_TOKEN.
set -euo pipefail
cd "$(dirname "$0")/../.."

# The published members, in dependency order. Each name is reserved on
# crates.io and its Trusted Publishing configured by the owner before it is
# listed here.
readonly CRATES=(
  ihe-iti
  nl-generic-functions
)

# The version cargo resolves for one member: its own, or the workspace's when
# it inherits it.
manifest_version() {
  cargo metadata --format-version 1 --no-deps --locked |
    jq -r --arg name "$1" '.packages[] | select(.name == $name) | .version'
}

# cargo colours the status word, so "Uploaded" is followed by a reset sequence
# before the crate name; strip the colour before matching.
readonly ESC=$'\033'
strip_ansi() {
  sed -E "s/${ESC}\\[[0-9;]*m//g"
}

do_publish() {
  local crate out plain failed=""
  for crate in ${CRATES[@]+"${CRATES[@]}"}; do
    echo "::group::$crate $(manifest_version "$crate")"
    out="$(cargo publish -p "$crate" --locked 2>&1)" || true
    printf '%s\n' "$out"
    echo "::endgroup::"
    plain="$(printf '%s' "$out" | strip_ansi)"
    case "$plain" in
      *"already exists on crates.io index"* | *"already uploaded"*)
        echo "$crate: already published at this version, nothing to do"
        ;;
      *)
        if printf '%s' "$plain" | grep -q "Uploaded $crate"; then
          echo "$crate: uploaded"
        else
          failed="$failed $crate"
        fi
        ;;
    esac
  done
  [[ -z "$failed" ]] || {
    echo "::error::failed to publish:$failed"
    return 1
  }
  echo "publish-crates: every crate is at its version or was already there"
}

# Package every member in one cargo invocation, so a member that requires a
# version of another not yet on crates.io (nl-generic-functions on ihe-iti)
# resolves it from the set being packaged
# (<https://doc.rust-lang.org/cargo/commands/cargo-publish.html>).
do_dry_run() {
  local crate args=()
  for crate in ${CRATES[@]+"${CRATES[@]}"}; do
    args+=(-p "$crate")
  done
  if [[ ${#args[@]} -eq 0 ]]; then
    echo "no published crate yet; nothing to package"
    return 0
  fi
  cargo publish "${args[@]}" --dry-run --locked
}

# Package every member in one `cargo package` invocation, in dependency
# order, for the same reason as the dry run (#446): a member packaged alone
# resolves a sibling from crates.io, which does not hold the version being
# released yet (<https://doc.rust-lang.org/cargo/commands/cargo-package.html>).
# Each .crate lands in DIR beside its checksum, the files the release lane
# attests and attaches.
do_package() {
  local dir="$1" crate version file args=()
  for crate in ${CRATES[@]+"${CRATES[@]}"}; do
    args+=(-p "$crate")
  done
  if [[ ${#args[@]} -eq 0 ]]; then
    echo "no published crate yet; nothing to package"
    return 0
  fi
  cargo package "${args[@]}" --locked
  mkdir -p "$dir"
  for crate in "${CRATES[@]}"; do
    version="$(manifest_version "$crate")"
    file="target/package/${crate}-${version}.crate"
    [[ -s "$file" ]] || {
      echo "::error::cargo package wrote no $file" >&2
      return 1
    }
    cp "$file" "$dir/"
    (cd "$dir" && sha256sum "${crate}-${version}.crate" >"${crate}-${version}.crate.sha256sum")
  done
  echo "publish-crates: packaged ${#CRATES[@]} crate(s) as one set into $dir"
}

# Read the registry, never the exit code alone. The index is eventually
# consistent right after an upload: a miss is retried, and a failed request
# counts as "not seen yet", not as a miss.
do_verify() {
  local crate want got body bad=""
  for crate in ${CRATES[@]+"${CRATES[@]}"}; do
    want="$(manifest_version "$crate")"
    got=""
    for _ in 1 2 3 4 5 6; do
      if body="$(curl --proto '=https' --tlsv1.2 -sSL --fail \
        -H 'User-Agent: ferropix-publish-verify (scripts/release/publish-crates.sh)' \
        "https://crates.io/api/v1/crates/$crate/versions" 2> /dev/null)"; then
        got="$(printf '%s' "$body" |
          jq -r --arg v "$want" '.versions[]? | select(.num == $v) | .num' |
          head -1)" || got=""
      fi
      [[ -n "$got" ]] && break
      sleep 10
    done
    printf '%-22s want %-10s %s\n' "$crate" "$want" "${got:-MISSING}"
    [[ -n "$got" ]] || bad="$bad $crate@$want"
  done
  [[ -z "$bad" ]] || {
    echo "::error::the published set is incomplete:$bad"
    return 1
  }
  echo "publish-crates: confirmed on crates.io, all ${#CRATES[@]} crates at their versions"
}

case "${1:-}" in
  publish) do_publish ;;
  dry-run) do_dry_run ;;
  package) do_package "${2:?usage: publish-crates.sh package DIR}" ;;
  verify) do_verify ;;
  version) manifest_version "${2:?usage: publish-crates.sh version CRATE}" ;;
  list) for crate in ${CRATES[@]+"${CRATES[@]}"}; do printf '%s\n' "$crate"; done ;;
  *)
    echo "publish-crates: expected 'publish', 'dry-run', 'package', 'verify', 'version' or 'list', got '${1:-<none>}'" >&2
    exit 2
    ;;
esac
