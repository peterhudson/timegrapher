//! Test sessions: one watch measured in several positions, and perhaps at
//! several states of wind, read together the way a multi-position
//! timegrapher (Witschi's SEQ mode) reads them.
//!
//! Each recording gives one [`Reading`]: rate, amplitude and beat error
//! after a settling time, plus any periodic change and the beat shape.
//! [`evaluate`] turns the readings into Witschi's characteristic values
//! (mean rate X, largest difference D, vertical minus horizontal DVH,
//! Di = 6H − CH, isochronism and Im, Ie) and into findings: conditions
//! detected in the sound, each with the figures that show it. The
//! positions and indices follow Witschi, *Testing Methods for Mechanical
//! Watches* (D40e, 18.11.2025). Nothing here judges the watch: tolerances and what a
//! figure means for the movement are for the person, or for the
//! watchmaker skill, to weigh.

use crate::beats::{self, Beat};
use crate::breaks::Break;
use crate::clock::ClockFit;
use crate::dsp::median;
use crate::longrun::LongReport;
use crate::periodicity::Wheel;
use crate::shape::{ShapeReport, SideReport};
use crate::stream::BeatLog;
use crate::timing;
use crate::twostate::{self, TwoState};
use serde::{Deserialize, Serialize, Serializer};

/// A test position, named as Witschi names them: CH dial up, CB dial
/// down, and the vertical positions by the hour mark at the top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Position {
    /// Dial up (DU).
    CH,
    /// Dial down (DD).
    CB,
    /// 9 o'clock up: crown down (CD, PD).
    H9,
    /// 6 o'clock up: crown left (CL, PL).
    H6,
    /// 3 o'clock up: crown up (CU, PU).
    H3,
    /// 12 o'clock up: crown right (CR, PR), as the watch is worn upright.
    H12,
}

const ALIASES: [(Position, &[&str]); 6] = [
    (Position::CH, &["ch", "du", "dialup", "faceup", "fu"]),
    (Position::CB, &["cb", "dd", "dialdown", "facedown", "fd"]),
    (
        Position::H9,
        &["9h", "cd", "pd", "crowndown", "pendantdown", "stemdown"],
    ),
    (
        Position::H6,
        &["6h", "cl", "pl", "crownleft", "pendantleft", "stemleft"],
    ),
    (
        Position::H3,
        &["3h", "cu", "pu", "crownup", "pendantup", "stemup"],
    ),
    (
        Position::H12,
        &["12h", "cr", "pr", "crownright", "pendantright", "stemright"],
    ),
];

impl Position {
    /// In the order Witschi lists them.
    pub const ALL: [Position; 6] = [
        Position::CH,
        Position::CB,
        Position::H9,
        Position::H6,
        Position::H3,
        Position::H12,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Position::CH => "CH",
            Position::CB => "CB",
            Position::H9 => "9H",
            Position::H6 => "6H",
            Position::H3 => "3H",
            Position::H12 => "12H",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Position::CH => "dial up",
            Position::CB => "dial down",
            Position::H9 => "crown down",
            Position::H6 => "crown left",
            Position::H3 => "crown up",
            Position::H12 => "crown right",
        }
    }

    /// The name in words, in Title Case as the app shows it.
    pub fn name(self) -> &'static str {
        match self {
            Position::CH => "Dial Up",
            Position::CB => "Dial Down",
            Position::H9 => "Crown Down",
            Position::H6 => "Crown Left",
            Position::H3 => "Crown Up",
            Position::H12 => "Crown Right",
        }
    }

    /// The name with Witschi's code, e.g. "Crown Up (3H)": the crown sits
    /// at 3 o'clock, so crown up is 3H, crown left 6H, crown down 9H and
    /// crown right 12H.
    pub fn label(self) -> String {
        format!("{} ({})", self.name(), self.code())
    }

    /// The short name most hobby timegraphers use.
    pub fn common(self) -> &'static str {
        match self {
            Position::CH => "DU",
            Position::CB => "DD",
            Position::H9 => "CD",
            Position::H6 => "CL",
            Position::H3 => "CU",
            Position::H12 => "CR",
        }
    }

    pub fn is_vertical(self) -> bool {
        !matches!(self, Position::CH | Position::CB)
    }

    /// Any of Witschi's names, the common ones (DU, DD, CU, CD, CL, CR),
    /// the pendant ones (PU, PD, PL, PR) or the words ("crown left"),
    /// ignoring case, spaces, hyphens and underscores.
    pub fn parse(s: &str) -> Option<Position> {
        let k: String = s
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        ALIASES
            .iter()
            .find(|(_, names)| names.contains(&k.as_str()))
            .map(|(p, _)| *p)
    }

    /// The position named in a file name such as `ym42_DU_48000-01.flac`.
    /// The letter names match in any case; 3H, 6H, 9H and 12H only in
    /// capitals, so that a duration like `2h` or `12h` is not read as one.
    pub fn from_file_name(name: &str) -> Option<Position> {
        let tokens: Vec<&str> = name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| !t.is_empty())
            .collect();
        let letters = tokens.iter().find_map(|t| {
            if t.chars().all(|c| c.is_ascii_alphabetic()) {
                Position::parse(t)
            } else {
                None
            }
        });
        letters.or_else(|| {
            tokens.iter().find_map(|t| match *t {
                "3H" => Some(Position::H3),
                "6H" => Some(Position::H6),
                "9H" => Some(Position::H9),
                "12H" => Some(Position::H12),
                _ => None,
            })
        })
    }
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

