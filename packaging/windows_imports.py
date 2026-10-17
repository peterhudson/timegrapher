"""Fail when a shipped Windows program needs a DLL a fresh Windows lacks.

    python packaging/windows_imports.py DIR_OR_EXE...

Reads each .exe's import table (no Windows tools needed) and rejects the
Visual C++ runtime DLLs, which come only with the VC++ Redistributable.
"""

from pathlib import Path
import struct
import sys

NOT_ON_FRESH_WINDOWS = ("vcruntime", "msvcp", "concrt", "vccorlib", "ucrtbased")


def imports(path):
    data = Path(path).read_bytes()
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        raise ValueError(f"{path}: not a PE file")
    sections_n = struct.unpack_from("<H", data, pe + 6)[0]
    optional_size = struct.unpack_from("<H", data, pe + 20)[0]
    optional = pe + 24
    magic = struct.unpack_from("<H", data, optional)[0]
    directories = optional + (112 if magic == 0x20B else 96)
    import_rva = struct.unpack_from("<I", data, directories + 8)[0]
    sections = []
    for i in range(sections_n):
        s = optional + optional_size + 40 * i
        size, rva, _, raw = struct.unpack_from("<IIII", data, s + 8)
        sections.append((rva, size, raw))

    def offset(rva):
        for start, size, raw in sections:
            if start <= rva < start + size:
                return rva - start + raw
        raise ValueError(f"{path}: address {rva:#x} outside every section")

    names = []
    if not import_rva:
        return names
    entry = offset(import_rva)
    while True:
        lookup, _, _, name_rva, thunk = struct.unpack_from("<IIIII", data, entry)
        if not (lookup or name_rva or thunk):
            return names
        at = offset(name_rva)
        names.append(data[at:data.index(b"\0", at)].decode("ascii"))
        entry += 20


def main(args):
    exes = []
    for arg in args:
        p = Path(arg)
        exes += sorted(p.glob("*.exe")) if p.is_dir() else [p]
    if not exes:
        sys.exit("no .exe files found")
    bad = False
    for exe in exes:
        dlls = imports(exe)
        missing = [d for d in dlls if d.lower().startswith(NOT_ON_FRESH_WINDOWS)]
        print(f"{exe.name}: {', '.join(dlls)}")
        if missing:
            print(f"  needs {', '.join(missing)}, which a fresh Windows does not have")
            bad = True
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main(sys.argv[1:])
