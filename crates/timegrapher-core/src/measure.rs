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
//! less than [`SHORT_SPAN_S`] of sound their timing is not assessed. What
//! is looked at instead, and on a longer stretch whose scatter is not far
//! below noise's, is the beats' averaged sound: when neither the Tick's
//! nor the Tock's shows an unlock or a drop, no regular beat is found
//! (`no_watch`, reason `no_beat_sound`). Otherwise the readings are given,
//! `watch_heard` stays true, and `short_span` says how short a stretch
//! under 10 s is (see [`no_watch_reason`]).
//!
//! The beat error's sign: positive means Tick comes late. Each take starts
//! positive: Tick is the beat that comes late over the take's first stretch
//! with a clear sign, live and from the file alike ([`crate::beats::SideCheck`]);
//! near no beat error it is the take's first beat. Text shows the size.

use crate::amplitude::AmplitudeWindow;
use crate::confidence::{self, Confidence, Doubts};
use crate::timing::WindowFit;
use serde::{Deserialize, Serialize};

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
/// regularity is not assessed and this is true. [`no_watch_reason`] also
/// looks at the beats' sound.
pub fn watch_heard(jitter_us: Option<f64>, bph: u32, listened_s: f64) -> bool {
    !regularity_assessed(listened_s) || crate::beats::watch_heard(jitter_us, bph)
}

/// Why no regular beat was found, or `None` when one was: with at least
/// [`SHORT_SPAN_S`] of sound, the timing scatter ([`watch_heard`]). Then
/// the beats' averaged sound, when `beat_sound` (from
/// [`beat_sound_found`]) says neither side shows an unlock or a drop: on a
/// stretch under [`SHORT_SPAN_S`], or on a longer one whose scatter is
/// at or above [`crate::beats::NOISE_DOUBT_FRACTION`], where noise can
/// still scatter as little as this (#675). A watch's scatter is far
/// below it, so a noisy watch whose profile is hard to find keeps its
/// readings. With no profile to look at, the scatter alone.
pub fn no_watch_reason(
    jitter_us: Option<f64>,
    bph: u32,
    listened_s: f64,
    beat_sound: Option<bool>,
) -> Option<NoWatchReason> {
    let assessed = regularity_assessed(listened_s);
    if assessed && !crate::beats::watch_heard(jitter_us, bph) {
        return Some(NoWatchReason::Scatter);
    }
    let in_doubt = !assessed
        || crate::beats::timing_scatter_fraction(jitter_us, bph)
            .is_some_and(|f| f >= crate::beats::NOISE_DOUBT_FRACTION);
    (in_doubt && beat_sound == Some(false)).then_some(NoWatchReason::NoBeatSound)
}

/// Whether the beats' averaged Tick and Tock sounds show a watch's sound:
/// an unlock or a drop on either side, as [`crate::profile`] finds them on
/// the same template amplitude uses. `snippets` are each beat's envelope
/// window ([`crate::profile::snippet`]) with whether it is a Tick (an even
/// beat). `None` when neither side has enough beats for a profile.
pub fn beat_sound_found<'a>(
    snippets: impl IntoIterator<Item = (bool, &'a [f32])>,
    fs: f64,
    osc_period_s: f64,
    cfg: &crate::amplitude::AmplitudeConfig,
) -> Option<bool> {
    let (tick, tock): (Vec<_>, Vec<_>) = snippets.into_iter().partition(|s| s.0);
    let shows = |side: crate::profile::Side, w: Vec<(bool, &[f32])>| {
        let w: Vec<&[f32]> = w.into_iter().map(|s| s.1).collect();
        crate::profile::from_snippets(&w, fs, side, osc_period_s, cfg, None)
            .map(|p| p.unlock_ms.is_some() || p.drop_ms.is_some())
    };
    match (
        shows(crate::profile::Side::A, tick),
        shows(crate::profile::Side::B, tock),
    ) {
        (None, None) => None,
        (a, b) => Some(a == Some(true) || b == Some(true)),
    }
}

/// The reason on the `no_watch` caveat among `caveats`, if there is one.
pub fn no_watch_reason_in(caveats: &[Caveat]) -> Option<NoWatchReason> {
    caveats.iter().find_map(|c| match c {
        Caveat::NoWatch { reason } => Some(*reason),
        _ => None,
    })
}

/// The sentence for no regular beat found, as every surface states it:
/// with the timing scatter, as [`crate::beats::no_regular_beat_text`]
/// gives it, or that the beats' sound shows no unlock and no drop.
pub fn no_watch_text(
    reason: NoWatchReason,
    jitter_us: Option<f64>,
    fraction: Option<f64>,
) -> String {
    match reason {
        NoWatchReason::Scatter => crate::beats::no_regular_beat_text(jitter_us, fraction),
        NoWatchReason::NoBeatSound => {
            "No regular beat found (neither the Tick's nor the Tock's sound shows an unlock \
             or a drop)"
                .into()
        }
    }
}

