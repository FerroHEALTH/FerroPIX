- `ihe-iti` 0.21.2 adds the PIXm Patient Identity Source (ITI-104) behind
  the new `pixm-source` feature: `pixm::source::PixmSourceClient` sends Add
  or Revise Patient and Resolve Duplicate Patient as the conditional update
  `PUT [base]/Patient?identifier=system|value` and Remove Patient as the
  conditional delete, over the same outbound client and credentials as the
  ITI-83 Consumer. Each answer is a typed outcome: `Fed::Created` (`201`),
  `Fed::Updated` (`200`), `Removed::Deleted` (`200`, `204`),
  `Removed::Accepted` (`202`), `SourceError::NotSelective` (`412`) and
  `SourceError::Rejected` with the status and the `OperationOutcome` issue
  types for any other answer. With `balp`, each message is recorded as the
  PIXm Feed Update or Delete Source audit profile fixes it
  (§2:3.104.5.1.1), and the vendored PIXm package now carries those two
  profiles and their examples.
