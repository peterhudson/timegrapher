//! The Power Reserve Test: the watch wound fully, set on the stand in one
//! position, and listened to until it stops. The app finds the stop
//! (core [`power_reserve`]: the last beat heard before ten minutes with
//! none), keeps the take in the watch's history with the run's results
//! beside it in `power-reserve.json`, and shows the run time. No verdict
//! is given.
//!
//! While it runs the computer is kept awake, the sound is kept, and the
//! position can't change and the listening can't pause: a run is one
//! position from wind to stop.

use crate::app::Position;
use crate::theme;
use eframe::egui::{self, RichText};
use std::path::PathBuf;
use timegrapher_core::beats::Beat;
use timegrapher_core::power_reserve::{self, Ending, Heard, Results};

/// Bytes of sound written an hour at `sample_rate` and `bits`, one
/// channel, as the recorder keeps it.
pub fn bytes_per_hour(sample_rate: u32, bits: u16) -> u64 {
    u64::from(sample_rate) * u64::from(bits.div_ceil(8)) * 3600
}

/// The sound a run of [`power_reserve::LIMIT_S`] could write at most.
pub fn bytes_needed(sample_rate: u32, bits: u16) -> u64 {
    bytes_per_hour(sample_rate, bits) * (power_reserve::LIMIT_S / 3600.0) as u64
}

/// Bytes free to this user on the disk holding `dir`.
#[cfg(unix)]
pub fn free_bytes(dir: &std::path::Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is a valid C string and `st` a place for the answer.
    if unsafe { libc::statvfs(path.as_ptr(), &mut st) } != 0 {
        return None;
    }
    #[allow(clippy::unnecessary_cast, clippy::useless_conversion)]
    Some(u64::from(st.f_bavail as u64) * u64::from(st.f_frsize as u64))
}

/// Bytes free to this user on the disk holding `dir`.
#[cfg(windows)]
pub fn free_bytes(dir: &std::path::Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
    let mut free = 0u64;
    // SAFETY: `wide` ends in a nul; the other answers aren't asked for.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

/// A size in GB, one decimal: "17.3 GB".
pub fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}

/// Beats a run keeps, from its first to its limit at 43,200 bph, the
/// fastest beat rate, so its start and its hours are read from every beat
/// of it, not only the day the Bench keeps (about 80 MB at most).
pub const RUN_BEATS_KEPT: usize =
    (power_reserve::LIMIT_S as usize + power_reserve::STOP_AFTER_S as usize) * 12;

/// A test under way or finished.
#[derive(Debug, Clone)]
pub struct Test {
    pub position: Position,
    /// Keep the sound in the take, or keep its analysis only.
    pub keep_sound: bool,
    /// When it started, UTC, RFC 3339.
    pub started_utc: String,
    /// The session's audio seconds when it started: beats before don't
    /// count.
    pub began_s: f64,
    /// The beats heard since.
    pub heard: Heard,
    /// The session's audio seconds when last looked at.
    pub now_s: f64,
    /// How it ended, once it has.
    pub ending: Option<Ending>,
    /// Its results, once it has ended and the stop was found.
    pub results: Option<Results>,
    /// The take it was saved as.
    pub saved: Option<PathBuf>,
    /// Its take is being written.
    pub saving: bool,
}

impl Test {
    pub fn new(position: Position, keep_sound: bool, started_utc: String, began_s: f64) -> Test {
        Test {
            position,
            keep_sound,
            started_utc,
            began_s,
            heard: power_reserve::heard(&[], 1.0),
            now_s: began_s,
            ending: None,
            results: None,
            saved: None,
            saving: false,
        }
    }

    pub fn running(&self) -> bool {
        self.ending.is_none()
    }

    /// Read the session's `beats` up to `now_s` at `beat_s` seconds a
    /// beat: how it ended, when it just has (the watch stopped, or the
    /// time limit).
    pub fn update(&mut self, beats: &[Beat], beat_s: f64, now_s: f64) -> Option<Ending> {
        if !self.running() {
            return None;
        }
        let from = beats.partition_point(|b| b.time < self.began_s);
        self.heard = power_reserve::heard(&beats[from..], beat_s);
        self.now_s = now_s;
        let ending = if self.heard.stopped(now_s) {
            Ending::Stopped
        } else if now_s - self.began_s >= power_reserve::LIMIT_S {
            Ending::Limit
        } else {
            return None;
        };
        self.ending = Some(ending);
        Some(ending)
    }

