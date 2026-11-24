- Breaking: the issuer keys `kind` (other than `"federation"`), `preset`,
  `profile_file` and `scopeless_subjects`, the `[auth.idp]` table, and the
  profile keys `first_party_clients`, `delegates`, `audience_delegates`,
  `issuer_client_is_person`, `patient_context`, `fhir_user_patient`,
  `representation`, `act_represents_patient`, `guest`, `client_claims` and
  the other vendor profile keys are removed. A configuration that sets one is
  refused at start as an unknown key; remove the key, and register each
  client at the authorization server.
