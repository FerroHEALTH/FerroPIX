- The console refuses `session.secure_cookie = false` unless it is plain
  HTTP on loopback: its `oidc.redirect_uri`, or, when it signs in nowhere,
  its `server.listen` address (#358). Before, a console with no `[oidc]`
  section took it on any listen address, and sent its cookies without
  `Secure`.
