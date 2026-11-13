//! A figure as text, the one rule every surface writes it by: its sign,
//! its ± and its place (math guide section 11; #806, #815, #1470).
//! Core keeps full precision and JSON is never rounded; only text is, once,
//! here.

/// A signed figure as the app shows it, to `digits` decimals: `+1.2`,
/// `-1.2`, and a figure that rounds to zero takes no sign (`0.0`, never
/// `-0.0` or `+0.0`). The app sets the minus as a true minus sign.
pub fn signed_text(x: f64, digits: usize) -> String {
    let s = format!("{:.digits$}", x.abs());
    if s.bytes().all(|b| b == b'0' || b == b'.') {
        s
    } else if x < 0.0 {
        format!("-{s}")
    } else {
        format!("+{s}")
    }
}

/// A figure to `digits` decimals with its sign as it is, but one that
/// rounds to zero written without a minus (`0.0`, never `-0.0`): the one
/// rule for that (#815). [`signed_text`] also adds a plus.
pub fn fixed_text(x: f64, digits: usize) -> String {
    let text = format!("{x:.digits$}");
    match text.strip_prefix('-') {
        Some(rest) if rest.bytes().all(|b| b == b'0' || b == b'.') => rest.to_string(),
        _ => text,
    }
}

/// Text as a screen shows it: every hyphen-minus set as a true minus
/// sign (U+2212), `−1.2 s/d` rather than `-1.2 s/d`. The one place that
/// does it; text meant for a file or a pipe (CSV, JSON, the command
/// line) keeps the ASCII minus.
pub fn true_minus(text: &str) -> String {
    text.replace('-', "\u{2212}")
}

/// A sentence as a screen shows it: a hyphen-minus that starts a figure
/// (a digit or a point and a digit follow it, and no letter or digit
/// comes before it) set as a true minus, every other hyphen kept, so
/// "level shift: -212.6 s/d" turns and "2-second" and "10-30" do not.
/// For core sentences that mix words and figures (#1437); a figure
/// alone takes [`true_minus`].
pub fn figure_minus(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let digit = |i: usize| chars.get(i).is_some_and(|c| c.is_ascii_digit());
    chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let after = digit(i + 1) || (chars.get(i + 1) == Some(&'.') && digit(i + 2));
            let before = i > 0 && chars[i - 1].is_alphanumeric();
            if c == '-' && after && !before {
                '\u{2212}'
            } else {
                c
            }
        })
        .collect()
}

/// Each figure's base decimals, with no ± to follow (the math guide's
/// section 11): what [`figure_digits`] starts from.
pub mod base_digits {
    /// Rate, s/d.
    pub const RATE: usize = 1;
    /// Rate p05 to p95, s/d.
    pub const RATE_RANGE: usize = 0;
    /// Amplitude, degrees.
    pub const AMPLITUDE: usize = 0;
    /// Beat error, ms.
    pub const BEAT_ERROR: usize = 2;
    /// Jitter, µs.
    pub const JITTER: usize = 0;
    /// Clock, ppm.
    pub const CLOCK: usize = 1;
    /// Slope of the rate against the amplitude, s/d per degree.
    pub const SLOPE: usize = 2;
}

/// The decimals a difference within one take takes beyond its figure's
/// place: a trend per hour, a cycle's peak to peak. Both ends share the
/// take's lift angle and sound card clock, so the clock cancels and the
/// lift angle only scales it by a few percent; repeatability sets its
/// limit, not the accuracy that sets [`base_digits`] (Norm on #1470, math
/// guide section 11).
pub const DIFFERENCE_EXTRA_DIGITS: usize = 1;

/// The decimals a within-take difference of a figure written to `base`
/// ([`base_digits`]) is written to: [`DIFFERENCE_EXTRA_DIGITS`] more.
pub fn difference_digits(base: usize) -> usize {
    base + DIFFERENCE_EXTRA_DIGITS
}

/// How many decimals a figure is written to: its place in the math
/// guide's section 11, `base` ([`base_digits`]), whatever its ± (Peter, 12
/// Nov, #1470). The ± measures repeatability only; the lift angle and the
/// sound card's clock limit accuracy to about these places, so a narrow ±
/// adds no decimals (this replaces #510 K3's "decimals follow the ±" for
/// text). `se` is taken so every caller says what it has; it never moves
/// the place.
pub fn figure_digits(_se: Option<f64>, base: usize) -> usize {
    base
}

