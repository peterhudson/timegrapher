//! The pane's tests as one family (#659): every p-value that can name
//! something on the pane, each series' period search lines and its
//! Ljung–Box test, its trend's F test and its CUSUM (#917), is held to
//! Holm's correction at [`FAMILY_ALPHA`], so a steady watch shows
//! nothing in at least 95 takes in 100. Tests with no p-value (the
//! two-state finder's BIC, a fold's share) keep their own bars; the
//! change finder's penalty only places the cuts. Each test's own figures
//! stay as they are: only what is named changes.

use super::{
    check_series, Changes, Config, Grid, Lead, Period, SeriesCheck, SeriesKind, TwoState, Wheel,
};
use crate::clock::ClockFit;
use crate::longrun::LongReport;
use crate::longterm::LongConfig;
use crate::stream::BeatLog;

/// The trend's place in a series' tests, beside the Ljung–Box test
/// (`None`) and the search lines (`Some(line)`, `usize::MAX` for none).
const TREND: usize = usize::MAX - 1;
/// The level change's (the CUSUM's) place.
const LEVEL: usize = usize::MAX - 2;

/// The chance, over the whole pane, of naming anything on a steady
/// watch.
pub const FAMILY_ALPHA: f64 = 0.05;

/// One series' inputs to [`check_series`]: its kind, readings, short
/// windows, period search, the significance (−log10 of the false-alarm
/// chance) of each line the search found, strongest first, and its
/// two-state finding.
pub(super) type Input = (
    SeriesKind,
    Grid,
    Option<Grid>,
    Option<Period>,
    Vec<f64>,
    Option<TwoState>,
);

/// Holm's step-down: which of `p` are kept at family level `alpha`.
pub fn holm(p: &[f64], alpha: f64) -> Vec<bool> {
    let mut order: Vec<usize> = (0..p.len()).collect();
    order.sort_by(|&a, &b| p[a].total_cmp(&p[b]));
    let mut kept = vec![false; p.len()];
    for (k, &i) in order.iter().enumerate() {
        if p[i] > alpha / (p.len() - k) as f64 {
            break;
        }
        kept[i] = true;
    }
    kept
}

/// Check each series, naming only what its own test and Holm's
/// correction across all of them both keep. A series whose period search
/// found nothing still counts as one test, with p 1. With the checks,
/// which of each series' search lines the pane names, in the inputs'
/// order: the one kept set every list of cycles reads ([`hold_to_family`]).
pub(super) fn check_all(
    inputs: Vec<Input>,
    wheels: &[Wheel],
    cfg: &Config,
) -> (Vec<SeriesCheck>, Vec<Vec<bool>>) {
    // Each series is checked on its own, on every core (#1839); only the
    // family below reads them together.
    let first: Vec<SeriesCheck> = crate::cores::on_every_core(&inputs, |(k, g, f, q, _, t)| {
        check_series(*k, g.clone(), f.clone(), q.clone(), t.clone(), wheels, cfg)
    });
    // One entry per test: (series, None for Ljung–Box or Some(line)).
    let mut tests = Vec::new();
    let mut p = Vec::new();
    for (k, (i, s)) in inputs.iter().zip(&first).enumerate() {
        tests.push((k, None));
        p.push(s.autocorrelation.as_ref().map_or(1.0, |a| a.p_value));
        tests.push((k, Some(TREND)));
        p.push(s.trend.as_ref().map_or(1.0, |t| t.p_value));
        tests.push((k, Some(LEVEL)));
        p.push(s.changes.as_ref().map_or(1.0, |c| c.p_value));
        if i.4.is_empty() {
            tests.push((k, Some(usize::MAX)));
            p.push(1.0);
        }
        for (j, &sig) in i.4.iter().enumerate() {
            tests.push((k, Some(j)));
            p.push(10f64.powf(-sig));
        }
    }
    let kept = holm(&p, FAMILY_ALPHA);
    let keeps =
        |k: usize, line: Option<usize>| tests.iter().zip(&kept).any(|(t, &x)| *t == (k, line) && x);
    // The strongest line gone, the search names none.
    let named: Vec<Vec<bool>> = inputs
        .iter()
        .enumerate()
        .map(|(k, i)| {
            let lines: Vec<bool> = (0..i.4.len()).map(|j| keeps(k, Some(j))).collect();
            let any = lines.first() == Some(&true);
            lines.into_iter().map(|x| x && any).collect()
        })
        .collect();
    let checks = inputs
        .into_iter()
        .zip(first)
        .enumerate()
        .map(|(k, ((kind, grid, fine, period, sig, two_state), s))| {
            let lines = &named[k];
            let memory = keeps(k, None);
            let trend = keeps(k, Some(TREND));
            let level = keeps(k, Some(LEVEL));
            if lines.len() == sig.len()
                && lines.iter().all(|&x| x)
                && (memory || !named_memory(&s, cfg))
                && (trend || !trend_holds(&s, cfg))
                && (level || !level_holds(&s, cfg))
            {
                return s;
            }
            let period = period
                .filter(|_| lines.first() == Some(&true))
                .map(|mut q| {
                    let keep = &lines[1..];
                    q.other_periods_s = pick(&q.other_periods_s, keep);
                    q.other_harmonic_of = pick(&q.other_harmonic_of, keep);
                    q
                });
            let cfg = Config {
                alpha: if memory { cfg.alpha } else { 0.0 },
                // 0 only for a trend Holm drops after its own test kept
                // it: the bar a dropped trend's note tells apart.
                trend_alpha: if trend || !trend_holds(&s, cfg) {
                    cfg.trend_alpha
                } else {
                    0.0
                },
                level_alpha: if level || !level_holds(&s, cfg) {
                    cfg.level_alpha
                } else {
                    0.0
                },
                ..cfg.clone()
            };
            check_series(kind, grid, fine, period, two_state, wheels, &cfg)
        })
        .collect();
    (checks, named)
}

