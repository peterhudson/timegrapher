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

use crate::amplitude::AmplitudeWindow;
use crate::beats::Beat;
use crate::breaks::Break;
use crate::clean::Cuts;
use crate::clock::ClockFit;
use crate::dsp::percentile;
use crate::dsp::{mean_of, median_of};
use crate::longrun::LongReport;
use crate::periodicity::Wheel;
use crate::shape::{ShapeReport, SideReport};
use crate::stream::BeatLog;
use crate::timing::{self, WindowFit};
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
    /// Half past one up (1H30): a 45° stop between 12H and 3H (#1473).
    H1h30,
    /// Half past four up (4H30), between 3H and 6H.
    H4h30,
    /// Half past seven up (7H30), between 6H and 9H.
    H7h30,
    /// Half past ten up (10H30), between 9H and 12H.
    H10h30,
}

/// The hour at the top on each half hour, from 12H at 0° clockwise in
/// 15° steps: "12H", "12H30", "1H" … "11H30". The upright positions' codes
/// and [`crate::poise::hour_code`] both read it, so an hour is named once.
pub const HOUR_CODES: [&str; 24] = [
    "12H", "12H30", "1H", "1H30", "2H", "2H30", "3H", "3H30", "4H", "4H30", "5H", "5H30", "6H",
    "6H30", "7H", "7H30", "8H", "8H30", "9H", "9H30", "10H", "10H30", "11H", "11H30",
];

const ALIASES: [(Position, &[&str]); 10] = [
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
    (Position::H1h30, &["1h30", "130up"]),
    (Position::H4h30, &["4h30", "430up"]),
    (Position::H7h30, &["7h30", "730up"]),
    (Position::H10h30, &["10h30", "1030up"]),
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

    /// The eight upright stops, 45° apart, in the order the stand turns
    /// them for a Dynamic Poise test: 9H, 7H30, 6H … 10H30.
    pub const UPRIGHT: [Position; 8] = [
        Position::H9,
        Position::H7h30,
        Position::H6,
        Position::H4h30,
        Position::H3,
        Position::H1h30,
        Position::H12,
        Position::H10h30,
    ];

    /// The four 45° stops, in hour order.
    pub const HALF_HOURS: [Position; 4] = [
        Position::H1h30,
        Position::H4h30,
        Position::H7h30,
        Position::H10h30,
    ];

    /// All ten: [`Position::ALL`], then the four 45° stops.
    pub const TEN: [Position; 10] = [
        Position::CH,
        Position::CB,
        Position::H9,
        Position::H6,
        Position::H3,
        Position::H12,
        Position::H1h30,
        Position::H4h30,
        Position::H7h30,
        Position::H10h30,
    ];

    /// The angle of the hour at the top, degrees clockwise from 12H; none
    /// lying flat.
    pub fn top_deg(self) -> Option<f64> {
        let half_hours = match self {
            Position::CH | Position::CB => return None,
            Position::H12 => 0,
            Position::H1h30 => 3,
            Position::H3 => 6,
            Position::H4h30 => 9,
            Position::H6 => 12,
            Position::H7h30 => 15,
            Position::H9 => 18,
            Position::H10h30 => 21,
        };
        Some(15.0 * f64::from(half_hours))
    }

    /// Whether this is one of the four 45° stops.
    pub fn is_half_hour(self) -> bool {
        Position::HALF_HOURS.contains(&self)
    }

    /// Witschi's code: CH and CB lying, and the hour at the top standing,
    /// from [`HOUR_CODES`].
    pub fn code(self) -> &'static str {
        match self {
            Position::CH => "CH",
            Position::CB => "CB",
            p => p
                .top_deg()
                .map_or("", |deg| HOUR_CODES[(deg / 15.0) as usize]),
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
            Position::H1h30 => "1:30 up",
            Position::H4h30 => "4:30 up",
            Position::H7h30 => "7:30 up",
            Position::H10h30 => "10:30 up",
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
            Position::H1h30 => "1:30 Up",
            Position::H4h30 => "4:30 Up",
            Position::H7h30 => "7:30 Up",
            Position::H10h30 => "10:30 Up",
        }
    }

    /// The name with Witschi's code, e.g. "Crown Up (3H)": the crown sits
    /// at 3 o'clock, so crown up is 3H, crown left 6H, crown down 9H and
    /// crown right 12H.
    pub fn label(self) -> String {
        format!("{} ({})", self.name(), self.code())
    }

    /// The name as the app and the text report show it: "Crown Up (CU,
    /// 3H)", the common code with Witschi's beside it, or "Dial Up (DU)"
    /// where Witschi's is a letter code, or "1:30 Up (1H30)" where the two
    /// are one. JSON keeps [`Position::label`].
    pub fn display_label(self) -> String {
        if self.common() != self.code() && self.code().starts_with(|c: char| c.is_ascii_digit()) {
            format!("{} ({}, {})", self.name(), self.common(), self.code())
        } else {
            format!("{} ({})", self.name(), self.common())
        }
    }

    /// The short name most hobby timegraphers use; the 45° stops have
    /// none but their hour code.
    pub fn common(self) -> &'static str {
        match self {
            Position::CH => "DU",
            Position::CB => "DD",
            Position::H9 => "CD",
            Position::H6 => "CL",
            Position::H3 => "CU",
            Position::H12 => "CR",
            p => p.code(),
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

    /// The same names as [`Position::accepted_names`], one by one, for a
    /// list a caller picks from (the MCP `record_position` choices).
    pub fn names() -> Vec<&'static str> {
        let (mut common, witschi) = Position::name_groups();
        common.extend(witschi);
        common
    }

    fn name_groups() -> (Vec<&'static str>, Vec<&'static str>) {
        let common = Position::TEN.iter().map(|p| p.common()).collect();
        let witschi = Position::TEN
            .iter()
            .filter(|p| p.code() != p.common())
            .map(|p| p.code())
            .collect();
        (common, witschi)
    }

    /// The names every surface lists as accepted, from [`Position::TEN`]:
    /// the common ones first, then Witschi's where they differ, "DU, DD,
    /// CD, CL, CU, CR, 1H30, 4H30, 7H30 or 10H30 (or CH, CB, 9H, 6H, 3H or
    /// 12H)". [`Position::parse`] takes these and more (#1492).
    pub fn accepted_names() -> String {
        let (common, witschi) = Position::name_groups();
        let or_list = |names: &[&str]| match names.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
            Some((last, _)) => (*last).to_string(),
            None => String::new(),
        };
        format!("{} (or {})", or_list(&common), or_list(&witschi))
    }

    /// The position named in a file name such as `ym42_DU_48000-01.flac`.
    /// The letter names match in any case; 3H, 6H, 9H, 12H and the 45°
    /// stops (1H30 …) only in capitals, so that a duration like `2h` or
    /// `12h` is not read as one.
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
            tokens.iter().find_map(|t| {
                Position::TEN
                    .into_iter()
                    .filter(|p| p.is_vertical())
                    .find(|p| p.code() == *t)
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
    /// The clock the reference's rate was read on; left out, not recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<ReferenceClock>,
}

/// The clock a [`Reference`]'s rate was read on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceClock {
    /// The reference instrument's own time base (a bench timegrapher's
    /// crystal): set beside a clock-corrected reading.
    Own,
    /// This sound card's clock (another program reading the same
    /// recording): set beside an uncorrected reading.
    SoundCard,
}

impl Reference {
    /// Whether the reference's rate is on the same clock as a reading's:
    /// `own` beside a clock-corrected reading, `sound_card` beside an
    /// uncorrected one. A reference with no `clock` is taken as matching an
    /// uncorrected reading, as the report has always set them side by side.
    pub fn same_clock_as(&self, calibrated: bool) -> bool {
        match self.clock {
            Some(ReferenceClock::Own) => calibrated,
            Some(ReferenceClock::SoundCard) | None => !calibrated,
        }
    }
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
    /// Seconds within the stretch masked out of the readings by hand.
    #[serde(skip_serializing_if = "is_zero")]
    pub masked_s: f64,
    pub beats: usize,
    /// Share of the expected beats found with a clean match, 0..1.
    pub clean_fraction: f64,
    /// Whether rate is corrected for the sound card's clock.
    pub calibrated: bool,
    /// The steady error the take's clock corrects, ppm, and where the
    /// clock came from in words, as [`crate::take::OpenedEntry`] gives
    /// them: set for a reading of a take opened by
    /// [`crate::take::measure_entry`], none otherwise (#714).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock_ppm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock_source: Option<String>,
    pub rate_s_per_day: Option<f64>,
    /// Signed beat error from the drop (the fitted beat times), ms (even
    /// beats late is positive).
    pub beat_error_ms: Option<f64>,
    /// Signed beat error from the unlock, the figure checked against tg's
    /// beat error in regress (median of the amplitude windows), ms.
    pub beat_error_unlock_ms: Option<f64>,
    /// Source of the selected beat error, absent when neither event was read.
    pub beat_error_source: Option<BeatErrorSource>,
    pub amplitude_deg: Option<f64>,
    pub amplitude_even_deg: Option<f64>,
    pub amplitude_odd_deg: Option<f64>,
    /// Median beat-to-beat timing scatter in 10 s windows, microseconds.
    pub jitter_us: Option<f64>,
    /// Whether a regular beat was found (see [`crate::measure::watch_heard`]).
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
    /// Per-figure nominal and measured windows within kept time.
    pub coverage: crate::confidence::FigureCoverage,
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
    /// The lift angle was a default, not the calibre's: the caveat is
    /// named, as `analyze` names it. [`measure`] takes the lift angle as
    /// known.
    pub fn lift_unknown(&mut self) {
        crate::measure::add(&mut self.caveats, crate::measure::Caveat::LiftUnknown);
    }

    /// The take's breaks are not all its own list's ([`crate::breaks::Unlisted`]):
    /// the caveat is named.
    pub fn breaks_unlisted(&mut self, why: crate::breaks::Unlisted) {
        crate::measure::add(
            &mut self.caveats,
            crate::measure::Caveat::BreaksUnlisted { why },
        );
    }

    /// What a take's reading names beside its figures, as every reader of
    /// the take names it: its lift, when that is not known
    /// ([`Self::lift_unknown`]), and why its breaks are not all its own
    /// list's, when they are not ([`Self::breaks_unlisted`]).
    pub fn take_doubts(&mut self, lift_known: bool, unlisted: Option<crate::breaks::Unlisted>) {
        if !lift_known {
            self.lift_unknown();
        }
        if let Some(why) = unlisted {
            self.breaks_unlisted(why);
        }
    }

    /// The beat error the report shows ([`beat_error_size`]). Unsigned, ms.
    pub fn beat_error(&self) -> Option<f64> {
        beat_error_size(self.beat_error_unlock_ms, self.beat_error_ms)
    }

    /// That beat error with its sign ([`beat_error_signed`]), ms.
    pub fn beat_error_signed(&self) -> Option<f64> {
        beat_error_signed(self.beat_error_unlock_ms, self.beat_error_ms)
    }

