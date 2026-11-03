# syntax=docker/dockerfile:1
#
# PHP-based image: the phast binary on top of an official php-cli base, so
# `phast install`, scripts, and platform detection all work out of the box.
#
# Build args select the PHP line and distro variant:
#   PHP_VERSION=8.4  PHP_VARIANT=-cli          -> phast:8.4         (Debian)
#   PHP_VERSION=8.4  PHP_VARIANT=-cli-alpine   -> phast:8.4-alpine
#
# Example:
#   docker buildx build -f docker/php/Dockerfile \
#     --build-arg PHAST_VERSION=v0.2.0 \
#     --build-arg PHP_VERSION=8.4 --build-arg PHP_VARIANT=-cli \
#     --platform linux/amd64,linux/arm64 -t phast:8.4 .

ARG PHP_VERSION=8.4
ARG PHP_VARIANT=-cli

# --- fetch: download and verify the phast release binary -------------------
# Alpine is used regardless of the target base, purely to get curl/tar/sha256.
FROM alpine:3.24 AS fetch
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

# --- runtime: phast on the official php-cli base ---------------------------
FROM php:${PHP_VERSION}${PHP_VARIANT}

LABEL org.opencontainers.image.source="https://github.com/phast-sh/phast"
LABEL org.opencontainers.image.description="Phast, a dependency manager for PHP, on the official php-cli image"
LABEL org.opencontainers.image.licenses="MIT OR Apache-2.0"

# Tools phast shells out to at runtime: git for VCS repositories, ca-certificates
# for HTTPS registry access. Zip extraction is native, so no unzip is needed.
RUN <<EOF

set -eu

if command -v apk >/dev/null 2>&1; then
  apk add --no-cache git ca-certificates
else
  apt-get update
  apt-get install -y --no-install-recommends git ca-certificates
  rm -rf /var/lib/apt/lists/*
fi

EOF

COPY --from=fetch /out/phast /usr/local/bin/phast

WORKDIR /app
ENTRYPOINT ["phast"]
CMD ["--help"]
