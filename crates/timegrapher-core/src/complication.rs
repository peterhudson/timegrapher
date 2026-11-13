//! The complication test: a watch read with a complication off, on, then
//! off again, all in one position, and how much the complication moves
//! its amplitude, rate and beat error (#1576, Hugh's spec, Peter's calls
//! of 13 Nov 2026).
//!
//! Watchmakers read the amplitude with the complication (most often a
//! chronograph) off, then on, and compare. The second Off cancels a
//! steady run-down: a drift that is straight over the run drops out.
//!
//! **On less Off** = On − Off(t_on): the On stretch less the two Offs
//! joined in a straight line, read at the On stretch's middle. With the
//! stretches evenly spaced, as the guided test spaces them, that is
//! On − (Off₁ + Off₂)/2. With the Off weights w₁ = (t₂ − t_on)/(t₂ − t₁) and
//! w₂ = 1 − w₁, and the three stretches measured apart, the errors add in
//! quadrature: ± = √(SE_on² + w₁²·SE_off1² + w₂²·SE_off2²), which is
//! √(SE_on² + (SE_off1² + SE_off2²)/4) when evenly spaced. The joined
//! line is the one the plots draw, so the gap they show is the table's
//! figure even when the watchmaker waited before pressing Start.
//!
//! The line only interpolates: an On stretch outside the Offs gives no
//! figure. Each stretch is read after its settling and none overlaps
//! another, so the three ±s are independent (Norm, 13 Nov 2026).
//!
//! A difference is named (lower, slower, larger) only when it stands clear
//! of zero by [`CLEAR_SE`] of its ±, the guide's 2 SE; otherwise it reads
//! as no clear change, with the figure and ± still given. The difference is
//! written at its inputs' place ([`base_digits`](crate::figure_text::base_digits)): a derived value keeps
//! the significant figures of its inputs (Peter, 13 Nov 2026).

use crate::confidence::{clear_of_zero, CLEAR_SE};
use crate::figure_text::figure_with_pm;
use crate::poise::Term;
use crate::session::Measurement;
use crate::steadiness::{unit_after, SeriesKind};

/// One stretch of the test: the complication off or on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stretch {
    Off,
    On,
}

impl Stretch {
    /// Its name on the table, the buttons and the strip.
    pub fn name(self) -> &'static str {
        match self {
            Stretch::Off => "Off",
            Stretch::On => "On",
        }
    }
}

/// The test's order: Off, On, Off (Peter, 13 Nov 2026).
pub const ORDER: [Stretch; 3] = [Stretch::Off, Stretch::On, Stretch::Off];

/// The table's last row: the On stretch less the two Offs joined at its
/// middle ([`difference`]).
pub const DIFFERENCE_NAME: &str = "On less Off";

/// A figure and its standard error, as one stretch gives it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Figure {
    pub value: f64,
    pub se: Option<f64>,
}

/// One stretch's three figures, each as the guided tests' tables read it:
/// amplitude in degrees, rate in s/d, and the beat error's size in ms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Figures {
    pub amplitude: Option<Figure>,
    pub rate: Option<Figure>,
    pub beat_error: Option<Figure>,
}

/// A stretch's [`Figures`] from its measurement, with the ±s core gives
/// them.
pub fn figures(m: &Measurement) -> Figures {
    let c = m.confidence;
    let with = |value: Option<f64>, se: Option<f64>| value.map(|value| Figure { value, se });
    Figures {
        amplitude: with(m.amplitude_deg, c.and_then(|c| c.amplitude.se)),
        rate: with(m.rate_s_per_day, c.and_then(|c| c.rate.se)),
        beat_error: with(m.beat_error(), c.and_then(|c| c.beat_error.se)),
    }
}

/// The weight of the first Off at the On stretch's middle `t_on`, between
/// the Offs' middles `t1` and `t2`: 1 at `t1`, 0 at `t2`, one half midway.
/// One half when the Offs' middles are the same.
fn first_weight(t1: f64, t2: f64, t_on: f64) -> f64 {
    let span = t2 - t1;
    if span.abs() < f64::EPSILON {
        return 0.5;
    }
    (t2 - t_on) / span
}