/// A standard error as every surface writes it after "±": at the figure's
/// place (`digits`, [`base_digits`]), rounded up, and never below one unit
/// of the place (#1470). So an amplitude's 0.3° reads `1`, a rate's
/// 0.04 s/d `0.1`, a beat error's 0.011 ms `0.02`. Rounding an uncertainty
/// up is the safe direction. `None` when it isn't a positive, finite
/// number.
pub fn pm_text(se: f64, digits: usize) -> Option<String> {
    if !(se.is_finite() && se > 0.0) {
        return None;
    }
    let units = 10f64.powi(digits as i32);
    // A ± already at a place (0.1 at 1 decimal) stays there: the
    // tolerance takes up the float error of the scaling, far under a
    // unit.
    let up = (se * units - 1e-9).ceil().max(1.0);
    Some(format!("{:.digits$}", up / units))
}

/// A figure as text at its place (`base`, [`base_digits`], #1470).
/// `signed` writes a rate's or clock's sign, none on a zero
/// ([`signed_text`]); else a zero takes no minus ([`fixed_text`]). `se` is
/// the figure's standard error, if it has one; it never moves the place.
pub fn figure(v: f64, se: Option<f64>, base: usize, signed: bool) -> String {
    let d = figure_digits(se, base);
    if signed {
        signed_text(v, d)
    } else {
        fixed_text(v, d)
    }
}

/// A figure and, where it has one, its ± after it, each at the figure's
/// place ([`figure`], [`pm_text`]): "+1.2 ± 0.1".
pub fn figure_with_pm(v: f64, se: Option<f64>, base: usize, signed: bool) -> String {
    let pm = se
        .and_then(|e| pm_text(e, base))
        .map_or_else(String::new, |t| format!(" ± {t}"));
    format!("{}{pm}", figure(v, se, base, signed))
}

/// How many significant figures `x` has as written to `digits` decimals
/// ([`fixed_text`]): 302 at 0 decimals has 3, 0.46 at 2 has 2, 0.40 at 2
/// has 2 (a written trailing zero counts). A figure that writes as zero
/// has none.
pub fn significant_figures(x: f64, digits: usize) -> usize {
    let text = fixed_text(x, digits);
    let figures: String = text.chars().filter(char::is_ascii_digit).collect();
    figures.trim_start_matches('0').len()
}

