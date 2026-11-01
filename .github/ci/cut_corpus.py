"""Write a cut of a corpus checkout: each take's first few sound files and
its last.

  cut_corpus.py SOURCE CUT [--files N]

SOURCE is a corpus checkout with its audio fetched. CUT, which must not
exist yet, gets the same layout: corpus.json, and takes/<id>/ for each take,
with only the first N sound files (3 by default) of each take, in the order
corpus.json lists them, and its last. A take's last file is often short (the
recording stopped mid-file), and past app and MCP differences were found
there (#581, #626). Everything else in a take's folder is linked, not
copied, except two files rewritten to match the kept sound files:

- session.toml keeps its header and each [[recording]] entry whose file is
  kept, or that names the take's whole folder (".");
- session.json's "segments" keeps only the kept files.

A take with an entry for its whole folder (".") is not cut: that entry reads
every file, against the breaks and duration session.json gives for the whole
take, which a cut would no longer match.

corpus.json lists only the kept files of each take, so a check that reads
the takes it lists reads the cut. Pull requests' app and MCP corpus checks
read a cut; main and the nightly run read the whole corpus (ci.yml).
"""

import argparse
import json
import os
import pathlib
import re
import sys

RECORDING = re.compile(r"^\[\[recording\]\][ \t]*$", re.M)
FILE = re.compile(r'^file[ \t]*=[ \t]*"([^"]*)"', re.M)
WHOLE_FOLDER = "."


def cut_manifest(text, kept):
    """session.toml's text with only the [[recording]] entries for kept
    files (and whole-folder entries). The header and each kept entry are
    left exactly as written."""
    starts = [m.start() for m in RECORDING.finditer(text)]
    if not starts:
        return text
    out = [text[:starts[0]]]
    for start, end in zip(starts, starts[1:] + [len(text)]):
        entry = text[start:end]
        name = FILE.search(entry)
        if name is None:
            raise ValueError(f"a [[recording]] entry names no file: {entry[:60]!r}")
        if name.group(1) == WHOLE_FOLDER or name.group(1) in kept:
            out.append(entry)
    return "".join(out)


def cut_session_json(text, kept):
    """session.json's text with "segments" holding only the kept files."""
    doc = json.loads(text)
    if isinstance(doc.get("segments"), list):
        doc["segments"] = [s for s in doc["segments"] if s in kept]
    return json.dumps(doc, indent=2) + "\n"


def reads_whole_folder(manifest):
    """Whether a session.toml has an entry for its take's whole folder."""
    return any(m.group(1) == WHOLE_FOLDER for m in FILE.finditer(manifest))


def cut(source, dest, files):
    corpus = json.loads((source / "corpus.json").read_text(encoding="utf-8"))
    dest.mkdir(parents=False)
    (dest / "takes").mkdir()
    for take in corpus["takes"]:
        listed = {f["name"] for f in take["files"]}
        src = source / "takes" / take["id"]
        if not src.is_dir():
            take["files"] = take["files"][:files] + take["files"][files:][-1:]
            continue
        manifest = src / "session.toml"
        if not (manifest.is_file() and reads_whole_folder(manifest.read_text(encoding="utf-8"))):
            take["files"] = take["files"][:files] + take["files"][files:][-1:]
        kept = {f["name"] for f in take["files"]}
        out = dest / "takes" / take["id"]
        out.mkdir()
        for entry in sorted(src.iterdir()):
            name = entry.name
            if name == "session.toml":
                text = cut_manifest(entry.read_text(encoding="utf-8"), kept)
                (out / name).write_text(text, encoding="utf-8")
            elif name == "session.json":
                text = cut_session_json(entry.read_text(encoding="utf-8"), kept)
                (out / name).write_text(text, encoding="utf-8")
            elif name in listed and name not in kept:
                continue
            else:
                os.symlink(entry.resolve(), out / name)
    (dest / "corpus.json").write_text(json.dumps(corpus, indent=2) + "\n", encoding="utf-8")


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("source", type=pathlib.Path)
    parser.add_argument("dest", type=pathlib.Path)
    parser.add_argument("--files", type=int, default=3)
    args = parser.parse_args(argv)
    if args.files < 1:
        parser.error("--files must be at least 1")
    cut(args.source, args.dest, args.files)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
