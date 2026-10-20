#!/usr/bin/env bash
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# scripts/vendor/mitz.sh
#
# Pins the Mitz documents that define the closed authorization question
# ("gesloten autorisatievraag"), the consent question of GF-Consent in the
# Netherlands:
#
# - the Implementatiehandleiding Open en gesloten autorisatievraag, the wire
#   of the question (SOAP 1.2 with an XACML 3.0 decision query);
# - the PvE AMC Aansluiting Mitz-connector, the attributes a question carries
#   and how a connector handles the answer and its errors;
# - the Implementatiehandleiding Berichtauthenticatie, the optional message
#   token a connector may opt into when it is admitted.
#
# The Mitz feature of crates/nl-generic-functions is built over them.
#
# VZVZ publishes the documents as attachments of one page of the Mitz
# afsprakenstelsel on its Confluence site, readable without an account, and
# nowhere else: no repository, no package and no WSDL or schema. Each is
# pinned by its attachment URL at version 1 and its sha256
# (scripts/vendor/lib/pinned.sh), and the "Mitz closed authorization
# question" row of docs/VERSIONS.md carries the pin-set digest.
#
# Redistribution: none of the documents states a licence or terms of use, and
# the PvE is marked "Classificatie: Vertrouwelijk", so each is cache only:
# fetched into the git-ignored .vendor-cache/mitz/ and never committed. The
# script writes only docs/specs/mitz/PROVENANCE.md into the tree.
#
# Usage:
#   scripts/vendor/mitz.sh
#
# Requires: curl, shasum, awk.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

# shellcheck source=scripts/vendor/lib/corpus.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$root/scripts/vendor/lib/corpus.sh"
# shellcheck source=scripts/vendor/lib/pinned.sh
# shellcheck disable=SC1091 # shellcheck is not run with -x; the library is checked on its own
. "$root/scripts/vendor/lib/pinned.sh"

corpus_require curl shasum awk
pinned_begin

site="https://vzvz.atlassian.net/wiki/download/attachments/828314367"
unstated='None stated: the document carries no licence, copyright notice or terms of use, and its Confluence page states none'

pin cache VZVZ_Mitz_Implementatiehandleiding_OpenGesloten_v3.8.2.pdf \
  "3.8.2, 27 May 2024, Definitief; attachment version 1" \
  "$site/VZVZ_Mitz_Implementatiehandleiding_OpenGesloten_v3.8.2.pdf?version=1&api=v2" \
  a5ce8f0d7eba8969a395a8560cf69f9e359f9da4c76145adc0748b9907ac0cf3 "$unstated" \
  "§2.3 and §3.2 the closed authorization question (attributes §3.2.4.2, examples §3.2.4.4, §3.2.4.5 and §3.2.5.4, expected actions §3.2.4.6), §3.3 the transport, §4 the OIDs, §5 the professional identifier, §6 the HTTP headers"
pin cache VZVZ_Mitz_PvE_AMC_Aansluiting_Mitz-connector_v3.8.1.ad1.pdf \
  "3.8.1.ad1, 2 June 2023; attachment version 1" \
  "$site/VZVZ_Mitz_PvE_AMC_Aansluiting_Mitz-connector_v3.8.1.ad1.pdf?version=1&api=v2" \
  b1f18b48969475ce969472299179b37663cdcf67b067e9275532e0fa35bb59cb \
  "$unstated; the document is marked \"Classificatie: Vertrouwelijk\"" \
  "AUS-TR-e0040 and AUS-TR-e0050 the attributes of a question and how Mitz answers it, AUS-TR-e0900 error handling, AUS-TR-e0250 the optional tokens"
pin cache VZVZ_Mitz_Implementatiehandleiding_Berichtauthenticatie_v3.8.1.ad1.pdf \
  "3.8.1.ad1; attachment version 1" \
  "$site/VZVZ_Mitz_Implementatiehandleiding_Berichtauthenticatie_v3.8.1.ad1.pdf?version=1&api=v2" \
  9659bdcd20a4deebc699357a455aaca7b3643c78a53b078329edcb899ef02f01 "$unstated" \
  "The message token a connector receives on every interface when it opts in at admission (AUS-TR-e0250)"
pinned_corpus mitz "Mitz closed authorization question" \
  "the Mitz closed authorization question" \
  "The documents of the VZVZ Mitz afsprakenstelsel that define the closed
authorization question of GF-Consent, attachments of the Confluence page
\"Bijlage Architectuurdocumenten\" (page id \`828314367\` in space \`MA11\`,
<https://vzvz.atlassian.net/wiki/spaces/MA11/pages/828314367/Bijlage+Architectuurdocumenten>),
readable without an account. Each download URL redirects to the Atlassian
media store. None states a licence, so each is cache only and this
repository carries none of their content.

Examined and not taken: the FHIR package \`vzvz.fhir.mitz\` 3.8.0 on
packages.fhir.org (tarball sha256
\`f36b10fb7fbf4aa5f7ae76ddc3a99b9cfcdb0b9beaec1aba0344ac739832f7fe\`), whose
Consent, Subscription and notification profiles serve the subscribe, notify
and migrate interfaces and define no closed authorization question, its
\`package.json\` stating no licence; the \`VZVZ\` GitHub organisation, which
publishes no Mitz repository; and the other attachments of the page, which
define no part of the closed authorization question." \
  "" \
  "the closed authorization question of \`crates/nl-generic-functions\` feature \`mitz\`"

say "done"
