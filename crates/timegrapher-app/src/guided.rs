//! The Positions Test: the app leads the watchmaker through the six
//! positions in Witschi's order, waits for the watch to settle in each,
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

/// Seconds the watch is left to settle after it is turned, and then
/// measured. Witschi asks for at least 40 s of measurement; a minute
/// brings the rate's ± near half a second a day.
pub const SETTLE_S: f64 = 20.0;
pub const MEASURE_S: f64 = 60.0;

/// Witschi's order: the two horizontals, then crown down, left, up, right.
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
    /// The measured stretch of each position done so far.
    spans: Vec<(Position, f64, f64)>,
    /// The six readings, once the last position is measured.
    pub readings: Option<Vec<Reading>>,
    pub show_results: bool,
}

impl Test {
    pub fn new() -> Self {
        Test {
            step: 0,
            start: None,
            spans: Vec::new(),
            readings: None,
            show_results: false,
        }
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
        self.step += 1;
        self.start = Some(now);
        self.next_or_none()
    }

    fn next_or_none(&self) -> Option<Position> {
        ORDER.get(self.step).copied()
    }

    /// Measure this position again from `now`.
    pub fn redo(&mut self, now: f64) {
        self.spans.truncate(self.step);
        self.start = Some(now);
    }

    #[cfg(test)]
    fn measured(&self) -> usize {
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
                    wind_h: None,
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
                    format!(
                        "{} ({} of {})",
                        test.position().name(),
                        test.step + 1,
                        ORDER.len()
                    ),
                    format!("Letting the watch settle, {left:.0} s"),
                    ((SETTLE_S - left) / (SETTLE_S + MEASURE_S)) as f32,
                ),
                Phase::Measuring(done) => (
                    format!(
                        "{} ({} of {})",
                        test.position().name(),
                        test.step + 1,
                        ORDER.len()
                    ),
                    format!("Measuring, {done:.0} of {MEASURE_S:.0} s"),
                    ((SETTLE_S + done) / (SETTLE_S + MEASURE_S)) as f32,
                ),
                Phase::Turn => match test.next_position() {
                    Some(p) => (
                        format!("Turn the watch to {}", p.name()),
                        format!("then press Start {} Measurement", p.code()),
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
                            .button(format!("Measure {} Again", test.position().code()))
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
                            .button(format!("Measure {} Again", test.position().code()))
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
    /// The largest difference between the rates, s/d (Witschi's D).
    pub largest_difference: Option<f64>,
    /// The vertical positions' mean rate less the horizontal ones', s/d
    /// (Witschi's DVH).
    pub dvh_rate: Option<f64>,
    /// The same for amplitude, degrees.
    pub dvh_amplitude: Option<f64>,
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
                    r.position.label(),
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
    }
}

/// The results: a table of each position's rate, its difference from the
/// mean, amplitude and beat error, with the means, as `timegrapher
/// session` reads them. No verdict: the figures are for the watchmaker to
/// judge.
pub fn results(ctx: &egui::Context, test: &mut Test) {
    let Some(readings) = &test.readings else {
        return;
    };
    let mut open = test.show_results;
    let t = table(readings);
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
            .on_hover_text("Witschi's D: the fastest position's rate less the slowest's");
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
                "Witschi's DVH: the vertical positions' mean less the horizontal ones', \
                 for rate and for amplitude",
            );
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
        let report = session::evaluate(readings, &Default::default(), &Default::default());
        let d = tbl.largest_difference.unwrap();
        assert!((d - report.states[0].d_rate.unwrap()).abs() < 1e-9);
        assert!((tbl.mean_rate.unwrap() - report.states[0].x.unwrap()).abs() < 1e-9);
        let dvh = tbl.dvh_rate.unwrap();
        assert!((dvh - report.states[0].dvh_rate.unwrap()).abs() < 1e-9);
        let dvh_a = tbl.dvh_amplitude.unwrap();
        assert!((dvh_a - report.states[0].dvh_amplitude.unwrap()).abs() < 1e-9);
        assert!((d - 9.0).abs() < 1.5, "D {d}");
        assert_eq!(tbl.rows[4].0, "Crown Up (3H)");
        let spread: f64 = tbl.rows.iter().filter_map(|r| r.2).sum();
        assert!(spread.abs() < 1e-9, "differences from the mean sum to zero");
    }

    #[test]
    fn the_test_settles_measures_and_waits_for_each_turn() {
        let mut t = Test::new();
        assert_eq!(t.tick(100.0), Phase::Settling(SETTLE_S));
        assert_eq!(t.tick(100.0 + SETTLE_S + 10.0), Phase::Measuring(10.0));
        let end = 100.0 + SETTLE_S + MEASURE_S;
        assert_eq!(t.tick(end), Phase::Turn);
        assert_eq!(t.measured(), 1);
        assert_eq!(t.next_position(), Some(Position::DialDown));
        // Measured again: the first stretch is dropped and made afresh.
        t.redo(end);
        assert_eq!(t.measured(), 0);
        assert_eq!(t.tick(end + SETTLE_S + MEASURE_S), Phase::Turn);
        assert_eq!(t.measured(), 1);
        for (i, &p) in ORDER.iter().enumerate().skip(1) {
            let now = 1000.0 * i as f64;
            assert_eq!(t.advance(now), Some(p));
            assert_eq!(t.tick(now + SETTLE_S + MEASURE_S), Phase::Turn);
        }
        assert_eq!(t.measured(), 6);
        assert_eq!(t.next_position(), None);
    }
}
