"""Offline tests of the typed-in number ratchet (guard G11), and the check
itself on this tree. Reads the tree; no network, git or cargo."""

import copy
import unittest

import literal_ratchet as r


def kinds(src):
    return {k: len(v) for k, v in r.count(src).items()}


class Count(unittest.TestCase):
    def test_a_threshold_compared_is_counted(self):
        self.assertEqual(kinds("fn f(q: f64) -> bool { q > 0.4 }")["comparisons"], 1)
        self.assertEqual(kinds("fn f(n: usize) -> bool { 30 <= n }")["comparisons"], 1)
        self.assertEqual(kinds("fn f(x: f64) -> bool { x >= -12.5 }")["comparisons"], 1)

    def test_empty_one_and_pair_checks_are_not(self):
        self.assertEqual(kinds("fn f(v: &[u8]) -> bool { v.len() > 0 && v.len() >= 2 || v.len() == 1 }")
                         ["comparisons"], 0)

    def test_arrows_shifts_and_generics_are_not_comparisons(self):
        src = ("fn f(k: u8) -> u32 { match k { 3 => 40, _ => 1 << 20 } }\n"
               "fn g(n: &[u8]) { for v in n.as_chunks::<4>().0 {} }\n")
        self.assertEqual(kinds(src)["comparisons"], 0)

    def test_comments_and_strings_are_not_code(self):
        src = ('// if q > 0.4\n/* x < 7.0 /* nested 9 > 3 */ */\n'
               'fn f() -> String { format!("{} > 0.4", r#"y >= 5.0"#) }\n')
        self.assertEqual(kinds(src), {"comparisons": 0, "constants": 0, "defaults": 0, "bounds": 0})

    def test_test_code_is_left_out(self):
        src = ("#[cfg(test)]\nmod tests {\n    const N: f64 = 3.0;\n    fn f(x: f64) -> bool { x > 9.0 }\n}\n"
               "#[test]\nfn t() { assert!(5.0 > 4.0); }\n")
        self.assertEqual(kinds(src), {"comparisons": 0, "constants": 0, "defaults": 0, "bounds": 0})

    def test_constants_from_a_literal_but_not_f32_or_from_core(self):
        src = ("const SETTLE_S: f64 = 30.0;\npub const WIDTH: f32 = 146.0;\n"
               "const MEASURE_S: f64 = timegrapher_core::clock::MEASURE_S;\n"
               "const MARKS: [f64; 2] = [-10.0, -6.0];\n")
        self.assertEqual(kinds(src)["constants"], 2)

    def test_defaults_handed_in(self):
        src = ("fn f(b: Option<u32>) -> u32 { b.unwrap_or(18_000) + b.unwrap_or(0) }\n"
               "struct O { #[arg(long, default_value_t = 52.0)] lift: f64 }\n")
        self.assertEqual(kinds(src)["defaults"], 2)

    def test_a_literal_bound_to_a_name_then_compared_is_counted(self):
        self.assertEqual(kinds("fn f(q: f64) -> bool { let limit = 0.4; q > limit }")["comparisons"], 1)
        self.assertEqual(kinds("fn f() -> usize { let n = 7; n * 3 }")["comparisons"], 0)

    def test_a_default_from_a_closure_is_counted(self):
        self.assertEqual(kinds("fn f(b: Option<u32>) -> u32 { b.unwrap_or_else(|| 28_800) }")["defaults"], 1)

    def test_literal_limits_are_bounds(self):
        src = ("fn f(l: f64, w: f32) -> bool { (10.0..=90.0).contains(&l) && w.max(80.0) > w.min(36.0) }\n"
               "fn g(x: f64) -> f64 { x.clamp(5.0, 600.0) + x.clamp(0.0, 1.0) }\n"
               "fn h() { for _ in 0..6 {} for _ in 0..2 {} }\n")
        self.assertEqual(kinds(src)["bounds"], 6)

    def test_a_tuple_field_or_range_is_not_a_number(self):
        self.assertEqual(kinds("fn f(p: (f64, f64)) -> bool { p.0 < p.1 && (0..5).len() > 3 }")
                         ["comparisons"], 1)


class Ratchet(unittest.TestCase):
    def setUp(self):
        self.now = {c: {"total": {k: 1 for k in r.KINDS}, "files": {f"{c}/a.rs": {k: 1 for k in r.KINDS}}}
                    for c in r.CRATES}
        self.lists = {c: [("a::b", "why")] for c in r.CRATES}

    def test_at_or_under_the_baseline_passes(self):
        self.assertEqual(r.problems(self.now, self.now, self.lists), [])
        lower = copy.deepcopy(self.now)
        lower["timegrapher-app"]["total"]["comparisons"] = 0
        self.assertEqual(r.problems(lower, self.now, self.lists), [])

    def test_a_rise_fails_and_names_the_file(self):
        more = copy.deepcopy(self.now)
        more["timegrapher-cli"]["total"]["defaults"] = 2
        more["timegrapher-cli"]["files"]["timegrapher-cli/a.rs"]["defaults"] = 2
        bad = r.problems(more, self.now, self.lists)
        self.assertEqual(len(bad), 1)
        self.assertIn("timegrapher-cli: 2 defaults, baseline 1", bad[0])
        self.assertIn("timegrapher-cli/a.rs (2, was 1)", bad[0])

    def test_clippy_lists_must_match(self):
        lists = copy.deepcopy(self.lists)
        lists["timegrapher-mcp"].append(("c::d", "why"))
        bad = r.problems(self.now, self.now, lists)
        self.assertEqual(len(bad), 1)
        self.assertIn("only in timegrapher-mcp: ['c::d']", bad[0])


class Tree(unittest.TestCase):
    """The guard itself: this tree against the checked-in baseline."""

    def test_no_surface_crate_types_more_numbers_than_its_baseline(self):
        import json
        baseline = json.loads(r.BASELINE.read_text(encoding="utf-8"))
        self.assertEqual(r.problems(r.scan(), baseline, r.clippy_lists()), [])

    def test_every_surface_crate_has_a_clippy_list(self):
        for crate, entries in r.clippy_lists().items():
            self.assertTrue(entries, crate)


if __name__ == "__main__":
    unittest.main()