/// Where the On stretch would sit with the complication off: the Offs,
/// each at its middle, `t1` and `t2`, joined in a straight line and read
/// at the On stretch's middle `t_on`, with its ±. Evenly spaced, the
/// Offs' mean. None when `t_on` isn't between the Offs: the line is never
/// carried past them.
pub fn off_at(off1: Figure, t1: f64, off2: Figure, t2: f64, t_on: f64) -> Option<Term> {
    if !(t1 <= t_on && t_on <= t2) {
        return None;
    }
    let w1 = first_weight(t1, t2, t_on);
    let w2 = 1.0 - w1;
    let value = w1 * off1.value + w2 * off2.value;
    let se = match (off1.se, off2.se) {
        (Some(s1), Some(s2)) => Some((w1 * w1 * s1 * s1 + w2 * w2 * s2 * s2).sqrt()),
        _ => None,
    };
    Some(Term { value, se })
}

/// On less Off for one figure, from the three stretches in order, each
/// with its middle in seconds: On less the Offs joined at its middle
/// ([`off_at`]), with the ±s in quadrature. The ± is none when any stretch
/// lacks one; the difference is none when any stretch lacks the figure.
pub fn difference(stretches: [(Option<Figure>, f64); 3]) -> Option<Term> {
    let [(a, t1), (b, t_on), (c, t2)] = stretches;
    let (a, b, c) = (a?, b?, c?);
    let off = off_at(a, t1, c, t2, t_on)?;
    let se = match (off.se, b.se) {
        (Some(o), Some(s)) => Some((o * o + s * s).sqrt()),
        _ => None,
    };
    Some(Term {
        value: b.value - off.value,
        se,
    })
}

/// What a run found for one figure: where On would sit with the
/// complication off (the Offs joined at its middle, [`off_at`]), and On
/// less that ([`difference`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Change {
    pub off: Term,
    pub on_less_off: Term,
}

/// A stretch's middle, seconds of the take.
pub fn middle(m: &Measurement) -> f64 {
    (m.start_s + m.end_s) / 2.0
}

/// What the run's three stretches, `stretches` in order Off, On, Off,
/// found for `q`. None until all three are in, or when one lacks the
/// figure or On isn't between the Offs.
pub fn change(q: SeriesKind, stretches: &[Measurement]) -> Option<Change> {
    let [a, b, c] = stretches else {
        return None;
    };
    debug_assert!(
        a.start_s <= a.end_s
            && a.end_s <= b.start_s
            && b.start_s <= b.end_s
            && b.end_s <= c.start_s
            && c.start_s <= c.end_s,
        "the stretches are in order and none overlaps another (Norm)"
    );
    let fig = |m: &Measurement| figure(q, &figures(m));
    let off = off_at(fig(a)?, middle(a), fig(c)?, middle(c), middle(b))?;
    let on_less_off = difference([
        (fig(a), middle(a)),
        (fig(b), middle(b)),
        (fig(c), middle(c)),
    ])?;
    Some(Change { off, on_less_off })
}

/// The three figures, in the order the table and the plots give them:
/// amplitude first, as the drop in it is what a complication test is
/// most often for (Peter, 13 Nov 2026). Each one's name, unit, place and
/// sign are [`SeriesKind`]'s.
pub const SHOWN: [SeriesKind; 3] = [
    SeriesKind::Amplitude,
    SeriesKind::Rate,
    SeriesKind::BeatError,
];

/// This figure of a stretch's [`Figures`].
pub fn figure(q: SeriesKind, f: &Figures) -> Option<Figure> {
    match q {
        SeriesKind::Amplitude => f.amplitude,
        SeriesKind::Rate => f.rate,
        SeriesKind::BeatError => f.beat_error,
    }
}

/// The figure's unit as it follows a number: "°" close up, " s/d" and
/// " ms" spaced ([`unit_after`]).
pub fn unit_text(q: SeriesKind) -> String {
    let (gap, u) = unit_after(q.unit());
    format!("{gap}{u}")
}

/// The figure's unit as a table's heading writes it: "°", "s/d", "ms".
pub fn unit_head(q: SeriesKind) -> &'static str {
    unit_after(q.unit()).1
}

/// A stretch's figure with its ± and unit, as a point's hover gives it:
/// "280 ± 1°", "+4.0 ± 0.3 s/d" (the rate signed).
pub fn figure_text(q: SeriesKind, f: Figure) -> String {
    let text = figure_with_pm(f.value, f.se, q.digits(), q == SeriesKind::Rate);
    format!("{text}{}", unit_text(q))
}

