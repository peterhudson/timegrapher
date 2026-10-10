#!/usr/bin/env bash
# Prints the workspace version from the root Cargo.toml (the one place it
# lives). With a tag as the first argument, fails unless the tag is
# v<version>, so a release can never carry a different number from the
# programs inside it.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$root/Cargo.toml")"
if [ -z "$version" ]; then
  echo "no version in [workspace.package] of Cargo.toml" >&2
  exit 1
fi
if [ $# -gt 0 ] && [ "$1" != "v$version" ]; then
  echo "tag $1 does not match the Cargo.toml version $version (expected v$version)" >&2
  exit 1
fi
echo "$version"
