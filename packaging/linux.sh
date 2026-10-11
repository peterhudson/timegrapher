#!/usr/bin/env bash
# Linux release files: a tarball with both programs, and an AppImage of the
# desktop app (one file: download, make executable, double-click).
#
#   packaging/linux.sh DIST
#
# Expects target/release/timegrapher and timegrapher-app built (and
# timegrapher-mcp, the MCP server for AI chat apps, where the workspace has
# it), and appimagetool on PATH (or APPIMAGETOOL naming it). Writes into
# DIST.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
dist="$(mkdir -p "$1" && cd "$1" && pwd)"
version="$("$here/version.sh")"
arch="$(uname -m)"
name="timegrapher-$version-linux-$arch"
bin="$root/target/release"
# The MCP server finds the command line tool beside itself, so they travel
# together.
cli=("$bin/timegrapher")
[ -f "$bin/timegrapher-mcp" ] && cli+=("$bin/timegrapher-mcp")
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Tarball
mkdir "$work/$name"
cp "${cli[@]}" "$bin/timegrapher-app" "$root/README.md" "$root/LICENSE" "$work/$name/"
cp "$here/linux/timegrapher.desktop" "$here/icon.png" "$work/$name/"
tar -C "$work" -czf "$dist/$name.tar.gz" "$name"

# AppImage. The app finds ALSA, X11 or Wayland and OpenGL on the system at
# run time, as every Linux desktop has them, so nothing else is bundled.
app="$work/Timegrapher.AppDir"
mkdir -p "$app/usr/bin"
cp "$bin/timegrapher-app" "${cli[@]}" "$app/usr/bin/"
cp "$here/linux/timegrapher.desktop" "$app/"
cp "$here/icon.png" "$app/timegrapher.png"
cp "$here/linux/AppRun" "$app/AppRun"
chmod +x "$app/AppRun"
ARCH="$arch" "${APPIMAGETOOL:-appimagetool}" --no-appstream "$app" "$dist/$name.AppImage"
chmod +x "$dist/$name.AppImage"
