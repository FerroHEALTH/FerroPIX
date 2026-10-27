- The Helm chart is 0.3.2: the server and the console containers carry an
  ephemeral-storage request and limit beside their memory limit
  (`resources`, `console.resources`; 256 MiB for the server, which holds the
  audit spool on its default `emptyDir`), and `deploy/helm/validate.sh`
  refuses a container without both limits (#360).
