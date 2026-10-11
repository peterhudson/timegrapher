#!/usr/bin/env bash
# macOS release files: Timegrapher.app (one binary for Apple silicon and
# Intel) in a .dmg, a tarball of the command line tool and, where the
# workspace has the MCP server, a Claude Desktop bundle (.mcpb).
#
#   packaging/macos.sh DIST
#
# Expects both programs built for aarch64-apple-darwin and
# x86_64-apple-darwin under target/<triple>/release. Runs on macOS only
# (lipo, sips, iconutil, codesign, hdiutil).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
dist="$(mkdir -p "$1" && cd "$1" && pwd)"
version="$("$here/version.sh")"
arm="$root/target/aarch64-apple-darwin/release"
intel="$root/target/x86_64-apple-darwin/release"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

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

# The disk image: the app beside a link to Applications, to drag across.
img="$work/img"
mkdir "$img"
cp -R "$app" "$img/"
ln -s /Applications "$img/Applications"
hdiutil create -volname "Timegrapher $version" -srcfolder "$img" -ov -format UDZO \
  "$dist/timegrapher-$version-macos.dmg"

# The command line tool alone (with the MCP server beside it)
name="timegrapher-$version-macos-cli"
mkdir "$work/$name"
cp "$root/README.md" "$root/LICENSE" "$work/$name/"
for b in "${progs[@]}"; do [ "$b" = timegrapher-app ] || cp "$work/bin/$b" "$work/$name/"; done
tar -C "$work" -czf "$dist/$name.tar.gz" "$name"

# The Claude Desktop bundle, packed by the MCP crate's own script once it
# has one: mcpb.sh BINDIR OUTFILE, BINDIR holding timegrapher-mcp and
# timegrapher.
mcpb="$root/crates/timegrapher-mcp/packaging/mcpb.sh"
if [ -f "$work/bin/timegrapher-mcp" ] && [ -f "$mcpb" ]; then
  bash "$mcpb" "$work/bin" "$dist/timegrapher-$version-macos.mcpb"
fi
