"""The documentation a download carries, as web pages that open offline.

    python3 packaging/docs_html.py render OUT PLATFORM [--ref COMMIT]
    python3 packaging/docs_html.py check FOLDER

render writes OUT/Start Here.html (from packaging/PLATFORM/start-here.md),
OUT/Documentation/ (README.md, docs/ apart from the calibre research
evidence, and the watchmaker skill with its reference pages) and
OUT/LICENSE.txt, for the platform script to copy into the download's
folder. Links between those files are rewritten to the pages beside them;
links to anything else in the repository (source files, workflows) go to
GitHub at COMMIT (default: HEAD). Needs pandoc.

check fails when a page in FOLDER links to a file that is not there, and
warns about a link to a heading that is not there.
"""

import argparse
import fnmatch
import html
import os
import posixpath
import re
import shutil
import subprocess
import sys
from pathlib import Path
from urllib.parse import quote, unquote, urlsplit

ROOT = Path(__file__).resolve().parent.parent
# Set by GitHub Actions; the default is for local runs.
REPO = os.environ.get("GITHUB_REPOSITORY") or "peterhudson/timegrapher"
PLATFORMS = ("linux", "macos", "windows")
INDEX = "docs/index.md"
# Research evidence, the calibre train counts with their photographs (over
# 200 MB), stays on GitHub: links to it from the download go there.
ON_GITHUB = ("docs/*-train-wheel-counts.md", "docs/*-train-research.md",
             "docs/assets/train-wheels/*", "docs/assets/seiko-miyota/*")

STYLE = """
:root { color-scheme: light dark; --fg: #1d1d1f; --muted: #6e6e73; --bg: #fff;
  --line: #d2d2d7; --code: #f5f5f7; --link: #0066cc; }
@media (prefers-color-scheme: dark) {
  :root { --fg: #f5f5f7; --muted: #a1a1a6; --bg: #1d1d1f; --line: #424245;
    --code: #2c2c2e; --link: #2997ff; } }
body { margin: 0; background: var(--bg); color: var(--fg);
  font: 16px/1.55 -apple-system, BlinkMacSystemFont, "Segoe UI", Ubuntu,
  Cantarell, "Helvetica Neue", Arial, sans-serif; }
nav { border-bottom: 1px solid var(--line); padding: 10px 16px; font-size: 14px; }
nav a { margin-right: 18px; }
main { max-width: 46rem; margin: 0 auto; padding: 8px 16px 48px; }
a { color: var(--link); text-decoration: none; } a:hover { text-decoration: underline; }
h1, h2, h3 { line-height: 1.25; } h1 { font-size: 2em; } h2 { margin-top: 2em; }
code, pre { font: 0.9em ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  background: var(--code); border-radius: 6px; }
code { padding: 0.1em 0.3em; } pre { padding: 12px 14px; overflow-x: auto; }
pre code { padding: 0; background: none; }
table { border-collapse: collapse; display: block; overflow-x: auto; }
th, td { border: 1px solid var(--line); padding: 4px 10px; text-align: left; }
blockquote { margin: 0; padding-left: 14px; border-left: 3px solid var(--line);
  color: var(--muted); }
img { max-width: 100%; }
""".strip()

