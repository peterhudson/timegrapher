//! The Positions Test: the app leads the watchmaker through the six
//! positions in the app's order, waits for the watch to settle in each,
//! measures it, and fills in a table in its own pane as it goes, with the
//! figures `timegrapher session` reads for the same stretches: each rate,
//! its difference from the mean, amplitude and beat error, and the means.
//! No verdict is given.
//!
//! Each position is a stretch of the one listening session, so the strip,
//! the charts and a saved session keep the positions as any change of
//! position does.

use crate::app::Position;
use crate::theme;
use eframe::egui::{self, RichText, Vec2};
use timegrapher_core::clock::ClockFit;
use timegrapher_core::figure_text::base_digits::{AMPLITUDE, BEAT_ERROR, RATE};
use timegrapher_core::figure_text::{fixed_text, signed_text, true_minus};
use timegrapher_core::session::{self, Measurement, Reading, SourcedFigures};
use timegrapher_core::steadiness::SeriesKind;
use timegrapher_core::stream::BeatLog;
use timegrapher_core::{complication, poise};

/// The file a finished test just after winding is saved to, from the
/// pane or beside a saved take, for the same six positions 24
/// hours later.
pub use timegrapher_core::library::POSITIONS_TEST_FILE as RESULTS_FILE;
/// The file's `format`, versioned: a later version adds keys.
pub const RESULTS_FORMAT: &str = "timegrapher.positions-test/1";

/// A finished test's results as saved: a test just after winding, for
/// the 24 hour half, or a 24 hour half with the full-wind results it was
/// set against.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FullWind {
    /// [`RESULTS_FORMAT`].
    pub format: String,
    /// When the test began: `2026-10-08T21:04:05.123Z`, and Unix seconds.
    pub began_utc: String,
    pub began_unix: f64,
    /// The watch's name and the calibre as they were then, or empty.
    pub watch: String,
    pub calibre: String,
    /// The beat rate and lift angle the test was read at, and its
    /// settling and measuring times in seconds: absent from files saved
    /// before they were kept.
    #[serde(default)]
    pub bph: Option<u32>,
    #[serde(default)]
    pub lift_angle_deg: Option<f64>,
    #[serde(default)]
    pub settle_s: Option<f64>,
    #[serde(default)]
    pub measure_s: Option<f64>,
    /// Each position, in the order measured.
    pub positions: Vec<KeptRow>,
    /// For the 24 hour half: hours since the full-wind test began, to a
    /// tenth, and that test's results, which this one was set against.
    /// Absent for a full-wind test.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wind_h: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub against: Option<Box<FullWind>>,
}

/// One position's figures from the full-wind test.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct KeptRow {
    /// The position's code as the app writes it in session.toml and
    /// session.json: DU, DD, CD, CL, CU, CR.
    pub position: String,
    pub rate_s_per_day: Option<f64>,
    pub amplitude_deg: Option<f64>,
    /// A size, from the unlock where found, ms.
    pub beat_error_ms: Option<f64>,
    /// Older saved rows retain an unknown source.
    #[serde(default)]
    pub beat_error_source: Option<session::BeatErrorSource>,
}

pub fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

impl FullWind {
    /// Hours since this test began, at `now_unix`.
    pub fn hours_ago(&self, now_unix: f64) -> f64 {
        (now_unix - self.began_unix) / 3600.0
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }

    /// Read a file saved by [`FullWind::to_json`], or say why not.
    pub fn from_json(text: &str) -> Result<FullWind, String> {
        let f: FullWind =
            serde_json::from_str(text).map_err(|e| format!("not a saved Positions Test ({e})"))?;
        if f.format != RESULTS_FORMAT {
            return Err(format!("a format this app doesn't read ({})", f.format));
        }
        if f.positions.is_empty() {
            return Err("no positions in it".into());
        }
        Ok(f)
    }
}

/// The full-wind tests kept in the takes folder `takes` (each take's
/// [`RESULTS_FILE`]) that began [`FULL_WIND_HOURS`] before `now_unix`,
/// newest first: those a watch's 24 hours after test can be set against,
/// for the one testing to pick from. A 24 hour half's results are never
/// among them.
pub fn full_winds_in(takes: &std::path::Path, now_unix: f64) -> Vec<FullWind> {
    let Ok(dir) = std::fs::read_dir(takes) else {
        return Vec::new();
    };
    let mut out: Vec<FullWind> = dir
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path().join(RESULTS_FILE)).ok())
        .filter_map(|t| FullWind::from_json(&t).ok())
        .filter(|f| f.wind_h.is_none())
        .filter(|f| FULL_WIND_HOURS.contains(&f.hours_ago(now_unix)))
        .collect();
    out.sort_by(|a, b| b.began_unix.total_cmp(&a.began_unix));
    out
}

/// A full-wind test as the list to pick from names it: when it began and
/// what it measured, "2026-10-27 18:58 UTC · 6 positions", or
/// "… · Dial Up only" for a test stopped after one.
pub fn choice_text(full: &FullWind) -> String {
    let measured = match full.positions.as_slice() {
        [one] => format!(
            "{} only",
            Position::from_name(&one.position).map_or(one.position.as_str(), |p| p.name())
        ),
        all => format!("{} positions", all.len()),
    };
    format!("{} · {measured}", began_text(&full.began_utc))
}

/// How long before a 24 hours after test its full-wind test began, hours.
pub const FULL_WIND_HOURS: std::ops::RangeInclusive<f64> = 12.0..=36.0;

/// [`FULL_WIND_HOURS`] in words: "12 to 36 hours ago".
pub fn full_wind_ago() -> String {
    format!(
        "{:.0} to {:.0} hours ago",
        FULL_WIND_HOURS.start(),
        FULL_WIND_HOURS.end()
    )
}

/// Seconds the watch is left to settle after it is turned, and then
/// measured, unless set otherwise in the pane: the project's choice. A
/// minute brings the rate's ± near half a second a day. The settling is
/// core's, as `session` reads a take.
pub use timegrapher_core::take::SETTLE_S;
pub const MEASURE_S: f64 = 60.0;
/// The times the pane lets be set, seconds.
pub const SETTLE_RANGE: std::ops::RangeInclusive<f64> = 5.0..=600.0;
pub const MEASURE_RANGE: std::ops::RangeInclusive<f64> = 20.0..=3600.0;

/// The watch and how it was read, for the saved and copied results.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct About {
    /// The watch's name and the calibre, or empty when not set.
    pub watch: String,
    pub calibre: String,
    pub bph: Option<u32>,
    pub lift_angle_deg: f64,
}

/// The app's order: the two horizontals, then crown down, left, up, right.
pub const ORDER: [Position; 6] = [
    Position::DialUp,
    Position::DialDown,
    Position::CrownDown,
    Position::CrownLeft,
    Position::CrownUp,
    Position::CrownRight,
];

/// The Dynamic Poise test's order: the eight upright stops, 45° apart,
/// turned the same way round as the Positions Test turns the four (#1474).
pub const POISE_ORDER: [Position; 8] = [
    Position::CrownDown,
    Position::Up7h30,
    Position::CrownLeft,
    Position::Up4h30,
    Position::CrownUp,
    Position::Up1h30,
    Position::CrownRight,
    Position::Up10h30,
];

/// Which guided test: the six positions, the eight upright stops of the
/// Dynamic Poise test, or the Complication Test's Off, On, Off in one
/// position. All run the same way; they differ in their steps, their
/// names and their tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Positions,
    Poise,
    /// Off, On, Off with the watch in one position (#1576).
    Complication,
}

impl Kind {
    /// The positions in the order the test leads the watch through them.
    /// The Complication Test's stay in the position the card shows, so
    /// they are set as the test is made ([`Test::complication`]).
    pub fn order(self) -> &'static [Position] {
        match self {
            Kind::Positions => &ORDER,
            Kind::Poise => &POISE_ORDER,
            Kind::Complication => &[],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Positions => "Positions Test",
            Kind::Poise => "Dynamic Poise Test",
            Kind::Complication => "Complication Test",
        }
    }

    /// What the strip says over time the test didn't use.
    pub fn not_used(self) -> &'static str {
        match self {
            Kind::Positions => NOT_USED,
            Kind::Poise => NOT_USED_POISE,
            Kind::Complication => NOT_USED_COMPLICATION,
        }
    }

    /// A position's code on the test's buttons and chips: the Positions
    /// Test's DU … CR, and the hour at the top for the upright stops, as
    /// the stand names them.
    pub fn code(self, p: Position) -> &'static str {
        match self {
            Kind::Positions | Kind::Complication => p.code(),
            Kind::Poise => p.witschi(),
        }
    }
}

/// Where the test is in the position it is on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    /// Seconds of settling left.
    Settling(f64),
    /// Seconds measured so far.
    Measuring(f64),
    /// Measured; waiting for the watch to be turned to the next position.
    Turn,
    /// Paused by the watchmaker: this position is settled and measured
    /// again from the start on Resume.
    Paused,
    Finished,
}

/// What the strip says over the time the test didn't use: the settling
/// in each position, the rest of the time in a position that wasn't
/// measured (waiting to be turned, a measurement started again), and the
/// time before the test began.
pub const SETTLING: &str = "Settling";
pub const NOT_USED: &str = "Not Used for Positions Test";
pub const NOT_USED_POISE: &str = "Not Used for Dynamic Poise Test";
pub const NOT_USED_COMPLICATION: &str = "Not Used for Complication Test";
pub const BEFORE_TEST: &str = "Before Test";

/// What the pane's buttons asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Next,
    Redo,
    Pause,
    Resume,
    /// Set aside the earlier results loaded for the 24 hour half.
    ClearEarlier,
}

pub struct Test {
    pub kind: Kind,
    /// The position of each step, in order: the kind's, or the
    /// Complication Test's one position three times.
    order: Vec<Position>,
    pub bph_set: Option<u32>,
    pub step: usize,
    /// When the watch was put in this step's position, seconds of the
    /// session's sound; set on the first frame that has a session.
    start: Option<f64>,
    /// When the test began, seconds of the session's sound: the first
    /// position's first start.
    began: Option<f64>,
    /// The measured stretch of each position done so far.
    spans: Vec<(Position, f64, f64)>,
    /// The time the test didn't use, from the steps before this one:
    /// start, end and why ([`SETTLING`], or [`NOT_USED`] while the watch
    /// waited to be turned or a measurement was started again).
    unused: Vec<(f64, f64, &'static str)>,
    /// Each position's reading as soon as it is measured, in order.
    done: Vec<Reading>,
    /// The six readings, once the last position is measured.
    pub readings: Option<Vec<Reading>>,
    /// Seconds of settling and of measuring in each position.
    pub settle_s: f64,
    pub measure_s: f64,
    /// Paused at this second of the session's sound.
    paused: Option<f64>,
    /// When the test was started, Unix seconds.
    began_unix: f64,
    /// For the 24 hour half: the full-wind test it follows, and the hours
    /// since that one began, to a tenth (the readings' `wind_h`).
    pub full: Option<(FullWind, f64)>,
}

impl Test {
    pub fn new() -> Self {
        Test {
            kind: Kind::Positions,
            order: ORDER.to_vec(),
            bph_set: None,
            step: 0,
            start: None,
            began: None,
            spans: Vec::new(),
            unused: Vec::new(),
            done: Vec::new(),
            readings: None,
            settle_s: SETTLE_S,
            measure_s: MEASURE_S,
            paused: None,
            began_unix: unix_now(),
            full: None,
        }
    }

