- The console's Sign out goes to the provider's end-session endpoint only
  when the session it ends holds an ID Token, and otherwise to the
  signed-out page, through `ferrohealth-session` 0.2.1. A Basic sign-out on
  a console with a provider configured went to the provider, which held no
  session of the operator's to end.