    /// The seconds measured: the stretch read, less what was masked out.
    pub fn measured_s(&self) -> f64 {
        self.end_s - self.start_s - self.masked_s
    }
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// Masked stretches as given, cleaned for use: finite, start before end,
/// sorted, and overlapping ones joined.
pub fn masked_spans(spans: &[[f64; 2]]) -> Vec<(f64, f64)> {
    let mut v: Vec<(f64, f64)> = spans
        .iter()
        .filter(|s| s[0].is_finite() && s[1].is_finite() && s[1] > s[0])
        .map(|s| (s[0], s[1]))
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(v.len());
    for (a, b) in v {
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// Whether `t` falls in one of the (cleaned) masked stretches.
pub fn in_mask(masked: &[(f64, f64)], t: f64) -> bool {
    masked.iter().any(|&(a, b)| t >= a && t <= b)
}

/// Seconds of the masked stretches that fall between `from_s` and `to_s`.
pub fn masked_within(masked: &[(f64, f64)], from_s: f64, to_s: f64) -> f64 {
    masked
        .iter()
        .map(|&(a, b)| (b.min(to_s) - a.max(from_s)).max(0.0))
        .sum()
}

/// Leave the masked stretches out of a beat log: their beats, the
/// amplitude windows that reach into them, and their clipped beats.
pub fn mask_log(log: &mut BeatLog, masked: &[(f64, f64)]) {
    if masked.is_empty() {
        return;
    }
    log.beats.retain(|b| !in_mask(masked, b.time));
    log.amplitude_windows.retain(|w| !window_in_mask(masked, w));
    log.clipped_beats.retain(|&t| !in_mask(masked, t));
}

/// Whether the amplitude window `w` reaches into one of the masked
/// stretches, as [`mask_log`] leaves such a window out.
pub fn window_in_mask(masked: &[(f64, f64)], w: &AmplitudeWindow) -> bool {
    masked.iter().any(|&(a, b)| w.start_s < b && w.end_s > a)
}

/// The stretches masked by hand, `masked`, with the `holes` (a take's
/// settlings) left out as a mask is:
/// together, cleaned ([`masked_spans`]). What a reading over a whole take
/// culls, so none falls in a settling (#1233).
pub fn with_holes(masked: &[(f64, f64)], holes: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let pairs: Vec<[f64; 2]> = masked.iter().chain(holes).map(|&(a, b)| [a, b]).collect();
    masked_spans(&pairs)
}

/// Leave the holes `holes` out of a log read in stretches, as a mask is:
/// their beats and the windows reaching into them out of `log`
/// ([`mask_log`]), the holes added to `masked` (the stretches masked by
/// hand), and every stretch left out cut at both ends among `breaks`
/// ([`crate::clean::mask_breaks`]), so no reading spans one. What
/// `series` and `long` do with a take's holes and masks, and the app's
/// Distributions and Steadiness panes with a position's (#1156).
pub fn leave_out(
    log: &mut BeatLog,
    breaks: &mut Vec<crate::breaks::Break>,
    masked: &mut Vec<(f64, f64)>,
    holes: &[(f64, f64)],
) {
    if !holes.is_empty() {
        mask_log(log, holes);
        *masked = with_holes(masked, holes);
    }
    breaks.extend(crate::clean::mask_breaks(masked));
    breaks.sort_by(|a, b| a.audio_s.total_cmp(&b.audio_s));
}

/// The beats a reading of the stretch from `from_s` to `to_s` (audio
/// seconds) takes, from a time-ordered `beats`: those at `from_s` or after
/// and before `to_s`. Live and saved readings both take them so
/// ([`crate::live::LiveAnalyzer::reading_at`], [`measure_masked`]).
pub fn stretch_beats(beats: &[Beat], from_s: f64, to_s: f64) -> std::ops::Range<usize> {
    let lo = beats.partition_point(|b| b.time < from_s);
    let hi = beats.partition_point(|b| b.time < to_s);
    lo.min(hi)..hi
}

/// The amplitude windows a reading of the stretch from `from_s` to `to_s`
/// (audio seconds) takes: those `holes` keeps, clear of every hole on
/// true time, whose midpoint lies in the stretch, from `from_s` up to but
/// not at `to_s`, so each window belongs to exactly one of two stretches
/// that meet (#632). Live and saved readings both take them so, so one
/// stretch reads one way.
pub fn stretch_windows<'w>(
    holes: &Cuts,
    windows: &'w [AmplitudeWindow],
    from_s: f64,
    to_s: f64,
) -> Vec<&'w AmplitudeWindow> {
    holes
        .amplitude_windows(windows)
        .into_iter()
        .filter(|w| window_in(w, from_s, to_s))
        .collect()
}

/// How far either side of a reading its ± looks for the values it
/// borrows its noise from when it holds too few (#1470), s: an hour. Noise
/// from further away describes a different state of the watch
/// (math-standards section 7); it also keeps a figure whose windows
/// rarely give a value (an unlock seldom found) from searching a long
/// take end to end at every reading.
pub const BORROW_LOOK_S: f64 = 3600.0;

/// How far a saved reading's borrowing of the rate's windows first
/// reaches, s: the 300 s its 30 values need ([`RATE_SE_MIN_S`]), since a
/// reading short enough to borrow holds few of them itself. Live starts
/// shorter (`live::RATE_FIRST_REACH_S`), its run counting with the pool.
pub const RATE_FIRST_REACH_S: f64 = RATE_SE_MIN_S;

/// How far borrowing of the amplitude's windows first reaches, saved or
/// live, s: 600 s, ten times the minute their 30 values of 2 s need, so a
/// take whose unlock is often missed is still found in one reach.
pub const AMPLITUDE_FIRST_REACH_S: f64 = crate::confidence::MIN_NOISE_VALUES as f64
    * crate::distribution::STRETCH_S
    * AMPLITUDE_REACH_MARGIN;
/// [`AMPLITUDE_FIRST_REACH_S`] over the time its values need.
const AMPLITUDE_REACH_MARGIN: f64 = 10.0;

/// The windows `fetch(a, b)` gives for the take from `a` to `b` (s), over
/// the least reach either side of a reading from `from_s` to `to_s` that
/// gives `enough`: back first, as far as the take's start, then on past
/// the reading towards `end_s`, the take's end (#1470). Each reach starts
/// at `first_s` and doubles, up to [`BORROW_LOOK_S`].
pub fn nearest_windows<T>(
    (from_s, to_s, end_s): (f64, f64, f64),
    first_s: f64,
    fetch: impl Fn(f64, f64) -> Vec<T>,
    enough: impl Fn(&[T]) -> bool,
) -> Vec<T> {
    let reach = |at: &dyn Fn(f64) -> (f64, f64), done: &dyn Fn(f64) -> bool| {
        let mut look = first_s;
        loop {
            let (a, b) = at(look);
            let w = fetch(a, b);
            if enough(&w) || done(look) || look >= BORROW_LOOK_S {
                return (w, look);
            }
            look *= 2.0;
        }
    };
    let back = |look: f64| ((from_s - look).max(0.0), to_s);
    let (w, look) = reach(&back, &|look| from_s - look <= 0.0);
    if enough(&w) || to_s >= end_s {
        return w;
    }
    let a = back(look).0;
    reach(&|on| (a, (to_s + on).min(end_s)), &|on| to_s + on >= end_s).0
}

/// The parts of the take from `a` to `b` (s) a reading may borrow its
/// noise from: those inside the take's `read` stretches (#1624), in time
/// order, or the whole of it where none are given.
pub fn read_within(read: &[(f64, f64)], a: f64, b: f64) -> Vec<(f64, f64)> {
    if read.is_empty() {
        return vec![(a, b)];
    }
    read.iter()
        .map(|&(x, y)| (x.max(a), y.min(b)))
        .filter(|(x, y)| x < y)
        .collect()
}

/// Whether the 2 s window `w` belongs to the stretch from `from_s` to
/// `to_s` of a take whose test read the stops `stops`: wholly inside it,
/// since a window over a stop's edge holds the handling between stops
/// (Norm on #1624, masks cull, never fill); with no stops (a plain take),
/// by its midpoint ([`window_in`]).
pub fn stop_window_in(w: &AmplitudeWindow, stops: &[(f64, f64)], from_s: f64, to_s: f64) -> bool {
    if stops.is_empty() {
        window_in(w, from_s, to_s)
    } else {
        w.start_s >= from_s && w.end_s <= to_s
    }
}

/// Whether enough of a figure's values are among `windows` to borrow its
/// noise from: [`crate::confidence::MIN_NOISE_VALUES`] of each of the
/// amplitude and the unlock beat error.
pub fn noise_enough(windows: &[&AmplitudeWindow]) -> bool {
    let n = crate::confidence::MIN_NOISE_VALUES;
    windows.iter().filter(|w| w.mean().is_some()).count() >= n
        && windows
            .iter()
            .filter(|w| w.beat_error_unlock_ms.is_some())
            .count()
            >= n
}

/// Whether enough rate windows are among `windows` to borrow the rate's
/// noise from: every other one, as [`crate::confidence::rate_se`] takes
/// them, [`crate::confidence::MIN_NOISE_VALUES`] times.
pub fn rate_noise_enough(windows: &[WindowFit]) -> bool {
    crate::confidence::rate_value_count(windows.len()) >= crate::confidence::MIN_NOISE_VALUES
}

/// Whether the 2 s window `w` belongs to the stretch from `from_s` to
/// `to_s` (seconds on the windows' own clock): its midpoint lies in the
/// stretch, from `from_s` up to but not at `to_s`, so each window belongs
/// to exactly one of two stretches that meet (#632). The one rule for
/// which windows a stretch takes.
pub fn window_in(w: &AmplitudeWindow, from_s: f64, to_s: f64) -> bool {
    window_span_in(w.start_s, w.end_s, from_s, to_s)
}

/// Midpoint membership for measured and nominal amplitude windows alike.
pub(crate) fn window_span_in(start: f64, end: f64, from_s: f64, to_s: f64) -> bool {
    let mid = (start + end) / 2.0;
    mid >= from_s && mid < to_s
}

/// What a stretch's amplitude windows ([`stretch_windows`]) read.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WindowFigures {
    pub amplitude_deg: Option<f64>,
    pub amplitude_even_deg: Option<f64>,
    pub amplitude_odd_deg: Option<f64>,
    /// Signed, as [`timing::median_signed`] gives it.
    pub beat_error_unlock_ms: Option<f64>,
    /// The unlock beat errors disagree in sign more than their spread
    /// allows, so no unlock beat error is given ([`unlock_inconsistent`],
    /// #1439).
    pub unlock_inconsistent: bool,
}

/// [`WindowFigures`] of `wins`: the medians of their amplitude, Tick's and
/// Tock's, and of their unlock beat error, taken as sizes where `turned`
/// ([`timing::sign_turns`] over the stretch's beats). No unlock beat error
/// where its windows disagree in sign more than their spread allows
/// ([`unlock_inconsistent`], #1439): the drop's is used.
pub fn window_figures(wins: &[&AmplitudeWindow], turned: bool) -> WindowFigures {
    let inconsistent = unlock_inconsistent(wins, turned);
    WindowFigures {
        amplitude_deg: median_of(wins.iter().filter_map(|w| w.mean())),
        amplitude_even_deg: median_of(wins.iter().filter_map(|w| w.even_deg)),
        amplitude_odd_deg: median_of(wins.iter().filter_map(|w| w.odd_deg)),
        beat_error_unlock_ms: (!inconsistent)
            .then(|| {
                timing::median_signed(wins.iter().filter_map(|w| w.beat_error_unlock_ms), turned)
            })
            .flatten(),
        unlock_inconsistent: inconsistent,
    }
}

