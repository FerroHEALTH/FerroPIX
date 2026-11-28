<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Post-quantum inventory

This page lists every cryptographic use in FerroPIX, its crates and its
release workflows, the algorithm each uses, and whether it resists a
cryptographically relevant quantum computer. The machine-readable copy is
`conformance/post-quantum.tsv`: after its `#` comment lines, one row per
use with five tab-separated columns: `use`, `algorithm`, `safe` (`yes` or `no`), `pq_standard` (the
standard that defines the post-quantum form, or `no standard yet`) and
`location`. Every change that adds, removes or changes a cryptographic use
updates the file in the same pull request, and
`scripts/checks/post-quantum.sh --update` rewrites the table below and the
README badge from it. CI fails when the table, the badge or a location
named in the file disagrees with the tree.

## The sources

- **NIST IR 8547 (final):** ECDSA, EdDSA and RSA signatures (§4.1.1) and
  ECDH, DH and RSA key establishment (§4.1.2) are deprecated after 2030 and
  disallowed after 2035. Symmetric primitives with at least 128 bits of
  classical security, AES-128 and up, SHA-256 and HMAC-SHA-256 among them,
  stay (§4.1.3). FIPS 203, 204 and 205 define ML-KEM, ML-DSA and SLH-DSA.
- **The EU coordinated implementation roadmap (NIS Cooperation Group,
  v1.1):** a use case is high-risk when its confidentiality must hold for
  10 years or more, which a patient's identity needs, and high-risk use
  cases complete the transition by 31.12.2030 (§4.2). After 2030 they use
  no quantum-vulnerable public-key mechanism stand-alone, and standardised
  hybrid solutions are recommended (§4.1).
- **RFC 10024:** the TLS 1.3 hybrid group X25519MLKEM768 (codepoint
  0x11EC), within the hybrid design of RFC 9954.
- **RFC 9964:** ML-DSA for JOSE and COSE. **RFC 9881:** ML-DSA in X.509.

A row is `safe` when every key it depends on is established or verified by
a post-quantum or hybrid mechanism, or when it is a symmetric primitive at
128 bits or more. A row whose post-quantum form has no standard yet counts
as not safe and stays out of the badge, which counts the safe rows of the
rows a post-quantum standard covers; the `no standard yet` rows of the table
below are the rest.

## What FerroPIX does

The server and console listeners come from `ferrohealth-server`, which
offers X25519MLKEM768 first in the TLS 1.3 key exchange, then X25519, P-256
and P-384 for a peer that lacks the hybrid group. It enables rustls's
`prefer-post-quantum` feature, so every other rustls user in FerroPIX takes
the same order from the default provider: the clients FerroPIX builds
itself (the ITI-93 feed, the XCPD delivery and the console's client of the
server), the family clients built through `ferrohealth-smart`, and the
PostgreSQL connection of `sqlx`. Tests show each client FerroPIX builds
itself agreeing on X25519MLKEM768 with a peer that offers it beside X25519,
and on X25519 with a peer that offers only that.

The key exchange protects what a session carries against an attacker who
records it now and decrypts it later, the risk the roadmap gives first
place. The certificate signatures that authenticate a peer stay classical.
RFC 9881 (October 2025, Proposed Standard) defines ML-DSA keys and
signatures in X.509 certificates. The TLS 1.3 signature schemes for ML-DSA
(`mldsa44` 0x0904, `mldsa65` 0x0905, `mldsa87` 0x0906) are defined by
draft-ietf-tls-mldsa-06, which the IESG approved and which waits in the RFC
Editor queue, not yet an RFC
(<https://datatracker.ietf.org/doc/draft-ietf-tls-mldsa/>, read
2026-11-27). The row stays `no` while that document is a draft and the
operator's certificates decide the algorithm.

## JOSE signatures

`ferrohealth-smart` 0.35.0 signs and verifies every JWS in its `jws` module,
ML-DSA-44, ML-DSA-65 and ML-DSA-87 (RFC 9964, `AKP` keys) among the
algorithms. What it accepts on a verification is the list it is given:

