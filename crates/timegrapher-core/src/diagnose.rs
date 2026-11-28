//! Signal checks for setting up a microphone: the level, clipping, whether
//! the background rises between ticks, and whether a regular beat is
//! found.
//!
//! Works on a few seconds of audio from a live device or a file. It only
//! measures and names the conditions detected; the mixer changes for
//! `--apply` are the caller's business (see the CLI's `doctor` command).

use crate::audio::Audio;
use crate::beats;
use crate::dsp::{envelope, median, EnvelopeConfig};
use crate::session::BeatErrorSource;
use serde::Serialize;

/// The peak level, dBFS, that `doctor --apply` sets the input to: about
/// −10 dBFS.
pub const TARGET_PEAK_DBFS: f64 = -10.0;

/// Peaks above this, dBFS, are the `hot` condition.
pub const HOT_PEAK_DBFS: f64 = -6.0;

/// Peaks below this, dBFS, are the `too_quiet` condition.
pub const QUIET_PEAK_DBFS: f64 = -30.0;

/// A peak under this, linear (−80 dBFS), is no signal at all.
const SILENT_PEAK: f64 = 1e-4;

/// Background rising more than this between ticks, dB, is the
/// `agc_suspected` condition.
pub const GAP_RISE_DB: f64 = 3.0;

/// Below this `signal_x` the `no_ticks` condition is named: pure noise
/// reads about 2× (see `docs/research/validation.md`).
pub const NO_TICKS_SIGNAL_X: f64 = 3.0;

/// Whether a signal figure shows the ticks too faint over the noise: under
/// [`NO_TICKS_SIGNAL_X`]. With no figure (the envelope's median zero, as
/// over digital silence) the ticks are not shown faint, and the beat tests
/// decide (#784).
fn faint(signal_x: Option<f64>) -> bool {
    signal_x.is_some_and(|x| x < NO_TICKS_SIGNAL_X)
}

/// The signal figure as every line of the check shows it: to 0.1×,
/// rounded down, as "2.9×", so the figure shown never claims more signal
/// than was measured, and it reads under 3.0× exactly when the ticks are
/// [`faint`] (Norm on #2091). The one formatter for it.
pub fn signal_text(signal_x: f64) -> String {
    // The nudge, on the f32 scale `signal_x` is measured on, keeps 2.3f32
    // (2.2999999523 as f64) at 2.3; the largest f32 under 3.0 still reads 2.9.
    let shown = (signal_x * 10.0 + 1e-6).floor() / 10.0;
    format!("{}×", crate::library::fixed_text(shown, 1))
}

#[derive(Debug, Clone)]
pub struct DiagnoseConfig {
    pub envelope: EnvelopeConfig,
    /// Beat rate; `None` guesses it.
    pub bph: Option<u32>,
    /// A sample at or above this magnitude counts as clipped.
    pub clip_level: f32,
}

