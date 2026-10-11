//! The Positions Test: the app leads the watchmaker through the six
//! positions in Witschi's order, waits for the watch to settle in each,
//! measures it, and then reads the six together as `timegrapher session`
//! does (core `session::evaluate`: the mean rate X, the largest difference
//! D, vertical minus horizontal, and the findings).
//!
//! Each position is a stretch of the one listening session, so the strip,
//! the charts and a saved session keep the positions as any change of
//! position does.

use crate::app::Position;
use crate::theme;
use eframe::egui::{self, RichText, Vec2};
use timegrapher_core::clock::ClockFit;
use timegrapher_core::session::{
    self, evaluate, Limits, Mark, Measurement, Reading, Severity, Tolerance,
};
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Standard {
    Mens,
    Ladies,
    Cosc,
}

impl Standard {
    fn tolerance(self) -> Tolerance {
        Tolerance::named(match self {
            Standard::Mens => "mens",
            Standard::Ladies => "ladies",
            Standard::Cosc => "cosc",
        })
        .unwrap_or_default()
    }
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
    pub standard: Standard,
}

impl Test {
    pub fn new() -> Self {
        Test {
            step: 0,
            start: None,
            spans: Vec::new(),
            readings: None,
            show_results: false,
            standard: Standard::Mens,
        }
    }

    pub fn position(&self) -> Position {
        ORDER[self.step.min(ORDER.len() - 1)]
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
                        "then press Next".to_string(),
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
                        let last = test.next_position().is_none();
                        if ui
                            .add(theme::primary(ui, if last { "Finish" } else { "Next" }))
                            .on_hover_text(if last {
                                "Read the six positions together"
                            } else {
                                "The watch is in the new position: settle it and measure"
                            })
                            .clicked()
                        {
                            action = Some(Action::Next);
                        }
                        if ui
                            .button("Measure Again")
                            .on_hover_text("Measure this position again, from now")
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
                            .button("Start Again")
                            .on_hover_text(
                                "Settle and measure this position again from now, as after \
                                 a knock",
                            )
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

/// The results: each position's rate, amplitude and beat error, marked
/// against the chosen standard, Witschi's characteristic values, and the
/// findings.
pub fn results(ctx: &egui::Context, test: &mut Test) {
    let Some(readings) = &test.readings else {
        return;
    };
    let mut open = test.show_results;
    let mut standard = test.standard;
    egui::Window::new("Positions Test")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .show(ctx, |ui| {
            let pal = theme::pal(ui);
            theme::row(
                ui,
                "Judged Against",
                Some("The tolerances for rate, amplitude and beat error"),
                |ui| {
                    theme::segmented(
                        ui,
                        &mut standard,
                        &[
                            (Standard::Mens, "Men's", "−5 to +15 s/d"),
                            (Standard::Ladies, "Ladies'", "−5 to +25 s/d"),
                            (Standard::Cosc, "COSC", "−4 to +6 s/d"),
                        ],
                    );
                },
            );
            let tol = standard.tolerance();
            let report = evaluate(readings, &tol, &Limits::default());
            ui.add_space(6.0);
            let mark_color = |m: Mark| match m {
                Mark::Within => pal.text,
                Mark::Outside => pal.bad,
                Mark::NotJudged | Mark::Unreliable => pal.text_secondary,
            };
            egui::Grid::new("positions")
                .num_columns(4)
                .spacing(Vec2::new(24.0, 6.0))
                .show(ui, |ui| {
                    for h in ["Position", "Rate", "Amplitude", "Beat Error"] {
                        ui.label(theme::caption(h).color(pal.text_secondary));
                    }
                    ui.end_row();
                    for (r, v) in readings.iter().zip(&report.verdicts) {
                        let m = &r.measurement;
                        ui.label(&r.label);
                        let dash = || "—".to_string();
                        ui.label(
                            RichText::new(
                                m.rate_s_per_day
                                    .map_or_else(dash, |x| format!("{x:+.1} s/d"))
                                    .replace('-', "−"),
                            )
                            .color(mark_color(v.rate)),
                        );
                        ui.label(
                            RichText::new(
                                m.amplitude_deg.map_or_else(dash, |x| format!("{x:.0}°")),
                            )
                            .color(mark_color(v.amplitude)),
                        );
                        ui.label(
                            RichText::new(
                                m.beat_error_unlock_ms
                                    .or(m.beat_error_ms)
                                    .map_or_else(dash, |x| format!("{:.2} ms", x.abs())),
                            )
                            .color(mark_color(v.beat_error)),
                        );
                        ui.end_row();
                    }
                });
            if let Some(s) = report.states.first() {
                ui.add_space(8.0);
                let f = |v: Option<f64>, unit: &str| {
                    v.map_or("—".to_string(), |x| {
                        format!("{x:+.1}{unit}").replace('-', "−")
                    })
                };
                let line = format!(
                    "Mean rate {} · largest difference {} · vertical minus horizontal {} · \
                     amplitude vertical minus horizontal {}",
                    f(s.x, " s/d"),
                    s.d_rate.map_or("—".into(), |x| format!("{x:.1} s/d")),
                    f(s.dvh_rate, " s/d"),
                    f(s.dvh_amplitude, "°"),
                );
                ui.add(egui::Label::new(RichText::new(line).color(pal.text)).wrap())
                    .on_hover_text(
                        "Witschi's characteristic values: X, the mean of the six rates; D, \
                         the largest difference between them; DVH, the vertical positions' \
                         mean minus the horizontal ones'. A large D or DVH points at poise \
                         or a hairspring out of true rather than the rate setting.",
                    );
            }
            ui.add_space(8.0);
            if report.findings.is_empty() {
                ui.label(
                    RichText::new(format!("Nothing outside {} tolerances.", tol.name))
                        .color(pal.text_secondary),
                );
            }
            for f in &report.findings {
                let color = match f.severity {
                    Severity::Fault => pal.bad,
                    Severity::Warning => pal.warn,
                    Severity::Note => pal.text_secondary,
                };
                let title = match f.recording.and_then(|i| readings.get(i)) {
                    Some(r) => format!("{}: {}", r.label, f.title),
                    None => f.title.clone(),
                };
                ui.horizontal(|ui| {
                    theme::dot(ui, color);
                    ui.add(egui::Label::new(RichText::new(title).color(pal.text)).wrap())
                        .on_hover_text(format!("{}\n\n{}", f.evidence, f.advice));
                });
            }
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!(
                    "Each position settled {SETTLE_S:.0} s and was measured for {MEASURE_S:.0} s. \
                     Save the session to keep the positions with the sound; \
                     `timegrapher session` reads it the same way."
                ))
                .small()
                .color(pal.text_tertiary),
            );
        });
    test.show_results = open;
    test.standard = standard;
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
        let report = evaluate(readings, &Tolerance::default(), &Limits::default());
        let d = report.states[0].d_rate.unwrap();
        assert!((d - 9.0).abs() < 1.5, "D {d}");
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