/// The decimals that write `x` to `sig` significant figures, never under
/// one: 1.746 to 2 is 1 ("1.7"), 0.7634 to 2 is 2 ("0.76"), 302.4 to 2 is
/// 0 ("302": a place left of the point is never blanked). The place is
/// read again after rounding, so 0.996 to 2 is "1.0", not "1.00". Math
/// guide section 11: a value worked out from figures keeps as many
/// significant figures as the fewest among them (Peter, 13 Nov, #1576).
pub fn derived_digits(x: f64, sig: usize) -> usize {
    let sig = sig.max(1) as i32;
    let at = |v: f64| {
        if !(v.is_finite() && v != 0.0) {
            return 0;
        }
        (sig - 1 - v.abs().log10().floor() as i32).max(0) as usize
    };
    let d = at(x);
    let rounded: f64 = fixed_text(x, d).parse().unwrap_or(x);
    at(rounded).min(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed and signed figures as a screen writes them, held to the
    /// fixture the MCP view's num and signed are tested against too
    /// (figure_texts.test.cjs, #1431).
    #[test]
    fn screen_figures_match_the_views_fixture() {
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../tests/fixtures/figure_texts.json")).unwrap();
        assert!(!cases.is_empty());
        for c in cases {
            let v = c["v"].as_f64().unwrap();
            let d = c["digits"].as_u64().unwrap() as usize;
            assert_eq!(
                true_minus(&fixed_text(v, d)),
                c["fixed"].as_str().unwrap(),
                "{v} {d}"
            );
            assert_eq!(
                true_minus(&signed_text(v, d)),
                c["signed"].as_str().unwrap(),
                "{v} {d}"
            );
        }
    }

    #[test]
    fn a_screens_minus_is_a_true_minus() {
        assert_eq!(
            true_minus("-1.2 to -0.4 s/d"),
            "\u{2212}1.2 to \u{2212}0.4 s/d"
        );
        assert_eq!(true_minus("+1.2"), "+1.2");
    }

    #[test]
    fn a_sentences_figures_take_a_true_minus_and_its_words_keep_their_hyphens() {
        let m = '\u{2212}';
        assert_eq!(
            figure_minus("Rate shifts level 1 time: -212.6 s/d, then -.5 and (-3 ms)"),
            format!("Rate shifts level 1 time: {m}212.6 s/d, then {m}.5 and ({m}3 ms)")
        );
        assert_eq!(
            figure_minus("2-second stretches, 10-30 min, beat-error, Ljung-Box, a -"),
            "2-second stretches, 10-30 min, beat-error, Ljung-Box, a -"
        );
        assert_eq!(figure_minus("-1.2 s/d"), format!("{m}1.2 s/d"));
    }

    /// The ± at the figure's place, rounded up, never below one unit of
    /// it (#1470): the acceptance cases 0.3 → 1°, 0.04 → 0.1 s/d and
    /// 0.011 → 0.02 ms.
    #[test]
    fn a_standard_error_is_rounded_up_at_its_figures_place() {
        use base_digits as base;
        assert_eq!(pm_text(0.3, base::AMPLITUDE).as_deref(), Some("1"));
        assert_eq!(pm_text(0.04, base::RATE).as_deref(), Some("0.1"));
        assert_eq!(pm_text(0.011, base::BEAT_ERROR).as_deref(), Some("0.02"));
        // Up, not to the nearest.
        assert_eq!(pm_text(1.2, base::AMPLITUDE).as_deref(), Some("2"));
        assert_eq!(pm_text(0.31, base::RATE).as_deref(), Some("0.4"));
        // One already at the place stays there, float error and all.
        assert_eq!(pm_text(0.1, base::RATE).as_deref(), Some("0.1"));
        assert_eq!(pm_text(0.3, base::RATE).as_deref(), Some("0.3"));
        assert_eq!(pm_text(0.07, base::BEAT_ERROR).as_deref(), Some("0.07"));
        assert_eq!(pm_text(2.0, base::AMPLITUDE).as_deref(), Some("2"));
        // Never under one unit, however small.
        assert_eq!(pm_text(1e-9, base::AMPLITUDE).as_deref(), Some("1"));
        assert_eq!(pm_text(0.0001, base::BEAT_ERROR).as_deref(), Some("0.01"));
        assert_eq!(pm_text(25.04, base::RATE).as_deref(), Some("25.1"));
        assert_eq!(pm_text(0.0, 1), None);
        assert_eq!(pm_text(-0.2, 1), None);
        assert_eq!(pm_text(f64::NAN, 1), None);
        assert_eq!(pm_text(f64::INFINITY, 1), None);
    }

    /// A figure is written at its place whatever its ± (#1470): the
    /// amplitude Peter saw as 148.94° ± 0.01° reads 149° ± 1°.
    #[test]
    fn a_figure_is_written_at_its_place_whatever_its_plus_minus() {
        use base_digits as base;
        for se in [None, Some(0.0001), Some(0.04), Some(0.3), Some(25.0)] {
            assert_eq!(figure_digits(se, 2), 2, "{se:?}");
            assert_eq!(figure(1.234, se, base::RATE, true), "+1.2", "{se:?}");
            assert_eq!(figure(285.37, se, base::AMPLITUDE, false), "285");
            assert_eq!(figure(0.1234, se, base::BEAT_ERROR, false), "0.12");
        }
        assert_eq!(
            figure_with_pm(148.94, Some(0.01), base::AMPLITUDE, false),
            "149 ± 1"
        );
        assert_eq!(
            figure_with_pm(11.43, Some(0.04), base::RATE, true),
            "+11.4 ± 0.1"
        );
        assert_eq!(
            figure_with_pm(0.1234, Some(0.011), base::BEAT_ERROR, false),
            "0.12 ± 0.02"
        );
        assert_eq!(figure(-0.04, Some(0.3), base::RATE, true), "0.0");
        assert_eq!(figure(-0.2, None, base::AMPLITUDE, false), "0");
        // No ±: the value alone.
        assert_eq!(figure_with_pm(3.0, None, base::RATE, true), "+3.0");
        assert_eq!(
            figure_with_pm(3.0, Some(f64::NAN), base::RATE, true),
            "+3.0"
        );
    }

    /// A worked-out value keeps its inputs' fewest significant figures
    /// (math guide section 11, #1576).
    #[test]
    fn significant_figures_flow_through() {
        assert_eq!(significant_figures(302.0, 0), 3);
        assert_eq!(significant_figures(0.46, 2), 2);
        assert_eq!(significant_figures(0.4, 2), 2);
        assert_eq!(significant_figures(0.04, 2), 1);
        assert_eq!(significant_figures(0.0, 2), 0);
        assert_eq!(fixed_text(1.746, derived_digits(1.746, 2)), "1.7");
        assert_eq!(fixed_text(0.7634, derived_digits(0.7634, 2)), "0.76");
        assert_eq!(fixed_text(302.4, derived_digits(302.4, 2)), "302");
        assert_eq!(fixed_text(0.996, derived_digits(0.996, 2)), "1.0");
        assert_eq!(fixed_text(1.0, derived_digits(1.0, 2)), "1.0");
    }
}