/// Keep in `long` only the period-search lines `rep` names, so the CLI's
/// `long` text, its report and summary, and a session's cycles list what
/// the Steadiness pane names (#659): Holm's correction decides once, here.
pub fn hold_to_family(long: &mut LongReport, rep: &super::Report) {
    let keep = |i: usize| rep.named_lines.get(i).cloned().unwrap_or_default();
    let (rate, amp) = (keep(0), keep(1));
    let mut k = rate.iter();
    long.rate_components.retain(|_| *k.next().unwrap_or(&false));
    let mut k = rate.iter();
    long.timing
        .components
        .retain(|_| *k.next().unwrap_or(&false));
    let mut k = amp.iter();
    long.amplitude
        .components
        .retain(|_| *k.next().unwrap_or(&false));
}

/// [`super::check`] `long` at the default settings and [`hold_to_family`]
/// it: for a list of cycles read without the pane.
pub fn family_long(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    long: &mut LongReport,
    long_cfg: &LongConfig,
) {
    let rep = super::check(log, clock, long, long_cfg, &Config::default());
    hold_to_family(long, &rep);
}

/// The change finder's stretches when they say the level moves: cut,
/// the CUSUM's test kept (#917), and explaining at least `min_explained`
/// of the variance.
pub(super) fn level_moves<'a>(s: &'a SeriesCheck, cfg: &Config) -> Option<&'a Changes> {
    s.changes
        .as_ref()
        .filter(|c| c.segments.len() > 1 && c.explained >= cfg.min_explained)
        .filter(|_| level_holds(s, cfg))
}

/// The one rule that names a level change, read by the headline and the
/// CUSUM's corner alike (#697): the level moves and the lead is not a
/// slide, which a slide cut into steps is.
pub(super) fn level_change<'a>(s: &'a SeriesCheck, cfg: &Config) -> Option<&'a Changes> {
    level_moves(s, cfg).filter(|_| s.lead != Lead::Drifting)
}

/// Whether the CUSUM's test clears `cfg.level_alpha` (#917).
pub(super) fn level_holds(s: &SeriesCheck, cfg: &Config) -> bool {
    s.changes
        .as_ref()
        .is_some_and(|c| c.p_value < cfg.level_alpha)
}

/// Why a test that `bar` held to did not pass: its own bar, or (a bar
/// of 0) Holm's correction dropping it after its own bar kept it.
fn not_significant(p: f64, bar: f64, what: &str) -> String {
    let p = super::p_text(p);
    if bar == 0.0 {
        format!("not significant after Holm's correction (p {p} alone)")
    } else {
        format!("not significant: p {p}, over the {bar} a {what} needs")
    }
}

/// Mark whether the trend passed its test and which of the trend and the
/// levels the headline names, so a detail row the headline leaves out
/// can say so.
pub(super) fn mark_named(s: &mut SeriesCheck, cfg: &Config) {
    let significant = trend_holds(s, cfg);
    let drifting = s.lead == Lead::Drifting;
    let levels = level_change(s, cfg).is_some() || (drifting && level_moves(s, cfg).is_some());
    let level_ok = level_holds(s, cfg);
    let bar = |x: f64, what: &str| {
        (x < cfg.min_explained)
            .then(|| format!("under the {:.0}% a {what} needs", cfg.min_explained * 100.0))
    };
    let other = || "not the headline's finding".to_string();
    if let Some(t) = s.trend.as_mut() {
        t.significant = significant;
        t.named = drifting;
        t.note = (!drifting).then(|| {
            let p = (!significant).then(|| not_significant(t.p_value, cfg.trend_alpha, "trend"));
            p.or_else(|| bar(t.explained.max(t.curve_explained), "trend"))
                .unwrap_or_else(other)
        });
    }
    if let Some(c) = s.changes.as_mut() {
        c.named = levels;
        c.note = (!levels).then(|| {
            let p =
                (!level_ok).then(|| not_significant(c.p_value, cfg.level_alpha, "level change"));
            p.or_else(|| bar(c.explained, "level change"))
                .unwrap_or_else(other)
        });
    }
}

