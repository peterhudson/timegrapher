"""The corpus audio on a self-hosted runner (agents#48).

GitHub's runners restore the audio (about 7 GB) from the Actions cache. On
the workstation's runners that would download it again for every regress
job, so each runner keeps its own copy in a store outside the workspace
and hard-links it into the fresh corpus checkout instead: no copying.

    restore STORE TAKES  link each stored .flac into TAKES/<take>/ where
                         the checkout has that take and lacks the file
    keep STORE TAKES     make the store exactly the .flac files TAKES holds
                         (after the run fetched any it lacked and dropped
                         any the corpus no longer lists)

The regress step still checks every file's SHA-256 (`--fetch`), so a bad
stored file is fetched again, not read. Offline tests: test_self_hosted_audio.py.
"""

import os
import pathlib
import shutil
import sys


def flacs(root):
    """Paths of the .flac files one take folder down, relative to root."""
    return sorted(p.relative_to(root) for p in root.glob("*/*.flac") if p.is_file())


def restore(store, takes):
    linked = 0
    if not store.is_dir():
        return linked
    for rel in flacs(store):
        dest = takes / rel
        if dest.parent.is_dir() and not dest.exists():
            os.link(store / rel, dest)
            linked += 1
    return linked


def keep(store, takes):
    fresh = store.with_name(store.name + ".new")
    shutil.rmtree(fresh, ignore_errors=True)
    for rel in flacs(takes):
        (fresh / rel.parent).mkdir(parents=True, exist_ok=True)
        os.link(takes / rel, fresh / rel)
    fresh.mkdir(parents=True, exist_ok=True)
    old = store.with_name(store.name + ".old")
    shutil.rmtree(old, ignore_errors=True)
    if store.exists():
        store.rename(old)
    fresh.rename(store)
    shutil.rmtree(old, ignore_errors=True)
    return len(flacs(store))


def main(argv):
    if len(argv) != 4 or argv[1] not in ("restore", "keep"):
        raise SystemExit(__doc__)
    store, takes = pathlib.Path(argv[2]), pathlib.Path(argv[3])
    if argv[1] == "restore":
        print(f"linked {restore(store, takes)} stored audio files into {takes}")
    else:
        print(f"kept {keep(store, takes)} audio files in {store}")


if __name__ == "__main__":
    main(sys.argv)
