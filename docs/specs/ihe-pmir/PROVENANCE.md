<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: the IHE PMIR FHIR package

Vendored verbatim by `scripts/vendor/ihe-pmir.sh`. Never edit a file here:
change the pin in docs/VERSIONS.md and re-run the script.

- Source: <https://packages.fhir.org/ihe.iti.pmir/1.6.0>, the FHIR package registry's copy of the IG published at
  <https://profiles.ihe.net/ITI/PMIR/1.6.0/>
- Pin: package `ihe.iti.pmir` version `1.6.0`, tarball sha256 `ec9d25fc64ac2f3087f921c14c0da56afc7e794caa80298db3f130bc0a40fe70`
- Fetched: 2026-10-20
- Upstream licence: Creative Commons Attribution 4.0 International
  (`CC-BY-4.0`, the `license` of the package manifest, listed under What is
  left out;
  <https://creativecommons.org/licenses/by/4.0/>). The package ships no licence
  file of its own. Attribution: IHE International, IT Infrastructure Technical
  Committee, *Patient Master Identity Registry (PMIR)* 1.6.0.
- FHIR version: 4.0.1
- Layout: the upstream paths inside the package, unchanged
- Files: 29 of the package's 87, listed below
- Tree digest (sha256 over the sorted per-file `sha256  path` listing,
  `PROVENANCE.md` excluded): `5370ae99fe47215e2dcae2d42b1949b20c587627c10b89f8499fb145364e0fd1`
- Read by: the ITI-94 subscriber and the ITI-93 feed reader of
  `crates/ihe-iti` (feature `pmir`), whose tests decode the example
  message Bundles and hold the subscription and the feed response to the
  profiles. The audit artefacts are kept as FerroFED vendors them

## What is here

The artefacts of ITI-93, Mobile Patient Identity Feed, and ITI-94, Subscribe
to Patient Updates: the Patient Identity Consumer, Registry, Source and
Subscriber capability statements, the feed MessageDefinition and its response,
the message Bundle, history Bundle, MessageHeader, MessageHeader response and
merged Patient profiles, the Subscription and Subscription request profiles,
the ImplementationGuide, and the IG's examples of the create, update, delete
and merge message Bundles, a response MessageHeader and the two
Subscriptions, with the audit records of the two transactions (§2:3.93.5.1,
§2:3.94.5.1): the Feed audit profile and the Consumer's example, and the
Subscription Create, Read and Delete audit profiles and the Subscriber's
examples. The package's other files serve no reader here: the Subscription
Update audit profile and the other BALP audit examples, the related-person
profiles and examples, the
standalone history Bundle and Patient examples, which the message Bundle
examples taken here inline, the OpenAPI and XML renderings, and the registry's
validation output. They are not taken.

| File | sha256 |
|---|---|
| `package/CapabilityStatement-IHE.PMIR.PatientIdentityConsumer.json` | `57042941cb039e04721143d143466476aff623e6368eea0ee37311ed453d5819` |
| `package/CapabilityStatement-IHE.PMIR.PatientIdentityRegistry.json` | `93aa5ba56e6e87efa291679ebd146e7a8b215fb7ee16b378dc21301f2281167a` |
| `package/CapabilityStatement-IHE.PMIR.PatientIdentitySource.json` | `e7837ff1cb389533101df6bdc389e0fdfb90880db292f5a02af6e2bd5d618956` |
| `package/CapabilityStatement-IHE.PMIR.PatientIdentitySubscriber.json` | `b58630feabd8343ee095590d6919ae2940362fa4235ef8b9f5a982c1d833aa72` |
| `package/ImplementationGuide-ihe.iti.pmir.json` | `e444ef5502e4870413152fe0982ea23b904b4274d7cdc76daa6fb64f715d026d` |
| `package/MessageDefinition-IHE.PMIR.MessageDefinition.Response.json` | `0da4d5bcc6dd34a0712d84d796aaf1c8f80cd1d1416c280e417e045152d5665a` |
| `package/MessageDefinition-IHE.PMIR.MessageDefinition.json` | `de2170c3047f68185ba6c07251e87f9feb44ee9bc1d2b4418572102f3056db7c` |
| `package/StructureDefinition-IHE.PMIR.Audit.Subscription.Create.json` | `b182b1000316e34f2f3e0e0c43628e11dd8cdbab26b1455b0cdf3ac9718eacbf` |
| `package/StructureDefinition-IHE.PMIR.Audit.Subscription.Delete.json` | `66aaa37cfe2835cac9901ffb9f565db1263ef0e727d325587415de649f6e88f4` |
| `package/StructureDefinition-IHE.PMIR.Audit.Subscription.Read.json` | `37ed29e9bbeae27d204b4002a80d11f8b32afdbab263a49d87e50446406ad318` |
| `package/StructureDefinition-IHE.PMIR.Bundle.History.json` | `88fd05bfe3e2e1ec28312de304b9f003db03323755d0cdc0b2843f4a3c785fe8` |
| `package/StructureDefinition-IHE.PMIR.Bundle.json` | `d664d22f6c472ed569721be5406974a5b76a7de7088062714e643d7e9bc22e3b` |
| `package/StructureDefinition-IHE.PMIR.Feed.Audit.json` | `fa9ff77d60a7659c11e7a8508e63a8d976769e23224261cc33c03193ca90567c` |
| `package/StructureDefinition-IHE.PMIR.MessageHeader.Response.json` | `9d40d477a9a922614bdf2869cab06d6a7a8befd8ed3a174e947104fe717abffe` |
| `package/StructureDefinition-IHE.PMIR.MessageHeader.json` | `edc788acb8f5a7ebb224e200387e995c53cafeed7801bfa9c68d71055bf2f756` |
| `package/StructureDefinition-IHE.PMIR.Patient.Merge.json` | `38b590ebe68b7c42d1f82fb2b12f1e0b7b7ebbf241546aabd8619d2bf00d68d4` |
| `package/StructureDefinition-IHE.PMIR.Subscription.Request.json` | `978e2ecd778a37613bc8de49c72106e0b073f084f1a99824261370f1cf1a1ca8` |
| `package/StructureDefinition-IHE.PMIR.Subscription.json` | `9b993f76527ceaebd3e7a1f0bbd28600aaec75644f88ad3164fc22795beb0e69` |
| `package/example/AuditEvent-ex-auditPmirFeed-consumer.json` | `505712635ef541a227af3d7fd09e6d231788d2eaaa5fd05561ff87f6181caf1a` |
| `package/example/AuditEvent-ex-auditPmirSubscription-subscriber-create.json` | `649fff0569b6cd07ffbd19afecef0497d442dccd68a7eb8e86f21bbab98e70e2` |
| `package/example/AuditEvent-ex-auditPmirSubscription-subscriber-delete.json` | `ce6e6253f58d123f94c772085ad6fed607d022064b0fe33159d743d0349f1bd0` |
| `package/example/AuditEvent-ex-auditPmirSubscription-subscriber-read.json` | `89ac10caaec48f1b04853c2af0beea60159662b1f4ea093782aed893e0b09356` |
| `package/example/Bundle-ex-PMIRBundleCreate.json` | `07081c55732b390b80c7533828b4808ca601c55b7bcc1e3ae67427a6d29aa3af` |
| `package/example/Bundle-ex-PMIRBundleDelete.json` | `b18ef3da015f688d7cd13fe1ff34c6300fba278cba944ee9489087bd22d1716f` |
| `package/example/Bundle-ex-PMIRBundleMerge.json` | `56327f5cb3c268e72e4b274918f60aa066b8090af8560759fbdeb9b937012e1c` |
| `package/example/Bundle-ex-PMIRBundleUpdate.json` | `a6c6034e24dab0c4f7c819780d8eca8f4b2f0a682041e38c7f8da821dd8da4fd` |
| `package/example/MessageHeader-ex-messageheader-create-response.json` | `bd09112d19cfd85985f800792bc8366828d5d034314e90e7980453480019995f` |
| `package/example/Subscription-ex-subscription-request.json` | `71d73c68243516f7dd1969b90a19441f62f0a93942e3462a3259baccfeee9f07` |
| `package/example/Subscription-ex-subscription.json` | `d37baa99e3a92c6a00c5225e8f5736ce5527bf8acb01121422648b16e8fb612e` |

## What is left out

The package manifest, `package.json`. The script reads its name, version
and licence from the tarball and checks them against the pin. A vendored copy
would make this repository's dependency graph claim an npm package that
depends on `hl7.fhir.r4.core`, a FHIR registry package whose name the GitHub
advisory database flags as a malicious npm package; nothing here installs
either.

| File | sha256 |
|---|---|
| `package/package.json` | `99d7807ccc8e73ba41fd805df97de3ef9ec8abbe1f2554be92f7ec2f81480b1e` |
