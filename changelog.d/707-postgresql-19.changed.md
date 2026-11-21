- FerroPIX runs on PostgreSQL 19 and every store job (`serve`, `db
  migrate`) refuses an older server, naming the `server_version_num` it
  found and the 190000 it needs. The quickstart, the testkit and the
  chart's cluster check run `postgres:19.0`. PostgreSQL 18 is no longer
  supported and there is no upgrade path: a quickstart volume holding an 18
  cluster is removed and created again.