    /// End it by hand.
    pub fn stop(&mut self) {
        if self.running() {
            self.ending = Some(Ending::ByHand);
        }
    }

    /// Its results, with `to_true` mapping audio seconds to true seconds:
    /// `None` until it ends, or when no beat was heard.
    pub fn finish(&mut self, to_true: impl Fn(f64) -> f64) -> Option<&Results> {
        let ending = self.ending?;
        self.results = Results::of(
            &self.heard,
            ending,
            &self.started_utc,
            self.position.code(),
            self.keep_sound,
            to_true,
        );
        self.results.as_ref()
    }

    /// Seconds from the first beat heard to now, or to the last beat once
    /// ended, in audio seconds.
    pub fn run_so_far_s(&self) -> f64 {
        match (self.heard.first_s, self.heard.last_s) {
            (Some(a), Some(b)) if !self.running() => b - a,
            (Some(a), _) => (self.now_s - a).max(0.0),
            _ => 0.0,
        }
    }
}

/// What the pane's body asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Stop,
}

/// A run's ending in words.
pub fn ending_text(e: Ending) -> &'static str {
    match e {
        Ending::Stopped => "The watch stopped",
        Ending::ByHand => "Stopped by hand while the watch still ran",
        Ending::Limit => "Reached the 80 h limit while the watch still ran",
    }
}

/// The pane's body: the run time, how the beats stand, and at the end the
/// result. `card` is the position card's title as drawn, for the steps.
pub fn pane(
    ui: &mut egui::Ui,
    test: Option<&Test>,
    free: Option<(u64, u64)>,
    card: &str,
) -> Option<Action> {
    let pal = theme::pal(ui);
    let mut action = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let Some(t) = test else {
                let stop = power_reserve::STOP_AFTER_S / 60.0;
                let steps = [
                    "1.  Wind the watch fully and set it to a minute marker.".to_string(),
                    format!("2.  Put it on the stand in the position on the {card} card."),
                    "3.  Press Start Test.".to_string(),
                ];
                let after = [
                    "•  The app listens until the watch stops.".to_string(),
                    format!("•  The run ends at the last beat heard before {stop} min with none."),
                    "•  The take is kept in the watch's history with its run time.".to_string(),
                    "•  While it runs the computer is kept awake and the sound is kept; the \
                     position can't change and the listening can't pause."
                        .to_string(),
                ];
                for l in steps.iter().chain(after.iter()) {
                    ui.add(egui::Label::new(RichText::new(l).color(pal.text_secondary)).wrap());
                }
                if let Some((have, need)) = free {
                    let line = if have >= need {
                        format!(
                            "{} free where the take is kept; a run of up to 80 h writes up to {} \
                             of sound.",
                            gb(have),
                            gb(need)
                        )
                    } else {
                        format!(
                            "Only {} free where the take is kept; a run of up to 80 h writes up \
                             to {} of sound, so a long run could fill the disk.",
                            gb(have),
                            gb(need)
                        )
                    };
                    if have >= need {
                        ui.label(RichText::new(line).small().color(pal.text_tertiary));
                    } else {
                        theme::alert_label(ui, theme::Alert::Warning, &line, true);
                    }
                }
                return;
            };
            let run = power_reserve::run_text(t.run_so_far_s());
            ui.label(
                RichText::new(match &t.results {
                    Some(r) => power_reserve::run_text(r.run_s),
                    None => run,
                })
                .size(34.0)
                .strong()
                .color(pal.text),
            )
            .on_hover_text(
                "From the first beat heard to the last (or to now while it runs), in true time \
                 once it has ended",
            );
            let state = if t.running() {
                match t.heard.silent_s(t.now_s) {
                    None => "Waiting for the first beats…".to_string(),
                    Some(s) if s >= power_reserve::PAUSE_S => format!(
                        "No beat heard for {}; the run ends after {} min with none.",
                        crate::strip::fmt_time(s),
                        power_reserve::STOP_AFTER_S / 60.0
                    ),
                    Some(_) => format!("Running, in {}.", t.position.label()),
                }
            } else {
                match t.ending {
                    Some(e) => format!("{}, in {}.", ending_text(e), t.position.label()),
                    None => String::new(),
                }
            };
            ui.label(RichText::new(state).color(pal.text_secondary));
            if !t.heard.pauses.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "Beats stopped and came back {} time{} on the way.",
                        t.heard.pauses.len(),
                        if t.heard.pauses.len() == 1 { "" } else { "s" }
                    ))
                    .color(pal.text_secondary),
                )
                .on_hover_text(format!(
                    "Silences of {:.0} s or more inside the run, after which the beats came \
                     back: a watch near the end can stop and start again with a knock",
                    power_reserve::PAUSE_S
                ));
            }
            if t.running() {
                ui.add_space(6.0);
                if ui
                    .add(theme::primary(ui, "Stop Test"))
                    .on_hover_text(
                        "End the run now and keep the take: its run time then says it was \
                         stopped by hand",
                    )
                    .clicked()
                {
                    action = Some(Action::Stop);
                }
                ui.label(
                    RichText::new(
                        "A closed lid or Sleep from a menu still stops the computer, and the \
                         run with it; a laptop is best left on mains power.",
                    )
                    .small()
                    .color(pal.text_tertiary),
                );
            } else if t.saving {
                ui.label(
                    RichText::new("Saving the test's take…")
                        .small()
                        .color(pal.text_secondary),
                );
            } else if let Some(p) = &t.saved {
                ui.label(
                    RichText::new(format!(
                        "Kept as \"{}\"{}. Every pane reads it as any take.",
                        p.file_name()
                            .map_or_else(String::new, |n| n.to_string_lossy().into()),
                        if t.keep_sound {
                            ""
                        } else {
                            ", as its analysis only once that is written"
                        }
                    ))
                    .small()
                    .color(pal.text_tertiary),
                );
            }
            if let Some(r) = t.results.as_ref().filter(|r| !r.hours.is_empty()) {
                ui.add_space(8.0);
                hour_chart(ui, r);
                ui.add_space(8.0);
                hour_table(ui, r);
            }
        });
    action
}

