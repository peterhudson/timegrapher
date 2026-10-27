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
use timegrapher_core::session::{self, Figures, Measurement, Reading};
use timegrapher_core::stream::BeatLog;

/// The file a finished test just after winding is saved to, from the
/// pane or beside a saved take, for the same six positions 24
/// hours later.
pub const RESULTS_FILE: &str = "positions-test.json";
/// The file's `format`, versioned: a later version adds keys.
pub const RESULTS_FORMAT: &str = "timegrapher.positions-test/1";

/// A finished test just after winding, as saved for the 24 hour half.
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

/// The newest full-wind test kept in the takes folder `takes` (each
/// take's [`RESULTS_FILE`]) that began 12 to 36 hours before `now_unix`:
/// the half a watch's 24 hours after test is set against.
pub fn full_wind_in(takes: &std::path::Path, now_unix: f64) -> Option<FullWind> {
    std::fs::read_dir(takes)
        .ok()?
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path().join(RESULTS_FILE)).ok())
        .filter_map(|t| FullWind::from_json(&t).ok())
        .filter(|f| (12.0..=36.0).contains(&f.hours_ago(now_unix)))
        .max_by(|a, b| a.began_unix.total_cmp(&b.began_unix))
}

/// Seconds the watch is left to settle after it is turned, and then
/// measured, unless set otherwise in the pane: the project's choice. A
/// minute brings the rate's ± near half a second a day.
pub const SETTLE_S: f64 = 20.0;
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

    /// The finished full-wind test, to keep for the 24 hour half: None
    /// for a 24 hour half or an unfinished test.
    pub fn kept(&self, about: &About) -> Option<FullWind> {
        if self.full.is_some() {
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
                })
                .collect(),
        })
    }

    /// When the test began: `2026-10-08T21:04:05.123Z`.
    pub fn began_utc(&self) -> String {
        timegrapher_core::recorder::utc(
            std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(self.began_unix),
        )
    }

    pub fn position(&self) -> Position {
        ORDER[self.step.min(ORDER.len() - 1)]
    }

    /// The measured stretch of each position done so far: position,
    /// start and end in seconds of the session.
    pub fn stretches(&self) -> &[(Position, f64, f64)] {
        &self.spans
    }

    pub fn next_position(&self) -> Option<Position> {
        ORDER.get(self.step + 1).copied()
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
        let (settle, measure) = (self.settle_s, self.measure_s);
        let start = *self.start.get_or_insert(now);
        self.began.get_or_insert(start);
        let t = now - start;
        if t < settle {
            return Phase::Settling(settle - t);
        }
        if t < settle + measure {
            return Phase::Measuring(t - settle);
        }
        if self.spans.len() <= self.step {
            let from = start + settle;
            self.spans.push((self.position(), from, from + measure));
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
                self.unused.push((p, now, NOT_USED));
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
            let (p, from, to) = self.spans[self.done.len()];
            match self.reading(p, from, to, log, clock, masked) {
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
        ORDER.get(self.step).copied()
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
            out.push((p, now, NOT_USED));
        }
        out
    }

    /// One attempt at a position, started at `start`, seen at `now`.
    fn step_unused(&self, start: f64, now: f64, given_up: bool) -> Vec<(f64, f64, &'static str)> {
        let mut out = Vec::new();
        if now <= start {
            return out;
        }
        let settled = start + self.settle_s;
        out.push((start, now.min(settled), SETTLING));
        let end = settled + self.measure_s;
        if given_up && now > settled {
            out.push((settled, now, NOT_USED));
        } else if now > end {
            out.push((end, now, NOT_USED));
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
            self.unused.push((p, now, NOT_USED));
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
            let from = s + self.settle_s;
            if now > from {
                out.push((from, now.min(from + self.measure_s)));
            }
        }
        out
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
            .filter_map(|&(p, from, to)| self.reading(p, from, to, log, clock, masked))
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

    /// One position's stretch of the session, `from` to `to` seconds,
    /// leaving out the stretches `masked` as `session` does.
    fn reading(
        &self,
        p: Position,
        from: f64,
        to: f64,
        log: &BeatLog,
        clock: Option<&ClockFit>,
        masked: &[(f64, f64)],
    ) -> Option<Reading> {
        let position = session::Position::parse(p.code())?;
        // Cut at every jump in the beats, as the whole take is, so no fit
        // runs across one.
        let jumps = timegrapher_core::breaks::with_jumps(Vec::new(), &log.jumps);
        let measurement: Measurement =
            session::measure_masked(log, clock, &jumps, masked, from, to);
        Some(Reading {
            label: p.name().into(),
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
    let (name, n, of) = (test.position().name(), test.step + 1, ORDER.len());
    match test.wind_h() {
        Some(h) => format!("{name} ({n} of {of}, {h:.1} h After Full Wind)"),
        None => format!("{name} ({n} of {of})"),
    }
}

/// The step panel at the top of the pane while the test runs, one height
/// throughout: the position, a chip for each of the six (done, now, to
/// come), a progress bar for its settling and measuring, one line of what
/// happens next, and Pause and Restart. Stop Test… is on the Listen card.
fn banner(ui: &mut egui::Ui, test: &Test, phase: Phase) -> Option<Action> {
    let pal = theme::pal(ui);
    let mut action = None;
    let (settle, measure) = (test.settle_s, test.measure_s);
    let frame = theme::card()
        .fill(pal.accent.gamma_multiply(0.14))
        .inner_margin(egui::Margin::symmetric(14, 10));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        let (title, detail, frac) = match phase {
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
                     then the strip marks the time Not Used for Positions Test.",
                    test.position().name()
                ),
                0.0,
            ),
            Phase::Turn => match test.next_position() {
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
                    "All six positions measured".to_string(),
                    "Reading them together and keeping the take…".to_string(),
                    1.0,
                ),
            },
            Phase::Finished => ("Positions Test done".to_string(), String::new(), 1.0),
        };
        // Each button has its own slot, as wide as the widest it holds, so
        // none moves as the phase changes.
        let pad = 2.0 * ui.spacing().button_padding.x;
        let widest = |ui: &egui::Ui, words: &mut dyn Iterator<Item = String>| {
            let font = theme::semibold(13.0);
            words
                .map(|w| ui.fonts_mut(|f| f.layout_no_wrap(w, font.clone(), pal.text).size().x))
                .fold(84.0_f32, |a, w| a.max(w + pad))
        };
        let first_w = widest(
            ui,
            &mut ORDER
                .iter()
                .map(|p| format!("Start {} Measurement", p.code()))
                .chain(["Resume", "Pause"].map(String::from)),
        );
        let restart_w = widest(
            ui,
            &mut ORDER
                .iter()
                .map(|p| format!("Restart {} Measurement", p.code())),
        );
        // Each button at its own width at the left of its slot, all one
        // height on one line, so none moves from phase to phase.
        let slot = |ui: &mut egui::Ui, w: f32, b: egui::Button<'_>| {
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
        };
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
            let code = test.position().code();
            let name = test.position().name();
            match phase {
                // The sixth measured: the test ends by itself.
                Phase::Turn if test.next_position().is_none() => {
                    ui.allocate_space(Vec2::new(0.0, 26.0));
                }
                Phase::Turn => {
                    let (next, next_hint) = match test.next_position() {
                        Some(p) => (
                            format!("Start {} Measurement", p.code()),
                            format!(
                                "Press once the watch is {}: it settles for {settle:.0} s, \
                                 then {} is measured for {measure:.0} s",
                                p.name().to_lowercase(),
                                p.name()
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
                             watch back {} and settle and measure it again",
                        name.to_lowercase()
                    ))
                    .clicked()
                    {
                        action = Some(Action::Redo);
                    }
                }
                Phase::Paused => {
                    if slot(ui, first_w, theme::primary(ui, "Resume"))
                        .on_hover_text(format!(
                            "Settle and measure {name} again from now, with the watch {}",
                            name.to_lowercase()
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
        });
        // One line, whole in its hover: the panel never grows.
        ui.horizontal(|ui| {
            let chips_w = ORDER.len() as f32 * 30.0 + 8.0;
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
                        egui::Label::new(RichText::new(&detail).small().color(pal.text_secondary))
                            .truncate(),
                    )
                    .on_hover_text(&detail);
                },
            );
            step_chips(ui, test);
        });
        ui.add_space(4.0);
        ui.add(
            egui::ProgressBar::new(frac.clamp(0.0, 1.0))
                .desired_height(6.0)
                .fill(pal.accent),
        );
    });
    action
}

