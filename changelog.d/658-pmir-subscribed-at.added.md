- `ihe-iti` 0.10.1 makes `pmir::subscription::Subscribed::at` public: a
  PMIR subscriber that keeps no state between requests rebuilds the
  subscription handle from the `Location` it stored, to read its status or
  delete it. A `Location` off the Registry's `[base]/Subscription` is
  refused as before, so the subscriber's credential never goes elsewhere.
