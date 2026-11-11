- Each release attaches the quickstart as one self-contained `compose.yaml`,
  with its `.sha256sum`. The server's configuration and the database role
  are inlined as Compose configs, so the file needs Docker Compose 2.23.1 or
  newer and no checkout, and the README and book quick start download it
  from the latest release. `scripts/quickstart/compose-configs.sh` holds
  each inlined copy to its source in the repository, and CI runs the
  quickstart tour over that same file (#619).