- **Access tokens:** an issuer's `algorithms` defaults to RS256, RS384,
  PS256, PS384, ES256 and ES384. An ML-DSA parameter set is accepted where
  the list names it. FerroPIX checks no DPoP proof: it refuses an
  `[auth.dpop]` table.
- **The console's sign-in ID Token:** the relying party's default is RS256
  to PS512, ES256, ES384 and EdDSA; the console adds the three ML-DSA
  parameter sets to it.
- **Client assertions:** a key signs with the algorithm of its kind, an
  ML-DSA key (RFC 9881 §6) with its parameter set and an RSA key with RS384.
- **Nuts presentations:** Nuts RFC021 §4.2 requires a JWT under the
  Verifiable Credentials Data Model 1.1 §6.3.1, which requires an `alg` and
  names none; the Authorization Server lists the algorithms it takes in its
  `vp_formats`. A holder with an ML-DSA key signs its presentation with the
  key's parameter set, and a holder with a P-256 or P-384 key signs ES256 or
  ES384.

`[auth] post_quantum_only = true` in the server's configuration narrows
every JWT issuer's `algorithms` left at its default to the three ML-DSA
parameter sets, and stops the start when a list names another algorithm.
The console's `[jose] post_quantum_only = true` accepts an ID Token signed
with ML-DSA alone. Both are off by default, for the issuers that still sign
classically.

The badge counts the JOSE rows in that configuration: both settings on and
ML-DSA keys for the client assertions and the Nuts holder. A deployment
that leaves the settings off, or keeps classical keys, uses the classical
algorithms the rows name, and those rows are then not post-quantum safe for
it.

## The inventory