/// The words for a difference below and above zero.
fn ways(q: SeriesKind) -> (&'static str, &'static str) {
    match q {
        SeriesKind::Amplitude => ("lower", "higher"),
        SeriesKind::Rate => ("slower", "faster"),
        SeriesKind::BeatError => ("smaller", "larger"),
    }
}

/// On less Off as the table writes it: signed, at the figure's place, with
/// its ± rounded up: "-5 ± 1" (a surface sets the true minus).
pub fn difference_text(q: SeriesKind, d: Term) -> String {
    figure_with_pm(d.value, d.se, q.digits(), true)
}

/// Whether a difference stands clear of zero by [`CLEAR_SE`] of its ±.
pub fn differs(d: Term) -> bool {
    clear_of_zero(d.value, d.se)
}

/// A plot's finding in words, after its name: "5 ± 1° lower with the
/// complication on" when the difference stands clear of zero, else "no
/// clear change, +0.01 ± 0.02 ms" (Norm: "no change" would overclaim).
/// With no ± to weigh it by, the figure and why there is no verdict:
/// "-10.0 s/d, no ± yet (too few readings)" (Hugh).
pub fn finding_text(q: SeriesKind, d: Term) -> String {
    let unit = unit_text(q);
    if d.se.is_none() {
        return format!("{}{unit}, {NO_PM_YET}", difference_text(q, d));
    }
    if !differs(d) {
        return format!("no clear change, {}{unit}", difference_text(q, d));
    }
    let (less, more) = ways(q);
    let way = if d.value < 0.0 { less } else { more };
    let size = figure_with_pm(d.value.abs(), d.se, q.digits(), false);
    format!("{size}{unit} {way} with the complication on")
}

/// Why a difference with no ± gets no verdict, in Hugh's words: a ± needs
/// [`crate::confidence::MIN_NOISE_VALUES`] values near the stretch, which
/// a short take (the rate's, under about five minutes) doesn't yet hold.
pub const NO_PM_YET: &str = "no ± yet (too few readings)";

