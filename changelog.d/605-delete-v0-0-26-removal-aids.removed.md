- The Helm chart's values schema, which refused `config.auth.audience` by
  name in the charts of v0.0.26. The value now reaches the server's
  configuration as an unknown key under `[auth]`, and the `config check`
  each image runs over the rendered configuration refuses it. Write each
  issuer's `audience` under `config.auth.issuer`, as v0.0.26 asked.