/// A value or a dash.
fn cell(v: Option<f64>, d: usize, unit: &str) -> String {
    v.map_or_else(|| "–".to_string(), |x| format!("{x:.d$}{unit}"))
}

/// The hour column's words: "0–1 h", and the last part hour's length.
fn hour_label(h: &power_reserve::Hour) -> String {
    if h.span_s >= 3599.0 {
        format!("{}–{} h", h.hour, h.hour + 1)
    } else {
        format!("{} h + {}", h.hour, crate::strip::fmt_time(h.span_s))
    }
}

/// The run hour by hour as a table.
fn hour_table(ui: &mut egui::Ui, r: &Results) {
    let pal = theme::pal(ui);
    egui::Grid::new("reserve hours")
        .striped(true)
        .spacing([18.0, 3.0])
        .show(ui, |ui| {
            for (head, hint) in [
                ("Hours", "Hours since the first beat heard"),
                (
                    "Rate",
                    "The hour's rate, s/d, as `timegrapher session` reads a stretch",
                ),
                ("Amplitude", "The hour's median amplitude"),
                (
                    "Beat Error",
                    "The hour's beat error from the unlock, ms (its size)",
                ),
                ("Read", "How much of the hour its whole 10 s readings cover"),
            ] {
                ui.label(RichText::new(head).strong().color(pal.text_secondary))
                    .on_hover_text(hint);
            }
            ui.end_row();
            for h in &r.hours {
                ui.label(hour_label(h));
                ui.label(cell(h.rate_s_per_day, 1, " s/d"));
                ui.label(cell(h.amplitude_deg, 0, "°"));
                ui.label(cell(h.beat_error_ms.map(f64::abs), 1, " ms"));
                ui.label(format!("{:.0}%", 100.0 * h.read_s / h.span_s.max(1.0)));
                ui.end_row();
            }
        });
}

/// The steps, in hours, the Power Reserve charts' hours go up in.
const HOUR_STEPS: [f64; 7] = [1.0, 2.0, 3.0, 6.0, 12.0, 24.0, 48.0];

