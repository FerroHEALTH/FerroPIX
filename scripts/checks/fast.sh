#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# The default local gate: rustfmt, clippy, nextest and the doctests on the
# workspace packages a change reaches, and a check of the fuzz crate when the
# change reaches a crate it builds on. CI stays the full gate: its Rust lanes
# run every package on every pull request (.github/workflows/ci.yml).
# No specification governs this: our own design.
#
# A package is reached when the change touches a file under its directory,
# or when it depends, directly or through other workspace packages, on one
# that is. A change to any other file reaches every package, because the
# packages read the root manifests, the toolchain and lint files, and files
# under conformance/, website/ and the root CHANGELOG.md, except for the files no
# build reads: the ones under .claude/, .github/, changelog.d/, docs/,
# scripts/ and security/, and the root's other Markdown.
#
# Usage:
#   scripts/checks/fast.sh [--base <rev>] [--all] [--dry-run]
#   scripts/checks/fast.sh --self-test
#
#   --base <rev>   measure from this revision instead of the merge base
#                  with origin/main
#   --all          every package, as CI runs them
#   --dry-run      print the plan and run nothing
#   --self-test    prove the plan over a fixture workspace and run nothing
#
# The change is measured from the base to the working tree, untracked files
# included. Each step is one cargo invocation over the whole reached set, so
# a shared dependency builds once. A member whose name the lock holds twice,
# as while a published crate moves ahead of the release the server pins, is
# named `name@version`, since cargo refuses a bare `-p` for it; `cargo fmt`
# refuses that spelling and reads only workspace members, so it takes the name.
# Exit 0 when every step passed, 1 when one failed, 2 on a usage error.
set -euo pipefail
self="$(cd "$(dirname "$0")" && pwd -P)/$(basename "$0")"

usage() {
  sed -n '/^# Usage:/,/^# The change is/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//' >&2
  exit 2
}

# `name=name@version` for each workspace member whose name cargo metadata on
# stdin lists more than once.
duplicated_members() {
  jq -r '.workspace_members as $members
    | [.packages | group_by(.name)[] | select(length > 1) | .[0].name] as $twice
    | .packages[]
    | select(.id as $id | $members | index($id))
    | select(.name as $name | $twice | index($name))
    | "\(.name)=\(.name)@\(.version)"'
}

# The `-p` value of package $1: its entry in $specs, or its bare name.
spec() {
  local named
  named="$(printf '%s\n' "$specs" | sed -n "s/^$1=//p")"
  printf '%s\n' "${named:-$1}"
}

# The `-p` value of package $1 for step $2: `cargo fmt` takes the bare name,
# every other step its entry in $specs.
step_spec() {
  if [[ "$2" = fmt ]]; then printf '%s\n' "$1"; else spec "$1"; fi
}

# The `-p` spellings over a metadata fixture: a member whose name a
# published package shares, a member with a name of its own, a dependency,
# and an empty workspace.
spec_self_test() {
  local metadata got
  metadata='{"workspace_members":["path+file:///w/crates/ihe-iti#0.31.0","path+file:///w/crates/nl-generic-functions#0.31.0"],
    "packages":[
      {"name":"ihe-iti","version":"0.31.0","id":"path+file:///w/crates/ihe-iti#0.31.0"},
      {"name":"ihe-iti","version":"0.30.2","id":"registry+https://github.com/rust-lang/crates.io-index#ihe-iti@0.30.2"},
      {"name":"nl-generic-functions","version":"0.31.0","id":"path+file:///w/crates/nl-generic-functions#0.31.0"},
      {"name":"serde","version":"1.0.0","id":"registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0"}]}'
  specs="$(printf '%s' "$metadata" | duplicated_members)"
  got="$(spec ihe-iti) $(spec nl-generic-functions) $(spec serde)"
  if [[ "$got" != "ihe-iti@0.31.0 nl-generic-functions serde" ]]; then
    echo "fast: self-test failed: expected 'ihe-iti@0.31.0 nl-generic-functions serde', got '$got'." >&2
    return 1
  fi
  got="$(step_spec ihe-iti fmt) $(step_spec ihe-iti clippy)"
  if [[ "$got" != "ihe-iti ihe-iti@0.31.0" ]]; then
    echo "fast: self-test failed: expected 'ihe-iti ihe-iti@0.31.0' for fmt and clippy, got '$got'." >&2
    return 1
  fi
  specs="$(printf '%s' '{"workspace_members":[],"packages":[]}' | duplicated_members)"
  if [[ "$(spec ihe-iti)" != ihe-iti ]]; then
    echo "fast: self-test failed: an empty workspace should leave every name bare." >&2
    return 1
  fi
}