/// Whether the unlock beat errors of `wins`, as the figure takes them
/// (sizes where `turned`), disagree in sign more than their spread allows
/// ([`crate::confidence::unlock_inconsistent`], #1439).
pub fn unlock_inconsistent(wins: &[&AmplitudeWindow], turned: bool) -> bool {
    let v: Vec<f64> = wins
        .iter()
        .filter_map(|w| w.beat_error_unlock_ms)
        .map(|x| if turned { x.abs() } else { x })
        .collect();
    crate::confidence::unlock_inconsistent(&v)
}

/// The 2 s cells of the take's one tiling on true time (#636), from 0 to
/// `duration_t`, that a stretch from `from_t` to `to_t` (true time) takes
/// in by the rule its windows are taken in by (middle inside, half-open)
/// and that are clear of every hole: with nothing culled, every one holds
/// a window. The nominal count each window figure's coverage is read
/// against.
pub fn window_cells(holes: &Cuts, duration_t: f64, part: f64, from_t: f64, to_t: f64) -> usize {
    crate::clean::nominal_windows(0.0, duration_t, part, part)
        .filter(|&(a, b)| window_span_in(a, b, from_t, to_t))
        .filter(|&(a, b)| holes.clear(a, b))
        .count()
}

/// `figures` with each one withheld that too few of the stretch's
/// windows `wins` give, against its `cells` nominal windows
/// ([`crate::confidence::too_few_windows`], #1429).
pub fn floor_figures(
    figures: WindowFigures,
    wins: &[&AmplitudeWindow],
    cells: usize,
) -> WindowFigures {
    let keep = |v: Option<f64>, f: fn(&AmplitudeWindow) -> bool| {
        v.filter(|_| {
            !crate::confidence::too_few_windows(wins.iter().filter(|w| f(w)).count(), cells)
        })
    };
    WindowFigures {
        amplitude_deg: keep(figures.amplitude_deg, |w| w.mean().is_some()),
        amplitude_even_deg: keep(figures.amplitude_even_deg, |w| w.even_deg.is_some()),
        amplitude_odd_deg: keep(figures.amplitude_odd_deg, |w| w.odd_deg.is_some()),
        beat_error_unlock_ms: keep(figures.beat_error_unlock_ms, |w| {
            w.beat_error_unlock_ms.is_some()
        }),
        unlock_inconsistent: figures.unlock_inconsistent,
    }
}

impl WindowFigures {
    /// `c` with no ± for a figure these give none of: a withheld amplitude
    /// ([`floor_figures`], #1429) has no ±, and nor has a beat error with
    /// no unlock figure, as a reading whose beat error comes from the drop
    /// has none.
    pub fn floor_confidence(&self, c: &mut crate::confidence::Confidence) {
        if self.amplitude_deg.is_none() {
            c.amplitude.se = None;
        }
        if self.beat_error_unlock_ms.is_none() {
            c.beat_error.se = None;
        }
    }
}

/// A stretch's timing fit and window figures, as every reading of a
/// stretch takes them: saved (`session`), whole (`analyze`) and live
/// (#512, one engine reads a stretch).
#[derive(Debug, Clone)]
pub struct StretchFits {
    /// Rate and beat error, fitted piece by piece between the cuts
    /// ([`timing::fit_pieces`]); none with no beat rate.
    pub fit: Option<timing::TimingFit>,
    /// Whether the beat error's sign turns between pieces
    /// ([`timing::sign_turns`]).
    pub turned: bool,
    /// The amplitude windows' figures, the unlock beat error as sizes
    /// where it turns ([`window_figures`]).
    pub figures: WindowFigures,
}

/// [`StretchFits`] of a stretch's `beats` (true time, in order) at `bph`,
/// cut at `cuts`, and of its amplitude windows `wins`: the one way a
/// reading of a stretch is fitted (#512).
pub fn stretch_fits(
    beats: &[Beat],
    bph: Option<u32>,
    cuts: &[f64],
    wins: &[&AmplitudeWindow],
) -> StretchFits {
    let turned = bph.is_some_and(|bph| timing::sign_turns(beats, bph, cuts));
    StretchFits {
        fit: bph.and_then(|bph| timing::fit_pieces(beats, bph, cuts)),
        turned,
        figures: window_figures(wins, turned),
    }
}

/// The length of the parts a stretch's windows must be whole in, seconds:
/// its amplitude windows' length, else 2 s.
pub fn window_part_s(amp: &[AmplitudeWindow]) -> f64 {
    amp.first().map_or(2.0, |w| w.end_s - w.start_s)
}

use crate::measure::{RATE_SE_MIN_S, RATE_WINDOW_S, RATE_WINDOW_STEP_S};

/// The 10 s windows, one every 5 s, that a stretch from `from_s` to
/// `to_s` (true time) is read in for its rates and its timing scatter.
pub fn scatter_windows(
    holes: &Cuts,
    beats: &[Beat],
    bph: u32,
    from_s: f64,
    to_s: f64,
    part_s: f64,
) -> Vec<WindowFit> {
    holes.windows(
        beats,
        bph,
        from_s,
        to_s,
        RATE_WINDOW_S,
        RATE_WINDOW_STEP_S,
        part_s,
    )
}

/// The timing scatter a stretch is judged on for whether a watch is
/// heard: the median jitter of its windows. Live and saved readings and
/// `analyze` all decide on it (#680).
pub fn scatter_jitter_us(windows: &[WindowFit]) -> Option<f64> {
    median_of(windows.iter().map(|w| w.fit.jitter_us))
}

/// The least a watch must be heard for after a lead of noise for the lead
/// to be left out ([`watch_lead_s`]): two whole scatter windows.
pub const LEAD_WATCH_MIN_S: f64 = 20.0;

/// How much more than the windows after it the first window of a watch
/// after a lead may scatter ([`watch_lead_s`], #1418). Stand noise can
/// scatter under [`crate::beats::NOISE_DOUBT_FRACTION`] in a 10 s window,
/// at 0.7% of a beat at 18,000 and 19,800 bph, where the watch after it
/// scatters 0.01%. A watch's first window may scatter up to this many
/// times its later ones and still count as the watch's; past it, the
/// lead ends a window later.
pub const LEAD_SCATTER_RATIO: f64 = 4.0;

/// The least scatter, as a fraction of a beat, the windows after a lead
/// are taken to have ([`LEAD_SCATTER_RATIO`]): a clean watch's 0.01%
/// would otherwise hold its own first window to a few microseconds. The
/// line for a clean watch is then 0.08% of a beat, under the 0.34% a
/// 10 s window of noise came down to at 18,000 bph.
pub const LEAD_SCATTER_FLOOR: f64 = 0.0002;

/// Where the watch starts in a stretch that opens with a lead of noise
/// (#844), from its scatter windows (`windows`, in time order): the start
/// of the first window that is the watch's: its timing clearly regular
/// (scatter under [`crate::beats::NOISE_DOUBT_FRACTION`] of a beat), the
/// windows from it on clearly regular by their median, and its scatter
/// at most [`LEAD_SCATTER_RATIO`] times theirs, and the same of the next
/// window that doesn't overlap it (#1418), with no found jump from the
/// first's start to the second's end and every window between them on
/// the grid present, with at least
/// [`LEAD_WATCH_MIN_S`] from it to the end. `None` when the first window
/// is the watch's or no such window is found. The whole
/// stretch's median is not asked: a lead about as long as the watch can
/// put it either side of the line. Noise scatters about as much as a doubtful watch
/// (1.5 to 4% of a beat on a stand), and a watch far less, so a noise
/// lead can outweigh the watch after it in the whole stretch's verdict.
/// Where the beats jumped (`jumps`, audio seconds) between that window's
/// start and the one before's, no more than one grid step back, the watch
/// starts at the last such jump instead: there the beats stepped from the
/// noise's onto the watch's. Where windows are missing before it, a jump
/// further back is in the noise (#1423).
/// A whole file leaves the lead out; a reading of a set average (live or
/// saved) does not, so a watch heard briefly after noise there is still
/// no watch.
pub fn watch_lead_s(windows: &[WindowFit], bph: u32, jumps: &[f64]) -> Option<f64> {
    let fraction =
        |w: &[WindowFit]| crate::beats::timing_scatter_fraction(scatter_jitter_us(w), bph);
    let clear =
        |w: &[WindowFit]| fraction(w).is_some_and(|f| f < crate::beats::NOISE_DOUBT_FRACTION);
    // A window passes when it is clearly regular and scatters about as
    // the windows after it do (#1418).
    let passes = |k: usize| {
        clear(&windows[k..=k])
            && clear(&windows[k..])
            && fraction(&windows[k..=k])
                .zip(fraction(&windows[k..]))
                .is_some_and(|(f, r)| f <= LEAD_SCATTER_RATIO * r.max(LEAD_SCATTER_FLOOR))
    };
    // It is the watch's when the next window clear of it (its confirmer)
    // passes too: one window of noise can scatter as little as a watch's,
    // two apart don't. The pair may not straddle a found jump, and every
    // window on the grid from it to its confirmer must be there: a pair
    // across the step onto the watch, or across a window missing there,
    // would set noise beside the watch's own (#1418).
    let watch = |k: usize| {
        let Some(c) =
            (k + 1..windows.len()).find(|&c| windows[c].start_s >= windows[k].end_s - 1e-9)
        else {
            return false;
        };
        let unbroken = windows[k..=c]
            .windows(2)
            .all(|p| p[1].start_s - p[0].start_s <= RATE_WINDOW_STEP_S + 1e-9);
        let (from, to) = (windows[k].start_s, windows[c].end_s);
        unbroken && !jumps.iter().any(|&j| j > from && j <= to) && passes(k) && passes(c)
    };
    let last_end = windows.last()?.end_s;
    if watch(0) {
        return None;
    }
    for (k, w) in windows.iter().enumerate().skip(1) {
        if last_end - w.start_s < LEAD_WATCH_MIN_S - 1e-9 {
            return None;
        }
        if watch(k) {
            // Only a jump within one step of it: where the grid before it
            // is broken, a jump further back is in the noise (#1423).
            let after = windows[k - 1].start_s.max(w.start_s - RATE_WINDOW_STEP_S);
            let jump = jumps
                .iter()
                .copied()
                .filter(|&j| j > after && j <= w.start_s);
            return Some(jump.fold(w.start_s, |_, j| j));
        }
    }
    None
}

/// [`watch_lead_s`] for the stretch of `log` from `from_s` to `to_s`
/// (audio seconds), read on its scatter windows as [`measure_masked`]
/// reads them: where the watch starts after a lead of noise, as the audio
/// time of the first beat there. Every whole reading (`analyze`, and a
/// take's entries) leaves its lead out from here.
pub fn lead_s(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[Break],
    masked: &[(f64, f64)],
    from_s: f64,
    to_s: f64,
) -> Option<f64> {
    let to_s = to_s.min(log.duration_s).max(0.0);
    let from_s = from_s.max(0.0).min(to_s);
    let holes = Cuts::new(clock, breaks, masked);
    let map = |t: f64| holes.true_time(t);
    let in_range: Vec<Beat> = log.beats[stretch_beats(&log.beats, from_s, to_s)].to_vec();
    let beats = holes.beats(&in_range);
    let part = window_part_s(&log.amplitude_windows);
    let windows = scatter_windows(&holes, &beats, log.bph, map(from_s), map(to_s), part);
    let jumps: Vec<f64> = log.jumps.iter().map(|&j| map(j)).collect();
    let lead_t = watch_lead_s(&windows, log.bph, &jumps)?;
    in_range
        .iter()
        .find(|b| map(b.time) >= lead_t - 1e-9)
        .map(|b| b.time)
}

