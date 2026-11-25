- `ihe-iti` 0.23.0 requires `ihe-atna` 0.28, whose `user::PurposeOfUse`
  gains `text`, the purpose as the user stated it, and
  `nl-generic-functions` 0.23.0 requires `ihe-iti` 0.23.0. Both are breaking
  for a consumer that names `ihe-atna` types: move `ihe-atna` to 0.28 with
  them and give every `PurposeOfUse` literal its `text`.
