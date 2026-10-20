<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: the IHE PDQm FHIR package

Vendored verbatim by `scripts/vendor/ihe-pdqm.sh`. Never edit a file here:
change the pin in docs/VERSIONS.md and re-run the script.

- Source: <https://packages.fhir.org/ihe.iti.pdqm/3.2.0>, the FHIR package registry's copy of the IG published at
  <https://profiles.ihe.net/ITI/PDQm/3.2.0/>
- Pin: package `ihe.iti.pdqm` version `3.2.0`, tarball sha256 `61e09fbee991ff7c131b6ba5474921001e07782209961f3e85cee5f3ebcaedc2`
- Fetched: 2026-10-20
- Upstream licence: Creative Commons Attribution 4.0 International
  (`CC-BY-4.0`, the `license` of the package manifest, listed under What is
  left out;
  <https://creativecommons.org/licenses/by/4.0/>). The package ships no licence
  file of its own. Attribution: IHE International, IT Infrastructure Technical
  Committee, *Patient Demographics Query for Mobile (PDQm)* 3.2.0.
- FHIR version: 4.0.1
- Layout: the upstream paths inside the package, unchanged
- Files: 26 of the package's 53, listed below
- Tree digest (sha256 over the sorted per-file `sha256  path` listing,
  `PROVENANCE.md` excluded): `cfcd10d2402dcc4a3b659ac06b8242a3d5baadcd12d69cb0ef4c7e3e5c628a00`
- Read by: the ITI-78 client of `crates/ihe-iti` (feature `pdqm`), whose
  tests hold the query to the Supplier's Patient search parameters and decode
  the example response Bundle and Patients, and its ITI-119 `$match`
  client, held to the operation, its input and output profiles and their
  examples. The audit artefacts are kept as FerroFED vendors them

## What is here

The artefacts of ITI-78, Mobile Patient Demographics Query: the Patient
Demographics Consumer (Query) and Supplier capability statements, which list
the Patient search parameters a Supplier processes, the Query Patient Resource
Response Message profile of the `searchset` Bundle, the PDQm Patient profile,
the ImplementationGuide, and the IG's examples of a response Bundle and two
Patients, with the Consumer's ITI-78 audit profile, built on the BALP Patient
Query pattern, and its example. With them, the artefacts of ITI-119, Patient
Demographics Match: the `$match` OperationDefinition, the Consumer and
Supplier match capability statements, the input Parameters, input Patient and
output Bundle profiles, the IG's input and output examples, and the
Consumer's ITI-119 audit profile and its example. The package's other files
serve no reader here: the Supplier audit profiles and their examples, the
OperationOutcome examples, the XML renderings, and the registry's validation
output. They are not taken.

| File | sha256 |
|---|---|
| `package/CapabilityStatement-IHE.PDQm.PatientDemographicsConsumerMatch.json` | `f329869a114e4eea3c852c8be03f34e493449399339162609cc97a7ecf450c1c` |
| `package/CapabilityStatement-IHE.PDQm.PatientDemographicsConsumerQuery.json` | `c32bc60f13e95091f4564b62ce037d55a2d827b4e58120229f8ad7ed29e32c4d` |
| `package/CapabilityStatement-IHE.PDQm.PatientDemographicsSupplier.json` | `b5a57396695b4ada78474d47e2a77b6d7f82f79a89a3f9d3ff4735fda885406f` |
| `package/CapabilityStatement-IHE.PDQm.PatientDemographicsSupplierMatch.json` | `e96c006183c5ec0d74cf49792bf605455be382a9e92889600ee92167aca47b7e` |
| `package/ImplementationGuide-ihe.iti.pdqm.json` | `05acdb5bf6b8e021b99b8fc845484c938dd3e2c4abba4e0c9b836979065c0c5f` |
| `package/OperationDefinition-PDQmMatch.json` | `1cfc61f1d9373172f3050357d5a9f75ff642ae5eb58af68df27a7b4452e6c57f` |
| `package/StructureDefinition-IHE.PDQm.Match.Audit.Consumer.json` | `3bef44c549f0d948f5469a1f876576a04f0d6ceedfb1421f2b4cf1c8182e9648` |
| `package/StructureDefinition-IHE.PDQm.MatchInputPatient.json` | `82e1557949b25057e284f4886795bf5219ac9df62bfa00926bfd7295eda90de5` |
| `package/StructureDefinition-IHE.PDQm.MatchParametersIn.json` | `08dec57c385a3c7fd808dfa0e52059224855199ce6a674f1f1bf98370651949b` |
| `package/StructureDefinition-IHE.PDQm.MatchParametersOut.json` | `10d0969fb4e10bb4bf767ee237900f25a963ffe0c15a801a517900ae7f15d950` |
| `package/StructureDefinition-IHE.PDQm.Patient.json` | `d184b3cfb58b91eecb87e6ee865c7108a779a2efe942a580c166776d8d704549` |
| `package/StructureDefinition-IHE.PDQm.Query.Audit.Consumer.json` | `52742d6b2081eda2a3589cc985c12d3ac345325456e5929001ca2bd8c3903453` |
| `package/StructureDefinition-IHE.PDQm.QueryPatientResourceResponseMessage.json` | `0488cf89754f6c914463789758919bec63f3ce297dc6d11088c0eafe0f31f590` |
| `package/example/AuditEvent-ex-auditPdqmMatch-consumer.json` | `00b8f8dd97084e4d48cc5a44fe65b7f7035a56b128f6fc98bbb21f9c28efce41` |
| `package/example/AuditEvent-ex-auditPdqmQuery-consumer.json` | `9f6e1844440b6d2399204c1c6d701cd0106c2a847b9f4e9a26921c698ad97d38` |
| `package/example/Bundle-ex-QueryPatientResourceResponseMessage.json` | `851a55358466f91e261ee90c3f03c3de605679f1b3d8abe19d082bf8da6a9f39` |
| `package/example/Bundle-ex-match-output-empty.json` | `8ffaca0ec1518ef027278c7abcc5070b0e30002cc85489d59a99466c319d5365` |
| `package/example/Bundle-ex-match-output-error.json` | `fff8f751b9f8c7556035c21056318b31fde449383d7fbd3c3348dcd8e675e3b8` |
| `package/example/Bundle-ex-match-output-multiple.json` | `8ac4eab12b2d03db8628d1e8ff8cc411c14eb98ac22659fbf56c56b68ebbed9b` |
| `package/example/Bundle-ex-match-output-warning.json` | `056a561f183d89aeeaa15fc0d3c34d88beb26e45fe11c3bdaf01f2e01c4477de` |
| `package/example/Bundle-ex-match-output.json` | `63bdf9195dda1d8920af99c6865cffb9125e6371ff631b5af643e231b22ae18d` |
| `package/example/Parameters-ex-match-input-onlyCertainMatches.json` | `22dbacd166d313593455fb94b0e880163ad0eb08c8a146d45ddbf6692dce93cb` |
| `package/example/Parameters-ex-match-input-patient-only.json` | `32f0276adc28f145289ecd759c7c7753e344d067b8ee351dcd06b5ec76fadf63` |
| `package/example/Patient-ex-match-input-patient.json` | `b708f5ebd9fa7da094ebcaa4058e7c460490c93f317f57e55e4aa57d525d6ea3` |
| `package/example/Patient-ex-patient-mothers-maiden-name.json` | `3b8396d82d2cb858c0b2124d75809156ca4f516b8c74636098201f64546a3e09` |
| `package/example/Patient-ex-patient.json` | `83f68fd6bf11e0efd8c12bea2e4536c4415c1ee77f22cfbc91cc30768d6f1014` |

## What is left out

The package manifest, `package.json`. The script reads its name, version
and licence from the tarball and checks them against the pin. A vendored copy
would make this repository's dependency graph claim an npm package that
depends on `hl7.fhir.r4.core`, a FHIR registry package whose name the GitHub
advisory database flags as a malicious npm package; nothing here installs
either.

| File | sha256 |
|---|---|
| `package/package.json` | `fbd6988ebb77e8fed14590e874704b4a775a2470e08b4673c9c6bdb1f6dd4288` |
