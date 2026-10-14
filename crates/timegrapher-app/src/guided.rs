//! The Positions Test: the app leads the watchmaker through the six
//! positions in the app's order, waits for the watch to settle in each,
//! measures it, and then shows the six in a table with the figures
//! `timegrapher session` reads for the same stretches: each rate, its
//! difference from the mean, amplitude and beat error, and the means. No
//! verdict is given.
//!
//! Each position is a stretch of the one listening session, so the strip,
//! the charts and a saved session keep the positions as any change of
//! position does.

use crate::app::Position;
use crate::theme;
use eframe::egui::{self, RichText, Vec2};
use timegrapher_core::clock::ClockFit;
use timegrapher_core::session::{self, Measurement, Reading};
use timegrapher_core::stream::BeatLog;

/// The file a finished test just after winding is saved to, from the
/// results window or beside a saved take, for the same six positions 24
/// hours later.
pub const RESULTS_FILE: &str = "positions-test.json";
/// The file's `format`, versioned: a later version adds keys.
pub const RESULTS_FORMAT: &str = "timegrapher-positions-test/1";

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
    /// Each position, in the order measured.
    pub positions: Vec<KeptRow>,
}

/// One position's figures from the full-wind test.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct KeptRow {
    /// The position's code as `timegrapher session` reads it: CH, CB,
    /// 9H, 6H, 3H, 12H.
    pub position: String,
    pub rate_s_per_day: Option<f64>,
    pub amplitude_deg: Option<f64>,
    /// A size, from the unlock where found, ms.
    pub beat_error_ms: Option<f64>,
}

fn unix_now() -> f64 {
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

/// Seconds the watch is left to settle after it is turned, and then
/// measured: the project's choice. A minute brings the rate's ± near half
/// a second a day.
pub const SETTLE_S: f64 = 20.0;
pub const MEASURE_S: f64 = 60.0;

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
    Finished,
}

/// What the strip says over the time the test didn't use: the settling
/// in each position, the rest of the time in a position that wasn't
/// measured (waiting to be turned, a measurement started again), and the
/// time before the test began.
pub const SETTLING: &str = "Settling";
pub const NOT_USED: &str = "Not Used for Positions Test";
pub const BEFORE_TEST: &str = "Before Test";