impl Serialize for Position {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.code())
    }
}

/// A reading from the watch's own timegrapher (or anyone else's), to set
/// beside this one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    #[serde(alias = "rate")]
    pub rate_s_per_day: Option<f64>,
    #[serde(alias = "amplitude")]
    pub amplitude_deg: Option<f64>,
    #[serde(alias = "beat_error")]
    pub beat_error_ms: Option<f64>,
    /// Where the numbers came from, e.g. "No. 1900, lift 52°".
    pub source: Option<String>,
}

/// Rate, amplitude and beat error over the measured part of a recording.
#[derive(Debug, Clone, Serialize)]
pub struct Measurement {
    /// The measured stretch, seconds from the start of the recording on
    /// the sound card's clock.
    pub start_s: f64,
    pub end_s: f64,
    /// Pauses and resumes within the stretch; rate and beat error are
    /// fitted on each side of them apart.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub breaks: Vec<Break>,
    pub beats: usize,
    /// Share of the expected beats found with a clean match, 0..1.
    pub clean_fraction: f64,
    /// Whether rate is corrected for the sound card's clock.
    pub calibrated: bool,
    pub rate_s_per_day: Option<f64>,
    /// Signed beat error from the drop (the fitted beat times), ms (even
    /// beats late is positive).
    pub beat_error_ms: Option<f64>,
    /// Signed beat error from the unlock, the figure checked against tg's
    /// beat error in regress (median of the amplitude windows), ms.
    pub beat_error_unlock_ms: Option<f64>,
    pub amplitude_deg: Option<f64>,
    pub amplitude_even_deg: Option<f64>,
    pub amplitude_odd_deg: Option<f64>,
    /// Median beat-to-beat timing scatter in 10 s windows, microseconds.
    pub jitter_us: Option<f64>,
    /// Whether a regular beat was found (see [`beats::watch_heard`]).
    /// When false, as on an empty stand, rate, amplitude and beat error
    /// would describe noise, and they are left out; the findings state
    /// [`no_regular_beat`].
    pub watch_heard: bool,
    /// Median of the rate in 10 s windows, s/d. For a watch whose rate
    /// wanders with dips, this sits above `rate_s_per_day` (the fit over
    /// every beat, what the hands keep), as tg's median sits above its
    /// mean; the report headlines the fit and shows this beside it.
    pub rate_window_median_s_per_day: Option<f64>,
    /// Rate in 10 s windows, 5th and 95th percentile, s/d.
    pub rate_p05: Option<f64>,
    pub rate_p95: Option<f64>,
    /// Amplitude in its windows, 5th and 95th percentile, deg.
    pub amplitude_p05: Option<f64>,
    pub amplitude_p95: Option<f64>,
    /// Amplitude windows (2 s) in the measured stretch, and the share of
    /// them that found the unlock on both sides. Amplitude and the beat
    /// error from the unlock rest on those windows.
    pub amplitude_windows: usize,
    pub unlock_coverage: Option<f64>,
    /// Whether amplitude (from the 2 s windows) and rate (from 10 s
    /// readings) sit at two levels the watch switches between.
    pub amplitude_states: TwoState,
    pub rate_states: TwoState,
    /// Beats in the measured stretch with a clipped sample within their
    /// template window, and their share of the beats. Clipping flattens
    /// the loudest part of a tick, which can move amplitude and beat error.
    pub clipped_beats: usize,
    pub clipped_fraction: f64,
    /// A ± and a grade for rate, amplitude and beat error, judged as
    /// `analyze` judges them (see [`crate::measure`]); `None` when no
    /// watch is heard.
    pub confidence: Option<crate::confidence::Confidence>,
    /// What limits how far the readings can be trusted.
    pub caveats: Vec<crate::measure::Caveat>,
}

impl Measurement {
    /// The lift angle was a default, not the calibre's: amplitude is
    /// graded one step lower and the caveat named, as `analyze` does.
    /// [`measure`] takes the lift angle as known.
    pub fn lift_unknown(&mut self) {
        use crate::measure::Caveat;
        if self.caveats.contains(&Caveat::LiftUnknown) {
            return;
        }
        crate::measure::add(&mut self.caveats, Caveat::LiftUnknown);
        let poor = self.caveats.iter().any(|c| {
            matches!(
                c,
                Caveat::SetRateDisagrees { .. } | Caveat::ShortSpan { .. }
            )
        });
        let d = crate::confidence::Doubts {
            clipped: self.clipped_fraction > crate::analysis::Clipping::WARN_FRACTION,
            beats_missing: self
                .caveats
                .iter()
                .any(|c| matches!(c, Caveat::BeatsMissing { .. })),
            lift_unknown: true,
            clock_unknown: !self.calibrated,
        };
        if let Some(c) = &mut self.confidence {
            if !poor {
                c.amplitude = crate::confidence::amplitude(c.amplitude.se, d);
            }
        }
    }

