- An `[[auth.issuer]]` that sets `role_claims` refuses the start, naming
  `auth.issuer[N].role_claims`: FerroPIX's permission table reads scopes,
  so a role read from the token would never apply. Drop the key from the
  issuer's table.