PAGE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
{style}
</style>
</head>
<body>
<nav><a href="{start}">Start Here</a><a href="{readme}">Read Me</a><a href="{docs}">All Documentation</a></nav>
<main>
{body}
</main>
</body>
</html>
"""


def outputs(platform):
    """Repository path -> path in the download folder, for every page and file."""
    out = {
        f"packaging/{platform}/start-here.md": "Start Here.html",
        "README.md": "Documentation/README.html",
        "skills/watchmaker/SKILL.md": "Documentation/watchmaker-skill.html",
        "LICENSE": "LICENSE.txt",
        # Written by index_page, not rendered: the start pages link to it.
        INDEX: "Documentation/index.html",
    }
    for path in sorted((ROOT / "docs").rglob("*")):
        rel = path.relative_to(ROOT).as_posix()
        if path.is_file() and not any(fnmatch.fnmatch(rel, p) for p in ON_GITHUB):
            dest = "Documentation/" + rel[len("docs/"):]
            out[rel] = dest[:-3] + ".html" if dest.endswith(".md") else dest
    # The skill's reference pages, which SKILL.md links to throughout.
    for path in sorted((ROOT / "skills/watchmaker/references").glob("*.md")):
        rel = path.relative_to(ROOT).as_posix()
        out[rel] = f"Documentation/watchmaker-skill/{path.stem}.html"
    return out


def rewrite_links(fragment, source, dest, mapping, ref):
    """Point each relative href or src at the page beside it, or at GitHub."""
    here = posixpath.dirname(source)
    there = posixpath.dirname(dest)

    def fix(match):
        attr, url = match.group(1), html.unescape(match.group(2))
        parts = urlsplit(url)
        if parts.scheme or parts.netloc or url.startswith("#") or not parts.path:
            return match.group(0)
        target = posixpath.normpath(posixpath.join(here, unquote(parts.path)))
        fragment_id = f"#{parts.fragment}" if parts.fragment else ""
        if target in mapping:
            rel = posixpath.relpath(mapping[target], there or ".")
            new = quote(rel) + fragment_id
        else:
            kind = "tree" if (ROOT / target).is_dir() else "blob"
            new = f"https://github.com/{REPO}/{kind}/{ref}/{quote(target)}{fragment_id}"
        return f'{attr}="{html.escape(new, quote=True)}"'

    return re.sub(r'\b(href|src)="([^"]*)"', fix, fragment)


def title_of(fragment, fallback):
    match = re.search(r"<h1[^>]*>(.*?)</h1>", fragment, re.S)
    if not match:
        return fallback
    return html.escape(html.unescape(re.sub(r"<[^>]+>", "", match.group(1))).strip())


def render_page(source, dest, mapping, ref):
    fragment = subprocess.run(
        ["pandoc", "-f", "gfm", "-t", "html5", "--no-highlight", str(ROOT / source)],
        check=True, capture_output=True, text=True, encoding="utf-8").stdout
    fragment = rewrite_links(fragment, source, dest, mapping, ref)
    there = posixpath.dirname(dest) or "."

    def link(path):
        return quote(posixpath.relpath(path, there))

    return PAGE.format(
        title=title_of(fragment, posixpath.basename(source)), style=STYLE,
        start=link("Start Here.html"), readme=link("Documentation/README.html"),
        docs=link("Documentation/index.html"), body=fragment.strip())


def index_page(mapping):
    """Documentation/index.html: every page in the download, by title."""
    rows = []
    for source, dest in mapping.items():
        if source != INDEX and dest.startswith("Documentation/") and dest.endswith(".html"):
            text = (ROOT / source).read_text(encoding="utf-8")
            match = re.search(r"^# (.+)$", text, re.M)
            name = match.group(1).strip() if match else posixpath.basename(source)
            rows.append((name.lower(), name, dest[len("Documentation/"):]))
    items = "\n".join(f'<li><a href="{quote(d)}">{html.escape(n)}</a></li>'
                      for _, n, d in sorted(rows))
    return PAGE.format(
        title="Documentation", style=STYLE, start="../Start%20Here.html",
        readme="README.html", docs="index.html",
        body=f"<h1>Documentation</h1>\n<ul>\n{items}\n</ul>")


def render(out, platform, ref):
    out = Path(out)
    if out.exists():
        shutil.rmtree(out)
    mapping = outputs(platform)
    for source, dest in mapping.items():
        if source == INDEX:
            continue
        path = out / dest
        path.parent.mkdir(parents=True, exist_ok=True)
        if source.endswith(".md"):
            path.write_text(render_page(source, dest, mapping, ref), encoding="utf-8",
                            newline="\n")
        else:
            shutil.copyfile(ROOT / source, path)
    (out / "Documentation/index.html").write_text(index_page(mapping), encoding="utf-8",
                                                  newline="\n")


def check(folder):
    """(missing, unknown): relative links to files that do not exist, and
    #fragments naming no id in their page."""
    folder = Path(folder)
    missing, unknown, pages = [], [], 0

    def ids(path):
        return set(re.findall(r'\bid="([^"]*)"', path.read_text(encoding="utf-8")))

    # os.walk does not follow links, such as the disk image's Applications.
    found = [Path(d) / f for d, _, fs in os.walk(folder) for f in fs if f.endswith(".html")]
    for page in sorted(found):
        pages += 1
        text = page.read_text(encoding="utf-8")
        for url in re.findall(r'\b(?:href|src)="([^"]*)"', text):
            parts = urlsplit(html.unescape(url))
            if parts.scheme or parts.netloc:
                continue
            target = (page.parent / unquote(parts.path)) if parts.path else page
            if not target.exists():
                missing.append(f"{page.relative_to(folder)}: {url}")
            elif parts.fragment and target.suffix == ".html" and \
                    unquote(parts.fragment) not in ids(target):
                unknown.append(f"{page.relative_to(folder)}: {url}")
    if not pages:
        missing.append("no pages")
    return missing, unknown


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    r = sub.add_parser("render")
    r.add_argument("out")
    r.add_argument("platform", choices=PLATFORMS)
    r.add_argument("--ref")
    c = sub.add_parser("check")
    c.add_argument("folder")
    args = parser.parse_args(argv)
    if args.command == "render":
        ref = args.ref or subprocess.run(["git", "-C", str(ROOT), "rev-parse", "HEAD"],
                                         check=True, capture_output=True, text=True).stdout.strip()
        render(args.out, args.platform, ref)
    else:
        missing, unknown = check(args.folder)
        for line in unknown:
            # A heading renamed in a guide: worth fixing, not worth a failed
            # release.
            print(f"::warning::no such heading: {line}")
        for line in missing:
            print(f"broken link: {line}", file=sys.stderr)
        if missing:
            sys.exit(1)


if __name__ == "__main__":
    main()