    /// The beat error the report shows: from the unlock where it was
    /// found, else from the drop. Unsigned, ms.
    pub fn beat_error(&self) -> Option<f64> {
        self.beat_error_unlock_ms
            .or(self.beat_error_ms)
            .map(f64::abs)
    }
}

fn percentile(v: &[f64], q: f64) -> Option<f64> {
    let mut w: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if w.is_empty() {
        return None;
    }
    w.sort_by(|a, b| a.total_cmp(b));
    Some(w[((w.len() - 1) as f64 * q).round() as usize])
}

fn median_of(v: impl Iterator<Item = f64>) -> Option<f64> {
    let mut w: Vec<f64> = v.filter(|x| x.is_finite()).collect();
    (!w.is_empty()).then(|| median(&mut w))
}

/// Measure the stretch of `log` from `from_s` to `to_s` (seconds on the
/// sound card's clock; `to_s` past the end means to the end). With a
/// clock fit, beat times are mapped onto true time first.
pub fn measure(log: &BeatLog, clock: Option<&ClockFit>, from_s: f64, to_s: f64) -> Measurement {
    measure_with_breaks(log, clock, &[], from_s, to_s)
}

/// [`measure`] for a take with breaks, whose beats have been renumbered
/// with [`crate::breaks::renumber`]: rate and beat error are fitted per
/// stretch between breaks, and no 10 s window reaches across one.
pub fn measure_with_breaks(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[Break],
    from_s: f64,
    to_s: f64,
) -> Measurement {
    let to_s = to_s.min(log.duration_s);
    let from_s = from_s.clamp(0.0, to_s);
    let map = crate::longrun::true_time(clock, breaks);
    let cuts = crate::longrun::cuts(clock, breaks);
    let apart = |w: &timing::WindowFit| !cuts.iter().any(|&c| w.start_s < c && c < w.end_s);
    let beats: Vec<Beat> = log
        .beats
        .iter()
        .filter(|b| b.time >= from_s && b.time < to_s)
        .map(|b| Beat {
            time: map(b.time),
            ..*b
        })
        .collect();
    let fit = timing::fit_pieces(&beats, log.bph, &cuts);
    let mut windows = timing::windows(&beats, log.bph, 10.0, 5.0);
    windows.retain(apart);
    let rates: Vec<f64> = windows.iter().map(|w| w.fit.rate_s_per_day).collect();
    let rate_samples: Vec<twostate::Sample> = timing::windows(&beats, log.bph, 10.0, 10.0)
        .iter()
        .filter(|w| apart(w))
        .map(|w| twostate::Sample {
            start_s: w.start_s,
            end_s: w.end_s,
            value: w.fit.rate_s_per_day,
            ..Default::default()
        })
        .collect();
    let amp: Vec<_> = log
        .amplitude_windows
        .iter()
        .filter(|w| w.start_s >= from_s && w.end_s <= to_s)
        .collect();
    let amps: Vec<f64> = amp.iter().filter_map(|w| w.mean()).collect();
    let amp_samples: Vec<twostate::Sample> = amp
        .iter()
        .filter_map(|w| {
            Some(twostate::Sample {
                start_s: w.start_s,
                end_s: w.end_s,
                value: w.mean()?,
                tick: w.even_deg,
                tock: w.odd_deg,
                beat_error_unlock_ms: w.beat_error_unlock_ms,
                beat_error_drop_ms: w.beat_error_ms,
                tick_unlock_ms: w.even_unlock_ms,
                tock_unlock_ms: w.odd_unlock_ms,
            })
        })
        .collect();
    let gaps: f64 = breaks
        .iter()
        .filter(|b| b.audio_s > from_s && b.audio_s < to_s)
        .map(|b| b.gap_s)
        .sum();
    let span = map(to_s) - map(from_s) - gaps;
    let expected = span * log.bph as f64 / 3600.0;
    let clean = beats.iter().filter(|b| b.quality > 0.4).count();
    let clipped = log
        .clipped_beats
        .iter()
        .filter(|&&t| t >= from_s && t < to_s)
        .count();
    let jitter_us = median_of(windows.iter().map(|w| w.fit.jitter_us));
    let mut m = Measurement {
        start_s: from_s,
        end_s: to_s,
        breaks: breaks
            .iter()
            .filter(|b| b.audio_s > from_s && b.audio_s < to_s)
            .copied()
            .collect(),
        beats: beats.len(),
        clean_fraction: (clean as f64 / expected.max(1.0)).min(1.0),
        calibrated: clock.is_some(),
        rate_s_per_day: fit.map(|f| f.rate_s_per_day),
        beat_error_ms: fit.map(|f| f.beat_error_ms),
        beat_error_unlock_ms: median_of(amp.iter().filter_map(|w| w.beat_error_unlock_ms)),
        amplitude_deg: median_of(amps.iter().copied()),
        amplitude_even_deg: median_of(amp.iter().filter_map(|w| w.even_deg)),
        amplitude_odd_deg: median_of(amp.iter().filter_map(|w| w.odd_deg)),
        jitter_us,
        watch_heard: beats::watch_heard(jitter_us, log.bph),
        rate_window_median_s_per_day: median_of(rates.iter().copied()),
        rate_p05: percentile(&rates, 0.05),
        rate_p95: percentile(&rates, 0.95),
        amplitude_p05: percentile(&amps, 0.05),
        amplitude_p95: percentile(&amps, 0.95),
        amplitude_windows: amp.len(),
        unlock_coverage: (!amp.is_empty()).then(|| {
            amp.iter()
                .filter(|w| w.beat_error_unlock_ms.is_some())
                .count() as f64
                / amp.len() as f64
        }),
        amplitude_states: twostate::find(&amp_samples, &twostate::Params::default()),
        rate_states: twostate::find(&rate_samples, &twostate::Params::default()),
        clipped_beats: clipped,
        clipped_fraction: if beats.is_empty() {
            0.0
        } else {
            clipped as f64 / beats.len() as f64
        },
        confidence: None,
        caveats: Vec::new(),
    };
    let a = crate::measure::assess(
        &windows,
        &amp,
        &crate::measure::Context {
            watch_heard: m.watch_heard,
            bph_heard: None,
            lift_known: true,
            clock_known: m.calibrated,
            beats: beats.len(),
            expected,
            clipped_fraction: m.clipped_fraction,
            span_s: match (beats.first(), beats.last()) {
                (Some(a), Some(b)) => b.time - a.time - gaps,
                _ => 0.0,
            },
            listened_s: to_s - from_s,
            breaks: m.breaks.len(),
            average_s: None,
        },
    );
    m.confidence = a.confidence;
    m.caveats = a.caveats;
    if !m.watch_heard {
        m.rate_s_per_day = None;
        m.beat_error_ms = None;
        m.beat_error_unlock_ms = None;
        m.amplitude_deg = None;
        m.amplitude_even_deg = None;
        m.amplitude_odd_deg = None;
        m.rate_window_median_s_per_day = None;
        m.rate_p05 = None;
        m.rate_p95 = None;
        m.amplitude_p05 = None;
        m.amplitude_p95 = None;
        m.unlock_coverage = None;
        let none = twostate::find(&[], &twostate::Params::default());
        m.amplitude_states = none.clone();
        m.rate_states = none;
    }
    m
}

