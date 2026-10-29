- On a kept-alive HTTP/1.1 connection, `[server] header_read_timeout_ms`
  counts from the first byte of each request head. It used to count from the
  end of the previous answer, so it also closed an idle connection after
  30 s, sometimes as a client sent its next request into it, which a `POST`
  cannot safely retry.
