#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
#
# The SemVer judge of the published crates (.claude/rules/crates-publishing.md,
# #586). cargo-semver-checks compares a crate's public API with a baseline and
# refuses a breaking change the version does not answer: in 0.y.z only the
# left-most non-zero component is incompatible, so a break needs the minor
# while the crate is 0.x (The Cargo Book, "SemVer Compatibility",
# <https://doc.rust-lang.org/cargo/reference/semver.html>). It judges twice:
# on the tool's default feature selection, and with --only-explicit-features,
# which is what a consumer built with `default-features = false` compiles
# against, since the tool does not catch a break that exists only under a
# subset of the features (its README, <https://github.com/obi1kenobi/cargo-semver-checks>).
# A public dependency (scripts/checks/lib/public-deps.sh) moved to another
# compatibility line is a break the tool cannot see, so it needs the minor too.
# No specification governs where the judge runs: our own design.
# crate-version-guard.sh names the crates whose version moved on a pull
# request and in the merge queue, judged against the target branch; the
# release plan judges every crate it publishes against crates.io
# (scripts/release/publish-crates.sh semver).
#
#   semver.sh [--baseline-rev <rev>] [<crate>...]
#       Judge the named crates, or every crate of the publish list, against
#       <rev> or, without it, against each crate's newest release on
#       crates.io. A crate at its baseline's version, at 0.0.0, or new since
#       the baseline is named and left out.
#   semver.sh --pin
#       Print the cargo-semver-checks version docs/VERSIONS.md pins.
#   semver.sh --self-test
#       Proves the verdicts offline with a stand-in cargo.
#
# The installed cargo-semver-checks must be the pinned version. Exit 0 when
# every crate's version answers its change, 1 when one does not, 2 on a usage
# or setup error.
set -euo pipefail
script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
root="${SEMVER_ROOT:-$(cd "$(dirname "$script")/../.." && pwd)}"
cd "$root"
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$(dirname "$script")/lib/public-deps.sh"

usage() {
  echo "usage: semver.sh [--baseline-rev <rev>] [<crate>...] | --pin | --self-test" >&2
  exit 2
}

# The `[package]` table's own `version` in the manifest on stdin.
package_version() {
  awk -F'"' '/^\[package\]/{p=1; next} /^\[/{p=0} p && /^version[[:space:]]*=/{print $2; exit}'
}

# The crates.io API document at path $1 under /api/v1/crates/.
crates_io() {
  curl --proto '=https' --tlsv1.2 -sSL --fail --retry 3 \
    -H 'User-Agent: ferropix-semver (https://github.com/FerroHEALTH/FerroPIX)' \
    "https://crates.io/api/v1/crates/$1" 2> /dev/null
}

# The newest stable release of crate $1 on crates.io; empty when it has none.
newest_release() {
  # The self-test's stand-in for crates.io: `crate version` lines.
  if [[ -n "${SEMVER_RELEASES+x}" ]]; then
    awk -v c="$1" '$1 == c { print $2 }' <<< "$SEMVER_RELEASES"
    return
  fi
  crates_io "$1" | jq -r '.crate.max_stable_version // empty' || true
}

# The version crate $1 has at the baseline: its manifest at the baseline
# revision, or its newest stable release on crates.io; empty when it has none.
baseline_version() {
  local crate="$1"
  if [[ -n "$baseline_rev" ]]; then
    git show "$baseline_rev:crates/$crate/Cargo.toml" 2> /dev/null | package_version || true
  else
    newest_release "$crate"
  fi
}

# Whether $1 and $2 share their compatibility line (the Cargo caret rule).
same_line() {
  [[ "$(compat_line "$1")" == "$(compat_line "$2")" ]]
}

# The public dependency lines of crate $1 in the tree, at revision $2, and of
# release $2 on crates.io.
tree_lines() {
  manifest_requirements Cargo.toml "crates/$1/Cargo.toml" | public_lines
}
revision_lines() {
  manifest_requirements <(git show "$2:Cargo.toml") <(git show "$2:crates/$1/Cargo.toml") | public_lines
}
release_lines() {
  crates_io "$1/$2/dependencies" |
    jq -r '.dependencies[] | select(.kind == "normal") | "\(.crate_id) \(.req)"' | public_lines
}

