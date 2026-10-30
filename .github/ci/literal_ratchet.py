"""Stop new typed-in numbers in the app, CLI and MCP server (guard G11).

  literal_ratchet.py check     fail if a surface crate has more than the
                               baseline allows, a file is too long, or the
                               clippy lists differ (offline, seconds)
  literal_ratchet.py copies    fail if jscpd finds more copied product
                               blocks than the baseline (needs npx)
  literal_ratchet.py update    write the current counts as the baseline
                               (with npx, the copies count too)

Every threshold, default and constant has one home, in timegrapher-core;
the surfaces call it. This counts, in each surface crate's product code
(tests left out), three kinds of typed-in number:

  comparisons  a number compared with a literal (`x > 0.4`, `n >= 30`).
               0, 1 and 2 are left out: they test for empty, one or a pair.
  constants    a `const` or `static` that isn't f32 (f32 is drawing) set
               from a number literal.
               A `let` set from a literal that is then compared counts
               here too (`let limit = 0.4; ... x > limit`).
  defaults     a literal handed in as a default: `unwrap_or(18_000)`,
               `unwrap_or_else(|| 28_800)`, `default_value_t = 20.0`. 0
               and 1 are left out.
  bounds       a literal limit: a range with a literal end
               (`10.0..=90.0`, `5..600`), `clamp(10.0, 90.0)`, `.max(80.0)`
               or `.min(36.0)`. 0, 1 and 2 are left out.

A crate's count may not rise above BASELINE. A PR that must add one (a
layout constant that is the app's alone, say) raises the baseline in the
same PR with `update`, so the reviewer sees it; a PR that removes some
lowers it. Counts are per crate, so moving code between files is free;
the per-file counts are kept only to say where a rise came from.

The check also fails when the three crates' clippy.toml disallowed-methods
lists (guard G1) are not identical, and when a Rust file in crates/ is
longer than MAX_FILE_LINES: a file over it today is listed in the baseline
with its length, which may only fall (the architecture guide's code shape).

`copies` runs jscpd over crates/ and counts the copied blocks that are not
test code on both sides. The count may not rise above the baseline's.
"""

import json
import pathlib
import re
import subprocess
import sys
import tempfile
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]
BASELINE = pathlib.Path(__file__).with_name("literal_baseline.json")
CRATES = ("timegrapher-app", "timegrapher-cli", "timegrapher-mcp")
KINDS = ("comparisons", "constants", "defaults", "bounds")
MAX_FILE_LINES = 3000
JSCPD = ["npx", "--yes", "jscpd@4.3.0", "--format", "rust", "--min-lines", "8", "--min-tokens", "70",
         # Without these jscpd silently skips files over 1,000 lines, app.rs among them.
         "--max-lines", "100000", "--max-size", "10mb", "--reporters", "json", "--silent"]

NUMBER = re.compile(
    r"(?<!\w)(?<!(?<!\.)\.)(?:0x[0-9a-fA-F_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)"
    r"(?:_?(?:f32|f64|u8|u16|u32|u64|u128|usize|i8|i16|i32|i64|i128|isize))?(?!\w)(?!\.(?!\.))"
)
# A comparison operator right before or after a number. `=>`, `->`, `<<`
# and `>>` are not comparisons.
CMP_BEFORE = re.compile(r"(?:(?<![<>=!\-])(?:<=|>=|==|!=)|(?<![<>=\-])[<>](?!=))\s*-?\s*$")
CMP_AFTER = re.compile(r"^\s*(?:(?:<=|>=|==|!=)(?![=>])|[<>](?![<>=]))")
CONST = re.compile(r"\b(?:const|static)\s+(?:mut\s+)?[A-Z_][A-Z0-9_]*\s*:\s*([^=]+?)\s*=\s*([^;]*);")
DEFAULT = re.compile(r"(?:\bunwrap_or\s*\(\s*|\bunwrap_or_else\s*\(\s*\|\s*\|\s*|\bdefault_value_t\s*=\s*)(-?\s*"
                     + NUMBER.pattern + r")")
