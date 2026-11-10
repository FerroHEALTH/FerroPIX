- Each release attaches the quickstart as a bundle: `quickstart.tar.gz`,
  with its `.sha256sum`, holds `compose.yaml` with the server and console
  images pinned by the release's digests and every file it mounts, and the
  pinned `compose.yaml` rides beside it. CI builds the same bundle over the
  images it builds and runs the quickstart tour from the unpacked bundle
  (#619).
