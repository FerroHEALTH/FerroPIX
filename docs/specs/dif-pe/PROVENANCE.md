<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: DIF Presentation Exchange 2.0.0

Vendored verbatim by `scripts/vendor/dif-pe.sh`. Never edit a file here:
change the pin in docs/VERSIONS.md and re-run the script.

- Source: <https://github.com/decentralized-identity/presentation-exchange>, rendered at
  <https://identity.foundation/presentation-exchange/spec/v2.0.0/>
- Pin: commit `7cbe949c95fe1e19413b24c9b49dc34f76f5d76a`; the claim format registry,
  <https://github.com/decentralized-identity/claim-format-registry>, at commit `4a15817a7717efdda29912a1eef6e59135d8d02a`, under
  `claim-format-registry/`
- Document: *Presentation Exchange 2.0.0*, Decentralized Identity Foundation
- Fetched: 2026-10-20
- Upstream licence: the Apache License 2.0 (each repository's `LICENSE`,
  vendored beside its files)
- Layout: the upstream paths, unchanged, the registry's under
  `claim-format-registry/`
- Files: 11
- Tree digest (sha256 over the sorted per-file `sha256  path` listing,
  `PROVENANCE.md` excluded): `a4632ffea5dbb879fdae4cabac74eba5806d8360ba5409197326692856751b10`
- Read by: the Presentation Definition the access token request of
  `crates/nl-generic-functions` feature `nuts-auth` reads and the
  Presentation Submission it writes, held to the schemas

## What is taken

The v2.0.0 specification text, the version Nuts RFC021 cites, and its JSON
Schemas, and the claim format designations the submission schema
references for its `format`. The other versions, the playground, the sample implementation and
the test vectors serve no reader here and are not taken.

| File | sha256 | git blob id |
|---|---|---|
| `LICENSE` | `c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4` | `261eeb9e9f8b2b4b0d119366dda99c6fd7d35c64` |
| `claim-format-registry/LICENSE` | `c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4` | `261eeb9e9f8b2b4b0d119366dda99c6fd7d35c64` |
| `claim-format-registry/schemas/presentation-definition-claim-format-designations.json` | `91eb9058670158488da42771f3836760020c2d24a033354c86e92a037d9be062` | `9506edd2abf0df5b06a18a0556fe4cb0ace93174` |
| `claim-format-registry/schemas/presentation-submission-claim-format-designations.json` | `108dba7a7b81e71faca87b9d6552aab6c5ce7d727107e8cda027b0d928c58e0d` | `80b593a6f7e6e8e3350fb9c8f32d74ffba67a9b1` |
| `schemas/v2.0.0/input-descriptor.json` | `1fdc3c32bc3b360345e16eeeb191b0a49daea6344eed9b4ab17095c7daf9b1a7` | `2112e900f84ede399aca599cbeb87437f9e30dba` |
| `schemas/v2.0.0/presentation-definition-envelope.json` | `8a04335f2452018dcdeced25f59889b14890b56f02820e55bc21de84d6414037` | `246fdb0de64f3c2c977c20c6ca78cb0ea9e024c2` |
| `schemas/v2.0.0/presentation-definition.json` | `58cbbfdf0ed87f99d18ec92f9fbe5671bab783c39ed873b4acf3b1d500b5a66f` | `a1e6ee269d7f23ef767097f642090212a12fd497` |
| `schemas/v2.0.0/presentation-submission.json` | `718f9087c7b6baa9b0ab4d52844100acfa70a25efc5bc856160e5916363822e0` | `b0f599c1ef32808f28ae823145c058ffee462b26` |
| `schemas/v2.0.0/submission-requirement.json` | `60fde820088b3eb4fc6d454024db0f36615e680c9eb3aea2d1521530f7fecb18` | `9650087ef1f9dca37c5b58540e69166c2b3e6bb6` |
| `schemas/v2.0.0/submission-requirements.json` | `796564fc9260664f11c6fc90125995286bbc8f8531099db3118baaf9fad1de53` | `c04701eb811751b9acddf71a473e6b3991778adb` |
| `spec/v2.0.0/spec.md` | `4845ea6460acde6dfdbf60518b53af156d023b710ab958af3af0d16eef384c74` | `ceeaa3e866c96e3b6994950043bf76e66546b220` |
