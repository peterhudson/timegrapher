#!/usr/bin/env bash
# The Windows download: one zip holding a folder with the desktop app, the
# command line tools, the Claude Desktop extension (.mcpb, where the
# workspace has the MCP server) and the documentation.
#
#   packaging/windows.sh DIST [DOCS]
#
# Runs in Git Bash on Windows; expects target/release/*.exe built and 7-Zip
# (7z) on PATH, as on GitHub's Windows runners. DOCS is the folder
# packaging/docs_html.py rendered for windows; without it the pages are
# rendered here, which needs pandoc.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
dist="$(mkdir -p "$1" && cd "$1" && pwd)"
version="$("$here/version.sh")"
bin="$root/target/release"
cli=("$bin/timegrapher.exe")
[ -f "$bin/timegrapher-mcp.exe" ] && cli+=("$bin/timegrapher-mcp.exe")
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
docs="${2:-}"
if [ -z "$docs" ]; then
  docs="$work/docs"
  py=python3; command -v python3 >/dev/null || py=python
  "$py" "$here/docs_html.py" render "$docs" windows
fi

folder="$work/Timegrapher $version"
mkdir -p "$folder/Command Line"
cp -R "$docs/." "$folder/"
cp "$bin/timegrapher-app.exe" "$folder/Timegrapher.exe"
cp "${cli[@]}" "$folder/Command Line/"

# The Claude Desktop extension (see macos.sh)
mcpb="$root/crates/timegrapher-mcp/packaging/mcpb.sh"
if [ -f "$bin/timegrapher-mcp.exe" ] && [ -f "$mcpb" ]; then
  mkdir "$folder/Claude Desktop"
  bash "$mcpb" "$folder/Command Line" "$folder/Claude Desktop/Timegrapher.mcpb"
fi

rm -f "$dist/timegrapher-$version-windows-x64.zip"
(cd "$work" && 7z a -tzip -bso0 "$dist/timegrapher-$version-windows-x64.zip" "Timegrapher $version")