/// What the `no_watch` caveat ([`Caveat::NoWatch`]) was found on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoWatchReason {
    /// The beat timing scatter, with at least [`SHORT_SPAN_S`] of sound.
    #[default]
    Scatter,
    /// With less sound, the beats' averaged sound: neither the Tick's nor
    /// the Tock's shows an unlock or a drop ([`beat_sound_found`]).
    NoBeatSound,
}

/// Something that limits how far a measurement can be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum Caveat {
    /// No regular beat found: the beat timing scatter is at or above
    /// [`crate::beats::NO_WATCH_JITTER_FRACTION`] of a beat (`reason`
    /// `scatter`), or, with under [`SHORT_SPAN_S`] of sound, neither the
    /// Tick's nor the Tock's averaged sound shows an unlock or a drop
    /// (`no_beat_sound`). No readings are given.
    NoWatch {
        #[serde(default)]
        reason: NoWatchReason,
    },
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
    /// The recording's USB lines correct the rates, and its clock log, which
    /// would correct them on its own, measures the card differently by
    /// more than its own scatter ([`crate::clock::ClockFit::sources_disagree`]):
    /// ppm the card runs
    /// slow by each, and the log's standard error.
    ClockSourcesDisagree {
        usb_ppm: f64,
        log_ppm: f64,
        log_ppm_sd: f64,
    },
    /// The beats span less than [`SHORT_SPAN_S`].
    ShortSpan { span_s: f64 },
    /// The take was paused or resumed inside the stretch.
    Breaks { count: usize },
    /// The take's breaks are not all from the list its `session.json`
    /// keeps, but from its clock log: `why` says what stood in the way
    /// ([`crate::breaks::Unlisted`]). A pause the clock log does not show
    /// is then not a break, and the rate may be fitted across it.
    BreaksUnlisted { why: crate::breaks::Unlisted },
    /// At `count` of those breaks Tick and Tock were not told apart by
    /// their sound or by the beat error's sign (too alike, too few good
    /// beats, or no jump in the beats for the side check to run at), so the
    /// sides after each are named by the beat count across it alone.
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
    /// The caveat in a few words, for a line of conditions or a mark beside
    /// a reading, as the app words it: "2 breaks", "input clock not
    /// corrected". `lift_deg` is the lift angle the amplitude used, and
    /// `beats` the beats found. `None` for no regular beat (said by
    /// [`no_watch_text`]) and for a masked sliver under 0.05 s.
    pub fn short(&self, lift_deg: f64, beats: usize) -> Option<String> {
        Some(match self {
            Caveat::NoWatch { .. } => return None,
            Caveat::SetRateDisagrees { heard } => format!("{heard} bph heard"),
            Caveat::ShortSpan { span_s } => format!("{span_s:.0} s of beats"),
            Caveat::Clipped { fraction } => format!("{:.0}% clipped", fraction * 100.0),
            Caveat::BeatsMissing { fraction } => {
                let expected = (beats as f64 / fraction.max(1e-9)).round() as usize;
                if beats > 0 {
                    format!("{beats} of {expected} beats")
                } else {
                    format!("{:.0}% of beats found", fraction * 100.0)
                }
            }
            Caveat::LiftUnknown => format!("lift angle {lift_deg}°, the default"),
            Caveat::ClockUnknown => "input clock not corrected".to_string(),
            Caveat::ClockSourcesDisagree {
                usb_ppm, log_ppm, ..
            } => format!(
                "USB lines and clock log {:.0} ppm apart",
                (usb_ppm - log_ppm).abs()
            ),
            Caveat::Breaks { count } => format!("{count} break{}", plural(*count)),
            Caveat::BreaksUnlisted { .. } => "breaks from the clock log".to_string(),
            Caveat::SidesByCount { .. } => "sides named by count".to_string(),
            Caveat::BeatErrorAsSize => {
                "beat error given as a size: its sign differs either side of a cut".to_string()
            }
            Caveat::Masked { seconds } => format!("{} s masked", masked_seconds(*seconds)?),
        })
    }

    /// The caveat as a plain fact, a sentence, as the app words it in a
    /// reading's hover: what limits the reading, with no advice. `lift_deg`
    /// is the lift angle the amplitude used, when known. `None` as for
    /// [`Caveat::short`].
    pub fn long(&self, lift_deg: Option<f64>) -> Option<String> {
        Some(match self {
            Caveat::NoWatch { .. } => return None,
            Caveat::SetRateDisagrees { heard } => {
                format!("The beat rate set differs from the {heard} bph heard.")
            }
            Caveat::ShortSpan { span_s } => format!(
                "The beats span {span_s:.0} s, under the {SHORT_SPAN_S:.0} s needed to judge \
                 whether they are regular."
            ),
            Caveat::Clipped { fraction } => format!(
                "{:.0}% of the beats have a clipped sample, which moves the timing of the \
                 sounds inside a beat.",
                fraction * 100.0
            ),
            Caveat::BeatsMissing { fraction } => format!(
                "{:.0}% of the beats the beat rate expects in this stretch were found.",
                fraction * 100.0
            ),
            Caveat::LiftUnknown => match lift_deg {
                Some(l) => format!(
                    "The lift angle is the default {l}°, not one set for the calibre. Every \
                     amplitude is worked out from it."
                ),
                None => "The lift angle is the default, not one set for the calibre. Every \
                         amplitude is worked out from it."
                    .to_string(),
            },
            Caveat::ClockUnknown => "This input's clock hasn't been measured, so the rate \
                                     includes the sound card's own clock error."
                .to_string(),
            Caveat::ClockSourcesDisagree {
                usb_ppm,
                log_ppm,
                log_ppm_sd,
            } => {
                let note = crate::clock::SourcesDisagree {
                    usb_ppm: *usb_ppm,
                    log_ppm: *log_ppm,
                    log_ppm_sd: *log_ppm_sd,
                }
                .note();
                format!("{}{}.", note[..1].to_uppercase(), &note[1..])
            }
            Caveat::Breaks { count } => format!(
                "{count} break{} in this stretch: a hole in the sound, such as when the \
                 computer slept, or a stumble where the beats stepped off together by more \
                 than {:.0} ms.",
                plural(*count),
                crate::beats::JUMP_S * 1000.0
            ),
            Caveat::BreaksUnlisted { why } => {
                let what = match why {
                    crate::breaks::Unlisted::Unreadable => {
                        "This take's session.json could not be read"
                    }
                    crate::breaks::Unlisted::EntriesSkipped => {
                        "Some of the breaks this take's session.json lists give no place or gap"
                    }
                    crate::breaks::Unlisted::NotTheTake => {
                        "These files are not the whole of the take its session.json describes"
                    }
                };
                format!(
                    "{what}, so its breaks were read from the clock log instead, which shows \
                     a pause only where the time it lost is {:.1} s or more.",
                    crate::breaks::MIN_GAP_S
                )
            }
            Caveat::SidesByCount { count } => format!(
                "Tick and Tock are named by the beat count across {count} break{} in this \
                 stretch, not told apart by their sound.",
                plural(*count)
            ),
            Caveat::BeatErrorAsSize => "The beat error's sign differs either side of a cut \
                                        in the stretch, so it is given as a size."
                .to_string(),
            Caveat::Masked { seconds } => format!(
                "{} s of this stretch were masked out and left out of the readings.",
                masked_seconds(*seconds)?
            ),
        })
    }

    /// Where the caveat sits in a list.
    fn rank(&self) -> u8 {
        match self {
            Caveat::NoWatch { .. } => 0,
            Caveat::SetRateDisagrees { .. } => 1,
            Caveat::ShortSpan { .. } => 2,
            Caveat::Clipped { .. } => 3,
            Caveat::BeatsMissing { .. } => 4,
            Caveat::LiftUnknown => 5,
            Caveat::ClockUnknown => 6,
            Caveat::ClockSourcesDisagree { .. } => 7,
            Caveat::Breaks { .. } => 8,
            Caveat::BreaksUnlisted { .. } => 9,
            Caveat::SidesByCount { .. } => 10,
            Caveat::Masked { .. } => 11,
            Caveat::BeatErrorAsSize => 12,
        }
    }
}

