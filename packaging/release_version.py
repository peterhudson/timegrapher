"""The version a release build carries, so a draft can be made at any time.

The root Cargo.toml holds the base version X.Y.Z. A release takes one more
than the highest patch number any remaining release or tag uses for that
X.Y, never less than the base: with 0.4.0 and 0.4.1 already made from a
base of 0.4.0, the next draft is 0.4.2. Raise X or Y in Cargo.toml for a
new feature release; nobody raises Z by hand.

  release_version.py next TAGS_FILE   print the version for the next release,
                                      given the release and tag names in use,
                                      one per line
  release_version.py set VERSION      write VERSION into Cargo.toml and the
                                      workspace crates' entries in Cargo.lock,
                                      in the build's own checkout only
  release_version.py changes          turn `git log --merges --format=
                                      '%s%x1f%b%x1e'` on stdin into a
                                      Markdown list of pull requests

Standard library only; no network.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
VERSION = re.compile(r"(\d+)\.(\d+)\.(\d+)")


def parse(text):
    match = VERSION.fullmatch(text)
    if not match:
        raise SystemExit(f"not a version X.Y.Z: {text!r}")
    return tuple(int(part) for part in match.groups())


def base_version(cargo_toml):
    """The [workspace.package] version, as packaging/version.sh reads it."""
    in_section = False
    for line in cargo_toml.splitlines():
        if line.startswith("["):
            in_section = line.strip() == "[workspace.package]"
        elif in_section:
            match = re.fullmatch(r'version = "(.*)"', line.strip())
            if match:
                return match.group(1)
    raise SystemExit("no version in [workspace.package] of Cargo.toml")


def next_version(base, used_names):
    """One above the highest patch used for base's X.Y, and never below base."""
    major, minor, patch = parse(base)
    taken = set()
    for name in used_names:
        match = re.fullmatch(r"v?(\d+)\.(\d+)\.(\d+)", name.strip())
        if match and (int(match.group(1)), int(match.group(2))) == (major, minor):
            taken.add(int(match.group(3)))
    # Above every patch still used, so a number below a remaining release
    # or tag never comes back; deleting the newest draft frees its number
    # unless its tag remains.
    if taken:
        patch = max(patch, max(taken) + 1)
    return f"{major}.{minor}.{patch}"


def set_cargo_toml(text, version):
    out, in_section, done = [], False, False
    for line in text.splitlines(keepends=True):
        if line.startswith("["):
            in_section = line.strip() == "[workspace.package]"
        elif in_section and not done and re.match(r'version = ".*"', line):
            line = f'version = "{version}"\n'
            done = True
        out.append(line)
    if not done:
        raise SystemExit("no version in [workspace.package] of Cargo.toml")
    return "".join(out)


def set_cargo_lock(text, old, version):
    """Workspace crates are the lock entries with no source line."""
    blocks = text.split("\n[[package]]\n")
    changed = 0
    for i, block in enumerate(blocks[1:], start=1):
        if "\nsource = " not in block and f'\nversion = "{old}"' in "\n" + block:
            blocks[i] = block.replace(f'version = "{old}"', f'version = "{version}"', 1)
            changed += 1
    if not changed:
        raise SystemExit(f"no workspace crate at {old} in Cargo.lock")
    return "\n[[package]]\n".join(blocks)


def changes(log):
    """One line per merged pull request: its title and number."""
    lines = []
    for record in log.split("\x1e"):
        if not record.strip():
            continue
        subject, _, body = record.strip("\n").partition("\x1f")
        title = body.strip().splitlines()[0] if body.strip() else ""
        match = re.match(r"Merge pull request #(\d+) ", subject)
        if match and title:
            lines.append(f"- {title} (#{match.group(1)})")
        else:
            lines.append(f"- {subject}")
    return "\n".join(lines) + "\n" if lines else ""


def read(path):
    """Text with LF endings, and whether the file used CRLF (Windows checkouts)."""
    with open(path, newline="") as f:
        text = f.read()
    return text.replace("\r\n", "\n"), "\r\n" in text


def write(path, text, crlf):
    with open(path, "w", newline="") as f:
        f.write(text.replace("\n", "\r\n") if crlf else text)


def main(argv):
    if len(argv) == 3 and argv[1] == "next":
        base = base_version(read(ROOT / "Cargo.toml")[0])
        names = read(argv[2])[0].splitlines()
        print(next_version(base, names))
    elif len(argv) == 3 and argv[1] == "set":
        version = argv[2]
        parse(version)
        toml_path, lock_path = ROOT / "Cargo.toml", ROOT / "Cargo.lock"
        toml, toml_crlf = read(toml_path)
        lock, lock_crlf = read(lock_path)
        old = base_version(toml)
        if old != version:
            write(lock_path, set_cargo_lock(lock, old, version), lock_crlf)
            write(toml_path, set_cargo_toml(toml, version), toml_crlf)
        print(version)
    elif len(argv) == 2 and argv[1] == "changes":
        sys.stdout.write(changes(sys.stdin.read()))
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main(sys.argv)
