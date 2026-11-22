- A write's BALP record (ITI-104, ITI-93, ITI-94's writes, ITI-8, a
  pseudonym minted on an ITI-83 ask, the operator writes) is inserted into
  the new `pix.audit_outbox` table in the write's own transaction, so the
  change and its record commit together or not at all: a crash before the
  commit leaves neither, and a committed change is never answered `500`
  because its record was late. The record is compressed and sealed under
  the store's key, sent to the repository as a batch Bundle, deleted once
  delivered, and moved to `pix.audit_quarantine` when refused; the readiness
  indicator `audit_outbox` is down while the quarantine holds one. A write
  refused before it changed anything keeps the record of its refusal there,
  and writes no longer take a place in the audit spool.
- A read whose audit record the spool does not store within
  `[audit] spool_write_timeout` is now recorded as refused (outcome `8`)
  and answers `503`; before, its record of success landed later while the
  client saw the refusal.
- The changes of one ITI-93 message commit together with its record. A
  refused change is rolled back on its own; a store failure or contention
  rolls back the whole message and answers `5xx`, and contention tries the
  whole message again. A message of more than 100 changes is refused `413`
  `too-costly` before any change runs.
