"""Offline tests of CI's light-run rule. Reads the tree; no network or git."""

import pathlib
import re
import tempfile
import unittest

import changes


ROOT = pathlib.Path(__file__).resolve().parents[2]


class Light(unittest.TestCase):
    def test_edited_or_added_plain_markdown_is_light(self):
        self.assertTrue(changes.light([("M", "docs/app.md"), ("A", "docs/new-page.md"),
                                       ("M", "README.md"), ("M", "personas/README.md")]))

    def test_code_or_any_other_file_is_full(self):
        for path in ("crates/timegrapher-core/src/lib.rs", "Cargo.lock", "regress/baseline.json",
                     ".github/workflows/ci.yml", "personas/harness/round.py", "docs/assets/x.png"):
            self.assertFalse(changes.light([("M", "docs/app.md"), ("M", path)]), path)

    def test_compiled_in_markdown_is_full(self):
        for path in sorted(changes.MCP_DOCS) + ["skills/watchmaker/SKILL.md",
                                                 "skills/watchmaker/references/long-runs.md",
                                                 "packaging/macos/start-here.md"]:
            self.assertFalse(changes.light([("M", path)]), path)

    def test_deleting_or_renaming_markdown_is_full(self):
        # packaging/test_docs_html.py checks that the pages it maps exist.
        self.assertFalse(changes.light([("D", "docs/app.md")]))
        self.assertFalse(changes.light([("D", "docs/app.md"), ("A", "docs/application.md")]))

    def test_no_changes_is_full(self):
        self.assertFalse(changes.light([]))


class Tree(unittest.TestCase):
    """The rule must keep covering every Markdown file the code builds in or
    the tests read.

    These scan for Markdown paths written out as string literals: in any Rust
    source under crates/ (include_str!, include_bytes!, a doc!-style macro or a
    file a test opens at run time), in the MCP renderer's JavaScript tests,
    and in packaging's and .github's Python, except this folder's own files,
    whose literals are the rule and its test cases. They cannot see a path
    that code assembles from pieces at run time, or a whole folder listed at
    run time; nothing in CI's tests reads Markdown that way today
    (packaging's docs_html.py lists docs/ only to map page names, whose files'
    existence its tests check, so deleting or renaming any file is a full
    run).
    """

    # Literals that name a Markdown file without reading the repository's copy.
    # An entry covers every literal of that path in its file, so a real read
    # of the same path added there later would be missed: remove the entry.
    NOT_READ = {
        # Writes a new README.md into the synthetic take's folder.
        ("crates/timegrapher-cli/src/synth_cmd.rs", "README.md"),
        # Page names and existence checks; the text is read only when the
        # Release workflow renders the pages.
        ("packaging/docs_html.py", "README.md"),
        ("packaging/test_docs_html.py", "README.md"),
        ("packaging/test_docs_html.py", "docs/app.md"),
        ("packaging/test_docs_html.py", "docs/mcp.md"),
    }

    def sources(self):
        paths = [path for path in (ROOT / "crates").rglob("*")
                 if path.suffix in {".rs", ".cjs", ".js", ".py"} and "target" not in path.parts]
        paths += (ROOT / "packaging").rglob("*.py")
        paths += [path for path in (ROOT / ".github").rglob("*.py")
                  if path.parent != pathlib.Path(__file__).resolve().parent]
        return paths

    def named_markdown(self, sources=None):
        """(source, repository path) for each literal naming a Markdown file here."""
        found = set()
        for source in self.sources() if sources is None else sources:
            text = source.read_text(encoding="utf-8", errors="replace")
            for literal in re.findall(r"[\"']([^\"'\s]*\.md)[\"']", text):
                for candidate in (ROOT / literal.lstrip("/"), source.parent / literal,
                                  ROOT / literal.split("../../")[-1].lstrip("/")):
                    candidate = candidate.resolve()
                    if candidate.is_file() and candidate.is_relative_to(ROOT):
                        found.add((source.name if sources else source.relative_to(ROOT).as_posix(),
                                   candidate.relative_to(ROOT).as_posix()))
                        break
        return found

    def test_mcp_docs_match_guidance(self):
        guidance = (ROOT / "crates/timegrapher-mcp/src/guidance.rs").read_text(encoding="utf-8")
        compiled = set(re.findall(r'"(docs/[^"]+\.md)"', guidance))
        self.assertEqual(compiled, changes.MCP_DOCS)

    def test_every_markdown_file_the_code_names_is_full(self):
        found = self.named_markdown()
        # guidance.rs alone names the four docs, the skill and its references.
        self.assertGreater(len(found), 15)
        for source, path in sorted(found - self.NOT_READ):
            self.assertFalse(changes.light([("M", path)]), f"{source} names {path}")

    def test_the_not_read_list_is_current(self):
        self.assertLessEqual(self.NOT_READ, self.named_markdown())

    def test_the_scan_sees_each_kind_of_reference(self):
        forms = {"a.rs": 'include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/app.md"))',
                 "b.rs": 'const ICON: &[u8] = include_bytes!("/docs/app.md");',
                 "c.rs": 'doc!("timegrapher://docs/app", "app", "App", "x", "docs/app.md")',
                 "d.rs": 'let text = fs::read_to_string("docs/app.md").unwrap();',
                 "e.cjs": "const text = fs.readFileSync('docs/app.md', 'utf8');",
                 "f.py": 'TEXT = (ROOT / "docs/app.md").read_text()'}
        with tempfile.TemporaryDirectory() as tmp:
            sources = []
            for name, line in forms.items():
                (pathlib.Path(tmp) / name).write_text(line + "\n", encoding="utf-8")
                sources.append(pathlib.Path(tmp) / name)
            found = self.named_markdown(sources)
        self.assertEqual(found, {(name, "docs/app.md") for name in forms})
        # docs/app.md is plain Markdown, so a real line like these would fail
        # test_every_markdown_file_the_code_names_is_full.
        self.assertTrue(changes.light([("M", "docs/app.md")]))


if __name__ == "__main__":
    unittest.main()
