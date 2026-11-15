- The ITI-93 sender authenticates to each Patient Identity Consumer named
  in `[[pmir.feed.consumers]]`, the family's endpoint table: a message to a
  subscription whose channel endpoint lies under the entry's `url` carries
  its IUA client credentials token (`credentials.oauth`), renewed once when
  the Consumer answers `401`, and presents its client certificate for ATNA
  mutual TLS (`tls.client_identity_file`), as PMIR 1.6.0 §2:3.93.5 asks.
  The token replaces any `Authorization` header the subscription names,
  and an endpoint under no entry gets no credential. A `401` or `403` after
  the renewal is retried within `[pmir.feed]` and then sets the
  subscription to `error` naming the refusal.
