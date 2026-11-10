- `[auth]` refuses the start on a table FerroPIX would read and never
  apply, naming the key: `[auth.dpop]` (FerroPIX checks no DPoP proof),
  `[[auth.party]]` and `[auth.idp]` (FerroPIX binds no login to a party),
  and a Basic user's `roles` (the permission table reads scopes). A
  configuration that sets one of them must drop it.
  `[[auth.federation]]` and `[[auth.attributes]]` are applied, and the book
  documents both. The IHE IUA attributes that an `[[auth.attributes]]` table
  reads from a token (the user's name, role, purpose of use, provider id and
  organisation) now appear on the BALP record's user agent, as BALP 1.1.4
  §3:5.7.5.4 maps them.