/// The six positions as chips after what the step is doing: done, the one
/// under way, and those to come, each its code. Left out where the panel
/// has no room for them.
fn step_chips(ui: &mut egui::Ui, test: &Test) {
    let pal = theme::pal(ui);
    let font = egui::FontId::proportional(11.0);
    let (w, h, gap) = (26.0, 16.0, 4.0);
    let all = ORDER.len() as f32 * (w + gap);
    if ui.available_width() < all + 8.0 {
        return;
    }
    ui.add_space(ui.available_width() - all);
    let done = test.stretches().len();
    for (i, p) in ORDER.iter().enumerate() {
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
            p.code(),
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
        resp.on_hover_text(format!("{}: {state}", p.name()));
    }
}

/// The figures as the pane writes them: a signed figure with a true minus
/// sign, a plain one, or a dash when missing. A figure that rounds to zero
/// takes no sign.
fn signed(v: Option<f64>, d: usize) -> String {
    v.map_or_else(
        || "—".to_string(),
        |x| {
            let s = format!("{:.d$}", x.abs());
            if s.bytes().all(|b| b == b'0' || b == b'.') {
                s
            } else if x < 0.0 {
                format!("−{s}")
            } else {
                format!("+{s}")
            }
        },
    )
}

fn plain(v: Option<f64>, d: usize) -> String {
    v.map_or_else(|| "—".to_string(), |x| format!("{x:.d$}"))
}

/// A table's title, in the pane's own type.
fn heading(ui: &mut egui::Ui, text: &str, hint: &str) {
    let pal = theme::pal(ui);
    ui.label(RichText::new(text).strong().color(pal.text))
        .on_hover_text(hint);
}

/// A figure at the right of a cell of its own width, so the tables'
/// columns line up and their stripes end with the last column.
fn right(ui: &mut egui::Ui, text: String, strong: bool, color: egui::Color32) {
    right_in(ui, FIGURE_W, text, strong, color);
}

/// [`right`] in a cell `w` wide.
fn right_in(ui: &mut egui::Ui, w: f32, text: String, strong: bool, color: egui::Color32) {
    let size = Vec2::new(w, ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.set_min_size(size);
            let t = RichText::new(text).color(color);
            ui.label(if strong { t.strong() } else { t });
        },
    );
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

/// A figure column's heading, at the right like its figures.
fn column_head(ui: &mut egui::Ui, w: f32, text: &str, hint: &str) {
    column_head_swatch(ui, w, text, hint, None);
}

/// [`column_head`], with a swatch of `swatch` before the words: for a
/// column whose figures are in the text colour because the reading's own
/// is too faint for small text (beat error's amber in light mode).
fn column_head_swatch(
    ui: &mut egui::Ui,
    w: f32,
    text: &str,
    hint: &str,
    swatch: Option<egui::Color32>,
) {
    let pal = theme::pal(ui);
    // Never narrower than its words, so a long heading can't run into the
    // one before it.
    let w = w.max(head_w(ui, text) + if swatch.is_some() { 12.0 } else { 0.0 });
    let size = Vec2::new(w, ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.set_min_size(size);
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(theme::caption(text).color(pal.text_secondary))
                .on_hover_text(hint);
            if let Some(c) = swatch {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, c);
            }
        },
    );
}

/// How wide a column's heading is in its own type.
fn head_w(ui: &egui::Ui, text: &str) -> f32 {
    let font = theme::semibold(12.5);
    let color = theme::pal(ui).text;
    ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, color).size().x)
}

/// The width of a figure's column, and of the position names'.
const FIGURE_W: f32 = 88.0;
/// The comparison's figure columns, whose headings are longer.
const WIDE_W: f32 = 100.0;
const LABEL_W: f32 = 150.0;

