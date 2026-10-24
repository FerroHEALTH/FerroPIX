<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Machine review (SonarQube Cloud): advisory, never authority

Every pull request and every push to `main` is analyzed by SonarQube Cloud
(`.github/workflows/sonar.yml`; scope in `sonar-project.properties`; project
`FerroHEALTH_FerroPIX`, organization `ferrohealth`, the built-in "Sonar way"
quality gate). It is a second opinion beside the local gates and CodeQL, and
it also reads the trees the Rust gates never see: shell, workflow YAML, and
JSON. No specification governs this: our own design, as FerroBRIDGE and
FerroFED run it.

The lane runs the multi-language sweep and the Rust analysis, and imports the
lcov report the coverage step writes one workspace member at a time. The
analyzer imports no Clippy report: Clippy gates in `ci.yml` at `-D warnings`,
so a second copy of its findings would add scan time and no new finding.

It is a second opinion. It is not authority, and it gates no merge: the
`conclusion` job of `ci.yml`, the single required check on `main`, does not
need it.

## Precedence: a finding never outranks the sources

1. The specifications (`spec-adherence.md`): the IHE ITI Technical
   Framework, the PIXm, PDQm and PMIR implementation guides, HL7 FHIR R4, and
   the HL7 v2 PIX and PDQ transactions. They are the oracle.
2. The hard rules: `CLAUDE.md` and the `.claude/rules/*.md` files.
3. The local gates: `shellcheck`, `actionlint`, `zizmor`, `cargo fmt`,
   `clippy`, `cargo nextest`, `cargo deny`, and the CI guards.
4. The analyzer.

A finding that contradicts a citation, or asks for something the rules
forbid, is wrong by construction. Nothing it reports relaxes `testing.md`:
never weaken, skip or delete a test because a finding suggested it.

## Rules

- **It gates no merge.** The quality gate is informational; merges are
  decided by the local gates and review.
- **Findings are acted on by hand, in a normal change**, never applied
  through a UI that would attribute a commit to a bot. The no-AI-attribution
  rule has no exceptions.
- **Automatic Analysis stays OFF.** CI-based analysis and SonarQube Cloud's
  Automatic Analysis cannot both run; `sonar.yml` is the analysis path.
- **The lane skips without its token.** A run that cannot read the
  `SONAR_TOKEN` secret (a fork, a Dependabot pull request, a repository where
  it is not provisioned) skips the analysis and stays green.
- **A wrong finding is data, not a silent suppression:** record it on the
  tracker, or adjust `sonar-project.properties` when the scope is
  wrong, never to make a number go down.

CodeQL (`.github/workflows/codeql.yml`) runs separately as the security
scanner and is advisory here too.

## Official documentation (durable citations)

- <https://docs.sonarsource.com/sonarqube-cloud/>
- <https://docs.sonarsource.com/sonarqube-cloud/analyzing-source-code/languages/rust>
