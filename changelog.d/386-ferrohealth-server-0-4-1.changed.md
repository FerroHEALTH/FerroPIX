- FerroPIX runs on `ferrohealth-server` 0.4.1, which keeps a product's own
  fallback and the layers around it when the stack merges its routes
  (FerroHEALTH/FerroSYS#199). FerroPIX was not exposed to the regression
  0.4.1 fixes: every release ran on 0.3.8, which predates it, and the change
  that took 0.4.0 declared the fallback of the FHIR face and the operator API
  inside their guard, so an anonymous request to a path no route matches is
  refused with `401` before and after.
