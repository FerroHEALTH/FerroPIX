- The server and console images build on the distroless static base the
  `nonroot` tag resolves to on 2026-11-03, the compose quickstart's
  development issuer and the testkit's stub OpenID Provider run
  `mock-oauth2-server` 6.1.4, the console journeys run Chrome for Testing
  156.0.8078.80, CI audits the workflows with zizmor 1.32.0 through
  `taiki-e/install-action` 2.90.0, and the release lanes write the build
  SBOMs with syft 1.56.0.
