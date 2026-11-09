- The XCPD Responding Gateway reads its delivery's outbound TLS from
  `[xcpd.tls]`, the family's `TlsClientSection` of `ferrohealth-server`
  0.6.3: `client_identity_file` (or `client_identity`) holds the client
  certificate chain and its private key in one PEM, `trust_roots_file` the
  roots trusted beside the platform's, and `min_version` the oldest TLS
  version, `1.3` unless lowered to `1.2`. Upgrade: join the files of
  `[xcpd] client_certificate_file` and `client_key_file` into one PEM and
  name it in `[xcpd.tls] client_identity_file`, and move
  `[xcpd] reply_trust_file` to `[xcpd.tls] trust_roots_file`; in this release the
  old keys refuse to load and name their replacement, and from v0.0.23 each
  fails as an unknown key (#558). A reply endpoint that speaks
  only TLS 1.2 now needs `[xcpd.tls] min_version = "1.2"`.
