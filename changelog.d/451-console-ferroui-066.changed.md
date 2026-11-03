- The operator console draws with FerroUI 0.6.6 and signs in through
  `ferrohealth-session` 0.2.0, whose `[oidc]` section takes `resolve`, to
  reach a provider host at a fixed address, and `[oidc.tls]`, the client
  certificate the console presents to the provider and the lowest TLS
  version of that connection. The server takes `ferrohealth-smart` 0.8.8,
  which that release requires.