    /// A Dynamic Poise test: the eight upright stops.
    pub fn poise() -> Self {
        Test {
            kind: Kind::Poise,
            order: POISE_ORDER.to_vec(),
            ..Test::new()
        }
    }

    /// A Complication Test: the complication off, on, then off again,
    /// with the watch kept at `p` throughout.
    pub fn complication(p: Position) -> Self {
        Test {
            kind: Kind::Complication,
            order: vec![p; complication::ORDER.len()],
            ..Test::new()
        }
    }

    /// The positions this test leads the watch through, in order.
    pub fn order(&self) -> &[Position] {
        &self.order
    }

    /// Step `i`'s code on the buttons and chips: a position's code, or
    /// the Complication Test's Off and On.
    pub fn step_code(&self, i: usize) -> &'static str {
        match self.kind {
            Kind::Complication => complication::ORDER.get(i).map_or("", |s| s.name()),
            k => self.order.get(i).map_or("", |&p| k.code(p)),
        }
    }

    /// Step `i` named in words: the position, or "Complication On".
    pub fn step_name(&self, i: usize) -> String {
        match self.kind {
            Kind::Complication => format!("Complication {}", self.step_code(i)),
            _ => self
                .order
                .get(i)
                .map_or(String::new(), |p| p.name().to_string()),
        }
    }

    /// How the watch is for step `i`, after "once the watch is" or "with":
    /// "dial up", or "the complication on".
    fn step_state(&self, i: usize) -> String {
        match self.kind {
            Kind::Complication => {
                format!("the complication {}", self.step_code(i).to_lowercase())
            }
            _ => self.step_name(i).to_lowercase(),
        }
    }

    /// The same test with its own settling and measuring times, seconds.
    pub fn timed(mut self, settle_s: f64, measure_s: f64) -> Self {
        self.settle_s = settle_s;
        self.measure_s = measure_s;
        self
    }

    /// The same six positions again, after the full-wind test `full`,
    /// with the watch neither wound nor worn since.
    pub fn after(full: FullWind) -> Self {
        let mut t = Test::new();
        let h = (full.hours_ago(t.began_unix) * 10.0).round() / 10.0;
        t.full = Some((full, h.max(0.1)));
        t
    }

    /// Hours since the watch was fully wound: none for a full-wind test.
    pub fn wind_h(&self) -> Option<f64> {
        self.full.as_ref().map(|f| f.1)
    }

    /// The finished Positions Test's results, to keep beside its take: a
    /// full-wind test's for the 24 hour half, or a 24 hour half's with the
    /// results it was set against. None for an unfinished test.
    pub fn kept(&self, about: &About) -> Option<FullWind> {
        if self.kind != Kind::Positions {
            return None;
        }
        let readings = self.readings.as_ref()?;
        Some(FullWind {
            format: RESULTS_FORMAT.into(),
            began_utc: self.began_utc(),
            began_unix: self.began_unix,
            watch: about.watch.trim().to_string(),
            calibre: about.calibre.clone(),
            bph: about.bph,
            lift_angle_deg: Some(about.lift_angle_deg),
            settle_s: Some(self.settle_s),
            measure_s: Some(self.measure_s),
            positions: readings
                .iter()
                .map(|r| KeptRow {
                    position: r.position.common().to_string(),
                    rate_s_per_day: r.measurement.rate_s_per_day,
                    amplitude_deg: r.measurement.amplitude_deg,
                    beat_error_ms: r.measurement.beat_error(),
                    beat_error_source: r.measurement.beat_error_source,
                })
                .collect(),
            wind_h: self.wind_h(),
            against: self.full.as_ref().map(|f| Box::new(f.0.clone())),
        })
    }

    /// When the test began: `2026-10-08T21:04:05.123Z`.
    pub fn began_utc(&self) -> String {
        timegrapher_core::recorder::utc(
            std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(self.began_unix),
        )
    }

    /// When the test began, seconds of the session's sound: the first
    /// step's first start. None before its first frame.
    pub fn began_s(&self) -> Option<f64> {
        self.began
    }

    pub fn position(&self) -> Position {
        let order = self.order();
        order[self.step.min(order.len() - 1)]
    }

    /// The measured stretch of each position done so far: position,
    /// start and end in seconds of the session.
    pub fn stretches(&self) -> &[(Position, f64, f64)] {
        &self.spans
    }

    pub fn next_position(&self) -> Option<Position> {
        self.order().get(self.step + 1).copied()
    }

    /// Move the test on to `now`, seconds of the session's sound. Records
    /// the position's measured stretch once it is complete.
    pub fn tick(&mut self, now: f64) -> Phase {
        if self.readings.is_some() {
            return Phase::Finished;
        }
        if self.paused.is_some() {
            return Phase::Paused;
        }
        let start = *self.start.get_or_insert(now);
        self.began.get_or_insert(start);
        let (from, to) = self.window(start);
        if now < from {
            return Phase::Settling(from - now);
        }
        if now < to {
            return Phase::Measuring(now - from);
        }
        if self.spans.len() <= self.step {
            self.spans.push((self.position(), from, to));
        }
        Phase::Turn
    }

    /// Stop settling or measuring this position at `now`, until Resume.
    pub fn pause(&mut self, now: f64) {
        if self.paused.is_some() || self.readings.is_some() {
            return;
        }
        self.close_step(now, true);
        self.spans.truncate(self.step);
        self.done.truncate(self.step);
        self.start = None;
        self.paused = Some(now);
    }

    /// Settle and measure this position again, from `now`.
    pub fn resume(&mut self, now: f64) {
        if let Some(p) = self.paused.take() {
            if now > p {
                self.unused.push((p, now, self.kind.not_used()));
            }
            self.start = Some(now);
        }
    }

    /// Whether a position's stretch is measured but not yet read.
    pub fn needs_reading(&self) -> bool {
        self.done.len() < self.spans.len()
    }

    /// Read each position measured since the last call, so the pane's
    /// table fills in as the test goes.
    pub fn read_new(&mut self, log: &BeatLog, clock: Option<&ClockFit>, masked: &[(f64, f64)]) {
        while self.done.len() < self.spans.len() {
            let i = self.done.len();
            let (_, from, to) = self.spans[i];
            match self.reading(i, from, to, log, clock, masked) {
                Some(r) => self.done.push(r),
                None => break,
            }
        }
    }

    /// The readings so far: each position measured, in order.
    pub fn readings_so_far(&self) -> &[Reading] {
        self.readings.as_deref().unwrap_or(&self.done)
    }

    /// On to the next position, from `now`. Returns it, or None after the
    /// last, when the readings are to be made.
    pub fn advance(&mut self, now: f64) -> Option<Position> {
        self.close_step(now, false);
        self.step += 1;
        self.start = Some(now);
        self.next_or_none()
    }

    fn next_or_none(&self) -> Option<Position> {
        self.order().get(self.step).copied()
    }

    /// Measure this position again from `now`.
    pub fn redo(&mut self, now: f64) {
        if self.paused.is_some() {
            return self.resume(now);
        }
        self.close_step(now, true);
        self.spans.truncate(self.step);
        self.done.truncate(self.step);
        self.start = Some(now);
    }

    /// The time the test doesn't use, up to `now`: the time before it
    /// began, each settling, and what came between a measurement's end (or
    /// a measurement given up) and the next start. The strip marks it so
    /// the sound outside the measured stretches reads as left out.
    pub fn unused(&self, now: f64) -> Vec<(f64, f64, &'static str)> {
        let mut out: Vec<_> = self
            .began
            .filter(|&b| b > 0.0)
            .map(|b| (0.0, b, BEFORE_TEST))
            .into_iter()
            .collect();
        out.extend(self.unused.iter().copied());
        // Once finished, the last step was closed as the test ended.
        if let (Some(s), None) = (self.start, &self.readings) {
            out.extend(self.step_unused(s, now, false));
        }
        if let Some(p) = self.paused.filter(|&p| now > p) {
            out.push((p, now, self.kind.not_used()));
        }
        out
    }

    /// One attempt at a position, started at `start`, seen at `now`.
    fn step_unused(&self, start: f64, now: f64, given_up: bool) -> Vec<(f64, f64, &'static str)> {
        let mut out = Vec::new();
        if now <= start {
            return out;
        }
        let (settled, end) = self.window(start);
        out.push((start, now.min(settled), SETTLING));
        if given_up && now > settled {
            out.push((settled, now, self.kind.not_used()));
        } else if now > end {
            out.push((end, now, self.kind.not_used()));
        }
        out
    }

    /// The test ended at `now` before it finished: what this position
    /// measured so far isn't used either.
    pub fn stop(&mut self, now: f64) {
        // A position already measured, waiting to be turned, keeps its
        // measurement: only the wait since is unused.
        let measured = self.spans.len() > self.step;
        self.close_step(now, !measured);
        if let Some(p) = self.paused.take().filter(|&p| now > p) {
            self.unused.push((p, now, self.kind.not_used()));
        }
        self.start = None;
    }

    /// Keep this attempt's unused time, at `now` as it ends.
    fn close_step(&mut self, now: f64, given_up: bool) {
        if let Some(s) = self.start {
            let u = self.step_unused(s, now, given_up);
            self.unused.extend(u);
        }
    }

    /// The stretches the test measures, up to `now`: each position done,
    /// and the one under way as far as it has got.
    pub fn measured(&self, now: f64) -> Vec<(f64, f64)> {
        let mut out: Vec<(f64, f64)> = self.spans.iter().map(|&(_, a, b)| (a, b)).collect();
        if let (Some(s), None, true) = (self.start, &self.readings, self.spans.len() <= self.step) {
            let (from, to) = self.window(s);
            out.extend((now > from).then(|| (from, now.min(to))));
        }
        out
    }

    /// What a position started at `start` measures, as core places a session entry's.
    fn window(&self, start: f64) -> (f64, f64) {
        let (settle, measure) = (Some(self.settle_s), Some(self.measure_s));
        timegrapher_core::take::measured(start, f64::INFINITY, settle, measure)
    }

    #[cfg(test)]
    fn measured_count(&self) -> usize {
        self.spans.len()
    }

    /// Read each position's stretch of the session, as `session` reads a
    /// recording after its settling time.
    pub fn finish(&mut self, log: &BeatLog, clock: Option<&ClockFit>, masked: &[(f64, f64)]) {
        let readings = self
            .spans
            .iter()
            .enumerate()
            .filter_map(|(i, &(_, from, to))| self.reading(i, from, to, log, clock, masked))
            .collect();
        self.readings = Some(readings);
    }

