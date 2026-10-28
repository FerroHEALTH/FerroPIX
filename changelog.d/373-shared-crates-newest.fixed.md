- `[auth] ca_file` is trusted under `mode = "introspection"` too: with
  `ferrohealth-smart` 0.8.6 the server reads the issuer's metadata, calls
  the introspection endpoint and requests its own token trusting it, so an
  issuer behind a private certificate authority is reached with the setting
  that reaches it under `mode = "jwt"`.
- The console takes `ferrohealth-ui` 0.5.5: the skip link moves the focus
  to the main area once the page has hydrated, so the next Tab reaches the
  page's first control (WCAG 2.2 SC 2.4.1); a search answered with `401`
  sends every other tab of the console to sign-in; and the search panel no
  longer panics when a navigation leaves the page while it opens.