# The self-test builds a fixture workspace of two crates, `core` and `edge`,
# where `edge` depends on `core`, copies this script into it and reads the
# plan of a dry run for each kind of change: a file of `edge` reaches `edge`
# alone, a file of `core` reaches both, a root manifest reaches both, and a
# file under docs/ reaches none. It builds nothing and needs no network.
self_test() {
  local work failed=0
  work="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand $work now: the local is gone at EXIT.
  trap "rm -rf '$work'" EXIT
  export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
  mkdir -p "$work/scripts/checks" "$work/core/src" "$work/edge/src" "$work/docs"
  cp "$self" "$work/scripts/checks/fast.sh"
  printf '[workspace]\nmembers = ["core", "edge"]\nresolver = "3"\n' > "$work/Cargo.toml"
  printf '[package]\nname = "core"\nversion = "0.0.0"\nedition = "2024"\n' > "$work/core/Cargo.toml"
  printf '[package]\nname = "edge"\nversion = "0.0.0"\nedition = "2024"\n\n[dependencies]\ncore = { path = "../core" }\n' > "$work/edge/Cargo.toml"
  : > "$work/core/src/lib.rs"
  : > "$work/edge/src/lib.rs"
  printf 'notes\n' > "$work/docs/notes.md"
  (cd "$work" && cargo generate-lockfile --offline --quiet && git init -q && git add -A &&
    git -c user.name=fixture -c user.email=fixture@example.invalid commit -q -m fixture)
  # reached NAME FILE WANT: a change to FILE reaches the packages WANT.
  reached() {
    local name="$1" file="$2" want="$3" got
    (cd "$work" && git checkout -q -- . && printf '\n' >> "$file")
    got="$(cd "$work" && bash scripts/checks/fast.sh --base HEAD --dry-run 2>&1 |
      sed -n 's/^fast: reached: //p; s/^fast: the change reaches no package.*/none/p' |
      tr ' ' '\n' | sed '/^$/d' | sort | tr '\n' ' ')"
    if [[ "$got" != "$want" ]]; then
      echo "fast: self-test failed: $name reached '$got', wanted '$want'." >&2
      failed=1
    fi
  }
  reached "a dependant's file" edge/src/lib.rs "edge "
  reached "a dependency's file" core/src/lib.rs "core edge "
  reached "a root manifest" Cargo.toml "core edge "
  reached "a document" docs/notes.md "none "
  spec_self_test || failed=1
  if [[ "$failed" -ne 0 ]]; then
    exit 1
  fi
  echo "fast: self-test OK (a dependant, a dependency, a root manifest and a document; the bare and name@version spellings)."
}

base=""
all=false
dry=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --base)
      [[ $# -ge 2 ]] || usage
      base="$2"
      shift 2
      ;;
    --all)
      all=true
      shift
      ;;
    --dry-run)
      dry=true
      shift
      ;;
    --self-test)
      self_test
      exit 0
      ;;
    *) usage ;;
  esac
done
cd "$(dirname "$self")/../.."

root="$(pwd -P)"
metadata="$(cargo metadata --format-version 1 --locked)"

# Every workspace package as `name<TAB>directory`, the directory relative to
# the root.
packages_and_dirs="$(jq -r --arg root "$root/" '
  .workspace_members as $members
  | .packages[]
  | select(.id as $id | $members | index($id))
  | [.name, (.manifest_path | ltrimstr($root) | sub("/Cargo.toml$"; ""))]
  | @tsv
' <<<"$metadata")"