    /// Read every position measured so far again, as when the stretches
    /// masked change; a finished test is finished again.
    pub fn reread(&mut self, log: &BeatLog, clock: Option<&ClockFit>, masked: &[(f64, f64)]) {
        self.done.clear();
        self.read_new(log, clock, masked);
        if self.readings.is_some() {
            self.finish(log, clock, masked);
        }
    }

    /// Step `i`'s stretch of the session, `from` to `to` seconds, leaving
    /// out the stretches `masked` as `session` does.
    fn reading(
        &self,
        i: usize,
        from: f64,
        to: f64,
        log: &BeatLog,
        clock: Option<&ClockFit>,
        masked: &[(f64, f64)],
    ) -> Option<Reading> {
        let p = *self.order.get(i)?;
        let position = session::Position::parse(p.code())?;
        // Cut at every jump in the beats, as the whole take is, so no fit
        // runs across one.
        let jumps = crate::steady::reading_breaks(Vec::new(), log);
        let measurement: Measurement =
            session::measure_masked(log, clock, &jumps, masked, from, to);
        Some(Reading {
            bph_set: self.bph_set,
            label: self.step_name(i),
            position,
            wind_h: self.wind_h(),
            date: None,
            notes: None,
            duration_s: to - from,
            bph: log.bph,
            measurement,
            cycles: Vec::new(),
            shape: None,
            reference: None,
        })
    }
}

/// The title for the position under way: "Dial Up (1 of 6)", and in the
/// 24 hour half "Dial Up (1 of 6, 24.1 h After Full Wind)".
fn step_title(test: &Test) -> String {
    let i = test.step.min(test.order().len() - 1);
    let (name, n, of) = (test.step_name(i), test.step + 1, test.order().len());
    match test.wind_h() {
        Some(h) => format!("{name} ({n} of {of}, {h:.1} h After Full Wind)"),
        None => format!("{name} ({n} of {of})"),
    }
}

/// The step panel at the top of the pane while the test runs, one height
/// throughout: the position, a chip for each of the six (done, now, to
/// come), a progress bar for its settling and measuring, one line of what
/// happens next, and Pause and Restart. Stop Test… is on the Listen card.
pub(crate) fn banner(
    ui: &mut egui::Ui,
    test: &Test,
    phase: Phase,
    note: Option<&str>,
) -> Option<Action> {
    let pal = theme::pal(ui);
    let mut action = None;
    let frame = theme::card()
        .fill(pal.accent.gamma_multiply(0.14))
        .inner_margin(egui::Margin::symmetric(14, 10));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        let (title, mut detail, frac) = phase_words(test, phase);
        if let Some(n) = note {
            detail = format!("{detail} · {n}");
        }
        let (first_w, restart_w) = slot_widths(ui, test);
        // The step's buttons on the title's line, in reach in the shortest
        // pane; the chips and what is happening on the line under it.
        let slots_w = first_w + restart_w + 2.0 * ui.spacing().item_spacing.x;
        ui.horizontal(|ui| {
            let title_w = (ui.available_width() - slots_w).max(40.0);
            ui.allocate_ui_with_layout(
                Vec2::new(title_w, 26.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_width(title_w);
                    ui.add(
                        egui::Label::new(
                            RichText::new(&title)
                                .font(theme::semibold(15.0))
                                .color(pal.text),
                        )
                        .truncate(),
                    );
                },
            );
            action = step_buttons(ui, test, phase, first_w, restart_w);
        });
        // One line, whole in its hover: the panel never grows.
        ui.horizontal(|ui| detail_line(ui, &detail, test));
        ui.add_space(4.0);
        ui.add(
            egui::ProgressBar::new(frac.clamp(0.0, 1.0))
                .desired_height(6.0)
                .fill(pal.accent),
        );
    });
    action
}

/// The step panel's title, what is happening, and how far through its
/// settling and measuring the position is.
fn phase_words(test: &Test, phase: Phase) -> (String, String, f32) {
    let (settle, measure) = (test.settle_s, test.measure_s);
    match phase {
        Phase::Settling(left) => (
            step_title(test),
            format!("Letting the watch settle, {left:.0} s"),
            ((settle - left) / (settle + measure)) as f32,
        ),
        Phase::Measuring(done) => (
            step_title(test),
            format!("Measuring, {done:.0} of {measure:.0} s"),
            ((settle + done) / (settle + measure)) as f32,
        ),
        Phase::Paused => (
            step_title(test),
            format!(
                "Paused. Resume settles and measures {} again from the start; until \
                 then the strip marks the time {}.",
                test.step_name(test.step),
                test.kind.not_used()
            ),
            0.0,
        ),
        Phase::Turn => match test.next_position() {
            Some(_) if test.kind == Kind::Complication => {
                let next = test.step_code(test.step + 1);
                (
                    format!("Turn the complication {}", next.to_lowercase()),
                    format!(
                        "then press Start {next} Measurement. Nothing until then is \
                         measured; the strip marks it {}.",
                        test.kind.not_used()
                    ),
                    1.0,
                )
            }
            Some(p) if test.kind == Kind::Poise => (
                format!("Done: {}", step_title(test)),
                format!(
                    "Turn the watch 45°, to {} ({}) at the top, then press Start {} \
                     Measurement. Until then the strip marks it Not Used.",
                    p.name(),
                    p.witschi(),
                    p.witschi(),
                ),
                1.0,
            ),
            Some(p) => (
                format!("Turn the watch to {}", p.name()),
                format!(
                    "then press Start {} Measurement. Nothing until then is measured; \
                     the strip marks it Not Used for Positions Test.",
                    p.code()
                ),
                1.0,
            ),
            None => (
                match test.kind {
                    Kind::Positions => "All six positions measured",
                    Kind::Poise => "All eight positions measured",
                    Kind::Complication => "All three stretches measured",
                }
                .to_string(),
                "Reading them together and keeping the take…".to_string(),
                1.0,
            ),
        },
        Phase::Finished => (format!("{} done", test.kind.name()), String::new(), 1.0),
    }
}

/// The widths of the step panel's two button slots, each as wide as the
/// widest button it holds.
fn slot_widths(ui: &egui::Ui, test: &Test) -> (f32, f32) {
    let pal = theme::pal(ui);
    // Each button has its own slot, as wide as the widest it holds, so
    // none moves as the phase changes.
    let pad = 2.0 * ui.spacing().button_padding.x;
    let widest = |ui: &egui::Ui, words: &mut dyn Iterator<Item = String>| {
        let font = theme::semibold(13.0);
        words
            .map(|w| ui.fonts_mut(|f| f.layout_no_wrap(w, font.clone(), pal.text).size().x))
            .fold(84.0_f32, |a, w| a.max(w + pad))
    };
    let steps = 0..test.order().len();
    let first_w = widest(
        ui,
        &mut steps
            .clone()
            .map(|i| format!("Start {} Measurement", test.step_code(i)))
            .chain(["Resume", "Pause"].map(String::from)),
    );
    let restart_w = widest(
        ui,
        &mut steps.map(|i| format!("Restart {} Measurement", test.step_code(i))),
    );
    (first_w, restart_w)
}

/// A button at its own width at the left of a slot `w` wide.
fn slot(ui: &mut egui::Ui, w: f32, b: egui::Button<'_>) -> egui::Response {
    let size = Vec2::new(w, 26.0);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(size);
            ui.add(b)
        },
    )
    .inner
}

/// The step's buttons for this phase, each in its slot.
fn step_buttons(
    ui: &mut egui::Ui,
    test: &Test,
    phase: Phase,
    first_w: f32,
    restart_w: f32,
) -> Option<Action> {
    let (settle, measure) = (test.settle_s, test.measure_s);
    let code = test.step_code(test.step);
    let name = test.step_name(test.step);
    let state = test.step_state(test.step);
    let mut action = None;
    match phase {
        // The sixth measured: the test ends by itself.
        Phase::Turn if test.next_position().is_none() => {
            ui.allocate_space(Vec2::new(0.0, 26.0));
        }
        Phase::Turn => {
            let n = test.step + 1;
            let (next, next_hint) = match test.next_position() {
                Some(_) => (
                    format!("Start {} Measurement", test.step_code(n)),
                    format!(
                        "Press once the watch is {}: it settles for {settle:.0} s, \
                         then {} is measured for {measure:.0} s",
                        test.step_state(n),
                        test.step_name(n)
                    ),
                ),
                None => (String::new(), String::new()),
            };
            if slot(ui, first_w, theme::primary(ui, &next))
                .on_hover_text(next_hint)
                .clicked()
            {
                action = Some(Action::Next);
            }
            if slot(
                ui,
                restart_w,
                theme::secondary(ui, &format!("Restart {code} Measurement")),
            )
            .on_hover_text(format!(
                "The {name} measurement was disturbed (a knock, a noise): put the \
                     watch back {state} and settle and measure it again"
            ))
            .clicked()
            {
                action = Some(Action::Redo);
            }
        }
        Phase::Paused => {
            if slot(ui, first_w, theme::primary(ui, "Resume"))
                .on_hover_text(format!(
                    "Settle and measure {name} again from now, with the watch {state}"
                ))
                .clicked()
            {
                action = Some(Action::Resume);
            }
            // Restart's slot stays, empty.
            ui.add_space(restart_w + ui.spacing().item_spacing.x);
        }
        Phase::Settling(_) | Phase::Measuring(_) => {
            if slot(ui, first_w, theme::secondary(ui, "Pause"))
                .on_hover_text(format!(
                    "Hold the test here, to deal with the watch or the room. What \
                     {name} measured so far isn't used: Resume settles and measures it \
                     again from the start."
                ))
                .clicked()
            {
                action = Some(Action::Pause);
            }
            if slot(
                ui,
                restart_w,
                theme::secondary(ui, &format!("Restart {code} Measurement")),
            )
            .on_hover_text(format!(
                "Settle and measure {name} again from now, as after a knock"
            ))
            .clicked()
            {
                action = Some(Action::Redo);
            }
        }
        Phase::Finished => {
            ui.allocate_space(Vec2::new(0.0, 26.0));
        }
    }
    action
}

/// What is happening, in one line whole in its hover, and the chips.
fn detail_line(ui: &mut egui::Ui, detail: &str, test: &Test) {
    let pal = theme::pal(ui);
    let chips_w = test.order().len() as f32 * (chip_w(test.kind) + 4.0) + 8.0;
    let room = ui.available_width();
    let detail_w = if room >= chips_w + 60.0 {
        room - chips_w
    } else {
        room
    };
    ui.allocate_ui_with_layout(
        Vec2::new(detail_w, 18.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_width(detail_w);
            ui.add(
                egui::Label::new(RichText::new(detail).small().color(pal.text_secondary))
                    .truncate(),
            )
            .on_hover_text(detail);
        },
    );
    step_chips(ui, test);
}

