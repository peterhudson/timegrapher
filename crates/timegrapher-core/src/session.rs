//! Test sessions: one watch measured in several positions, and perhaps at
//! several states of wind, read together the way a multi-position
//! timegrapher (Witschi's SEQ mode) reads them.
//!
//! Each recording gives one [`Reading`]: rate, amplitude and beat error
//! after a settling time, plus any periodic change and the beat shape.
//! [`evaluate`] turns the readings into Witschi's characteristic values
//! (mean rate X, largest difference D, vertical minus horizontal DVH,
//! Di = 6H − CH, isochronism and Im, Ie) and into findings, each with the
//! evidence that triggered it. The positions, indices and tolerances
//! follow Witschi, *Testing methods for mechanical watches* (2025); the
//! thresholds Witschi does not give are this project's defaults, in
//! [`Limits`], and say so in their findings.

use crate::beats::Beat;
use crate::clock::ClockFit;
use crate::dsp::median;
use crate::longrun::LongReport;
use crate::periodicity::Wheel;
use crate::shape::{ShapeReport, SideReport};
use crate::stream::BeatLog;
use crate::timing;
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

/// Witschi's tolerances for a fully wound watch (1.9 in the project's
/// notes on the testing-methods document).
#[derive(Debug, Clone, Serialize)]
pub struct Tolerance {
    pub name: String,
    pub rate_min: f64,
    pub rate_max: f64,
    /// Amplitude range in the horizontal and vertical positions, degrees.
    pub amplitude_h: (f64, f64),
    pub amplitude_v: (f64, f64),
    pub beat_error_ms: f64,
}

impl Tolerance {
    /// `ladies`, `mens`, `cosc-small` (movement under 20 mm), `cosc` or
    /// `cosc-large`, and `metas`.
    pub fn named(name: &str) -> Option<Tolerance> {
        let k: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        let (label, lo, hi) = match k.as_str() {
            "ladies" | "lady" | "small" => ("Ladies'", -5.0, 25.0),
            "mens" | "men" | "standard" => ("Men's", -5.0, 15.0),
            "coscsmall" | "cosc20" => ("COSC, movement under 20 mm", -5.0, 8.0),
            "cosc" | "cosclarge" => ("COSC, movement over 20 mm", -4.0, 6.0),
            "metas" => ("METAS", 0.0, 5.0),
            _ => return None,
        };
        Some(Tolerance {
            name: label.into(),
            rate_min: lo,
            rate_max: hi,
            amplitude_h: (260.0, 320.0),
            amplitude_v: (240.0, 280.0),
            beat_error_ms: 0.5,
        })
    }

    pub fn amplitude(&self, p: Position) -> (f64, f64) {
        if p.is_vertical() {
            self.amplitude_v
        } else {
            self.amplitude_h
        }
    }
}

impl Default for Tolerance {
    fn default() -> Self {
        Tolerance::named("mens").expect("built in")
    }
}

