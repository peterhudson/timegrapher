#!/usr/bin/env bash
# A Claude Desktop extension (.mcpb) of the MCP server for one platform: a
# zip holding manifest.json, the icon, and the server with the timegrapher
# program beside it (docs/mcp.md).
#
#   crates/timegrapher-mcp/packaging/mcpb.sh BINDIR OUTFILE
#
# BINDIR holds timegrapher-mcp and timegrapher (with .exe on Windows), as
# packaging/macos.sh and packaging/windows.sh lay them out. Needs zip, 7z or
# python3 to write the zip.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
bin="$1"
out="$2"
version="$("$root/packaging/version.sh")"

if [ -f "$bin/timegrapher-mcp.exe" ]; then
  platform=win32 ext=.exe
elif [ -f "$bin/timegrapher-mcp" ]; then
  case "$(uname -s)" in
    Darwin) platform=darwin ;;
    *) platform=linux ;;
  esac
  ext=
else
  echo "no timegrapher-mcp in $bin" >&2
  exit 1
fi
if [ ! -f "$bin/timegrapher$ext" ]; then
  echo "no timegrapher$ext in $bin: the server runs it" >&2
  exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/server"
cp "$bin/timegrapher-mcp$ext" "$bin/timegrapher$ext" "$work/server/"
chmod +x "$work/server/"*
cp "$root/packaging/icon.png" "$work/icon.png"
cp "$root/LICENSE" "$work/LICENSE"
sed -e "s/@VERSION@/$version/g" \
    -e "s/@SERVER@/timegrapher-mcp$ext/g" \
    -e "s/@CLI@/timegrapher$ext/g" \
    -e "s/@PLATFORM@/$platform/g" \
    "$here/manifest.json" > "$work/manifest.json"

mkdir -p "$(dirname "$out")"
out="$(cd "$(dirname "$out")" && pwd)/$(basename "$out")"
rm -f "$out"
if command -v zip >/dev/null; then
  (cd "$work" && zip -qr -X "$out" manifest.json icon.png LICENSE server)
elif command -v 7z >/dev/null; then
  (cd "$work" && 7z a -tzip -bso0 "$out" manifest.json icon.png LICENSE server)
else
  python3 - "$work" "$out" <<'PY'
import os, sys, zipfile
work, out = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for top in ["manifest.json", "icon.png", "LICENSE", "server"]:
        p = os.path.join(work, top)
        paths = [p] if os.path.isfile(p) else [os.path.join(d, f) for d, _, fs in os.walk(p) for f in fs]
        for f in paths:
            info = zipfile.ZipInfo.from_file(f, os.path.relpath(f, work))
            with open(f, "rb") as h:
                z.writestr(info, h.read(), zipfile.ZIP_DEFLATED)
PY
fi
echo "$out"
