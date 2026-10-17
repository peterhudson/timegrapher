"""Offline tests of docs_html.py's link handling (rendering needs pandoc and
runs in the Release workflow's docs job)."""

import tempfile
import unittest
from pathlib import Path

import docs_html

REF = "0123abc"


class Outputs(unittest.TestCase):
    def test_every_guide_has_a_page(self):
        mapping = docs_html.outputs("macos")
        self.assertEqual(mapping["packaging/macos/start-here.md"], "Start Here.html")
        self.assertEqual(mapping["README.md"], "Documentation/README.html")
        self.assertEqual(mapping["docs/app.md"], "Documentation/app.html")
        self.assertEqual(mapping["LICENSE"], "LICENSE.txt")
        for source in mapping:
            if source != docs_html.INDEX:
                self.assertTrue((docs_html.ROOT / source).is_file(), source)

    def test_each_platform_has_a_start_page(self):
        for platform in docs_html.PLATFORMS:
            self.assertTrue((docs_html.ROOT / f"packaging/{platform}/start-here.md").is_file())


class RewriteLinks(unittest.TestCase):
    def rewrite(self, href, source="README.md"):
        mapping = docs_html.outputs("linux")
        out = docs_html.rewrite_links(f'<a href="{href}">x</a>', source,
                                      mapping[source], mapping, REF)
        return out[len('<a href="'):-len('">x</a>')]

    def test_guides_link_to_the_pages_beside_them(self):
        self.assertEqual(self.rewrite("docs/app.md"), "app.html")
        self.assertEqual(self.rewrite("docs/long-runs.md#how-the-search-works"),
                         "long-runs.html#how-the-search-works")
        self.assertEqual(self.rewrite("skills/watchmaker/SKILL.md"), "watchmaker-skill.html")
        self.assertEqual(self.rewrite("sessions.md", "docs/app.md"), "sessions.html")
        self.assertEqual(self.rewrite("../README.md", "docs/app.md"), "README.html")

    def test_start_page_links_into_documentation(self):
        self.assertEqual(self.rewrite("../../docs/mcp.md", "packaging/linux/start-here.md"),
                         "Documentation/mcp.html")
        self.assertEqual(self.rewrite("../../README.md", "packaging/linux/start-here.md"),
                         "Documentation/README.html")

    def test_back_to_the_start_page_quotes_the_space(self):
        mapping = docs_html.outputs("linux")
        mapping["docs/x.md"] = "Documentation/x.html"
        out = docs_html.rewrite_links('<a href="../packaging/linux/start-here.md">x</a>',
                                      "docs/x.md", "Documentation/x.html", mapping, REF)
        self.assertIn('href="../Start%20Here.html"', out)

    def test_other_repository_files_go_to_github_at_the_commit(self):
        self.assertEqual(
            self.rewrite("../crates/timegrapher-core/src/measure.rs", "docs/app.md"),
            f"https://github.com/{docs_html.REPO}/blob/{REF}/crates/timegrapher-core/src/measure.rs")
        self.assertEqual(self.rewrite("../crates", "docs/app.md"),
                         f"https://github.com/{docs_html.REPO}/tree/{REF}/crates")

    def test_web_links_and_anchors_are_kept(self):
        self.assertEqual(self.rewrite("https://example.com/a.md"), "https://example.com/a.md")
        self.assertEqual(self.rewrite("#install"), "#install")


class Check(unittest.TestCase):
    def test_reports_missing_files_and_headings(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            (folder / "Documentation").mkdir()
            (folder / "Start Here.html").write_text(
                '<a href="Documentation/a.html#b">ok</a><a href="Documentation/gone.html">x</a>'
                '<a href="Documentation/a.html#nope">y</a><a href="https://e.com/">z</a>')
            (folder / "Documentation/a.html").write_text('<h2 id="b">B</h2><a href="#b">self</a>')
            missing, unknown = docs_html.check(folder)
            self.assertEqual(missing, ["Start Here.html: Documentation/gone.html"])
            self.assertEqual(unknown, ["Start Here.html: Documentation/a.html#nope"])

    def test_an_empty_folder_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(docs_html.check(tmp)[0], ["no pages"])


if __name__ == "__main__":
    unittest.main()
