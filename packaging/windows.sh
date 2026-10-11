#!/usr/bin/env bash
# Windows release files: a .zip with the programs (unzip, double-click
# timegrapher-app.exe) and, where the workspace has the MCP server, a Claude
# Desktop bundle (.mcpb).
#
#   packaging/windows.sh DIST
#
# Runs in Git Bash on Windows; expects target/release/*.exe built and 7-Zip
# (7z) on PATH, as on GitHub's Windows runners.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
dist="$(mkdir -p "$1" && cd "$1" && pwd)"
version="$("$here/version.sh")"
name="timegrapher-$version-windows-x86_64"
bin="$root/target/release"
progs=("$bin/timegrapher.exe" "$bin/timegrapher-app.exe")
[ -f "$bin/timegrapher-mcp.exe" ] && progs+=("$bin/timegrapher-mcp.exe")
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

mkdir "$work/$name"
cp "${progs[@]}" "$root/README.md" "$root/LICENSE" "$work/$name/"
cp "$here/windows/FIRST-LAUNCH.txt" "$work/$name/"
(cd "$work" && 7z a -tzip -bso0 "$dist/$name.zip" "$name")

# The Claude Desktop bundle (see macos.sh)
mcpb="$root/crates/timegrapher-mcp/packaging/mcpb.sh"
if [ -f "$bin/timegrapher-mcp.exe" ] && [ -f "$mcpb" ]; then
  mkdir "$work/mcp"
  cp "$bin/timegrapher-mcp.exe" "$bin/timegrapher.exe" "$work/mcp/"
  bash "$mcpb" "$work/mcp" "$dist/timegrapher-$version-windows-x64.mcpb"
fi