/// A clock fit for a sound card known to be `ppm` slow (negative: fast),
/// for recordings made without a clock log on a card calibrated before.
pub fn fixed_clock(ppm: f64, duration_s: f64) -> Option<ClockFit> {
    Some(ClockFit::fixed(ppm, duration_s))
}

/// A periodic change found in one recording.
#[derive(Debug, Clone, Serialize)]
pub struct Cycle {
    /// "rate" or "amplitude".
    pub series: &'static str,
    pub period_s: f64,
    /// Peak to peak, in `size_unit`: degrees for amplitude; for rate, s/d,
    /// or the timing swing in ms for a cycle shorter than
    /// [`crate::longrun::TIMING_UNIT_BELOW_S`], which a rate reading cannot
    /// follow (see [`crate::longrun::RateComponent::size`]).
    pub size: f64,
    /// "s/d", "ms" or "deg".
    pub size_unit: &'static str,
    /// -log10 of the false-alarm probability.
    pub significance: f64,
    pub explained: f64,
    pub wheel: Option<String>,
    pub nearest_wheel: Option<(String, f64)>,
    /// A wheel whose turn is a whole number of these periods, and the
    /// number: a short, sharp change once per turn also shows here.
    pub fraction_of: Option<(String, u32)>,
}

impl Cycle {
    pub fn unit(&self) -> &'static str {
        self.size_unit
    }

    /// The size with its unit, e.g. "10.9 s/d" or "0.052 ms".
    pub fn size_text(&self) -> String {
        match self.size_unit {
            "ms" => format!("{:.3} ms", self.size),
            u => format!("{:.1} {u}", self.size),
        }
    }
}

/// The periodic changes `long` found, without their long arrays.
/// `wheels` are the turn periods searched, used to mark a component at a
/// whole fraction of a wheel's turn.
pub fn cycles(r: &LongReport, wheels: &[Wheel]) -> Vec<Cycle> {
    let fraction = |period: f64, wheel: &Option<String>| -> Option<(String, u32)> {
        if wheel.is_some() {
            return None;
        }
        wheels.iter().find_map(|w| {
            let n = w.period_s / period;
            (n > 1.5 && n < 6.5 && (n - n.round()).abs() < 0.01 * n)
                .then(|| (w.name.clone(), n.round() as u32))
        })
    };
    let rate = r.rate_components.iter().map(|c| Cycle {
        series: "rate",
        period_s: c.component.period_s,
        size: c.size().0,
        size_unit: c.size().1,
        significance: c.component.significance,
        explained: c.component.explained,
        wheel: c.component.wheel.clone(),
        nearest_wheel: c.component.nearest_wheel.clone(),
        fraction_of: fraction(c.component.period_s, &c.component.wheel),
    });
    let amp = r.amplitude.components.iter().map(|c| Cycle {
        series: "amplitude",
        period_s: c.period_s,
        size: c.peak_to_peak,
        size_unit: "deg",
        significance: c.significance,
        explained: c.explained,
        wheel: c.wheel.clone(),
        nearest_wheel: c.nearest_wheel.clone(),
        fraction_of: fraction(c.period_s, &c.wheel),
    });
    rate.chain(amp).collect()
}

