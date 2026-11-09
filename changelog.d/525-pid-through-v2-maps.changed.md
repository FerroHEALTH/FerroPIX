- The HL7 v2 face maps PID to a `Patient` and back through
  `ferrobridge-hl7v2`'s shipped v2-to-FHIR maps, translated in process,
  in place of FerroPIX's own copy of them. A cell phone (XTN-3 `CP`) is fed
  as a `phone` in `mobile` use, and a number with no equipment type keeps a
  `system` carrying the data-absent-reason `unknown`, where the face fed
  `phone` before. An ITI-21 PID leaves XTN-2 empty on an email the
  `Patient` gives no `use`, where it wrote `NET`.
