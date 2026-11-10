- With `[pmir]` on, `GET {base}/fhir/metadata` declares the ITI-93 message
  as `sender` and as `receiver`, as the PMIR 1.6.0 Patient Identity Registry
  statement does: the Registry sends the feed to each subscriber. With
  `[pixm]` and `[pdqm]` on as well, the statement instantiates
  `https://profiles.ihe.net/ITI/PMIR/CapabilityStatement/IHE.PMIR.PatientIdentityRegistry`.
  `implementation.description` now names the IHE actors the instance
  serves.
