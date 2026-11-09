- The HL7 v2 face writes every answer through `ferrobridge-hl7v2` 0.1.203:
  the acknowledgment by its `ack::render`, and the RSP^K23, the RSP^K22 and
  ITI-10's ADT^A31 by its `write::Writer`. The RSP^K23 and the ADT^A31 are
  checked against their structure before they leave: a RSP^K23 that does
  not conform is answered `AE`, and an ADT^A31 that does not conform fails
  the change that would queue it.
