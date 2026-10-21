//! What was measured over a stretch of beats beyond the readings
//! themselves: the ± of each reading and the conditions detected, decided
//! in one place for every reading of a recording (`analyze`, `session`,
//! the live screen and `long`'s slices).
//!
//! The readings themselves come from the one beat pipeline
//! ([`crate::stream`]); this module only states what they rest on. Each
//! caveat names a condition detected, with its figure; none of them
//! changes a reading or its ±, except `no_watch`: when no regular beat is
//! found, rate, amplitude and beat error are not given, since they would
//! describe noise.
//!
//! Whether the beats are regular is assessed on 10 s windows, so with
//! less than [`SHORT_SPAN_S`] of sound it is not assessed: the readings
//! are given, `watch_heard` stays true, and `short_span` says how short
//! the stretch is (see [`watch_heard`]).
//!
//! The beat error's sign: positive means Tick, the first beat of the run,
//! comes late. Tick is whichever beat a reading locks on first, so the sign
//! holds within one run only; text shows the size.

use crate::amplitude::{standard_error, AmplitudeWindow};
use crate::confidence::{self, Confidence, Doubts};
use crate::timing::WindowFit;
use serde::Serialize;

/// Seconds of sound needed to assess whether the beats are regular, and
/// below which a stretch of beats is named `short_span`.
pub const SHORT_SPAN_S: f64 = 10.0;

/// Whether `listened_s` seconds of sound are enough to assess whether the
/// beats are regular (see [`SHORT_SPAN_S`]).
pub fn regularity_assessed(listened_s: f64) -> bool {
    // A tolerance, so a 10 s average reached by subtraction counts.
    listened_s >= SHORT_SPAN_S - 1e-6
}

/// False when a regular beat was looked for and not found: with at least
/// [`SHORT_SPAN_S`] of sound, the beat timing scatter is at or above
/// [`crate::beats::NO_WATCH_JITTER_FRACTION`] of a beat. With less sound,
/// regularity is not assessed and this is true.
pub fn watch_heard(jitter_us: Option<f64>, bph: u32, listened_s: f64) -> bool {
    !regularity_assessed(listened_s) || crate::beats::watch_heard(jitter_us, bph)
}

/// Something that limits how far a measurement can be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum Caveat {
    /// No regular beat found: the beat timing scatter is at or above
    /// [`crate::beats::NO_WATCH_JITTER_FRACTION`] of a beat. No readings
    /// are given.
    NoWatch,
    /// A set beat rate (`--bph`, `--calibre`) differs from the one heard.
    SetRateDisagrees { heard: u32 },
    /// More than [`crate::analysis::Clipping::NOTED_FRACTION`] of the beats
    /// have a clipped sample; `fraction` is their share.
    Clipped { fraction: f64 },
    /// Fewer than [`confidence::MIN_BEAT_FRACTION`] of the beats expected
    /// were found; `fraction` is the share found.
    BeatsMissing { fraction: f64 },
    /// The lift angle is a default, not the calibre's.
    LiftUnknown,
    /// The sound card's clock is not corrected.
    ClockUnknown,
    /// The beats span less than [`SHORT_SPAN_S`].
    ShortSpan { span_s: f64 },
    /// The take was paused or resumed inside the stretch.
    Breaks { count: usize },
    /// At `count` of those breaks Tick and Tock were not told apart by
    /// their sound (too alike, too few good beats, or no jump in the beats
    /// for the side check to run at), so the sides after each are named by
    /// the beat count across it alone.
    SidesByCount { count: usize },
    /// The beat error's sign differs either side of a cut in the stretch
    /// ([`crate::timing::sign_turns`]), so it is given as a size, positive,
    /// rather than signed.
    BeatErrorAsSize,
    /// Stretches of the take were masked out of the readings by hand;
    /// `seconds` is their total within the stretch.
    Masked { seconds: f64 },
}

impl Caveat {
    /// Where the caveat sits in a list.
    fn rank(&self) -> u8 {
        match self {
            Caveat::NoWatch => 0,
            Caveat::SetRateDisagrees { .. } => 1,
            Caveat::ShortSpan { .. } => 2,
            Caveat::Clipped { .. } => 3,
            Caveat::BeatsMissing { .. } => 4,
            Caveat::LiftUnknown => 5,
            Caveat::ClockUnknown => 6,
            Caveat::Breaks { .. } => 7,
            Caveat::SidesByCount { .. } => 8,
            Caveat::Masked { .. } => 9,
            Caveat::BeatErrorAsSize => 10,
        }
    }
}

/// Add a caveat in its place, once.
pub fn add(caveats: &mut Vec<Caveat>, c: Caveat) {
    if !caveats.contains(&c) {
        caveats.push(c);
        caveats.sort_by_key(Caveat::rank);
    }
}