LET = re.compile(r"\blet\s+(?:mut\s+)?([a-z_][a-z0-9_]*)\s*(?::\s*[\w\[\]; ]+)?=\s*(-?\s*" + NUMBER.pattern + r")\s*;")
LIMIT = re.compile(r"\.(?:max|min)\s*\(\s*(-?\s*" + NUMBER.pattern + r")\s*\)|\bclamp\s*\(([^()]*)\)")


def strip(src):
    """`src` with comments, strings and chars blanked to spaces, keeping
    newlines, so positions and line numbers stay."""
    out = list(src)
    i, n = 0, len(src)

    def blank(a, b):
        for k in range(a, b):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            blank(i, j)
            i = j
        elif src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            blank(i, j)
            i = j
        elif (m := re.compile(r'b?r(#*)"').match(src, i)) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            end = '"' + m.group(1)
            j = src.find(end, m.end())
            j = n if j < 0 else j + len(end)
            blank(i, j)
            i = j
        elif c == '"' or (c == "b" and src.startswith('b"', i) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_"))):
            j = i + (2 if c == "b" else 1)
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            blank(i, j + 1)
            i = j + 1
        elif c == "'":
            # A char literal ('a', '\n', '\u{1F600}'), not a lifetime ('a).
            m = re.compile(r"'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^\\'\n])'").match(src, i)
            if m:
                blank(i, m.end())
                i = m.end()
            else:
                i += 1
        else:
            i += 1
    return "".join(out)


def test_mask(code):
    """Per character: True inside test code (an item after `#[cfg(test)]`
    or `#[test]`), in `code` as `strip` left it."""
    mask = [False] * len(code)
    for m in re.finditer(r"#\[(?:cfg\(test\)|test)\]", code):
        i = m.end()
        # The item ends at its closing brace, or at `;` for `mod x;`.
        while i < len(code) and code[i] not in "{;":
            i += 1
        if i < len(code) and code[i] == "{":
            depth = 0
            while i < len(code):
                depth += {"{": 1, "}": -1}.get(code[i], 0)
                i += 1
                if depth == 0:
                    break
        for k in range(m.start(), min(i + 1, len(code))):
            mask[k] = True
    return mask


def value(text):
    t = re.sub(r"[\s_]", "", text)
    t = re.sub(r"(f32|f64|u8|u16|u32|u64|u128|usize|i8|i16|i32|i64|i128|isize)$", "", t)
    try:
        return float(int(t, 16)) if t.lstrip("-").startswith("0x") else float(t)
    except ValueError:
        return None


def count(src):
    """The typed-in numbers of one file's product code: {kind: [line, ...]}."""
    code = strip(src)
    mask = test_mask(code)
    line_of = lambda pos: code.count("\n", 0, pos) + 1
    found = {k: [] for k in KINDS}
    for m in NUMBER.finditer(code):
        if mask[m.start()] or value(m.group()) in (0.0, 1.0, 2.0):
            continue
        before = code[max(0, m.start() - 8):m.start()]
        before = before.split("\n")[-1]
        after = code[m.end():m.end() + 3]
        if re.search(r"<\s*$", before) and re.match(r"\s*>", after):
            continue  # a const generic, `::<4>`
        if CMP_BEFORE.search(before) or CMP_AFTER.match(after):
            found["comparisons"].append(line_of(m.start()))
    for m in CONST.finditer(code):
        ty, init = m.group(1), m.group(2)
        if mask[m.start()] or "f32" in ty or not NUMBER.search(init):
            continue
        found["constants"].append(line_of(m.start()))
    for m in DEFAULT.finditer(code):
        if mask[m.start()] or value(m.group(1)) in (0.0, 1.0):
            continue
        found["defaults"].append(line_of(m.start()))
    for m in LET.finditer(code):
        name = m.group(1)
        if mask[m.start()] or value(m.group(2)) in (0.0, 1.0, 2.0):
            continue
        cmp = r"(?:<=|>=|==|!=|(?<![<>=\-])[<>](?![<>=]))"
        rest = code[m.end():]
        if re.search(r"\b" + name + r"\b\s*" + cmp + r"|" + cmp + r"\s*\b" + name + r"\b", rest):
            found["comparisons"].append(line_of(m.start()))
    for m in NUMBER.finditer(code):
        if mask[m.start()] or value(m.group()) in (0.0, 1.0, 2.0):
            continue
        if re.search(r"\.\.=?\s*-?\s*$", code[max(0, m.start() - 6):m.start()]) or \
                re.match(r"\s*\.\.", code[m.end():m.end() + 4]):
            found["bounds"].append(line_of(m.start()))
    for m in LIMIT.finditer(code):
        if mask[m.start()]:
            continue
        lits = [m.group(1)] if m.group(1) else NUMBER.findall(m.group(2) or "")
        if any(value(x) not in (0.0, 1.0, 2.0, None) for x in lits):
            found["bounds"].append(line_of(m.start()))
    return found


def is_test_file(path):
    return path.name.endswith("_tests.rs") or "tests" in path.parts or "benches" in path.parts


def scan(root=ROOT):
    """{crate: {"total": {kind: n}, "files": {path: {kind: n}}}}."""
    out = {}
    for crate in CRATES:
        files, total = {}, {k: 0 for k in KINDS}
        for path in sorted((root / "crates" / crate / "src").rglob("*.rs")):
            if is_test_file(path.relative_to(root)):
                continue
            found = count(path.read_text(encoding="utf-8"))
            n = {k: len(v) for k, v in found.items()}
            if any(n.values()):
                files[path.relative_to(root).as_posix()] = n
            for k in KINDS:
                total[k] += n[k]
        out[crate] = {"total": total, "files": files}
    return out


# A function-limit mark (#[expect] for too_many_lines or cognitive_complexity
# alone) is not code: it comes off when the function is split.
LIMIT_MARK = re.compile(
    r"^[ \t]*#\[expect\(\s*(?:clippy::(?:too_many_lines|cognitive_complexity)\s*,\s*)+"
    r"reason\s*=\s*\x22[^\x22]*\x22\s*,?\s*\)\]\n",
    re.M,
)


def file_lines(text):
    """Lines in `text`, less those of its function-limit marks."""
    return text.count("\n") - sum(m.group().count("\n") for m in LIMIT_MARK.finditer(text))


def long_files(root=ROOT):
    """{path: lines} for every Rust file in crates/ over MAX_FILE_LINES,
    its function-limit marks left out ([`file_lines`])."""
    out = {}
    for path in sorted((root / "crates").rglob("*.rs")):
        n = file_lines(path.read_text(encoding="utf-8"))
        if n > MAX_FILE_LINES:
            out[path.relative_to(root).as_posix()] = n
    return out


def is_test_line(path, line, root=ROOT, cache={}):
    """Whether `line` of `path` (relative to root) is test code."""
    if is_test_file(pathlib.PurePosixPath(path)):
        return True
    if path not in cache:
        code = strip((root / path).read_text(encoding="utf-8"))
        mask, starts, k = test_mask(code), [0], 0
        for ch in code:
            k += 1
            if ch == "\n":
                starts.append(k)
        cache[path] = (mask, starts)
    mask, starts = cache[path]
    return line - 1 < len(starts) and starts[line - 1] < len(mask) and mask[starts[line - 1]]


def copies(report, root=ROOT):
    """The product blocks in a jscpd JSON report: those not test code on both sides."""
    out = []
    for d in report["duplicates"]:
        a, b = d["firstFile"], d["secondFile"]
        if not (is_test_line(a["name"], a["start"], root) and is_test_line(b["name"], b["start"], root)):
            out.append(f'{a["name"]}:{a["start"]} = {b["name"]}:{b["start"]} ({d["lines"]} lines)')
    return out


def run_jscpd(root=ROOT):
    with tempfile.TemporaryDirectory() as out:
        subprocess.run(JSCPD + ["--output", out, "crates"], cwd=root, check=True,
                       stdout=subprocess.DEVNULL, shell=sys.platform == "win32")
        return json.loads((pathlib.Path(out) / "jscpd-report.json").read_text(encoding="utf-8"))


def clippy_lists(root=ROOT):
    """Each surface crate's disallowed-methods list, as (path, reason) pairs."""
    lists = {}
    for crate in CRATES:
        f = root / "crates" / crate / "clippy.toml"
        data = tomllib.loads(f.read_text(encoding="utf-8")) if f.exists() else {}
        lists[crate] = sorted((d["path"], d.get("reason", "")) for d in data.get("disallowed-methods", []))
    return lists


def problems(now, baseline, lists, long=None):
    """What fails the check, one line each."""
    out = []
    allowed = baseline.get("long_files", {})
    for f, n in (long or {}).items():
        if n > allowed.get(f, MAX_FILE_LINES):
            limit = f"its baseline {allowed[f]}" if f in allowed else f"{MAX_FILE_LINES}"
            out.append(f"{f}: {n} lines, over {limit}. Split it by concern; a file over "
                       f"{MAX_FILE_LINES} lines may only shrink.")
    for crate in (c for c in CRATES if c in now):
        base = baseline.get("crates", {}).get(crate, {"total": {}, "files": {}})
        for k in KINDS:
            have, allowed = now[crate]["total"][k], base["total"].get(k, 0)
            if have > allowed:
                where = [f"{f} ({n[k]}, was {base['files'].get(f, {}).get(k, 0)})"
                         for f, n in now[crate]["files"].items()
                         if n[k] > base["files"].get(f, {}).get(k, 0)]
                out.append(f"{crate}: {have} {k}, baseline {allowed}. Take the number from core, "
                           f"or raise the baseline in this PR with a reason. More than before in: "
                           + (", ".join(where) or "(moved between files)"))
    first, *rest = CRATES
    for crate in rest:
        if lists[crate] != lists[first]:
            a, b = set(lists[first]), set(lists[crate])
            out.append(f"clippy.toml disallowed-methods differ: {first} vs {crate}; only in {first}: "
                       f"{sorted(p for p, _ in a - b)}; only in {crate}: {sorted(p for p, _ in b - a)}")
    return out


def main(argv):
    cmd = argv[1] if len(argv) > 1 else "check"
    baseline = json.loads(BASELINE.read_text(encoding="utf-8")) if BASELINE.exists() else {}
    if cmd == "update":
        baseline["crates"] = scan()
        baseline["long_files"] = long_files()
        try:
            baseline["copies"] = len(copies(run_jscpd()))
        except (OSError, subprocess.CalledProcessError) as e:
            print(f"jscpd didn't run ({e}); the copies count is unchanged")
        BASELINE.write_text(json.dumps(baseline, indent=1, sort_keys=True) + "\n", encoding="utf-8")
        print(f"wrote {BASELINE.relative_to(ROOT)}")
        return 0
    if cmd == "copies":
        found = copies(run_jscpd())
        allowed = baseline.get("copies", 0)
        print(f"copied product blocks: {len(found)} (baseline {allowed})")
        if len(found) > allowed:
            print("\n".join(found))
            print(f"FAIL more copied product blocks than the baseline {allowed}: take the copy out, "
                  "or raise the baseline in this PR with a reason")
            return 1
        if len(found) < allowed:
            print("  below the baseline: lower it with `update` in this PR")
        return 0
    if cmd != "check":
        print(__doc__)
        return 2
    now, long = scan(), long_files()
    bad = problems(now, baseline, clippy_lists(), long)
    for crate in CRATES:
        t, b = now[crate]["total"], baseline.get("crates", {}).get(crate, {}).get("total", {})
        print(crate + ": " + ", ".join(f"{k} {t[k]} (baseline {b.get(k, 0)})" for k in KINDS))
        if any(t[k] < b.get(k, 0) for k in KINDS):
            print(f"  {crate} is below its baseline: lower it with `update` in this PR")
    for f, n in long.items():
        print(f"{f}: {n} lines (baseline {baseline.get('long_files', {}).get(f, MAX_FILE_LINES)})")
    for line in bad:
        print("FAIL " + line)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