impl Default for DiagnoseConfig {
    fn default() -> Self {
        DiagnoseConfig {
            envelope: EnvelopeConfig::default(),
            bph: None,
            clip_level: 0.99,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCode {
    /// Peak under −80 dBFS: no signal.
    Silent,
    /// Peak under [`QUIET_PEAK_DBFS`].
    TooQuiet,
    /// Three or more samples at full scale or in flat tops.
    Clipping,
    /// Peak above [`HOT_PEAK_DBFS`], not clipped.
    Hot,
    /// The background rises more than [`GAP_RISE_DB`] between ticks, as
    /// it does when the input's automatic gain turns down on each tick
    /// and back up in the gaps.
    AgcSuspected,
    /// No regular beat found, as `analyze` finds it, or `signal_x` under
    /// [`NO_TICKS_SIGNAL_X`].
    NoTicks,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    /// Stable identifier for programs and agents; the title is for people.
    pub code: IssueCode,
    /// The condition in words, with its figure.
    pub title: String,
    /// The measurements it was detected from.
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignalCheck {
    pub sample_rate: u32,
    pub duration_s: f64,
    /// Largest sample magnitude, dBFS.
    pub peak_dbfs: f64,
    pub rms_dbfs: f64,
    /// Mean of the samples relative to full scale.
    pub dc_offset: f64,
    /// Samples at the clip level, plus samples in flat tops (four or more
    /// equal samples at the recording's peak), which is how clipping in
    /// the analogue stage shows up below digital full scale.
    pub clipped_samples: usize,
    /// Beat rate used, bph (guessed unless given).
    pub bph: u32,
    pub beats_expected: usize,
    /// Beats found with a typical tick shape.
    pub beats_found: usize,
    /// Median tick peak in the high-passed signal, dBFS.
    pub tick_level_dbfs: Option<f64>,
    /// Median background level between ticks in the high-passed signal, dBFS.
    pub noise_level_dbfs: f64,
    /// Tick peak over background, dB.
    pub tick_to_noise_db: Option<f64>,
    /// Median beat peak over the median of the envelope, as the desktop
    /// app's "signal Nx" ([`crate::live::signal_figure`]); None when no
    /// beat was found, or where the envelope's median is zero, as over
    /// digital silence. Pure noise reads about 2×.
    pub signal_x: Option<f64>,
    /// Background late in the gap between ticks over early in it, dB.
    /// Near 0 for a fixed gain; several dB when automatic gain pumps.
    pub gap_rise_db: Option<f64>,
    /// Whether a watch is heard, as `analyze` decides it
    /// ([`crate::analysis::Summary::watch_heard`]): false where a regular
    /// beat was looked for and not found, as on an empty stand. The rate
    /// and beat error are then none (#2091).
    pub watch_heard: bool,
    /// The whole-recording rate on the sound card's clock, `analyze`'s
    /// ([`crate::analysis::Summary::rate_s_per_day`]), s/d.
    pub rate_s_per_day: Option<f64>,
    /// That rate corrected for the sound card's clock from the USB lines
    /// in the recording, where they apply, as `analyze` leads with
    /// ([`crate::analysis::Summary::rate_corrected_s_per_day`]), s/d.
    pub rate_corrected_s_per_day: Option<f64>,
    /// The beat error `analyze` leads with, unsigned, ms: from the unlock,
    /// else from the drop (`beat_error_source`; #2095).
    pub beat_error_ms: Option<f64>,
    /// Where `beat_error_ms` is measured from: `unlock` or `drop`.
    pub beat_error_source: Option<BeatErrorSource>,
    /// Why `analyze` gives no figures though it hears a watch: the
    /// recording is under 10 s and the beat sound is not yet judged
    /// ([`crate::measure::not_assessed_text`]); none otherwise.
    pub not_assessed: Option<String>,
    /// The gain change `doctor --apply` makes, dB: it sets the peak to
    /// about −10 dBFS ([`TARGET_PEAK_DBFS`]), or steps down 6 dB when the
    /// input clips. Negative means down. `None` when the peak is between
    /// [`QUIET_PEAK_DBFS`] and [`HOT_PEAK_DBFS`].
    pub suggested_gain_change_db: Option<f64>,
    /// The conditions detected; empty when none of them is.
    pub issues: Vec<Issue>,
}

impl SignalCheck {
    pub fn has(&self, code: IssueCode) -> bool {
        self.issues.iter().any(|i| i.code == code)
    }
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-10).log10()
}

/// The mean of `v`, 0 for none: core's one mean (`dsp::mean_of`, #512).
fn mean(v: &[f32]) -> f64 {
    crate::dsp::mean_of(v.iter().map(|&x| f64::from(x))).unwrap_or(0.0)
}

fn count_clipped(x: &[f32], clip_level: f32) -> usize {
    clipped_indices(x, clip_level).len()
}

/// The samples that are clipped: at or above `clip_level`, or in a flat top
/// (four or more equal samples at the recording's peak, which is how
/// clipping before the converter shows once the level is turned down
/// after it). In order.
pub fn clipped_indices(x: &[f32], clip_level: f32) -> Vec<usize> {
    let peak = x.iter().fold(0f32, |m, v| m.max(v.abs()));
    if peak >= clip_level {
        return (0..x.len()).filter(|&i| x[i].abs() >= clip_level).collect();
    }
    let mut out = Vec::new();
    if peak > 0.25 {
        let near = |v: f32| (v.abs() - peak).abs() <= peak * 1e-3;
        let mut run = 0;
        for i in 1..x.len() {
            let (a, b) = (x[i - 1], x[i]);
            if near(a) && near(b) && (a - b).abs() <= peak * 1e-3 {
                run += 1;
            } else {
                if run >= 3 {
                    out.extend(i - run - 1..i);
                }
                run = 0;
            }
        }
        if run >= 3 {
            out.extend(x.len() - run - 1..x.len());
        }
    }
    out
}

/// Measure a short recording and name the conditions detected in it.
pub fn check(audio: &Audio, cfg: &DiagnoseConfig) -> SignalCheck {
    let fs = audio.sample_rate as f64;
    let x = &audio.samples;
    let duration_s = audio.duration();
    let peak = x.iter().fold(0f32, |m, v| m.max(v.abs())) as f64;
    let dc = mean(x);
    let rms =
        (x.iter().map(|&v| (v as f64 - dc).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt();
    let clipped = count_clipped(x, cfg.clip_level);

    // The check's own beats below are for the levels and the signal
    // figure; the readings are `analyze`'s.
    let read = quick_look(audio, cfg);
    let env = envelope(x, fs, &cfg.envelope);
    let bph = read.bph;
    let beat_s = 3600.0 / bph as f64;
    let expected = (duration_s / beat_s).floor() as usize;
    let (found, _) = if peak > 0.0 {
        beats::detect(&env, fs, bph)
    } else {
        (Vec::new(), Vec::new())
    };
    let good: Vec<_> = found.iter().filter(|b| b.is_good()).copied().collect();

    let Levels {
        tick,
        noise,
        gap_rise,
    } = tick_levels(&env, fs, &good, beat_s);
    let tick_to_noise = tick.map(|t| db(t) - db(noise));
    // The readings' own signal figure (`live::signal_figure`), none with
    // no beat found or where the envelope's median is zero, as over
    // digital silence, which has no figure (#784).
    let signal_x = (!found.is_empty())
        .then(|| crate::live::signal_figure(&env, fs, &found))
        .flatten()
        .map(f64::from);

    let mut issues = Vec::new();
    let (silent, clipping) = level_issues(&mut issues, peak, clipped, gap_rise);
    let mut add = |code, title: String, evidence: String| {
        issues.push(Issue {
            code,
            title,
            evidence,
        })
    };
    let faint = faint(signal_x);
    let signal = signal_x.map_or("no signal figure".into(), |x| {
        format!("signal {}", signal_text(x))
    });
    // While `analyze` waits (under 10 s, Norm on #2333) there is no
    // verdict either way: neither "No regular beat" nor "Ticks too faint",
    // which would say ticks are there over an empty stand (Norm 31).
    if !silent && !read.awaiting && (!read.watch_heard || faint) {
        // No regular beat is `analyze`'s verdict alone, in its words
        // (Norm on #2091); ticks too faint over the noise is a set-up
        // condition, named however well they time (#772), that leaves the
        // readings to `analyze`.
        let (title, evidence) = if !read.watch_heard {
            let reason = crate::measure::no_watch_reason_in(&read.caveats).unwrap_or_default();
            (
                crate::measure::no_watch_text(reason, read.jitter_us, read.bph_set),
                signal.clone(),
            )
        } else {
            (
                format!("Ticks too faint over the noise ({signal}, under {NO_TICKS_SIGNAL_X:.0}×)"),
                format!("{} of about {expected} beats found, {signal}", good.len()),
            )
        };
        add(IssueCode::NoTicks, title, evidence);
    }
    let too_quiet = !silent && !clipping && db(peak) < QUIET_PEAK_DBFS;
    if too_quiet {
        add(
            IssueCode::TooQuiet,
            format!("Peak {:.0} dBFS", db(peak)),
            format!("under {QUIET_PEAK_DBFS:.0} dBFS, too quiet to time well"),
        );
    }
    let suggested = suggested_gain(peak, silent, clipping, too_quiet);
    let beat_error = read.headline_beat_error();
    // `analyze` gives no verdict and no figures yet: under 10 s of sound
    // (#2014, #2333); said in `analyze`'s words.
    let not_assessed = not_assessed(read.watch_heard, beat_error.is_some(), duration_s);

    SignalCheck {
        sample_rate: audio.sample_rate,
        duration_s,
        peak_dbfs: db(peak),
        rms_dbfs: db(rms),
        dc_offset: dc,
        clipped_samples: clipped,
        bph,
        beats_expected: expected,
        beats_found: good.len(),
        // The ticks' levels only once `analyze` has a verdict: while it
        // waits, the "beats" may be noise (#2091, #2333).
        tick_level_dbfs: tick.map(db).filter(|_| !read.awaiting),
        noise_level_dbfs: db(noise),
        tick_to_noise_db: tick_to_noise.filter(|_| !read.awaiting),
        signal_x: signal_x.filter(|_| !read.awaiting),
        gap_rise_db: gap_rise,
        watch_heard: read.watch_heard,
        rate_s_per_day: read.rate_s_per_day,
        rate_corrected_s_per_day: read.rate_corrected_s_per_day,
        beat_error_ms: beat_error.map(|(v, _)| v.abs()),
        beat_error_source: beat_error.map(|(_, s)| s),
        not_assessed,
        suggested_gain_change_db: suggested,
        issues,
    }
}

/// The quick look: `analyze`'s reading of the same sound, at the beat rate
/// given if one is: whether a watch is heard, its rate and its beat error
/// (#2091, #2095; Norm: the doctor fits nothing of its own).
fn quick_look(audio: &Audio, cfg: &DiagnoseConfig) -> crate::analysis::Summary {
    crate::analysis::analyze(
        audio,
        &crate::analysis::AnalysisConfig {
            bph: cfg.bph,
            bph_from: crate::set_rate::BphFrom::of(cfg.bph, None, None),
            envelope: cfg.envelope.clone(),
            ..Default::default()
        },
    )
    .summary
}

/// Why `analyze` gives no figures though it hears a watch: `duration_s`
/// of sound read from its start is too short for its regularity to be
/// assessed, and the beat sound did not decide (#2014). None where a
/// watch is not heard (the no-regular-beat condition says why), where
/// there is a beat error, or where the recording is long enough.
fn not_assessed(watch_heard: bool, has_figures: bool, duration_s: f64) -> Option<String> {
    (watch_heard && !has_figures && !crate::measure::regularity_assessed(duration_s, true))
        .then(|| crate::measure::not_assessed_text(duration_s))
}

/// The quick look's words after its figures: where the beat error is
/// measured from and which clock the rate is on, as `analyze` says them
/// (#2095): "(from the unlock; corrected for the sound card's clock)".
pub fn quick_look_basis(source: Option<BeatErrorSource>, corrected: bool) -> String {
    let from = match source {
        Some(BeatErrorSource::Unlock) => "from the unlock; ",
        Some(BeatErrorSource::Drop) => "from the drop; ",
        None => "",
    };
    let clock = if corrected {
        "corrected for the sound card's clock"
    } else {
        "on the sound card's clock"
    };
    format!("({from}{clock})")
}

/// The tick, background and in-gap rise levels of a recording.
struct Levels {
    /// The median tick peak, none with no good beat.
    tick: Option<f64>,
    /// The background, scaled to read as RMS.
    noise: f64,
    /// How far the background late in the gap sits above early in it, in
    /// dB, once there are 8 gaps to compare.
    gap_rise: Option<f64>,
}

/// Levels around each tick, from the envelope `env`.
fn tick_levels(env: &[f32], fs: f64, good: &[beats::Beat], beat_s: f64) -> Levels {
    // Levels around each tick, from the envelope (high-passed, rectified):
    // tick peak just around the beat, background in the gap after it.
    let at = |t: f64| ((t * fs).round().max(0.0) as usize).min(env.len());
    let level = |a: f64, b: f64| -> Option<f64> {
        let (i, j) = (at(a), at(b));
        (j > i).then(|| mean(&env[i..j]))
    };
    let mut ticks = Vec::new();
    let mut early = Vec::new();
    let mut late = Vec::new();
    for b in good {
        let (i, j) = (at(b.time - 0.015), at(b.time + 0.005));
        if j > i {
            ticks.push(env[i..j].iter().fold(0f32, |m, &v| m.max(v)) as f64);
        }
        if let (Some(e), Some(l)) = (
            level(b.time + 0.30 * beat_s, b.time + 0.45 * beat_s),
            level(b.time + 0.75 * beat_s, b.time + 0.90 * beat_s),
        ) {
            early.push(e);
            late.push(l);
        }
    }
    // The envelope is the mean of the rectified signal, which is about
    // 0.8 of its RMS for noise; scale back so levels read as RMS.
    let rect_to_rms = (std::f64::consts::PI / 2.0).sqrt();
    let noise = if late.is_empty() {
        let mut e: Vec<f64> = env.iter().map(|&v| v as f64).collect();
        if e.is_empty() {
            0.0
        } else {
            median(&mut e)
        }
    } else {
        let mut both: Vec<f64> = early.iter().chain(&late).copied().collect();
        median(&mut both)
    } * rect_to_rms;
    let tick = (!ticks.is_empty()).then(|| median(&mut ticks));
    let gap_rise = (early.len() >= 8).then(|| {
        let mut r: Vec<f64> = early
            .iter()
            .zip(&late)
            .filter(|(e, _)| **e > 0.0)
            .map(|(e, l)| db(l / e))
            .collect();
        if r.is_empty() {
            0.0
        } else {
            median(&mut r)
        }
    });
    Levels {
        tick,
        noise,
        gap_rise,
    }
}

/// Adds the issues the levels alone show (no signal, clipping, a hot peak,
/// a background rising between ticks) to `issues`, and says whether the
/// recording is silent and whether it clips.
fn level_issues(
    issues: &mut Vec<Issue>,
    peak: f64,
    clipped: usize,
    gap_rise: Option<f64>,
) -> (bool, bool) {
    let mut add = |code, title: String, evidence: String| {
        issues.push(Issue {
            code,
            title,
            evidence,
        })
    };
    let silent = peak < SILENT_PEAK;
    if silent {
        add(
            IssueCode::Silent,
            format!("No signal: peak {:.0} dBFS", db(peak)),
            format!(
                "under {:.0} dBFS, the level taken as no signal at all",
                db(SILENT_PEAK)
            ),
        );
    }
    let clipping = clipped >= 3;
    if clipping {
        add(
            IssueCode::Clipping,
            format!("Clipping: {clipped} samples"),
            format!("{clipped} samples clipped, peak {:.1} dBFS", db(peak)),
        );
    } else if !silent && db(peak) > HOT_PEAK_DBFS {
        add(
            IssueCode::Hot,
            format!("Peak {:.1} dBFS", db(peak)),
            format!(
                "peak {:.1} dBFS, {:.1} dB below full scale",
                db(peak),
                -db(peak)
            ),
        );
    }
    if let Some(r) = gap_rise.filter(|&r| r > GAP_RISE_DB) {
        add(
            IssueCode::AgcSuspected,
            format!("Background rises {r:.1} dB between ticks"),
            format!("background late in the gap {r:.1} dB above early in it"),
        );
    }
    (silent, clipping)
}

/// The gain change to suggest, in dB: none for silence or a good level.
fn suggested_gain(peak: f64, silent: bool, clipping: bool, too_quiet: bool) -> Option<f64> {
    if silent {
        None
    } else if clipping {
        // The true peak is unknown once clipped; step down and re-check.
        Some(-6.0)
    } else if too_quiet || db(peak) > HOT_PEAK_DBFS {
        Some(((TARGET_PEAK_DBFS - db(peak)) * 2.0).round() / 2.0)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A figure under 3× is faint and one at 3× or more is not; no figure
    /// at all is not shown faint, so the beat tests decide (#784).
    #[test]
    fn only_a_figure_under_the_threshold_is_faint() {
        assert!(faint(Some(2.9)));
        assert!(!faint(Some(NO_TICKS_SIGNAL_X)));
        assert!(!faint(Some(40.0)));
        assert!(!faint(None));
    }

    /// The signal figure is shown rounded down to 0.1×, so it reads under
    /// 3.0× exactly when it is faint (Norm on #2091): 2.96 reads 2.9× and
    /// is faint, 3.0 reads 3.0× and is not.
    #[test]
    fn the_signal_figure_reads_under_3_exactly_when_faint() {
        assert_eq!(signal_text(2.96), "2.9×");
        assert!(faint(Some(2.96)));
        assert_eq!(signal_text(3.0), "3.0×");
        assert!(!faint(Some(3.0)));
        assert_eq!(signal_text(2.3), "2.3×");
        assert_eq!(signal_text(17.87), "17.8×");
        // `signal_x` is an f32 figure (`live::signal_figure`).
        assert_eq!(signal_text(f64::from(2.3f32)), "2.3×");
        assert_eq!(signal_text(f64::from(2.9999998f32)), "2.9×");
    }

    /// One or two clipped samples are a stray, not clipping: only three or
    /// more name Clipping and step the gain down 6 dB. Below that a peak
    /// near full scale is only Hot.
    #[test]
    fn three_clipped_samples_make_clipping_and_two_do_not() {
        for clipped in [0, 1, 2] {
            let mut issues = Vec::new();
            assert_eq!(
                level_issues(&mut issues, 1.0, clipped, None),
                (false, false)
            );
            let codes: Vec<_> = issues.iter().map(|i| i.code).collect();
            assert_eq!(codes, [IssueCode::Hot], "{clipped}");
        }
        let mut issues = Vec::new();
        assert_eq!(level_issues(&mut issues, 1.0, 3, None), (false, true));
        assert_eq!(issues[0].code, IssueCode::Clipping);
        assert_eq!(issues[0].title, "Clipping: 3 samples");
        assert_eq!(suggested_gain(1.0, false, true, false), Some(-6.0));
    }

    /// The gain suggested sets the peak to the target, to the nearest half
    /// dB: a peak 7.3 dB under it is told +7.5, one 7.2 dB under +7.0,
    /// and a hot peak of −3 dBFS −7.0.
    #[test]
    fn the_gain_suggested_is_to_the_nearest_half_db() {
        let peak = |dbfs: f64| 10f64.powf(dbfs / 20.0);
        let quiet = |dbfs: f64| suggested_gain(peak(dbfs), false, false, true);
        assert_eq!(quiet(TARGET_PEAK_DBFS - 7.3), Some(7.5));
        assert_eq!(quiet(TARGET_PEAK_DBFS - 7.2), Some(7.0));
        assert_eq!(quiet(TARGET_PEAK_DBFS - 30.6), Some(30.5));
        assert_eq!(suggested_gain(peak(-3.0), false, false, false), Some(-7.0));
        // A good level, or silence, is told nothing.
        assert_eq!(suggested_gain(peak(-10.0), false, false, false), None);
        assert_eq!(suggested_gain(0.0, true, false, true), None);
    }

    /// A flat top is found wherever it is, also in the recording's last
    /// samples, where no smaller sample follows to end it.
    #[test]
    fn a_flat_top_at_the_recordings_end_is_found() {
        let mut x: Vec<f32> = (0..100).map(|i| 0.3 * (i as f32 * 0.37).sin()).collect();
        x.extend([0.6, 0.6, 0.6, 0.6]);
        assert_eq!(clipped_indices(&x, 0.99), [100, 101, 102, 103]);
        // Three equal samples are not a flat top, at the end or not.
        x.pop();
        assert!(clipped_indices(&x, 0.99).is_empty());
    }

    /// The quick look is `analyze`'s reading of the same sound (Norm on
    /// #2095): its rate on the card's clock, and its headline beat error
    /// from the unlock with its source, not a fit of the check's own.
    #[test]
    fn the_quick_look_is_analyzes_reading() {
        use crate::synth::{generate, SynthConfig};
        let a = generate(
            &SynthConfig {
                duration_s: 30.0,
                rate_s_per_day: 12.0,
                beat_error_ms: 0.6,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        );
        let c = check(&a, &DiagnoseConfig::default());
        let s = crate::analysis::analyze(&a, &crate::analysis::AnalysisConfig::default()).summary;
        assert!(c.watch_heard);
        assert_eq!(c.rate_s_per_day, s.rate_s_per_day);
        assert_eq!(c.rate_corrected_s_per_day, s.rate_corrected_s_per_day);
        assert_eq!(c.beat_error_source, Some(BeatErrorSource::Unlock));
        assert_eq!(c.beat_error_ms, s.beat_error_unlock_ms.map(f64::abs));
        assert!(c.issues.is_empty(), "{:?}", c.issues);
    }

    /// Noise alone, as on an empty stand, gives no rate and no beat error,
    /// and the condition is `analyze`'s no regular beat, in its words,
    /// with no count of beats (#2091).
    #[test]
    fn noise_alone_gives_no_readings() {
        let c = check(&white_noise(30), &DiagnoseConfig::default());
        assert!(!c.watch_heard);
        assert_eq!((c.rate_s_per_day, c.beat_error_ms), (None, None));
        assert_eq!(c.rate_corrected_s_per_day, None);
        assert_eq!(c.beat_error_source, None);
        let no_ticks: Vec<_> = c
            .issues
            .iter()
            .filter(|i| i.code == IssueCode::NoTicks)
            .collect();
        assert_eq!(no_ticks.len(), 1, "{:?}", c.issues);
        assert!(
            no_ticks[0].title.starts_with("No regular beat found ("),
            "{}",
            no_ticks[0].title
        );
        assert!(
            !no_ticks[0].evidence.contains("beats found"),
            "{}",
            no_ticks[0].evidence
        );
    }

    /// Under 10 s of noise (5 s was the live doctor's default on an
    /// empty stand) there is no verdict either way (Norm on #2333): no
    /// figures, no "No regular beat" or faint condition, and the
    /// not-assessed fact says the 10 s it needs; never both (Vernon on
    /// #2363, Norm 31 on #2478).
    #[test]
    fn five_seconds_of_noise_awaits_a_verdict() {
        for seconds in [3, 5, 9] {
            awaits_a_verdict(&check(&white_noise(seconds), &DiagnoseConfig::default()));
        }
    }

    /// 5 s of a clean watch waits as noise does: no verdict, no figures,
    /// no faint condition (Norm 31 on #2478).
    #[test]
    fn five_seconds_of_a_watch_awaits_a_verdict() {
        let a = crate::synth::generate(
            &crate::synth::SynthConfig {
                duration_s: 5.0,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        );
        awaits_a_verdict(&check(&a, &DiagnoseConfig::default()));
    }

    /// A check still waiting on `analyze` says only its not-assessed fact
    /// about the beats: no figures, no tick level or signal figure, and
    /// no "No regular beat" or "Ticks too faint" condition (Norm 31).
    fn awaits_a_verdict(c: &SignalCheck) {
        let at = format!("{} s: {:?}", c.duration_s, c.issues);
        assert_eq!((c.rate_s_per_day, c.beat_error_ms), (None, None), "{at}");
        assert_eq!(
            c.not_assessed.as_deref(),
            Some(crate::measure::not_assessed_text(c.duration_s).as_str()),
            "{at}"
        );
        assert!(
            !c.issues.iter().any(|i| i.code == IssueCode::NoTicks),
            "{at}"
        );
        assert_eq!(
            (c.tick_level_dbfs, c.tick_to_noise_db, c.signal_x),
            (None, None, None),
            "{at}"
        );
    }

    /// `seconds` of seeded white noise at 48 kHz, about −41 dBFS RMS.
    fn white_noise(seconds: usize) -> Audio {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let samples = (0..seconds * 48_000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (((state >> 11) as f64 / (1u64 << 53) as f64) - 0.5) as f32 * 0.02
            })
            .collect();
        Audio {
            sample_rate: 48_000,
            samples,
        }
    }

    /// The not-assessed fact is given only where a watch is heard, with no
    /// figures, in under 10 s of sound.
    #[test]
    fn not_assessed_only_for_a_short_watch_with_no_figures() {
        assert_eq!(
            not_assessed(true, false, 5.0).as_deref(),
            Some("Beat regularity not assessed: 5.0 s of sound, 10 s needed")
        );
        assert_eq!(not_assessed(false, false, 5.0), None);
        assert_eq!(not_assessed(true, true, 5.0), None);
        assert_eq!(not_assessed(true, false, 10.0), None);
    }

    /// The quick look's basis words, whole, for every source and clock.
    #[test]
    fn the_quick_look_basis_names_source_and_clock() {
        let card = "on the sound card's clock";
        let fixed = "corrected for the sound card's clock";
        let cases = [
            (
                Some(BeatErrorSource::Unlock),
                false,
                format!("(from the unlock; {card})"),
            ),
            (
                Some(BeatErrorSource::Unlock),
                true,
                format!("(from the unlock; {fixed})"),
            ),
            (
                Some(BeatErrorSource::Drop),
                false,
                format!("(from the drop; {card})"),
            ),
            (
                Some(BeatErrorSource::Drop),
                true,
                format!("(from the drop; {fixed})"),
            ),
            (None, false, format!("({card})")),
            (None, true, format!("({fixed})")),
        ];
        for (source, corrected, want) in cases {
            assert_eq!(quick_look_basis(source, corrected), want);
        }
    }

    /// A beat rate given is the one read, not the one guessed: a 28,800
    /// bph watch checked at 21,600 is read at 21,600.
    #[test]
    fn a_beat_rate_given_is_used() {
        use crate::synth::{generate, SynthConfig};
        let a = generate(
            &SynthConfig {
                duration_s: 12.0,
                bph: 28_800,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        );
        assert_eq!(check(&a, &DiagnoseConfig::default()).bph, 28_800);
        let given = DiagnoseConfig {
            bph: Some(21_600),
            ..Default::default()
        };
        assert_eq!(check(&a, &given).bph, 21_600);
    }

    /// Loud clicks at irregular times, as a stand knocked about: the
    /// signal is well over 3×, so the faint test does not fire, yet no
    /// regular beat is heard: one `no_ticks`, in `analyze`'s words, and
    /// no readings (Vernon on #2363).
    #[test]
    fn loud_irregular_clicks_give_no_readings() {
        let fs = 48_000usize;
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut uniform = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut samples: Vec<f32> = (0..30 * fs)
            .map(|_| ((uniform() - 0.5) * 0.002) as f32)
            .collect();
        let mut t = 0.05;
        while t < 29.9 {
            let i = (t * fs as f64) as usize;
            for k in 0..96 {
                let decay = (-(k as f64) / 20.0).exp();
                samples[i + k] += (0.5 * decay * if k % 2 == 0 { 1.0 } else { -1.0 }) as f32;
            }
            t += 0.08 + 0.25 * uniform();
        }
        let c = check(
            &Audio {
                sample_rate: fs as u32,
                samples,
            },
            &DiagnoseConfig::default(),
        );
        assert!(
            c.signal_x.is_some_and(|x| x >= NO_TICKS_SIGNAL_X),
            "{:?}",
            c.signal_x
        );
        assert!(!c.watch_heard);
        assert_eq!((c.rate_s_per_day, c.beat_error_ms), (None, None));
        let no_ticks: Vec<_> = c
            .issues
            .iter()
            .filter(|i| i.code == IssueCode::NoTicks)
            .collect();
        assert_eq!(no_ticks.len(), 1, "{:?}", c.issues);
        assert!(
            no_ticks[0].title.starts_with("No regular beat found ("),
            "{}",
            no_ticks[0].title
        );
    }

    /// With the USB bus's lines in the sound, the quick look carries
    /// `analyze`'s corrected rate beside the card's.
    #[test]
    fn usb_lines_give_the_corrected_rate() {
        use crate::synth::{generate, SynthConfig};
        let a = generate(
            &SynthConfig {
                duration_s: 120.0,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        );
        let a = Audio {
            samples: crate::edge_takes::with_usb_lines(a.samples, 20.0),
            sample_rate: a.sample_rate,
        };
        let c = check(&a, &DiagnoseConfig::default());
        let s = crate::analysis::analyze(&a, &crate::analysis::AnalysisConfig::default()).summary;
        assert!(c.rate_corrected_s_per_day.is_some());
        assert_eq!(c.rate_corrected_s_per_day, s.rate_corrected_s_per_day);
        assert_ne!(c.rate_corrected_s_per_day, c.rate_s_per_day);
    }

    /// Gaussian noise of standard deviation `sigma`, rectified: the
    /// envelope of pure noise. Seeded, so the test is the same each run.
    fn rectified_noise(n: usize, sigma: f64) -> Vec<f32> {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut uniform = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        (0..n)
            .map(|_| {
                let (u, v) = (uniform().max(1e-300), uniform());
                let g = (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos();
                (sigma * g).abs() as f32
            })
            .collect()
    }

    fn beats_every(beat_s: f64, from: f64, to: f64) -> Vec<beats::Beat> {
        (0..)
            .map(|k| beats::Beat {
                index: k,
                time: from + k as f64 * beat_s,
                quality: 1.0,
            })
            .take_while(|b| b.time < to)
            .collect()
    }

    /// Between the ticks, noise of a known RMS reads as that RMS: the
    /// envelope's rectified mean, √(2/π) of it, is scaled back.
    #[test]
    fn the_background_reads_as_the_noises_rms() {
        let fs = 48_000.0;
        let sigma = 0.01;
        let env = rectified_noise(10 * 48_000, sigma);
        let good = beats_every(0.2, 0.1, 9.5);
        let levels = tick_levels(&env, fs, &good, 0.2);
        assert!(
            (levels.noise / sigma - 1.0).abs() < 0.02,
            "{}",
            levels.noise
        );
        // Fixed gain: the background is as high late in the gap as early.
        assert!(levels.gap_rise.unwrap().abs() < 0.2);
    }

    /// A tick is measured at its peak anywhere from 15 ms before its beat
    /// time to 5 ms after, as its unlock comes before the drop it is timed
    /// by; a louder sound further from it is not the tick.
    #[test]
    fn a_ticks_peak_is_looked_for_from_15_ms_before_its_beat() {
        let fs = 48_000.0;
        let mut env = vec![0.001f32; 10 * 48_000];
        let good = beats_every(0.2, 0.1, 9.5);
        let at = |t: f64| (t * fs).round() as usize;
        for b in &good {
            env[at(b.time - 0.010)] = 0.5;
            env[at(b.time - 0.020)] = 0.9;
            env[at(b.time + 0.008)] = 0.9;
        }
        assert_eq!(tick_levels(&env, fs, &good, 0.2).tick, Some(0.5));
    }
}
