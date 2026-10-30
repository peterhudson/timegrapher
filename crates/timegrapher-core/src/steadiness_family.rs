//! The pane's tests as one family (#659): every p-value that can name
//! something on the pane, each series' period search lines and its
//! Ljung–Box test, is held to Holm's correction at [`FAMILY_ALPHA`], so
//! a steady watch shows nothing in at least 95 takes in 100. Tests with
//! no p-value (the change finder's penalty, the two-state finder's BIC,
//! a fold's share) keep their own bars, and each test's own figures stay
//! as they are: only what is named changes.

use super::{check_series, Config, Grid, Period, SeriesCheck, SeriesKind, TwoState, Wheel};
use crate::clock::ClockFit;
use crate::longrun::LongReport;
use crate::longterm::LongConfig;
use crate::stream::BeatLog;

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
    let first: Vec<SeriesCheck> = inputs
        .iter()
        .map(|(k, g, f, q, _, t)| {
            check_series(*k, g.clone(), f.clone(), q.clone(), t.clone(), wheels, cfg)
        })
        .collect();
    // One entry per test: (series, None for Ljung–Box or Some(line)).
    let mut tests = Vec::new();
    let mut p = Vec::new();
    for (k, (i, s)) in inputs.iter().zip(&first).enumerate() {
        tests.push((k, None));
        p.push(s.autocorrelation.as_ref().map_or(1.0, |a| a.p_value));
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
            if lines.len() == sig.len()
                && lines.iter().all(|&x| x)
                && (memory || !named_memory(&s, cfg))
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
