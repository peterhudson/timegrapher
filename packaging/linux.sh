#!/usr/bin/env bash
# The Linux download: one zip holding a folder with the desktop app as an
# AppImage (one file: make executable, double-click), the command line
# tools and the documentation.
#
#   packaging/linux.sh DIST [DOCS]
#
# Expects target/release/timegrapher and timegrapher-app built (and
# timegrapher-mcp, the MCP server for AI chat apps, where the workspace has
# it), appimagetool on PATH (or APPIMAGETOOL naming it; APPIMAGE_RUNTIME
# names the runtime file it uses, else it fetches one) and zip. DOCS is
# the folder packaging/docs_html.py rendered for linux; without it the
# pages are rendered here, which needs pandoc. Writes into DIST.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
dist="$(mkdir -p "$1" && cd "$1" && pwd)"
version="$("$here/version.sh")"
arch="$(uname -m)"
bin="$root/target/release"
# The MCP server finds the command line tool beside itself, so they travel
# together.
cli=("$bin/timegrapher")
[ -f "$bin/timegrapher-mcp" ] && cli+=("$bin/timegrapher-mcp")
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
docs="${2:-}"
if [ -z "$docs" ]; then
  docs="$work/docs"
  python3 "$here/docs_html.py" render "$docs" linux
fi

folder="$work/Timegrapher $version"
mkdir -p "$folder/Command Line"
cp -R "$docs/." "$folder/"
cp "${cli[@]}" "$folder/Command Line/"

# AppImage. The app finds ALSA, X11 or Wayland and OpenGL on the system at
# run time, as every Linux desktop has them, so nothing else is bundled.
app="$work/Timegrapher.AppDir"
mkdir -p "$app/usr/bin"
cp "$bin/timegrapher-app" "${cli[@]}" "$app/usr/bin/"
cp "$here/linux/timegrapher.desktop" "$app/"
cp "$here/icon.png" "$app/timegrapher.png"
cp "$here/linux/AppRun" "$app/AppRun"
chmod +x "$app/AppRun"
ARCH="$arch" "${APPIMAGETOOL:-appimagetool}" --no-appstream \
  ${APPIMAGE_RUNTIME:+--runtime-file "$APPIMAGE_RUNTIME"} "$app" "$folder/Timegrapher.AppImage"
chmod +x "$folder/Timegrapher.AppImage" "$folder/Command Line/"*

# Info-ZIP keeps the programs' executable bits.
rm -f "$dist/timegrapher-$version-linux-$arch.zip"
(cd "$work" && zip -qr -X "$dist/timegrapher-$version-linux-$arch.zip" "Timegrapher $version")
