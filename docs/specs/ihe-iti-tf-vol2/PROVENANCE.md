<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: IHE ITI Technical Framework Volume 2, transactions ITI-38 and ITI-55

Vendored by `scripts/vendor/ihe-iti-tf.sh`, each artefact pinned by its
URL and the sha256 of its bytes. Never edit a file here: change the pins in
the script and the pin-set digest in docs/VERSIONS.md, and re-run the script.

- Pin-set digest (sha256 over the sorted `mode  file  url  sha256` lines of
  the pins): `84c3ff8cfad148b5c066ae364d1f8a3044c6210ed806e9b87f363d8dda2d5546`
- Fetched: 2026-10-20, with the User-Agent `ferropix-vendor (scripts/vendor)`
- Artefacts: 1 committed, 1 cache only, 0 needing
  manual retrieval
- Files in this directory: 1 besides this one, each verbatim as the
  publisher serves it
- Tree digest (sha256 over the sorted per-file `sha256  path` listing,
  `PROVENANCE.md` excluded): `4c5a4b82f0b70aeca20fc43870677b084b75745c0751bbd2536d409d2ef55092`
- Read by: the IHE section citation guard, `scripts/checks/ihe-citations.sh`,
  over the ITI-55 citations of crates/ihe-iti

The Cross Gateway Query [ITI-38] and Cross Gateway Patient Discovery
[ITI-55] transactions, whose sections the XCPD code cites and the IHE
section citation guard (`scripts/checks/ihe-citations.sh`) checks. ITI-38 is IHE's own text, which the General Introduction §9
(<https://profiles.ihe.net/GeneralIntro/ch-9.html>) licenses for
reproduction and distribution; it names the OASIS ebXML registry standards
and reproduces none of their text. ITI-55 reproduces HL7 Version 3 tables,
whose rights HL7 reserves (General Introduction §9.1.2), so it is cache
only: the guard checks the ITI-55 citations against the cached page when a
local run has fetched it, and counts them otherwise.

## Artefacts

### `ITI-38.html` (committed)

- Source: <https://profiles.ihe.net/ITI/TF/Volume2/ITI-38.html>
- Version: ITI TF Revision 20.2, page as served on 2026-10-06
- sha256: `49be7b6b435ff05153c2da655369dfb8eac2c6685489ec237285d05af394a192`
- Licence: IHE Technical Frameworks General Introduction §9: IHE International grants "to any other user of these documents, an irrevocable, worldwide, perpetual, royalty-free, nontransferable, nonexclusive, non-sublicensable license under its copyrights in any IHE profiles and Technical Framework documents [...] to reproduce and distribute" them (page footer "© 2000 — IHE International")
- Note: Rendered HTML page whose bytes may change on a re-render. The page links one figure, media/Figure_3.38.4-1.png, which is not taken.

### `ITI-55.html` (cache only, not redistributed)

- Source: <https://profiles.ihe.net/ITI/TF/Volume2/ITI-55.html>
- Version: ITI TF Volume 2, the page states Revision 20.1, served on 2026-10-06
- sha256: `67dcce3339dd2fae07db50422c02f36e8d11053b5740aceef847361e33bfb801`
- Licence: IHE Technical Frameworks General Introduction §9.1.2: "Health Level Seven, Inc. has granted permission to IHE to reproduce tables from the HL7 standard. The HL7 tables in this document are copyrighted by Health Level Seven, Inc. All rights reserved." The page reproduces the HL7 Version 3 message information model tables of its query and its response, so it is not redistributed.
- Not redistributed: the script fetches it into `.vendor-cache/ihe-iti-tf-vol2/`, which git
  ignores; this repository carries none of its content.
- Note: Rendered HTML page whose bytes may change on a re-render.
