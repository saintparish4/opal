#!/usr/bin/env bash
# Installs opal into ~/.opal/bin and adds it to PATH.
#
#   curl -fsSL https://opal.dev/install.sh | bash
#
# Pin a version instead of the latest release:
#   OPAL_VERSION=v0.1.0 curl -fsSL https://opal.dev/install.sh | bash
#
# v1 targets macOS, Linux, and WSL2 only (WSL2 reports as Linux via uname,
# so the linux-x64/linux-arm64 assets cover it directly). Native Windows is
# a v2 target and is not built here.

set -euo pipefail

REPO="saintparish4/opal"
BIN_NAME="opal"
INSTALL_DIR="${OPAL_INSTALL_DIR:-$HOME/.opal/bin}"
VERSION="${OPAL_VERSION:-latest}"
# Set by main(), read by the EXIT trap; declared here (rather than left to
# spring into existence on first assignment) so `rm -rf "$tmp"` is always
# defined under `set -u`, even if the script dies before main() gets to it.
tmp=""

log() { printf 'opal: %s\n' "$1"; }
die() {
  printf 'opal: error: %s\n' "$1" >&2
  exit 1
}

detect_asset() {
  local os arch
  case "$(uname -s)" in
    Linux) os=linux ;;
    Darwin) os=macos ;;
    *) die "unsupported OS: $(uname -s). opal v1 supports macOS, Linux, and WSL2 only — see https://github.com/${REPO}#deployment" ;;
  esac
  case "$(uname -m)" in
    x86_64 | amd64) arch=x64 ;;
    aarch64 | arm64) arch=arm64 ;;
    *) die "unsupported architecture: $(uname -m)" ;;
  esac
  printf '%s-%s' "$os" "$arch"
}

release_url() {
  local file="$1"
  if [ "$VERSION" = "latest" ]; then
    printf 'https://github.com/%s/releases/latest/download/%s' "$REPO" "$file"
  else
    printf 'https://github.com/%s/releases/download/%s/%s' "$REPO" "$VERSION" "$file"
  fi
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    die "neither sha256sum nor shasum is available — cannot verify the download"
  fi
}

add_to_path() {
  local rc marker line
  marker="# added by opal's install.sh"
  line="export PATH=\"$INSTALL_DIR:\$PATH\""

  case "${SHELL:-}" in
    */zsh) rc="$HOME/.zshrc" ;;
    */bash) rc="$HOME/.bashrc" ;;
    *) rc="$HOME/.profile" ;;
  esac

  if [ -f "$rc" ] && grep -qF "$marker" "$rc" 2>/dev/null; then
    return 0
  fi
  {
    printf '\n%s\n%s\n' "$marker" "$line"
  } >>"$rc"
  log "added $INSTALL_DIR to PATH in $rc"
}

main() {
  local asset archive expected actual

  asset="$(detect_asset)"
  archive="${BIN_NAME}-${asset}.tar.gz"

  # Script-scoped, not local: the EXIT trap fires after main() returns, once
  # bash has already discarded any local variables, so a local tmp here would
  # be unbound by the time the trap tries to read it.
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  log "downloading ${archive} (${VERSION})"
  curl -fsSL "$(release_url "$archive")" -o "$tmp/$archive" ||
    die "download failed — is ${VERSION} a real release? https://github.com/${REPO}/releases"
  curl -fsSL "$(release_url "SHA256SUMS")" -o "$tmp/SHA256SUMS" ||
    die "failed to download SHA256SUMS"

  expected="$(grep -F "  ${archive}" "$tmp/SHA256SUMS" | awk '{print $1}')"
  [ -n "$expected" ] || die "${archive} is not listed in SHA256SUMS"
  actual="$(sha256_of "$tmp/$archive")"
  [ "$expected" = "$actual" ] ||
    die "checksum mismatch for ${archive}: expected ${expected}, got ${actual}"
  log "checksum verified"

  mkdir -p "$INSTALL_DIR"
  tar -xzf "$tmp/$archive" -C "$tmp"
  install -m 755 "$tmp/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"

  add_to_path

  log "installed to $INSTALL_DIR/$BIN_NAME"
  log "$("$INSTALL_DIR/$BIN_NAME" --version 2>/dev/null || echo "$BIN_NAME $VERSION")"
  log "open a new shell, or run: export PATH=\"$INSTALL_DIR:\$PATH\""
}

main "$@"