/// One pallet stone's beat shape, from `shape`.
#[derive(Debug, Clone, Serialize)]
pub struct ShapeSide {
    pub i12_ms: Option<f64>,
    pub i13_ms: Option<f64>,
    pub ratio13: Option<f64>,
    pub ratio23: Option<f64>,
    pub noise_ratio: Option<f64>,
    /// Share of the windows in which a distinct unlock (sound 1) was found.
    pub sound1_found: f64,
    /// Extra sounds before the unlock and after the drop on the
    /// whole-recording template: (ms from the unlock edge, level of the drop).
    pub extra_pre: Vec<(f64, f64)>,
    pub extra_post: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShapeSummary {
    /// Seconds of audio the shape was measured on.
    pub measured_s: f64,
    pub even: ShapeSide,
    pub odd: ShapeSide,
}

fn shape_side(s: &SideReport) -> ShapeSide {
    let w = &s.windows;
    let ev = |l: Option<&Vec<crate::shape::ExtraEvent>>| -> Vec<(f64, f64)> {
        l.map(|v| v.iter().map(|e| (e.t_ms, e.level)).collect())
            .unwrap_or_default()
    };
    ShapeSide {
        i12_ms: w.i12_ms,
        i13_ms: w.i13_ms,
        ratio13: w.ratio13,
        ratio23: w.ratio23,
        noise_ratio: w.noise_ratio,
        sound1_found: s.windows_with_1 as f64 / s.windows_measured.max(1) as f64,
        extra_pre: ev(s.whole.as_ref().map(|w| &w.extra_pre)),
        extra_post: ev(s.whole.as_ref().map(|w| &w.extra_post)),
    }
}

pub fn shape_summary(r: &ShapeReport, measured_s: f64) -> ShapeSummary {
    ShapeSummary {
        measured_s,
        even: shape_side(&r.even),
        odd: shape_side(&r.odd),
    }
}

/// Everything learnt from one recording.
#[derive(Debug, Clone, Serialize)]
pub struct Reading {
    /// The recording's name, as shown in the report.
    pub label: String,
    pub position: Position,
    /// Hours since the watch was fully wound (`None`: not given, taken as full).
    pub wind_h: Option<f64>,
    pub date: Option<String>,
    pub notes: Option<String>,
    pub duration_s: f64,
    pub bph: u32,
    pub measurement: Measurement,
    pub cycles: Vec<Cycle>,
    pub shape: Option<ShapeSummary>,
    pub reference: Option<Reference>,
}

/// One position at one state of wind (repeat readings averaged).
#[derive(Debug, Clone, Serialize)]
pub struct PositionValue {
    pub position: Position,
    pub rate_s_per_day: Option<f64>,
    pub amplitude_deg: Option<f64>,
    /// Unsigned, from the unlock where found, else from the drop.
    pub beat_error_ms: Option<f64>,
    /// Readings averaged into this value.
    pub readings: usize,
}

/// Witschi's characteristic values for one state of wind.
#[derive(Debug, Clone, Serialize)]
pub struct StateIndices {
    pub wind_h: f64,
    pub positions: Vec<PositionValue>,
    /// Mean rate over the positions, and over the horizontal and the
    /// vertical ones.
    pub x: Option<f64>,
    pub xh: Option<f64>,
    pub xv: Option<f64>,
    /// Largest difference between positions, rate and amplitude, over all,
    /// vertical only and horizontal only.
    pub d_rate: Option<f64>,
    pub d_amplitude: Option<f64>,
    pub dv_rate: Option<f64>,
    pub dh_rate: Option<f64>,
    /// Vertical mean minus horizontal mean.
    pub dvh_rate: Option<f64>,
    pub dvh_amplitude: Option<f64>,
    /// 6H minus CH.
    pub di: Option<f64>,
    pub amplitude_mean: Option<f64>,
}

/// How one position's rate and amplitude change as the watch runs down.
#[derive(Debug, Clone, Serialize)]
pub struct Isochronism {
    pub position: Position,
    pub from_wind_h: f64,
    pub to_wind_h: f64,
    pub rate_change: f64,
    pub amplitude_change: Option<f64>,
    /// Change in the unsigned beat error (from the unlock where found,
    /// else from the drop), ms.
    pub beat_error_change: Option<f64>,
}

/// How a steadiness finding is ranked (see [`crate::steadiness`]). The
/// session's own findings carry none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Fault,
    Warning,
    Note,
}

/// A condition detected in one recording's sound, with the figures that
/// show it. A finding states what was measured; it does not say what it
/// means for the watch.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    /// Stable identifier of the condition, for programs and agents
    /// reading the report (e.g. "clipping"); the title is for people.
    pub code: &'static str,
    /// Index into the readings, for a finding about one recording.
    pub recording: Option<usize>,
    pub title: String,
    /// The figures, and the recording they come from.
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionReport {
    /// One per state of wind, fullest first.
    pub states: Vec<StateIndices>,
    pub isochronism: Vec<Isochronism>,
    /// Largest isochronism over the positions excluding 12H (NIHS 93-10),
    /// and over all positions, signed, s/d.
    pub im: Option<f64>,
    pub im_all: Option<f64>,
    /// Change of the mean rate X from the fullest to the least wound state,
    /// over the positions measured in both, s/d.
    pub ie: Option<f64>,
    pub findings: Vec<Finding>,
}

