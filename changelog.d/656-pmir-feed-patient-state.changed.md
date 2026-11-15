- Breaking for crate users: `ihe-iti` 0.10.0 reads the state of every
  Patient an ITI-93 message carries. `pmir::feed::PatientIdentity` now has
  `active` and every `Patient.link`, read as `pmir::feed::Link` with its
  `LinkType` (FHIR R4 `link-type`) and the other reference, and
  `Feed::read` refuses a created, updated or merged Patient that omits
  `active` (`FeedError::NoActive`) or a link with an unknown type or no
  reference (`FeedError::Link`). A subscriber reads an unmerge, an update
  with `active` true and no `replaced-by` link, from the message alone.
  `nl-generic-functions` 0.10.0 requires `ihe-iti` 0.10.
- The ITI-93 feed FerroPIX sends states `active` and the current links of
  every `Patient`, and a steward's revert of a merge is one message with
  both patients as they now stand, each `active` true with no
  `replaced-by` link (PMIR 1.6.0 §2:3.93.4.1.3).
