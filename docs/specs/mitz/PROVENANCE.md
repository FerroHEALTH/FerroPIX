<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: the Mitz closed authorization question

Vendored by `scripts/vendor/mitz.sh`, each artefact pinned by its
URL and the sha256 of its bytes. Never edit a file here: change the pins in
the script and the pin-set digest in docs/VERSIONS.md, and re-run the script.

- Pin-set digest (sha256 over the sorted `mode  file  url  sha256` lines of
  the pins): `1f0196ab826e5093f6f7b8cbb935b106c22e19b6f9b246d4cce8c157602a14cd`
- Fetched: 2026-10-20, with the User-Agent `ferropix-vendor (scripts/vendor)`
- Artefacts: 0 committed, 3 cache only, 0 needing
  manual retrieval
- Files in this directory: 0 besides this one, each verbatim as the
  publisher serves it
- Tree digest (sha256 over the sorted per-file `sha256  path` listing,
  `PROVENANCE.md` excluded): `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`
- Read by: the closed authorization question of `crates/nl-generic-functions` feature `mitz`

The documents of the VZVZ Mitz afsprakenstelsel that define the closed
authorization question of GF-Consent, attachments of the Confluence page
"Bijlage Architectuurdocumenten" (page id `828314367` in space `MA11`,
<https://vzvz.atlassian.net/wiki/spaces/MA11/pages/828314367/Bijlage+Architectuurdocumenten>),
readable without an account. Each download URL redirects to the Atlassian
media store. None states a licence, so each is cache only and this
repository carries none of their content.

Examined and not taken: the FHIR package `vzvz.fhir.mitz` 3.8.0 on
packages.fhir.org (tarball sha256
`f36b10fb7fbf4aa5f7ae76ddc3a99b9cfcdb0b9beaec1aba0344ac739832f7fe`), whose
Consent, Subscription and notification profiles serve the subscribe, notify
and migrate interfaces and define no closed authorization question, its
`package.json` stating no licence; the `VZVZ` GitHub organisation, which
publishes no Mitz repository; and the other attachments of the page, which
define no part of the closed authorization question.

## Artefacts

### `VZVZ_Mitz_Implementatiehandleiding_OpenGesloten_v3.8.2.pdf` (cache only, not redistributed)

- Source: <https://vzvz.atlassian.net/wiki/download/attachments/828314367/VZVZ_Mitz_Implementatiehandleiding_OpenGesloten_v3.8.2.pdf?version=1&api=v2>
- Version: 3.8.2, 27 May 2024, Definitief; attachment version 1
- sha256: `a5ce8f0d7eba8969a395a8560cf69f9e359f9da4c76145adc0748b9907ac0cf3`
- Licence: None stated: the document carries no licence, copyright notice or terms of use, and its Confluence page states none
- Not redistributed: the script fetches it into `.vendor-cache/mitz/`, which git
  ignores; this repository carries none of its content.
- Note: §2.3 and §3.2 the closed authorization question (attributes §3.2.4.2, examples §3.2.4.4, §3.2.4.5 and §3.2.5.4, expected actions §3.2.4.6), §3.3 the transport, §4 the OIDs, §5 the professional identifier, §6 the HTTP headers

### `VZVZ_Mitz_PvE_AMC_Aansluiting_Mitz-connector_v3.8.1.ad1.pdf` (cache only, not redistributed)

- Source: <https://vzvz.atlassian.net/wiki/download/attachments/828314367/VZVZ_Mitz_PvE_AMC_Aansluiting_Mitz-connector_v3.8.1.ad1.pdf?version=1&api=v2>
- Version: 3.8.1.ad1, 2 June 2023; attachment version 1
- sha256: `b1f18b48969475ce969472299179b37663cdcf67b067e9275532e0fa35bb59cb`
- Licence: None stated: the document carries no licence, copyright notice or terms of use, and its Confluence page states none; the document is marked "Classificatie: Vertrouwelijk"
- Not redistributed: the script fetches it into `.vendor-cache/mitz/`, which git
  ignores; this repository carries none of its content.
- Note: AUS-TR-e0040 and AUS-TR-e0050 the attributes of a question and how Mitz answers it, AUS-TR-e0900 error handling, AUS-TR-e0250 the optional tokens

### `VZVZ_Mitz_Implementatiehandleiding_Berichtauthenticatie_v3.8.1.ad1.pdf` (cache only, not redistributed)

- Source: <https://vzvz.atlassian.net/wiki/download/attachments/828314367/VZVZ_Mitz_Implementatiehandleiding_Berichtauthenticatie_v3.8.1.ad1.pdf?version=1&api=v2>
- Version: 3.8.1.ad1; attachment version 1
- sha256: `9659bdcd20a4deebc699357a455aaca7b3643c78a53b078329edcb899ef02f01`
- Licence: None stated: the document carries no licence, copyright notice or terms of use, and its Confluence page states none
- Not redistributed: the script fetches it into `.vendor-cache/mitz/`, which git
  ignores; this repository carries none of its content.
- Note: The message token a connector receives on every interface when it opts in at admission (AUS-TR-e0250)
