#!/bin/sh
set -eu

REPOSITORY="COPPSARY/subshell"

if [ "$(uname -s)" != "Linux" ]; then
  echo "SubShell's Linux installer only runs on Linux." >&2
  exit 1
fi

case "$(uname -m)" in
  x86_64|amd64) ;;
  *)
    echo "SubShell releases currently support Linux x86_64 only; detected $(uname -m)." >&2
    exit 1
    ;;
esac

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required to install SubShell." >&2
  exit 1
fi

release_json=$(curl -fsSL \
  -H "Accept: application/vnd.github+json" \
  -H "X-GitHub-Api-Version: 2022-11-28" \
  "https://api.github.com/repos/$REPOSITORY/releases/latest") || {
    echo "Could not read the latest SubShell release from GitHub." >&2
    exit 1
  }
asset_url=$(printf '%s\n' "$release_json" | sed -n 's/.*"browser_download_url": "\([^"]*\.AppImage\)".*/\1/p' | head -n 1)

case "$asset_url" in
  "https://github.com/$REPOSITORY/releases/download/"*.AppImage) ;;
  *)
    echo "The latest SubShell release does not contain a Linux x86_64 AppImage." >&2
    exit 1
    ;;
esac

install_dir=${SUBSHELL_INSTALL_DIR:-${XDG_BIN_HOME:-"$HOME/.local/bin"}}
temporary_file=$(mktemp "${TMPDIR:-/tmp}/subshell.XXXXXX.AppImage")
trap 'rm -f "$temporary_file"' EXIT HUP INT TERM

echo "Downloading the latest SubShell AppImage..."
curl -fL --retry 3 --output "$temporary_file" "$asset_url"
mkdir -p "$install_dir"
install -m 0755 "$temporary_file" "$install_dir/subshell"

echo "Installed SubShell at $install_dir/subshell"
case ":$PATH:" in
  *":$install_dir:"*) ;;
  *) echo "Add $install_dir to PATH, then run: subshell" ;;
esac