fn mean(v: impl Iterator<Item = f64>) -> Option<f64> {
    let w: Vec<f64> = v.filter(|x| x.is_finite()).collect();
    (!w.is_empty()).then(|| w.iter().sum::<f64>() / w.len() as f64)
}

fn spread(v: impl Iterator<Item = f64>) -> Option<f64> {
    let w: Vec<f64> = v.filter(|x| x.is_finite()).collect();
    if w.len() < 2 {
        return None;
    }
    let lo = w.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = w.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Some(hi - lo)
}

fn wind_key(w: Option<f64>) -> i64 {
    (w.unwrap_or(0.0) * 10.0).round() as i64
}

fn state(wind_h: f64, readings: &[&Reading]) -> StateIndices {
    let positions: Vec<PositionValue> = Position::ALL
        .iter()
        .filter_map(|&p| {
            let rs: Vec<&&Reading> = readings.iter().filter(|r| r.position == p).collect();
            if rs.is_empty() {
                return None;
            }
            Some(PositionValue {
                position: p,
                rate_s_per_day: mean(rs.iter().filter_map(|r| r.measurement.rate_s_per_day)),
                amplitude_deg: mean(rs.iter().filter_map(|r| r.measurement.amplitude_deg)),
                beat_error_ms: mean(rs.iter().filter_map(|r| r.measurement.beat_error())),
                readings: rs.len(),
            })
        })
        .collect();
    let rate = |pred: &dyn Fn(Position) -> bool| -> Vec<f64> {
        positions
            .iter()
            .filter(|v| pred(v.position))
            .filter_map(|v| v.rate_s_per_day)
            .collect()
    };
    let amp = |pred: &dyn Fn(Position) -> bool| -> Vec<f64> {
        positions
            .iter()
            .filter(|v| pred(v.position))
            .filter_map(|v| v.amplitude_deg)
            .collect()
    };
    let all = |_: Position| true;
    let vert = |p: Position| p.is_vertical();
    let hor = |p: Position| !p.is_vertical();
    let xh = mean(rate(&hor).into_iter());
    let xv = mean(rate(&vert).into_iter());
    let ah = mean(amp(&hor).into_iter());
    let av = mean(amp(&vert).into_iter());
    let get = |p: Position| {
        positions
            .iter()
            .find(|v| v.position == p)
            .and_then(|v| v.rate_s_per_day)
    };
    StateIndices {
        wind_h,
        x: mean(rate(&all).into_iter()),
        xh,
        xv,
        d_rate: spread(rate(&all).into_iter()),
        d_amplitude: spread(amp(&all).into_iter()),
        dv_rate: spread(rate(&vert).into_iter()),
        dh_rate: spread(rate(&hor).into_iter()),
        dvh_rate: xv.zip(xh).map(|(v, h)| v - h),
        dvh_amplitude: av.zip(ah).map(|(v, h)| v - h),
        di: get(Position::H6).zip(get(Position::CH)).map(|(a, b)| a - b),
        amplitude_mean: mean(amp(&all).into_iter()),
        positions,
    }
}

fn largest(v: impl Iterator<Item = f64>) -> Option<f64> {
    v.max_by(|a, b| a.abs().total_cmp(&b.abs()))
}

/// Witschi's characteristic values for the readings, and the conditions
/// found in each recording.
pub fn evaluate(readings: &[Reading]) -> SessionReport {
    // States of wind, fullest first.
    let mut keys: Vec<i64> = readings.iter().map(|r| wind_key(r.wind_h)).collect();
    keys.sort_unstable();
    keys.dedup();
    let states: Vec<StateIndices> = keys
        .iter()
        .map(|&k| {
            let rs: Vec<&Reading> = readings
                .iter()
                .filter(|r| wind_key(r.wind_h) == k)
                .collect();
            state(k as f64 / 10.0, &rs)
        })
        .collect();

    // Isochronism: each position from the fullest to the least wound
    // state it was measured in.
    let mut isochronism = Vec::new();
    for p in Position::ALL {
        let at: Vec<(&StateIndices, &PositionValue)> = states
            .iter()
            .filter_map(|s| {
                s.positions
                    .iter()
                    .find(|v| v.position == p && v.rate_s_per_day.is_some())
                    .map(|v| (s, v))
            })
            .collect();
        if let (Some(a), Some(b)) = (at.first(), at.last()) {
            if at.len() >= 2 {
                isochronism.push(Isochronism {
                    position: p,
                    from_wind_h: a.0.wind_h,
                    to_wind_h: b.0.wind_h,
                    rate_change: b.1.rate_s_per_day.unwrap() - a.1.rate_s_per_day.unwrap(),
                    amplitude_change: b.1.amplitude_deg.zip(a.1.amplitude_deg).map(|(x, y)| x - y),
                    beat_error_change: b.1.beat_error_ms.zip(a.1.beat_error_ms).map(|(x, y)| x - y),
                });
            }
        }
    }
    let im = largest(
        isochronism
            .iter()
            .filter(|i| i.position != Position::H12)
            .map(|i| i.rate_change),
    );
    let im_all = largest(isochronism.iter().map(|i| i.rate_change));
    let ie = match (states.first(), states.last()) {
        (Some(a), Some(b)) if states.len() >= 2 => {
            let common = |s: &StateIndices, o: &StateIndices| {
                mean(
                    s.positions
                        .iter()
                        .filter(|v| o.positions.iter().any(|w| w.position == v.position))
                        .filter_map(|v| v.rate_s_per_day),
                )
            };
            common(a, b).zip(common(b, a)).map(|(x, y)| (y - x).abs())
        }
        _ => None,
    };
    let findings = findings(readings);
    SessionReport {
        states,
        isochronism,
        im,
        im_all,
        ie,
        findings,
    }
}

