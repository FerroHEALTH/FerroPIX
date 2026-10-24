#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# Every CI tool, corpus and test image pin no ecosystem watches, compared
# with its newest upstream release (no specification governs this: our own
# design, after FerroFED's lane of the same name). Dependabot's
# `github-actions` and `cargo` ecosystems read `uses:` references and the
# manifests; they do not read a version handed to an installer as an input,
# an image named in a `run:` block, a corpus a vendor script fetches, or an
# image the testkit starts from a Rust constant. scripts/checks/versions.sh
# catches drift between two files we control and cannot tell you that
# upstream published a newer release, which is what this reads.
#
#   scripts/checks/pin-freshness.sh
#
# The lint and install pins of .github/workflows/ci.yml (zizmor, shellcheck,
# the actionlint image, yq, cargo-leptos), the documentation toolchain and
# the console's Binaryen of docs/VERSIONS.md are read against the upstream
# project's newest GitHub release. A corpus of docs/VERSIONS.md pinned by commit on a repository with
# no releases is read against the newest commit of the branch it follows, a
# corpus pinned by a release tag against the newest tag of that shape, a FHIR
# package or implementation guide pinned by version against the versions its
# entry on the FHIR package registry lists, a document at an unversioned URL
# against the sha256 of the bytes served there now, an RFC against the RFC
# Editor's record of what obsoletes it, and a dated W3C publication against
# the newest version the W3C API lists. Every corpus row of docs/VERSIONS.md
# has a reader, or its vendor script is listed in UNWATCHED_SCRIPTS with the
# reason none is needed. Each row of the container image table of
# docs/VERSIONS.md, which the testkit holds its `PinnedImage` constants to,
# is read against the newest stable tag of its shape in the registry that
# serves it. Needs an authenticated `gh`, awk, curl, find, jq, sed and
# shasum.
#
#   scripts/checks/pin-freshness.sh --self-test
#
# Runs the tag and version comparisons and the readers over fixtures, and
# checks that every corpus row of docs/VERSIONS.md has a reader or a
# recorded reason and that every CI tool pin is found, offline.
#
# Exit 0 when every pin is current, 1 when at least one is behind (each such
# line starts with STALE), 2 when a release could not be read, so a network
# failure never reads as a fresh pin.
set -euo pipefail
cd "$(dirname "$0")/../.."

readonly MATRIX=docs/VERSIONS.md

# The workflow whose `run:` blocks and installer inputs name the CI tool pins.
readonly CI=.github/workflows/ci.yml
readonly REGISTRY_UA=ferropix-pin-check
# One "tool<TAB>upstream repository" record per line: a CI tool ci.yml pins
# by version, read against the newest GitHub release of its project.
# The wasm-bindgen CLI is not here: it equals the `wasm-bindgen` crate of
# the root Cargo.toml, which Dependabot's cargo ecosystem watches.
readonly CI_TOOLS="\
zizmor	zizmorcore/zizmor
shellcheck	koalaman/shellcheck
actionlint	rhysd/actionlint
yq	mikefarah/yq
cargo-leptos	leptos-rs/cargo-leptos"
# The most pages of one tag list the read follows before it gives up.
readonly MAX_TAG_PAGES=200

# One "matrix label<TAB>upstream repository<TAB>branch" record per line: a
# corpus pinned by commit on a repository that publishes no releases, read
# against the newest commit of the branch the pin follows.
readonly WATCHED_COMMITS="\
Nuts specifications	nuts-foundation/nuts-specification	master
did:web Method Specification	w3c-ccg/did-method-web	main
DIF Presentation Exchange 2.0.0	decentralized-identity/presentation-exchange	main
DIF Claim Format Registry	decentralized-identity/claim-format-registry	main"

# One "matrix label<TAB>upstream repository<TAB>tag prefix<TAB>hold" record
# per line: a corpus pinned by a release tag, read against the newest tag of
# the same shape, the prefix (`-` for none) and a dotted version. The IHE IUA
# supplement and the Dutch Generic Functions IG tag each release and publish
# no GitHub release. A held corpus is pinned at the release another pin binds
# (the hold says which), so a newer release of it is printed and is not a
# stale pin.
readonly WATCHED_TAGS="\
IHE IUA supplement	IHE/ITI.IUA	-	-
Netherlands Generic Functions IG source	nuts-foundation/nl-generic-functions-ig	v	-"

# One "matrix label<TAB>package<TAB>hold" record per line: a FHIR package, or
# the pages of the implementation guide it publishes, pinned by version and
# read against the versions its entry on the FHIR package registry lists. A
# held package is pinned at the version another pinned package depends on
# (the hold says which), so a newer release of it is printed and is not a
# stale pin by itself: the dependent's release moves it.
readonly FHIR_REGISTRY=https://packages.fhir.org
readonly WATCHED_PACKAGES="\
IHE PIXm FHIR package	ihe.iti.pixm	-
IHE PDQm FHIR package	ihe.iti.pdqm	-
IHE mCSD FHIR package	ihe.iti.mcsd	-
IHE PMIR FHIR package	ihe.iti.pmir	-
IHE BALP FHIR package	ihe.iti.balp	-
IHE PIXm narrative pages	ihe.iti.pixm	-
IHE PDQm narrative pages	ihe.iti.pdqm	-
IHE PMIR narrative pages	ihe.iti.pmir	-
IHE mCSD narrative pages	ihe.iti.mcsd	-
IHE BALP narrative pages	ihe.iti.balp	-"

