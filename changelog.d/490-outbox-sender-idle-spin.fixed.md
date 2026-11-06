- The outbox sender sleeps again while nothing waits in the outbox. Its look
  at the next due row answered "due now" for an empty outbox, or one holding
  only delivered or abandoned rows, so an idle server ran its claim back to
  back: about 8,000 PostgreSQL transactions a second and over a CPU core
  between the server and its database. A claim that takes no row is now
  always followed by a wait of at least a second.