# The public dependencies of crate $1 that moved to another compatibility line
# since version $2 at the baseline. Against a revision, the newest release on
# crates.io counts too while it shares the line of $2: a move that landed under
# an unpublished patch still breaks the consumers of that release.
moved_since() {
  local crate="$1" previous="$2" current published moved=""
  current="$(tree_lines "$crate")"
  if [[ -n "$baseline_rev" ]]; then
    moved="$(moved_lines "$(revision_lines "$crate" "$baseline_rev")" "$current")"
    published="$(newest_release "$crate")"
    if [[ -n "$published" ]] && ! same_line "$published" "$previous"; then published=""; fi
  else
    published="$previous"
  fi
  if [[ -z "$moved" ]] && [[ -n "$published" ]]; then
    moved="$(moved_lines "$(release_lines "$crate" "$published")" "$current")"
  fi
  echo "$moved"
}

# judge CRATE PASS ARGS...: one cargo-semver-checks run; prints its output and
# returns 0 when it accepts, 1 on a break, 2 when it could not judge.
judge() {
  local crate="$1" pass="$2" status=0
  shift 2
  echo "semver: $crate, $pass"
  cargo semver-checks check-release --package "$crate" "$@" > "$log" 2>&1 || status=$?
  cat "$log"
  # Only a summary the tool printed counts as a judgement; a build that fails
  # is named apart from a break.
  if [[ "$status" -ne 0 ]] && grep -q 'semver requires new' "$log"; then
    return 1
  fi
  if [[ "$status" -ne 0 ]] || ! grep -q 'Summary' "$log"; then
    echo "::error::semver: cargo-semver-checks could not judge $crate ($pass, exit $status); see its output above." >&2
    return 2
  fi
}

run() {
  # The Pin cell of the cargo-semver-checks row, its backticks stripped.
  local pin installed
  pin="$(awk -F'|' '{ k = $2; gsub(/[` ]/, "", k) } k == "cargo-semver-checks" { v = $3; gsub(/[^0-9.]/, "", v); print v; exit }' docs/VERSIONS.md)"
  if [[ -z "$pin" ]]; then
    echo "semver: docs/VERSIONS.md pins no cargo-semver-checks version" >&2
    exit 2
  fi
  if [[ "${1:-}" == "--pin" ]]; then
    [[ "$#" -eq 1 ]] || usage
    echo "$pin"
    exit 0
  fi
  baseline_rev=""
  if [[ "${1:-}" == "--baseline-rev" ]]; then
    [[ "$#" -ge 2 ]] || usage
    baseline_rev="$2"
    shift 2
  fi
  [[ "${1:-}" != -* ]] || usage
  if ! installed="$(cargo semver-checks --version 2> /dev/null)"; then
    echo "semver: cargo-semver-checks is not installed; install $pin (cargo binstall cargo-semver-checks@$pin)" >&2
    exit 2
  fi
  if [[ "$installed" != "cargo-semver-checks $pin" ]]; then
    echo "semver: found '$installed', docs/VERSIONS.md pins $pin" >&2
    exit 2
  fi

  local crates=("$@")
  if [[ "${#crates[@]}" -eq 0 ]]; then
    while IFS= read -r crate; do
      [[ -n "$crate" ]] && crates+=("$crate")
    done < <(awk '/^readonly CRATES=\(/ { p = 1; next } p && /^\)/ { exit } p { gsub(/[[:space:]]/, ""); if ($0 != "" && $0 !~ /^#/) print }' scripts/release/publish-crates.sh)
  fi
  [[ "${#crates[@]}" -gt 0 ]] || { echo "semver: no crate to judge"; exit 0; }

  log="$(mktemp)"
  trap 'rm -f "$log"' EXIT
  local against="${baseline_rev:-crates.io}" failed="" crate current previous moved verdict code
  for crate in "${crates[@]}"; do
    [[ -f "crates/$crate/Cargo.toml" ]] || { echo "semver: no crate crates/$crate" >&2; exit 2; }
    current="$(package_version < "crates/$crate/Cargo.toml")"
    if [[ "$current" == "0.0.0" ]]; then
      echo "semver: $crate is at 0.0.0, a name reservation outside the line"
      continue
    fi
    previous="$(baseline_version "$crate")"
    if [[ -z "$previous" ]] || [[ "$previous" == "0.0.0" ]]; then
      echo "semver: $crate has no release at $against to compare with"
      continue
    fi
    if [[ "$previous" == "$current" ]]; then
      echo "semver: $crate is at $current at $against too, nothing to judge"
      continue
    fi
    echo "semver: $crate $previous -> $current against $against"
    local args=()
    [[ -z "$baseline_rev" ]] || args+=(--baseline-rev "$baseline_rev")
    verdict=0
    code=0
    judge "$crate" "the default feature selection" ${args[@]+"${args[@]}"} || code=$?
    (( code > verdict )) && verdict=$code
    code=0
    judge "$crate" "no default features (--only-explicit-features)" --only-explicit-features ${args[@]+"${args[@]}"} || code=$?
    (( code > verdict )) && verdict=$code
    if [[ "$verdict" -eq 1 ]]; then
      echo "::error::semver: $crate $current breaks its public API against $previous; a breaking change bumps the minor while the crate is 0.x (.claude/rules/crates-publishing.md)." >&2
    fi
    if [[ "$verdict" -ne 0 ]]; then
      failed="$failed $crate"
      continue
    fi
    moved="$(moved_since "$crate" "$previous")"
    if [[ -n "$moved" ]] && same_line "$previous" "$current"; then
      echo "::error::semver: $crate moves from $previous to $current while a public dependency moves to another compatibility line ($moved); that breaks its consumers, so it bumps the minor (.claude/rules/crates-publishing.md)." >&2
      failed="$failed $crate"
      continue
    fi
    [[ -z "$moved" ]] || echo "semver: $crate leaves its line for its public dependencies ($moved)"
  done

  if [[ -n "$failed" ]]; then
    echo "semver: refused:$failed" >&2
    exit 1
  fi
  echo "semver: every judged version answers its change"
}