#[path = "beat_error_aggregate.rs"]
mod beat_error_aggregate;
pub use beat_error_aggregate::{
    select_beat_errors, select_window_beat_errors, BeatErrorSelection, BeatErrorSources,
};

/// The event used for a reading's selected beat error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeatErrorSource {
    Unlock,
    Drop,
}

impl std::fmt::Display for BeatErrorSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unlock => "unlock",
            Self::Drop => "drop",
        })
    }
}

impl BeatErrorSource {
    /// The factual caveat for falling back to the drop.
    pub fn caveat(self) -> Option<crate::measure::Caveat> {
        self.caveat_why(false)
    }

    /// [`Self::caveat`], naming why: the unlock found `inconsistent`
    /// ([`WindowFigures::unlock_inconsistent`], #1439), else not found.
    pub fn caveat_why(self, inconsistent: bool) -> Option<crate::measure::Caveat> {
        use crate::measure::{Caveat, DropWhy};
        (self == Self::Drop).then_some(Caveat::BeatErrorFromDrop {
            why: if inconsistent {
                DropWhy::UnlockInconsistent
            } else {
                DropWhy::UnlockNotFound
            },
        })
    }
}

/// A selected beat error's label, including legacy readings of unknown origin.
pub fn beat_error_source_label(source: Option<BeatErrorSource>) -> &'static str {
    match source {
        Some(BeatErrorSource::Unlock) => "Beat Error from the Unlock",
        Some(BeatErrorSource::Drop) => "Beat Error from the Drop",
        None => "Beat Error (source not recorded)",
    }
}

/// Choose the unlock where found, otherwise the drop, retaining its source.
pub fn chosen_beat_error(
    unlock_ms: Option<f64>,
    drop_ms: Option<f64>,
) -> Option<(f64, BeatErrorSource)> {
    unlock_ms
        .map(|v| (v, BeatErrorSource::Unlock))
        .or_else(|| drop_ms.map(|v| (v, BeatErrorSource::Drop)))
}

/// The beat error a reading gives, from its signed beat errors: from the
/// unlock (`unlock_ms`) where it was found, else from the drop
/// (`drop_ms`). Signed, ms; both sides are named alike, so the sign means
/// the same from either. Every beat error a take's reading gives is read
/// here or through [`beat_error_size`].
pub fn beat_error_signed(unlock_ms: Option<f64>, drop_ms: Option<f64>) -> Option<f64> {
    chosen_beat_error(unlock_ms, drop_ms).map(|(value, _)| value)
}

/// The beat error a reading shows: [`beat_error_signed`]'s size, ms. The
/// MCP server's leads take theirs here too.
pub fn beat_error_size(unlock_ms: Option<f64>, drop_ms: Option<f64>) -> Option<f64> {
    beat_error_signed(unlock_ms, drop_ms).map(f64::abs)
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
    measure_masked(log, clock, breaks, &[], from_s, to_s)
}

/// [`measure_with_breaks`] leaving out stretches masked by hand (cleaned
/// with [`masked_spans`]). Each mask, like each break's gap, is cut at
/// both ends, so no fit, 10 s window or two-state sample reaches into
/// one; the masks' beats and amplitude windows are left out,
/// the beats expected and the sound listened to leave out their time, and
/// the caveats say how much was masked.
pub fn measure_masked(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[Break],
    masked: &[(f64, f64)],
    from_s: f64,
    to_s: f64,
) -> Measurement {
    measure_reading(log, clock, breaks, masked, (from_s, to_s), &[])
}

