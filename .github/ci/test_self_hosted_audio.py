"""Offline tests of self_hosted_audio.py (agents#48)."""

import importlib.util
import pathlib
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("self_hosted_audio", HERE / "self_hosted_audio.py")
audio = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audio)


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


class Audio(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = pathlib.Path(self.tmp.name)
        self.store, self.takes = root / "store", root / "work/corpus/takes"

    def tearDown(self):
        self.tmp.cleanup()

    def test_restore_without_a_store_links_nothing(self):
        self.takes.mkdir(parents=True)
        self.assertEqual(audio.restore(self.store, self.takes), 0)

    def test_restore_links_only_into_takes_the_checkout_has_and_never_overwrites(self):
        write(self.store / "a/1.flac", "stored a1")
        write(self.store / "a/2.flac", "stored a2")
        write(self.store / "gone/1.flac", "old take")
        write(self.takes / "a/take.json", "{}")
        write(self.takes / "a/2.flac", "fetched a2")
        self.assertEqual(audio.restore(self.store, self.takes), 1)
        self.assertEqual((self.takes / "a/1.flac").read_text(), "stored a1")
        self.assertEqual((self.takes / "a/1.flac").stat().st_ino, (self.store / "a/1.flac").stat().st_ino)
        self.assertEqual((self.takes / "a/2.flac").read_text(), "fetched a2")
        self.assertFalse((self.takes / "gone").exists())

    def test_keep_makes_the_store_exactly_the_audio_the_checkout_holds(self):
        write(self.store / "a/stale.flac", "dropped from the corpus")
        write(self.takes / "a/1.flac", "a1")
        write(self.takes / "a/take.json", "{}")
        write(self.takes / "b/1.flac", "b1")
        self.assertEqual(audio.keep(self.store, self.takes), 2)
        self.assertEqual(
            sorted(p.as_posix() for p in audio.flacs(self.store)), ["a/1.flac", "b/1.flac"])
        self.assertFalse((self.store / "a/take.json").exists())
        self.assertFalse(self.store.with_name("store.new").exists())
        self.assertFalse(self.store.with_name("store.old").exists())
        # The next job's checkout gets them back.
        fresh = self.takes.parent.parent / "next/corpus/takes"
        (fresh / "a").mkdir(parents=True)
        (fresh / "b").mkdir()
        self.assertEqual(audio.restore(self.store, fresh), 2)


if __name__ == "__main__":
    unittest.main()