/// Below this share of amplitude windows that found the unlock on both
/// sides, the share is listed among the findings.
const UNLOCK_COVERAGE_LISTED: f64 = 0.6;

/// Below this many seconds measured after settling, the length is listed
/// among the findings: the "≥40 s measurement time" of Witschi's
/// *Testing Methods for Mechanical Watches* (D40e, 2025, measurement tips).
const SHORT_MEASURE_S: f64 = 40.0;

/// The sentence for a recording with no regular beat, e.g. "No regular
/// beat found (timing scatter 8.6 ms, 2.4% of a beat)", as every surface
/// states it.
pub fn no_regular_beat(jitter_us: Option<f64>, bph: u32) -> String {
    match jitter_us.filter(|j| j.is_finite()) {
        Some(j) => format!(
            "No regular beat found (timing scatter {:.1} ms, {:.1}% of a beat)",
            j / 1000.0,
            100.0 * j * 1e-6 * bph as f64 / 3600.0
        ),
        None => "No regular beat found".into(),
    }
}

fn findings(readings: &[Reading]) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut push = |code: &'static str, recording: usize, title: String, evidence: String| {
        out.push(Finding {
            code,
            recording: Some(recording),
            title,
            evidence,
        })
    };
    let at = |r: &Reading| {
        let w = r
            .wind_h
            .filter(|&w| w > 0.0)
            .map_or(String::new(), |w| format!(", {w:.0} h after winding"));
        let name = r
            .label
            .rsplit(['/', '\\'])
            .find(|s| !s.is_empty())
            .unwrap_or(&r.label);
        format!("{} ({name}{w})", r.position.label())
    };

    for (ri, r) in readings.iter().enumerate() {
        let m = &r.measurement;
        if !m.watch_heard {
            push(
                "no_regular_beat",
                ri,
                no_regular_beat(m.jitter_us, r.bph),
                format!("in {}; rate, amplitude and beat error are not shown", at(r)),
            );
            continue;
        }
        if let Some(c) = m.unlock_coverage.filter(|&c| c < UNLOCK_COVERAGE_LISTED) {
            let sides = match (m.amplitude_even_deg, m.amplitude_odd_deg) {
                (Some(e), Some(o)) => format!("; Tick and Tock read {e:.0}° and {o:.0}°"),
                _ => String::new(),
            };
            push(
                "unlock_unreliable",
                ri,
                format!("Unlock found in {:.0}% of windows", 100.0 * c),
                format!(
                    "{:.0}% of the {} amplitude windows in {} found the unlock on both sides; amplitude and the beat error from the unlock rest on those windows{sides}",
                    100.0 * c,
                    m.amplitude_windows,
                    at(r),
                ),
            );
        }
        if m.clipped_fraction > crate::analysis::Clipping::WARN_FRACTION {
            push(
                "clipping",
                ri,
                format!("Clipped on {:.1}% of beats", 100.0 * m.clipped_fraction),
                format!(
                    "{} of the {} beats in {} have a clipped sample",
                    m.clipped_beats,
                    m.beats,
                    at(r),
                ),
            );
        }
        let measured = m.end_s - m.start_s;
        if measured < SHORT_MEASURE_S {
            push(
                "measurement_short",
                ri,
                format!("{measured:.0} s measured"),
                format!("{measured:.0} s measured after settling in {}", at(r)),
            );
        }
        // A sharp event once per turn also shows at whole fractions of
        // the turn; those are reported with the wheel, not on their own.
        let harmonic_of = |c: &Cycle| {
            r.cycles.iter().find(|w| {
                let n = w.period_s / c.period_s;
                w.wheel.is_some()
                    && w.series == c.series
                    && n > 1.5
                    && (n - n.round()).abs() < 0.01 * n
            })
        };
        for c in &r.cycles {
            let size = format!("{} peak to peak", c.size_text());
            let fa = 10f64.powf(-c.significance.min(300.0));
            if c.wheel.is_none() && harmonic_of(c).is_some() {
                continue;
            }
            let harmonics: Vec<String> = r
                .cycles
                .iter()
                .filter(|h| h.wheel.is_none() && harmonic_of(h).is_some_and(|w| std::ptr::eq(w, c)))
                .map(|h| format!("{:.2} s", h.period_s))
                .collect();
            let harmonics = if harmonics.is_empty() {
                String::new()
            } else {
                format!(
                    "; also at whole fractions of the turn ({})",
                    harmonics.join(", ")
                )
            };
            match &c.wheel {
                Some(w) => push(
                    "cycle_wheel",
                    ri,
                    format!("Regular {} change once per turn of the {w}", c.series),
                    format!(
                        "{} repeats every {:.2} s ({size}, {:.0}% of the variation) in {}; false-alarm chance {fa:.0e}{harmonics}",
                        c.series, c.period_s, c.explained * 100.0, at(r)
                    ),
                ),
                None if c.fraction_of.is_some() => {
                    let (w, n) = c.fraction_of.clone().unwrap();
                    push(
                        "cycle_wheel_fraction",
                        ri,
                        format!("Regular {} change at 1/{n} of the {w}'s turn", c.series),
                        format!(
                            "{} repeats every {:.2} s ({size}) in {}; false-alarm chance {fa:.0e}",
                            c.series,
                            c.period_s,
                            at(r)
                        ),
                    )
                }
                None => push(
                    "cycle_other",
                    ri,
                    format!("Regular {} change not tied to a listed wheel", c.series),
                    format!(
                        "{} repeats every {:.2} s ({size}) in {}; false-alarm chance {fa:.0e}{}",
                        c.series,
                        c.period_s,
                        at(r),
                        c.nearest_wheel
                            .as_ref()
                            .map_or(String::new(), |(w, off)| format!("; nearest is the {w}, {off:+.1}% off"))
                    ),
                ),
            }
        }
        if let Some(sh) = &r.shape {
            for (side, v) in [("Tick", &sh.even), ("Tock", &sh.odd)] {
                if let Some(q) = v.ratio13.filter(|&q| q >= 1.0) {
                    push(
                        "shape_unlock_loud",
                        ri,
                        format!("Unlocking as loud as the drop on {side}"),
                        format!(
                            "level of sound 1 to sound 3 is {q:.2} on {side} beats in {}",
                            at(r)
                        ),
                    );
                }
                let extra = v.extra_pre.len() + v.extra_post.len();
                if extra > 0 {
                    push(
                        "shape_extra_sounds",
                        ri,
                        format!("Extra sounds around the beat on {side}"),
                        format!(
                            "{} before the unlock and {} after the drop on {side} beats in {}",
                            v.extra_pre.len(),
                            v.extra_post.len(),
                            at(r)
                        ),
                    );
                }
            }
        }
    }
    out
}