/// The six positions as chips after what the step is doing: done, the one
/// under way, and those to come, each its code. Left out where the panel
/// has no room for them.
fn step_chips(ui: &mut egui::Ui, test: &Test) {
    let pal = theme::pal(ui);
    let font = egui::FontId::proportional(11.0);
    let (w, h, gap) = (chip_w(test.kind), 16.0, 4.0);
    let all = test.order().len() as f32 * (w + gap);
    if ui.available_width() < all + 8.0 {
        return;
    }
    ui.add_space(ui.available_width() - all);
    let done = test.stretches().len();
    for i in 0..test.order().len() {
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), egui::Sense::hover());
        let (fill, ink) = if i < done {
            (pal.accent.gamma_multiply(0.35), pal.text)
        } else if i == test.step {
            (pal.accent, egui::Color32::WHITE)
        } else {
            (egui::Color32::TRANSPARENT, pal.text_tertiary)
        };
        ui.painter().rect(
            rect,
            4.0,
            fill,
            egui::Stroke::new(1.0_f32, pal.accent.gamma_multiply(0.6)),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            test.step_code(i),
            font.clone(),
            ink,
        );
        let state = if i < done {
            "measured"
        } else if i == test.step {
            "under way"
        } else {
            "to come"
        };
        resp.on_hover_text(format!("{}: {state}", test.step_name(i)));
    }
}

/// A chip's width: room for "10H30" in the Dynamic Poise test.
fn chip_w(kind: Kind) -> f32 {
    match kind {
        Kind::Positions | Kind::Complication => 26.0,
        Kind::Poise => 38.0,
    }
}

/// The figures as the pane writes them, by core's `figure_text`: signed
/// or plain, with a true minus, no sign on a zero, a dash when missing.
fn signed(v: Option<f64>, d: usize) -> String {
    v.map_or_else(|| "—".to_string(), |x| true_minus(&signed_text(x, d)))
}

fn plain(v: Option<f64>, d: usize) -> String {
    v.map_or_else(|| "—".to_string(), |x| true_minus(&fixed_text(x, d)))
}

/// A table's title, in the pane's own type.
fn heading(ui: &mut egui::Ui, text: &str, hint: &str) {
    let pal = theme::pal(ui);
    ui.label(RichText::new(text).strong().color(pal.text))
        .on_hover_text(hint);
}

/// A figure at the right of a cell of its own width, so the tables'
/// columns line up and their stripes end with the last column.
fn right(ui: &mut egui::Ui, text: String, strong: bool, color: egui::Color32) -> egui::Response {
    right_in(ui, FIGURE_W, text, strong, color)
}

/// [`right`] in a cell `w` wide.
fn right_in(
    ui: &mut egui::Ui,
    w: f32,
    text: String,
    strong: bool,
    color: egui::Color32,
) -> egui::Response {
    let size = Vec2::new(w, ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.set_min_size(size);
            let t = RichText::new(text).color(color);
            ui.label(if strong { t.strong() } else { t })
        },
    )
    .inner
}

/// The position column's heading, at the left, as wide as the longest
/// name.
fn position_head(ui: &mut egui::Ui) {
    let pal = theme::pal(ui);
    let size = Vec2::new(LABEL_W, ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(size);
            ui.label(theme::caption("Position").color(pal.text_secondary));
        },
    );
}

/// A figure column's heading, at the right like its figures: its name,
/// and its unit on its own line under it (Hugh's ruling on #1086, as #1052
/// set for axes); `unit` empty for none.
fn column_head(ui: &mut egui::Ui, w: f32, name: &str, unit: &str, hint: &str) {
    column_head_swatch(ui, w, name, unit, hint, None);
}

/// [`column_head`], with a swatch of `swatch` before the name: for a
/// column whose figures are in the text colour because the reading's own
/// is too faint for small text (beat error's amber in light mode).
fn column_head_swatch(
    ui: &mut egui::Ui,
    w: f32,
    name: &str,
    unit: &str,
    hint: &str,
    swatch: Option<egui::Color32>,
) {
    let pal = theme::pal(ui);
    // Never narrower than its words, so a long heading can't run into the
    // one before it.
    let w = w.max(head_w(ui, name) + if swatch.is_some() { 12.0 } else { 0.0 });
    let line = Vec2::new(w, ui.spacing().interact_size.y);
    // The grid's row takes the height asked for, not what overflows it:
    // room for the unit's line (Walter's walk of #1110).
    let lines = if unit.is_empty() { 1.0 } else { 2.0 };
    let cell = Vec2::new(w, lines * line.y);
    let r = ui
        .allocate_ui_with_layout(cell, egui::Layout::top_down(egui::Align::Max), |ui| {
            ui.set_min_width(w);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.allocate_ui_with_layout(
                line,
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(theme::caption(name).color(pal.text_secondary));
                    if let Some(c) = swatch {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, 2.0, c);
                    }
                },
            );
            if !unit.is_empty() {
                ui.label(RichText::new(unit).color(pal.text_secondary));
            }
        })
        .response;
    ui.interact(
        r.rect,
        ui.id().with(("column head", name)),
        egui::Sense::hover(),
    )
    .on_hover_text(hint);
}

/// How wide a column's heading is in its own type.
fn head_w(ui: &egui::Ui, text: &str) -> f32 {
    let font = theme::semibold(12.5);
    let color = theme::pal(ui).text;
    ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, color).size().x)
}

/// The width of a figure's column, and of the position names'.
const FIGURE_W: f32 = 88.0;
/// The gap between a table's columns.
const GAP: f32 = 16.0;
/// A figure column's width, and the gap, in the tight Dynamic Poise
/// table: each heading still fits, as [`column_head`] keeps it.
const TIGHT_FIGURE_W: f32 = 54.0;
const TIGHT_GAP: f32 = 10.0;
/// The Dynamic Poise table's width at its widths, and tight.
pub(crate) const TABLE_W: f32 = 590.0;
pub(crate) const TIGHT_TABLE_W: f32 = 470.0;
/// The comparison's figure columns, whose headings are longer.
const WIDE_W: f32 = 100.0;
const LABEL_W: f32 = 150.0;

/// One test's table: a row for each of its positions, in order. The
/// first `t.rows.len()` hold figures; `status` names what the next row is
/// doing (settling, measuring) while the test runs. Then the means, and
/// for the Positions Test the largest difference, vertical less
/// horizontal and any conditions. The Dynamic Poise table calls its means
/// Mean, as its words do, and has no largest difference, an NIHS figure
/// for the six (Hugh's spec).
fn table_grid(
    ui: &mut egui::Ui,
    (id, kind): (&str, Kind),
    t: &Table,
    status: Option<String>,
    live: Option<&FreeCell>,
) {
    let pal = theme::pal(ui);
    let poise = kind == Kind::Poise;
    let (mean_word, from_word) = if poise {
        ("Mean", "From Mean")
    } else {
        ("Average", "From Average")
    };
    let (fw, from_w, gap) = widths(ui, poise, from_word);
    // The dimming's key over the table, as the Power Reserve table's;
    // the Dynamic Poise pane's is in its Info panel (#1537).
    (!poise).then(|| crate::reserve::dim_key(ui, t.beat_error_dimmed.iter().any(|d| *d)));
    let be_hint = crate::reserve::beat_error_hint(
        "As a size, from the unlock, or from the drop where the unlock isn't found.",
    );
    egui::Grid::new(id)
        .num_columns(5)
        .spacing(Vec2::new(gap, 5.0))
        .min_col_width(LABEL_W.min(fw))
        .striped(true)
        .show(ui, |ui| {
            head_row(ui, fw, from_word, &be_hint);
            let mut status = status;
            for (i, p) in kind.order().iter().enumerate() {
                match t.rows.get(i) {
                    Some((label, rate, from_mean, amp, be)) => {
                        ui.label(RichText::new(label).color(pal.text));
                        right_in(ui, fw, signed(*rate, RATE), false, pal.text);
                        right_in(ui, from_w, signed(*from_mean, RATE), false, pal.text);
                        right_in(ui, fw, plain(*amp, AMPLITUDE), false, pal.text);
                        let be_ink = if t.beat_error_dimmed.get(i).copied().unwrap_or(false) {
                            pal.text_secondary
                        } else {
                            pal.text
                        };
                        right_in(ui, fw, plain(*be, BEAT_ERROR), false, be_ink).on_hover_text(
                            t.beat_error_hovers.get(i).map(String::as_str).unwrap_or(""),
                        );
                    }
                    None => {
                        let label = position_label(
                            session::Position::parse(p.code()).unwrap_or(session::Position::CH),
                        );
                        match (status.take(), live) {
                            // Being measured, with 10 s or more: its figures
                            // so far, in the colour of the words under way.
                            (Some(_), Some(FreeCell::Read(rate, amp, be))) => {
                                ui.label(RichText::new(label).strong().color(pal.text))
                                    .on_hover_text("Being measured: its figures so far");
                                right_in(ui, fw, signed(*rate, RATE), false, pal.accent);
                                ui.label("");
                                right_in(ui, fw, plain(*amp, AMPLITUDE), false, pal.accent);
                                right_in(ui, fw, plain(*be, BEAT_ERROR), false, pal.accent);
                            }
                            (Some(s), _) => {
                                ui.label(RichText::new(label).strong().color(pal.text));
                                status_cell(ui, fw, s, pal.accent);
                                for _ in 0..3 {
                                    ui.label("");
                                }
                            }
                            (None, _) => {
                                ui.label(RichText::new(label).color(pal.text_tertiary));
                                for _ in 0..4 {
                                    ui.label("");
                                }
                            }
                        }
                    }
                }
                ui.end_row();
            }
            if !t.rows.is_empty() {
                ui.label(RichText::new(mean_word).strong().color(pal.text));
                right_in(ui, fw, signed(t.mean_rate, RATE), true, pal.text);
                ui.label("");
                right_in(ui, fw, plain(t.mean_amplitude, AMPLITUDE), true, pal.text);
                right_in(ui, fw, plain(t.mean_beat_error, BEAT_ERROR), true, pal.text)
                    .on_hover_text(&t.beat_error_summary);
                ui.end_row();
            }
            if t.rows.len() >= 2 && !poise {
                largest_label(ui);
                right_in(ui, fw, plain(t.largest_difference, RATE), true, pal.text);
                ui.label("");
                right_in(
                    ui,
                    fw,
                    plain(t.largest_amplitude, AMPLITUDE),
                    true,
                    pal.text,
                );
                right_in(
                    ui,
                    fw,
                    plain(t.largest_beat_error, BEAT_ERROR),
                    true,
                    pal.text,
                );
                ui.end_row();
            }
        });
    (!poise).then(|| table_notes(ui, t));
}

/// What the Dynamic Poise pane's Info panel says of its table (Hugh on
/// #1537): the grey beat errors' key where one is grey, then the notes
/// the Positions Test's table has under it.
pub(crate) fn poise_table_notes(readings: &[Reading]) -> Vec<String> {
    let t = table(readings, Kind::Poise);
    let dimmed = t.beat_error_dimmed.iter().any(|d| *d);
    dimmed
        .then(|| crate::reserve::DIM_KEY_TEXT.to_string())
        .into_iter()
        .chain(t.notes)
        .collect()
}

