- The server and the console no longer refuse by name the configuration keys
  v0.0.24 renamed or moved, under the one-release rule for removal aids: the
  millisecond, second, day and byte keys of FerroPIX's own sections
  (`timeout_ms`, `retention_days`, `spool_max_bytes` and the rest), the
  earlier `[terminology]` keys (`base_url`, `timeout_ms`, `retry`,
  `credentials`, `tls`) and `audit.url`, `audit.timeout_ms` and
  `audit.retry_max_ms` that moved under `endpoint`, the renamed keys of the
  shared `[server]`, `[auth]`, `[telemetry]`, `[session]` and `[oidc]`
  sections, and the console's `pix.timeout_ms`. Each now fails at start as
  an unknown key, named in the refusal; write the key that replaced it, as
  the v0.0.24 entries say.