/// What the banner's buttons asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Next,
    Redo,
    Stop,
    Results,
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
    /// The six readings, once the last position is measured.
    pub readings: Option<Vec<Reading>>,
    pub show_results: bool,
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
            readings: None,
            show_results: false,
            began_unix: unix_now(),
            full: None,
        }
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
    pub fn kept(&self, watch: &str, calibre: &str) -> Option<FullWind> {
        if self.full.is_some() {
            return None;
        }
        let readings = self.readings.as_ref()?;
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(self.began_unix);
        Some(FullWind {
            format: RESULTS_FORMAT.into(),
            began_utc: timegrapher_core::recorder::utc(at),
            began_unix: self.began_unix,
            watch: watch.trim().to_string(),
            calibre: calibre.to_string(),
            positions: readings
                .iter()
                .map(|r| KeptRow {
                    position: r.position.code().to_string(),
                    rate_s_per_day: r.measurement.rate_s_per_day,
                    amplitude_deg: r.measurement.amplitude_deg,
                    beat_error_ms: r.measurement.beat_error(),
                })
                .collect(),
        })
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
        let start = *self.start.get_or_insert(now);
        self.began.get_or_insert(start);
        let t = now - start;
        if t < SETTLE_S {
            return Phase::Settling(SETTLE_S - t);
        }
        if t < SETTLE_S + MEASURE_S {
            return Phase::Measuring(t - SETTLE_S);
        }
        if self.spans.len() <= self.step {
            let from = start + SETTLE_S;
            self.spans.push((self.position(), from, from + MEASURE_S));
        }
        Phase::Turn
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
        self.close_step(now, true);
        self.spans.truncate(self.step);
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
            out.extend(Self::step_unused(s, now, false));
        }
        out
    }

    /// One attempt at a position, started at `start`, seen at `now`.
    fn step_unused(start: f64, now: f64, given_up: bool) -> Vec<(f64, f64, &'static str)> {
        let mut out = Vec::new();
        if now <= start {
            return out;
        }
        let settled = start + SETTLE_S;
        out.push((start, now.min(settled), SETTLING));
        let end = settled + MEASURE_S;
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
        self.close_step(now, true);
        self.start = None;
    }

    /// Keep this attempt's unused time, at `now` as it ends.
    fn close_step(&mut self, now: f64, given_up: bool) {
        if let Some(s) = self.start {
            self.unused.extend(Self::step_unused(s, now, given_up));
        }
    }

    /// The stretches the test measures, up to `now`: each position done,
    /// and the one under way as far as it has got.
    pub fn measured(&self, now: f64) -> Vec<(f64, f64)> {
        let mut out: Vec<(f64, f64)> = self.spans.iter().map(|&(_, a, b)| (a, b)).collect();
        if let (Some(s), None, true) = (self.start, &self.readings, self.spans.len() <= self.step) {
            let from = s + SETTLE_S;
            if now > from {
                out.push((from, now.min(from + MEASURE_S)));
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
    pub fn finish(&mut self, log: &BeatLog, clock: Option<&ClockFit>) {
        let readings = self
            .spans
            .iter()
            .filter_map(|&(p, from, to)| {
                let position = session::Position::parse(p.code())?;
                let measurement: Measurement = session::measure(log, clock, from, to);
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
            })
            .collect();
        self.readings = Some(readings);
        self.show_results = true;
    }
}

/// The banner's title for the position under way: "Dial Up (1 of 6)", and
/// in the 24 hour half "Dial Up (1 of 6, 24.1 h After Full Wind)".
fn step_title(test: &Test) -> String {
    let (name, n, of) = (test.position().name(), test.step + 1, ORDER.len());
    match test.wind_h() {
        Some(h) => format!("{name} ({n} of {of}, {h:.1} h After Full Wind)"),
        None => format!("{name} ({n} of {of})"),
    }
}

/// The banner over the readings while the test runs: the position, a
/// progress bar for its settling and measuring, and what to do next.
pub fn banner(ui: &mut egui::Ui, test: &Test, phase: Phase) -> Option<Action> {
    let pal = theme::pal(ui);
    let mut action = None;
    let fill = pal.accent;
    let frame = theme::card()
        .fill(fill.gamma_multiply(0.14))
        .inner_margin(egui::Margin::symmetric(14, 10));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (title, detail, frac) = match phase {
                Phase::Settling(left) => (
                    step_title(test),
                    format!("Letting the watch settle, {left:.0} s"),
                    ((SETTLE_S - left) / (SETTLE_S + MEASURE_S)) as f32,
                ),
                Phase::Measuring(done) => (
                    step_title(test),
                    format!("Measuring, {done:.0} of {MEASURE_S:.0} s"),
                    ((SETTLE_S + done) / (SETTLE_S + MEASURE_S)) as f32,
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
                        "press Finish to see the results".to_string(),
                        1.0,
                    ),
                },
                Phase::Finished => ("Positions Test done".to_string(), String::new(), 1.0),
            };
            ui.vertical(|ui| {
                ui.label(theme::caption("Positions Test").color(pal.text_secondary));
                ui.label(
                    RichText::new(title)
                        .font(theme::semibold(15.0))
                        .color(pal.text),
                );
                ui.label(RichText::new(detail).small().color(pal.text_secondary));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(egui::Button::new("Stop").frame_when_inactive(false))
                    .on_hover_text("End the test. The session and its positions stay.")
                    .clicked()
                {
                    action = Some(Action::Stop);
                }
                match phase {
                    Phase::Turn => {
                        let (next, next_hint) = match test.next_position() {
                            Some(p) => (
                                format!("Start {} Measurement", p.code()),
                                format!(
                                    "Press once the watch is {}: it settles for {SETTLE_S:.0} s, \
                                     then {} is measured for {MEASURE_S:.0} s",
                                    p.name().to_lowercase(),
                                    p.name()
                                ),
                            ),
                            None => (
                                "Finish".to_string(),
                                "Read the six positions together".to_string(),
                            ),
                        };
                        if ui
                            .add(theme::primary(ui, &next))
                            .on_hover_text(next_hint)
                            .clicked()
                        {
                            action = Some(Action::Next);
                        }
                        if ui
                            .button(format!("Restart {} Measurement", test.position().code()))
                            .on_hover_text(format!(
                                "The {} measurement was disturbed (a knock, a noise): put the \
                                 watch back {} and settle and measure it again",
                                test.position().name(),
                                test.position().name().to_lowercase()
                            ))
                            .clicked()
                        {
                            action = Some(Action::Redo);
                        }
                    }
                    Phase::Finished => {
                        if ui.add(theme::primary(ui, "Show Results")).clicked() {
                            action = Some(Action::Results);
                        }
                    }
                    _ => {
                        if ui
                            .button(format!("Restart {} Measurement", test.position().code()))
                            .on_hover_text(format!(
                                "Settle and measure {} again from now, as after a knock",
                                test.position().name()
                            ))
                            .clicked()
                        {
                            action = Some(Action::Redo);
                        }
                    }
                }
                ui.add_space(12.0);
                ui.add(
                    egui::ProgressBar::new(frac.clamp(0.0, 1.0))
                        .desired_width(160.0)
                        .desired_height(8.0)
                        .fill(pal.accent),
                );
            });
        });
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
    use timegrapher_core::measure::Caveat;
    let mut out = Vec::new();
    if !m.watch_heard {
        out.push(match m.jitter_us {
            Some(j) => format!(
                "{label}: no regular beat found (timing scatter {:.1} ms, {:.1}% of a beat)",
                j / 1000.0,
                j / (3.6e9 / bph.max(1) as f64) * 100.0
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
    for c in &m.caveats {
        if let Caveat::ShortSpan { span_s } = c {
            out.push(format!("{label}: {span_s:.0} s of beats"));
        }
    }
    out
}

fn mean(v: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let v: Vec<f64> = v.flatten().collect();
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

/// The table from the readings, with the figures `timegrapher session`
/// prints for the same stretches and its position names.
pub fn table(readings: &[Reading]) -> Table {
    let mean_rate = mean(readings.iter().map(|r| r.measurement.rate_s_per_day));
    let rates: Vec<f64> = readings
        .iter()
        .filter_map(|r| r.measurement.rate_s_per_day)
        .collect();
    let largest_difference = (rates.len() >= 2).then(|| {
        rates.iter().copied().fold(f64::MIN, f64::max)
            - rates.iter().copied().fold(f64::MAX, f64::min)
    });
    // Vertical less horizontal, as `session` takes it.
    let side = |vertical: bool, f: fn(&Measurement) -> Option<f64>| {
        mean(
            readings
                .iter()
                .filter(|r| r.position.is_vertical() == vertical)
                .map(|r| f(&r.measurement)),
        )
    };
    let dvh =
        |f: fn(&Measurement) -> Option<f64>| side(true, f).zip(side(false, f)).map(|(v, h)| v - h);
    let dvh_rate = dvh(|m| m.rate_s_per_day);
    let dvh_amplitude = dvh(|m| m.amplitude_deg);
    Table {
        rows: readings
            .iter()
            .map(|r| {
                let m = &r.measurement;
                (
                    position_label(r.position),
                    m.rate_s_per_day,
                    m.rate_s_per_day.zip(mean_rate).map(|(x, x0)| x - x0),
                    m.amplitude_deg,
                    m.beat_error(),
                )
            })
            .collect(),
        mean_rate,
        mean_amplitude: mean(readings.iter().map(|r| r.measurement.amplitude_deg)),
        mean_beat_error: mean(readings.iter().map(|r| r.measurement.beat_error())),
        largest_difference,
        dvh_rate,
        dvh_amplitude,
        notes: readings
            .iter()
            .flat_map(|r| conditions(&position_label(r.position), r.bph, &r.measurement))
            .collect(),
    }
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
    /// The change in the mean rate over the positions measured in both
    /// (Ie), s/d.
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
fn position_label(p: session::Position) -> String {
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
        "From Mean, s/d".into(),
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
        "Mean".into(),
        num(t.mean_rate, 1),
        String::new(),
        num(t.mean_amplitude, 0),
        num(t.mean_beat_error, 2),
    ]);
    out.push('\n');
    out += &line(&[
        "Largest difference between rates, s/d".into(),
        num(t.largest_difference, 1),
    ]);
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
    cells.push(vec!["Change in the mean rate (Ie), s/d".into(), num(c.ie)]);
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

/// Ask where to save the full-wind results for the 24 hour half, starting
/// in `dir`, and save them. Says what happened, or None when cancelled.
fn save_results(k: &FullWind, dir: &std::path::Path) -> Option<String> {
    let stamp: String = k
        .began_utc
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'T')
        .take(15)
        .collect();
    let mut d = rfd::FileDialog::new()
        .set_title("Save the Results for the Test 24 Hours Later")
        .set_file_name(format!("positions-test-{stamp}Z.json"))
        .add_filter("Positions Test results", &["json"]);
    if dir.is_dir() {
        d = d.set_directory(dir);
    }
    let path = d.save_file()?;
    Some(match std::fs::write(&path, k.to_json()) {
        Ok(()) => format!("Saved the Positions Test's results to {}", path.display()),
        Err(e) => format!("Couldn't save the results to {}: {e}", path.display()),
    })
}

/// Ask where to save the table as a CSV file, starting in `dir`, and save
/// it there. Says what happened, or None when the dialog was cancelled.
fn save_csv(text: &str, dir: &std::path::Path) -> Option<String> {
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

/// The results: a table of each position's rate, its difference from the
/// mean, amplitude and beat error, with the means, as `timegrapher
/// session` reads them, and buttons to save it as a CSV file (the dialog
/// starting in `dir`) or copy it. No verdict: the figures are for the
/// watchmaker to judge. Returns a line saying what a button did.
pub fn results(
    ctx: &egui::Context,
    test: &mut Test,
    dir: &std::path::Path,
    watch: &str,
    calibre: &str,
) -> Option<String> {
    let readings = test.readings.as_ref()?;
    let mut open = test.show_results;
    let mut said = None;
    let t = table(readings);
    let full_wind = test.full.is_none();
    let c = test
        .full
        .as_ref()
        .and_then(|(f, h)| comparison(f, *h, readings));
    let text = |sep: char| {
        let mut out = table_text(&t, sep);
        if let Some(c) = &c {
            out.push('\n');
            out += &comparison_text(c, sep);
        }
        out
    };
    egui::Window::new("Positions Test")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .show(ctx, |ui| {
            let pal = theme::pal(ui);
            let dash = || "—".to_string();
            let signed =
                |v: Option<f64>| v.map_or_else(dash, |x| format!("{x:+.1}").replace('-', "−"));
            let plain = |v: Option<f64>, d: usize| v.map_or_else(dash, |x| format!("{x:.d$}"));
            let right = |ui: &mut egui::Ui, text: String, strong: bool| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let t = RichText::new(text).color(pal.text);
                    ui.label(if strong { t.strong() } else { t });
                });
            };
            egui::Grid::new("positions")
                .num_columns(5)
                .spacing(Vec2::new(24.0, 6.0))
                .show(ui, |ui| {
                    ui.label(theme::caption("Position").color(pal.text_secondary));
                    for (h, hint) in [
                        ("Rate, s/d", "Seconds a day gained (+) or lost (−)"),
                        (
                            "From Mean, s/d",
                            "This position's rate less the mean of all six",
                        ),
                        (
                            "Amplitude, °",
                            "The balance's swing, from the lift angle set",
                        ),
                        ("Beat Error, ms", "From the unlock, as a size"),
                    ] {
                        ui.label(theme::caption(h).color(pal.text_secondary))
                            .on_hover_text(hint);
                    }
                    ui.end_row();
                    for (label, rate, from_mean, amp, be) in &t.rows {
                        ui.label(label);
                        right(ui, signed(*rate), false);
                        right(ui, signed(*from_mean), false);
                        right(ui, plain(*amp, 0), false);
                        right(ui, plain(*be, 2), false);
                        ui.end_row();
                    }
                    ui.label(RichText::new("Mean").strong());
                    right(ui, signed(t.mean_rate), true);
                    right(ui, String::new(), false);
                    right(ui, plain(t.mean_amplitude, 0), true);
                    right(ui, plain(t.mean_beat_error, 2), true);
                    ui.end_row();
                });
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!(
                    "Largest difference between the positions' rates: {} s/d.",
                    plain(t.largest_difference, 1)
                ))
                .color(pal.text),
            )
            .on_hover_text("D: the fastest position's rate less the slowest's");
            ui.label(
                RichText::new(format!(
                    "Vertical less horizontal: {} s/d in rate, {}° in amplitude.",
                    signed(t.dvh_rate),
                    t.dvh_amplitude
                        .map_or_else(dash, |x| format!("{x:+.0}").replace('-', "−"))
                ))
                .color(pal.text),
            )
            .on_hover_text(
                "DVH: the vertical positions' mean less the horizontal ones', \
                 for rate and for amplitude",
            );
            for n in &t.notes {
                ui.label(RichText::new(n).color(pal.text));
            }
            if let Some(c) = &c {
                ui.add_space(12.0);
                ui.label(
                    RichText::new(format!("Full Wind Against {:.1} Hours", c.hours))
                        .strong()
                        .color(pal.text),
                )
                .on_hover_text(
                    "The full-wind test loaded at the start, against this one, run without \
                     winding or wearing the watch in between",
                );
                ui.add_space(4.0);
                egui::Grid::new("full-wind")
                    .num_columns(7)
                    .spacing(Vec2::new(18.0, 6.0))
                    .show(ui, |ui| {
                        let h = format!("{:.1} h", c.hours);
                        ui.label(theme::caption("Position").color(pal.text_secondary));
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
                            ui.label(theme::caption(&head).color(pal.text_secondary))
                                .on_hover_text(hint);
                        }
                        ui.end_row();
                        for (label, v) in &c.rows {
                            ui.label(label);
                            right(ui, signed(v[0]), false);
                            right(ui, signed(v[1]), false);
                            right(ui, signed(v[2]), true);
                            right(ui, plain(v[3], 0), false);
                            right(ui, plain(v[4], 0), false);
                            right(
                                ui,
                                v[5].map_or_else(dash, |x| format!("{x:+.0}").replace('-', "−")),
                                true,
                            );
                            ui.end_row();
                        }
                    });
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!(
                        "Largest change in rate, crown right left out: {} s/d. Change in \
                         the mean rate: {} s/d.",
                        signed(c.im),
                        signed(c.ie)
                    ))
                    .color(pal.text),
                )
                .on_hover_text(
                    "Im: the largest change over the positions without crown right (12H), \
                     as NIHS 93-10 takes it. Ie: the mean rate's change over the positions \
                     measured both times.",
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .button("Save as CSV…")
                    .on_hover_text(
                        "Save the table, the means and the differences as a CSV file a \
                         spreadsheet opens",
                    )
                    .clicked()
                {
                    said = save_csv(&text(','), dir);
                }
                if ui
                    .button("Copy")
                    .on_hover_text(
                        "Copy the table with tabs between the columns, to paste into a \
                         spreadsheet or a message",
                    )
                    .clicked()
                {
                    ui.ctx().copy_text(text('\t'));
                    said = Some("Copied the Positions Test table".to_string());
                }
                if let Some(k) = test.kept(watch, calibre) {
                    if ui
                        .button("Save Results for 24 Hours…")
                        .on_hover_text(format!(
                            "Save these results to load when you run the test again 24 hours \
                             after winding. Saving the session keeps them too, as {RESULTS_FILE} \
                             in its folder."
                        ))
                        .clicked()
                    {
                        said = save_results(&k, dir);
                    }
                }
            });
            if full_wind {
                ui.add_space(8.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "For the same test 24 hours after winding, save these results \
                             (or the session), leave the watch unwound and off the wrist, then \
                             start the Positions Test again and load them.",
                        )
                        .color(pal.text_secondary),
                    )
                    .wrap(),
                );
            }
            ui.add_space(8.0);
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "Each position settled {SETTLE_S:.0} s and was measured for \
                         {MEASURE_S:.0} s. Beat error is a size: which beat is longer depends on \
                         the beat the analysis locked on first. Save the session to keep the \
                         positions with the sound; `timegrapher session` reads it the same way."
                    ))
                    .small()
                    .color(pal.text_tertiary),
                )
                .wrap(),
            );
        });
    test.show_results = open;
    said
}