/// What a stretch of beats was measured on, beyond its readings.
#[derive(Debug, Clone, Copy, Default)]
pub struct Context {
    /// See [`watch_heard`].
    pub watch_heard: bool,
    /// The beat rate heard, when a set one differs from it.
    pub bph_heard: Option<u32>,
    pub lift_known: bool,
    pub clock_known: bool,
    /// Beats found, and the beats the stretch should hold.
    pub beats: usize,
    pub expected: f64,
    /// Share of the beats with a clipped sample.
    pub clipped_fraction: f64,
    /// Seconds from the first beat to the last.
    pub span_s: f64,
    /// Seconds of sound the beats were sought in: the recording, the
    /// stretch, or the live screen's averaging time so far.
    pub listened_s: f64,
    pub breaks: usize,
    /// Of `breaks`, those where the sides were kept by the beat count.
    pub sides_by_count: usize,
    /// The beat error is given as a size ([`Caveat::BeatErrorAsSize`]).
    pub beat_error_as_size: bool,
    /// The seconds a shown rate is averaged over, when shorter than the
    /// stretch (the live screen's averaging time). `None`: the rate is the
    /// fit over the whole stretch.
    pub average_s: Option<f64>,
}

/// The length of a rate window, s.
const RATE_WINDOW_S: f64 = 10.0;

/// The rate's ± for a figure averaged over `average_s` within a run whose
/// rate windows are given: the windows' scatter scaled to that time. Over
/// the whole run it is the standard error [`confidence::rate_se`] gives;
/// over a shorter time it is wider, since fewer windows went into it.
pub fn rate_se_over(rate_windows: &[WindowFit], average_s: f64) -> Option<f64> {
    let independent = rate_windows.len().div_ceil(2) as f64;
    let se = confidence::rate_se(rate_windows)?;
    let averaged = (average_s / RATE_WINDOW_S).clamp(1.0, independent);
    Some(se * (independent / averaged).sqrt())
}

/// What one measurement rests on.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Assessment {
    /// Each reading's ±; `None` when no regular beat is found.
    pub confidence: Option<Confidence>,
    pub caveats: Vec<Caveat>,
    pub doubts: Doubts,
}