/// Rate and amplitude against hours since the first beat, one above the
/// other on the same hours.
fn hour_chart(ui: &mut egui::Ui, r: &Results) {
    use egui_plot::{Line, Plot, PlotPoints};
    let pal = theme::pal(ui);
    let x = |h: &power_reserve::Hour| f64::from(h.hour) + h.span_s / 7200.0;
    for (name, colour, unit, get) in [
        (
            "rate",
            pal.trace_rate,
            " s/d",
            (|h: &power_reserve::Hour| h.rate_s_per_day) as fn(&power_reserve::Hour) -> Option<f64>,
        ),
        ("amplitude", pal.trace_amplitude, "°", |h| h.amplitude_deg),
    ] {
        let pts: Vec<[f64; 2]> = r
            .hours
            .iter()
            .filter_map(|h| get(h).map(|v| [x(h), v]))
            .collect();
        // Whole hours from 0 h, an hour at least, so a short run reads
        // 0 h to 1 h rather than the one hour over and over.
        let last = pts.iter().map(|p| p[0]).fold(1.0, f64::max);
        Plot::new(format!("reserve {name}"))
            .height(110.0)
            .allow_drag(false)
            .allow_zoom(false)
            .allow_scroll(false)
            .include_x(0.0)
            .include_x(last)
            .x_grid_spacer(|g| theme::even_grid(g, 48.0, &HOUR_STEPS))
            // Plus zero: never "-0 h".
            .x_axis_formatter(|m, _| format!("{:.0} h", m.value + 0.0))
            .y_grid_spacer(|g| theme::even_grid(g, 24.0, &[]))
            .y_axis_formatter(move |m, _| {
                let places = if m.step_size < 1.0 { 1 } else { 0 };
                format!("{:.*}{unit}", places, m.value)
            })
            .show(ui, |p| {
                p.line(
                    Line::new(name, PlotPoints::from(pts))
                        .color(colour)
                        .width(1.5_f32),
                );
            });
    }
}

/// Save the run as a CSV file, in a folder the user picks from `dir`:
/// what the app said.
pub fn save_csv(text: &str, dir: &std::path::Path) -> Option<String> {
    let stamp: String = timegrapher_core::recorder::utc(std::time::SystemTime::now())
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'T')
        .take(15)
        .collect();
    let mut d = rfd::FileDialog::new()
        .set_title("Save the Power Reserve Test as CSV")
        .set_file_name(format!("power-reserve-{stamp}Z.csv"))
        .add_filter("CSV", &["csv"]);
    if dir.is_dir() {
        d = d.set_directory(dir);
    }
    let path = d.save_file()?;
    Some(match std::fs::write(&path, text) {
        Ok(()) => format!("Saved the Power Reserve Test to {}", path.display()),
        Err(e) => format!(
            "Couldn't save the Power Reserve Test to {}: {e}",
            path.display()
        ),
    })
}