#[cfg(test)]
mod tests {
    use super::*;
    use timegrapher_core::live::{LiveAnalyzer, LiveConfig};
    use timegrapher_core::synth::{generate, SynthConfig};

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
            t.advance(each * i as f64);
        }
        t.finish(&log, None);
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
        let full = t.kept("Test Watch", "ETA 2824-2").unwrap();
        assert_eq!(full.positions.len(), 6);
        assert_eq!(full.positions[5].position, "12H");
        // Saved and read back as it was.
        let full = FullWind::from_json(&full.to_json()).unwrap();
        assert_eq!(full.calibre, "ETA 2824-2");
        assert!(FullWind::from_json("{}").is_err());
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
        assert!(half.kept("Test Watch", "").is_none());
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
            dvh_rate: None,
            dvh_amplitude: Some(-31.0),
            notes: vec!["Crown Up (CU, 3H): no regular beat found".into()],
        };
        let csv = table_text(&t, ',');
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(
            lines[0],
            "Position,\"Rate, s/d\",\"From Mean, s/d\",\"Amplitude, °\",\"Beat Error, ms\""
        );
        assert_eq!(lines[1], "Dial Up (DU),-3.2,-1.0,281,0.40");
        assert_eq!(lines[2], "\"Crown Up (CU, 3H)\",,,250,");
        assert_eq!(lines[3], "Mean,-2.2,,266,0.40");
        assert_eq!(lines[4], "");
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