/// What a session file describes: the watch and its recordings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub watch: Option<String>,
    pub calibre: Option<String>,
    pub owner: Option<String>,
    pub bph: Option<u32>,
    #[serde(alias = "lift_angle")]
    pub lift: Option<f64>,
    pub escape_teeth: Option<u32>,
    /// More wheels to name in the cycle search, as name = seconds per
    /// turn, e.g. { "third wheel" = 450.0 }.
    #[serde(default)]
    pub wheels: std::collections::BTreeMap<String, f64>,
    /// Read and ignored: session files written before schema /2 named a
    /// tolerance class here.
    #[serde(default, skip_serializing)]
    pub tolerance: Option<serde::de::IgnoredAny>,
    /// Seconds to skip at the start of each recording while the watch
    /// settles after a position change (Witschi: 20 s).
    pub settle_s: Option<f64>,
    /// Seconds to measure after settling (default: the rest of the recording).
    pub measure_s: Option<f64>,
    /// A known sound-card error for recordings with no clock log: ppm slow
    /// (negative: fast).
    pub card_ppm: Option<f64>,
    /// Run the beat-shape measurement on each recording (default true).
    pub shape: Option<bool>,
    /// Run the periodic-change search on each recording (default true).
    pub cycles: Option<bool>,
    pub notes: Option<String>,
    /// Read and ignored, as `tolerance` is.
    #[serde(default, skip_serializing)]
    pub limits: Option<serde::de::IgnoredAny>,
    #[serde(default, rename = "recording")]
    pub recordings: Vec<RecordingEntry>,
}

/// One recording in a session file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordingEntry {
    /// A WAV or FLAC file, or a folder of segment files read as one
    /// recording, relative to the session file.
    pub file: String,
    /// Position name, e.g. "CH", "DU", "6H", "crown left". Guessed from
    /// the file name when left out.
    pub position: Option<String>,
    /// Hours since the watch was fully wound; 0 or left out means full.
    pub wind_h: Option<f64>,
    pub date: Option<String>,
    /// Clock log for this recording (see docs/long-runs.md).
    pub clock: Option<String>,
    /// Start of this reading, seconds into the file or folder; left out
    /// means its start. With `end_s`, one continuous take that moved
    /// through several positions is listed once per position.
    pub start_s: Option<f64>,
    /// End of this reading, seconds into the file or folder; left out
    /// means its end.
    pub end_s: Option<f64>,
    pub settle_s: Option<f64>,
    pub measure_s: Option<f64>,
    pub notes: Option<String>,
    pub reference: Option<Reference>,
    /// A fault confirmed at the bench, on a corpus holdout take:
    /// `fault = { id = "unlock_too_strong", side = "Tick", source = "bench: ..." }`.
    /// Left out, the recording is of a healthy watch.
    pub fault: Option<KnownFault>,
}

/// A fault a recording is known to show, and how it was confirmed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownFault {
    /// One of the fault ids `analyze` reports in `faults[]`.
    pub id: String,
    /// Tick, Tock or both; left out, either side counts.
    pub side: Option<String>,
    /// How the fault was confirmed, e.g. "bench: ...".
    pub source: Option<String>,
}
