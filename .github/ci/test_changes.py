"""Offline tests of CI's light-run rule. Reads the tree; no network or git."""

import pathlib
import re
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
    """The rule must keep covering every Markdown file the code builds in."""

    def rust(self):
        return {path: path.read_text(encoding="utf-8")
                for path in (ROOT / "crates").rglob("*.rs") if "target" not in path.parts}

    def test_mcp_docs_match_guidance(self):
        guidance = (ROOT / "crates/timegrapher-mcp/src/guidance.rs").read_text(encoding="utf-8")
        compiled = set(re.findall(r'"(docs/[^"]+\.md)"', guidance))
        self.assertEqual(compiled, changes.MCP_DOCS)

    def test_every_included_markdown_file_is_full(self):
        found = []
        for path, text in self.rust().items():
            for literal in re.findall(r'include_str!\((?:concat!\()?[^;]*?"([^"]*\.md)"', text, re.S):
                found.append((path, literal.split("../../")[-1].lstrip("/")))
        guidance = ROOT / "crates/timegrapher-mcp/src/guidance.rs"
        found += [(guidance, literal.split("../../")[-1].lstrip("/")) for literal in
                  re.findall(r'"([^"\s]+\.md)"', guidance.read_text(encoding="utf-8"))]
        self.assertGreater(len(found), 10)
        for path, relative in found:
            self.assertTrue((ROOT / relative).is_file(), f"{path}: {relative}")
            self.assertFalse(changes.light([("M", relative)]), f"{path}: {relative}")


if __name__ == "__main__":
    unittest.main()
