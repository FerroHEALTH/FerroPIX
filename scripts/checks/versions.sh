#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# The pin and licence guard (no specification governs this: our own design).
#
#   1. Toolchain: the toolchain, MSRV and edition rows of docs/VERSIONS.md
#      agree with rust-toolchain.toml, the root Cargo.toml and rustfmt.toml,
#      each checked once the file exists.
#   2. Corpora: every tree under docs/specs/ carries a PROVENANCE.md and is
#      named in a row of the corpus table of docs/VERSIONS.md.
#   3. Tool pins: each row REPEATED_ROWS names is carried word for word by
#      every file its "Repeated in" cell names.
#   3b. FerroUI: the root Cargo.toml pins ferrohealth-ui at the pin of the
#      FerroUI row word for word, and the tag that row reads the FerroUI
#      documents at is `v` plus the pinned version.
#   4. The quickstart: the image tags compose.yaml defaults to are the
#      [workspace.package] version of the root Cargo.toml, and so is the
#      appVersion of the Helm chart, the default tag of its images, and
#      every `git clone` of FerroPIX in README.md names `--branch v` plus
#      that version, so a reader clones the configuration the pinned image
#      reads (#619).
#   5. Licence: every tracked first-party file names Cadasto B.V. in an
#      `SPDX-FileCopyrightText` line within its first 25 lines, and no
#      header there claims a licence other than BUSL-1.1. LICENSE, NOTICE,
#      the lockfiles, JSON (which has no comment syntax), the raster images
#      (PNG and ICO, which have none either), test fixtures and the fuzz
#      seeds (bytes a target reads, written from the vendored examples),
#      the HL7 v2 messages of the performance gate (bytes the tool sends)
#      and the changelog fragments (text the release cut moves into
#      CHANGELOG.md, which carries the header) carry no header.
#
# Usage:
#   scripts/checks/versions.sh [--root DIR]   check this repository, or DIR
#   scripts/checks/versions.sh --self-test    prove each refusal on fixtures
#
# Exit 0 when everything agrees, 1 on drift, 2 on a usage error.
set -euo pipefail
self="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
root="$(dirname "$0")/../.."