/// Whether the trend's own test clears `cfg.trend_alpha` (#659).
pub(super) fn trend_holds(s: &SeriesCheck, cfg: &Config) -> bool {
    s.trend
        .as_ref()
        .is_some_and(|t| t.p_value < cfg.trend_alpha)
}

/// [`super::Trend::p_value`] for readings `y` (NaN a gap) whose quadratic
/// explains `r2` of their variance.
pub(super) fn trend_p(y: &[f64], r2: f64) -> f64 {
    let n = y.iter().filter(|v| v.is_finite()).count() as f64;
    let rho = crate::confidence::long_run_rho(y, crate::confidence::Slow::Line);
    let n_eff = n * (1.0 - rho) / (1.0 + rho);
    if n_eff <= 3.0 || !r2.is_finite() {
        return 1.0;
    }
    (1.0 - r2.clamp(0.0, 1.0)).powf((n_eff - 3.0) / 2.0)
}

/// Whether the Ljung–Box test at the series' own α names memory.
fn named_memory(s: &SeriesCheck, cfg: &Config) -> bool {
    s.autocorrelation
        .as_ref()
        .is_some_and(|a| a.p_value < cfg.alpha)
}

fn pick<T: Clone>(v: &[T], keep: &[bool]) -> Vec<T> {
    v.iter()
        .zip(keep)
        .filter(|(_, &k)| k)
        .map(|(x, _)| x.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The trend's F test with two terms is (1 − R²)^((n_eff − 3)/2):
    /// on independent readings n_eff is their count, and readings that
    /// follow themselves count for fewer, so the same R² is likelier.
    #[test]
    fn a_trend_s_p_counts_readings_that_follow_themselves_as_fewer() {
        let mut z = crate::synth::Rng::new(5);
        let white: Vec<f64> = (0..360).map(|_| z.normal()).collect();
        let rho = crate::confidence::long_run_rho(&white, crate::confidence::Slow::Line);
        let n_eff = 360.0 * (1.0 - rho) / (1.0 + rho);
        let p = super::trend_p(&white, 0.05);
        assert!((p - 0.95f64.powf((n_eff - 3.0) / 2.0)).abs() < 1e-12, "{p}");
        let mut x = 0.0;
        let red: Vec<f64> = (0..360)
            .map(|_| {
                x = 0.9 * x + z.normal();
                x
            })
            .collect();
        assert!(super::trend_p(&red, 0.05) > 100.0 * p);
        // Three readings and fewer leave nothing to test.
        assert_eq!(super::trend_p(&[1.0, 2.0, 3.0], 0.9), 1.0);
    }

    /// A trend or levels the headline leaves out say so, and a trend
    /// says whether its test passed, so a detail row cannot read as a
    /// finding the headline denies.
    #[test]
    fn a_dropped_trend_or_level_is_marked_unnamed() {
        let mut z = crate::synth::Rng::new(3);
        let run = |y: Vec<f64>, cfg: &Config| {
            let g = Grid {
                t0: 0.0,
                step: 10.0,
                y,
            };
            check_series(SeriesKind::Amplitude, g, None, None, None, &[], cfg)
        };
        let slide: Vec<f64> = (0..720)
            .map(|i| 280.0 - 20.0 * i as f64 / 720.0 + 2.0 * z.normal())
            .collect();
        let on = run(slide.clone(), &Config::default());
        let t = on.trend.as_ref().unwrap();
        assert!(t.significant && t.named, "{}", on.headline);
        // Holm dropping the trend sets its bar to 0.
        let dropped = Config {
            trend_alpha: 0.0,
            ..Config::default()
        };
        let off = run(slide, &dropped);
        let t = off.trend.as_ref().unwrap();
        assert!(!t.significant && !t.named, "{}", off.headline);
        let note = t.note.as_deref().unwrap();
        assert!(
            note.starts_with("not significant after Holm's correction (p "),
            "{note}"
        );
        // Short of its own bar, the note gives the bar.
        let white: Vec<f64> = (0..720).map(|_| 280.0 + 2.0 * z.normal()).collect();
        let w = run(white, &Config::default());
        let note = w.trend.unwrap().note.unwrap();
        assert!(note.starts_with("not significant: p "), "{note}");
        assert!(note.ends_with(", over the 0.01 a trend needs"), "{note}");
        assert_eq!(on.trend.as_ref().unwrap().note, None);
        let step: Vec<f64> = (0..720)
            .map(|i| if i < 360 { 270.0 } else { 280.0 } + 2.0 * z.normal())
            .collect();
        let s = run(step.clone(), &Config::default());
        assert!(s.changes.as_ref().unwrap().named, "{}", s.headline);
        let bar = Config {
            min_explained: 0.99,
            ..Config::default()
        };
        let s = run(step, &bar);
        let c = s.changes.as_ref().unwrap();
        assert!(!c.named, "{}", s.headline);
        assert_eq!(
            c.note.as_deref(),
            Some("under the 99% a level change needs")
        );
    }

    /// A level change is named only when the CUSUM's test passes as well
    /// as the split's share (#917): the same step with the bar out of
    /// reach is left out, and its row says which test it missed.
    #[test]
    fn a_level_change_needs_its_cusum_test() {
        let mut z = crate::synth::Rng::new(6);
        let step: Vec<f64> = (0..720)
            .map(|i| if i < 360 { 270.0 } else { 274.0 } + 2.0 * z.normal())
            .collect();
        let run = |cfg: &Config| {
            let g = Grid {
                t0: 0.0,
                step: 10.0,
                y: step.clone(),
            };
            check_series(SeriesKind::Amplitude, g, None, None, None, &[], cfg)
        };
        let s = run(&Config::default());
        assert_eq!(s.lead, Lead::ShiftingMean, "{}", s.headline);
        let c = s.changes.as_ref().unwrap();
        assert!(c.p_value > 0.0 && c.named, "{}", c.p_value);
        // The bar just under the step's own p.
        let strict = Config {
            level_alpha: c.p_value / 2.0,
            ..Config::default()
        };
        let s = run(&strict);
        assert_ne!(s.lead, Lead::ShiftingMean, "{}", s.headline);
        let note = s.changes.unwrap().note.unwrap();
        assert!(note.starts_with("not significant: p "), "{note}");
        assert!(note.ends_with(" a level change needs"), "{note}");
    }

    /// A steady 30 min take whose readings follow themselves (ρ 0.9)
    /// names no level change: seed 1 of the #659 grid, where 7 in 100
    /// still do (#917).
    #[test]
    fn a_steady_red_take_names_no_level_change() {
        use crate::longterm::LongConfig;
        let lc = LongConfig {
            wheels: crate::periodicity::standard_wheels(28_800, 20),
            ..Default::default()
        };
        let sc = crate::scenarios::steady_red(1800.0, 0.9, 1);
        let log = crate::scenarios::beat_log(&sc, 1);
        let long = crate::longrun::analyse(&log, None, &lc);
        let r = super::super::check(&log, None, &long, &lc, &Config::default());
        assert!(r.findings.iter().all(|f| f.code != "shifting_mean"));
    }

    /// A series checked again because Holm dropped its period line keeps
    /// its trend's own bar when the trend failed that bar, so its note
    /// does not blame Holm.
    #[test]
    fn a_rerun_keeps_a_failed_trend_s_own_bar() {
        let mut z = crate::synth::Rng::new(4);
        let white: Vec<f64> = (0..720).map(|_| 280.0 + 2.0 * z.normal()).collect();
        let g = Grid {
            t0: 0.0,
            step: 10.0,
            y: white,
        };
        // A search line at p 0.1 that Holm drops, so the series is re-run.
        let input = (SeriesKind::Amplitude, g, None, None, vec![1.0], None);
        let (checks, named) = check_all(vec![input], &[], &Config::default());
        assert_eq!(named, [vec![false]]);
        let note = checks[0].trend.as_ref().unwrap().note.clone().unwrap();
        assert!(note.ends_with(", over the 0.01 a trend needs"), "{note}");
    }

    /// Holm keeps the smallest p while each is under α over the tests
    /// left, and stops at the first that is not.
    #[test]
    fn holm_steps_down_and_stops() {
        let kept = holm(&[0.04, 0.001, 0.015, 0.5], 0.05);
        assert_eq!(kept, [false, true, true, false]);
        // 0.0125 is the bar for the smallest of four; just over it, none.
        assert_eq!(holm(&[0.013, 0.013, 0.9, 0.9], 0.05), [false; 4]);
        // Under it, the next is held to α/3, 0.0167.
        let next = |q| holm(&[0.012, q, 0.9, 0.9], 0.05);
        assert_eq!(next(0.016), [true, true, false, false]);
        assert_eq!(next(0.017), [true, false, false, false]);
    }
}
