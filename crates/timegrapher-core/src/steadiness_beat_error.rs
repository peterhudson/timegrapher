use super::*;
use crate::session::BeatErrorSelection;

/// Length of each stretch in the wind-down comparison, seconds.
const WIND_STRETCH_S: f64 = 1800.0;

fn stretch(
    rate: &Grid,
    amp: &Grid,
    be: &Grid,
    from_s: f64,
    cfg: &Config,
    selected: &BeatErrorSelection,
) -> Stretch {
    let to_s = from_s + WIND_STRETCH_S;
    let med = |g: &Grid| {
        let mut v: Vec<f64> = (0..g.y.len())
            .filter(|&i| g.time(i) >= from_s && g.time(i) < to_s && g.y[i].is_finite())
            .map(|i| g.y[i])
            .collect();
        (v.len() >= 30).then(|| median(&mut v))
    };
    Stretch {
        beat_error_source: selected.beat_error_source,
        beat_error_sources: selected.beat_error_sources,
        caveats: selected.caveats(),
        from_h: from_s / 3600.0,
        to_h: to_s / 3600.0,
        from_wind_h: super::hours_since_wind(cfg.wind_h, from_s / 3600.0),
        to_wind_h: super::hours_since_wind(cfg.wind_h, to_s / 3600.0),
        readings: (0..rate.y.len())
            .filter(|&i| rate.time(i) >= from_s && rate.time(i) < to_s && rate.y[i].is_finite())
            .count(),
        rate_s_per_day: med(rate),
        amplitude_deg: med(amp),
        beat_error_ms: med(be),
    }
}

/// The first half hour against the half hour 24 h after full wind, or the
/// last half hour when the take ends sooner or its wind was not given
/// (#1665). Needs a take of at least two hours.
pub(super) fn wind_down_sourced(
    rate: &Grid,
    amp: &Grid,
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[crate::breaks::Break],
    cfg: &Config,
) -> Option<WindDown> {
    wind_down_by(rate, cfg, false, |from| {
        let (fine, selected) = grid(log, clock, breaks, Some((from, from + WIND_STRETCH_S)));
        let be = rebin(&fine, rate.t0, rate.step, rate.y.len());
        stretch(rate, amp, &be, from, cfg, &selected)
    })
}

#[cfg(test)]
pub(super) fn wind_down(rate: &Grid, amp: &Grid, be: &Grid, cfg: &Config) -> Option<WindDown> {
    let unknown = crate::session::select_beat_errors([]);
    wind_down_by(rate, cfg, true, |from| {
        stretch(rate, amp, be, from, cfg, &unknown)
    })
}

fn wind_down_by(
    rate: &Grid,
    cfg: &Config,
    legacy: bool,
    read: impl Fn(f64) -> Stretch,
) -> Option<WindDown> {
    if rate.y.is_empty() {
        return None;
    }
    let t_end = rate.t0 + rate.span();
    let t_start = rate.t0;
    if t_end - t_start < 2.0 * 3600.0 {
        return None;
    }
    let at24 = cfg.wind_h.map(|w| t_start + (24.0 - w) * 3600.0);
    let (end_from, at_24h) = match at24 {
        Some(at24) if at24 > t_start + WIND_STRETCH_S && at24 + WIND_STRETCH_S <= t_end => {
            (at24, true)
        }
        _ => (t_end - WIND_STRETCH_S, false),
    };
    let start = read(t_start);
    let end = read(end_from);
    let selected = crate::session::select_beat_errors([
        start.beat_error_ms.zip(start.beat_error_source),
        end.beat_error_ms.zip(end.beat_error_source),
    ]);
    let diff = |a: Option<f64>, b: Option<f64>| Some(b? - a?);
    let rate_change = diff(start.rate_s_per_day, end.rate_s_per_day);
    let amplitude_change = diff(start.amplitude_deg, end.amplitude_deg);
    let beat_error_change = if legacy {
        diff(start.beat_error_ms, end.beat_error_ms)
    } else {
        (selected.values.len() == 2).then(|| selected.values[1].1 - selected.values[0].1)
    };
    let hours_apart = (end_from - t_start) / 3600.0;
    // Hours since full wind only when the wind was given (#1665).
    let span = super::wind::from_to_words(start.from_h, end.from_h, cfg.wind_h);
    let headline = match (rate_change, amplitude_change) {
        (Some(r), Some(a)) => format!(
            "{span} the rate moves {:+.1} s/d ({:+.1} to {:+.1}) and the amplitude {:+.0}° ({:.0}° to {:.0}°){}.",
            r,
            start.rate_s_per_day.unwrap_or(f64::NAN),
            end.rate_s_per_day.unwrap_or(f64::NAN),
            a,
            start.amplitude_deg.unwrap_or(f64::NAN),
            end.amplitude_deg.unwrap_or(f64::NAN),
            beat_error_change.map_or(String::new(), |b| format!(", beat error {b:+.2} ms"))
        ),
        _ => "Too few readings at the start or the end to compare them.".to_string(),
    };
    Some(WindDown {
        beat_error_source: selected.beat_error_source,
        beat_error_sources: selected.beat_error_sources,
        caveats: selected.caveats(),
        start,
        end,
        hours_apart,
        at_24h,
        rate_change_s_per_day: rate_change,
        amplitude_change_deg: amplitude_change,
        beat_error_change_ms: beat_error_change,
        headline,
    })
}

