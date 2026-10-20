<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: IHE IUA Revision 2.3, the revision Swiss Annex 5 pins

Vendored by `scripts/vendor/ch.sh`, each artefact pinned by its
URL and the sha256 of its bytes. Never edit a file here: change the pins in
the script and the pin-set digest in docs/VERSIONS.md, and re-run the script.

- Pin-set digest (sha256 over the sorted `mode  file  url  sha256` lines of
  the pins): `2a5f13a87ccf307fecda8c48e77a61fd09c440089b2172be0edea912dea85439`
- Fetched: 2026-10-20, with the User-Agent `ferropix-vendor (scripts/vendor)`
- Artefacts: 1 committed, 0 cache only, 0 needing
  manual retrieval
- Files in this directory: 1 besides this one, each verbatim as the
  publisher serves it
- Tree digest (sha256 over the sorted per-file `sha256  path` listing,
  `PROVENANCE.md` excluded): `74afdde5d941f98f9100e7bf1d4bd48b8b14fe9bd5f170436dbf8df3ca2ea349`
- Read by: the crates of this repository

EPDV-EDI Annex 5 pins IHE IUA Revision 2.3; `crates/ihe-iti` binds
Revision 2.5 (`docs/specs/ihe-iua/`). The repository archive at the commit
tag `2.3` resolves to is kept whole, so the two revisions can be compared.

## Artefacts

### `ITI.IUA-2.3-7c70ff7.tar.gz` (committed)

- Source: <https://codeload.github.com/IHE/ITI.IUA/tar.gz/7c70ff7b2b18d392baf083d9eee4b17a3f1a7c52>
- Version: tag 2.3, commit 7c70ff7b2b18d392baf083d9eee4b17a3f1a7c52
- sha256: `7a4f88dc7268f2716515daacb6bac951397771205a9085bb479b08f749473c0b`
- Licence: CC-BY-4.0: the repository LICENSE inside the archive, "Attribution 4.0 International"; the IHE Technical Frameworks General Introduction §9 also grants every user a licence to reproduce and distribute IHE Technical Documents
- Note: Fetched by commit, so the archive's top directory names the commit; GitHub does not promise byte-stable archives, so a re-generated archive fails the pin loudly.