self_test() {
  local work status
  work="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand work now, so the trap removes this directory
  trap "rm -r -- '$work'" EXIT
  # A stand-in cargo: `semver-checks --version` answers PIN, and a judgement
  # of a crate named in BREAKS fails as the tool does on a break the version
  # leaves on the line of 0.2.0, or, under --only-explicit-features, of a
  # crate named in SUBSET_BREAKS.
  mkdir -p "$work/bin" "$work/repo/crates/pix-a" "$work/repo/docs" "$work/repo/scripts/release"
  cat > "$work/bin/cargo" <<'CARGO'
#!/usr/bin/env bash
[[ "$1" == semver-checks ]] || exit 3
[[ "$2" == --version ]] && { echo "cargo-semver-checks ${PIN}"; exit 0; }
crate="$4"
list="${BREAKS:-}"
[[ " $* " == *" --only-explicit-features "* ]] && list="${list} ${SUBSET_BREAKS:-}"
version="$(awk -F'"' '/^version/ { print $2; exit }' "crates/$crate/Cargo.toml")"
if [[ " $list " == *" $crate "* && "${version%.*}" == 0.2 ]]; then
  echo "--- failure function_missing: pub fn removed ---"
  echo "     Summary semver requires new minor version: 1 major and 0 minor checks failed"
  exit 1
fi
[[ "${BROKEN:-}" == "$crate" ]] && { echo "error: could not compile"; exit 101; }
echo "     Summary no semver update required"
CARGO
  chmod 0755 "$work/bin/cargo"
  local repo="$work/repo"
  local tick='`'
  printf '| Item | Pin |\n|---|---|\n| %scargo-semver-checks%s | %s0.51.0%s |\n' "$tick" "$tick" "$tick" "$tick" > "$repo/docs/VERSIONS.md"
  printf 'readonly CRATES=(\n  pix-a\n)\n' > "$repo/scripts/release/publish-crates.sh"
  printf '[workspace]\nmembers = ["crates/*"]\n\n[workspace.dependencies]\nfhir-types = "0.1.130"\n' > "$repo/Cargo.toml"
  # crate VERSION SMART: crates/pix-a at VERSION, requiring ferrohealth-smart
  # at SMART and the workspace fhir-types.
  crate() {
    printf '[package]\nname = "pix-a"\nversion = "%s"\n\n[dependencies]\nferrohealth-smart = { version = "%s", optional = true }\nfhir-types = { workspace = true }\nserde = "1"\n' "$1" "$2" > "$repo/crates/pix-a/Cargo.toml"
  }
  g() { git -C "$repo" -c user.name=self-test -c user.email=self-test@example.org -c commit.gpgsign=false -c core.hooksPath=/dev/null "$@"; }
  g init -q -b main
  crate 0.2.0 0.16.0
  g add -A
  g commit -q -m base
  # expect NAME WANT [ENV...] -- ARGS...: the judge exits WANT.
  expect() {
    local name="$1" want="$2" envs=()
    shift 2
    while [[ "$1" != -- ]]; do envs+=("$1"); shift; done
    shift
    status=0
    env PATH="$work/bin:$PATH" PIN=0.51.0 SEMVER_ROOT="$repo" SEMVER_RELEASES= ${envs[@]+"${envs[@]}"} \
      bash "$script" "$@" > "$work/out" 2>&1 || status=$?
    if [[ "$status" -ne "$want" ]]; then
      echo "semver: self-test failed: $name exited $status, wanted $want." >&2
      cat "$work/out" >&2
      exit 1
    fi
  }
  expect "the pin" 0 -- --pin
  grep -qx '0.51.0' "$work/out" || { echo "semver: self-test failed: the pin was not printed" >&2; exit 1; }
  expect "another installed version" 2 PIN=0.50.0 -- --baseline-rev main pix-a
  expect "a crate at its baseline's version" 0 -- --baseline-rev main pix-a
  grep -q 'nothing to judge' "$work/out" || { echo "semver: self-test failed: an unmoved crate was judged" >&2; exit 1; }
  crate 0.2.1 0.16.0
  expect "a compatible patch" 0 -- --baseline-rev main pix-a
  expect "a break on a patch" 1 BREAKS=pix-a -- --baseline-rev main pix-a
  grep -q 'breaks its public API against 0.2.0' "$work/out" || { echo "semver: self-test failed: the break was not named" >&2; exit 1; }
  expect "a break under the explicit features alone, on a patch" 1 SUBSET_BREAKS=pix-a -- --baseline-rev main pix-a
  grep -q 'only-explicit-features' "$work/out" || { echo "semver: self-test failed: the second pass did not run" >&2; exit 1; }
  expect "a crate the tool cannot build" 1 BROKEN=pix-a -- --baseline-rev main pix-a
  grep -q 'could not judge pix-a' "$work/out" || { echo "semver: self-test failed: a failed build read as a verdict" >&2; exit 1; }
  crate 0.3.0 0.16.0
  expect "a break on the next minor" 0 BREAKS=pix-a -- --baseline-rev main pix-a
  expect "a minor with no break" 0 -- --baseline-rev main pix-a
  crate 0.2.1 0.17.0
  expect "a public dependency on another line, on a patch" 1 -- --baseline-rev main pix-a
  grep -q 'ferrohealth-smart 0.16->0.17' "$work/out" || { echo "semver: self-test failed: the moved dependency was not named" >&2; exit 1; }
  crate 0.3.0 0.17.0
  expect "a public dependency on another line, on a minor" 0 -- --baseline-rev main pix-a
  crate 0.2.1 0.16.0
  sed -i.bak 's/^serde = "1"/serde = "2"/' "$repo/crates/pix-a/Cargo.toml" && rm "$repo/crates/pix-a/Cargo.toml.bak"
  expect "a private dependency on another line, on a patch" 0 -- --baseline-rev main pix-a
  crate 0.0.0 0.16.0
  expect "a name reservation" 0 -- --baseline-rev main pix-a
  grep -q 'name reservation' "$work/out" || { echo "semver: self-test failed: the reservation was not named" >&2; exit 1; }
  crate 0.2.1 0.16.0
  expect "a crate against its newest release" 1 BREAKS=pix-a "SEMVER_RELEASES=pix-a 0.2.0" -- pix-a
  expect "a crate with no release" 0 "SEMVER_RELEASES=other 1.0.0" -- pix-a
  expect "an unknown crate" 2 -- --baseline-rev main pix-b
  echo "semver: self-test passed."
}

case "${1:-}" in
  --self-test)
    [[ "$#" -eq 1 ]] || usage
    self_test
    ;;
  *) run "$@" ;;
esac
