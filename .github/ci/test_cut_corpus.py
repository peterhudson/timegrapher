"""Offline tests of cut_corpus.py. No network, build or audio."""

import json
import pathlib
import tempfile
import unittest

import cut_corpus

MANIFEST = '''watch = "A watch"
bph = 28800
notes = "Header kept as written."

[[recording]]
file = "take_part01-001.flac"
position = "CR"

[[recording]]
file = "take_part01-002.flac"
position = "CR"
measure_s = 337

[[recording]]
file = "take_part01-002.flac"
position = "CR"
settle_s = 377

[[recording]]
file = "take_part01-003.flac"
position = "CR"

[[recording]]
file = "take_part01-004.flac"
position = "CR"

[[recording]]
file = "take_part01-005.flac"
position = "CR"
'''


def names(n):
    return [f"take_part01-{i:03d}.flac" for i in range(1, n + 1)]


LAST = "take_part01-005.flac"


class CutTests(unittest.TestCase):
    def test_manifest_keeps_header_and_entries_for_kept_files(self):
        text = cut_corpus.cut_manifest(MANIFEST, set(names(2)))
        self.assertTrue(text.startswith('watch = "A watch"\nbph = 28800\nnotes = "Header kept as written."\n'))
        self.assertEqual(text.count("[[recording]]"), 3)
        self.assertIn("measure_s = 337", text)
        self.assertIn("settle_s = 377", text)
        self.assertNotIn("take_part01-003.flac", text)

    def test_manifest_keeps_whole_folder_entries(self):
        text = 'bph = 28800\n\n[[recording]]\nfile = "."\nposition = "DU"\n'
        self.assertEqual(cut_corpus.cut_manifest(text, set()), text)

    def test_manifest_without_entries_is_unchanged(self):
        self.assertEqual(cut_corpus.cut_manifest('bph = 21600\n', set()), 'bph = 21600\n')

    def test_an_entry_without_a_file_is_an_error(self):
        with self.assertRaises(ValueError):
            cut_corpus.cut_manifest('[[recording]]\nposition = "DU"\n', set())

    def test_session_json_keeps_only_kept_segments(self):
        text = json.dumps({"bph": 28800, "segments": names(5)})
        doc = json.loads(cut_corpus.cut_session_json(text, set(names(3))))
        self.assertEqual(doc, {"bph": 28800, "segments": names(3)})

    def test_a_corpus_is_cut_to_the_first_files_of_each_take(self):
        with tempfile.TemporaryDirectory() as tmp:
            src, dest = pathlib.Path(tmp, "corpus"), pathlib.Path(tmp, "cut")
            take = src / "takes" / "ym42_CR_long"
            (take / "raw").mkdir(parents=True)
            (src / "takes" / "not_listed").mkdir()
            files = names(5)
            for name in files:
                (take / name).write_bytes(b"audio")
            (take / "clock.csv").write_text("1,2\n")
            (take / "session.toml").write_text(MANIFEST)
            (take / "session.json").write_text(json.dumps({"segments": files}))
            corpus = {"schema": 1, "takes": [
                {"id": "ym42_CR_long", "files": [{"name": n} for n in files]},
                {"id": "not_checked_out", "files": [{"name": "a.flac"}]},
            ]}
            (src / "corpus.json").write_text(json.dumps(corpus))
            cut_corpus.cut(src, dest, 2)

            out = dest / "takes" / "ym42_CR_long"
            self.assertEqual(sorted(p.name for p in out.iterdir()),
                             ["clock.csv", "raw", "session.json", "session.toml"] + names(2) + [LAST])
            for name in ["clock.csv", "raw", LAST] + names(2):
                self.assertTrue((out / name).is_symlink(), name)
                self.assertEqual((out / name).resolve(), (take / name).resolve())
            self.assertNotIn("take_part01-003.flac", (out / "session.toml").read_text())
            self.assertNotIn("take_part01-004.flac", (out / "session.toml").read_text())
            self.assertIn(f'file = "{LAST}"', (out / "session.toml").read_text())
            self.assertEqual(json.loads((out / "session.json").read_text())["segments"], names(2) + [LAST])
            listed = json.loads((dest / "corpus.json").read_text())["takes"]
            self.assertEqual([f["name"] for f in listed[0]["files"]], names(2) + [LAST])
            self.assertEqual(listed[1]["files"], [{"name": "a.flac"}])
            self.assertFalse((dest / "takes" / "not_checked_out").exists())
            self.assertFalse((dest / "takes" / "not_listed").exists())
            # The source is left as it was.
            self.assertEqual((take / "session.toml").read_text(), MANIFEST)
            self.assertEqual(len(json.loads((src / "corpus.json").read_text())["takes"][0]["files"]), 5)

    def test_a_short_take_keeps_every_file_once(self):
        with tempfile.TemporaryDirectory() as tmp:
            src, dest = pathlib.Path(tmp, "corpus"), pathlib.Path(tmp, "cut")
            (src / "takes" / "short").mkdir(parents=True)
            for name in names(3):
                (src / "takes" / "short" / name).write_bytes(b"audio")
            corpus = {"takes": [{"id": "short", "files": [{"name": n} for n in names(3)]}]}
            (src / "corpus.json").write_text(json.dumps(corpus))
            cut_corpus.cut(src, dest, 3)
            listed = json.loads((dest / "corpus.json").read_text())["takes"][0]["files"]
            self.assertEqual([f["name"] for f in listed], names(3))

    def test_the_cut_folder_must_be_new(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = pathlib.Path(tmp, "corpus")
            (src / "takes").mkdir(parents=True)
            (src / "corpus.json").write_text(json.dumps({"takes": []}))
            pathlib.Path(tmp, "cut").mkdir()
            with self.assertRaises(FileExistsError):
                cut_corpus.cut(src, pathlib.Path(tmp, "cut"), 3)


if __name__ == "__main__":
    unittest.main()