/// The method behind the findings, for the pane's Info: a one-line
/// answer, then the ±, then when a change is named, as short paragraphs
/// split by blank lines (Hugh).
pub fn method_note() -> String {
    format!(
        "{DIFFERENCE_NAME} is the On stretch less the mean of the Offs either side.\n\n\
         Strictly, the Offs are joined in a straight line and read at the On stretch's \
         middle; with the stretches evenly spaced, that is their mean. A steady run-down \
         drops out.\n\n\
         Its ± adds the stretches' own ±s as independent errors: \
         √(SE On² + (SE Off₁² + SE Off₂²)/4) when evenly spaced.\n\n\
         A change is named only when it is more than {CLEAR_SE:.0} times its ± from zero; \
         otherwise it reads as no clear change. With no ± yet, the figure is given without \
         a verdict."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(value: f64, se: f64) -> Option<Figure> {
        Some(Figure {
            value,
            se: Some(se),
        })
    }

    /// Hugh's mock's amplitudes, 268, 262 and 265 with 1° each: −4.5, with
    /// ± √(1 + 2/4) ≈ 1.22.
    #[test]
    fn on_less_off_is_on_less_the_offs_mean_with_the_errors_in_quadrature() {
        let d = difference([
            (f(268.0, 1.0), 1.0),
            (f(262.0, 1.0), 2.0),
            (f(265.0, 1.0), 3.0),
        ])
        .unwrap();
        assert!((d.value + 4.5).abs() < 1e-12);
        assert!((d.se.unwrap() - 1.5f64.sqrt()).abs() < 1e-12);
        // A steady drift drops out: 300 and 280 either side of 290.
        let d = difference([
            (f(300.0, 0.5), 1.0),
            (f(290.0, 0.5), 2.0),
            (f(280.0, 0.5), 3.0),
        ])
        .unwrap();
        assert!(d.value.abs() < 1e-12);
    }

    /// The watchmaker waited before the second Off: a steady drift still
    /// drops out, as the plot's joined line shows it, and the nearer Off
    /// weighs more in the ±.
    #[test]
    fn uneven_stretches_join_the_offs_at_the_on_stretchs_middle() {
        // 1 s/d an hour of drift; On at 1 h, the Offs at 0 h and 4 h.
        let d = difference([
            (f(10.0, 0.2), 0.0),
            (f(9.0, 0.2), 3600.0),
            (f(6.0, 0.2), 4.0 * 3600.0),
        ])
        .unwrap();
        assert!(d.value.abs() < 1e-12, "{d:?}");
        let (w1, w2) = (0.75, 0.25);
        let se = (0.04 + w1 * w1 * 0.04 + w2 * w2 * 0.04_f64).sqrt();
        assert!((d.se.unwrap() - se).abs() < 1e-12);
    }

    /// The line is never carried past the Offs (Norm).
    #[test]
    fn an_on_stretch_outside_the_offs_gives_no_figure() {
        let (a, b) = (f(268.0, 1.0).unwrap(), f(264.0, 1.0).unwrap());
        assert_eq!(off_at(a, 0.0, b, 2.0, 3.0), None);
        assert_eq!(off_at(a, 1.0, b, 2.0, 0.5), None);
        assert_eq!(
            difference([(Some(a), 2.0), (f(260.0, 1.0), 1.0), (Some(b), 3.0)]),
            None
        );
    }

    #[test]
    fn a_missing_figure_gives_no_difference_and_a_missing_se_no_pm() {
        assert_eq!(
            difference([(None, 1.0), (f(1.0, 0.1), 2.0), (f(1.0, 0.1), 3.0)]),
            None
        );
        let no_se = Some(Figure {
            value: 1.0,
            se: None,
        });
        let d = difference([(f(1.0, 0.1), 1.0), (no_se, 2.0), (f(1.0, 0.1), 3.0)]).unwrap();
        assert_eq!(d.se, None);
        assert!(!differs(d), "no ± is never named");
    }

    #[test]
    fn a_clear_difference_is_named_and_a_small_one_reads_as_no_change() {
        let amp = Term {
            value: -5.0,
            se: Some(1.0),
        };
        assert_eq!(
            finding_text(SeriesKind::Amplitude, amp),
            "5 ± 1° lower with the complication on"
        );
        let rate = Term {
            value: -1.9,
            se: Some(0.5),
        };
        assert_eq!(
            finding_text(SeriesKind::Rate, rate),
            "1.9 ± 0.5 s/d slower with the complication on"
        );
        let be = Term {
            value: 0.01,
            se: Some(0.02),
        };
        assert_eq!(
            finding_text(SeriesKind::BeatError, be),
            "no clear change, +0.01 ± 0.02 ms"
        );
        let up = Term {
            value: 0.3,
            se: Some(0.1),
        };
        assert_eq!(
            finding_text(SeriesKind::BeatError, up),
            "0.30 ± 0.10 ms larger with the complication on"
        );
        assert_eq!(difference_text(SeriesKind::Amplitude, amp), "-5 ± 1");
    }

    /// With no ± the finding gives the figure and why there is no
    /// verdict, never "no clear change" (Hugh).
    #[test]
    fn a_difference_with_no_pm_gets_no_verdict() {
        let rate = Term {
            value: -10.0,
            se: None,
        };
        let text = finding_text(SeriesKind::Rate, rate);
        assert_eq!(text, "-10.0 s/d, no ± yet (too few readings)");
        assert!(!text.contains("change"));
    }

    /// The Info note says when a change is named in words a reader can
    /// parse, with the number from [`CLEAR_SE`] (Norm).
    #[test]
    fn the_method_note_states_the_rule_plainly() {
        let note = method_note();
        assert!(note.contains("more than 2 times its ± from zero"), "{note}");
        assert!(note.split("\n\n").count() >= 3, "short paragraphs");
    }

    /// Exactly 2 SE is not clear: the guide's gate is strict.
    #[test]
    fn two_se_is_the_edge() {
        let at = Term {
            value: -2.0,
            se: Some(1.0),
        };
        assert!(!differs(at));
        let past = Term {
            value: -2.01,
            se: Some(1.0),
        };
        assert!(differs(past));
    }

    #[test]
    fn the_offs_joined_at_the_on_stretchs_middle() {
        let (a, b) = (f(268.0, 1.0).unwrap(), f(264.0, 1.0).unwrap());
        assert!((off_at(a, 0.0, b, 2.0, 1.0).unwrap().value - 266.0).abs() < 1e-12);
        assert!((off_at(a, 0.0, b, 4.0, 1.0).unwrap().value - 267.0).abs() < 1e-12);
        assert!((off_at(a, 1.0, b, 1.0, 1.0).unwrap().value - 266.0).abs() < 1e-12);
    }
}