/// Thresholds for findings that Witschi names without a number. These
/// are this project's defaults; a session file can change them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Largest rate difference between positions before it is a finding, s/d.
    pub delta_rate: f64,
    /// Amplitude loss from horizontal to vertical before it is a finding, deg.
    pub vh_amplitude_drop: f64,
    /// Beat error at which it is a fault rather than out of tolerance, ms.
    pub beat_error_fault: f64,
    /// Amplitude below which the movement needs service, deg.
    pub amplitude_fault: f64,
    /// Amplitude above which the balance can knock (Witschi: 330°), deg.
    pub overbanking: f64,
    /// Spread of the 10 s rate readings (5th to 95th percentile) before
    /// the rate counts as unsteady, s/d.
    pub rate_spread: f64,
    /// Shortest measurement Witschi recommends per position, s.
    pub min_measure_s: f64,
    /// Readings taken this soon after a full wind count as fully wound
    /// for the tolerance checks, hours.
    pub full_wind_h: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            delta_rate: 10.0,
            vh_amplitude_drop: 50.0,
            beat_error_fault: 2.0,
            amplitude_fault: 200.0,
            overbanking: 330.0,
            rate_spread: 20.0,
            min_measure_s: 40.0,
            full_wind_h: 2.0,
        }
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
    pub beats: usize,
    /// Share of the expected beats found with a clean match, 0..1.
    pub clean_fraction: f64,
    /// Whether rate is corrected for the sound card's clock.
    pub calibrated: bool,
    pub rate_s_per_day: Option<f64>,
    /// Signed beat error, ms (even beats late is positive).
    pub beat_error_ms: Option<f64>,
    pub amplitude_deg: Option<f64>,
    pub amplitude_even_deg: Option<f64>,
    pub amplitude_odd_deg: Option<f64>,
    /// Median beat-to-beat timing scatter in 10 s windows, microseconds.
    pub jitter_us: Option<f64>,
    /// Rate in 10 s windows, 5th and 95th percentile, s/d.
    pub rate_p05: Option<f64>,
    pub rate_p95: Option<f64>,
    /// Amplitude in its windows, 5th and 95th percentile, deg.
    pub amplitude_p05: Option<f64>,
    pub amplitude_p95: Option<f64>,
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
    let to_s = to_s.min(log.duration_s);
    let from_s = from_s.clamp(0.0, to_s);
    let map = |t: f64| clock.map_or(t, |c| c.map(t));
    let beats: Vec<Beat> = log
        .beats
        .iter()
        .filter(|b| b.time >= from_s && b.time < to_s)
        .map(|b| Beat {
            time: map(b.time),
            ..*b
        })
        .collect();
    let fit = timing::fit(&beats, log.bph);
    let windows = timing::windows(&beats, log.bph, 10.0, 5.0);
    let rates: Vec<f64> = windows.iter().map(|w| w.fit.rate_s_per_day).collect();
    let amp: Vec<_> = log
        .amplitude_windows
        .iter()
        .filter(|w| w.start_s >= from_s && w.end_s <= to_s)
        .collect();
    let amps: Vec<f64> = amp.iter().filter_map(|w| w.mean()).collect();
    let span = map(to_s) - map(from_s);
    let expected = span * log.bph as f64 / 3600.0;
    let clean = beats.iter().filter(|b| b.quality > 0.4).count();
    Measurement {
        start_s: from_s,
        end_s: to_s,
        beats: beats.len(),
        clean_fraction: (clean as f64 / expected.max(1.0)).min(1.0),
        calibrated: clock.is_some(),
        rate_s_per_day: fit.map(|f| f.rate_s_per_day),
        beat_error_ms: fit.map(|f| f.beat_error_ms),
        amplitude_deg: median_of(amps.iter().copied()),
        amplitude_even_deg: median_of(amp.iter().filter_map(|w| w.even_deg)),
        amplitude_odd_deg: median_of(amp.iter().filter_map(|w| w.odd_deg)),
        jitter_us: median_of(windows.iter().map(|w| w.fit.jitter_us)),
        rate_p05: percentile(&rates, 0.05),
        rate_p95: percentile(&rates, 0.95),
        amplitude_p05: percentile(&amps, 0.05),
        amplitude_p95: percentile(&amps, 0.95),
    }
}

/// A clock fit for a sound card known to be `ppm` slow (negative: fast),
/// for recordings made without a clock log on a card calibrated before.
pub fn fixed_clock(ppm: f64, duration_s: f64) -> Option<ClockFit> {
    let k = 1.0 + ppm * 1e-6;
    let d = duration_s.max(3.0);
    let pairs = [(0.0, 0.0), (d / 2.0, d / 2.0 * k), (d, d * k)];
    ClockFit::new(&pairs).ok()
}

/// A periodic change found in one recording.
#[derive(Debug, Clone, Serialize)]
pub struct Cycle {
    /// "rate" or "amplitude".
    pub series: &'static str,
    pub period_s: f64,
    /// Peak to peak, in s/d for rate and degrees for amplitude.
    pub size: f64,
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
        if self.series == "rate" {
            "s/d"
        } else {
            "deg"
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
            (n > 1.5 && (n - n.round()).abs() < 0.01 * n)
                .then(|| (w.name.clone(), n.round() as u32))
        })
    };
    let rate = r.rate_components.iter().map(|c| Cycle {
        series: "rate",
        period_s: c.component.period_s,
        size: c.rate_swing_s_per_day,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Fault,
    Warning,
    Note,
}

/// Something worth the watchmaker's attention, with what triggered it.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    /// Stable identifier of the rule, for programs and agents reading the
    /// report (e.g. "positional_delta"); the title is for people.
    pub code: &'static str,
    /// Index into the readings, for a finding about one recording.
    pub recording: Option<usize>,
    pub severity: Severity,
    pub title: String,
    /// The measurement that triggered it, with the threshold.
    pub evidence: String,
    /// What Witschi recommends, or what to check.
    pub advice: String,
}

/// Whether a value is within tolerance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    Within,
    Outside,
    /// Not judged: not measured, or not fully wound.
    NotJudged,
}