impl From<crate::clock::SourcesDisagree> for Caveat {
    fn from(d: crate::clock::SourcesDisagree) -> Caveat {
        Caveat::ClockSourcesDisagree {
            usb_ppm: d.usb_ppm,
            log_ppm: d.log_ppm,
            log_ppm_sd: d.log_ppm_sd,
        }
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// Masked seconds as the words give them: a tenth under 10 s, so a mask
/// reaching just into a stretch doesn't read 0, and nothing under 0.05 s,
/// where that would read 0.0.
fn masked_seconds(s: f64) -> Option<String> {
    if s < 0.05 {
        None
    } else if s < 9.95 {
        Some(format!("{s:.1}"))
    } else {
        Some(format!("{s:.0}"))
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
    /// What no regular beat was found on, when `watch_heard` is false.
    pub no_watch_reason: NoWatchReason,
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
        caveats.push(Caveat::NoWatch {
            reason: c.no_watch_reason,
        });
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
    if c.beat_error_as_size && c.watch_heard {
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
            confidence::standard_error(&amps),
            confidence::standard_error(&bes),
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
    fn the_no_watch_reason_is_read_from_its_caveat_only() {
        assert_eq!(no_watch_reason_in(&[]), None);
        assert_eq!(
            no_watch_reason_in(&[Caveat::SetRateDisagrees { heard: 28_800 }]),
            None
        );
        assert_eq!(
            no_watch_reason_in(&[
                Caveat::SetRateDisagrees { heard: 28_800 },
                Caveat::NoWatch {
                    reason: NoWatchReason::NoBeatSound
                },
            ]),
            Some(NoWatchReason::NoBeatSound)
        );
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
        assert_eq!(
            a.caveats,
            vec![Caveat::NoWatch {
                reason: NoWatchReason::Scatter
            }]
        );
        assert!(a.confidence.is_none());
    }

    #[test]
    fn no_regular_beat_says_nothing_of_a_beat_error_size() {
        // A cut take with no regular beat has no beat error to call a size.
        let a = assess(
            &[],
            &[],
            &Context {
                watch_heard: false,
                beat_error_as_size: true,
                ..heard()
            },
        );
        assert_eq!(
            a.caveats,
            vec![Caveat::NoWatch {
                reason: NoWatchReason::Scatter
            }]
        );
        let a = assess(
            &[],
            &[],
            &Context {
                beat_error_as_size: true,
                ..heard()
            },
        );
        assert_eq!(a.caveats, vec![Caveat::BeatErrorAsSize]);
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
            vec![
                Caveat::NoWatch {
                    reason: NoWatchReason::Scatter
                },
                Caveat::ShortSpan { span_s: 0.0 }
            ]
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

    #[test]
    fn caveats_are_worded_once_for_every_surface() {
        let unread = Caveat::BreaksUnlisted {
            why: crate::breaks::Unlisted::Unreadable,
        };
        assert_eq!(unread.short(52.0, 0).unwrap(), "breaks from the clock log");
        assert_eq!(
            unread.long(None).unwrap(),
            "This take's session.json could not be read, so its breaks were read from the \
             clock log instead, which shows a pause only where the time it lost is 0.5 s or \
             more."
        );
        assert_eq!(
            serde_json::to_value(unread).unwrap(),
            serde_json::json!({ "code": "breaks_unlisted", "why": "unreadable" })
        );
        assert_eq!(
            Caveat::Breaks { count: 1 }.short(52.0, 0).unwrap(),
            "1 break"
        );
        assert_eq!(
            Caveat::Breaks { count: 2 }.short(52.0, 0).unwrap(),
            "2 breaks"
        );
        assert_eq!(
            Caveat::BeatsMissing { fraction: 0.5 }
                .short(52.0, 400)
                .unwrap(),
            "400 of 800 beats"
        );
        assert_eq!(
            Caveat::LiftUnknown.short(52.0, 0).unwrap(),
            "lift angle 52°, the default"
        );
        assert_eq!(
            Caveat::Masked { seconds: 12.4 }.short(0.0, 0).unwrap(),
            "12 s masked"
        );
        assert_eq!(
            Caveat::Masked { seconds: 0.3 }.short(0.0, 0).unwrap(),
            "0.3 s masked"
        );
        assert_eq!(Caveat::Masked { seconds: 0.02 }.short(0.0, 0), None);
        assert_eq!(
            Caveat::NoWatch {
                reason: NoWatchReason::default()
            }
            .long(None),
            None
        );
        assert_eq!(
            Caveat::SidesByCount { count: 1 }.long(None).unwrap(),
            "Tick and Tock are named by the beat count across 1 break in this stretch, not \
             told apart by their sound."
        );
        assert!(Caveat::LiftUnknown
            .long(Some(52.0))
            .unwrap()
            .contains("default 52°"));
        // Both clocks' figures, and which one corrects (#498).
        let d = Caveat::ClockSourcesDisagree {
            usb_ppm: 120.0,
            log_ppm: 20.0,
            log_ppm_sd: 2.0,
        };
        assert_eq!(
            d.short(52.0, 0).unwrap(),
            "USB lines and clock log 100 ppm apart"
        );
        assert_eq!(
            d.long(None).unwrap(),
            "The USB lines measure the sound card at +120.0 ppm and the clock log at +20 ± 2 \
             ppm; the rates are corrected by the USB lines."
        );
        // The JSON a caveat is written as reads back into the same words.
        let c: Caveat = serde_json::from_str(r#"{"code":"clipped","fraction":0.04}"#).unwrap();
        assert_eq!(c.short(52.0, 0).unwrap(), "4% clipped");
    }
}