/// Source selection is over eligible fine readings, before any coarser statistic.
/// The original grid remains intact: values from the other source become holes.
pub(super) fn grid(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[crate::breaks::Break],
    span: Option<(f64, f64)>,
) -> (Grid, BeatErrorSelection) {
    let cuts = Cuts::new(clock, breaks, &[]);
    let in_span = |w: &crate::amplitude::AmplitudeWindow| {
        span.is_none_or(|(from, to)| cuts.window_in(w, from, to))
    };
    let selected = crate::session::select_window_beat_errors(
        cuts.amplitude_windows(&log.amplitude_windows)
            .into_iter()
            .filter(|w| in_span(w)),
    );
    let turned = crate::longrun::beat_error_turns(log, clock, breaks);
    let grid = windows_grid(log, clock, breaks, &|w| {
        if !in_span(w) {
            return None;
        }
        let (value, source) =
            crate::session::chosen_beat_error(w.beat_error_unlock_ms, w.beat_error_ms)?;
        (Some(source) == selected.beat_error_source).then_some(if turned {
            value.abs()
        } else {
            value
        })
    });
    (grid, selected)
}

pub(super) fn set_provenance(series: &mut [SeriesCheck], selected: &BeatErrorSelection) {
    if let Some(series) = series
        .iter_mut()
        .find(|s| s.series == SeriesKind::BeatError)
    {
        series.beat_error_source = selected.beat_error_source;
        series.beat_error_sources = selected.beat_error_sources;
        series.caveats = selected.caveats();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{BeatErrorSource, BeatErrorSources};
    fn log(n: usize) -> BeatLog {
        use crate::amplitude::AmplitudeWindow;
        BeatLog {
            lock_template_kept: false,
            sample_rate: 48000,
            duration_s: n as f64 * 2.0,
            bph: 28800,
            lift_deg: 52.0,
            beats: vec![],
            clipped_beats: vec![],
            clipped_samples: 0,
            jumps: vec![],
            sided: vec![],
            by_count: vec![],
            snippets: vec![],
            amplitude_windows: (0..n)
                .map(|i| AmplitudeWindow {
                    start_s: i as f64 * 2.0,
                    end_s: (i + 1) as f64 * 2.0,
                    even_deg: Some(270.0),
                    odd_deg: Some(270.0),
                    beat_error_ms: Some(8.0),
                    beat_error_unlock_ms: (i < n / 2).then_some(0.4),
                    even_unlock_ms: None,
                    odd_unlock_ms: None,
                    even_drop_ms: None,
                    odd_drop_ms: None,
                    culled_ms: None,
                    faint_withheld: [false; 2],
                })
                .collect(),
        }
    }
    #[test]
    fn series_source_selection_leaves_other_source_as_holes() {
        let log = log(10);
        let (g, selected) = grid(&log, None, &[], None);
        assert_eq!(
            selected.beat_error_sources,
            BeatErrorSources { unlock: 5, drop: 5 }
        );
        assert!(g.y[..5].iter().all(|&v| v == 0.4));
        assert!(g.y[5..].iter().all(|v| v.is_nan()));
        // The steadiness figures count the amplitude's windows, and say so.
        assert!(matches!(
            selected.caveats().as_slice(),
            [crate::measure::Caveat::BeatErrorSourceReadings {
                used: 5,
                readings: 10,
                counted: crate::measure::Counted::Windows,
                ..
            }]
        ));
        let (g, selected) = grid(&log, None, &[], Some((10.0, 20.0)));
        assert_eq!(selected.beat_error_source, Some(BeatErrorSource::Drop));
        assert!(g.y[5..].iter().all(|&v| v == 8.0));
        assert!(matches!(
            selected.caveats().as_slice(),
            [crate::measure::Caveat::FROM_DROP]
        ));
    }
    #[test]
    fn coarse_beat_error_reading_counts_only_its_chosen_source() {
        let log = log(4);
        let (fine, _) = grid(&log, None, &[], None);
        assert!(rebin(&fine, 0.0, 8.0, 1).y[0].is_nan());
    }
    #[test]
    fn wind_down_never_subtracts_different_sources() {
        let log = log(3600);
        let rate = Grid {
            t0: 0.0,
            step: 10.0,
            y: vec![1.0; 720],
        };
        let amp = Grid {
            t0: 0.0,
            step: 10.0,
            y: vec![270.0; 720],
        };
        let wind = wind_down_sourced(&rate, &amp, &log, None, &[], &Config::default()).unwrap();
        assert_eq!(wind.start.beat_error_ms, Some(0.4));
        assert_eq!(wind.end.beat_error_ms, Some(8.0));
        assert_eq!(wind.beat_error_change_ms, None);
        assert_eq!(
            wind.beat_error_sources,
            BeatErrorSources { unlock: 1, drop: 1 }
        );
    }
    #[test]
    fn selected_source_median_uncertainty_covers_mixed_and_gapped_readings() {
        let mut rng = crate::synth::Rng::new(658_2026);
        for gapped in [false, true] {
            let mut covered = 0;
            for _ in 0..600 {
                let mut log = log(120);
                for (i, w) in log.amplitude_windows.iter_mut().enumerate() {
                    if i < 60 {
                        w.beat_error_unlock_ms = Some(0.4 + 0.05 * rng.normal());
                    }
                    if gapped && (25..35).contains(&i) {
                        w.beat_error_unlock_ms = None;
                        w.beat_error_ms = None;
                    }
                }
                let (g, _) = grid(&log, None, &[], None);
                let stretches: Vec<_> =
                    g.y.split(|v| !v.is_finite())
                        .filter(|s| !s.is_empty())
                        .collect();
                let se = crate::confidence::standard_error_in(stretches).unwrap();
                let mut values: Vec<_> = g.y.iter().copied().filter(|v| v.is_finite()).collect();
                let value = crate::dsp::median(&mut values);
                covered += usize::from((value - 0.4).abs() <= 1.96 * se);
            }
            eprintln!("mixed beat-error 95% interval, gapped={gapped}: {covered}/600");
            assert!((540..=594).contains(&covered), "{gapped}: {covered}/600");
        }
    }
}