# One "matrix label<TAB>URL" record per line: a document IHE publishes at a
# URL that names no revision, pinned by the sha256 of its bytes. Bytes that
# differ from the pin mean a new revision or an edit of the pinned one.
readonly WATCHED_DOCUMENTS="\
IHE ITI-20 Record Audit Event	https://profiles.ihe.net/ITI/TF/Volume2/ITI-20.html
IHE RESTful ATNA supplement	https://www.ihe.net/uploadedFiles/Documents/ITI/IHE_ITI_Suppl_RESTful-ATNA.pdf"

# Every corpus row labelled `IETF RFC <number>` is read against the RFC
# Editor's record of the RFC: an RFC is never revised in place, so it is
# stale only once a later RFC obsoletes it.
readonly RFC_INDEX=https://www.rfc-editor.org/rfc

# One "matrix label<TAB>W3C shortname<TAB>hold" record per line: a dated W3C
# publication, read against the newest version of its specification the W3C
# API lists.
readonly W3C_API=https://api.w3.org/specifications
readonly WATCHED_W3C="\
W3C Verifiable Credentials Data Model 1.1	vc-data-model	the version the pinned Netherlands Generic Functions IG cites
W3C Decentralized Identifiers 1.0	did-core	-
W3C DID Resolution 1.0	did-resolution	-
W3C Bitstring Status List 1.0	vc-bitstring-status-list	-"

# One "vendor script<TAB>reason" record per line: a script whose corpus rows
# no reader above watches, with the reason none is needed. The self-test
# fails on a corpus row with neither.
readonly UNWATCHED_SCRIPTS="\
mitz.sh	VZVZ publishes no release feed for the Mitz architecture documents
ihe-iti-tf.sh	the IHE ITI-20 Record Audit Event row reads the same ITI Technical Framework revision, and the script fails when an upstream byte of a Volume 1 or Volume 2 page moves
ch.sh	the Swiss pins are the IHE revisions EPDV-EDI Annex 5 names, so a newer IHE release is no refresh of them; the script fails when an upstream byte moves"

# corpus_rows [MATRIX]: one "label<TAB>script" record per row of MATRIX whose
# third cell names a scripts/vendor/ script, which is what makes it a corpus
# row.
corpus_rows() {
  awk -F'|' '
    NF >= 4 {
      label = $2; where = $4
      gsub(/`/, "", label); gsub(/^[ \t]+|[ \t]+$/, "", label)
      if (match(where, /scripts\/vendor\/[a-z0-9-]+\.sh/)) print label "\t" substr(where, RSTART + 15, RLENGTH - 15)
    }' "${1:-$MATRIX}"
}

# unwatched_rows [MATRIX]: the label of every corpus row of MATRIX that no
# reader watches and whose script UNWATCHED_SCRIPTS does not list, one per
# line.
unwatched_rows() {
  local label script watched
  watched="$(printf '%s\n' "$WATCHED_COMMITS" "$WATCHED_TAGS" "$WATCHED_PACKAGES" \
    "$WATCHED_DOCUMENTS" "$WATCHED_W3C" | cut -f1)"
  while IFS=$'\t' read -r label script; do
    [[ -n "$label" ]] || continue
    grep -qxF -- "$label" <<< "$watched" && continue
    [[ "$label" =~ ^IETF\ RFC\ [0-9]+$ ]] && continue
    awk -F'\t' -v s="$script" '$1 == s { found = 1 } END { exit !found }' <<< "$UNWATCHED_SCRIPTS" && continue
    printf '%s\n' "$label"
  done < <(corpus_rows "$@")
}

# matrix_cell_token LABEL KEY [MATRIX]: the token after the word KEY in the
# pin cell of the matrix row whose first cell is LABEL, backticks and
# trailing punctuation removed.
matrix_cell_token() {
  awk -F'|' -v want="$1" -v key="$2" '
    NF >= 3 {
      label = $2; value = $3
      gsub(/`/, "", label); gsub(/^[ \t]+|[ \t]+$/, "", label)
      if (label != want) next
      gsub(/`/, "", value)
      n = split(value, word, " ")
      for (i = 1; i < n; i++) if (word[i] == key) { t = word[i + 1]; gsub(/[,.;:]+$/, "", t); print t; exit }
    }' "${3:-$MATRIX}"
}

# matrix_first LABEL PATTERN [MATRIX]: the first match of the awk regular
# expression PATTERN in the pin cell of the matrix row whose first cell is
# LABEL, backticks removed.
matrix_first() {
  awk -F'|' -v want="$1" -v pattern="$2" '
    NF >= 3 {
      label = $2; value = $3
      gsub(/`/, "", label); gsub(/^[ \t]+|[ \t]+$/, "", label)
      gsub(/`/, "", value)
      if (label == want && match(value, pattern)) { print substr(value, RSTART, RLENGTH); exit }
    }' "${3:-$MATRIX}"
}

# One "matrix label<TAB>reason" record per line: an image compared only
# within the major line its tag names, because the consumer needs that line.
# The newest tag of a later line is still printed beside it.
readonly HELD_LINES="\
PostgreSQL test image	the PostgreSQL major version of the store"

# matrix_images [MATRIX]: one "label<TAB>repository<TAB>tag" record per row of
# MATRIX whose pin cell is a single `repository:tag@sha256:digest` reference,
# the shape of the container image table.
matrix_images() {
  awk -F'|' '
    NF >= 4 {
      label = $2; value = $3
      gsub(/`/, "", label); gsub(/^[ \t]+|[ \t]+$/, "", label)
      gsub(/`/, "", value); gsub(/^[ \t]+|[ \t]+$/, "", value)
      if (value !~ /^[^ :@]+(:[0-9]+)?(\/[^ :@]+)*:[^ :@]+@sha256:[0-9a-f]{64}$/) next
      ref = value; sub(/@sha256:.*$/, "", ref)
      tag = ref; sub(/^.*:/, "", tag)
      repo = substr(ref, 1, length(ref) - length(tag) - 1)
      print label "\t" repo "\t" tag
    }' "${1:-$MATRIX}"
}

# ci_tools [WORKFLOW]: one "name<TAB>upstream repository<TAB>pinned version"
# record per CI tool pin of WORKFLOW (ci.yml by default): each `tool:
# name@version` input of an installer step, the tag of the actionlint image
# in a `run:` block, and the yq release of its install step. A pin that is
# not found prints the name with an empty version, which the caller refuses.
ci_tools() {
  local workflow="${1:-$CI}" name repo version
  while IFS=$'\t' read -r name repo; do
    case "$name" in
      actionlint)
        version="$(sed -nE 's/.*rhysd\/actionlint:([0-9][0-9.]*)@sha256:.*/\1/p' "$workflow" | head -n 1)"
        ;;
      yq)
        version="$(sed -nE 's/^[[:space:]]*YQ_VERSION:[[:space:]]*v?([0-9][0-9.]*)[[:space:]]*$/\1/p' "$workflow" | head -n 1)"
        ;;
      *)
        version="$(sed -nE "s/^[[:space:]]*tool:[[:space:]]*$name@([0-9][0-9.]*)[[:space:]]*\$/\\1/p" "$workflow" | head -n 1)"
        ;;
    esac
    printf '%s\t%s\t%s\n' "$name" "$repo" "$version"
  done <<< "$CI_TOOLS"
}

