#!/usr/bin/env bash
set -euo pipefail

REPO="${AMP808_REPO:-opsydyn/AMP808}"
INSTALL_DIR="${AMP808_INSTALL_DIR:-$HOME/.local/bin}"
RUN_AFTER_INSTALL="${AMP808_RUN_AFTER_INSTALL:-0}"
API_URL="https://api.github.com/repos/${REPO}/releases/latest"
TMP_DIR="$(mktemp -d)"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'error: required command not found: %s\n' "$1" >&2
    exit 1
  fi
}

require_cmd curl
require_cmd tar
require_cmd grep
require_cmd sed
require_cmd find
require_cmd mktemp
require_cmd chmod
require_cmd cp

if [[ "$(uname -s)" != "Darwin" ]]; then
  printf 'error: this installer is for macOS only\n' >&2
  exit 1
fi

release_json="$(curl -fsSL --retry 3 --retry-delay 1 "$API_URL")"
tag_name="$(
  printf '%s' "$release_json" \
    | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' \
    | head -n 1
)"
asset_url="$(
  printf '%s' "$release_json" \
    | grep -Eo 'https://[^"]+macos-universal\.tar\.gz' \
    | head -n 1 \
    || true
)"

if [[ -z "$tag_name" || -z "$asset_url" ]]; then
  printf 'error: could not find the latest macOS release asset for %s\n' "$REPO" >&2
  exit 1
fi

archive_path="$TMP_DIR/amp808-macos-universal.tar.gz"
extract_dir="$TMP_DIR/extract"
mkdir -p "$extract_dir" "$INSTALL_DIR"

printf 'Downloading amp808 %s...\n' "$tag_name"
curl -fL --retry 3 --retry-delay 1 "$asset_url" -o "$archive_path"

printf 'Extracting release archive...\n'
LC_ALL=C tar -xzf "$archive_path" -C "$extract_dir"

binary_path="$(find "$extract_dir" -type f -name amp808 -print -quit)"
if [[ -z "$binary_path" ]]; then
  printf 'error: amp808 binary was not found inside the release archive\n' >&2
  exit 1
fi

install_path="$INSTALL_DIR/amp808"
cp "$binary_path" "$install_path"
chmod +x "$install_path"

if command -v xattr >/dev/null 2>&1; then
  xattr -d com.apple.quarantine "$install_path" >/dev/null 2>&1 || true
fi

printf 'Installed amp808 to %s\n' "$install_path"

case ":$PATH:" in
  *":$INSTALL_DIR:"*)
    printf 'Run: amp808\n'
    ;;
  *)
    printf 'Run: %s\n' "$install_path"
    printf 'Optional: add %s to your PATH for easier launches.\n' "$INSTALL_DIR"
    ;;
esac

if [[ "$RUN_AFTER_INSTALL" == "1" || "$#" -gt 0 ]]; then
  printf 'Launching amp808...\n'
  exec "$install_path" "$@"
fi
