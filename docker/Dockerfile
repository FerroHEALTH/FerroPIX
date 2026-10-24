# syntax=docker/dockerfile:1
# SPDX-FileCopyrightText: Cadasto B.V.
# SPDX-License-Identifier: BUSL-1.1
# The FerroPIX server image: one static binary on distroless static, per
# platform. Nothing is compiled here. The release lane builds and attests the
# musl binaries (.github/workflows/release-build.yml) and release-image.yml
# stages them under dist/<os>_<arch>/ferropix before this file is built for
# linux/amd64 and linux/arm64 (docs/release.md). The root .dockerignore holds
# the build context to dist/.
#
# Build from a staged dist/ tree, at the repository root
# (scripts/release/stage-dist.sh stages a published release):
#   docker buildx build -f docker/Dockerfile --platform linux/arm64 .

# distroless static: ca-certificates, tzdata, an /etc/passwd carrying the
# nonroot user (uid 65532), a /tmp, no shell and no package manager. Pinned by
# the digest of its image index, which is what a multi-platform build
# resolves; docs/VERSIONS.md pins it, scripts/checks/versions.sh holds this
# line to that row and scripts/checks/pin-freshness.sh reads the tag's digest
# every week. https://github.com/GoogleContainerTools/distroless
FROM gcr.io/distroless/static-debian13:nonroot@sha256:18f4cc26c9ffa7f20a359298be7deadac5c607c8e03a02b8119f1a5fc2f82c99

# The platform of the manifest being built, set by buildx per platform.
# https://docs.docker.com/reference/dockerfile/#automatic-platform-args-in-the-global-scope
ARG TARGETOS
ARG TARGETARCH

# Root-owned, executable, and not writable by the runtime user.
COPY --chmod=0555 dist/${TARGETOS}_${TARGETARCH}/ferropix /usr/local/bin/ferropix

# The server's own state: the audit spool under audit/ ([audit] spool_dir, a
# directory of 0700 that holds each BALP record until the repository has it,
# and that the spool refuses to open when any other user may read it) and
# the usage report's instance id ([usage_report] state_dir). Both are owned
# by the runtime user, so a named volume mounted at /var/lib/ferropix starts
# with that ownership. The base has no shell to `mkdir`, so
# scripts/release/stage-dist.sh stages the tree under dist/state/ with its
# modes, and COPY keeps them.
COPY --chown=65532:65532 dist/state/ /var/lib/ferropix/

# The binary's own default is loopback, which no container can publish, so the
# image listens on every interface on the documented port. The MLLP listener
# of [hl7v2] binds 2575 when a deployment turns it on. Every key of the
# configuration tree is FERROPIX__<SECTION>__<KEY>, and every secret is read
# from the file its `_file` sibling names.
ENV FERROPIX__SERVER__LISTEN=0.0.0.0:8080
EXPOSE 8080/tcp 2575/tcp

# Numeric on purpose: the kubelet cannot resolve a user name against an image
# it does not read, so `runAsNonRoot: true` refuses a named USER.
USER 65532:65532

# The base has no shell and no HTTP client, so the probe is the binary's own
# `healthcheck` job: it reads the configuration the server reads, asks
# GET {base}/health/readiness over loopback and exits 0 on 200, 1 otherwise.
# Kubernetes ignores the instruction and keeps its own probes.
HEALTHCHECK --interval=15s --timeout=5s --start-period=30s --retries=3 \
    CMD ["/usr/local/bin/ferropix", "healthcheck"]

# Exec form: the binary is PID 1 and receives SIGTERM itself, which it drains
# on. CMD is the default job, so a Kubernetes Job sets `args:` alone (`db
# migrate`) and never overrides `command:`.
ENTRYPOINT ["/usr/local/bin/ferropix"]
CMD ["serve"]

# Build-independent labels only. The release lane sets the version, revision
# and created labels at build time and mirrors every label as an index
# annotation, which is what GHCR reads for the package page.
LABEL org.opencontainers.image.title="FerroPIX" \
      org.opencontainers.image.description="A Master Patient Index over IHE PIXm and PDQm, with the cross-reference in PostgreSQL" \
      org.opencontainers.image.url="https://ferropix.eu" \
      org.opencontainers.image.documentation="https://ferropix.eu/docs/operate/deployment.html" \
      org.opencontainers.image.source="https://github.com/FerroHEALTH/FerroPIX" \
      org.opencontainers.image.vendor="Cadasto B.V." \
      org.opencontainers.image.authors="Cadasto B.V. <info@cadasto.com>" \
      org.opencontainers.image.licenses="BUSL-1.1" \
      org.opencontainers.image.base.name="gcr.io/distroless/static-debian13:nonroot"
