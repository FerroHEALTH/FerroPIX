<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: the IHE ATNA audit text

Vendored verbatim by `scripts/vendor/ihe-atna.sh`. Never edit a file here:
change the pins in docs/VERSIONS.md and re-run the script.

- Sources:
  - <https://profiles.ihe.net/ITI/TF/Volume2/ITI-20.html> and the figure it shows,
    <https://profiles.ihe.net/ITI/TF/Volume2/media/Figure_3.20.4-1.png>: IHE IT Infrastructure Technical Framework Volume 2, §3.20 Record Audit Event
    [ITI-20], Revision 20.2, November 11, 2025, Final Text
  - <https://www.ihe.net/uploadedFiles/Documents/ITI/IHE_ITI_Suppl_RESTful-ATNA.pdf>: IHE IT Infrastructure Technical Framework
    Supplement *Add RESTful ATNA (Query and Feed)*, Rev. 3.6,
    November 25, 2025, Trial Implementation
- Pins: each file by its sha256, listed below; the URLs carry no revision
- Fetched: 2026-10-20
- Upstream licence: copyright IHE International, Inc. The IHE Technical
  Frameworks General Introduction §9
  (<https://profiles.ihe.net/GeneralIntro/ch-9.html>) grants it, verbatim:
  "IHE International hereby grants to each Member Organization, and to any
  other user of these documents, an irrevocable, worldwide, perpetual,
  royalty-free, nontransferable, nonexclusive, non-sublicensable license
  under its copyrights in any IHE profiles and Technical Framework documents,
  as well as any additional copyrighted materials that will be owned by IHE
  International and will be made available for use by Member Organizations,
  to reproduce and distribute (in any and all print, electronic or other
  means of reproduction, storage or transmission) such IHE Technical
  Documents." Both texts refer to DICOM PS3.15 Annex A.5 and HL7 FHIR R4 by
  link; neither base standard is vendored here.
- Layout: the page under `Volume2/` with its figure at the relative path the
  page links (`media/`), the supplement at the top
- Files: 3
- Tree digest (sha256 over the sorted per-file `sha256  path` listing,
  `PROVENANCE.md` excluded): `d88b675acba02428ef2f73f020b781ef648ff45e2a14f269be4c9f026a818a4c`
- Read by: the ITI-20 citations of `crates/ihe-iti` (feature `audit`),
  held to these files by `scripts/checks/ihe-citations.sh`

## What is here

`Volume2/ITI-20.html` is the whole Record Audit Event transaction: the
Send Audit Event syslog interaction (§3.20.4.1) with its trigger events and
the store-and-forward a sender that cannot reach its repository keeps
(§3.20.4.1.1), the syslog message semantics and transports (§3.20.4.1.2), and
the IHE Audit Trail Message Format (§3.20.7). The page's stylesheets, scripts
and logo serve no reader here and are not taken. The supplement adds the
ATX: FHIR Feed Option (ITI TF-1 §9.2.7.1) and the FHIR Feed interactions of
ITI-20: Send Audit Resource Request (§3.20.4.2), its mapping from the DICOM
audit message to the FHIR `AuditEvent` (Table 3.20.4.2.2.1-1) and the
Send Audit Resource Response (§3.20.4.3). The status is the document's own:
a Trial Implementation supplement may be amended before it is incorporated
into the Technical Framework.

| File | sha256 |
|---|---|
| `IHE_ITI_Suppl_RESTful-ATNA.pdf` | `d8451a4a0d951662b6a04b745084c33afff6196db5647f2cf79d9149dfa7265a` |
| `Volume2/ITI-20.html` | `881c7d6423fdf5ecaf4f9f50f8d25be61c3ed8ef97c87eff9591bd7fdf51570d` |
| `Volume2/media/Figure_3.20.4-1.png` | `7aba1a2437e3492202460e150a6dda85b8a1886035bd2aa8daa4c892a579b734` |
