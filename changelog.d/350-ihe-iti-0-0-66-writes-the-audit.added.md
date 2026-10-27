- `ihe-iti` 0.0.66 writes the audit records of the PIXm Manager and the
  PDQm Supplier behind `balp` (#350): `pixm::audit::manager_query` and
  `manager_feed` for ITI-83 and ITI-104, `pdqm::audit::supplier_records`
  for ITI-78 and ITI-119, from a `received::Received` request. The server
  writes its records through them, and the audit corpus checks all 13 IHE
  examples, both actors of each transaction.
