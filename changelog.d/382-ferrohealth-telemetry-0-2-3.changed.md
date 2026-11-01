- The server takes `ferrohealth-telemetry` 0.2.3. Each exported request span
  is a server span named `{method} {route}`, such as
  `GET /fhir/Patient/$ihe-pix` (the method alone when no route matched),
  where it was named `request`, and its status is an error on a 5xx, under
  the OpenTelemetry HTTP semantic conventions. A method outside the
  standard set reads `_OTHER` in the request line and the span. `/metrics`
  gains `http_server_active_requests`, the requests in flight by method and
  scheme. `[telemetry.otlp]` refuses `trust_roots_file` or
  `client_identity_file` beside an endpoint that is not `https://`, and a
  file that is no PEM of its kind, where it exported in plain text with the
  files ignored. Migration: point the endpoint at `https://` when the
  collector speaks TLS, or drop the two keys when it does not.