/// One test's table: a row for each of the six positions, in order. The
/// first `t.rows.len()` hold figures; `status` names what the next row is
/// doing (settling, measuring) while the test runs. Then the means, the
/// largest difference, vertical less horizontal and any conditions.
fn table_grid(
    ui: &mut egui::Ui,
    id: &str,
    t: &Table,
    status: Option<String>,
    live: Option<&FreeCell>,
) {
    let pal = theme::pal(ui);
    const FROM: &str = "From Average, s/d";
    let from_w = FIGURE_W.max(head_w(ui, FROM));
    egui::Grid::new(id)
        .num_columns(5)
        .spacing(Vec2::new(16.0, 5.0))
        .min_col_width(LABEL_W.min(FIGURE_W))
        .striped(true)
        .show(ui, |ui| {
            position_head(ui);
            for (h, hint) in [
                ("Rate, s/d", "Seconds a day gained (+) or lost (−)"),
                (
                    FROM,
                    "This position's rate less the mean of the positions measured",
                ),
                (
                    "Amplitude, °",
                    "The balance's swing, from the lift angle set",
                ),
                ("Beat Error, ms", "From the unlock, as a size"),
            ] {
                column_head(ui, FIGURE_W, h, hint);
            }
            ui.end_row();
            let mut status = status;
            for (i, p) in ORDER.iter().enumerate() {
                match t.rows.get(i) {
                    Some((label, rate, from_mean, amp, be)) => {
                        ui.label(RichText::new(label).color(pal.text));
                        right(ui, signed(*rate, 1), false, pal.text);
                        right_in(ui, from_w, signed(*from_mean, 1), false, pal.text);
                        right(ui, plain(*amp, 0), false, pal.text);
                        right(ui, plain(*be, 2), false, pal.text);
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
                                right(ui, signed(*rate, 1), false, pal.accent);
                                ui.label("");
                                right(ui, plain(*amp, 0), false, pal.accent);
                                right(ui, plain(*be, 2), false, pal.accent);
                            }
                            (Some(s), _) => {
                                ui.label(RichText::new(label).strong().color(pal.text));
                                right(ui, s, false, pal.accent);
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
                ui.label(RichText::new("Average").strong().color(pal.text));
                right(ui, signed(t.mean_rate, 1), true, pal.text);
                ui.label("");
                right(ui, plain(t.mean_amplitude, 0), true, pal.text);
                right(ui, plain(t.mean_beat_error, 2), true, pal.text);
                ui.end_row();
            }
            if t.rows.len() >= 2 {
                largest_label(ui);
                right(ui, plain(t.largest_difference, 1), true, pal.text);
                ui.label("");
                right(ui, plain(t.largest_amplitude, 0), true, pal.text);
                right(ui, plain(t.largest_beat_error, 2), true, pal.text);
                ui.end_row();
            }
        });
    if t.dvh_rate.is_some() || t.dvh_amplitude.is_some() {
        ui.label(
            RichText::new(format!(
                "Vertical less horizontal: {} s/d in rate, {}° in amplitude.",
                signed(t.dvh_rate, 1),
                signed(t.dvh_amplitude, 0)
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
            for (head, hint) in [
                (
                    "Rate, Full".to_string(),
                    "Rate at full wind, s/d".to_string(),
                ),
                (
                    format!("Rate, {h}"),
                    format!("Rate {h} after full wind, s/d"),
                ),
                (
                    "Change".to_string(),
                    "The later rate less the full-wind one, s/d".to_string(),
                ),
                (
                    "Amplitude, Full".to_string(),
                    "Amplitude at full wind, degrees".to_string(),
                ),
                (
                    format!("Amplitude, {h}"),
                    format!("Amplitude {h} after full wind, degrees"),
                ),
                (
                    "Change".to_string(),
                    "The later amplitude less the full-wind one, degrees".to_string(),
                ),
            ] {
                column_head(ui, WIDE_W, &head, &hint);
            }
            ui.end_row();
            for (label, v) in &c.rows {
                ui.label(RichText::new(label).color(pal.text));
                right_in(ui, WIDE_W, signed(v[0], 1), false, pal.text);
                right_in(ui, WIDE_W, signed(v[1], 1), false, pal.text);
                right_in(ui, WIDE_W, signed(v[2], 1), true, pal.text);
                right_in(ui, WIDE_W, plain(v[3], 0), false, pal.text);
                right_in(ui, WIDE_W, plain(v[4], 0), false, pal.text);
                right_in(ui, WIDE_W, signed(v[5], 0), true, pal.text);
                ui.end_row();
            }
        });
    ui.add_space(4.0);
    ui.label(
        RichText::new(format!(
            "Largest change in rate, crown right left out: {} s/d. Change in the mean rate, as \
             a size: {} s/d.",
            signed(c.im, 1),
            plain(c.ie, 1)
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
    free: Option<&FreeTable>,
    live: Option<&FreeCell>,
    settle_s: f64,
    measure_s: f64,
    watch: bool,
    note: Option<&str>,
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
    area.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        if let Some(n) = note {
            ui.label(RichText::new(n).color(pal.text_secondary));
        }
        if let (Some(t), Some(ph)) = (test, phase) {
            if ph != Phase::Finished {
                action = banner(ui, t, ph);
                ui.add_space(6.0);
            }
        }
        if let Some(t) = test {
            let title = match t.wind_h() {
                Some(h) => format!("This Test, {h:.1} h After Full Wind"),
                None => "This Test".to_string(),
            };
            heading(
                ui,
                &title,
                "Each position's figures as soon as it is measured, read the same way as \
                     `timegrapher session` reads them",
            );
            // A word in the row under way; the banner has the seconds.
            let status = match phase {
                Some(Phase::Settling(_)) => Some("Settling…".to_string()),
                Some(Phase::Measuring(_)) => Some("Measuring…".to_string()),
                Some(Phase::Paused) => Some("Paused".to_string()),
                Some(Phase::Turn) if t.needs_reading() => Some("Reading…".to_string()),
                _ => None,
            };
            let live = live.filter(|_| matches!(phase, Some(Phase::Measuring(_))));
            table_grid(ui, "positions", &table(t.readings_so_far()), status, live);
        }
        if let (None, Some(f)) = (test, free) {
            heading(
                ui,
                "This Take",
                "Each position marked on the Current Position card, read as \
                     `timegrapher session` reads a take",
            );
            free_grid(ui, f);
            ui.add_space(4.0);
            for line in [
                format!(
                    "Duration leaves out the first {} after each change of position \
                         (Settle) and masked time.",
                    table_duration(settle_s)
                ),
                "A position shows readings once it has 10 s.".to_string(),
                "Each Position applies only to the guided test.".to_string(),
            ] {
                ui.label(
                    RichText::new(format!("• {line}"))
                        .small()
                        .color(pal.text_tertiary),
                );
            }
        }
        let c = test.and_then(|t| {
            let (f, h) = t.full.as_ref()?;
            comparison(f, *h, t.readings_so_far())
        });
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
                    action = Some(Action::ClearEarlier);
                }
            });
            table_grid(ui, "earlier", &kept_table(full), None, None);
        }
        ui.add_space(10.0);
        let about = match (test, earlier) {
            (None, None) => format!(
                "Start Test, on the Listen card, leads you through the six positions:\n\
                     1. It says which position to turn the watch to.\n\
                     2. It lets the watch settle {settle_s:.0} s.\n\
                     3. It measures it for {measure_s:.0} s and fills in its row.\n\
                     After the sixth it ends by itself and keeps the take. {}",
                if watch {
                    "For the test 24 hours after winding, leave the watch unwound and off the \
                     wrist, then set Wind to 24 Hours After."
                } else {
                    "For the test 24 hours after winding, save the results, leave the watch \
                     unwound and off the wrist, then load them with Load Full-Wind Results…."
                }
            ),
            (None, Some(_)) => "Start Test runs the six positions again and sets them \
                     against these results."
                .to_string(),
            (Some(t), _) if t.readings.is_some() && t.full.is_none() && watch => {
                "For the same test 24 hours after winding, leave the watch unwound and off \
                     the wrist, then set Wind to 24 Hours After and press New Test."
                    .to_string()
            }
            (Some(t), _) if t.readings.is_some() && t.full.is_none() => {
                "For the same test 24 hours after winding, save these results with Save \
                     Results for 24 Hours… (or save the session, which keeps them), leave the \
                     watch unwound and off the wrist, then load them and start the test again."
                    .to_string()
            }
            _ => String::new(),
        };
        if !about.is_empty() {
            ui.add(egui::Label::new(RichText::new(about).color(pal.text_secondary)).wrap());
        }
        if let Some(t) = test {
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "Each position settles {:.0} s and is measured for {:.0} s. Beat \
                             error is a size: which beat is longer depends on the beat the \
                             analysis locked on first. Save the session to keep the positions \
                             with the sound; `timegrapher session` reads it the same way.",
                        t.settle_s, t.measure_s
                    ))
                    .small()
                    .color(pal.text_tertiary),
                )
                .wrap(),
            );
        }
    });
    action
}

/// A row of the table: position, rate, rate less the mean, amplitude and
/// beat error.
pub type Row = (String, Option<f64>, Option<f64>, Option<f64>, Option<f64>);

/// The figures behind the results table: each position's label, rate,
/// rate less the mean, amplitude and beat error (a size), then the means.
pub struct Table {
    pub rows: Vec<Row>,
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
fn conditions(label: &str, bph: u32, m: &Measurement) -> Vec<String> {
    let mut out = Vec::new();
    if !m.watch_heard {
        let fraction = timegrapher_core::beats::timing_scatter_fraction(m.jitter_us, bph);
        out.push(match m.jitter_us.zip(fraction) {
            Some((j, f)) => format!(
                "{label}: no regular beat found (timing scatter {:.1} ms, {:.1}% of a beat)",
                j / 1000.0,
                f * 100.0
            ),
            None => format!("{label}: no regular beat found"),
        });
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
            if *count == 1 { "" } else { "s" }
        )),
        _ => None,
    }
}

