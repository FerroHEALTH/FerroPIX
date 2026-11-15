- Breaking for crate users: `ihe-iti` 0.11.0 requires `ihe-atna` 0.19,
  and `nl-generic-functions` 0.11.0 requires `ihe-iti` 0.11. Their public
  API carries `ihe-atna` types (the audit user an exchange is made for), so
  a consumer moves `ihe-atna`, and `ehds-logging` with it, to 0.19 in the
  same change and links one copy. The server pins `ihe-atna` 0.19.0, which
  changes nothing FerroPIX records.