/// The Complication Test's table (#1576, Hugh's spec): a row for each
/// stretch, Off, On, Off, with amplitude first, then On less Off, core's
/// difference with its ±. `status` names what the next row is doing while
/// the test runs, and `live` its figures so far.
fn complication_grid(ui: &mut egui::Ui, t: &Test, status: Option<String>, live: Option<&FreeCell>) {
    let pal = theme::pal(ui);
    let readings = t.readings_so_far();
    let measurements: Vec<Measurement> = readings.iter().map(|r| r.measurement.clone()).collect();
    egui::Grid::new("complication")
        .num_columns(4)
        .spacing(Vec2::new(GAP, 5.0))
        .min_col_width(FIGURE_W)
        .striped(true)
        .show(ui, |ui| {
            let size = Vec2::new(FIGURE_W, ui.spacing().interact_size.y);
            ui.allocate_ui_with_layout(
                size,
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_size(size);
                    ui.label(theme::caption("Stretch").color(pal.text_secondary));
                },
            );
            for q in complication::SHOWN {
                column_head(
                    ui,
                    FIGURE_W,
                    q.title(),
                    complication::unit_head(q),
                    complication_hint(q),
                );
            }
            ui.end_row();
            let mut status = status;
            for i in 0..t.order().len() {
                let label = t.step_code(i);
                if let Some(r) = readings.get(i) {
                    let f = complication::figures(&r.measurement);
                    ui.label(RichText::new(label).color(pal.text));
                    for q in complication::SHOWN {
                        let cell = right(ui, figure_cell(q, &f), false, pal.text);
                        if q == SeriesKind::BeatError {
                            // Its source, as the Positions table hovers it.
                            let src = r.measurement.beat_error_source;
                            cell.on_hover_text(session::beat_error_source_label(src));
                        }
                    }
                } else {
                    match (status.take(), live) {
                        (Some(_), Some(FreeCell::Read(rate, amp, be))) => {
                            ui.label(RichText::new(label).strong().color(pal.text))
                                .on_hover_text("Being measured: its figures so far");
                            right(ui, plain(*amp, AMPLITUDE), false, pal.accent);
                            right(ui, signed(*rate, RATE), false, pal.accent);
                            right(ui, plain(*be, BEAT_ERROR), false, pal.accent);
                        }
                        (Some(st), _) => {
                            ui.label(RichText::new(label).strong().color(pal.text));
                            right(ui, st, false, pal.accent);
                            ui.label("");
                            ui.label("");
                        }
                        (None, _) => {
                            ui.label(RichText::new(label).color(pal.text_tertiary));
                            for _ in 0..3 {
                                ui.label("");
                            }
                        }
                    }
                }
                ui.end_row();
            }
            if measurements.len() == complication::ORDER.len() {
                ui.label(
                    RichText::new(complication::DIFFERENCE_NAME)
                        .strong()
                        .color(pal.text),
                )
                .on_hover_text(complication::method_note());
                for q in complication::SHOWN {
                    let d = complication::change(q, &measurements).map_or_else(
                        || "—".to_string(),
                        |c| true_minus(&complication::difference_text(q, c.on_less_off)),
                    );
                    right(ui, d, true, pal.text);
                }
                ui.end_row();
            }
        });
}

/// A stretch's figure as the table and Copy write it: core's number at
/// the series' place, the rate signed ([`SeriesKind::number`]).
fn figure_cell(q: SeriesKind, f: &complication::Figures) -> String {
    complication::figure(q, f).map_or_else(|| "—".to_string(), |f| true_minus(&q.number(f.value)))
}

/// A Complication Test column's hover.
fn complication_hint(q: SeriesKind) -> &'static str {
    match q {
        SeriesKind::Amplitude => {
            "The balance's swing, from the lift angle set. A complication that loads the \
             going train lowers it."
        }
        SeriesKind::Rate => "Seconds a day gained (+) or lost (−)",
        SeriesKind::BeatError => {
            "As a size, from the unlock, or from the drop where the unlock isn't found."
        }
    }
}

/// The Complication Test's table as text a spreadsheet takes: a header,
/// a row for each stretch, then On less Off with its ±. Fields are split
/// by `sep`.
fn complication_text(test: &Test, sep: char) -> String {
    let readings = test.readings_so_far();
    let mut out = "Stretch".to_string();
    for q in complication::SHOWN {
        out.push_str(&format!(
            "{sep}{} ({})",
            q.title(),
            complication::unit_head(q)
        ));
    }
    out.push('\n');
    for (i, r) in readings.iter().enumerate() {
        let f = complication::figures(&r.measurement);
        out.push_str(test.step_code(i));
        for q in complication::SHOWN {
            out.push_str(&format!("{sep}{}", figure_cell(q, &f)));
        }
        out.push('\n');
    }
    let m: Vec<Measurement> = readings.iter().map(|r| r.measurement.clone()).collect();
    if m.len() == complication::ORDER.len() {
        out.push_str(complication::DIFFERENCE_NAME);
        for q in complication::SHOWN {
            let d = complication::change(q, &m).map_or_else(
                || "—".to_string(),
                |c| true_minus(&complication::difference_text(q, c.on_less_off)),
            );
            out.push_str(&format!("{sep}{d}"));
        }
        out.push('\n');
        for q in complication::SHOWN {
            if let Some(c) = complication::change(q, &m) {
                out.push_str(&format!(
                    "{}: {}\n",
                    q.title(),
                    true_minus(&complication::finding_text(q, c.on_less_off))
                ));
            }
        }
    }
    out
}

/// What the next row is doing, in its Rate cell: from the cell's left
/// when the table is tight, running on into the From cell the row leaves
/// empty, so it never reaches the position's name.
fn status_cell(ui: &mut egui::Ui, fw: f32, s: String, ink: egui::Color32) {
    if fw < FIGURE_W {
        ui.label(RichText::new(s).color(ink));
    } else {
        right_in(ui, fw, s, false, ink);
    }
}

/// The table's headings: Position, then each figure's name and unit.
fn head_row(ui: &mut egui::Ui, fw: f32, from_word: &str, be_hint: &str) {
    position_head(ui);
    for (h, unit, hint) in [
        ("Rate", "s/d", "Seconds a day gained (+) or lost (−)"),
        (
            from_word,
            "s/d",
            "This position's rate less the mean of the positions measured",
        ),
        (
            "Amplitude",
            "°",
            "The balance's swing, from the lift angle set",
        ),
        ("Beat Error", "ms", be_hint),
    ] {
        column_head(ui, fw, h, unit, hint);
    }
    ui.end_row();
}

/// The figure columns' width, the From column's, and the gap between
/// columns: tight for the Dynamic Poise table when the pane gives it less
/// than [`TABLE_W`], so it can stay beside its picture (Hugh, #1499).
fn widths(ui: &egui::Ui, poise: bool, from_word: &str) -> (f32, f32, f32) {
    let (fw, gap) = if poise && ui.available_width() < TABLE_W {
        (TIGHT_FIGURE_W, TIGHT_GAP)
    } else {
        (FIGURE_W, GAP)
    };
    (fw, fw.max(head_w(ui, from_word)), gap)
}

fn table_notes(ui: &mut egui::Ui, t: &Table) {
    let pal = theme::pal(ui);
    if t.dvh_rate.is_some() || t.dvh_amplitude.is_some() {
        ui.label(
            RichText::new(format!(
                "Vertical less horizontal: {} s/d in rate, {}° in amplitude.",
                signed(t.dvh_rate, RATE),
                signed(t.dvh_amplitude, AMPLITUDE)
            ))
            .color(pal.text),
        )
        .on_hover_text(
            "DVH: the vertical positions' mean less the horizontal ones', for rate and for \
             amplitude",
        );
    }
    for n in &t.notes {
        ui.label(RichText::new(n).color(pal.text));
    }
}

/// The two tests side by side: each position's rate and amplitude at full
/// wind and after, and the change, then Im and Ie.
fn comparison_grid(ui: &mut egui::Ui, c: &Comparison) {
    let pal = theme::pal(ui);
    let h = format!("{:.1} h", c.hours);
    egui::Grid::new("full-wind")
        .num_columns(7)
        .spacing(Vec2::new(16.0, 5.0))
        .min_col_width(LABEL_W.min(FIGURE_W))
        .striped(true)
        .show(ui, |ui| {
            position_head(ui);
            for (head, unit, hint) in [
                (
                    "Rate at Full".to_string(),
                    "s/d",
                    "Rate at full wind (s/d)".to_string(),
                ),
                (
                    format!("Rate at {h}"),
                    "s/d",
                    format!("Rate {h} after full wind (s/d)"),
                ),
                (
                    "Rate Change".to_string(),
                    "s/d",
                    "The later rate less the full-wind one (s/d)".to_string(),
                ),
                (
                    "Amplitude at Full".to_string(),
                    "°",
                    "Amplitude at full wind (degrees)".to_string(),
                ),
                (
                    format!("Amplitude at {h}"),
                    "°",
                    format!("Amplitude {h} after full wind (degrees)"),
                ),
                (
                    "Amplitude Change".to_string(),
                    "°",
                    "The later amplitude less the full-wind one (degrees)".to_string(),
                ),
            ] {
                column_head(ui, WIDE_W, &head, unit, &hint);
            }
            ui.end_row();
            for (label, v) in &c.rows {
                ui.label(RichText::new(label).color(pal.text));
                right_in(ui, WIDE_W, signed(v[0], RATE), false, pal.text);
                right_in(ui, WIDE_W, signed(v[1], RATE), false, pal.text);
                right_in(ui, WIDE_W, signed(v[2], RATE), true, pal.text);
                right_in(ui, WIDE_W, plain(v[3], AMPLITUDE), false, pal.text);
                right_in(ui, WIDE_W, plain(v[4], AMPLITUDE), false, pal.text);
                right_in(ui, WIDE_W, signed(v[5], AMPLITUDE), true, pal.text);
                ui.end_row();
            }
        });
    ui.add_space(4.0);
    ui.label(
        RichText::new(format!(
            "Largest change in rate, crown right left out: {} s/d. Change in the mean rate, as \
             a size: {} s/d.",
            signed(c.im, RATE),
            plain(c.ie, RATE)
        ))
        .color(pal.text),
    )
    .on_hover_text(
        "Im: the largest change over the positions without crown right (12H), as NIHS 93-10 \
         takes it. Ie: the size of the mean rate's change over the positions measured both \
         times.",
    );
}

/// When a saved test began, as the pane names it: "2026-10-08 21:04 UTC".
fn began_text(utc: &str) -> String {
    match (utc.get(..10), utc.get(11..16)) {
        (Some(d), Some(t)) => format!("{d} {t} UTC"),
        _ => utc.to_string(),
    }
}

/// The earlier test's watch, calibre and time, on one line.
fn earlier_line(full: &FullWind) -> String {
    let mut parts = vec![began_text(&full.began_utc)];
    for s in [&full.watch, &full.calibre] {
        if !s.trim().is_empty() {
            parts.push(s.trim().to_string());
        }
    }
    parts.join(" · ")
}