/// The run as text with `sep` between columns, for Copy and a CSV file:
/// the run, then the hours.
pub fn results_text(r: &Results, watch: &str, sep: char) -> String {
    let mut out = String::new();
    let line = |cells: &[String]| {
        cells
            .iter()
            .map(|c| {
                if sep == ',' && c.contains(',') {
                    format!("\"{c}\"")
                } else {
                    c.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(&sep.to_string())
            + "\n"
    };
    out += &line(&["Watch".into(), watch.into()]);
    out += &line(&["Started (UTC)".into(), r.started_utc.clone()]);
    out += &line(&["Position".into(), r.position.clone()]);
    out += &line(&["Power reserve".into(), power_reserve::run_text(r.run_s)]);
    out += &line(&["Ended".into(), ending_text(r.ending).into()]);
    out += "\n";
    out += &line(&[
        "Hour".into(),
        "Length (s)".into(),
        "Rate (s/d)".into(),
        "Amplitude (deg)".into(),
        "Beat error (ms)".into(),
        "Read (s)".into(),
    ]);
    let num = |v: Option<f64>, d: usize| v.map_or_else(String::new, |x| format!("{x:.d$}"));
    for h in &r.hours {
        out += &line(&[
            h.hour.to_string(),
            format!("{:.0}", h.span_s),
            num(h.rate_s_per_day, 1),
            num(h.amplitude_deg, 0),
            num(h.beat_error_ms.map(f64::abs), 2),
            format!("{:.0}", h.read_s),
        ]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beats(from: f64, to: f64, step: f64) -> Vec<Beat> {
        let n = ((to - from) / step).round() as i64;
        (0..n)
            .map(|i| Beat {
                index: i,
                time: from + i as f64 * step,
                quality: 1.0,
            })
            .collect()
    }

    #[test]
    fn a_run_ends_ten_minutes_after_the_last_beat_and_keeps_its_results() {
        let step = 0.125;
        // Beats before the test started don't count.
        let mut b = beats(0.0, 50.0, step);
        b.extend(beats(100.0, 4000.0, step));
        let mut t = Test::new(
            Position::CrownLeft,
            false,
            "2026-10-22T09:15:00Z".into(),
            90.0,
        );
        assert_eq!(t.update(&b, step, 4100.0), None);
        assert!(t.running());
        let last = 4000.0 - step;
        assert_eq!(t.update(&b, step, last + 600.0), Some(Ending::Stopped));
        assert!(!t.running());
        assert!((t.run_so_far_s() - (last - 100.0)).abs() < 1e-9);
        let r = t.finish(|s| s).unwrap();
        assert_eq!(r.position, "CL");
        assert!(!r.sound_kept);
        assert!((r.first_beat_s - 100.0).abs() < 1e-9);
        // Nothing moves once it has ended.
        assert_eq!(t.update(&b, step, 1e6), None);
    }

    #[test]
    fn stopped_by_hand_or_at_the_limit() {
        let step = 0.2;
        let b = beats(0.0, 1000.0, step);
        let mut t = Test::new(Position::DialUp, true, String::new(), 0.0);
        t.update(&b, step, 1000.0);
        t.stop();
        assert_eq!(t.finish(|s| s).unwrap().ending, Ending::ByHand);
        let mut t = Test::new(Position::DialUp, true, String::new(), 0.0);
        let far = beats(power_reserve::LIMIT_S - 10.0, power_reserve::LIMIT_S, step);
        let all: Vec<Beat> = b.into_iter().chain(far).collect();
        assert_eq!(
            t.update(&all, step, power_reserve::LIMIT_S),
            Some(Ending::Limit)
        );
        assert_eq!(gb(bytes_needed(48_000, 16)), "27.6 GB");
        assert_eq!(bytes_per_hour(48_000, 24), 48_000 * 3 * 3600);
        assert!(free_bytes(&std::env::temp_dir()).is_some_and(|b| b > 0));
    }

    #[test]
    fn the_results_copy_as_a_table() {
        let mut t = Test::new(Position::DialUp, true, "2026-10-22T16:15:00Z".into(), 0.0);
        t.update(&beats(0.0, 100.0, 0.2), 0.2, 100.0);
        t.stop();
        let mut r = t.finish(|s| s).unwrap().clone();
        r.hours = vec![power_reserve::Hour {
            hour: 0,
            span_s: 99.8,
            rate_s_per_day: Some(4.25),
            amplitude_deg: Some(281.4),
            beat_error_ms: Some(-0.4),
            read_s: 90.0,
        }];
        let text = results_text(&r, "Pepsi, 1972", ',');
        assert!(text.starts_with("Watch,\"Pepsi, 1972\"\n"), "{text}");
        assert!(
            text.contains("Ended,Stopped by hand while the watch still ran"),
            "{text}"
        );
        assert!(
            // Beat error as its size, as every text gives it.
            text.ends_with("0,100,4.2,281,0.40,90\n") || text.ends_with("0,100,4.3,281,0.40,90\n"),
            "{text}"
        );
        assert_eq!(hour_label(&r.hours[0]), "0 h + 1:40");
    }

    /// A run of half a minute charts 0 h to 1 h, each hour named once,
    /// none "-0 h".
    #[test]
    fn a_short_runs_chart_names_each_hour_once() {
        let mut t = Test::new(Position::DialUp, true, String::new(), 0.0);
        t.update(&beats(0.0, 32.0, 0.2), 0.2, 32.0);
        t.stop();
        let mut r = t.finish(|s| s).unwrap().clone();
        r.hours = vec![power_reserve::Hour {
            hour: 0,
            span_s: 32.0,
            rate_s_per_day: Some(7.0),
            amplitude_deg: Some(281.0),
            beat_error_ms: Some(0.4),
            read_s: 30.0,
        }];
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut texts = Vec::new();
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| hour_chart(ui, &r));
            });
            texts = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>();
        }
        let hours: Vec<_> = texts.iter().filter(|t| t.ends_with(" h")).collect();
        assert!(!hours.is_empty(), "{texts:?}");
        assert!(texts.iter().all(|t| !t.starts_with("-0")), "{texts:?}");
        // Two charts, each naming 0 h and 1 h once.
        for h in ["0 h", "1 h"] {
            assert_eq!(hours.iter().filter(|t| **t == h).count(), 2, "{texts:?}");
        }
        assert_eq!(hours.len(), 4, "{texts:?}");
    }
}