#[derive(Debug, Clone, Serialize)]
pub struct Verdict {
    pub rate: Mark,
    pub amplitude: Mark,
    pub beat_error: Mark,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionReport {
    pub tolerance: Tolerance,
    pub limits: Limits,
    /// One per reading, in the readings' order.
    pub verdicts: Vec<Verdict>,
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
    /// Witschi's quality factor N = 0.15·|Im| + 0.1·Pmax + C, with the
    /// thermal coefficient C left at Witschi's default of 0.6.
    pub n: Option<f64>,
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
                beat_error_ms: mean(
                    rs.iter()
                        .filter_map(|r| r.measurement.beat_error_ms.map(f64::abs)),
                ),
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

fn s(v: f64) -> String {
    format!("{v:+.1} s/d")
}

/// Judge the readings against `tol` and `limits`.
pub fn evaluate(readings: &[Reading], tol: &Tolerance, limits: &Limits) -> SessionReport {
    let full = |r: &Reading| r.wind_h.unwrap_or(0.0) <= limits.full_wind_h;
    let verdicts: Vec<Verdict> = readings
        .iter()
        .map(|r| {
            let m = &r.measurement;
            let judge = |v: Option<f64>, lo: f64, hi: f64| match v {
                Some(v) if full(r) => {
                    if v >= lo && v <= hi {
                        Mark::Within
                    } else {
                        Mark::Outside
                    }
                }
                _ => Mark::NotJudged,
            };
            let (alo, ahi) = tol.amplitude(r.position);
            Verdict {
                rate: judge(m.rate_s_per_day, tol.rate_min, tol.rate_max),
                amplitude: judge(m.amplitude_deg, alo, ahi),
                beat_error: match m.beat_error_ms {
                    Some(b) if b.abs() < tol.beat_error_ms => Mark::Within,
                    Some(_) => Mark::Outside,
                    None => Mark::NotJudged,
                },
            }
        })
        .collect();

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
    let pmax = states.first().and_then(|s| s.d_rate);
    let n = im.zip(pmax).map(|(i, p)| 0.15 * i.abs() + 0.1 * p + 0.6);

    let findings = findings(readings, &states, tol, limits);
    SessionReport {
        tolerance: tol.clone(),
        limits: limits.clone(),
        verdicts,
        states,
        isochronism,
        im,
        im_all,
        ie,
        n,
        findings,
    }
}

fn findings(
    readings: &[Reading],
    states: &[StateIndices],
    tol: &Tolerance,
    lim: &Limits,
) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut push = |code: &'static str,
                    recording: Option<usize>,
                    severity,
                    title: &str,
                    evidence: String,
                    advice: &str| {
        out.push(Finding {
            code,
            recording,
            severity,
            title: title.into(),
            evidence,
            advice: advice.into(),
        })
    };
    let full = |r: &Reading| r.wind_h.unwrap_or(0.0) <= lim.full_wind_h;
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
        format!("{} ({name}{w})", r.position)
    };