# registry_tags REPOSITORY: every tag of the image, one per line, from the
# OCI distribution API (`GET /v2/<name>/tags/list`) with the anonymous pull
# token the registry's `WWW-Authenticate` challenge names. A repository with
# no registry host is a Docker Hub one, and a bare name is an official image.
registry_tags() {
  local repository="$1" host path challenge realm service token headers
  case "${repository%%/*}" in
    *.* | *:*)
      host="${repository%%/*}"
      path="${repository#*/}"
      ;;
    *)
      host=registry-1.docker.io
      path="$repository"
      [[ "$path" == */* ]] || path="library/$path"
      ;;
  esac
  challenge="$(curl --silent --show-error --user-agent "$REGISTRY_UA" --output /dev/null \
    --dump-header - "https://$host/v2/" | tr -d '\r' \
    | awk 'tolower($1) == "www-authenticate:" { sub(/^[^:]*:[ \t]*/, ""); print; exit }')" || return 1
  realm="$(sed -nE 's/.*realm="([^"]+)".*/\1/p' <<< "$challenge")"
  service="$(sed -nE 's/.*service="([^"]+)".*/\1/p' <<< "$challenge")"
  [[ -n "$realm" ]] || return 1
  token="$(curl --fail --silent --show-error --get --user-agent "$REGISTRY_UA" \
    --data-urlencode "service=$service" --data-urlencode "scope=repository:$path:pull" "$realm" \
    | jq --raw-output --exit-status '.token // .access_token')" || return 1
  headers="$registry_scratch/headers"
  # A registry may page the list whatever size is asked for (Quay stops at
  # 100), and names the next page in a `Link` header, so every page is read
  # and the list is never cut short. A page that fails fails the read.
  local url="https://$host/v2/$path/tags/list?n=100000" pages=0 next
  : > "$registry_scratch/tags.txt"
  while [[ -n "$url" ]]; do
    pages=$((pages + 1))
    [[ "$pages" -le "$MAX_TAG_PAGES" ]] || return 1
    curl --fail --silent --show-error --user-agent "$REGISTRY_UA" --dump-header "$headers" \
      --header "Authorization: Bearer $token" "$url" > "$registry_scratch/tags.json" || return 1
    jq --raw-output '.tags // [] | .[]' "$registry_scratch/tags.json" >> "$registry_scratch/tags.txt" \
      || return 1
    next="$(next_page "$headers")"
    case "$next" in
      '') url="" ;;
      https://*) url="$next" ;;
      /*) url="https://$host$next" ;;
      *) return 1 ;;
    esac
  done
  [[ -s "$registry_scratch/tags.txt" ]] || return 1
  cat "$registry_scratch/tags.txt"
}

# next_page HEADERS: the target of the `rel="next"` link in the response
# header file HEADERS (RFC 8288 §3), or nothing on the last page.
next_page() {
  tr -d '\r' < "$1" | awk '
    tolower($1) == "link:" && /rel="?next"?/ && match($0, /<[^>]+>/) {
      print substr($0, RSTART + 1, RLENGTH - 2); exit
    }'
}

# newest_release PINNED [LINE]: the newest tag read on stdin that is a stable
# release of the pinned tag's shape, within the major LINE when one is given.
# A release is a dotted version with an optional `_build` and the pinned
# suffix verbatim (`-eclipse-temurin-21`, `-jre-noble`), so a pre-release
# (`-rc1`, `beta1`) and a variant (`-alpine`) never match. A floating tag
# (`4.3`, `18`) names a line, not a release, so a release carries at least as
# many components as the pin, up to three: a Temurin respin adds a fourth.
newest_release() {
  awk -v pinned="$1" -v line="${2:-}" '
    function shape(tag) {
      if (!match(tag, /^[0-9]+(\.[0-9]+)*(_[0-9]+)?/)) return 0
      version = substr(tag, 1, RLENGTH); suffix = substr(tag, RLENGTH + 1)
      build = ""
      if (index(version, "_") > 0) { build = substr(version, index(version, "_") + 1); version = substr(version, 1, index(version, "_") - 1) }
      parts = split(version, part, ".")
      return 1
    }
    function key(   k, i) {
      k = ""
      for (i = 1; i <= 6; i++) k = k sprintf("%010d", (i <= parts ? part[i] : 0))
      return k sprintf("%010d", (build == "" ? 0 : build))
    }
    BEGIN {
      shape(pinned)
      want_suffix = suffix; want_build = (build != ""); want_parts = (parts < 3 ? parts : 3)
      best = ""; best_key = ""
    }
    shape($0) && suffix == want_suffix && (build != "") == want_build && parts >= want_parts \
      && (line == "" || part[1] == line) {
      k = key()
      if (best == "" || k > best_key) { best = $0; best_key = k }
    }
    END { if (best != "") print best }'
}

# newest_version PINNED: the newest version read on stdin, one per line, in
# the order of semantic versioning: the dotted release compared by number,
# and a pre-release (`1.0.0-ballot`) before the release it precedes. A
# pre-release counts only when the pin is one, so a ballot never reads as a
# newer release of a released pin, and the first release of a ballot pin
# does.
newest_version() {
  awk -v pinned="$1" '
    function key(v,   core, pre, n, p, i, k) {
      core = v; pre = ""
      if (index(v, "-") > 0) { core = substr(v, 1, index(v, "-") - 1); pre = substr(v, index(v, "-") + 1) }
      n = split(core, p, ".")
      k = ""
      for (i = 1; i <= 4; i++) k = k sprintf("%010d", (i <= n ? p[i] : 0))
      return k (pre == "" ? "~" : "-" pre)
    }
    BEGIN { pre_too = (index(pinned, "-") > 0); best = ""; best_key = "" }
    /^[0-9]+(\.[0-9]+)*(-[0-9A-Za-z.-]+)?$/ && (pre_too || index($0, "-") == 0) {
      k = key($0)
      if (best == "" || k > best_key) { best = $0; best_key = k }
    }
    END { if (best != "") print best }'
}

# matrix_package_version LABEL PACKAGE [MATRIX]: the version that follows the
# backticked PACKAGE in the pin cell of the matrix row whose first cell is
# LABEL, past a `version` word. A row that pins two versions names the newer
# first.
matrix_package_version() {
  awk -F'|' -v want="$1" -v package="\`$2\`" '
    NF >= 3 {
      label = $2; value = $3
      gsub(/`/, "", label); gsub(/^[ \t]+|[ \t]+$/, "", label)
      if (label != want) next
      n = split(value, word, " ")
      for (i = 1; i < n; i++) {
        if (word[i] != package) continue
        j = i + 1
        if (word[j] == "version" && j < n) j++
        v = word[j]; gsub(/`/, "", v); gsub(/[,;:]+$/, "", v)
        print v; exit
      }
    }' "${3:-$MATRIX}"
}

# The self-test runs newest_release, matrix_images and ci_tools over fixtures and
# reads nothing from the network, so a broken parser fails CI rather than
# reading every image as current.
self_test() {
  local failed=0 got work
  # expect WANT GOT WHAT: one comparison of the self-test.
  expect() {
    if [[ "$2" != "$1" ]]; then
      printf 'pin-freshness: self-test failed: %s gave "%s", wanted "%s".\n' "$3" "$2" "$1" >&2
      failed=1
    fi
  }
  got="$(printf '%s\n' 4.3.3 4.3 latest sha-4b271bc 4.4.0-rc1 4.3.10 4.3.9 | newest_release 4.3.3)"
  expect 4.3.10 "$got" "a release beside floating, commit and pre-release tags"
  got="$(printf '%s\n' 15.19 18 18.1 18.1-alpine 15.20 18beta1 | newest_release 15.19)"
  expect 18.1 "$got" "a two-component release across lines"
  got="$(printf '%s\n' 15.19 18 18.1 18.1-alpine 15.20 | newest_release 15.19 15)"
  expect 15.20 "$got" "a release held to its line"
  got="$(printf '%s\n' 21.0.12.1_1-jre-noble 21.0.12_7-jre-noble 21.0.13_8-jre-noble 21-jre-noble \
    21.0.13_8-jdk-noble | newest_release 21.0.12.1_1-jre-noble 21)"
  expect 21.0.13_8-jre-noble "$got" "a Temurin respin against the next update"
  got="$(printf '%s\n' 21.0.12.1_1-jre-noble 21.0.12_7-jre-noble | newest_release 21.0.12.1_1-jre-noble)"
  expect 21.0.12.1_1-jre-noble "$got" "a respin against the update it respins"
  got="$(printf '%s\n' 3.9.16-eclipse-temurin-21 3.9-eclipse-temurin-21 4.0.0-rc-4-eclipse-temurin-21 \
    3.9.17-eclipse-temurin-25 | newest_release 3.9.16-eclipse-temurin-21)"
  expect 3.9.16-eclipse-temurin-21 "$got" "a suffix that holds the Java release"
  got="$(printf '%s\n' latest main | newest_release 2.5.12)"
  expect "" "$got" "a repository with no release of the pinned shape"

  got="$(printf '%s\n' 1.0.0-ballot | newest_version 1.0.0-ballot)"
  expect 1.0.0-ballot "$got" "a ballot that is the only version"
  got="$(printf '%s\n' 1.0.0-ballot 1.0.0 | newest_version 1.0.0-ballot)"
  expect 1.0.0 "$got" "the first release of a ballot"
  got="$(printf '%s\n' 0.1.0 2.0.0-ballot 2.0.0 2.0.1 2.1.0-ballot | newest_version 2.0.1)"
  expect 2.0.1 "$got" "a later ballot of a released pin"
  got="$(printf '%s\n' 1.2.0 1.3.0 1.3.1 1.10.0 | newest_version 1.3.1)"
  expect 1.10.0 "$got" "a version compared by number"
  got="$(printf '%s\n' current 1.0.0-comment-2 | newest_version 1.0.0-ballot)"
  expect 1.0.0-comment-2 "$got" "a registry entry with a non-version key"

  work="$(mktemp -d)"
  # shellcheck disable=SC2016 # the backticks are Markdown, not a command substitution
  printf '%s\n' '| Item | Pin | Repeated in |' '|---|---|---|' \
    '| Hub image | `postgres:18.6@sha256:'"$(printf 'a%.0s' {1..64})"'` | x |' \
    '| Registry image | `ghcr.io/example/a:6.1.3@sha256:'"$(printf 'b%.0s' {1..64})"'` | x |' \
    '| Port image | `registry.example:5000/x/y:1.0@sha256:'"$(printf 'c%.0s' {1..64})"'` | x |' \
    '| No digest | `example/b:1.2.3` | x |' \
    '| Prose | the image `example/c:1.0@sha256:'"$(printf 'd%.0s' {1..64})"'` and more | x |' > "$work/matrix.md"
  got="$(matrix_images "$work/matrix.md" | tr '\t' ' ' | paste -sd ';' -)"
  rm "$work/matrix.md"
  expect "Hub image postgres 18.6;Registry image ghcr.io/example/a 6.1.3;Port image registry.example:5000/x/y 1.0" \
    "$got" "the image rows of a matrix"
  got="$(matrix_images "$MATRIX" | cut -f1 | paste -sd ';' -)"
  expect "PostgreSQL test image;Stub OpenID Provider image" "$got" "the image rows of $MATRIX"

  printf '%s\n' '      - uses: taiki-e/install-action@0000 # v2' '        with:' '          tool: zizmor@1.2.3' \
    '          tool: shellcheck@0.9.0' '            rhysd/actionlint:1.7.1@sha256:00 -color' \
    '          YQ_VERSION: v4.5.6' > "$work/ci.yml"
  got="$(ci_tools "$work/ci.yml" | cut -f1,3 | tr '\t' ' ' | paste -sd ';' -)"
  expect "zizmor 1.2.3;shellcheck 0.9.0;actionlint 1.7.1;yq 4.5.6;cargo-leptos " "$got" "the tool pins of a workflow"
  printf '%s\n' '          tool: cargo-nextest' > "$work/ci.yml"
  got="$(ci_tools "$work/ci.yml" | cut -f3 | paste -sd ';' -)"
  rm "$work/ci.yml"
  expect ";;;;" "$got" "a workflow with no tool pin"
  got="$(ci_tools | awk -F'\t' '$3 == "" { print $1 }' | paste -sd ';' -)"
  expect "" "$got" "the tools of $CI with no pin found"

  # shellcheck disable=SC2016 # the backticks are Markdown, not a command substitution
  printf '%s\n' '| Item | Pin | Repeated in |' '|---|---|---|' \
    '| A package | package `a.b` 1.0.0-ballot, pin-set digest `00` | x |' \
    '| Two | package `c.d` 1.3.1 and 1.3.0, pin-set digest `00` | x |' \
    '| Pages | the IG of package `e.f` 3.1.0 at `host/E/3.1.0/`, pin-set digest `00` | x |' \
    '| Worded | `g.h` version `1.1.4` from `packages.fhir.org` | x |' > "$work/matrix.md"
  got="$(for row in 'A package|a.b' 'Two|c.d' 'Pages|e.f' 'Worded|g.h' 'Two|a.b'; do
    printf '%s;' "$(matrix_package_version "${row%%|*}" "${row#*|}" "$work/matrix.md")"
  done)"
  rm "$work/matrix.md"
  expect "1.0.0-ballot;1.3.1;3.1.0;1.1.4;;" "$got" "the package versions of matrix rows"

  # shellcheck disable=SC2016 # the backticks are Markdown, not a command substitution
  printf '%s\n' '| Item | Pin | Repeated in |' '|---|---|---|' \
    '| Release | `org/spec` tag `Release-1.1.0`, the sources | `scripts/vendor/spec.sh` |' \
    '| Bare | `org/supp` tag `2.5`, the text | `scripts/vendor/supp.sh` |' \
    '| Doc | `https://example.org/a.html` sha256 `'"$(printf 'a%.0s' {1..64})"'` | `scripts/vendor/doc.sh` |' \
    '| Commit | `org/x` commit `'"$(printf 'b%.0s' {1..40})"'` | `scripts/vendor/x.sh` |' \
    '| A tool | 1.2.3 | `.github/workflows/ci.yml` |' > "$work/matrix.md"
  got="$(printf '%s;' "$(matrix_cell_token Release tag "$work/matrix.md")" \
    "$(matrix_cell_token Bare tag "$work/matrix.md")" "$(matrix_cell_token Doc tag "$work/matrix.md")" \
    "$(matrix_first Doc '[0-9a-f]{64}' "$work/matrix.md" | cut -c1-4)" \
    "$(matrix_first Commit '[0-9a-f]{40}' "$work/matrix.md" | cut -c1-4)")"
  expect "Release-1.1.0;2.5;;aaaa;bbbb;" "$got" "the tags, sha256 and commits of matrix rows"
  got="$(corpus_rows "$work/matrix.md" | tr '\t' ' ' | paste -sd ';' -)"
  expect "Release spec.sh;Bare supp.sh;Doc doc.sh;Commit x.sh" "$got" "the corpus rows of a matrix"

  # shellcheck disable=SC2016 # the backticks are Markdown, not a command substitution
  printf '%s\n' '| Item | Pin | Repeated in |' '|---|---|---|' \
    '| Nuts specifications | commit | `scripts/vendor/nuts-rfc.sh` |' \
    '| IETF RFC 9999 | the RFC | `scripts/vendor/ietf-oauth.sh` |' \
    '| A Swiss pin | pin-set digest | `scripts/vendor/ch.sh` |' \
    '| An unread corpus | commit | `scripts/vendor/new.sh` |' > "$work/matrix.md"
  got="$(unwatched_rows "$work/matrix.md" | paste -sd ';' -)"
  expect "An unread corpus" "$got" "a corpus row with no reader and no recorded reason"
  rm "$work/matrix.md"
  printf 'HTTP/2 200\r\nLink: </v2/a/b/tags/list?n=100&last=1.0>; rel="next"\r\n\r\n' > "$work/headers"
  expect "/v2/a/b/tags/list?n=100&last=1.0" "$(next_page "$work/headers")" "the next page a registry links"
  printf 'HTTP/2 200\r\ncontent-type: application/json\r\n\r\n' > "$work/headers"
  expect "" "$(next_page "$work/headers")" "the last page"
  rm "$work/headers"
  rmdir "$work"
  got="$(unwatched_rows "$MATRIX" | paste -sd ';' -)"
  expect "" "$got" "the corpus rows of $MATRIX with no reader and no recorded reason"

  [[ "$failed" -eq 0 ]] || exit 1
  echo "pin-freshness: self-test OK."
}

case "${1:-}" in
  --self-test)
    self_test
    exit 0
    ;;
  '') ;;
  *)
    echo "usage: scripts/checks/pin-freshness.sh [--self-test]" >&2
    exit 2
    ;;
esac

registry_scratch="$(mktemp -d)"
trap 'rm -f "$registry_scratch/headers" "$registry_scratch/tags.json" "$registry_scratch/tags.txt"; rmdir "$registry_scratch"' EXIT

# One "matrix label<TAB>upstream repository<TAB>hold" record per line: a row
# of the documentation toolchain of docs/VERSIONS.md. Every FerroHEALTH book
# builds with the same tools, so a newer release is printed and is not a
# stale pin of this repository alone.
readonly WATCHED_DOCS="\
mdBook	rust-lang/mdBook	the version the FerroHEALTH family book pins
mdbook-lint	joshrotenberg/mdbook-lint	the version the FerroHEALTH family book pins
lychee	lycheeverse/lychee	the version the FerroHEALTH family book pins"

# matrix_pin LABEL: the second cell of the matrix row whose first cell is
# LABEL, with the backticks stripped and only the first token kept, the same
# shape versions.sh reads.
matrix_pin() {
  local label="$1"
  awk -F'|' -v want="$label" '
    NF >= 3 {
      label = $2; value = $3
      gsub(/`/, "", label); gsub(/^[ \t]+|[ \t]+$/, "", label)
      gsub(/`/, "", value); gsub(/^[ \t]+|[ \t]+$/, "", value)
      if (label == want) { split(value, f, " "); print f[1]; exit }
    }' "$MATRIX"
}

# One "matrix label<TAB>upstream repository" record per line: a build tool
# of docs/VERSIONS.md a script installs by its release tag, read against the
# newest GitHub release of its project.
readonly WATCHED_TOOLS="\
Binaryen	WebAssembly/binaryen"

# release_pins: one "label<TAB>repository<TAB>pinned<TAB>hold" record per pin
# read against a GitHub release, the CI tools first.
release_pins() {
  local name repo version hold
  while IFS=$'\t' read -r name repo version; do
    printf '%s\t%s\t%s\t-\n' "$name" "$repo" "$version"
  done < <(ci_tools)
  while IFS=$'\t' read -r name repo hold; do
    printf '%s\t%s\t%s\t%s\n' "$name" "$repo" "$(matrix_pin "$name")" "$hold"
  done <<< "$WATCHED_DOCS"
  while IFS=$'\t' read -r name repo; do
    printf '%s\t%s\t%s\t-\n' "$name" "$repo" "$(matrix_pin "$name")"
  done <<< "$WATCHED_TOOLS"
}

stale=0
unreadable=0
while IFS=$'\t' read -r label repo pinned hold; do
  [[ -n "$label" ]] || continue

  if [[ -z "$pinned" ]]; then
    printf 'UNREADABLE %s: no pin in %s or %s\n' "$label" "$CI" "$MATRIX"
    unreadable=1
    continue
  fi

  if ! tag="$(gh api "repos/$repo/releases/latest" --jq '.tag_name' 2>&1)"; then
    printf 'UNREADABLE %s: could not read the newest release of %s (%s)\n' "$label" "$repo" "$tag"
    unreadable=1
    continue
  fi
  # A tag may lead with the repository's name: lychee tags its releases
  # lychee-vX.Y.Z.
  latest="${tag#"${repo##*/}"-}"
  latest="${latest#v}"

  if [[ "$pinned" = "$latest" ]]; then
    printf 'current    %s %s (%s)\n' "$label" "$pinned" "$repo"
  elif [[ "$hold" != "-" ]]; then
    printf 'held       %s: pinned %s, newest upstream release %s, held at %s\n' \
      "$label" "$pinned" "$latest" "$hold"
  else
    printf 'STALE      %s: pinned %s, newest upstream release %s (https://github.com/%s/releases/tag/%s)\n' \
      "$label" "$pinned" "$latest" "$repo" "$tag"
    stale=1
  fi
done < <(release_pins)

# The commit-pinned corpora, each against the head of its branch.
while IFS=$'\t' read -r label repo branch; do
  [[ -n "$label" ]] || continue

  pinned="$(matrix_first "$label" '[0-9a-f]{40}')"
  if [[ -z "$pinned" ]]; then
    printf 'UNREADABLE %s: no commit pin in %s\n' "$label" "$MATRIX"
    unreadable=1
    continue
  fi

  if ! head="$(gh api "repos/$repo/commits/$branch" --jq '.sha' 2>&1)"; then
    printf 'UNREADABLE %s: could not read the head of %s %s (%s)\n' "$label" "$repo" "$branch" "$head"
    unreadable=1
    continue
  fi

  if [[ "$pinned" = "$head" ]]; then
    printf 'current    %s %s (%s %s)\n' "$label" "$pinned" "$repo" "$branch"
  else
    printf 'STALE      %s: pinned %s, newest commit on %s %s (https://github.com/%s/commit/%s)\n' \
      "$label" "$pinned" "$branch" "$head" "$repo" "$head"
    stale=1
  fi
done <<< "$WATCHED_COMMITS"

# The tag-pinned corpora, each against the newest tag of its shape.
while IFS=$'\t' read -r label repo prefix hold; do
  [[ -n "$label" ]] || continue
  [[ "$prefix" != "-" ]] || prefix=""

  pinned_tag="$(matrix_cell_token "$label" tag)"
  pinned="${pinned_tag#"$prefix"}"
  if [[ -z "$pinned_tag" || "$pinned" = "$pinned_tag" && -n "$prefix" ]] \
    || ! [[ "$pinned" =~ ^[0-9]+(\.[0-9]+)*$ ]]; then
    printf 'UNREADABLE %s: no %sX.Y tag pin in %s\n' "$label" "$prefix" "$MATRIX"
    unreadable=1
    continue
  fi

  if ! tags="$(gh api --paginate "repos/$repo/tags" --jq '.[].name' 2>&1)"; then
    printf 'UNREADABLE %s: could not read the tags of %s (%s)\n' "$label" "$repo" "$tags"
    unreadable=1
    continue
  fi
  latest="$(sed -nE "s/^$prefix([0-9]+(\.[0-9]+)+)$/\1/p" <<< "$tags" | newest_version "$pinned")"
  if [[ -z "$latest" ]]; then
    printf 'UNREADABLE %s: %s has no %sX.Y tag\n' "$label" "$repo" "$prefix"
    unreadable=1
  elif [[ "$pinned" = "$latest" ]]; then
    printf 'current    %s %s (%s)\n' "$label" "$pinned_tag" "$repo"
  elif [[ "$hold" != "-" ]]; then
    printf 'held       %s: pinned %s, newest release tag %s%s, held at %s\n' \
      "$label" "$pinned_tag" "$prefix" "$latest" "$hold"
  else
    printf 'STALE      %s: pinned %s, newest release tag %s%s (https://github.com/%s/tree/%s%s)\n' \
      "$label" "$pinned_tag" "$prefix" "$latest" "$repo" "$prefix" "$latest"
    stale=1
  fi
done <<< "$WATCHED_TAGS"

# The FHIR packages and the guides they publish, each against its registry
# entry.
while IFS=$'\t' read -r label package hold; do
  [[ -n "$label" ]] || continue

  pinned="$(matrix_package_version "$label" "$package")"
  if [[ -z "$pinned" ]]; then
    printf 'UNREADABLE %s: no version of %s in %s\n' "$label" "$package" "$MATRIX"
    unreadable=1
    continue
  fi

  if ! versions="$(curl --fail --silent --show-error --user-agent "$REGISTRY_UA" "$FHIR_REGISTRY/$package" \
    | jq --raw-output --exit-status '.versions | keys[]' 2>&1)"; then
    printf 'UNREADABLE %s: could not read the registry entry of %s (%s)\n' "$label" "$package" "$versions"
    unreadable=1
    continue
  fi
  latest="$(newest_version "$pinned" <<< "$versions")"
  if [[ -z "$latest" ]]; then
    printf 'UNREADABLE %s: the registry entry of %s lists no version of the shape of %s\n' "$label" "$package" "$pinned"
    unreadable=1
  elif [[ "$pinned" = "$latest" ]]; then
    printf 'current    %s %s %s (%s/%s)\n' "$label" "$package" "$pinned" "$FHIR_REGISTRY" "$package"
  elif [[ "$hold" != "-" ]]; then
    printf 'held       %s: %s pinned %s, newest release %s, held at %s\n' \
      "$label" "$package" "$pinned" "$latest" "$hold"
  else
    printf 'STALE      %s: %s pinned %s, newest release %s (%s/%s)\n' \
      "$label" "$package" "$pinned" "$latest" "$FHIR_REGISTRY" "$package"
    stale=1
  fi
done <<< "$WATCHED_PACKAGES"

# The documents at unversioned URLs, each against the bytes served now.
while IFS=$'\t' read -r label url; do
  [[ -n "$label" ]] || continue

  pinned="$(matrix_first "$label" '[0-9a-f]{64}')"
  if [[ -z "$pinned" ]]; then
    printf 'UNREADABLE %s: no sha256 pin in %s\n' "$label" "$MATRIX"
    unreadable=1
    continue
  fi

  if ! served="$(curl --fail --silent --show-error --location --user-agent "$REGISTRY_UA" "$url" \
    | shasum -a 256 | cut -d' ' -f1)" || ! [[ "$served" =~ ^[0-9a-f]{64}$ ]]; then
    printf 'UNREADABLE %s: could not read %s\n' "$label" "$url"
    unreadable=1
    continue
  fi
  if [[ "$pinned" = "$served" ]]; then
    printf 'current    %s sha256 %s (%s)\n' "$label" "$pinned" "$url"
  else
    printf 'STALE      %s: pinned sha256 %s, %s now serves %s, a new revision or an edit\n' \
      "$label" "$pinned" "$url" "$served"
    stale=1
  fi
done <<< "$WATCHED_DOCUMENTS"

# The RFCs, each against what the RFC Editor records as obsoleting it.
while IFS=$'\t' read -r label _; do
  [[ "$label" =~ ^IETF\ RFC\ ([0-9]+)$ ]] || continue
  number="${BASH_REMATCH[1]}"
  if ! obsoleted="$(curl --fail --silent --show-error --user-agent "$REGISTRY_UA" "$RFC_INDEX/rfc$number.json" \
    | jq --raw-output --exit-status '.obsoleted_by | join(", ")' 2>&1)"; then
    printf 'UNREADABLE %s: could not read the RFC Editor record %s/rfc%s.json (%s)\n' \
      "$label" "$RFC_INDEX" "$number" "$obsoleted"
    unreadable=1
  elif [[ -z "$obsoleted" ]]; then
    printf 'current    %s, obsoleted by none (%s/rfc%s.json)\n' "$label" "$RFC_INDEX" "$number"
  else
    printf 'STALE      %s: obsoleted by %s (%s/rfc%s.json)\n' "$label" "$obsoleted" "$RFC_INDEX" "$number"
    stale=1
  fi
done < <(corpus_rows "$MATRIX")

# The dated W3C publications, each against the newest version of its
# specification.
while IFS=$'\t' read -r label shortname hold; do
  [[ -n "$label" ]] || continue

  pinned="$(matrix_first "$label" 'https://www\.w3\.org/TR/[^ ,;]+')"
  if [[ -z "$pinned" ]]; then
    printf 'UNREADABLE %s: no W3C publication URL in %s\n' "$label" "$MATRIX"
    unreadable=1
    continue
  fi

  if ! latest="$(curl --fail --silent --show-error --location --user-agent "$REGISTRY_UA" \
    --header 'Accept: application/json' "$W3C_API/$shortname/versions/latest" \
    | jq --raw-output --exit-status '.uri' 2>&1)"; then
    printf 'UNREADABLE %s: could not read the newest version of %s from %s (%s)\n' \
      "$label" "$shortname" "$W3C_API" "$latest"
    unreadable=1
  elif [[ "$pinned" = "$latest" ]]; then
    printf 'current    %s %s\n' "$label" "$pinned"
  elif [[ "$hold" != "-" ]]; then
    printf 'held       %s: pinned %s, newest version %s, held at %s\n' "$label" "$pinned" "$latest" "$hold"
  else
    printf 'STALE      %s: pinned %s, newest version %s\n' "$label" "$pinned" "$latest"
    stale=1
  fi
done <<< "$WATCHED_W3C"

# A corpus row with no reader and no recorded reason is a pin nobody reads.
while IFS= read -r label; do
  [[ -n "$label" ]] || continue
  printf 'UNREADABLE %s: no freshness reader, and its vendor script is not in UNWATCHED_SCRIPTS\n' "$label"
  unreadable=1
done < <(unwatched_rows "$MATRIX")

# The container images of the matrix, each against the registry that serves
# it.
images="$(matrix_images)"
if [[ -z "$images" ]]; then
  printf 'UNREADABLE container images: no image row in %s\n' "$MATRIX"
  unreadable=1
fi
while IFS=$'\t' read -r label repository tag; do
  [[ -n "$label" ]] || continue
  if ! tags="$(registry_tags "$repository")"; then
    printf 'UNREADABLE %s: could not read the tags of %s\n' "$label" "$repository"
    unreadable=1
    continue
  fi

  held="$(awk -F'\t' -v want="$label" '$1 == want { print $2; exit }' <<< "$HELD_LINES")"
  newest="$(newest_release "$tag" <<< "$tags")"
  compared="$newest"
  scope="newest stable tag"
  if [[ -n "$held" ]]; then
    compared="$(newest_release "$tag" "${tag%%[!0-9]*}" <<< "$tags")"
    scope="newest stable tag of the ${tag%%[!0-9]*} line, held for $held"
  fi
  if [[ -z "$compared" ]]; then
    printf 'UNREADABLE %s: %s has no stable tag of the shape of %s\n' "$label" "$repository" "$tag"
    unreadable=1
    continue
  fi

  later=""
  [[ "$newest" = "$compared" ]] || later="; newest stable tag of any line $newest"
  if [[ "$compared" = "$tag" ]]; then
    printf 'current    %s %s:%s%s\n' "$label" "$repository" "$tag" "$later"
  else
    printf 'STALE      %s: %s pinned %s, %s %s%s\n' \
      "$label" "$repository" "$tag" "$scope" "$compared" "$later"
    stale=1
  fi
done <<< "$images"

[[ "$unreadable" -eq 0 ]] || exit 2
exit "$stale"