/// The pane's body under its toolbar: the banner while the test runs,
/// the table filling in as each position is measured, and with earlier
/// results loaded, those results and both tests side by side. Returns
/// what a button asked for.
#[allow(clippy::too_many_arguments)]
pub fn pane(
    ui: &mut egui::Ui,
    test: Option<&Test>,
    phase: Option<Phase>,
    earlier: Option<&FullWind>,
    saved: Option<&FullWind>,
    free: Option<&FreeTable>,
    live: Option<&FreeCell>,
    settle_s: f64,
    watch: bool,
    note: Option<&str>,
    (card, info): (&str, bool),
) -> Option<Action> {
    let pal = theme::pal(ui);
    let mut action = None;
    // When the banner comes or goes (a test starts, stops or finishes),
    // the pane starts again from its top, not where it was scrolled.
    let banner_on = test.is_some() && phase.is_some_and(|ph| ph != Phase::Finished);
    let id = ui.id().with("guided banner on");
    let changed = ui.data(|d| d.get_temp::<bool>(id)) != Some(banner_on);
    ui.data_mut(|d| d.insert_temp(id, banner_on));
    let mut area = egui::ScrollArea::both().auto_shrink([false, false]);
    if changed {
        area = area.vertical_scroll_offset(0.0);
    }
    let lines = info_lines(settle_s);
    let panel = |ui: &mut egui::Ui, h: f32| {
        theme::info_text(
            ui,
            egui::Id::new("positions info panel"),
            ("Table", &lines),
            h,
        );
    };
    theme::with_info(ui, info, egui::Id::new("positions info"), panel, |ui| {
        area.show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            if let Some(n) = note {
                ui.label(RichText::new(n).color(pal.text_secondary));
            }
            if let (Some(t), Some(ph)) = (test, phase) {
                if ph != Phase::Finished {
                    action = banner(ui, t, ph, None);
                    ui.add_space(6.0);
                }
            }
            if let Some(t) = test {
                this_test(ui, t, phase, live);
            }
            // An opened 24 hour half shows the run as saved, not read again.
            match (test, saved) {
                (None, Some(s)) => saved_test(ui, s),
                (None, None) => {
                    if let Some(f) = free {
                        this_take(ui, f, card);
                    }
                }
                _ => {}
            }
            let c = match test {
                Some(t) => t
                    .full
                    .as_ref()
                    .and_then(|(f, h)| comparison(f, *h, t.readings_so_far())),
                None => saved.and_then(saved_comparison),
            };
            if let Some(c) = &c {
                ui.add_space(10.0);
                heading(
                    ui,
                    &format!("Full Wind Against {:.1} Hours", c.hours),
                    "The full-wind test loaded at the start, against this one, run without \
                     winding or wearing the watch in between",
                );
                comparison_grid(ui, c);
            }
            if let Some(full) = earlier {
                ui.add_space(10.0);
                if earlier_section(ui, full, phase) {
                    action = Some(Action::ClearEarlier);
                }
            }
            ui.add_space(10.0);
            let about = about_text(test, earlier, (watch, saved.is_some()));
            if !about.is_empty() {
                ui.add(egui::Label::new(RichText::new(about).color(pal.text_secondary)).wrap());
            }
            if let Some(t) = test {
                test_footer(ui, t);
            }
        });
    });
    action
}

/// This test's heading and its table, filling in as each position is
/// measured.
fn this_test(ui: &mut egui::Ui, t: &Test, phase: Option<Phase>, live: Option<&FreeCell>) {
    let title = match t.wind_h() {
        Some(h) => format!("This Test, {h:.1} h After Full Wind"),
        None => "This Test".to_string(),
    };
    heading(
        ui,
        &title,
        "Each position's figures as soon as it is measured, read the same way as \
             the command line reads them",
    );
    test_table(ui, t, phase, live);
}

/// An opened 24 hour half's heading and table, from its saved results.
fn saved_test(ui: &mut egui::Ui, s: &FullWind) {
    heading(
        ui,
        &format!(
            "This Test, {:.1} h After Full Wind",
            s.wind_h.unwrap_or(0.0)
        ),
        "Each position's figures as the test measured them, saved with the take",
    );
    table_grid(
        ui,
        ("positions", Kind::Positions),
        &kept_table(s),
        None,
        None,
    );
}

/// A Dynamic Poise table of a take's `readings` with no test on the
/// Bench: its upright stops read again (#1619).
pub(crate) fn poise_table(ui: &mut egui::Ui, readings: &[Reading]) {
    table_grid(
        ui,
        ("poise", Kind::Poise),
        &table(readings, Kind::Poise),
        None,
        None,
    );
}

/// A test's table, filling in as each position is measured, with a word
/// in the row under way; the banner has the seconds.
pub(crate) fn test_table(
    ui: &mut egui::Ui,
    t: &Test,
    phase: Option<Phase>,
    live: Option<&FreeCell>,
) {
    let status = match phase {
        Some(Phase::Settling(_)) => Some("Settling…".to_string()),
        Some(Phase::Measuring(_)) => Some("Measuring…".to_string()),
        Some(Phase::Paused) => Some("Paused".to_string()),
        Some(Phase::Turn) if t.needs_reading() => Some("Reading…".to_string()),
        _ => None,
    };
    let live = live.filter(|_| matches!(phase, Some(Phase::Measuring(_))));
    let id = match t.kind {
        Kind::Positions => "positions",
        Kind::Poise => "poise",
        Kind::Complication => return complication_grid(ui, t, status, live),
    };
    table_grid(
        ui,
        (id, t.kind),
        &table(t.readings_so_far(), t.kind),
        status,
        live,
    );
}

/// This take's positions, as marked on the card, with what the table
/// leaves out.
fn this_take(ui: &mut egui::Ui, f: &FreeTable, card: &str) {
    heading(
        ui,
        "This Take",
        &format!(
            "Each position marked on the {card} card, read as \
             the command line reads a take"
        ),
    );
    free_grid(ui, f);
}

/// The Info panel's lines (#1528): how the table for a take is read.
fn info_lines(settle_s: f64) -> Vec<String> {
    vec![
        format!(
            "Duration leaves out the first {} after each change of position (Settle) and \
             masked time.",
            table_duration(settle_s)
        ),
        "A position shows readings once it has 10 s.".to_string(),
    ]
}

/// The full-wind results loaded from earlier, with Remove while no test
/// runs: whether Remove was pressed.
fn earlier_section(ui: &mut egui::Ui, full: &FullWind, phase: Option<Phase>) -> bool {
    let pal = theme::pal(ui);
    let mut removed = false;
    ui.horizontal(|ui| {
        heading(
            ui,
            "Full Wind",
            "The results loaded from the test just after winding, to set the next \
                 test against",
        );
        ui.label(RichText::new(earlier_line(full)).color(pal.text_secondary));
        let running = phase.is_some_and(|p| p != Phase::Finished);
        if !running
            && ui
                .add(egui::Button::new("Remove").frame_when_inactive(false))
                .on_hover_text(
                    "Set these results aside: the next test is then a full-wind \
                         one",
                )
                .clicked()
        {
            removed = true;
        }
    });
    table_grid(
        ui,
        ("earlier", Kind::Positions),
        &kept_table(full),
        None,
        None,
    );
    removed
}

/// What the pane says under its tables, in one line: the next thing to
/// do. With nothing yet, where the test starts; with full-wind results
/// loaded, what the next test does with them; after a full-wind test, the
/// test 24 hours after winding. The steps are in its "?" (Hugh, #1528).
/// None over an opened 24 hour half (`saved`), whose tables have rows.
fn about_text(
    test: Option<&Test>,
    earlier: Option<&FullWind>,
    (watch, saved): (bool, bool),
) -> &'static str {
    match (test, earlier) {
        (None, None) if saved => "",
        (None, None) => "New Test leads you through the six positions.",
        (None, Some(_)) => {
            "Start Test runs the six positions again and sets them against these results."
        }
        (Some(t), _) if t.readings.is_some() && t.full.is_none() && watch => {
            "For the same test 24 hours after winding, leave the watch unwound and off \
                 the wrist, then set Wind to 24 Hours After and press New Test."
        }
        (Some(t), _) if t.readings.is_some() && t.full.is_none() => {
            "For the same test 24 hours after winding, first keep these results with Save \
                 Results for 24 Hours\u{2026}. \"?\" has the steps."
        }
        _ => "",
    }
}

/// How long each position settles and is measured, and how to read and
/// keep the results.
fn test_footer(ui: &mut egui::Ui, t: &Test) {
    let pal = theme::pal(ui);
    ui.add(
        egui::Label::new(
            RichText::new(format!(
                "Each position settles {:.0} s and is measured for {:.0} s. Beat \
                     error is a size: which beat is longer depends on the beat the \
                     analysis locked on first. Save the session to keep the positions \
                     with the sound; the command line reads it the same way.",
                t.settle_s, t.measure_s
            ))
            .small()
            .color(pal.text_tertiary),
        )
        .wrap(),
    );
}

/// A row of the table: position, rate, rate less the mean, amplitude and
/// beat error.
pub type Row = (String, Option<f64>, Option<f64>, Option<f64>, Option<f64>);

/// The figures behind the results table: each position's label, rate,
/// rate less the mean, amplitude and beat error (a size), then the means.
pub struct Table {
    pub rows: Vec<Row>,
    pub beat_error_hovers: Vec<String>,
    /// Each position's beat error dimmed: not from the unlock (from the
    /// drop, or its source not recorded), as in the Power Reserve table.
    pub beat_error_dimmed: Vec<bool>,
    pub beat_error_summary: String,
    pub mean_rate: Option<f64>,
    pub mean_amplitude: Option<f64>,
    pub mean_beat_error: Option<f64>,
    /// The largest difference between the rates, s/d (D).
    pub largest_difference: Option<f64>,
    /// The same for amplitude, degrees, and beat error, ms.
    pub largest_amplitude: Option<f64>,
    pub largest_beat_error: Option<f64>,
    /// The vertical positions' mean rate less the horizontal ones', s/d
    /// (DVH).
    pub dvh_rate: Option<f64>,
    /// The same for amplitude, degrees.
    pub dvh_amplitude: Option<f64>,
    /// Conditions found in a position's sound, one line each: no regular
    /// beat, clipping, a short stretch of beats.
    pub notes: Vec<String>,
}

/// The conditions found in one position's measurement, as plain facts.
fn conditions(label: &str, bph_set: Option<u32>, m: &Measurement) -> Vec<String> {
    let mut out = Vec::new();
    if !m.watch_heard {
        out.push(format!(
            "{label}: {}",
            session::no_regular_beat_in(m, bph_set)
        ));
    }
    if m.clipped_beats > 0 {
        let pct = m.clipped_fraction * 100.0;
        out.push(if pct < 0.5 {
            format!("{label}: clipping on under 1% of beats")
        } else {
            format!("{label}: clipping on {pct:.0}% of beats")
        });
    }
    out.extend(m.caveats.iter().filter_map(|c| caveat_condition(label, c)));
    out
}

