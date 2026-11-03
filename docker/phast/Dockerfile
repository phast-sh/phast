# syntax=docker/dockerfile:1
#
# Carrier image: nothing but the static phast binary on `scratch`.
#
# This is the image you `COPY --from` into your own build. Because the Linux
# release binary is static musl, it drops into any base image, glibc or musl:
#
#   COPY --from=ghcr.io/phast-sh/phast-bin:latest /phast /usr/local/bin/phast
#
# Build (buildx sets TARGETARCH per --platform):
#   docker buildx build -f docker/phast/Dockerfile \
#     --build-arg PHAST_VERSION=v0.2.0 \
#     --platform linux/amd64,linux/arm64 -t phast-bin .

FROM alpine:3.24 AS fetch
# PHAST_VERSION is the exact release tag (e.g. v0.2.0). TARGETARCH is provided
# automatically by buildx (amd64 | arm64).
ARG PHAST_VERSION
ARG TARGETARCH
ARG PHAST_REPO=phast-sh/phast

WORKDIR /out

# Tooling to download and verify the release archive.
RUN apk add --no-cache curl tar coreutils

# Download the release archive and its checksum for this build's architecture.
RUN <<EOF

set -eu

case "$TARGETARCH" in
  amd64) triple="x86_64-unknown-linux-musl" ;;
  arm64) triple="aarch64-unknown-linux-musl" ;;
  *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;;
esac

base="https://github.com/${PHAST_REPO}/releases/download/${PHAST_VERSION}"
asset="phast-${triple}.tar.gz"

curl -fsSL "$base/$asset"        -o phast.tar.gz
curl -fsSL "$base/$asset.sha256" -o phast.tar.gz.sha256

EOF

# Verify the checksum before trusting the archive.
RUN printf '%s  phast.tar.gz\n' "$(awk '{print $1}' phast.tar.gz.sha256)" | sha256sum -c -

# Unpack just the binary and make it executable.
RUN tar -xzf phast.tar.gz phast && chmod +x phast

FROM scratch

LABEL org.opencontainers.image.source="https://github.com/phast-sh/phast"
LABEL org.opencontainers.image.description="Phast, a dependency manager for PHP, as a static binary on scratch to COPY --from"
LABEL org.opencontainers.image.licenses="MIT OR Apache-2.0"

COPY --from=fetch /out/phast /phast

WORKDIR /io
ENTRYPOINT ["/phast"]