    // Per reading.
    for (ri, r) in readings.iter().enumerate() {
        let m = &r.measurement;
        if let Some(a) = m.amplitude_deg {
            if a > lim.overbanking {
                push(
                    "overbanking",
                    Some(ri),
                    Severity::Fault,
                    "Amplitude high enough to knock (overbanking)",
                    format!("{a:.0}° in {}; Witschi's limit is {:.0}°", at(r), lim.overbanking),
                    "Listen for a double tick. Witschi: replace the mainspring, pallet fork and/or escape wheel.",
                );
            } else if a < lim.amplitude_fault {
                push(
                    "amplitude_very_low",
                    Some(ri),
                    Severity::Fault,
                    "Very low amplitude",
                    format!("{a:.0}° in {}; below {:.0}° (project default)", at(r), lim.amplitude_fault),
                    "Usually a movement that needs servicing: dirty or dry, a weak mainspring, or a power loss in the train. Check the lift angle setting first.",
                );
            } else if full(r) {
                let (lo, hi) = tol.amplitude(r.position);
                if a < lo || a > hi {
                    push(
                        "amplitude_tolerance",
                        Some(ri),
                        Severity::Warning,
                        "Amplitude outside tolerance",
                        format!(
                            "{a:.0}° in {}; {} range for {} positions is {lo:.0}–{hi:.0}° fully wound",
                            at(r),
                            tol.name,
                            if r.position.is_vertical() { "vertical" } else { "horizontal" }
                        ),
                        if a < lo {
                            "Low amplitude points at lubrication, mainspring or train friction; confirm the lift angle for this calibre."
                        } else {
                            "High amplitude is rarely a problem below the knocking limit; check the mainspring is the right strength."
                        },
                    );
                }
            }
        }
        if let Some(b) = m.beat_error_ms.map(f64::abs) {
            if b >= lim.beat_error_fault {
                push(
                    "beat_error_large",
                    Some(ri),
                    Severity::Fault,
                    "Large beat error",
                    format!(
                        "{b:.2} ms in {}; at or above {:.1} ms (project default)",
                        at(r),
                        lim.beat_error_fault
                    ),
                    "Witschi: correct the beat error (repère) first, then adjust the rate.",
                );
            } else if b >= tol.beat_error_ms {
                push(
                    "beat_error_tolerance",
                    Some(ri),
                    Severity::Warning,
                    "Beat error outside tolerance",
                    format!(
                        "{b:.2} ms in {}; Witschi's tolerance is under {:.1} ms",
                        at(r),
                        tol.beat_error_ms
                    ),
                    "Put the watch in beat (turn the stud or collet) before regulating.",
                );
            }
        }
        if let (Some(lo), Some(hi)) = (m.rate_p05, m.rate_p95) {
            if hi - lo > lim.rate_spread {
                push(
                    "rate_unsteady",
                    Some(ri),
                    Severity::Warning,
                    "Rate unsteady within the reading",
                    format!(
                        "10 s readings from {lo:+.1} to {hi:+.1} s/d (5th to 95th percentile) in {}; more than {:.0} s/d (project default)",
                        at(r),
                        lim.rate_spread
                    ),
                    "Witschi calls a scattered, wandering rate a functional fault, usually from low amplitude, and a regular wave a gear-train fault. See the periodic changes for this recording.",
                );
            }
        }
        let measured = m.end_s - m.start_s;
        if measured < lim.min_measure_s {
            push(
                "measurement_short",
                Some(ri),
                Severity::Note,
                "Short measurement",
                format!(
                    "{measured:.0} s measured in {}; Witschi recommends at least {:.0} s",
                    at(r),
                    lim.min_measure_s
                ),
                "Record longer, or shorten the settling time if the watch had already settled.",
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
            let size = format!("{:.1} {} peak to peak", c.size, c.unit());
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
                    "; also at whole fractions of the turn ({}), as a short sharp change gives",
                    harmonics.join(", ")
                )
            };
            match &c.wheel {
                Some(w) => push(
                    "cycle_wheel",
                    Some(ri),
                    Severity::Warning,
                    &format!("Regular {} change once per turn of the {w}", c.series),
                    format!(
                        "{} repeats every {:.2} s ({size}, {:.0}% of the variation) in {}; false-alarm chance {fa:.0e}{harmonics}",
                        c.series, c.period_s, c.explained * 100.0, at(r)
                    ),
                    "Witschi: large but regular rate variations are a fault in the gear train. Inspect that wheel and its pinion for a damaged tooth, wear or eccentricity, and check the hands for rubbing if it is the fourth wheel.",
                ),
                None if c.fraction_of.is_some() => {
                    let (w, n) = c.fraction_of.clone().unwrap();
                    push(
                        "cycle_wheel_fraction",
                        Some(ri),
                        Severity::Note,
                        &format!("Regular {} change at 1/{n} of the {w}'s turn", c.series),
                        format!(
                            "{} repeats every {:.2} s ({size}) in {}; false-alarm chance {fa:.0e}",
                            c.series,
                            c.period_s,
                            at(r)
                        ),
                        "A short, sharp change once per turn of that wheel shows at whole fractions of its turn, even when the turn itself is too weak to stand out. Look at the wheel's turn in a longer recording.",
                    )
                }
                None => push(
                    "cycle_other",
                    Some(ri),
                    Severity::Note,
                    &format!("Regular {} change not tied to a listed wheel", c.series),
                    format!(
                        "{} repeats every {:.2} s ({size}) in {}; false-alarm chance {fa:.0e}{}",
                        c.series,
                        c.period_s,
                        at(r),
                        c.nearest_wheel
                            .as_ref()
                            .map_or(String::new(), |(w, off)| format!("; nearest is the {w}, {off:+.1}% off"))
                    ),
                    "Could be another wheel or pinion of the train (add its turn period to name it), or a disturbance near the microphone.",
                ),
            }
        }
        if let Some(sh) = &r.shape {
            for (side, v) in [("even", &sh.even), ("odd", &sh.odd)] {
                if let Some(q) = v.ratio13.filter(|&q| q >= 1.0) {
                    push(
                        "shape_unlock_loud",
                        Some(ri),
                        Severity::Note,
                        "Unlocking as loud as the drop",
                        format!("Level of sound 1 to sound 3 is {q:.2} on {side} beats in {}", at(r)),
                        "Witschi's 'unlocking too strong' (deep lock). The rule is not yet checked against watches with confirmed faults.",
                    );
                }
                let extra = v.extra_pre.len() + v.extra_post.len();
                if extra > 0 {
                    push(
                        "shape_extra_sounds",
                        Some(ri),
                        Severity::Note,
                        "Extra sounds around the beat",
                        format!(
                            "{} before the unlock and {} after the drop on {side} beats in {}",
                            v.extra_pre.len(),
                            v.extra_post.len(),
                            at(r)
                        ),
                        "Witschi lists the safety pin touching the roller, too little impulse-pin clearance, and the impulse pin knocking the fork horn. Not yet checked against watches with confirmed faults.",
                    );
                }
            }
        }
    }

    // Across positions.
    for st in states {
        let label = if states.len() > 1 && st.wind_h <= 0.0 {
            " fully wound".to_string()
        } else if states.len() > 1 {
            format!(" at {:.0} h after winding", st.wind_h)
        } else {
            String::new()
        };
        if st.wind_h <= lim.full_wind_h {
            if let Some(x) = st.x {
                if x < tol.rate_min || x > tol.rate_max {
                    push(
                        "rate_tolerance",
                        None,
                        Severity::Warning,
                        "Mean rate outside tolerance",
                        format!(
                            "X = {}{label} over {} position(s); {} range is {:+.0} to {:+.0} s/d",
                            s(x),
                            st.positions.len(),
                            tol.name,
                            tol.rate_min,
                            tol.rate_max
                        ),
                        "Witschi: adjust towards a target such as 0 to +10 s/d, after the beat error.",
                    );
                }
            }
        }
        if let Some(d) = st.d_rate.filter(|&d| d > lim.delta_rate) {
            let (hi, lo) = extremes(st);
            push(
                "positional_delta",
                None,
                Severity::Warning,
                "Large differences between positions",
                format!(
                    "D = {d:.1} s/d{label}, from {lo} to {hi}; above {:.0} s/d (project default){}",
                    lim.delta_rate,
                    st.dv_rate
                        .map_or(String::new(), |v| format!("; DV = {v:.1} s/d over the vertical positions"))
                ),
                "Witschi: centre the hairspring, poise the balance wheel, or replace the regulating organ. A large DV with a small DH points at poise.",
            );
        }
        if let Some(a) = st.dvh_amplitude.filter(|&a| a < -lim.vh_amplitude_drop) {
            push(
                "vh_amplitude_drop",
                None,
                Severity::Warning,
                "Large amplitude loss in the vertical positions",
                format!(
                    "Vertical positions average {:.0}° less than horizontal{label}; more than {:.0}° (project default)",
                    -a, lim.vh_amplitude_drop
                ),
                "Some loss is normal (pivot friction). A large one points at worn or dry balance pivots or jewels, or a bent pivot.",
            );
        }
        if let Some(v) = st.dvh_rate.filter(|v| v.abs() >= 5.0) {
            push(
                "dvh_rate",
                None,
                Severity::Note,
                "Vertical and horizontal rates differ",
                format!("DVH = {}{label}", s(v)),
                if v < 0.0 {
                    "Witschi: with regulator pins, a negative DVH means reduce the clearance (close the pins). Free-sprung balances have no pins."
                } else {
                    "Witschi: with regulator pins, a positive DVH means increase the clearance (open the pins). Free-sprung balances have no pins."
                },
            );
        }
    }
    out.sort_by_key(|f| f.severity);
    out
}

fn extremes(st: &StateIndices) -> (String, String) {
    let with: Vec<(Position, f64)> = st
        .positions
        .iter()
        .filter_map(|v| v.rate_s_per_day.map(|r| (v.position, r)))
        .collect();
    let fmt =
        |o: Option<&(Position, f64)>| o.map_or("-".into(), |(p, r)| format!("{} in {p}", s(*r)));
    let hi = with.iter().max_by(|a, b| a.1.total_cmp(&b.1));
    let lo = with.iter().min_by(|a, b| a.1.total_cmp(&b.1));
    (fmt(hi), fmt(lo))
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
    /// Witschi tolerance class: ladies, mens (default), cosc, cosc-small, metas.
    pub tolerance: Option<String>,
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
    pub limits: Option<Limits>,
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
    pub settle_s: Option<f64>,
    pub measure_s: Option<f64>,
    pub notes: Option<String>,
    pub reference: Option<Reference>,
}