| Use | Algorithm | Safe | Post-quantum standard | Location |
|---|---|---|---|---|
| TLS 1.3 key exchange of the server and console listeners | X25519MLKEM768 first; X25519, P-256, P-384 for a peer without it | yes | RFC 10024 | `ferrohealth-server tls, app/ferropix-server/src/serve.rs, app/ferropix-console/src/server/mod.rs` |
| TLS 1.3 key exchange of the clients FerroPIX builds: the ITI-93 feed, the XCPD delivery and the console's client of the server | X25519MLKEM768 first (rustls prefer-post-quantum); X25519, P-256, P-384 for a peer without it | yes | RFC 10024 | `app/ferropix-server/src/serve.rs (pmir_channel), app/ferropix-server/src/xcpd/mod.rs (delivery_client), app/ferropix-console/src/pix.rs` |
| TLS 1.3 key exchange of the family clients: the audit repository, the terminology server, the PMIR Consumers, the OpenID Provider and the OTLP export | X25519MLKEM768 first; X25519, P-256, P-384 for a peer without it | yes | RFC 10024 | `ferrohealth-server tls_client, ferrohealth-smart client, app/ferropix-server/src/config/section/audit.rs, app/ferropix-server/src/config/section/profiles.rs` |
| TLS 1.3 key exchange of the PostgreSQL connection | X25519MLKEM768 first (rustls prefer-post-quantum) | yes | RFC 10024 | `app/ferropix-server/Cargo.toml (sqlx tls-rustls-aws-lc-rs)` |
| TLS 1.2 key exchange where min_version lowers the minimum | ECDHE over X25519, P-256 or P-384 | no | no standard yet | `ferrohealth-server tls (protocol_versions)` |
| TLS server and client certificate authentication | ECDSA, RSA or Ed25519, as the operator's certificates hold | no | RFC 9881; draft-ietf-tls-mldsa-06 in the RFC Editor queue | `ferrohealth-server tls, ferrohealth-server tls_client` |
| Outbound TLS certificate verification, a chain to a root of trust_roots_file or trust_roots_without_revocation_file | ML-DSA (RFC 9881), ECDSA, RSA or Ed25519 certificates, verified by webpki on every operating system, the first with a revocation check | no | RFC 9881; draft-ietf-tls-mldsa-06 in the RFC Editor queue | `app/ferropix-server/src/xcpd/mod.rs (delivery_client, ferrohealth-server PlatformTls), app/ferropix-server/src/config/section/audit.rs, app/ferropix-server/src/config/section/profiles.rs, app/ferropix-server/src/config/section/terminology.rs (ferrohealth-smart ServerTls over PlatformTls)` |
| Outbound TLS certificate verification, a chain to a platform root | ECDSA, RSA or Ed25519, or ML-DSA where the platform's verifier takes it; the macOS verifier refuses ML-DSA (FerroHEALTH/FerroSYS#470) | no | RFC 9881; whether ML-DSA verifies is the platform verifier's choice | `app/ferropix-server/src/xcpd/mod.rs (delivery_client, ferrohealth-server PlatformTls), app/ferropix-server/src/config/section/audit.rs, app/ferropix-server/src/config/section/profiles.rs, app/ferropix-server/src/config/section/terminology.rs (ferrohealth-smart ServerTls over PlatformTls)` |
| TLS record protection | AES-128-GCM, AES-256-GCM, ChaCha20-Poly1305 | yes | NIST IR 8547 4.1.3 | `ferrohealth-server tls` |
| Access token verification with [auth] post_quantum_only = true (FerroPIX checks no DPoP proof) | ML-DSA-44, ML-DSA-65 or ML-DSA-87 (AKP) alone; off (the default), RS256, RS384, PS256, PS384, ES256 and ES384, and an ML-DSA parameter set where an algorithms list names it | yes | RFC 9964 | `ferrohealth-smart resource, app/ferropix-server/src/config/section/auth.rs, app/ferropix-server/src/auth` |
| Client assertions FerroPIX signs (private_key_jwt) with an ML-DSA key, as the example configures it | ML-DSA-44, ML-DSA-65 or ML-DSA-87, the parameter set of the key (ML-DSA-65 in the example); with a classical key RS384 (RSA) by default, RS256 to PS512, ES256, ES384 or EdDSA | yes | RFC 9964 | `ferrohealth-smart client assertion, app/ferropix-server/src/config/section/audit.rs, app/ferropix-server/tests/it/outbound_ml_dsa.rs` |
| Console sign-in ID token verification with [jose] post_quantum_only = true | ML-DSA-44, ML-DSA-65 or ML-DSA-87 alone; off (the default), those beside RS256, RS384, RS512, PS256, PS384, PS512, ES256, ES384 and EdDSA | yes | RFC 9964 | `ferrohealth-smart oidc, app/ferropix-console/src/config/mod.rs, app/ferropix-console/src/server/mod.rs` |
| Identifier sealing at rest | AES-256-GCM, HMAC-SHA-256, HKDF-SHA-256 | yes | NIST IR 8547 4.1.3 | `app/ferropix-server/src/store/seal.rs` |
| Nuts verifiable presentations FerroPIX signs with an ML-DSA holder key | ML-DSA-44, ML-DSA-65 or ML-DSA-87, the parameter set of the key, where the Authorization Server lists it in vp_formats; with an EC key ES256 or ES384 | yes | RFC 9964 | `crates/nl-generic-functions/src/nuts_auth/holder.rs, crates/nl-generic-functions/src/nuts_auth/mod.rs` |
| Basic password hashing | Argon2id | yes | NIST IR 8547 4.1.3 | `ferrohealth-smart basic, app/ferropix-server/src/config/section/auth.rs` |
| crates.io publishing | Trusted Publishing (OIDC token from GitHub, RS256) | no | no standard yet | `.github/workflows/release.yml, .github/workflows/publish-crates.yml` |
| Release provenance and SBOM attestations | Sigstore attestation (ECDSA P-256) | no | no standard yet | `.github/workflows/release-build.yml, .github/workflows/release-image.yml, .github/workflows/release-console.yml, .github/workflows/release-crate.yml` |
