- FerroPIX takes `ferrohealth-server` 0.12.2. A request past the overload
  bound is refused before the request id, the request log and the body limit
  run, so the refusal reads no body and leaves no request line. The refusals
  are logged at most once a second with their count, and an admitted request
  costs no more than it did before.
