<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: the IHE PIXm FHIR package

Vendored verbatim by `scripts/vendor/ihe-pixm.sh`. Never edit a file here:
change the pin in docs/VERSIONS.md and re-run the script.

- Source: <https://packages.fhir.org/ihe.iti.pixm/3.1.0>, the FHIR package registry's copy of the IG published at
  <https://profiles.ihe.net/ITI/PIXm/3.1.0/>
- Pin: package `ihe.iti.pixm` version `3.1.0`, tarball sha256 `19e2e8eaf3030ac7b4d809c5e1eeb8face02c8635318aeb6d35bc2bb889de0d0`
- Fetched: 2026-10-20
- Upstream licence: Creative Commons Attribution 4.0 International
  (`CC-BY-4.0`, the `license` of the package manifest, listed under What is
  left out;
  <https://creativecommons.org/licenses/by/4.0/>). The package ships no licence
  file of its own. Attribution: IHE International, IT Infrastructure Technical
  Committee, *Patient Identifier Cross-referencing for Mobile (PIXm)* 3.1.0.
- FHIR version: 4.0.1
- Layout: the upstream paths inside the package, unchanged
- Files: 24 of the package's 59, listed below
- Tree digest (sha256 over the sorted per-file `sha256  path` listing,
  `PROVENANCE.md` excluded): `9d25f84d0c21e4bf464a3d0be60df68a06b11760db885885f3152edae9f88f6e`
- Read by: the ITI-83 client of `crates/ihe-iti` (feature `pixm`), whose
  tests decode the examples and hold the request and response to the
  OperationDefinition. The Patient profiles and the audit artefacts are kept
  for the PIX Manager and its audit, as FerroFED vendors them

## What is here

The artefacts of ITI-83, Get Corresponding Identifiers: the `$ihe-pix`
OperationDefinition with its in and out parameters, the Query Parameters In
and Out profiles, the Consumer and Manager capability statements, and the
IG's examples of an ITI-83 request, a response and the not-found error. The
artefacts of ITI-104, Patient Identity Feed FHIR: the Source capability
statement, the Patient profile and its birth-date variant, and the IG's
example Patients. The audit record of ITI-83 as the Consumer records it
(§2:3.83.5.1.1): the PIXm Query Consumer audit profile, built on the BALP
Patient Query pattern, and its example. The package's other files serve no
reader here: the Manager's and the ITI-104 BALP audit profiles and examples,
the Schematron renderings, the OpenAPI renderings and the registry's
`.index.db`, a SQLite file. They are not taken.

| File | sha256 |
|---|---|
| `package/CapabilityStatement-IHE.PIXm.Consumer.json` | `0c07a1bb7e2998bc2eecacce87556c7592cca60ea81102a270afb1d06eb5c3cf` |
| `package/CapabilityStatement-IHE.PIXm.Manager.json` | `d377fc71a000175a78d30cbd40cb26811ee82ce2f86d8751bf6d3a906cdbe530` |
| `package/CapabilityStatement-IHE.PIXm.Source.json` | `79b1d1e63a50b774bccf4cf724cf5d36dd2c88ae25df763d6170edff18f6b4b8` |
| `package/ImplementationGuide-ihe.iti.pixm.json` | `63cfb8127b3767bdcf9bb0578eb7626f1b2148d56889c8139d89f81b2755e2d4` |
| `package/OperationDefinition-IHE.PIXm.pix.json` | `d16827774e5f74fc5b53613963d8dc80cc935f898458db5a5d67b50b86ee8ad6` |
| `package/StructureDefinition-IHE.PIXm.Patient.BirthDateRequired.json` | `c77538e1e297bb0d97e678562cbb7196a546333a6246909153be1d2ad8706386` |
| `package/StructureDefinition-IHE.PIXm.Patient.json` | `e57717a364ddbd83045ca376a953ee71c3ff84c273a5dc9e4f616033e050b9cf` |
| `package/StructureDefinition-IHE.PIXm.Query.Audit.Consumer.json` | `3cad9d2f3d38a21e5e9ebde120aade95326392f3ff8f542b187b1f225ced6a9d` |
| `package/StructureDefinition-IHE.PIXm.Query.Parameters.In.json` | `b545a09c2fa53b68241b739a9252af1f6fda80d517e2915036925a86a720ae7f` |
| `package/StructureDefinition-IHE.PIXm.Query.Parameters.Out.json` | `3631ba3f4236026ad29a52e560bc54ea9ff52c63c2731255b655df29fa02eda0` |
| `package/example/AuditEvent-ex-auditPixmQuery-consumer.json` | `196acb4772bfb87893d5a318c60e120af2437ff785a658a08d0b09dbfaecf8fe` |
| `package/example/OperationOutcome-pixm-response-error-not-found.json` | `6d12d0aff48a3672b7a859a33b0df7df0defd74c3b875a9af5a546b1b00f1e47` |
| `package/example/Parameters-pixm-request-mohralice-red-all.json` | `9957ade63bcbc551356f49cc90ddc318dbfe2c2d8f4d9f52dee181ab8a540758` |
| `package/example/Parameters-pixm-request-mohralice-red-to-blue.json` | `83499a196da8538c9f2d16c71d0598565c2a5c7f1c7277c88f78509394330fb3` |
| `package/example/Parameters-pixm-response-mohralice-red-all.json` | `1847411fe0bd952528840c278b117b485636469ed4c69b3be0ded0d991ae615c` |
| `package/example/Parameters-pixm-response-mohralice-red-to-blue.json` | `97f664e1c4eca6a9d25168b0d6ffe249167d3496d3f6ea483a650c211b6b5533` |
| `package/example/Patient-Patient-MaidenAlice-Red.json` | `70caf06fcee7e0c100a0d5874c2ebec7a344711c196f257ea98418ea18dda73e` |
| `package/example/Patient-Patient-MohrAlice-Blue.json` | `c63ea52c17341e3d52651a681eadba634db5f4c665e72e4fd7231daaa4de6290` |
| `package/example/Patient-Patient-MohrAlice-Green.json` | `84f5ffae38d62cb0c16fb3b223e72d813ea2dfab14c8cf18bcb1f58ab1fa916e` |
| `package/example/Patient-Patient-MohrAlice-Red.json` | `d3b5fa207a99e689097cc411025042d22cc7d700394d4afb119b8a9c6cda1587` |
| `package/example/Patient-Patient-MohrAlice.json` | `09fcf4ac0546b74d50a4730f2a171016fc3b0c17daa5be878f3bacce9fbd21b7` |
| `package/example/Patient-Patient-MohrAlissa-Red.json` | `0109b7b43c63d5ec1329019059ac4edce1b981105d0d016c59ea315bac3f8466` |
| `package/example/Patient-Patient-MohrMaidenResolvedByMohrMalice-Red.json` | `130d00046c2d391109d0fb13abd06fc2b6824798d6f4f19c3ab06051fc2fcb08` |
| `package/example/Patient-ex-patient.json` | `114b5f86edd1f177412f8833ea2d48619c9482b0d7de9109de897a12d729ca45` |

## What is left out

The package manifest, `package.json`. The script reads its name, version
and licence from the tarball and checks them against the pin. A vendored copy
would make this repository's dependency graph claim an npm package that
depends on `hl7.fhir.r4.core`, a FHIR registry package whose name the GitHub
advisory database flags as a malicious npm package; nothing here installs
either.

| File | sha256 |
|---|---|
| `package/package.json` | `2f1692aabdbb2d5a60650e7d529a341dac8de1668049bd581683394225d9113b` |