/// The table from the readings, with the figures `timegrapher session`
/// prints for the same stretches and its position names.
pub fn table(readings: &[Reading]) -> Table {
    let figures: Vec<Figures> = readings
        .iter()
        .map(|r| {
            let m = &r.measurement;
            (
                r.position,
                m.rate_s_per_day,
                m.amplitude_deg,
                m.beat_error(),
            )
        })
        .collect();
    let notes = readings
        .iter()
        .flat_map(|r| conditions(&position_label(r.position), r.bph, &r.measurement))
        .collect();
    table_of(&figures, notes)
}

/// The table of an earlier test's saved results.
pub fn kept_table(full: &FullWind) -> Table {
    let figures: Vec<Figures> = full
        .positions
        .iter()
        .filter_map(|k| {
            Some((
                session::Position::parse(&k.position)?,
                k.rate_s_per_day,
                k.amplitude_deg,
                k.beat_error_ms,
            ))
        })
        .collect();
    table_of(&figures, Vec::new())
}

/// The table's summary lines are core `session::indices`, the values
/// `timegrapher session` gives for the same readings (repeat readings of
/// a position averaged first).
fn table_of(figures: &[Figures], notes: Vec<String>) -> Table {
    let ix = session::indices(0.0, figures);
    Table {
        rows: figures
            .iter()
            .map(|&(p, rate, amp, be)| {
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
}

/// The free take's table from core's, in the Current Position card's
/// order.
pub fn free_table(t: &session::PositionsTable) -> FreeTable {
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
            for (h, hint) in [
                (
                    "Duration",
                    "Time marked in the position, less its settling and masked time",
                ),
                ("Rate, s/d", "Seconds a day gained (+) or lost (−)"),
                (
                    "Amplitude, °",
                    "The balance's swing, from the lift angle set",
                ),
                ("Beat Error, ms", "From the unlock, as a size"),
            ] {
                // Beat error's figures are in the text colour in light
                // mode: its swatch beside the heading says whose they are.
                let be = pal.trace_beat_error;
                let swatch = (h == "Beat Error, ms" && pal.ink(be) != be).then_some(be);
                column_head_swatch(ui, FIGURE_W, h, hint, swatch);
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
                        right(ui, signed(*rate, 1), false, pal.trace_rate);
                        right(ui, plain(*amp, 0), false, pal.trace_amplitude);
                        right(ui, plain(*be, 2), false, pal.ink(pal.trace_beat_error));
                    }
                }
                ui.end_row();
            }
            if t.read >= 2 {
                ui.label(RichText::new("Average").strong().color(pal.text))
                    .on_hover_text("Over the positions read");
                right(ui, format!("{} of 6", t.read), false, pal.text_secondary);
                right(ui, signed(t.mean[0], 1), true, pal.trace_rate);
                right(ui, plain(t.mean[1], 0), true, pal.trace_amplitude);
                right(ui, plain(t.mean[2], 2), true, pal.ink(pal.trace_beat_error));
                ui.end_row();
                largest_label(ui);
                ui.label("");
                right(ui, plain(t.largest[0], 1), true, pal.trace_rate);
                right(ui, plain(t.largest[1], 0), true, pal.trace_amplitude);
                right(
                    ui,
                    plain(t.largest[2], 2),
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
/// later, read by core `session::evaluate` as `session` reads a file with
/// both. The kept figures stand in for the full-wind measurements.
pub fn comparison(full: &FullWind, hours: f64, readings: &[Reading]) -> Option<Comparison> {
    let template = readings.first()?;
    let mut all: Vec<Reading> = full
        .positions
        .iter()
        .filter_map(|k| {
            let position = session::Position::parse(&k.position)?;
            let mut measurement = template.measurement.clone();
            measurement.rate_s_per_day = k.rate_s_per_day;
            measurement.amplitude_deg = k.amplitude_deg;
            measurement.beat_error_unlock_ms = k.beat_error_ms;
            measurement.beat_error_ms = k.beat_error_ms;
            Some(Reading {
                label: position_label(position),
                position,
                wind_h: Some(0.0),
                measurement,
                ..template.clone()
            })
        })
        .collect();
    all.extend(readings.iter().map(|r| Reading {
        wind_h: Some(hours),
        ..r.clone()
    }));
    let report = session::evaluate(&all);
    let (first, last) = (report.states.first()?, report.states.last()?);
    if report.states.len() < 2 {
        return None;
    }
    let rows = readings
        .iter()
        .filter_map(|r| {
            let p = r.position;
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
    if p.code().starts_with(|c: char| c.is_ascii_digit()) {
        format!("{} ({}, {})", p.name(), p.common(), p.code())
    } else {
        format!("{} ({})", p.name(), p.common())
    }
}

/// The table as text a spreadsheet takes: a header, a row for each
/// position, the means, then the largest difference, the vertical less
/// horizontal figures and any conditions, one a line. Fields are split by
/// `sep` (a comma for a CSV file, a tab for the clipboard) and quoted when
/// they hold it; numbers use a plain minus sign.
pub fn table_text(t: &Table, sep: char) -> String {
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
        "Rate, s/d".into(),
        "From Average, s/d".into(),
        "Amplitude, °".into(),
        "Beat Error, ms".into(),
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
        "Average".into(),
        num(t.mean_rate, 1),
        String::new(),
        num(t.mean_amplitude, 0),
        num(t.mean_beat_error, 2),
    ]);
    out += &line(&[
        "Largest Difference".into(),
        num(t.largest_difference, 1),
        String::new(),
        num(t.largest_amplitude, 0),
        num(t.largest_beat_error, 2),
    ]);
    out.push('\n');
    out += &line(&[
        "Vertical less horizontal rate, s/d".into(),
        num(t.dvh_rate, 1),
    ]);
    out += &line(&[
        "Vertical less horizontal amplitude, °".into(),
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
            let d = if i < 3 { 1 } else { 0 };
            row.push(x.map_or_else(String::new, |x| format!("{x:.d$}")));
        }
        cells.push(row);
    }
    cells.push(Vec::new());
    let num = |v: Option<f64>| v.map_or_else(String::new, |x| format!("{x:.1}"));
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
    out.push('\n');
    let readings = test.readings_so_far();
    out += &table_text(&table(readings), sep);
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

/// Ask where to save the full-wind results for the 24 hour half, starting
/// in `dir`, and save them. Says what happened, or None when cancelled.
pub fn save_results(k: &FullWind, dir: &std::path::Path) -> Option<String> {
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
    let path = d.save_file()?;
    Some(match std::fs::write(&path, k.to_json()) {
        Ok(()) => format!("Saved the Positions Test's results to {}", path.display()),
        Err(e) => format!("Couldn't save the results to {}: {e}", path.display()),
    })
}

/// Ask where to save the test as a CSV file, starting in `dir`, and save
/// it there. Says what happened, or None when the dialog was cancelled.
pub fn save_csv(text: &str, dir: &std::path::Path) -> Option<String> {
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
    let path = d.save_file()?;
    Some(match std::fs::write(&path, text) {
        Ok(()) => format!("Saved the Positions Test to {}", path.display()),
        Err(e) => format!(
            "Couldn't save the Positions Test to {}: {e}",
            path.display()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use timegrapher_core::live::{LiveAnalyzer, LiveConfig};
    use timegrapher_core::measure::Caveat;
    use timegrapher_core::synth::{generate, SynthConfig};

    /// The 24 Hours After half finds the newest full-wind test kept in the
    /// watch's takes that began 12 to 36 hours ago, and no other.
    #[test]
    fn the_full_wind_test_is_the_newest_from_12_to_36_hours_ago() {
        let dir = std::env::temp_dir().join(format!("tg-full-wind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let now = 1_800_000_000.0;
        let keep = |name: &str, hours: f64| {
            let take = dir.join(name);
            std::fs::create_dir_all(&take).unwrap();
            let f = FullWind {
                format: RESULTS_FORMAT.into(),
                began_utc: String::new(),
                began_unix: now - hours * 3600.0,
                watch: "W".into(),
                calibre: String::new(),
                bph: None,
                lift_angle_deg: None,
                settle_s: None,
                measure_s: None,
                positions: vec![KeptRow {
                    position: "DU".into(),
                    rate_s_per_day: Some(1.0),
                    amplitude_deg: None,
                    beat_error_ms: None,
                }],
            };
            std::fs::write(take.join(RESULTS_FILE), f.to_json()).unwrap();
        };
        assert_eq!(full_wind_in(&dir, now), None, "no takes folder");
        keep("a", 40.0);
        keep("b", 6.0);
        assert_eq!(full_wind_in(&dir, now), None, "too old and too new");
        keep("c", 30.0);
        keep("d", 20.0);
        let found = full_wind_in(&dir, now).expect("found");
        assert_eq!(found.hours_ago(now), 20.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A figure that rounds to zero reads 0.0, with no sign.
    #[test]
    fn a_zero_reads_without_a_sign() {
        assert_eq!(signed(Some(-0.01), 1), "0.0");
        assert_eq!(signed(Some(0.04), 1), "0.0");
        assert_eq!(signed(Some(-0.06), 1), "−0.1");
        assert_eq!(signed(Some(2.0), 1), "+2.0");
        assert_eq!(signed(None, 1), "—");
    }

    /// The pane drawn for `test` in `phase` with `live` figures: each
    /// piece of text and where it is.
    fn drawn(test: &Test, phase: Phase, live: Option<&FreeCell>) -> Vec<(String, egui::Rect)> {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut found = Vec::new();
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    pane(
                        ui,
                        Some(test),
                        Some(phase),
                        None,
                        None,
                        live,
                        20.0,
                        60.0,
                        false,
                        None,
                    );
                });
            });
            found = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => {
                        Some((t.galley.text().to_string(), t.visual_bounding_rect()))
                    }
                    _ => None,
                })
                .collect();
        }
        found
    }

    /// The banner's buttons keep their places as the test moves on: Stop
    /// never moves, and Restart stays where it was, so a click meant for
    /// Stop as a position's measuring ends never lands on Restart. They
    /// sit on one line at one height, each at the left of its slot.
    #[test]
    fn the_test_buttons_keep_their_places_from_phase_to_phase() {
        let mut test = Test::new().timed(20.0, 60.0);
        let rect = |found: &[(String, egui::Rect)], text: &str| {
            found
                .iter()
                .find(|(t, _)| t == text)
                .map(|(_, r)| *r)
                .unwrap_or_else(|| panic!("{text}: {found:?}"))
        };
        let at = |found: &[(String, egui::Rect)], text: &str| rect(found, text).center().x;
        assert!(matches!(test.tick(0.0), Phase::Settling(_)));
        let ph = test.tick(5.0);
        let settling = drawn(&test, ph, None);
        let measuring_phase = test.tick(30.0);
        assert!(matches!(measuring_phase, Phase::Measuring(_)));
        let measuring = drawn(&test, measuring_phase, None);
        assert_eq!(test.tick(85.0), Phase::Turn);
        let turn = drawn(&test, Phase::Turn, None);
        // Stop Test is on the Listen card; the banner keeps the rest in
        // their places.
        for found in [&settling, &measuring, &turn] {
            assert!(!found.iter().any(|(t, _)| t == "Stop"), "{found:?}");
        }
        let restart = at(&measuring, "Restart DU Measurement");
        assert_eq!(at(&settling, "Restart DU Measurement"), restart);
        assert_eq!(at(&turn, "Restart DU Measurement"), restart);
        test.advance(86.0);
        test.pause(90.0);
        let ph = test.tick(91.0);
        let paused = drawn(&test, ph, None);
        at(&paused, "Resume");
        // One line: every button's words start at the same height (their
        // ink ends lower where a letter has a tail).
        let y = rect(&measuring, "Restart DU Measurement").top();
        for (found, text) in [
            (&measuring, "Pause"),
            (&turn, "Start DD Measurement"),
            (&turn, "Restart DU Measurement"),
            (&paused, "Resume"),
        ] {
            let top = rect(found, text).top();
            assert!((top - y).abs() < 0.5, "{text} at {top}, Restart at {y}");
        }
        // On the title's line, so the step's button is in reach in the
        // shortest pane: never under the progress bar.
        let title = rect(&turn, "Turn the watch to Dial Down");
        let start = rect(&turn, "Start DD Measurement");
        assert!(
            (start.center().y - title.center().y).abs() < 6.0,
            "{title:?} {start:?}"
        );
        // Pause is its own width at the left of the first slot, where
        // Start begins, not stretched across it: its words sit well short
        // of the slot's middle, where a stretched button would centre them.
        let pause = rect(&measuring, "Pause");
        let start = rect(&turn, "Start DD Measurement");
        assert!(
            pause.center().x < start.center().x - 20.0,
            "{pause:?} {start:?}"
        );
        assert!(pause.left() > start.left(), "{pause:?} {start:?}");
    }

    /// The guided table's headings never run into each other: each ends
    /// before the next begins.
    #[test]
    fn the_guided_tables_headings_stand_apart() {
        let mut test = Test::new().timed(20.0, 60.0);
        test.tick(0.0);
        let ph = test.tick(30.0);
        let found = drawn(&test, ph, None);
        let heads = [
            "Position",
            "Rate, s/d",
            "From Average, s/d",
            "Amplitude, °",
            "Beat Error, ms",
        ];
        let rects: Vec<egui::Rect> = heads
            .iter()
            .map(|h| {
                found
                    .iter()
                    .find(|(t, _)| t == h)
                    .map(|(_, r)| *r)
                    .unwrap_or_else(|| panic!("{h}: {found:?}"))
            })
            .collect();
        for (i, w) in rects.windows(2).enumerate() {
            assert!(
                w[0].right() + 8.0 <= w[1].left(),
                "{} ends at {}, {} begins at {}",
                heads[i],
                w[0].right(),
                heads[i + 1],
                w[1].left()
            );
        }
    }

    /// The position being measured shows its figures so far, once it has
    /// them; before then it says it is measuring.
    #[test]
    fn the_position_being_measured_shows_its_figures_so_far() {
        let mut test = Test::new().timed(20.0, 60.0);
        test.tick(0.0);
        let phase = test.tick(30.0);
        let words = |found: Vec<(String, egui::Rect)>| {
            found.into_iter().map(|(t, _)| t).collect::<Vec<_>>()
        };
        let before = words(drawn(&test, phase, None));
        assert!(before.iter().any(|t| t == "Measuring…"), "{before:?}");
        let live = FreeCell::Read(Some(-4.25), Some(281.4), Some(0.31));
        let after = words(drawn(&test, phase, Some(&live)));
        for want in ["−4.2", "281", "0.31"] {
            assert!(after.iter().any(|t| t == want), "{want}: {after:?}");
        }
        assert!(!after.iter().any(|t| t == "Measuring…"), "{after:?}");
        // Settling, there is nothing to show yet, whatever is passed.
        let mut test = Test::new().timed(20.0, 60.0);
        test.tick(0.0);
        let ph = test.tick(5.0);
        let settling = words(drawn(&test, ph, Some(&live)));
        assert!(settling.iter().any(|t| t == "Settling…"), "{settling:?}");
        assert!(!settling.iter().any(|t| t == "281"), "{settling:?}");
    }

    #[test]
    fn a_position_says_how_its_beat_error_was_given() {
        assert_eq!(
            caveat_condition("Dial Up", &Caveat::BeatErrorAsSize).as_deref(),
            Some("Dial Up: beat error given as a size, its sign differs either side of a cut")
        );
        assert_eq!(
            caveat_condition("Dial Up", &Caveat::SidesByCount { count: 1 }).as_deref(),
            Some(
                "Dial Up: Tick and Tock named by the beat count across 1 break, not told \
                 apart by their sound"
            )
        );
        assert_eq!(caveat_condition("Dial Up", &Caveat::LiftUnknown), None);
    }

    /// A bump of the stand inside a position, masked: the position is read
    /// without it, its rate comes back, the masked caveat says how much was
    /// left out, and the masked seconds aren't counted as beats missing.
    /// Masking after the test has finished reads it again.
    #[test]
    fn a_masked_bump_inside_a_position_is_left_out() {
        let each = SETTLE_S + MEASURE_S;
        // The first position, dial up, measures from 20 s to 80 s; the
        // bump knocks the beats about for 6 s from 45 s.
        let bump = (45.0, 51.0);
        let cfg = SynthConfig {
            duration_s: each * 6.0 + 1.0,
            rate_s_per_day: 0.0,
            ..Default::default()
        };
        let audio = generate(
            &cfg,
            |_| 280.0,
            |t| {
                if t >= bump.0 && t < bump.1 {
                    4000.0
                } else {
                    3.0
                }
            },
        );
        let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
        for b in audio.samples.chunks(4800) {
            live.push(b);
        }
        let mut t = Test::new();
        t.tick(0.0);
        for i in 1..=6 {
            t.tick(each * i as f64);
            t.advance(each * i as f64);
        }
        let log = crate::steady::log_of(&live).expect("beat rate");
        t.finish(&log, None, &[]);
        let first = |t: &Test| t.readings.as_ref().unwrap()[0].measurement.clone();
        let bumped = first(&t).rate_s_per_day.unwrap();
        assert!((bumped - 3.0).abs() > 5.0, "the bump didn't show: {bumped}");

        let masked = [(bump.0 - 1.0, bump.1 + 1.0)];
        live.set_masked(&masked);
        let log = crate::steady::log_of(&live).expect("beat rate");
        t.reread(&log, None, &masked);
        let m = first(&t);
        let rate = m.rate_s_per_day.unwrap();
        assert!((rate - 3.0).abs() < 1.0, "rate {rate}");
        assert!((m.masked_s - 8.0).abs() < 1e-9, "masked {}", m.masked_s);
        assert!(
            m.caveats
                .iter()
                .any(|c| matches!(c, Caveat::Masked { seconds } if (seconds - 8.0).abs() < 1e-9)),
            "{:?}",
            m.caveats
        );
        assert!(
            !m.caveats
                .iter()
                .any(|c| matches!(c, Caveat::BeatsMissing { .. })),
            "{:?}",
            m.caveats
        );
        // The other positions are as they were.
        let r = t.readings.as_ref().unwrap();
        assert_eq!(r.len(), 6);
        assert_eq!(r[1].measurement.masked_s, 0.0);
    }

    /// A jump in the beats inside a position (the sound lost its place by
    /// 40 ms at 50 s): the position's rate is fitted on each side of it
    /// and combined, as the whole take's is, not across it.
    #[test]
    fn a_jump_inside_a_position_is_cut() {
        let each = SETTLE_S + MEASURE_S;
        let duration = each * 6.0 + 1.0;
        let period = 0.125 / (1.0 + 3.0 / 86400.0);
        let log = BeatLog {
            sample_rate: 48000,
            duration_s: duration,
            bph: 28800,
            lift_deg: 52.0,
            beats: (0..)
                .map(|index| {
                    let t = index as f64 * period;
                    timegrapher_core::beats::Beat {
                        index,
                        time: t + if t >= 50.0 { 0.04 } else { 0.0 },
                        quality: 1.0,
                    }
                })
                .take_while(|b| b.time < duration)
                .collect(),
            amplitude_windows: Vec::new(),
            clipped_beats: Vec::new(),
            clipped_samples: 0,
            jumps: vec![50.0],
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        };
        let mut t = Test::new();
        t.tick(0.0);
        for i in 1..=6 {
            t.tick(each * i as f64);
            t.advance(each * i as f64);
        }
        t.finish(&log, None, &[]);
        let m = &t.readings.as_ref().unwrap()[0].measurement;
        let rate = m.rate_s_per_day.unwrap();
        assert!((rate - 3.0).abs() < 0.5, "rate {rate}");
    }

    /// Six positions with their own rates, each held for one settle and
    /// one measurement: the test reads each position's rate and the
    /// largest difference between them.
    #[test]
    fn six_positions_read_their_own_rates() {
        let each = SETTLE_S + MEASURE_S;
        let rates = [2.0, 4.0, -3.0, -1.0, 0.0, 6.0];
        let cfg = SynthConfig {
            duration_s: each * 6.0 + 1.0,
            rate_s_per_day: 0.0,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 280.0, |t| rates[((t / each) as usize).min(5)]);
        let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
        for b in audio.samples.chunks(4800) {
            live.push(b);
        }
        let log = crate::steady::log_of(&live).expect("beat rate");
        let mut t = Test::new();
        t.tick(0.0);
        for i in 1..=6 {
            assert_eq!(t.tick(each * i as f64), Phase::Turn);
            // Each position is read as soon as it is measured.
            assert!(t.needs_reading());
            t.read_new(&log, None, &[]);
            assert_eq!(t.readings_so_far().len(), i);
            t.advance(each * i as f64);
        }
        let so_far = t.readings_so_far().to_vec();
        t.finish(&log, None, &[]);
        let rates_of = |v: &[Reading]| -> Vec<Option<f64>> {
            v.iter().map(|r| r.measurement.rate_s_per_day).collect()
        };
        assert_eq!(rates_of(t.readings.as_deref().unwrap()), rates_of(&so_far));
        let readings = t.readings.as_ref().unwrap();
        assert_eq!(readings.len(), 6);
        for (r, want) in readings.iter().zip(rates) {
            let got = r.measurement.rate_s_per_day.unwrap();
            assert!((got - want).abs() < 1.0, "{}: {got} for {want}", r.label);
        }
        // The table's figures are session's: its D and mean X, its names.
        let tbl = table(readings);
        let report = session::evaluate(readings);
        let d = tbl.largest_difference.unwrap();
        assert!((d - report.states[0].d_rate.unwrap()).abs() < 1e-9);
        assert!((tbl.mean_rate.unwrap() - report.states[0].x.unwrap()).abs() < 1e-9);
        let dvh = tbl.dvh_rate.unwrap();
        assert!((dvh - report.states[0].dvh_rate.unwrap()).abs() < 1e-9);
        let dvh_a = tbl.dvh_amplitude.unwrap();
        assert!((dvh_a - report.states[0].dvh_amplitude.unwrap()).abs() < 1e-9);
        assert!((d - 9.0).abs() < 1.5, "D {d}");
        assert_eq!(tbl.rows[4].0, "Crown Up (CU, 3H)");
        let spread: f64 = tbl.rows.iter().filter_map(|r| r.2).sum();
        assert!(spread.abs() < 1e-9, "differences from the mean sum to zero");

        // Kept as the full-wind test, then the same watch 24 hours on,
        // 2 s/d faster in every position but crown right, 8 s/d faster
        // there, and 20° lower.
        let about = About {
            watch: "Test Watch".into(),
            calibre: "ETA 2824-2".into(),
            bph: Some(28800),
            lift_angle_deg: 52.0,
        };
        let full = t.kept(&about).unwrap();
        assert_eq!(full.positions.len(), 6);
        assert_eq!(full.positions[5].position, "CR");
        // Saved and read back as it was, with how it was read.
        let full = FullWind::from_json(&full.to_json()).unwrap();
        assert_eq!(full.calibre, "ETA 2824-2");
        assert_eq!(
            (full.bph, full.lift_angle_deg, full.settle_s, full.measure_s),
            (Some(28800), Some(52.0), Some(SETTLE_S), Some(MEASURE_S))
        );
        assert!(FullWind::from_json("{}").is_err());
        // A file saved before those were kept still reads.
        let mut old: serde_json::Value = serde_json::from_str(&full.to_json()).unwrap();
        for k in ["bph", "lift_angle_deg", "settle_s", "measure_s"] {
            old.as_object_mut().unwrap().remove(k);
        }
        let old = FullWind::from_json(&old.to_string()).unwrap();
        assert_eq!((old.bph, old.settle_s), (None, None));
        // The text saved and copied names the watch and its calibre first.
        let csv = export_text(&t, &about, ',');
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "Watch,Test Watch");
        assert_eq!(lines[1], "Calibre,ETA 2824-2");
        assert_eq!(lines[2], "\"Beat Rate, bph\",28800");
        assert_eq!(lines[3], "\"Lift Angle, °\",52");
        assert!(lines[4].starts_with("\"Test Began, UTC\",20"));
        assert!(csv.contains("\nPosition,\"Rate, s/d\""));
        let later: Vec<Reading> = readings
            .iter()
            .map(|r| {
                let mut r = r.clone();
                let m = &mut r.measurement;
                let up = if r.position == session::Position::H12 {
                    8.0
                } else {
                    2.0
                };
                m.rate_s_per_day = m.rate_s_per_day.map(|x| x + up);
                m.amplitude_deg = m.amplitude_deg.map(|x| x - 20.0);
                r
            })
            .collect();
        let c = comparison(&full, 24.0, &later).unwrap();
        assert_eq!(c.rows.len(), 6);
        for (label, v) in &c.rows {
            let up = if label.starts_with("Crown Right") {
                8.0
            } else {
                2.0
            };
            assert!((v[2].unwrap() - up).abs() < 1e-9, "{label}: {v:?}");
            assert!((v[5].unwrap() + 20.0).abs() < 1e-9, "{label}: {v:?}");
        }
        assert!((c.im.unwrap() - 2.0).abs() < 1e-9, "crown right left out");
        assert!(
            (c.ie.unwrap() - 3.0).abs() < 1e-9,
            "the mean of the changes"
        );
        let csv = comparison_text(&c, ',');
        assert!(csv.starts_with("Position,\"Rate at Full Wind, s/d\",\"Rate at 24.0 h, s/d\""));
        // A 24 hour half is not kept in place of the full-wind one.
        let mut half = Test::after(full);
        assert!(half.wind_h().unwrap() > 0.0);
        half.readings = Some(later);
        assert!(half.kept(&about).is_none());
        // Its text carries the comparison and the hours since full wind.
        let csv = export_text(&half, &about, ',');
        assert!(csv.contains("Hours After Full Wind,"));
        assert!(csv.contains("\"Rate at Full Wind, s/d\""));
    }

    #[test]
    fn the_test_settles_measures_and_waits_for_each_turn() {
        let mut t = Test::new();
        assert_eq!(t.tick(100.0), Phase::Settling(SETTLE_S));
        assert_eq!(t.tick(100.0 + SETTLE_S + 10.0), Phase::Measuring(10.0));
        let end = 100.0 + SETTLE_S + MEASURE_S;
        assert_eq!(t.tick(end), Phase::Turn);
        assert_eq!(t.measured_count(), 1);
        assert_eq!(t.next_position(), Some(Position::DialDown));
        // Measured again: the first stretch is dropped and made afresh.
        t.redo(end);
        assert_eq!(t.measured_count(), 0);
        assert_eq!(t.tick(end + SETTLE_S + MEASURE_S), Phase::Turn);
        assert_eq!(t.measured_count(), 1);
        for (i, &p) in ORDER.iter().enumerate().skip(1) {
            let now = 1000.0 * i as f64;
            assert_eq!(t.advance(now), Some(p));
            assert_eq!(t.tick(now + SETTLE_S + MEASURE_S), Phase::Turn);
        }
        assert_eq!(t.measured_count(), 6);
        assert_eq!(t.next_position(), None);
    }

    /// A test with its own times settles and measures for them.
    #[test]
    fn the_times_in_each_position_can_be_set() {
        let mut t = Test::new().timed(45.0, 120.0);
        assert_eq!(t.tick(0.0), Phase::Settling(45.0));
        assert_eq!(t.tick(50.0), Phase::Measuring(5.0));
        assert_eq!(t.tick(165.0), Phase::Turn);
        assert_eq!(t.stretches(), [(Position::DialUp, 45.0, 165.0)]);
        assert_eq!(
            t.unused(170.0),
            [(0.0, 45.0, SETTLING), (165.0, 170.0, NOT_USED)]
        );
    }

    /// Paused part way through measuring: what was measured is left out,
    /// the pause is Not Used, and Resume settles and measures afresh.
    #[test]
    fn a_pause_starts_the_position_again_on_resume() {
        let (sm, m) = (SETTLE_S, MEASURE_S);
        let mut t = Test::new();
        t.tick(0.0);
        t.pause(sm + 10.0);
        assert_eq!(t.tick(500.0), Phase::Paused);
        assert!(t.measured(500.0).is_empty());
        assert_eq!(
            t.unused(100.0),
            [
                (0.0, sm, SETTLING),
                (sm, sm + 10.0, NOT_USED),
                (sm + 10.0, 100.0, NOT_USED)
            ]
        );
        t.resume(100.0);
        assert_eq!(t.tick(100.0), Phase::Settling(sm));
        assert_eq!(t.tick(100.0 + sm + m), Phase::Turn);
        assert_eq!(
            t.stretches(),
            [(Position::DialUp, 100.0 + sm, 100.0 + sm + m)]
        );
        assert_eq!(t.unused(100.0 + sm + m)[2], (sm + 10.0, 100.0, NOT_USED));
    }

    /// The time the test leaves out: each settling, the wait for the next
    /// turn, a measurement started again, and a test stopped part way.
    #[test]
    fn the_unused_time_is_named() {
        let (sm, m) = (SETTLE_S, MEASURE_S);
        let mut t = Test::new();
        t.tick(0.0);
        assert_eq!(t.unused(5.0), [(0.0, 5.0, SETTLING)]);
        assert_eq!(t.unused(sm + 1.0), [(0.0, sm, SETTLING)]);
        // Measured, then 30 s turning the watch before the next start.
        let end = sm + m;
        t.tick(end);
        assert_eq!(
            t.unused(end + 30.0),
            [(0.0, sm, SETTLING), (end, end + 30.0, NOT_USED)]
        );
        t.advance(end + 30.0);
        let s2 = end + 30.0;
        // Knocked 10 s into measuring: that try is left out whole.
        t.redo(s2 + sm + 10.0);
        let s3 = s2 + sm + 10.0;
        let u = t.unused(s3 + 1.0);
        assert_eq!(
            &u[2..],
            [
                (s2, s2 + sm, SETTLING),
                (s2 + sm, s3, NOT_USED),
                (s3, s3 + 1.0, SETTLING)
            ]
        );
        // Stopped while measuring: none of it was used.
        t.stop(s3 + sm + 5.0);
        let u = t.unused(s3 + 100.0);
        assert_eq!(u.last(), Some(&(s3 + sm, s3 + sm + 5.0, NOT_USED)));
        assert_eq!(t.stretches().len(), 1);
    }

    /// Stopped while waiting to turn the watch, the position just measured
    /// keeps its measured stretch: only the wait since is unused.
    #[test]
    fn a_stop_while_turning_keeps_the_measured_position() {
        let (sm, m) = (SETTLE_S, MEASURE_S);
        let mut t = Test::new();
        t.tick(0.0);
        let end = sm + m;
        assert_eq!(t.tick(end + 1.0), Phase::Turn);
        t.stop(end + 30.0);
        let u = t.unused(end + 40.0);
        assert_eq!(u, [(0.0, sm, SETTLING), (end, end + 30.0, NOT_USED)]);
        assert_eq!(t.stretches(), [(Position::DialUp, sm, end)]);
    }

    /// A test begun ten minutes into the sound: the time before it is
    /// named, and the first position is measured from its own settle, the
    /// stretch under way included as far as it has got.
    #[test]
    fn the_time_before_the_test_and_the_measuring_are_marked() {
        let (sm, m) = (SETTLE_S, MEASURE_S);
        let mut t = Test::new();
        t.tick(600.0);
        assert_eq!(t.unused(601.0)[0], (0.0, 600.0, BEFORE_TEST));
        assert!(t.measured(610.0).is_empty());
        assert_eq!(
            t.measured(600.0 + sm + 5.0),
            [(600.0 + sm, 600.0 + sm + 5.0)]
        );
        // Started again: Before Test stays where the test began.
        t.redo(600.0 + sm + 10.0);
        assert_eq!(t.unused(700.0)[0], (0.0, 600.0, BEFORE_TEST));
        let s = 600.0 + sm + 10.0;
        t.tick(s + sm + m);
        assert_eq!(t.measured(s + sm + m + 30.0), [(s + sm, s + sm + m)]);
    }

    /// The table's means and differences are `session`'s: a position read
    /// twice counts once, its readings averaged, as core
    /// `session::indices` takes it.
    #[test]
    fn the_table_summary_is_the_session_values() {
        use session::Position as P;
        let figures: Vec<Figures> = vec![
            (P::CH, Some(10.0), Some(280.0), Some(0.2)),
            (P::CH, Some(20.0), Some(290.0), Some(0.4)),
            (P::H3, Some(0.0), Some(250.0), Some(0.1)),
        ];
        let t = table_of(&figures, Vec::new());
        let ix = session::indices(0.0, &figures);
        // Dial Up averages to 15 s/d before the mean and the difference.
        assert_eq!(t.mean_rate, Some(7.5));
        assert_eq!(t.largest_difference, Some(15.0));
        // Dial Up's 285° and 0.3 ms against Crown Up's 250° and 0.1 ms.
        assert_eq!(t.largest_amplitude, Some(35.0));
        assert!((t.largest_beat_error.unwrap() - 0.2).abs() < 1e-9);
        assert_eq!(t.largest_amplitude, ix.d_amplitude);
        assert_eq!(t.largest_beat_error, ix.d_beat_error);
        assert_eq!(t.mean_rate, ix.x);
        assert_eq!(t.mean_amplitude, ix.amplitude_mean);
        assert_eq!(t.mean_beat_error, ix.beat_error_mean);
        assert_eq!(t.dvh_rate, ix.dvh_rate);
        assert_eq!(t.dvh_amplitude, ix.dvh_amplitude);
        assert_eq!(t.rows.len(), 3);
        assert_eq!(t.rows[0].2, Some(2.5));
    }

    /// The table saved and copied: a label holding the comma is quoted in
    /// the CSV and not with tabs, a missing figure is an empty field, and
    /// the minus sign is a plain one.
    #[test]
    fn the_table_is_written_for_a_spreadsheet() {
        let t = Table {
            rows: vec![
                (
                    "Dial Up (DU)".into(),
                    Some(-3.25),
                    Some(-1.0),
                    Some(281.0),
                    Some(0.4),
                ),
                ("Crown Up (CU, 3H)".into(), None, None, Some(250.0), None),
            ],
            mean_rate: Some(-2.25),
            mean_amplitude: Some(265.5),
            mean_beat_error: Some(0.4),
            largest_difference: None,
            largest_amplitude: Some(31.0),
            largest_beat_error: None,
            dvh_rate: None,
            dvh_amplitude: Some(-31.0),
            notes: vec!["Crown Up (CU, 3H): no regular beat found".into()],
        };
        let csv = table_text(&t, ',');
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(
            lines[0],
            "Position,\"Rate, s/d\",\"From Average, s/d\",\"Amplitude, °\",\"Beat Error, ms\""
        );
        assert_eq!(lines[1], "Dial Up (DU),-3.2,-1.0,281,0.40");
        assert_eq!(lines[2], "\"Crown Up (CU, 3H)\",,,250,");
        assert_eq!(lines[3], "Average,-2.2,,266,0.40");
        assert_eq!(lines[4], "Largest Difference,,,31,");
        assert_eq!(lines[5], "");
        assert_eq!(lines[7], "\"Vertical less horizontal amplitude, °\",-31");
        assert_eq!(lines[8], "\"Crown Up (CU, 3H): no regular beat found\"");
        let tsv = table_text(&t, '\t');
        assert!(tsv
            .lines()
            .nth(2)
            .unwrap()
            .starts_with("Crown Up (CU, 3H)\t\t"));
        assert!(!csv.contains('−') && !tsv.contains('"'));
    }
}
