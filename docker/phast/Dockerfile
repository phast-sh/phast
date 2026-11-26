# syntax=docker/dockerfile:1
#
# An image that contains only Phast binary, to copy into other images:
#
#   COPY --from=ghcr.io/phast-sh/phast:latest /phast /bin/
#
# The Linux binary is static musl, so it runs on any base image, glibc or musl. The
# release workflow builds this from the release's own binaries, put in <arch>/phast
# first, so the image holds the same file as the release download.

FROM scratch
ARG TARGETARCH
COPY ${TARGETARCH}/phast /phast

# Lets `docker run ghcr.io/phast-sh/phast --version` work. To run Phast for real,
# copy it into an image with PHP, as above.
ENTRYPOINT ["/phast"]
