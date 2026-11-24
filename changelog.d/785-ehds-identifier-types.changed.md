- Breaking: every identifier domain names the type of its identifiers,
  `identifier_type`, which `[[domain]]`, `POST /operator/domains` and the
  console's domain form require. The value set is the HL7 Identifier Type
  (`NI`, `SS`, `PI`, `MR`, `PPN`, `NH`, `JHN`, `HC`) extended with the EHDS
  codes of the draft EEHRxF Annexes, Table 26 (`HI`, `HIN`, `REGPID`,
  `REGION`). A migration gives every stored domain `PI`, or `NI` when it is
  authoritative. Every `Patient` FerroPIX serves (PDQm ITI-78 and ITI-119,
  the PMIR ITI-93 feed and XCPD) carries `identifier.type` on each
  identifier: a coding of `http://terminology.hl7.org/CodeSystem/v2-0203`,
  or, for the four EHDS codes, which no published code system holds yet, the
  code as `type.text`.