/// [`measure_masked`] of a stretch of a take whose readings are `read`
/// (audio seconds, in time order): the stretches its test read, each stop
/// after its settling. A reading too short for its own ± borrows its
/// noise only from inside them, never from the turns, settling or
/// handling between (#1624); with none given (a plain take), from the
/// take around it.
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
pub fn measure_reading(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[Break],
    masked: &[(f64, f64)],
    (from_s, to_s): (f64, f64),
    read: &[(f64, f64)],
) -> Measurement {
    let stops = read;
    // A reversed or out-of-range stretch reads as an empty one.
    let to_s = to_s.min(log.duration_s).max(0.0);
    let from_s = from_s.max(0.0).min(to_s);
    let masked_s = masked_within(masked, from_s, to_s);
    // Every cut (each gap, each mask, each clock step no break explains),
    // the beats outside the masks on true time, and which bins are whole:
    // no fit, window or sample reaches into a cut, and none takes in a
    // ragged edge of beats beside one. Its breaks take a gap under 0 s
    // (the recorder's timestamps put the first sound after a pause before
    // the last one before it) as none: true time does not run backwards.
    let holes = Cuts::new(clock, breaks, masked);
    let on_time = holes.breaks();
    let map = |t: f64| holes.true_time(t);
    let in_range: Vec<Beat> = log.beats[stretch_beats(&log.beats, from_s, to_s)].to_vec();
    let beats = holes.beats(&in_range);
    let cuts = holes.fit_cuts(&beats);
    let (from_t, to_t) = (map(from_s), map(to_s));
    let part = window_part_s(&log.amplitude_windows);
    let windows = scatter_windows(&holes, &beats, log.bph, from_t, to_t, part);
    let rates: Vec<f64> = windows.iter().map(|w| w.fit.rate_s_per_day).collect();
    let rate_samples: Vec<twostate::Sample> = holes
        .windows(
            &beats,
            log.bph,
            from_t,
            to_t,
            RATE_WINDOW_S,
            RATE_WINDOW_S,
            part,
        )
        .iter()
        .map(|w| twostate::Sample {
            start_s: w.start_s,
            end_s: w.end_s,
            value: w.fit.rate_s_per_day,
            ..Default::default()
        })
        .collect();
    let mut amp = stretch_windows(&holes, &log.amplitude_windows, from_s, to_s);
    // A test's stop takes only the windows wholly inside it: one over its
    // edge holds the handling between stops (#1624).
    amp.retain(|w| stop_window_in(w, stops, from_s, to_s));
    let StretchFits {
        fit,
        turned,
        figures,
    } = stretch_fits(&beats, Some(log.bph), &cuts, &amp);
    let cells = window_cells(&holes, holes.true_time(log.duration_s), part, from_t, to_t);
    let figures = floor_figures(figures, &amp, cells);
    let amps: Vec<f64> = amp.iter().filter_map(|w| w.mean()).collect();
    // On true time, as the rate samples are, so a pause is a hole in the
    // two-state blocks.
    let amp_samples: Vec<twostate::Sample> = amp
        .iter()
        .filter_map(|w| {
            Some(twostate::Sample {
                start_s: map(w.start_s),
                end_s: map(w.end_s),
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
    let gaps: f64 = on_time
        .iter()
        // A pause at the stretch's end is in `map(to_s)`, as one at its
        // start is in `map(from_s)`.
        .filter(|b| b.audio_s > from_s && b.audio_s <= to_s)
        .map(|b| b.gap_s)
        .sum();
    // The gaps `map` puts between the first and the last beat read: a
    // pause at either end of the stretch is outside them.
    let read = |t: f64| t >= from_s && t < to_s && !in_mask(masked, t);
    let first_s = log.beats.iter().map(|b| b.time).find(|&t| read(t));
    let last_s = log.beats.iter().rev().map(|b| b.time).find(|&t| read(t));
    let gaps_between: f64 = match (first_s, last_s) {
        (Some(a), Some(b)) => on_time
            .iter()
            .filter(|g| g.audio_s > a && g.audio_s <= b)
            .map(|g| g.gap_s)
            .sum(),
        _ => 0.0,
    };
    let span = map(to_s) - map(from_s) - gaps - masked_s;
    let expected = crate::beats::expected_beats(span, log.bph);
    let clean = beats.iter().filter(|b| b.is_good()).count();
    let clipped = log
        .clipped_beats
        .iter()
        .filter(|&&t| t >= from_s && t < to_s && !in_mask(masked, t))
        .count();
    let listened_s = to_s - from_s - masked_s;
    let jitter_us = scatter_jitter_us(&windows);
    // Under 10 s the beats' averaged sound (outside the masks) says
    // whether a watch is heard.
    let beat_sound = log.beat_sound(
        &in_range
            .iter()
            .filter(|b| !masked.iter().any(|m| b.time >= m.0 && b.time < m.1))
            .copied()
            .collect::<Vec<_>>(),
        &crate::amplitude::AmplitudeConfig {
            lift_deg: log.lift_deg,
            ..Default::default()
        },
    );
    let no_watch = crate::measure::no_watch_reason(jitter_us, log.bph, listened_s, beat_sound);
    let mut m = Measurement {
        start_s: from_s,
        end_s: to_s,
        breaks: breaks
            .iter()
            .filter(|b| b.audio_s > from_s && b.audio_s < to_s)
            .copied()
            .collect(),
        masked_s,
        beats: beats.len(),
        clean_fraction: (clean as f64 / expected.max(1.0)).min(1.0),
        calibrated: clock.is_some_and(ClockFit::corrects),
        clock_ppm: None,
        clock_source: None,
        rate_s_per_day: fit.map(|f| f.rate_s_per_day),
        beat_error_ms: fit.map(|f| f.beat_error_ms),
        beat_error_unlock_ms: figures.beat_error_unlock_ms,
        beat_error_source: None,
        amplitude_deg: figures.amplitude_deg,
        amplitude_even_deg: figures.amplitude_even_deg,
        amplitude_odd_deg: figures.amplitude_odd_deg,
        jitter_us,
        watch_heard: no_watch.is_none(),
        rate_window_median_s_per_day: median_of(rates.iter().copied()),
        rate_p05: percentile(&rates, 0.05),
        rate_p95: percentile(&rates, 0.95),
        amplitude_p05: figures.amplitude_deg.and(percentile(&amps, 0.05)),
        amplitude_p95: figures.amplitude_deg.and(percentile(&amps, 0.95)),
        coverage: Default::default(),
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
    // The take's windows nearest the stretch, to borrow the ±'s noise
    // from where it holds too few values (#1470).
    let span = (from_s, to_s, log.duration_s);
    let within = |a: f64, b: f64| read_within(stops, a, b);
    let take_rate = nearest_windows(
        span,
        RATE_FIRST_REACH_S,
        |a, b| {
            within(a, b)
                .into_iter()
                .flat_map(|(a, b)| {
                    let near = holes.beats(&log.beats[stretch_beats(&log.beats, a, b)]);
                    scatter_windows(&holes, &near, log.bph, map(a), map(b), part)
                })
                .collect()
        },
        rate_noise_enough,
    );
    let take_amp = nearest_windows(
        span,
        AMPLITUDE_FIRST_REACH_S,
        |a, b| {
            let parts = within(a, b);
            log.amplitude_windows
                .iter()
                .filter(|w| parts.iter().any(|&(a, b)| stop_window_in(w, stops, a, b)))
                .filter(|w| holes.clear(map(w.start_s), map(w.end_s)))
                .collect()
        },
        |w: &[&AmplitudeWindow]| noise_enough(w),
    );
    let take = crate::measure::TakeWindows {
        rate: &take_rate,
        amplitude: &take_amp,
    };
    let a = crate::measure::assess_in(
        &windows,
        &amp,
        take,
        &crate::measure::Context {
            watch_heard: m.watch_heard,
            no_watch_reason: no_watch.unwrap_or_default(),
            bph_heard: None,
            lift_known: true,
            clock_known: m.calibrated,
            beats: beats.len(),
            expected,
            clipped_fraction: m.clipped_fraction,
            span_s: match (beats.first(), beats.last()) {
                (Some(a), Some(b)) => {
                    b.time - a.time - gaps_between - masked_within(masked, a.time, b.time)
                }
                _ => 0.0,
            },
            listened_s,
            breaks: crate::breaks::events(
                &m.breaks.iter().map(|b| b.audio_s).collect::<Vec<_>>(),
                &log.beats,
                log.bph,
            ),
            sides_by_count: log
                .by_count
                .iter()
                .filter(|&&t| t > m.start_s && t < m.end_s)
                .count(),
            beat_error_as_size: turned && fit.is_some(),
            average_s: None,
        },
    );
    m.confidence = a.confidence;
    if let Some(c) = m.confidence.as_mut() {
        figures.floor_confidence(c);
    }
    m.caveats = a.caveats;
    if masked_s > 0.0 {
        crate::measure::add(
            &mut m.caveats,
            crate::measure::Caveat::Masked { seconds: masked_s },
        );
    }
    if let Some(d) = clock.and_then(ClockFit::sources_disagree) {
        crate::measure::add(&mut m.caveats, d.into());
    }
    // A listed pause in the stretch with no gap in the clock log adds no
    // time, and is said (#646).
    if let Some(count) = clock.map(|c| c.unseen(&m.breaks)).filter(|&n| n > 0) {
        crate::measure::add(
            &mut m.caveats,
            crate::measure::Caveat::PausesUnseen { count },
        );
    }
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
    m.beat_error_source =
        chosen_beat_error(m.beat_error_unlock_ms, m.beat_error_ms).map(|(_, source)| source);
    m.coverage = reading_coverage(
        log,
        &holes,
        &beats,
        &windows,
        &amp,
        from_s,
        to_s,
        m.beat_error_source,
    );
    if m.watch_heard {
        for (reading, coverage) in [
            (crate::confidence::Figure::Rate, m.coverage.rate),
            (crate::confidence::Figure::Amplitude, m.coverage.amplitude),
            (crate::confidence::Figure::BeatError, m.coverage.beat_error),
        ] {
            if coverage.is_thin() {
                crate::measure::add(
                    &mut m.caveats,
                    crate::measure::Caveat::ReadingWindows {
                        reading,
                        windows_used: coverage.windows_used,
                        windows_in_span: coverage.windows_in_span,
                    },
                );
            }
        }
    }
    if let Some(caveat) = m
        .beat_error_source
        .and_then(|s| s.caveat_why(figures.unlock_inconsistent))
    {
        crate::measure::add(&mut m.caveats, caveat);
    }
    m
}

/// A clock fit for a sound card known to be `ppm` fast (negative: slow),
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
    /// Past the simulated null's range ([`crate::longterm::beyond_simulated`]).
    pub beyond_simulated: bool,
    pub explained: f64,
    pub wheel: Option<String>,
    pub nearest_wheel: Option<(String, f64)>,
    /// A wheel whose turn is a whole number of these periods, and the
    /// number: a short, sharp change once per turn also shows here.
    pub fraction_of: Option<(String, u32)>,
    /// A longer cycle found in the same series whose period is this one's
    /// times a whole number, as core's search marks it
    /// ([`crate::longterm::Component::harmonic_of`]): this one is a
    /// harmonic of that cycle, not a cycle of its own.
    pub harmonic_of: Option<crate::longterm::Harmonic>,
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

/// The wheel whose turn a component not tied to a wheel sits at a whole
/// fraction of (1/2 to 1/6), and the n: by core's one whole-fraction test,
/// within one resolution (1/span) in frequency of n turns (#775). Only
/// when exactly one wheel and n fit: a turn about as long as the span
/// lets neighbouring n both fit, and then the fraction is not told (#787).
pub(crate) fn wheel_fraction(
    c: &crate::longterm::Component,
    wheels: &[Wheel],
) -> Option<(String, u32)> {
    if c.wheel.is_some() {
        return None;
    }
    let mut fits = wheels.iter().flat_map(|w| {
        (2..=6u8)
            .filter(|&n| {
                crate::longterm::harmonic_offset(c.period_s, c.resolution_s, n, w.period_s) < 1.0
            })
            .map(|n| (w.name.clone(), u32::from(n)))
    });
    let one = fits.next()?;
    fits.next().is_none().then_some(one)
}

/// The periodic changes `long` found, without their long arrays.
/// `wheels` are the turn periods searched, used to mark a component at a
/// whole fraction of a wheel's turn.
pub fn cycles(r: &LongReport, wheels: &[Wheel]) -> Vec<Cycle> {
    let fraction = |c: &crate::longterm::Component| wheel_fraction(c, wheels);
    let rate = r.rate_components.iter().map(|c| Cycle {
        series: "rate",
        period_s: c.component.period_s,
        size: c.size().0,
        size_unit: c.size().1,
        significance: c.component.significance,
        beyond_simulated: c.component.beyond_simulated,
        explained: c.component.explained,
        wheel: c.component.wheel.clone(),
        nearest_wheel: c.component.nearest_wheel.clone(),
        fraction_of: fraction(&c.component),
        harmonic_of: c.component.harmonic_of,
    });
    let amp = r.amplitude.components.iter().map(|c| Cycle {
        series: "amplitude",
        period_s: c.period_s,
        size: c.peak_to_peak,
        size_unit: "deg",
        significance: c.significance,
        beyond_simulated: c.beyond_simulated,
        explained: c.explained,
        wheel: c.wheel.clone(),
        nearest_wheel: c.nearest_wheel.clone(),
        fraction_of: fraction(c),
        harmonic_of: c.harmonic_of,
    });
    rate.chain(amp).collect()
}

/// One pallet stone's beat shape, from `shape`.
///
/// An analysis-only take stores this in its `timegrapher.analysis/1` file
/// ([`crate::analysis_take`]). A field added later must be an `Option`, so
/// that a file saved before it existed still reads, the field as not
/// measured; nothing missing is filled in with a value.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Both sides' beat shapes. Stored in an analysis-only take, so a field
/// added later must be an `Option`, as for [`ShapeSide`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShapeSummary {
    /// Where in the recording the shape was measured from, seconds; with
    /// `measured_s`, the stretch it was measured on. None in a take kept
    /// as its analysis before it was stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_s: Option<f64>,
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

/// The beat shape `r`, measured on `measured_s` seconds from `from_s`.
pub fn shape_summary(r: &ShapeReport, from_s: f64, measured_s: f64) -> ShapeSummary {
    ShapeSummary {
        from_s: Some(from_s),
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
    /// The supplied beat rate (option, calibre or session file), never a guess.
    /// With no supplied rate, no-watch text gives scatter only in ms (#857).
    pub bph_set: Option<u32>,
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
    /// Source and input counts for this aggregate's beat-error statistic.
    pub beat_error_source: Option<BeatErrorSource>,
    pub beat_error_sources: BeatErrorSources,
    pub caveats: Vec<crate::measure::Caveat>,
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
    /// Mean unsigned beat error over the positions, ms.
    pub beat_error_mean: Option<f64>,
    /// Largest difference between the positions' unsigned beat errors, ms.
    pub d_beat_error: Option<f64>,
    /// Source and input counts for this aggregate's beat-error statistic.
    pub beat_error_source: Option<BeatErrorSource>,
    pub beat_error_sources: BeatErrorSources,
    pub caveats: Vec<crate::measure::Caveat>,
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
    /// Source and input counts for this aggregate's beat-error statistic.
    pub beat_error_source: Option<BeatErrorSource>,
    pub beat_error_sources: BeatErrorSources,
    pub caveats: Vec<crate::measure::Caveat>,
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

fn spread(v: impl Iterator<Item = f64>) -> Option<f64> {
    let w: Vec<f64> = v.filter(|x| x.is_finite()).collect();
    if w.len() < 2 {
        return None;
    }
    let lo = w.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = w.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Some(hi - lo)
}

/// Hours after full wind to a tenth: the states of wind's one key.
pub(crate) fn wind_key(w: Option<f64>) -> i64 {
    (w.unwrap_or(0.0) * 10.0).round() as i64
}

fn state(wind_h: f64, readings: &[&Reading]) -> StateIndices {
    let figures: Vec<SourcedFigures> = readings
        .iter()
        .map(|r| {
            let m = &r.measurement;
            (
                r.position,
                m.rate_s_per_day,
                m.amplitude_deg,
                m.beat_error(),
                m.beat_error_source,
            )
        })
        .collect();
    sourced_indices(wind_h, &figures)
}

/// Witschi's characteristic values for one state of wind `wind_h`, from
/// each reading's figures, repeat readings of a position averaged: the
/// values `session` gives, for any interface that shows them.
#[path = "session_aggregates.rs"]
mod session_aggregates;
pub use session_aggregates::{
    comparison, position_rows, positions_table, sourced_indices, sourced_indices_over, Comparison,
    PositionRow, PositionsTable, SourcedFigures, TakeRead, IE_NAME, IM_NAME, POSITION_MIN_S,
};

fn largest(v: impl Iterator<Item = f64>) -> Option<f64> {
    v.max_by(|a, b| a.abs().total_cmp(&b.abs()))
}

/// Stretches of a take, start and end (s), in time order.
pub type Spans<'a> = &'a [(f64, f64)];

/// Stretches `spans` of a take (in time order) read together as one
/// reading, as [`measure_masked`] reads a take: from the first's start to
/// the last's end, with the time between them and the `masked` stretches
/// left out (cleaned with [`masked_spans`], so masked time is counted
/// once). Its `masked_s` is all that is left out; its masked caveat says
/// only the time masked by hand inside the stretches (#1228). None for no
/// stretches. What a Positions Test table reads each position by, and the
/// app its whole take. A reading too short for its own ± borrows its
/// noise from inside the take's `read` stretches ([`measure_reading`]):
/// all the stops of its test, other positions' too (#1624).
pub fn measure_together(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[Break],
    masked: &[(f64, f64)],
    (spans, read): (Spans<'_>, Spans<'_>),
) -> Option<Measurement> {
    let (&(from, _), &(_, to)) = (spans.first()?, spans.last()?);
    let pairs = |v: &[(f64, f64)]| v.iter().map(|&(a, b)| [a, b]).collect::<Vec<_>>();
    let mut cut = masked.to_vec();
    cut.extend(spans.windows(2).map(|w| (w[0].1, w[1].0)));
    let cut = masked_spans(&pairs(&cut));
    let mut m = measure_reading(log, clock, breaks, &cut, (from, to), read);
    let hand = masked_spans(&pairs(masked));
    let by_hand: f64 = spans.iter().map(|&(a, b)| masked_within(&hand, a, b)).sum();
    m.caveats
        .retain(|c| !matches!(c, crate::measure::Caveat::Masked { .. }));
    if by_hand > 0.0 {
        let c = crate::measure::Caveat::Masked { seconds: by_hand };
        crate::measure::add(&mut m.caveats, c);
    }
    Some(m)
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
    evaluate_states(states, findings(readings))
}

/// [`evaluate`] from the states of wind already read, fullest first,
/// with the conditions `findings`: for figures kept without their
/// readings, such as a saved Positions Test's.
pub fn evaluate_states(states: Vec<StateIndices>, findings: Vec<Finding>) -> SessionReport {
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
                let selected = select_beat_errors([
                    a.1.beat_error_ms.zip(a.1.beat_error_source),
                    b.1.beat_error_ms.zip(b.1.beat_error_source),
                ]);
                isochronism.push(Isochronism {
                    position: p,
                    from_wind_h: a.0.wind_h,
                    to_wind_h: b.0.wind_h,
                    rate_change: b.1.rate_s_per_day.unwrap() - a.1.rate_s_per_day.unwrap(),
                    amplitude_change: b.1.amplitude_deg.zip(a.1.amplitude_deg).map(|(x, y)| x - y),
                    beat_error_change: (selected.values.len() == 2)
                        .then(|| selected.values[1].1 - selected.values[0].1),
                    beat_error_source: selected.beat_error_source,
                    beat_error_sources: selected.beat_error_sources,
                    caveats: selected.caveats(),
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
            // The positions with a rate in both states, so each mean is
            // over the same positions.
            let common = |s: &StateIndices, o: &StateIndices| {
                mean_of(
                    s.positions
                        .iter()
                        .filter(|v| {
                            o.positions
                                .iter()
                                .any(|w| w.position == v.position && w.rate_s_per_day.is_some())
                        })
                        .filter_map(|v| v.rate_s_per_day),
                )
            };
            common(a, b).zip(common(b, a)).map(|(x, y)| (y - x).abs())
        }
        _ => None,
    };
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
/// beat found (timing scatter 8.6 ms, 6.9% of a beat)", as every surface
/// states it. `bph_set` is supplied by the user, calibre or session file;
/// without it, only ms are shown, never a percent of a guessed beat.
pub fn no_regular_beat(jitter_us: Option<f64>, bph_set: Option<u32>) -> String {
    let j = jitter_us.filter(|j| j.is_finite());
    crate::beats::no_regular_beat_text(
        j,
        bph_set.and_then(|b| crate::beats::timing_scatter_fraction(j, b)),
    )
}

/// What a recording with no regular beat has none of, said after
/// [`no_regular_beat`] or [`no_regular_beat_in`] where the readings would
/// otherwise stand (#510): "No regular beat found (…). There are no rate,
/// amplitude or beat error readings."
pub const NO_READINGS: &str = "There are no rate, amplitude or beat error readings.";

/// [`no_regular_beat`] for a measurement, or, when its beats' sound shows
/// no unlock or drop on either side, that fact.
pub fn no_regular_beat_in(m: &Measurement, bph_set: Option<u32>) -> String {
    match crate::measure::no_watch_reason_in(&m.caveats) {
        Some(r @ crate::measure::NoWatchReason::NoBeatSound) => {
            crate::measure::no_watch_text(r, None, None)
        }
        _ => no_regular_beat(m.jitter_us, bph_set),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
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
                no_regular_beat_in(m, r.bph_set),
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
        if crate::analysis::Clipping::noted_fraction(m.clipped_fraction) {
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
        let measured = m.measured_s();
        if measured < SHORT_MEASURE_S {
            push(
                "measurement_short",
                ri,
                format!("{measured:.0} s measured"),
                format!("{measured:.0} s measured after settling in {}", at(r)),
            );
        }
        // A sharp event once per period also shows at whole fractions of
        // it; a cycle core marks as a harmonic of another listed one is
        // reported with that one, not on its own, unless it is a wheel's
        // own line or explains more of the variation than that one does:
        // then it is reported in its own right, the mark a note.
        let harmonic_of = |c: &Cycle| {
            let h = c.harmonic_of?;
            r.cycles
                .iter()
                .find(|w| w.series == c.series && w.period_s == h.period_s)
        };
        let folded =
            |c: &Cycle| harmonic_of(c).filter(|w| c.wheel.is_none() && c.explained <= w.explained);
        for c in &r.cycles {
            let size = format!("{} peak to peak", c.size_text());
            let fa = crate::longterm::false_alarm_text(c.significance, c.beyond_simulated, |s| {
                format!("{:.0e}", 10f64.powf(-s.min(300.0)))
            });
            if folded(c).is_some() {
                continue;
            }
            let harmonics: Vec<String> = r
                .cycles
                .iter()
                .filter(|h| folded(h).is_some_and(|w| std::ptr::eq(w, c)))
                .map(|h| format!("{:.2} s", h.period_s))
                .collect();
            let of = if c.wheel.is_some() {
                "the turn"
            } else {
                "its period"
            };
            let mut harmonics = if harmonics.is_empty() {
                String::new()
            } else {
                format!(
                    "; also at whole fractions of {of} ({})",
                    harmonics.join(", ")
                )
            };
            if let (Some(w), Some(h)) = (harmonic_of(c), c.harmonic_of) {
                let than = if w.explained < c.explained {
                    "weaker"
                } else {
                    "stronger"
                };
                harmonics += &format!(
                    "; at 1/{} of the {:.2} s {} cycle, a {than} line",
                    h.n, w.period_s, w.series
                );
            }
            match &c.wheel {
                Some(w) => push(
                    "cycle_wheel",
                    ri,
                    format!("Regular {} change once per turn of the {w}", c.series),
                    format!(
                        "{} repeats every {:.2} s ({size}, {:.0}% of the variation) in {}; false-alarm chance {fa}{harmonics}",
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
                            "{} repeats every {:.2} s ({size}) in {}; false-alarm chance {fa}{harmonics}",
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
                        "{} repeats every {:.2} s ({size}) in {}; false-alarm chance {fa}{}{harmonics}",
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

/// The message for a session file's old `card_ppm`, ppm slow (#657).
pub const OLD_CARD_PPM: &str = "card_ppm is now card_ppm_fast, positive when the sound card \
     runs fast: write card_ppm_fast with the old value's sign changed";

fn refuse_card_ppm<'de, D: serde::Deserializer<'de>>(_: D) -> Result<Option<()>, D::Error> {
    Err(serde::de::Error::custom(OLD_CARD_PPM))
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
    /// Whether the accompanying lift was supplied rather than assumed.
    /// Absent metadata retains each reader's legacy inference.
    pub lift_known: Option<bool>,
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
    /// A known sound-card error for recordings with no clock log: ppm fast
    /// (negative: slow), as [`crate::clock::card_scale`] defines it.
    #[serde(rename = "card_ppm_fast")]
    pub card_ppm: Option<f64>,
    /// Refused: session files written before #657 gave the error as
    /// `card_ppm`, ppm slow. A file that still does is not read, so a
    /// saved take never changes meaning unseen.
    #[serde(
        default,
        rename = "card_ppm",
        deserialize_with = "refuse_card_ppm",
        skip_serializing
    )]
    pub old_card_ppm: Option<()>,
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
    /// False for a take kept as its analysis alone, with no sound: its
    /// beats are read from `analysis/` (see [`crate::analysis_take`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<bool>,
    /// The positions as recorded, where painting with the app's Position
    /// Brush changed them: one entry per stretch, as the recordings list
    /// them. The app reads them to bring the recorded positions back;
    /// readings take the positions from `recording`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recorded: Vec<RecordedPosition>,
}

/// One stretch in one position as recorded, before any painting.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedPosition {
    pub position: String,
    /// Start, once settled after the change of position, seconds into the
    /// sound; left out means its start.
    pub start_s: Option<f64>,
    /// End, seconds into the sound; left out means its end.
    pub end_s: Option<f64>,
}

impl Manifest {
    /// Check the four persisted analysis scalars at a manifest consumption
    /// boundary. Parsing, construction and serialization do not call this.
    /// Valid fields keep their values and precedence; no watch ranges are added.
    pub fn validate_analysis_inputs(&self) -> Result<(), String> {
        if let Some(value) = self.bph {
            crate::input_domain::bph(u64::from(value)).map_err(|e| format!("bph: {e}"))?;
        }
        if let Some(value) = self.lift {
            crate::input_domain::lift_deg(value).map_err(|e| format!("lift: {e}"))?;
        }
        if let Some(value) = self.escape_teeth {
            crate::input_domain::escape_teeth(value).map_err(|e| format!("escape_teeth: {e}"))?;
        }
        if let Some(value) = self.card_ppm {
            crate::input_domain::card_ppm(value).map_err(|e| format!("card_ppm_fast: {e}"))?;
        }
        Ok(())
    }
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
    /// Clock log for this recording (see docs/reference/long-runs.md).
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
    /// Stretches left out of the readings by hand, as `[start, end]` pairs
    /// in seconds into the file or folder, such as a bump of the stand:
    /// `masked = [[600.0, 612.5]]`. They are cut out like breaks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub masked: Vec<[f64; 2]>,
    pub notes: Option<String>,
    pub reference: Option<Reference>,
    /// The Complication Test's stretch this entry is, `"off"` or `"on"`
    /// (#1620): a take keeps its test's three stretches so they are read
    /// again when it is opened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complication: Option<String>,
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

#[cfg(test)]
mod manifest_input_tests {
    use super::*;

    /// A session file written before #657 gave the card's error as
    /// `card_ppm`, ppm slow: it is refused, saying how to write it now,
    /// rather than read with its meaning turned over.
    #[test]
    fn an_old_card_ppm_is_refused_with_how_to_write_it() {
        let e = toml::from_str::<Manifest>("card_ppm = 19.3\n")
            .unwrap_err()
            .to_string();
        assert!(e.contains(OLD_CARD_PPM), "{e}");
        let m: Manifest = toml::from_str("card_ppm_fast = -19.3\n").unwrap();
        assert_eq!(m.card_ppm, Some(-19.3));
        let text = toml::to_string(&m).unwrap();
        assert!(text.contains("card_ppm_fast = -19.3"), "{text}");
    }

    /// A window belongs to the stretch holding its midpoint, the start in
    /// and the end out, so two stretches that meet share none (#632).
    #[test]
    fn a_window_belongs_to_the_one_stretch_holding_its_midpoint() {
        let w = |a: f64| AmplitudeWindow {
            start_s: a,
            end_s: a + 2.0,
            even_deg: None,
            odd_deg: None,
            beat_error_ms: None,
            beat_error_unlock_ms: None,
            even_unlock_ms: None,
            odd_unlock_ms: None,
            even_drop_ms: None,
            odd_drop_ms: None,
            culled_ms: None,
        };
        // Midpoint 11 s: in 10..20 and 11..12, not 0..11 or 12..20.
        assert!(window_in(&w(10.0), 10.0, 20.0));
        assert!(window_in(&w(10.0), 11.0, 12.0));
        assert!(!window_in(&w(10.0), 0.0, 11.0));
        assert!(!window_in(&w(10.0), 12.0, 20.0));
        // Half in, half out: in the stretch holding its middle only.
        assert!(window_in(&w(9.5), 10.0, 20.0));
        assert!(!window_in(&w(8.5), 10.0, 20.0));
        assert!(!window_in(&w(19.0), 10.0, 20.0));
        assert!(window_in(&w(19.0), 20.0, 30.0));
    }

    #[test]
    fn no_regular_beat_without_a_set_rate_keeps_only_the_scatter_in_ms() {
        assert_eq!(
            no_regular_beat(Some(8600.0), None),
            "No regular beat found (timing scatter 8.6 ms)"
        );
        assert_eq!(
            crate::measure::no_watch_text(
                crate::measure::NoWatchReason::Scatter,
                Some(8600.0),
                None
            ),
            no_regular_beat(Some(8600.0), None)
        );
    }

    /// With no timing scatter the sentence is beats.rs's own, as with one.
    #[test]
    fn no_regular_beat_is_the_one_sentence_without_a_scatter() {
        let none = crate::beats::no_regular_beat_text(None, None);
        assert_eq!(no_regular_beat(None, Some(28800)), none);
        assert_eq!(no_regular_beat(Some(f64::NAN), Some(28800)), none);
        assert_eq!(
            no_regular_beat(Some(8600.0), Some(28800)),
            "No regular beat found (timing scatter 8.6 ms, 6.9% of a beat)"
        );
    }

    #[test]
    fn a_stored_shape_missing_an_optional_field_reads_it_as_not_measured() {
        // As an analysis-only take saved before an `Option` field existed
        // would hold it: the field reads as none, and nothing else changes.
        let mut v = serde_json::json!({
            "measured_s": 30.0,
            "even": {
                "i12_ms": 2.5, "i13_ms": 6.0, "ratio13": 0.4, "ratio23": 0.7,
                "noise_ratio": 0.1, "sound1_found": 0.9,
                "extra_pre": [[1.5, 0.2]], "extra_post": [],
            },
        });
        v["odd"] = v["even"].clone();
        v["odd"].as_object_mut().unwrap().remove("ratio23");
        let s: ShapeSummary = serde_json::from_value(v).unwrap();
        assert_eq!(s.odd.ratio23, None);
        assert_eq!(s.even.ratio23, Some(0.7));
        assert_eq!(s.odd.sound1_found, 0.9);
        assert_eq!(s.odd.extra_pre, vec![(1.5, 0.2)]);
        // A field that is not an `Option` is never filled in.
        let mut w = serde_json::to_value(&s).unwrap();
        w["odd"].as_object_mut().unwrap().remove("sound1_found");
        assert!(serde_json::from_value::<ShapeSummary>(w).is_err());
    }

    #[test]
    fn consumed_manifest_domains_reject_invalid_scalars() {
        let invalid = Manifest {
            bph: Some(0),
            ..Default::default()
        };
        assert!(invalid
            .validate_analysis_inputs()
            .unwrap_err()
            .starts_with("bph:"));
        let invalid = Manifest {
            escape_teeth: Some(0),
            ..Default::default()
        };
        assert!(invalid
            .validate_analysis_inputs()
            .unwrap_err()
            .starts_with("escape_teeth:"));
        for value in [
            0.0,
            -0.0,
            -1.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MIN_POSITIVE,
            9.99,
            90.01,
            f64::MAX,
        ] {
            let invalid = Manifest {
                lift: Some(value),
                ..Default::default()
            };
            assert!(invalid
                .validate_analysis_inputs()
                .unwrap_err()
                .starts_with("lift:"));
        }
        for value in [
            1_000_000.0,
            1_000_001.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let invalid = Manifest {
                card_ppm: Some(value),
                ..Default::default()
            };
            assert!(invalid
                .validate_analysis_inputs()
                .unwrap_err()
                .starts_with("card_ppm_fast:"));
        }
    }

    #[test]
    fn consumed_manifest_domains_preserve_defaults_valid_extremes_and_context() {
        Manifest::default().validate_analysis_inputs().unwrap();
        for value in [10.0, 52.0, 90.0] {
            for ppm in [999_999.999_999_999_9, 12.5, 0.0, -0.0, -12.5, -f64::MAX] {
                let m = Manifest {
                    bph: Some(u32::MAX),
                    lift: Some(value),
                    lift_known: Some(false),
                    escape_teeth: Some(u32::MAX),
                    card_ppm: Some(ppm),
                    calibre: Some("Unknown descriptive label".into()),
                    ..Default::default()
                };
                let before = serde_json::to_value(&m).unwrap();
                m.validate_analysis_inputs().unwrap();
                assert_eq!(
                    serde_json::to_value(&m).unwrap(),
                    before,
                    "validation must not normalize values/context"
                );
            }
        }
        // Consumption validation is explicit, not a Deserialize/Serialize hook.
        let m: Manifest = serde_json::from_str("{\"bph\":0,\"lift\":0}").unwrap();
        assert!(m.validate_analysis_inputs().is_err());
        assert_eq!(serde_json::to_value(&m).unwrap()["bph"], 0);
        let orphan: Manifest =
            serde_json::from_str("{\"lift_known\":true,\"calibre\":\"label\",\"settle_s\":-1}")
                .unwrap();
        orphan.validate_analysis_inputs().unwrap();
        assert_eq!(orphan.lift, None); // Context and interval policy are unchanged.
    }
}

#[cfg(test)]
mod beat_error_tests {
    use super::*;

    /// A take whose unlock is never found gives its beat error from the
    /// drop, signed as the drop gives it; one whose unlock is heard gives
    /// the unlock's, with the same sign.
    #[test]
    fn a_take_whose_unlock_is_never_found_gives_the_drops_beat_error() {
        let read = |audio: crate::audio::Audio| {
            let log = crate::stream::analyze_samples(
                &audio.samples,
                audio.sample_rate,
                &crate::stream::StreamConfig::default(),
            );
            measure(&log, None, 0.0, f64::INFINITY)
        };
        let m = read(crate::synth::unlock_unheard(60.0, 0.8));
        assert_eq!(m.beat_error_unlock_ms, None, "the unlock is never found");
        let drop = m.beat_error_ms.expect("the drop's beat error");
        assert_eq!(m.beat_error_signed(), Some(drop));
        assert_eq!(m.beat_error(), Some(drop.abs()));
        assert!((drop - 0.8).abs() < 0.05, "{drop}");
        let heard = read(crate::synth::generate(
            &crate::synth::SynthConfig {
                duration_s: 60.0,
                beat_error_ms: 0.8,
                ..Default::default()
            },
            |_| 250.0,
            |_| 0.0,
        ));
        let unlock = heard.beat_error_unlock_ms.expect("the unlock's beat error");
        assert_eq!(heard.beat_error_signed(), Some(unlock));
        assert_eq!(unlock.signum(), drop.signum(), "{unlock} and {drop}");
    }
}

#[cfg(test)]
mod beat_error_source_tests {
    use crate::{analyze, audio, library, power_reserve, session, stream, synth, AnalysisConfig};
    use serde_json::Value;

    fn check(audio: audio::Audio, source: &str) {
        let log = stream::analyze_samples(
            &audio.samples,
            audio.sample_rate,
            &stream::StreamConfig::default(),
        );
        let measurement = session::measure(&log, None, 0.0, log.duration_s);
        let mut summary = analyze(&audio, &AnalysisConfig::default()).summary;
        let hours = power_reserve::hours(&log, None, &[], &[], 0.0, log.duration_s);
        let dir = std::env::temp_dir().join(format!("tg-source-{source}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        audio::write_wav(&dir.join("take.wav"), &audio).unwrap();
        std::fs::write(
            dir.join("session.toml"),
            "[[recording]]\nfile = \"take.wav\"\nposition = \"DU\"\n",
        )
        .unwrap();
        let kept = library::reading(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        old_reading(&kept);
        old_reading(&hours[0]);
        for (name, value) in [
            ("measurement", serde_json::to_value(&measurement).unwrap()),
            ("summary", serde_json::to_value(&summary).unwrap()),
            ("hour", serde_json::to_value(&hours[0]).unwrap()),
            ("library", serde_json::to_value(&kept).unwrap()),
        ] {
            assert_eq!(value["beat_error_source"], source, "{name}");
            let drop = value["caveats"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["code"] == "beat_error_from_drop");
            assert_eq!(drop, source == "drop", "{name}");
        }
        summary.withhold_readings();
        let withheld = serde_json::to_value(summary).unwrap();
        assert_eq!(withheld.get("beat_error_source"), Some(&Value::Null));
        assert!(withheld["caveats"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| { c["code"] != "beat_error_from_drop" }));
    }

    fn old_reading<T: serde::Serialize + serde::de::DeserializeOwned>(reading: &T) {
        let mut old = serde_json::to_value(reading).unwrap();
        let fields = old.as_object_mut().unwrap();
        fields.remove("beat_error_source");
        fields.remove("caveats");
        let restored: T = serde_json::from_value(old).unwrap();
        let restored = serde_json::to_value(restored).unwrap();
        assert_eq!(restored.get("beat_error_source"), Some(&Value::Null));
        assert_eq!(restored["caveats"], serde_json::json!([]));
    }

    #[test]
    fn unlock_source_travels_with_direct_readings() {
        check(
            synth::generate(
                &synth::SynthConfig {
                    duration_s: 30.0,
                    beat_error_ms: 0.8,
                    seed: 658,
                    ..Default::default()
                },
                |_| 250.0,
                |_| 0.0,
            ),
            "unlock",
        );
    }

    #[test]
    fn unheard_unlock_uses_drop_with_a_factual_caveat() {
        check(synth::unlock_unheard(30.0, 0.8), "drop");
    }
}

/// Count the same grids and cut membership used by the selected figures.
#[allow(clippy::too_many_arguments)]
fn reading_coverage(
    log: &BeatLog,
    holes: &Cuts,
    beats: &[Beat],
    rate: &[WindowFit],
    amp: &[&AmplitudeWindow],
    from: f64,
    to: f64,
    source: Option<BeatErrorSource>,
) -> crate::confidence::FigureCoverage {
    use crate::confidence::{Coverage, FigureCoverage};
    let rate_total = beats.first().map_or(0, |b| {
        crate::clean::nominal_windows(
            crate::clean::lay_from(b.time, holes.true_time(from), RATE_WINDOW_STEP_S),
            holes.true_time(to),
            RATE_WINDOW_S,
            RATE_WINDOW_STEP_S,
        )
        .filter(|&(a, b)| holes.clear(a, b))
        .count()
    });
    let amp_total = window_cells(
        holes,
        holes.true_time(log.duration_s),
        window_part_s(&log.amplitude_windows),
        holes.true_time(from),
        holes.true_time(to),
    );
    FigureCoverage {
        rate: Coverage {
            windows_used: rate.len(),
            windows_in_span: rate_total,
        },
        amplitude: Coverage {
            windows_used: amp.iter().filter(|w| w.mean().is_some()).count(),
            windows_in_span: amp_total,
        },
        beat_error: Coverage {
            windows_used: amp
                .iter()
                .filter(|w| match source {
                    Some(BeatErrorSource::Unlock) => w.beat_error_unlock_ms.is_some(),
                    Some(BeatErrorSource::Drop) => w.beat_error_ms.is_some(),
                    None => false,
                })
                .count(),
            windows_in_span: amp_total,
        },
    }
}

#[cfg(test)]
mod reading_coverage_tests {
    use super::*;

    fn log() -> BeatLog {
        let cfg = crate::synth::SynthConfig {
            duration_s: 60.0,
            seed: 645,
            ..Default::default()
        };
        let audio = crate::synth::generate(&cfg, |_| 270.0, |_| 0.0);
        let mut log =
            crate::stream::analyze_samples(&audio.samples, audio.sample_rate, &Default::default());
        // Exact beat spacing makes both sides of a mask use the same rate grid.
        for (i, beat) in log.beats.iter_mut().enumerate() {
            beat.time = i as f64 * 3600.0 / log.bph as f64;
            beat.index = i as i64;
        }
        for w in &mut log.amplitude_windows {
            w.even_deg = Some(270.0);
            w.odd_deg = Some(270.0);
            w.beat_error_unlock_ms = Some(0.4);
        }
        log
    }

    fn thin(log: &mut BeatLog, keep: usize) {
        for w in log.amplitude_windows.iter_mut().skip(keep) {
            w.even_deg = None;
            w.odd_deg = None;
            w.beat_error_unlock_ms = None;
        }
    }

    /// [`log`] laid end to end `times` times, beats and windows alike.
    fn long_log(times: usize) -> BeatLog {
        let one = log();
        let span = 60.0;
        let n = one.beats.len() as i64;
        let mut long = one.clone();
        long.beats.clear();
        long.amplitude_windows.clear();
        for k in 0..times {
            let dt = k as f64 * span;
            long.beats.extend(one.beats.iter().map(|b| Beat {
                time: b.time + dt,
                index: b.index + k as i64 * n,
                ..*b
            }));
            long.amplitude_windows
                .extend(one.amplitude_windows.iter().map(|w| AmplitudeWindow {
                    start_s: w.start_s + dt,
                    end_s: w.end_s + dt,
                    ..w.clone()
                }));
        }
        long.duration_s = span * times as f64;
        long
    }

    fn amplitude_caveat(m: &Measurement) -> Option<(usize, usize)> {
        m.caveats.iter().find_map(|c| match c {
            crate::measure::Caveat::ReadingWindows {
                reading: crate::confidence::Figure::Amplitude,
                windows_used,
                windows_in_span,
            } => Some((*windows_used, *windows_in_span)),
            _ => None,
        })
    }

    /// The YM42 run-down's hour 56 in small (#1439): over 10 minutes, four
    /// in five unlock windows read about −5 ms and the rest the watch's
    /// +0.6 ms. The unlock beat error is withheld, the drop's is shown, and
    /// the caveat says the unlock was inconsistent, with no ± for it.
    #[test]
    fn an_inconsistent_unlock_falls_back_to_the_drop() {
        use crate::measure::{Caveat, DropWhy};
        let mut log = long_log(10);
        for (i, w) in log.amplitude_windows.iter_mut().enumerate() {
            let k = i as f64 * 0.001;
            w.beat_error_unlock_ms = Some(if i % 5 == 4 { 0.6 + k } else { -5.0 - k });
        }
        let m = measure(&log, None, 0.0, 600.0);
        assert_eq!(m.beat_error_unlock_ms, None);
        assert_eq!(m.beat_error_source, Some(BeatErrorSource::Drop));
        assert!(m.beat_error_ms.is_some());
        assert!(m.caveats.contains(&Caveat::BeatErrorFromDrop {
            why: DropWhy::UnlockInconsistent
        }));
        assert_eq!(m.confidence.unwrap().beat_error.se, None);
        // Its windows' own values, consistent, keep the unlock.
        for (i, w) in log.amplitude_windows.iter_mut().enumerate() {
            w.beat_error_unlock_ms = Some(0.6 + i as f64 * 0.001);
        }
        let m = measure(&log, None, 0.0, 600.0);
        assert_eq!(m.beat_error_source, Some(BeatErrorSource::Unlock));
    }

    /// Power Reserve's last hour of the YM42 65 h take (#1429): 2 stray
    /// windows of an hour's 1,800 give no amplitude and no unlock beat
    /// error, and the reading still says how many windows it had.
    #[test]
    fn two_windows_of_an_hour_give_no_amplitude() {
        let mut log = long_log(60);
        thin(&mut log, 2);
        let m = measure(&log, None, 0.0, 3600.0);
        assert_eq!(m.coverage.amplitude.windows_in_span, 1800);
        assert_eq!(m.amplitude_deg, None);
        assert_eq!(m.amplitude_even_deg, None);
        assert_eq!(m.amplitude_odd_deg, None);
        assert_eq!(m.amplitude_p05, None);
        assert_eq!(m.beat_error_unlock_ms, None);
        assert_ne!(m.beat_error_source, Some(BeatErrorSource::Unlock));
        let c = m.confidence.unwrap();
        assert_eq!(c.amplitude.se, None);
        assert_eq!(c.beat_error.se, None);
        assert_eq!(amplitude_caveat(&m), Some((2, 1800)));
    }

    /// The share floor: under 10% of the hour's 1,800 windows (180) no
    /// amplitude; at 10% it stands, with its coverage caveat.
    #[test]
    fn an_hour_reads_its_amplitude_from_a_tenth_of_its_windows() {
        for (keep, read) in [(179, false), (180, true)] {
            let mut log = long_log(60);
            thin(&mut log, keep);
            let m = measure(&log, None, 0.0, 3600.0);
            assert_eq!(m.amplitude_deg.is_some(), read, "{keep} windows");
            assert_eq!(m.beat_error_unlock_ms.is_some(), read, "{keep} windows");
            // A withheld figure has no ± either (#1429).
            let c = m.confidence.unwrap();
            assert_eq!(c.amplitude.se.is_some(), read, "{keep} windows");
            assert_eq!(c.beat_error.se.is_some(), read, "{keep} windows");
            assert_eq!(amplitude_caveat(&m), Some((keep, 1800)));
        }
    }

    /// In a minute's 30 windows, 10% is 3: 2 give no amplitude, 3 do.
    #[test]
    fn a_minute_reads_its_amplitude_from_three_windows() {
        for (keep, read) in [(2, false), (3, true)] {
            let mut log = log();
            thin(&mut log, keep);
            let m = measure(&log, None, 0.0, 60.0);
            assert_eq!(m.amplitude_deg.is_some(), read, "{keep} windows");
        }
    }

    /// A withheld figure gives no ± (#1429): `floor_confidence` clears
    /// the amplitude's and the unlock beat error's, and keeps the rate's.
    #[test]
    fn a_withheld_figure_has_no_uncertainty() {
        let mut c = crate::confidence::Confidence::new(Some(0.1), Some(2.0), Some(0.01));
        WindowFigures {
            amplitude_deg: Some(270.0),
            beat_error_unlock_ms: Some(0.4),
            ..Default::default()
        }
        .floor_confidence(&mut c);
        assert_eq!(
            (c.rate.se, c.amplitude.se, c.beat_error.se),
            (Some(0.1), Some(2.0), Some(0.01))
        );
        WindowFigures::default().floor_confidence(&mut c);
        assert_eq!(
            (c.rate.se, c.amplitude.se, c.beat_error.se),
            (Some(0.1), None, None)
        );
    }

    /// `analyze` takes the same floor (#1429).
    #[test]
    fn floor_figures_withholds_each_figure_on_its_own_count() {
        let log = log();
        let mut wins: Vec<AmplitudeWindow> = log.amplitude_windows.clone();
        // Tock read in 2 windows of 30, Tick in all.
        for w in wins.iter_mut().skip(2) {
            w.odd_deg = None;
        }
        let refs: Vec<&AmplitudeWindow> = wins.iter().collect();
        let f = floor_figures(window_figures(&refs, false), &refs, 30);
        assert_eq!(f.amplitude_odd_deg, None);
        assert_eq!(f.amplitude_deg, None);
        assert_eq!(f.amplitude_even_deg, Some(270.0));
        assert!(f.beat_error_unlock_ms.is_some());
    }

    #[test]
    fn missing_measurements_count_against_window_coverage() {
        let mut log = log();
        thin(&mut log, 5);
        let m = measure(&log, None, 0.0, 60.0);
        assert_eq!(m.coverage.amplitude.windows_used, 5);
        assert_eq!(m.coverage.amplitude.windows_in_span, 30);
        assert!(m.caveats.iter().any(|c| matches!(c,
            crate::measure::Caveat::ReadingWindows { reading, windows_used: 5, windows_in_span: 30 }
                if *reading == crate::confidence::Figure::Amplitude)));
    }

    #[test]
    fn two_measured_windows_have_no_uncertainty() {
        let mut log = log();
        thin(&mut log, 2);
        let m = measure(&log, None, 0.0, 60.0);
        assert_eq!(m.coverage.amplitude.windows_used, 2);
        let c = m.confidence.unwrap();
        assert_eq!(c.amplitude.se, None);
        assert_eq!(c.beat_error.se, None);
    }

    #[test]
    fn full_window_coverage_has_no_thin_caveat() {
        let log = log();
        let m = measure(&log, None, 0.0, 60.0);
        assert_eq!(m.coverage.amplitude.windows_used, 30);
        assert_eq!(m.coverage.amplitude.windows_in_span, 30);
        assert_eq!(
            m.coverage.rate.windows_used,
            m.coverage.rate.windows_in_span
        );
        assert_eq!(
            m.coverage.beat_error.windows_used,
            m.coverage.beat_error.windows_in_span
        );
        assert!(m.confidence.unwrap().amplitude.se.is_some());
        assert!(!m
            .caveats
            .iter()
            .any(|c| matches!(c, crate::measure::Caveat::ReadingWindows { .. })));
    }

    /// A stretch whose first beat falls just after a tiling point lays its
    /// rate windows from that point, and counts its span from it too, so
    /// none is used that the span does not hold (#636).
    #[test]
    fn rate_coverage_counts_the_windows_laid_from_the_stretchs_start() {
        let mut log = log();
        for b in &mut log.beats {
            b.time += 0.3;
        }
        let m = measure(&log, None, 0.0, 60.0);
        assert_eq!(m.coverage.rate.windows_used, 11);
        assert_eq!(m.coverage.rate.windows_in_span, 11);
    }

    /// The cells a stretch takes in are counted on true time (#636): on a
    /// sound card well off true time, its ends read on the audio's own
    /// clock would take in another number of cells.
    #[test]
    fn coverage_counts_cells_on_true_time() {
        let log = log();
        let k = 1.0 + 0.005;
        let pairs: Vec<(f64, f64)> = (0..=6)
            .map(|i| {
                let t = i as f64 * 10.0;
                (t, 1.79e9 + t * k)
            })
            .collect();
        let clock = ClockFit::new(&pairs).unwrap();
        let map = crate::longrun::true_time(Some(&clock), &[]);
        let (from, to) = (map(10.0), map(40.9));
        let cells = |a: f64, b: f64| {
            (0..40)
                .filter(|c| (a..b).contains(&(2.0 * *c as f64 + 1.0)))
                .count()
        };
        assert_ne!(
            cells(from, to),
            cells(10.0, 40.0),
            "the test needs ends that differ"
        );
        let m = measure(&log, Some(&clock), 10.0, 40.9);
        assert_eq!(m.coverage.amplitude.windows_in_span, cells(from, to));
    }

    #[test]
    fn mask_coverage_counts_only_the_kept_stretches() {
        let log = log();
        let masked = measure_masked(&log, None, &[], &[(20.0, 40.0)], 0.0, 60.0);
        let a = measure(&log, None, 0.0, 20.0);
        let b = measure(&log, None, 40.0, 60.0);
        for (whole, first, last) in [
            (masked.coverage.rate, a.coverage.rate, b.coverage.rate),
            (
                masked.coverage.amplitude,
                a.coverage.amplitude,
                b.coverage.amplitude,
            ),
            (
                masked.coverage.beat_error,
                a.coverage.beat_error,
                b.coverage.beat_error,
            ),
        ] {
            assert_eq!(
                whole.windows_in_span,
                first.windows_in_span + last.windows_in_span
            );
            assert_eq!(whole.windows_used, first.windows_used + last.windows_used);
        }
        assert_eq!(masked.coverage.amplitude.windows_in_span, 20);
    }
}
