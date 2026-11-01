"""Decide whether a CI run needs the full build and tests.

  changes.py EVENT BASE HEAD    prints full=true or full=false, then
                                pages=true or pages=false

For a pull request CI checks out GitHub's merge commit, so BASE is HEAD^1
(the base branch) and HEAD is HEAD: their diff is what merging would change.

Everything but a pull request is a full run. A pull request is a light run
only when every file it changes is Markdown that nothing compiles in and no
test reads, edited or added in place: never a deletion or rename, never the
watchmaker skill, packaging, or a document the MCP server compiles in
(MCP_DOCS, kept equal to guidance.rs by the offline tests). In a light run
all seven CI jobs still run and report: the test jobs run only their offline
Python tests, the three regress jobs skip their corpus and engine steps, and mcp-renderer
runs as usual. If git fails, this exits with an error and the job fails.

pages is true for a pull request, light or full, that changes a file the
downloads carry as web pages (PAGE_FILES and PAGE_FOLDERS), so CI runs
Release's render and link check of those pages. Release's own pull-request run
covers packaging/. Everything but a pull request prints pages=false.
"""

import subprocess
import sys


# Markdown under docs/ that crates/timegrapher-mcp/src/guidance.rs compiles in.
MCP_DOCS = {
    "docs/agent-interface.md",
    "docs/sessions.md",
    "docs/series.md",
    "docs/fault-signatures.md",
}
# Markdown here is compiled in (the skill) or read by packaging's tests.
FULL_PREFIXES = ("skills/", "packaging/")
# Files and folders packaging/docs_html.py renders into the downloads' pages.
PAGE_FILES = {"README.md", "LICENSE"}
PAGE_FOLDERS = ("docs/", "skills/watchmaker/")


def light(changes):
    """True when every (status, path) change spares the build and tests."""
    if not changes:
        return False
    for status, path in changes:
        if status not in {"M", "A"} or not path.endswith(".md"):
            return False
        if path in MCP_DOCS or path.startswith(FULL_PREFIXES):
            return False
    return True


def pages(changes):
    """True when a (status, path) change touches a page the downloads carry."""
    return any(path in PAGE_FILES or path.startswith(PAGE_FOLDERS) for _, path in changes)


def changed(base, head):
    out = subprocess.check_output(
        ["git", "diff", "--no-renames", "--name-status", base, head], text=True)
    rows = []
    for line in out.splitlines():
        status, _, path = line.partition("\t")
        rows.append((status[:1], path))
    return rows


def main(argv):
    if len(argv) != 4:
        raise SystemExit(__doc__)
    event, base, head = argv[1:]
    changes = changed(base, head) if event == "pull_request" else []
    full = event != "pull_request" or not light(changes)
    print(f"full={'true' if full else 'false'}")
    print(f"pages={'true' if pages(changes) else 'false'}")


if __name__ == "__main__":
    main(sys.argv)