# The pin of the docs/VERSIONS.md row whose first cell is $1, backticks off.
pin() {
  local item="$1"
  awk -F'|' -v item="$item" '{ gsub(/^[[:space:]]+|[[:space:]]+$/, "", $2) }
    $2 == item { v = $3; gsub(/[[:space:]`]/, "", v); print v; exit }' docs/VERSIONS.md
}

# The rows whose pin each file their "Repeated in" cell names must carry
# word for word: the build and test tools a workflow or a script installs.
readonly REPEATED_ROWS=(
  "Nightly toolchain (fuzz lane)"
  "cargo-fuzz"
  "thirtyfour"
  "Chrome for Testing (Chrome and chromedriver)"
  "Chrome for Testing, \`chrome-linux64.zip\` SHA-256"
  "Chrome for Testing, \`chrome-mac-arm64.zip\` SHA-256"
  "Chrome for Testing, \`chromedriver-linux64.zip\` SHA-256"
  "Chrome for Testing, \`chromedriver-mac-arm64.zip\` SHA-256"
  "distroless static base"
  "PostgreSQL quickstart image"
  "Quickstart issuer image"
  "hadolint image"
  "Trivy"
  "cargo-auditable"
  "ferrohealth-perf"
  "cargo-cyclonedx"
  "syft"
)

# Every backticked path of the "Repeated in" cell of the row whose first
# cell is $1, one per line.
repeats() {
  local item="$1"
  awk -F'|' -v item="$item" '{ gsub(/^[[:space:]]+|[[:space:]]+$/, "", $2) }
    $2 == item { n = split($4, part, "`"); for (i = 2; i <= n; i += 2) print part[i]; exit }' docs/VERSIONS.md
}

# The value of key $2 in the TOML file $1, read as a quoted string.
toml_value() {
  local file="$1" key="$2"
  awk -v key="$key" '$1 == key { if (match($0, /"[^"]*"/)) { print substr($0, RSTART + 1, RLENGTH - 2); exit } }' "$file"
}

# The self-test builds one fixture repository per case and runs the guard
# over it.
self_test() {
  local work n=0 failed=0 d
  work="$(mktemp -d)"
  # case_dir: a fresh, passing fixture repository in $d.
  case_dir() {
    n=$((n + 1))
    d="$work/$n"
    mkdir -p "$d/docs"
    git -C "$d" init -q
    # shellcheck disable=SC2016 # the backticks are Markdown in the fixture
    printf '<!-- SPDX-FileCopyrightText: Cadasto B.V. -->\n| Item | Pin |\n|---|---|\n| Rust toolchain | `1.98.1` |\n| Minimum supported Rust version | `1.97` |\n| Edition | `2024` |\n' > "$d/docs/VERSIONS.md"
    printf '# SPDX-FileCopyrightText: Cadasto B.V.\n# SPDX-License-Identifier: BUSL-1.1\n[toolchain]\nchannel = "1.98.1"\n' > "$d/rust-toolchain.toml"
    printf '# SPDX-FileCopyrightText: Cadasto B.V.\n# SPDX-License-Identifier: BUSL-1.1\n[workspace.package]\nedition = "2024"\nrust-version = "1.97"\n' > "$d/Cargo.toml"
    printf 'MIT text\n' > "$d/LICENSE"
    git -C "$d" add -A
  }
  # expect WANT NAME: the guard over $d exits WANT.
  expect() {
    local want="$1" name="$2" status=0
    git -C "$d" add -A
    bash "$self" --root "$d" > /dev/null 2>&1 || status=$?
    if [[ "$status" -ne "$want" ]]; then
      echo "versions: self-test failed: $name exited $status, wanted $want." >&2
      failed=1
    fi
  }
  case_dir
  expect 0 "a repository that agrees"
  case_dir
  sed -i.bak 's/1.98.1/1.98.0/' "$d/rust-toolchain.toml" && rm "$d/rust-toolchain.toml.bak"
  expect 1 "a toolchain behind its pin"
  case_dir
  sed -i.bak 's/"1.97"/"1.96"/' "$d/Cargo.toml" && rm "$d/Cargo.toml.bak"
  expect 1 "an MSRV off its pin"
  case_dir
  printf 'edition = "2021"\n' > "$d/rustfmt.toml"
  expect 1 "a rustfmt edition off its pin"
  case_dir
  printf 'echo hi\n' > "$d/run.sh"
  expect 1 "a first-party file without the header"
  case_dir
  printf '# SPDX-FileCopyrightText: Cadasto B.V.\n# SPDX-License-Identifier: MIT\n' > "$d/run.sh"
  expect 1 "a first-party file claiming another licence"
  case_dir
  mkdir -p "$d/docs/specs/ihe-pixm"
  printf 'upstream text\nSPDX-License-Identifier: CC-BY-4.0\n' > "$d/docs/specs/ihe-pixm/page.md"
  expect 1 "a vendored tree without a provenance"
  printf 'provenance\n' > "$d/docs/specs/ihe-pixm/PROVENANCE.md"
  expect 1 "a vendored tree without a row"
  # shellcheck disable=SC2016 # the backticks are Markdown in the fixture
  printf '| IHE PIXm package | 3.1.0 | `docs/specs/ihe-pixm/PROVENANCE.md` |\n' >> "$d/docs/VERSIONS.md"
  expect 0 "a vendored tree with its provenance and its row"
  case_dir
  mkdir -p "$d/scripts"
  # shellcheck disable=SC2016 # the backticks are Markdown in the fixture
  printf '| cargo-fuzz | 0.13.1 | `scripts/tools.sh` |\n' >> "$d/docs/VERSIONS.md"
  printf '# SPDX-FileCopyrightText: Cadasto B.V.\n# SPDX-License-Identifier: BUSL-1.1\nversion="0.13.1"\n' > "$d/scripts/tools.sh"
  expect 0 "a tool pin its script repeats"
  sed -i.bak 's/0\.13\.1/0.13.0/' "$d/scripts/tools.sh" && rm "$d/scripts/tools.sh.bak"
  expect 1 "a tool pin its script does not repeat"
  case_dir
  mkdir -p "$d/scripts/release"
  # shellcheck disable=SC2016 # the backticks are Markdown in the fixture
  printf '| Trivy | `0.76.1` | `scripts/release/install-trivy.sh` |\n' >> "$d/docs/VERSIONS.md"
  # shellcheck disable=SC2016 # the stub is a literal script, never expanded here
  printf '#!/usr/bin/env bash\n# SPDX-FileCopyrightText: Cadasto B.V.\n# SPDX-License-Identifier: BUSL-1.1\n# 0.76.1\n[[ "$1 $2" == "--sha256 0.76.1" ]]\n' > "$d/scripts/release/install-trivy.sh"
  expect 0 "a Trivy pin the install script pins a sha256 for"
  printf '#!/usr/bin/env bash\n# SPDX-FileCopyrightText: Cadasto B.V.\n# SPDX-License-Identifier: BUSL-1.1\n# 0.76.1\nexit 1\n' > "$d/scripts/release/install-trivy.sh"
  expect 1 "a Trivy pin the install script pins no sha256 for"
  rm "$d/scripts/release/install-trivy.sh"
  expect 1 "a Trivy pin with no install script"
  case_dir
  printf '[workspace.package]\nversion = "1.2.3"\n' >> "$d/Cargo.toml"
  # shellcheck disable=SC2016 # the fixture is literal compose text, never expanded here
  printf '# SPDX-FileCopyrightText: Cadasto B.V.\n# SPDX-License-Identifier: BUSL-1.1\nservices:\n  ferropix:\n    image: ghcr.io/ferrohealth/ferropix:${FERROPIX_VERSION:-1.2.3}\n' > "$d/compose.yaml"
  expect 0 "a quickstart on the workspace version"
  sed -i.bak 's/:-1.2.3/:-1.2.2/' "$d/compose.yaml" && rm "$d/compose.yaml.bak"
  expect 1 "a quickstart behind the workspace version"
  printf '# SPDX-FileCopyrightText: Cadasto B.V.\ngit clone --branch v1.2.3 --depth 1 https://github.com/FerroHEALTH/FerroPIX && cd FerroPIX\n' > "$d/README.md"
  sed -i.bak 's/:-1.2.2/:-1.2.3/' "$d/compose.yaml" && rm "$d/compose.yaml.bak"
  expect 0 "a quickstart that clones the workspace version"
  sed -i.bak 's/--branch v1.2.3 //' "$d/README.md" && rm "$d/README.md.bak"
  expect 1 "a quickstart that clones main"
  sed -i.bak 's/git clone /git clone --branch v1.2.2 /' "$d/README.md" && rm "$d/README.md.bak"
  expect 1 "a quickstart that clones an older release"
  case_dir
  printf '[workspace.package]\nversion = "0.0.6"\n' >> "$d/Cargo.toml"
  mkdir -p "$d/deploy/helm/ferropix"
  printf '# SPDX-FileCopyrightText: Cadasto B.V.\n# SPDX-License-Identifier: BUSL-1.1\napiVersion: v2\nversion: 0.2.3\nappVersion: "0.0.6"\n' > "$d/deploy/helm/ferropix/Chart.yaml"
  expect 0 "a chart appVersion on the workspace version"
  sed -i.bak 's/"0.0.6"/"0.0.5"/' "$d/deploy/helm/ferropix/Chart.yaml" && rm "$d/deploy/helm/ferropix/Chart.yaml.bak"
  expect 1 "a chart appVersion 0.0.5 against the workspace version 0.0.6"
  sed -i.bak 's/^appVersion: .*/appVersion: 0.0.6/' "$d/deploy/helm/ferropix/Chart.yaml" && rm "$d/deploy/helm/ferropix/Chart.yaml.bak"
  expect 0 "an unquoted chart appVersion on the workspace version"
  sed -i.bak '/^appVersion:/d' "$d/deploy/helm/ferropix/Chart.yaml" && rm "$d/deploy/helm/ferropix/Chart.yaml.bak"
  expect 1 "a chart without an appVersion"
  case_dir
  # shellcheck disable=SC2016 # the backticks are Markdown in the fixture
  printf '| FerroUI (`ferrohealth-ui`) | `=0.5.2` | `Cargo.toml`; the FerroUI documents read at the tag `v0.5.2` |\n' >> "$d/docs/VERSIONS.md"
  printf '[workspace.dependencies]\nferrohealth-ui = "=0.5.2"\n' >> "$d/Cargo.toml"
  expect 0 "a FerroUI row on the workspace pin"
  sed -i.bak 's/"=0.5.2"/"=0.5.3"/' "$d/Cargo.toml" && rm "$d/Cargo.toml.bak"
  expect 1 "a FerroUI row behind the workspace pin"
  sed -i.bak 's/"=0.5.3"/"=0.5.2"/' "$d/Cargo.toml" && rm "$d/Cargo.toml.bak"
  # shellcheck disable=SC2016 # the backticks are Markdown in the fixture
  sed -i.bak 's/tag `v0.5.2`/tag `v0.4.0`/' "$d/docs/VERSIONS.md" && rm "$d/docs/VERSIONS.md.bak"
  expect 1 "a FerroUI tag off the pinned version"
  case_dir
  mkdir -p "$d/fuzz/seeds/pix_query"
  printf 'sourceIdentifier=urn%%3Aoid%%3A2.999.1%%7C1' > "$d/fuzz/seeds/pix_query/gen-a.txt"
  expect 0 "a fuzz seed without the header"
  mkdir -p "$d/changelog.d"
  printf -- '- An entry (#1).\n' > "$d/changelog.d/1-an-entry.added.md"
  expect 0 "a changelog fragment without the header"
  printf 'The fragment format.\n' > "$d/changelog.d/README.md"
  expect 1 "the fragment README without the header"
  rm -r "$work"
  if [[ "$failed" -ne 0 ]]; then
    exit 1
  fi
  echo "versions: self-test OK ($n fixtures)."
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --self-test)
      self_test
      exit 0
      ;;
    --root)
      root="${2:?usage: versions.sh --root DIR}"
      shift 2
      ;;
    *)
      echo "usage: versions.sh [--root DIR] | --self-test" >&2
      exit 2
      ;;
  esac
done
cd "$root"

fail=0
drift() {
  echo "  DRIFT: $*" >&2
  fail=1
}

[[ -f docs/VERSIONS.md ]] || {
  echo "versions: docs/VERSIONS.md is missing" >&2
  exit 1
}

# 1. Toolchain.
if [[ -f rust-toolchain.toml ]]; then
  want="$(pin "Rust toolchain")"
  got="$(toml_value rust-toolchain.toml channel)"
  [[ "$got" == "$want" ]] || drift "rust-toolchain.toml pins $got, docs/VERSIONS.md $want"
fi
if [[ -f Cargo.toml ]]; then
  want="$(pin "Minimum supported Rust version")"
  got="$(toml_value Cargo.toml rust-version)"
  [[ "$got" == "$want" ]] || drift "Cargo.toml rust-version is ${got:-unset}, docs/VERSIONS.md $want"
  want="$(pin Edition)"
  got="$(toml_value Cargo.toml edition)"
  [[ "$got" == "$want" ]] || drift "Cargo.toml edition is ${got:-unset}, docs/VERSIONS.md $want"
fi
if [[ -f rustfmt.toml ]]; then
  want="$(pin Edition)"
  got="$(toml_value rustfmt.toml edition)"
  [[ "$got" == "$want" ]] || drift "rustfmt.toml edition is ${got:-unset}, docs/VERSIONS.md $want"
fi

# 2. Corpora.
if [[ -d docs/specs ]]; then
  for tree in docs/specs/*/; do
    tree="${tree%/}"
    [[ -d "$tree" ]] || continue
    [[ -f "$tree/PROVENANCE.md" ]] || drift "$tree has no PROVENANCE.md"
    grep -qF "\`$tree/" docs/VERSIONS.md || drift "$tree is named in no row of docs/VERSIONS.md"
  done
fi

# 3. The tool pins, each in every file its row names.
for item in "${REPEATED_ROWS[@]}"; do
  want="$(pin "$item")"
  [[ -n "$want" ]] || continue
  while IFS= read -r file; do
    [[ -f "$file" ]] || continue
    grep -qF -- "$want" "$file" || drift "$file does not carry the $item pin $want of docs/VERSIONS.md"
  done < <(repeats "$item")
done

# 3b. FerroUI: the workspace pin and the tag of its documents. The row's
#     pin carries the `=` of an exact requirement, the tag does not.
ferroui_row="FerroUI (\`ferrohealth-ui\`)"
want="$(pin "$ferroui_row")"
if [[ -n "$want" && -f Cargo.toml ]]; then
  got="$(toml_value Cargo.toml ferrohealth-ui)"
  [[ "$got" == "$want" ]] || drift "Cargo.toml pins ferrohealth-ui at ${got:-nothing}, docs/VERSIONS.md $want"
  tag="$(repeats "$ferroui_row" | grep -E '^v[0-9]' | head -n 1 || true)"
  [[ "$tag" == "v${want#=}" ]] || drift "docs/VERSIONS.md reads the FerroUI documents at the tag ${tag:-none}, wanted v${want#=}"
fi

# 3c. Trivy: the install script pins a sha256 for the version of its row,
#     so a bump that forgets the checksum fails here and not in a release.
want="$(pin Trivy)"
if [[ -n "$want" ]]; then
  if [[ ! -f scripts/release/install-trivy.sh ]]; then
    drift "docs/VERSIONS.md pins Trivy $want but scripts/release/install-trivy.sh is missing"
  elif ! bash scripts/release/install-trivy.sh --sha256 "$want" > /dev/null 2>&1; then
    drift "scripts/release/install-trivy.sh pins no sha256 for Trivy $want"
  fi
fi

# 4. The server image tag compose.yaml defaults to is the product version,
#    so the quickstart of a release pulls the image that release published.
if [[ -f compose.yaml && -f Cargo.toml ]]; then
  want="$(toml_value Cargo.toml version)"
  got="$(sed -nE 's|^[[:space:]]*image:[[:space:]]*ghcr\.io/ferrohealth/ferropix(-console)?:\$\{FERROPIX_VERSION:-([^}]+)\}[[:space:]]*$|\2|p' compose.yaml | sort -u)"
  [[ -n "$got" ]] || drift "compose.yaml names no ghcr.io/ferrohealth/ferropix:\${FERROPIX_VERSION:-<version>} image"
  [[ -z "$got" || "$got" == "$want" ]] || drift "compose.yaml defaults its images to $(tr '\n' ' ' <<< "$got")but Cargo.toml says $want"
fi

# A clone of main pairs main's quickstart.toml with the released image
# compose.yaml pins, which refuses a key main renamed since, so the README
# clones the release.
if [[ -f README.md && -f Cargo.toml ]]; then
  want="$(toml_value Cargo.toml version)"
  clones="$(grep -E 'git clone .*github\.com/FerroHEALTH/FerroPIX' README.md || true)"
  [[ -n "$clones" ]] || drift "README.md clones no FerroPIX release for its quick start"
  while IFS= read -r line; do
    [[ -z "$line" || "$line" == *"--branch v$want "* ]] || drift "README.md clones FerroPIX without --branch v$want: $line"
  done <<< "$clones"
fi

# The chart's appVersion is the default tag of both images it deploys, so
# it is the product version too.
chart=deploy/helm/ferropix/Chart.yaml
if [[ -f "$chart" && -f Cargo.toml ]]; then
  want="$(toml_value Cargo.toml version)"
  got="$(sed -nE 's/^appVersion:[[:space:]]*"?([^"[:space:]]+)"?[[:space:]]*$/\1/p' "$chart")"
  [[ -n "$got" ]] || drift "$chart has no appVersion"
  [[ -z "$got" || "$got" == "$want" ]] || drift "$chart has appVersion $got but Cargo.toml says $want"
fi

# 5. Licence.
while IFS= read -r file; do
  case "$file" in
    LICENSE | */LICENSE | NOTICE | Cargo.lock | */Cargo.lock | *.json | *.png | *.ico | */img/console/captured-version.txt | docs/specs/* | */testdata/* | */fixtures/* | fuzz/seeds/* | changelog.d/[0-9]*.md | perf/messages/*) continue ;;
    *) ;;
  esac
  [[ -f "$file" ]] || continue
  head -n 25 "$file" | grep -q 'SPDX-FileCopyrightText: Cadasto B.V.' ||
    drift "$file carries no SPDX-FileCopyrightText: Cadasto B.V. header"
  if head -n 25 "$file" | grep 'SPDX-License-Identifier:' | grep -qv 'BUSL-1.1'; then
    drift "$file claims a licence other than BUSL-1.1"
  fi
done < <(git ls-files)

if [[ "$fail" -ne 0 ]]; then
  echo "versions: drift found; docs/VERSIONS.md is the source of truth" >&2
  exit 1
fi
echo "versions: OK (toolchain, corpora, tool pins and licence headers agree)"
