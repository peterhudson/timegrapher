#!/usr/bin/env bash
# The macOS download: one disk image holding Timegrapher.app (one binary
# for Apple silicon and Intel) beside a link to Applications to drag it
# onto, the command line tools, the Claude Desktop extension (.mcpb, where
# the workspace has the MCP server) and the documentation.
#
#   packaging/macos.sh DIST [DOCS]
#
# Expects both programs built for aarch64-apple-darwin and
# x86_64-apple-darwin under target/<triple>/release. Runs on macOS only
# (lipo, sips, iconutil, codesign, hdiutil). DOCS is the folder
# packaging/docs_html.py rendered for macos; without it the pages are
# rendered here, which needs pandoc.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
dist="$(mkdir -p "$1" && cd "$1" && pwd)"
version="$("$here/version.sh")"
arm="$root/target/aarch64-apple-darwin/release"
intel="$root/target/x86_64-apple-darwin/release"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
docs="${2:-}"
if [ -z "$docs" ]; then
  docs="$work/docs"
  python3 "$here/docs_html.py" render "$docs" macos
fi

progs=(timegrapher timegrapher-app)
[ -f "$arm/timegrapher-mcp" ] && progs+=(timegrapher-mcp)
mkdir -p "$work/bin"
for b in "${progs[@]}"; do
  lipo -create -output "$work/bin/$b" "$arm/$b" "$intel/$b"
done

# The bundle
app="$work/Timegrapher.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
for b in "${progs[@]}"; do cp "$work/bin/$b" "$app/Contents/MacOS/"; done
sed "s/@VERSION@/$version/g" "$here/macos/Info.plist" > "$app/Contents/Info.plist"
iconset="$work/timegrapher.iconset"
mkdir "$iconset"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" "$here/icon.png" --out "$iconset/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  if [ "$d" -le 512 ]; then
    sips -z "$d" "$d" "$here/icon.png" --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
  fi
done
iconutil -c icns -o "$app/Contents/Resources/timegrapher.icns" "$iconset"
# Ad-hoc signature: free, needs no Apple account, and lets macOS remember
# the microphone permission. Not notarised, so the first launch of each
# version needs Open Anyway (docs/install.md).
codesign --force --deep --sign - "$app"
codesign --verify --verbose "$app"

folder="$work/Timegrapher $version"
mkdir -p "$folder/Command Line"
cp -R "$docs/." "$folder/"
cp -R "$app" "$folder/"
# The command line tool, with the MCP server beside it
for b in "${progs[@]}"; do [ "$b" = timegrapher-app ] || cp "$work/bin/$b" "$folder/Command Line/"; done

# The Claude Desktop extension, packed by the MCP crate's own script:
# mcpb.sh BINDIR OUTFILE, BINDIR holding timegrapher-mcp and timegrapher.
mcpb="$root/crates/timegrapher-mcp/packaging/mcpb.sh"
if [ -f "$work/bin/timegrapher-mcp" ] && [ -f "$mcpb" ]; then
  mkdir "$folder/Claude Desktop"
  bash "$mcpb" "$folder/Command Line" "$folder/Claude Desktop/Timegrapher.mcpb"
fi

# The disk image: the app beside a link to Applications, to drag across.
ln -s /Applications "$folder/Applications"
hdiutil create -volname "Timegrapher $version" -srcfolder "$folder" -ov -format UDZO \
  "$dist/timegrapher-$version-macos.dmg"
