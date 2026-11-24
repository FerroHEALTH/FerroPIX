- FerroPIX takes `ferrohealth-server` 0.12.2 and `ferrohealth-secret` 0.2.2,
  with `ihe-iti` 0.21.1 and `nl-generic-functions` 0.21.1 on the same
  secret line. A request past the overload bound is refused before the
  request id, the request log and the body limit run, so the refusal reads
  no body and leaves no request line. The refusals are logged at most once a
  second with their count, and an admitted request costs no more than it did
  before.
