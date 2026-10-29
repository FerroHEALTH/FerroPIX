- `[server] idle_timeout_ms` (120000 by default) closes a kept-alive HTTP/1.1
  connection that sends no next request within that time, with a clean end
  of stream. The default outlasts the 90 s a common HTTP client keeps an idle
  connection, so the client normally closes first.
