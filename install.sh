#!/bin/sh
# install.sh - install the phast CLI from GitHub Releases.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/phast-sh/phast/main/install.sh | sh
#
# Environment overrides:
#   PHAST_REPO         GitHub <owner>/<repo>    (default: phast-sh/phast)
#   PHAST_VERSION      release tag to install   (default: latest)
#   PHAST_INSTALL_DIR  install location         (default: $HOME/.local/bin)

set -eu

# The release assets are named `phast-<target>.tar.gz` (+ `.sha256`), where
# <target> is the Rust target triple, e.g. `phast-aarch64-apple-darwin.tar.gz`.
# Each archive contains the `phast` binary at its root. The CI release workflow
# must produce assets with these names for this script to find them.
REPO="${PHAST_REPO:-phast-sh/phast}"
INSTALL_DIR="${PHAST_INSTALL_DIR:-$HOME/.local/bin}"
BIN="phast"

say()  { printf '%s\n' "$*"; }
err()  { printf 'error: %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

# A downloader: `fetch <url>` writes to stdout, `download <url> <file>` to a file.
if have curl; then
    fetch()    { curl -fsSL "$1"; }
    download() { curl -fsSL "$1" -o "$2"; }
elif have wget; then
    fetch()    { wget -qO- "$1"; }
    download() { wget -qO "$2" "$1"; }
else
    err "need curl or wget to download phast"
fi

# Map `uname` to the Rust target triple used in the asset name.
detect_target() {
    os=$(uname -s)
    arch=$(uname -m)

    case "$os" in
        Linux)  os="unknown-linux-musl" ;;
        Darwin) os="apple-darwin" ;;
        *)      err "unsupported OS: $os (linux and macOS only for now)" ;;
    esac

    case "$arch" in
        x86_64 | amd64)  arch="x86_64" ;;
        arm64 | aarch64) arch="aarch64" ;;
        *)               err "unsupported architecture: $arch" ;;
    esac

    printf '%s-%s' "$arch" "$os"
}

# The latest release tag, read from the GitHub API.
latest_tag() {
    fetch "https://api.github.com/repos/$REPO/releases/latest" \
        | grep '"tag_name"' \
        | head -1 \
        | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/'
}

# Fail unless `file` matches the sha256 recorded in `sums` (`<hash>  <name>`).
verify_checksum() {
    file=$1
    sums=$2
    expected=$(awk '{ print $1 }' "$sums")

    if have sha256sum; then
        actual=$(sha256sum "$file" | awk '{ print $1 }')
    elif have shasum; then
        actual=$(shasum -a 256 "$file" | awk '{ print $1 }')
    else
        say "warning: no sha256 tool found, skipping checksum verification"
        return 0
    fi

    [ "$expected" = "$actual" ] || err "checksum mismatch for $(basename "$file")"
}

install() {
    target=$(detect_target)
    tag="${PHAST_VERSION:-$(latest_tag)}"
    [ -n "$tag" ] || err "could not determine the latest release of $REPO"

    asset="phast-${target}.tar.gz"
    base="https://github.com/$REPO/releases/download/$tag"

    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT

    say "Downloading phast $tag ($target)..."
    download "$base/$asset"        "$tmp/$asset"        || err "failed to download $asset"
    download "$base/$asset.sha256" "$tmp/$asset.sha256" || err "failed to download checksum"
    verify_checksum "$tmp/$asset"  "$tmp/$asset.sha256"

    tar -xzf "$tmp/$asset" -C "$tmp"
    [ -f "$tmp/$BIN" ] || err "archive did not contain a '$BIN' binary"

    mkdir -p "$INSTALL_DIR"
    mv "$tmp/$BIN" "$INSTALL_DIR/$BIN"
    chmod +x "$INSTALL_DIR/$BIN"

    say "Installed phast $tag to $INSTALL_DIR/$BIN"

    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            say ""
            say "$INSTALL_DIR is not on your PATH. Add it, e.g.:"
            say "  export PATH=\"$INSTALL_DIR:\$PATH\""
            ;;
    esac
}

install