if [[ "$all" = false ]]; then
  if [[ -z "$base" ]]; then
    base="$(git merge-base origin/main HEAD)" || {
      echo "fast: no merge base with origin/main; fetch it or pass --base <rev>" >&2
      exit 1
    }
  fi
  changed="$({
    git diff --name-only "$base" --
    git ls-files --others --exclude-standard
  } | sort -u)"
  touched=""
  while IFS= read -r file; do
    [[ -n "$file" ]] || continue
    owner="$(awk -F'\t' -v file="$file" '
      index(file, $2 "/") == 1 && length($2) > best { best = length($2); name = $1 }
      END { print name }
    ' <<<"$packages_and_dirs")"
    if [[ -n "$owner" ]]; then
      touched="$touched $owner"
      continue
    fi
    case "$file" in
      .claude/* | .github/* | changelog.d/* | docs/* | scripts/* | security/*) ;;
      CHANGELOG.md) all=true ;;
      */*) all=true ;;
      *.md) ;;
      *) all=true ;;
    esac
  done <<<"$changed"
fi

if [[ "$all" = true ]]; then
  reached="$(cut -f1 <<<"$packages_and_dirs" | sort -u | tr '\n' ' ')"
else
  # The touched packages and every workspace package that depends on one, to
  # a fixed point over the resolved dependency graph.
  reached="$(jq -r --arg touched "$touched" '
    .workspace_members as $members
    | ([.packages[] | {key: .id, value: .name}] | from_entries) as $names
    | [.resolve.nodes[] | select(.id as $id | $members | index($id))
       | {name: $names[.id], deps: [.dependencies[] | $names[.]]}] as $nodes
    | ($touched | split(" ") | map(select(. != ""))) as $seed
    | def grow($set):
        ($set + [$nodes[] | select(any(.deps[]; . as $d | $set | index($d))) | .name]
         | unique) as $next
        | if ($next | length) == ($set | length) then $set else grow($next) end;
      grow($seed | unique) | join(" ")
  ' <<<"$metadata")"
fi

if [[ -z "${reached// /}" ]]; then
  echo "fast: the change reaches no package; nothing to run"
  exit 0
fi
echo "fast: reached: $reached"

run() {
  echo "+ $*"
  if [[ "$dry" = false ]]; then
    "$@"
  fi
}

specs="$(duplicated_members <<<"$metadata")"
args=()
fmt_args=()
for package in $reached; do
  args+=(-p "$(step_spec "$package" cargo)")
  fmt_args+=(-p "$(step_spec "$package" fmt)")
done
run cargo fmt --check ${fmt_args[@]+"${fmt_args[@]}"}
run cargo clippy --locked ${args[@]+"${args[@]}"} --all-targets --all-features -- -D warnings
run cargo nextest run --locked ${args[@]+"${args[@]}"} --no-tests=pass
# `cargo test --doc` refuses a package without a library target.
libraries=" $(jq -r '
  .workspace_members as $members
  | .packages[]
  | select(.id as $id | $members | index($id))
  | select(any(.targets[]; any(.kind[]; . == "lib" or . == "rlib")))
  | .name
' <<<"$metadata" | tr '\n' ' ')"
docs=()
for package in $reached; do
  case "$libraries" in *" $package "*) docs+=(-p "$(step_spec "$package" doc)") ;; esac
done
if [[ "${#docs[@]}" -gt 0 ]]; then
  run cargo test --doc --locked ${docs[@]+"${docs[@]}"}
fi
# The fuzz crate sits outside the workspace (fuzz/Cargo.toml), so the steps
# above never compile it; it is checked when the change reaches a crate it
# takes by path, on the stable toolchain, since a check runs no sanitizer.
fuzz_paths=" "
if [[ -f fuzz/Cargo.toml ]]; then
  fuzz_paths=" $(cargo metadata --manifest-path fuzz/Cargo.toml --format-version 1 --no-deps --locked |
    jq -r '.packages[].dependencies[] | select(.path != null) | .name' | tr '\n' ' ')"
fi
for package in $reached; do
  case "$fuzz_paths" in
    *" $package "*)
      run cargo check --locked --manifest-path fuzz/Cargo.toml --bins
      break
      ;;
  esac
done
if [[ "$dry" = true ]]; then
  echo "fast: dry run, nothing ran"
else
  echo "fast: every step passed"
fi
