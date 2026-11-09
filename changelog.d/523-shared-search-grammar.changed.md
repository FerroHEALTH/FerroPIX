- `ihe-iti` reads and writes every FHIR search value through the family's
  FHIR R4 search grammar, `fhir-search` (exact `=0.1.129`, with no default
  feature), the one copy FerroBRIDGE's facade and `ihe-atna` read theirs
  with: the PIXm Manager, the PDQm Supplier and the PMIR Registry read their
  tokens, splits and escapes through it, and the PDQm `birthdate` its prefix
  and date. As FHIR R4 search.html §3.1.1.4.19 requires, a `\` before any
  character other than `\`, `,`, `$` and `|` is refused
  (`SearchError::Escape` and `pixm::server::query::QueryError::Escape`, both
  new), and a token with a second unescaped `|` is refused where it was read
  into the code. A `birthdate` must be a calendar day, so `1923-02-30` is
  refused. `pdqm::query::DatePrefix` gives way to
  `fhir_search::prefix::Prefix`; to upgrade, pass `Prefix` to
  `PatientQuery::birthdate` and read it from `DateValue::prefix`.
  `nl-generic-functions` escapes its NVI search values with the same
  grammar (#523).