/// The line for a caveat a position's reading carries, where the table
/// says it.
fn caveat_condition(label: &str, c: &timegrapher_core::measure::Caveat) -> Option<String> {
    use timegrapher_core::measure::Caveat;
    match c {
        Caveat::ShortSpan { span_s } => Some(format!("{label}: {span_s:.0} s of beats")),
        Caveat::BeatErrorAsSize => Some(format!(
            "{label}: beat error given as a size, its sign differs either side of a cut"
        )),
        Caveat::SidesByCount { count } => Some(format!(
            "{label}: Tick and Tock named by the beat count across {count} break{}, \
             not told apart by their sound",
            timegrapher_core::measure::plural(*count)
        )),
        _ => None,
    }
}

/// The table from the readings, with the figures `timegrapher session`
/// prints for the same stretches and its position names.
pub fn table(readings: &[Reading], kind: Kind) -> Table {
    let figures = reading_figures(readings);
    let notes = readings
        .iter()
        .flat_map(|r| conditions(&position_label(r.position), r.bph_set, &r.measurement))
        .collect();
    table_of(&figures, notes, kind)
}

/// The table of an earlier test's saved results.
pub fn kept_table(full: &FullWind) -> Table {
    table_of(&kept_figures(full), Vec::new(), Kind::Positions)
}

/// Each reading's figures, as core reads them.
fn reading_figures(readings: &[Reading]) -> Vec<SourcedFigures> {
    readings
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
        .collect()
}

/// A saved test's figures, as core reads them.
fn kept_figures(full: &FullWind) -> Vec<SourcedFigures> {
    full.positions
        .iter()
        .filter_map(|k| {
            Some((
                session::Position::parse(&k.position)?,
                k.rate_s_per_day,
                k.amplitude_deg,
                k.beat_error_ms,
                k.beat_error_source,
            ))
        })
        .collect()
}

/// The table's summary lines are core `session::sourced_indices`, the values
/// `timegrapher session` gives for the same readings (repeat readings of
/// a position averaged first), over the positions of the test's `kind`:
/// the six, or the Dynamic Poise test's eight upright stops.
fn table_of(figures: &[SourcedFigures], mut notes: Vec<String>, kind: Kind) -> Table {
    let over: Vec<session::Position> = kind.order().iter().map(|p| p.core()).collect();
    let ix = session::sourced_indices_over(0.0, figures, &over);
    let beat_error_summary = ix.beat_error_summary();
    notes.push(beat_error_summary.clone());
    Table {
        beat_error_hovers: figures
            .iter()
            .map(|f| session::beat_error_source_label(f.4).into())
            .collect(),
        beat_error_dimmed: figures
            .iter()
            .map(|f| crate::reserve::beat_error_dimmed(f.4))
            .collect(),
        beat_error_summary,
        rows: figures
            .iter()
            .map(|&(p, rate, amp, be, _)| {
                (
                    position_label(p),
                    rate,
                    rate.zip(ix.x).map(|(x, x0)| x - x0),
                    amp,
                    be,
                )
            })
            .collect(),
        mean_rate: ix.x,
        mean_amplitude: ix.amplitude_mean,
        mean_beat_error: ix.beat_error_mean,
        largest_difference: ix.d_rate,
        largest_amplitude: ix.d_amplitude,
        largest_beat_error: ix.d_beat_error,
        dvh_rate: ix.dvh_rate,
        dvh_amplitude: ix.dvh_amplitude,
        notes,
    }
}

/// One row of a free take's table: what the position's duration allows.
#[derive(Debug, Clone, PartialEq)]
pub enum FreeCell {
    /// No time marked in the position.
    NotMarked,
    /// Marked, but under the 10 s one reading needs.
    Short,
    /// 10 s or more marked, but no stretch of it clear enough to read
    /// (cut by masks, breaks or a beat lost).
    Unread,
    /// Its rate, amplitude and beat error (a size).
    Read(Option<f64>, Option<f64>, Option<f64>),
}

/// A free take's table: each position in the card's order with its
/// duration and readings, then the mean and the largest difference of
/// rate, amplitude and beat error over the positions read. The figures
/// are core `session::positions_table`'s.
#[derive(Debug, Clone, PartialEq)]
pub struct FreeTable {
    pub rows: Vec<(String, f64, FreeCell)>,
    /// Rate (s/d), amplitude (degrees) and beat error (ms).
    pub mean: [Option<f64>; 3],
    pub largest: [Option<f64>; 3],
    /// How many positions have readings.
    pub read: usize,
    /// The take's upright stops read the same way, for the Dynamic Poise
    /// pane with no test on the Bench (#1619).
    pub upright: Upright,
}

/// A take's upright stops read again, as readings: each stop marked long
/// enough to read, in [`session::Position::UPRIGHT`]'s order (core
/// `session::position_rows`).
#[derive(Debug, Clone, Default)]
pub struct Upright(pub Vec<Reading>);

/// The same when every reading has the same position, duration and
/// figures.
impl PartialEq for Upright {
    fn eq(&self, other: &Self) -> bool {
        let durations = |u: &Upright| u.0.iter().map(|r| r.duration_s).collect::<Vec<_>>();
        reading_figures(&self.0) == reading_figures(&other.0) && durations(self) == durations(other)
    }
}

/// The readings of core's `rows` that were read, for a take at `bph`,
/// `bph_set` the beat rate supplied (as a test's readings carry it).
pub fn upright(rows: &[session::PositionRow], bph: u32, bph_set: Option<u32>) -> Upright {
    Upright(
        rows.iter()
            .filter_map(|r| {
                Some(Reading {
                    label: position_label(r.position),
                    position: r.position,
                    wind_h: None,
                    date: None,
                    notes: None,
                    duration_s: r.duration_s,
                    bph,
                    bph_set,
                    measurement: r.measurement.clone()?,
                    cycles: Vec::new(),
                    shape: None,
                    reference: None,
                })
            })
            .collect(),
    )
}

/// The free take's table from core's, in the Current Position card's
/// order, with the take's `upright` stops.
pub fn free_table(t: &session::PositionsTable, upright: Upright) -> FreeTable {
    let rows: Vec<(String, f64, FreeCell)> = Position::ALL
        .iter()
        .filter_map(|p| {
            let sp = session::Position::parse(p.code())?;
            let r = t.rows.iter().find(|r| r.position == sp)?;
            let cell = match &r.measurement {
                Some(m) => match (m.rate_s_per_day, m.amplitude_deg, m.beat_error()) {
                    (None, None, None) => FreeCell::Unread,
                    (rate, amp, be) => FreeCell::Read(rate, amp, be),
                },
                None if r.duration_s > 0.0 => FreeCell::Short,
                None => FreeCell::NotMarked,
            };
            Some((position_label(sp), r.duration_s, cell))
        })
        .collect();
    let read = rows
        .iter()
        .filter(|r| matches!(r.2, FreeCell::Read(..)))
        .count();
    FreeTable {
        rows,
        mean: [
            t.indices.x,
            t.indices.amplitude_mean,
            t.indices.beat_error_mean,
        ],
        largest: [
            t.indices.d_rate,
            t.indices.d_amplitude,
            t.indices.d_beat_error,
        ],
        read,
        upright,
    }
}

/// A duration as the table writes it: "35 s", "4 min 10 s", "1 h 5 min".
fn table_duration(v: f64) -> String {
    let s = v.round().max(0.0) as u64;
    match (s / 3600, s % 3600 / 60, s % 60) {
        (0, 0, s) => format!("{s} s"),
        (0, m, 0) => format!("{m} min"),
        (0, m, s) => format!("{m} min {s} s"),
        (h, 0, _) => format!("{h} h"),
        (h, m, _) => format!("{h} h {m} min"),
    }
}

/// The free take's table: Position, Duration, Rate, Amplitude and Beat
/// Error, each reading in its own colour, then the mean and the largest
/// difference once two positions have readings.
fn free_grid(ui: &mut egui::Ui, t: &FreeTable) {
    let pal = theme::pal(ui);
    egui::Grid::new("free-positions")
        .num_columns(5)
        .spacing(Vec2::new(16.0, 5.0))
        .min_col_width(LABEL_W.min(FIGURE_W))
        .striped(true)
        .show(ui, |ui| {
            position_head(ui);
            for (h, unit, hint) in [
                (
                    "Duration",
                    "",
                    "Time marked in the position, less its settling and masked time",
                ),
                ("Rate", "s/d", "Seconds a day gained (+) or lost (−)"),
                (
                    "Amplitude",
                    "°",
                    "The balance's swing, from the lift angle set",
                ),
                ("Beat Error", "ms", "From the unlock, as a size"),
            ] {
                // Beat error's figures are in the text colour in light
                // mode: its swatch beside the heading says whose they are.
                let be = pal.trace_beat_error;
                let swatch = (h == "Beat Error" && pal.ink(be) != be).then_some(be);
                column_head_swatch(ui, FIGURE_W, h, unit, hint, swatch);
            }
            ui.end_row();
            for (label, duration, cell) in &t.rows {
                let marked = *cell != FreeCell::NotMarked;
                let color = if marked { pal.text } else { pal.text_tertiary };
                ui.label(RichText::new(label).color(color));
                match cell {
                    FreeCell::NotMarked => {
                        right(ui, "Not marked".into(), false, pal.text_tertiary);
                        for _ in 0..3 {
                            ui.label("");
                        }
                    }
                    FreeCell::Short => {
                        right(ui, table_duration(*duration), false, pal.text);
                        right(ui, "Under 10 s".into(), false, pal.text_secondary);
                        for _ in 0..2 {
                            ui.label("");
                        }
                    }
                    FreeCell::Unread => {
                        right(ui, table_duration(*duration), false, pal.text);
                        right(ui, "No reading".into(), false, pal.text_secondary);
                        for _ in 0..2 {
                            ui.label("");
                        }
                    }
                    FreeCell::Read(rate, amp, be) => {
                        right(ui, table_duration(*duration), false, pal.text);
                        right(ui, signed(*rate, RATE), false, pal.trace_rate);
                        right(ui, plain(*amp, AMPLITUDE), false, pal.trace_amplitude);
                        right(
                            ui,
                            plain(*be, BEAT_ERROR),
                            false,
                            pal.ink(pal.trace_beat_error),
                        );
                    }
                }
                ui.end_row();
            }
            if t.read >= 2 {
                ui.label(RichText::new("Average").strong().color(pal.text))
                    .on_hover_text("Over the positions read");
                right(ui, format!("{} of 6", t.read), false, pal.text_secondary);
                right(ui, signed(t.mean[0], RATE), true, pal.trace_rate);
                right(ui, plain(t.mean[1], AMPLITUDE), true, pal.trace_amplitude);
                right(
                    ui,
                    plain(t.mean[2], BEAT_ERROR),
                    true,
                    pal.ink(pal.trace_beat_error),
                );
                ui.end_row();
                largest_label(ui);
                ui.label("");
                right(ui, plain(t.largest[0], RATE), true, pal.trace_rate);
                right(
                    ui,
                    plain(t.largest[1], AMPLITUDE),
                    true,
                    pal.trace_amplitude,
                );
                right(
                    ui,
                    plain(t.largest[2], BEAT_ERROR),
                    true,
                    pal.ink(pal.trace_beat_error),
                );
                ui.end_row();
            }
        });
}