/// State what a measurement rests on, from its 10 s rate windows (half overlapping) and
/// its 2 s amplitude windows.
pub fn assess(
    rate_windows: &[WindowFit],
    amplitude_windows: &[&AmplitudeWindow],
    c: &Context,
) -> Assessment {
    let found = if c.expected > 0.0 {
        c.beats as f64 / c.expected
    } else {
        1.0
    };
    let doubts = Doubts {
        clipped: c.clipped_fraction > crate::analysis::Clipping::NOTED_FRACTION,
        beats_missing: found < confidence::MIN_BEAT_FRACTION,
        lift_unknown: !c.lift_known,
        clock_unknown: !c.clock_known,
    };
    let mut caveats = Vec::new();
    if !c.watch_heard {
        caveats.push(Caveat::NoWatch);
    }
    if let Some(heard) = c.bph_heard {
        caveats.push(Caveat::SetRateDisagrees { heard });
    }
    if c.span_s < SHORT_SPAN_S {
        caveats.push(Caveat::ShortSpan { span_s: c.span_s });
    }
    if doubts.clipped {
        caveats.push(Caveat::Clipped {
            fraction: c.clipped_fraction,
        });
    }
    if doubts.beats_missing {
        caveats.push(Caveat::BeatsMissing { fraction: found });
    }
    if doubts.lift_unknown {
        caveats.push(Caveat::LiftUnknown);
    }
    if doubts.clock_unknown {
        caveats.push(Caveat::ClockUnknown);
    }
    if c.breaks > 0 {
        caveats.push(Caveat::Breaks { count: c.breaks });
    }
    if c.sides_by_count > 0 {
        caveats.push(Caveat::SidesByCount {
            count: c.sides_by_count,
        });
    }
    if c.beat_error_as_size {
        caveats.push(Caveat::BeatErrorAsSize);
    }
    let confidence = c.watch_heard.then(|| {
        let amps: Vec<f64> = amplitude_windows.iter().filter_map(|w| w.mean()).collect();
        let bes: Vec<f64> = amplitude_windows
            .iter()
            .filter_map(|w| w.beat_error_unlock_ms)
            .collect();
        Confidence::new(
            match c.average_s {
                Some(t) => rate_se_over(rate_windows, t),
                None => confidence::rate_se(rate_windows),
            },
            standard_error(&amps),
            standard_error(&bes),
        )
    });
    Assessment {
        confidence,
        caveats,
        doubts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard() -> Context {
        Context {
            watch_heard: true,
            lift_known: true,
            clock_known: true,
            beats: 100,
            expected: 100.0,
            span_s: 60.0,
            listened_s: 60.0,
            ..Default::default()
        }
    }

    #[test]
    fn a_clean_stretch_has_no_caveats() {
        let a = assess(&[], &[], &heard());
        assert!(a.caveats.is_empty());
        assert!(a.confidence.is_some());
    }

    #[test]
    fn no_regular_beat_withholds_the_readings() {
        let a = assess(
            &[],
            &[],
            &Context {
                watch_heard: false,
                ..heard()
            },
        );
        assert_eq!(a.caveats, vec![Caveat::NoWatch]);
        assert!(a.confidence.is_none());
    }

    #[test]
    fn a_short_stretch_is_named_and_keeps_its_readings() {
        let a = assess(
            &[],
            &[],
            &Context {
                span_s: 6.0,
                ..heard()
            },
        );
        assert_eq!(a.caveats, vec![Caveat::ShortSpan { span_s: 6.0 }]);
        assert!(a.confidence.is_some());
    }

    /// Rate windows 5 s apart, alternately reading `rates`' values.
    fn rate_windows(rates: &[f64]) -> Vec<WindowFit> {
        rates
            .iter()
            .flat_map(|&r| [r, r])
            .enumerate()
            .map(|(i, r)| WindowFit {
                start_s: i as f64 * 5.0,
                end_s: i as f64 * 5.0 + 10.0,
                fit: crate::timing::TimingFit {
                    rate_s_per_day: r,
                    beat_error_ms: 0.0,
                    period_s: 0.125,
                    jitter_us: 0.0,
                    beats_used: 80,
                },
            })
            .collect()
    }

    #[test]
    fn a_short_average_has_a_wider_rate_error() {
        let rates: Vec<f64> = (0..36).map(|i| [1.0, -1.0, 0.5, -0.5][i % 4]).collect();
        let w = rate_windows(&rates);
        let whole = confidence::rate_se(&w).unwrap();
        // Averaged over the whole run, it is the standard error analyze uses.
        assert_eq!(rate_se_over(&w, 360.0), Some(whole));
        assert_eq!(rate_se_over(&w, 3600.0), Some(whole));
        // A 10 s average carries one window's scatter: six times wider
        // with 36 independent windows.
        let ten = rate_se_over(&w, 10.0).unwrap();
        assert!((ten / whole - 6.0).abs() < 1e-9, "{ten} against {whole}");
        let with = |average_s| {
            assess(
                &w,
                &[],
                &Context {
                    average_s,
                    ..heard()
                },
            )
            .confidence
            .unwrap()
            .rate
            .se
        };
        assert_eq!(with(None), Some(whole));
        assert_eq!(with(Some(10.0)), Some(ten));
    }

    #[test]
    fn under_three_windows_the_rate_error_is_unknown() {
        assert_eq!(rate_se_over(&rate_windows(&[1.0, -1.0]), 10.0), None);
    }

    /// Under 10 s of sound, regularity is not assessed: however scattered
    /// the beats, a regular beat is not said to be missing, the readings
    /// stay, and the short span is named.
    #[test]
    fn a_stretch_too_short_to_assess_keeps_its_readings() {
        assert!(!regularity_assessed(8.0));
        assert!(regularity_assessed(10.0));
        // Scatter of 8.6 ms at 21,600 bph: no regular beat over 10 s or
        // more, not assessed under it.
        assert!(!watch_heard(Some(8600.0), 21_600, 60.0));
        assert!(watch_heard(Some(8600.0), 21_600, 8.0));
        assert!(watch_heard(None, 21_600, 8.0));
        let a = assess(
            &[],
            &[],
            &Context {
                watch_heard: watch_heard(Some(8600.0), 21_600, 8.0),
                span_s: 8.0,
                listened_s: 8.0,
                ..heard()
            },
        );
        assert_eq!(a.caveats, vec![Caveat::ShortSpan { span_s: 8.0 }]);
        assert!(a.confidence.is_some());
    }

    /// Beats too scattered to make 10 s of a run, in a minute of sound, as
    /// on an empty stand heard live: no watch, not too short to tell.
    #[test]
    fn scattered_beats_in_long_sound_are_no_watch() {
        let a = assess(
            &[],
            &[],
            &Context {
                watch_heard: false,
                span_s: 0.0,
                ..heard()
            },
        );
        assert_eq!(
            a.caveats,
            vec![Caveat::NoWatch, Caveat::ShortSpan { span_s: 0.0 }]
        );
    }

    #[test]
    fn each_doubt_is_named() {
        let a = assess(
            &[],
            &[],
            &Context {
                clipped_fraction: 0.05,
                beats: 90,
                lift_known: false,
                clock_known: false,
                bph_heard: Some(21_600),
                breaks: 1,
                ..heard()
            },
        );
        let codes: Vec<String> = a
            .caveats
            .iter()
            .map(|c| {
                serde_json::to_value(c).unwrap()["code"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            codes,
            [
                "set_rate_disagrees",
                "clipped",
                "beats_missing",
                "lift_unknown",
                "clock_unknown",
                "breaks"
            ]
        );
    }
}