/// The label of a table's Largest Difference row, with its hover.
fn largest_label(ui: &mut egui::Ui) {
    let pal = theme::pal(ui);
    ui.label(RichText::new("Largest Difference").strong().color(pal.text))
        .on_hover_text(
            "Between the positions read: the highest rate, amplitude or beat error less the lowest",
        );
}

/// The full-wind test against the 24 hour half, with the figures
/// `timegrapher session` gives for the two states of wind.
pub struct Comparison {
    /// Hours between the two tests' starts.
    pub hours: f64,
    /// Each position measured in both: its label, then rate at full wind,
    /// rate after `hours` and the change (s/d), then the same for
    /// amplitude (degrees).
    pub rows: Vec<(String, [Option<f64>; 6])>,
    /// The largest change in rate over the positions, crown right (12H)
    /// left out (Im), s/d.
    pub im: Option<f64>,
    /// The size of the change in the mean rate over the positions
    /// measured in both (Ie), s/d.
    pub ie: Option<f64>,
}

/// Full wind (`full`) against the 24 hour half's `readings`, `hours`
/// later, read by core `session::evaluate_states` as `session` reads a
/// file with both. The kept figures stand in for the full-wind
/// measurements.
pub fn comparison(full: &FullWind, hours: f64, readings: &[Reading]) -> Option<Comparison> {
    comparison_of(&kept_figures(full), hours, &reading_figures(readings))
}

/// A saved 24 hour half (`later`) against the full-wind results it was set
/// against, both from the file: what the run showed, read again from
/// nothing but its saved rows. None for a full-wind test's results.
pub fn saved_comparison(later: &FullWind) -> Option<Comparison> {
    let full = later.against.as_deref()?;
    comparison_of(&kept_figures(full), later.wind_h?, &kept_figures(later))
}

fn comparison_of(
    full: &[SourcedFigures],
    hours: f64,
    later: &[SourcedFigures],
) -> Option<Comparison> {
    if full.is_empty() || later.is_empty() {
        return None;
    }
    let states = vec![
        session::sourced_indices(0.0, full),
        session::sourced_indices(hours, later),
    ];
    let report = session::evaluate_states(states, Vec::new());
    let (first, last) = (report.states.first()?, report.states.last()?);
    let rows = later
        .iter()
        .filter_map(|f| {
            let p = f.0;
            let a = first.positions.iter().find(|v| v.position == p)?;
            let b = last.positions.iter().find(|v| v.position == p)?;
            let change = |x: Option<f64>, y: Option<f64>| y.zip(x).map(|(y, x)| y - x);
            Some((
                position_label(p),
                [
                    a.rate_s_per_day,
                    b.rate_s_per_day,
                    change(a.rate_s_per_day, b.rate_s_per_day),
                    a.amplitude_deg,
                    b.amplitude_deg,
                    change(a.amplitude_deg, b.amplitude_deg),
                ],
            ))
        })
        .collect();
    Some(Comparison {
        hours,
        rows,
        im: report.im,
        ie: report.ie,
    })
}

/// A position as the app names it: "Crown Up (CU, 3H)", the hobby
/// code with the Swiss laboratory name beside it, or "Dial Up (DU)".
pub fn position_label(p: session::Position) -> String {
    p.display_label()
}

/// The table as text a spreadsheet takes: a header, a row for each
/// position, the means, then the largest difference, the vertical less
/// horizontal figures and any conditions, one a line. Fields are split by
/// `sep` (a comma for a CSV file, a tab for the clipboard) and quoted when
/// they hold it; numbers use a plain minus sign.
/// The Dynamic Poise table's means are Mean, as on its pane, with no
/// largest difference and no vertical less horizontal.
pub fn table_text(t: &Table, sep: char, kind: Kind) -> String {
    let poise = kind == Kind::Poise;
    let (mean_word, from_word) = if poise {
        ("Mean", "From Mean (s/d)")
    } else {
        ("Average", "From Average (s/d)")
    };
    let field = |s: &str| {
        if s.contains(sep) || s.contains('"') {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    let num = |v: Option<f64>, d: usize| v.map_or_else(String::new, |x| format!("{x:.d$}"));
    let line = |cells: &[String]| {
        let mut l = cells
            .iter()
            .map(|c| field(c))
            .collect::<Vec<_>>()
            .join(&sep.to_string());
        l.push('\n');
        l
    };
    let mut out = line(&[
        "Position".into(),
        "Rate (s/d)".into(),
        from_word.into(),
        "Amplitude (°)".into(),
        "Beat Error (ms)".into(),
    ]);
    for (label, rate, from_mean, amp, be) in &t.rows {
        out += &line(&[
            label.clone(),
            num(*rate, 1),
            num(*from_mean, 1),
            num(*amp, 0),
            num(*be, 2),
        ]);
    }
    out += &line(&[
        mean_word.into(),
        num(t.mean_rate, 1),
        String::new(),
        num(t.mean_amplitude, 0),
        num(t.mean_beat_error, 2),
    ]);
    if poise {
        return out;
    }
    out += &line(&[
        "Largest Difference".into(),
        num(t.largest_difference, 1),
        String::new(),
        num(t.largest_amplitude, 0),
        num(t.largest_beat_error, 2),
    ]);
    out.push('\n');
    out += &line(&[
        "Vertical less horizontal rate (s/d)".into(),
        num(t.dvh_rate, 1),
    ]);
    out += &line(&[
        "Vertical less horizontal amplitude (°)".into(),
        num(t.dvh_amplitude, 0),
    ]);
    for n in &t.notes {
        out += &line(std::slice::from_ref(n));
    }
    out
}

/// The comparison as [`table_text`] writes the table: a header, a row
/// for each position, then Im and Ie.
pub fn comparison_text(c: &Comparison, sep: char) -> String {
    let h = format!("{:.1} h", c.hours);
    let mut cells = vec![vec![
        "Position".to_string(),
        "Rate at Full Wind, s/d".into(),
        format!("Rate at {h}, s/d"),
        "Rate Change, s/d".into(),
        "Amplitude at Full Wind, °".into(),
        format!("Amplitude at {h}, °"),
        "Amplitude Change, °".into(),
    ]];
    for (label, v) in &c.rows {
        let mut row = vec![label.clone()];
        for (i, x) in v.iter().enumerate() {
            let d = if i < 3 { RATE } else { AMPLITUDE };
            row.push(x.map_or_else(String::new, |x| format!("{x:.d$}")));
        }
        cells.push(row);
    }
    cells.push(Vec::new());
    let num = |v: Option<f64>| v.map_or_else(String::new, |x| format!("{x:.RATE$}"));
    cells.push(vec![
        "Largest change in rate, crown right left out (Im), s/d".into(),
        num(c.im),
    ]);
    cells.push(vec![
        "Change in the mean rate, a size (Ie), s/d".into(),
        num(c.ie),
    ]);
    let field = |s: &str| {
        if s.contains(sep) || s.contains('"') {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    cells
        .iter()
        .map(|r| {
            let mut l = r
                .iter()
                .map(|c| field(c))
                .collect::<Vec<_>>()
                .join(&sep.to_string());
            l.push('\n');
            l
        })
        .collect()
}

/// The test as text a spreadsheet takes: the watch and how it was read,
/// the table, and in the 24 hour half both tests side by side, fields
/// split by `sep` as [`table_text`] splits them.
pub fn export_text(test: &Test, about: &About, sep: char) -> String {
    let field = |s: &str| {
        if s.contains(sep) || s.contains('"') {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    let mut out = String::new();
    let mut line = |k: &str, v: String| {
        out += &format!("{}{sep}{}\n", field(k), field(&v));
    };
    line("Watch", about.watch.trim().to_string());
    line("Calibre", about.calibre.clone());
    line(
        "Beat Rate, bph",
        about.bph.map_or_else(String::new, |b| b.to_string()),
    );
    line("Lift Angle, °", format!("{:.0}", about.lift_angle_deg));
    line("Test Began, UTC", test.began_utc());
    if let Some((full, h)) = &test.full {
        line("Full-Wind Test Began, UTC", full.began_utc.clone());
        line("Hours After Full Wind", format!("{h:.1}"));
    }
    line("Settling, s", format!("{:.0}", test.settle_s));
    line("Measured, s", format!("{:.0}", test.measure_s));
    if test.kind == Kind::Complication {
        line("Position", test.position().name().to_string());
        out.push('\n');
        out += &complication_text(test, sep);
        return out;
    }
    out.push('\n');
    let readings = test.readings_so_far();
    out += &table_text(&table(readings, test.kind), sep, test.kind);
    if let Some(f) = poise_fit(readings).filter(|_| test.kind == Kind::Poise) {
        out.push('\n');
        out += &format!("{}\n", field(&f.text));
    }
    if let Some(c) = test
        .full
        .as_ref()
        .and_then(|(f, h)| comparison(f, *h, readings))
    {
        out.push('\n');
        out += &comparison_text(&c, sep);
    }
    out
}

/// The Dynamic Poise table's Mean rate, and its name on the picture's
/// mean line and ring, "Mean +4.0", as the table's Mean row shows it.
pub fn poise_mean(readings: &[Reading]) -> Option<(f64, String)> {
    let m = table(readings, Kind::Poise).mean_rate?;
    Some((m, format!("Mean {}", signed(Some(m), RATE))))
}

/// The poise fit of a test's upright readings ([`poise::shown_fit`]).
pub fn poise_fit(readings: &[Reading]) -> Option<poise::PoiseFit> {
    poise::shown_fit(readings.iter().map(|r| (r.position, &r.measurement)))
}

/// The dialog for saving the full-wind results for the 24 hour half,
/// starting in `dir`.
pub fn results_dialog(k: &FullWind, dir: &std::path::Path) -> rfd::FileDialog {
    let stamp: String = k
        .began_utc
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'T')
        .take(15)
        .collect();
    let mut d = rfd::FileDialog::new()
        .set_title("Save the Results for the Test 24 Hours Later")
        .set_file_name(format!("positions-test-{stamp}Z.json"))
        .add_filter("Positions Test Results", &["json"]);
    if dir.is_dir() {
        d = d.set_directory(dir);
    }
    d
}

/// The dialog for saving the test as a CSV file, starting in `dir`.
pub fn csv_dialog(dir: &std::path::Path) -> rfd::FileDialog {
    let stamp: String = timegrapher_core::recorder::utc(std::time::SystemTime::now())
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'T')
        .take(15)
        .collect();
    let mut d = rfd::FileDialog::new()
        .set_title("Save the Positions Test as CSV")
        .set_file_name(format!("positions-test-{stamp}Z.csv"))
        .add_filter("CSV", &["csv"]);
    if dir.is_dir() {
        d = d.set_directory(dir);
    }
    d
}

#[cfg(test)]
#[path = "guided_tests.rs"]
mod tests;
