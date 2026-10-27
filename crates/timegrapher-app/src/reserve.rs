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
    /// While it runs, its hours so far, the newest still filling.
    pub so_far: Vec<power_reserve::Hour>,
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
            so_far: Vec::new(),
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

/// A run's ending in words.
pub fn ending_text(e: Ending) -> &'static str {
    match e {
        Ending::Stopped => "The watch stopped",
        Ending::ByHand => "Stopped by hand while the watch still ran",
        Ending::Limit => "Reached the 80 h limit while the watch still ran",
    }
}

/// The pane's body, in no more height than it shows: before a run one
/// line of what to do (the steps are in its "?"); while it runs the run
/// time and one line of how the beats stand; once ended one line of the
/// result over the hour charts and table, which take the rest of the pane
/// and scroll in it. `card` is the position card's title as drawn, and
/// `watch` the watch the take is kept in (none: Unsorted).
pub fn pane(
    ui: &mut egui::Ui,
    test: Option<&Test>,
    free: Option<(u64, u64)>,
    card: &str,
    watch: &str,
) {
    let pal = theme::pal(ui);
    ui.spacing_mut().item_spacing.y = 6.0;
    let Some(t) = test else {
        let mut line = ui.add(
            egui::Label::new(
                RichText::new(format!(
                    "Wind fully, set the position on the {card} card, then press Start Test."
                ))
                .color(pal.text_secondary),
            )
            .truncate(),
        );
        if let Some((have, need)) = free {
            let room = format!(
                "{} free where the take is kept; a run of up to 80 h writes up to {} of sound.",
                gb(have),
                gb(need)
            );
            if have >= need {
                line = line.on_hover_text(room);
            } else {
                theme::alert_label(
                    ui,
                    theme::Alert::Warning,
                    &format!(
                        "Only {} free where the take is kept: a long run could fill the disk.",
                        gb(have)
                    ),
                    true,
                )
                .on_hover_text(room);
            }
        }
        let _ = line;
        return;
    };
    let pauses = (!t.heard.pauses.is_empty()).then(|| {
        format!(
            " Beats stopped and came back {} time{} on the way.",
            t.heard.pauses.len(),
            if t.heard.pauses.len() == 1 { "" } else { "s" }
        )
    });
    let pause_hint = format!(
        "Silences of {:.0} s or more inside the run, after which the beats came back: a watch \
         near the end can stop and start again with a knock",
        power_reserve::PAUSE_S
    );
    if t.running() {
        // One line, then the hours so far as when it ends.
        let state = match t.heard.silent_s(t.now_s) {
            None => "Waiting for the first beats…".to_string(),
            Some(s) if s >= power_reserve::PAUSE_S => format!(
                "No beat heard for {}; the run ends after {} min with none.",
                crate::strip::fmt_time(s),
                power_reserve::STOP_AFTER_S / 60.0
            ),
            Some(_) => format!("Running, in {}.", t.position.label()),
        };
        let line = format!(
            "{} · {state}{}",
            power_reserve::run_text(t.run_so_far_s()),
            pauses.as_deref().unwrap_or("")
        );
        let mut hover = "The run so far, from the first beat heard to now".to_string();
        if pauses.is_some() {
            hover.push_str(&format!(". {pause_hint}"));
        }
        ui.add(egui::Label::new(RichText::new(line).color(pal.text)).truncate())
            .on_hover_text(hover);
        if !t.so_far.is_empty() {
            hour_area(ui, &t.so_far, true);
        }
        return;
    }
    // Ended: one line of the result.
    let run = power_reserve::run_text(t.results.as_ref().map_or(t.run_so_far_s(), |r| r.run_s));
    let ending = t
        .ending
        .map(|e| format!(" · {}, in {}", ending_text(e), t.position.label()))
        .unwrap_or_default();
    let kept = if t.saving {
        " · Saving the take…".to_string()
    } else if t.saved.is_some() {
        if watch.is_empty() {
            " · Kept in Unsorted".to_string()
        } else {
            format!(" · Kept in {watch}")
        }
    } else {
        String::new()
    };
    let mut hover = "The run, from the first beat heard to the last, in true time".to_string();
    if let Some(p) = &t.saved {
        hover.push_str(&format!(
            ". Kept as \"{}\"{}; every pane reads it as any take",
            p.file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into()),
            if t.keep_sound {
                ""
            } else {
                ", as its analysis only once that is written"
            }
        ));
    }
    if pauses.is_some() {
        hover.push_str(&format!(". {pause_hint}"));
    }
    ui.add(
        egui::Label::new(
            RichText::new(format!(
                "Ran {run}{ending}{kept}.{}",
                pauses.as_deref().unwrap_or("")
            ))
            .color(pal.text),
        )
        .truncate(),
    )
    .on_hover_text(hover);
    if let Some(r) = t.results.as_ref().filter(|r| !r.hours.is_empty()) {
        hour_area(ui, &r.hours, false);
    }
}

/// The hour charts and table, taking the rest of the pane and scrolling
/// in it, their bar drawn at rest, from the top each time the pane is
/// shown; `so_far` while the run goes on, its newest hour still filling.
fn hour_area(ui: &mut egui::Ui, hours: &[power_reserve::Hour], so_far: bool) {
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    let viewport_h = ui.available_height();
    let id = egui::Id::new("reserve run scroll");
    let pass = ui.ctx().cumulative_pass_nr();
    let last = ui.ctx().data_mut(|d| {
        let last = d.get_temp::<u64>(id);
        d.insert_temp(id, pass);
        last
    });
    // Never taller than the room left, however little: egui's least
    // scrolled height of 64 would run the foot past a short pane, out of
    // reach of the wheel.
    let mut area = egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .min_scrolled_height(0.0)
        .max_height(viewport_h.max(0.0))
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded);
    if last.is_none_or(|l| l + 1 < pass) {
        area = area.vertical_scroll_offset(0.0);
    }
    area.show(ui, |ui| {
        // The charts take the height left after the hours' table (or a
        // third of it, for a long table); what doesn't fit scrolls.
        let row_h = ui
            .text_style_height(&egui::TextStyle::Body)
            .max(ui.spacing().interact_size.y)
            + 3.0;
        let gap = ui.spacing().item_spacing.y;
        let room = viewport_h - 8.0 - 3.0 * gap;
        let table_h = ((hours.len() as f32 + 1.0) * row_h + 4.0).min(room / 3.0);
        let chart_h = ((room - table_h - gap) / 2.0).max(CHART_MIN_H);
        // While it runs the charts gain a point as each hour is whole.
        let whole = if so_far {
            hours.iter().take_while(|h| h.span_s >= 3599.0).count()
        } else {
            hours.len()
        };
        if whole > 0 {
            hour_chart(ui, &hours[..whole], chart_h);
            ui.add_space(8.0);
        }
        hour_table(ui, hours);
    });
}

/// The least height of each of the Power Reserve charts, points.
const CHART_MIN_H: f32 = 56.0;

/// The least height the pane's run needs under its toolbar once it has
/// charts: its line and the first chart at its least, the rest scrolling
/// under it; none before then. No more, so the panes above keep theirs.
pub fn least_h(test: Option<&Test>) -> Option<f32> {
    let t = test?;
    let charts = match t.results.as_ref() {
        Some(r) => !r.hours.is_empty(),
        None => t.so_far.iter().any(|h| h.span_s >= 3599.0),
    };
    charts.then_some(22.0 + CHART_MIN_H + 12.0)
}

/// A run stopped by hand after `hours` whole hours, for the tests.
#[cfg(test)]
pub fn finished_run(hours: u32) -> Test {
    let beats: Vec<Beat> = (0..160)
        .map(|i| Beat {
            index: i,
            time: i as f64 * 0.2,
            quality: 1.0,
        })
        .collect();
    let mut t = Test::new(Position::DialUp, true, String::new(), 0.0);
    t.update(&beats, 0.2, 32.0);
    t.stop();
    t.finish(|s| s);
    if let Some(r) = t.results.as_mut() {
        r.hours = (0..hours)
            .map(|h| power_reserve::Hour {
                hour: h,
                span_s: 3600.0,
                rate_s_per_day: Some(6.5),
                amplitude_deg: Some(281.0),
                beat_error_ms: Some(0.4),
                read_s: 3500.0,
            })
            .collect();
    }
    t
}

/// The height a run of `hours` rows reads well in: the toolbar and the
/// run's line, two charts of a readable height, and the table.
pub fn run_h(hours: usize) -> f32 {
    let row_h = 22.0;
    60.0 + 2.0 * 160.0 + 8.0 + (hours as f32 + 1.0) * row_h
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

/// The run hour by hour as a table, the hour in progress read like a
/// finished run's last, part hour.
fn hour_table(ui: &mut egui::Ui, hours: &[power_reserve::Hour]) {
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
            for h in hours {
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
fn hour_chart(ui: &mut egui::Ui, hours: &[power_reserve::Hour], height: f32) {
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
        let pts: Vec<[f64; 2]> = hours
            .iter()
            .filter_map(|h| get(h).map(|v| [x(h), v]))
            .collect();
        // Whole hours from 0 h, an hour at least, so a short run reads
        // 0 h to 1 h rather than the one hour over and over.
        let last = pts.iter().map(|p| p[0]).fold(1.0, f64::max);
        Plot::new(format!("reserve {name}"))
            .height(height)
            .allow_drag(false)
            .allow_zoom(false)
            .allow_scroll(false)
            .include_x(0.0)
            .include_x(last)
            .x_grid_spacer(|g| theme::even_grid(g, 48.0, &HOUR_STEPS))
            // Plus zero: never "-0 h".
            .x_axis_formatter(|m, _| format!("{:.0} h", m.value + 0.0))
            // The point under the pointer in words: hours into the run and
            // the hour's figure.
            .label_formatter(move |name, v| {
                if name.is_empty() {
                    return String::new();
                }
                format!(
                    "{} into the run\n{:.d$}{unit}",
                    crate::strip::fmt_time((v.x * 3600.0).max(0.0)),
                    v.y,
                    d = usize::from(unit != "°")
                )
            })
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

    /// After a run, its two charts take the pane's height and nothing is
    /// cut at its foot: a taller pane gives taller charts, and the hours'
    /// table ends inside the pane.
    #[test]
    fn a_finished_runs_charts_take_the_panes_height() {
        let mut t = Test::new(Position::DialUp, true, String::new(), 0.0);
        t.update(&beats(0.0, 32.0, 0.2), 0.2, 32.0);
        t.stop();
        t.finish(|s| s).unwrap();
        if let Some(r) = t.results.as_mut() {
            r.hours = (0..3)
                .map(|h| power_reserve::Hour {
                    hour: h,
                    span_s: 3600.0,
                    rate_s_per_day: Some(6.5),
                    amplitude_deg: Some(281.0),
                    beat_error_ms: Some(0.4),
                    read_s: 3500.0,
                })
                .collect();
        }
        let ctx = egui::Context::default();
        theme::install(&ctx);
        // Each "0 h" axis label's height, and the foot of the last text.
        let drawn = |h: f32| {
            let mut texts = Vec::new();
            for _ in 0..2 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700.0, h),
                    )),
                    ..Default::default()
                };
                let out = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        pane(ui, Some(&t), None, "Position", "Pepsi");
                    });
                });
                texts = out
                    .shapes
                    .iter()
                    .filter(|c| {
                        c.clip_rect.intersects(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(700.0, h),
                        ))
                    })
                    .filter_map(|c| match &c.shape {
                        egui::Shape::Text(t) => Some((
                            t.galley.text().to_string(),
                            t.galley.rect.translate(t.pos.to_vec2()),
                        )),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
            }
            let zero: Vec<f32> = texts
                .iter()
                .filter(|t| t.0 == "0 h")
                .map(|t| t.1.center().y)
                .collect();
            assert_eq!(zero.len(), 2, "{texts:?}");
            // Nothing drawn starts below the pane: what doesn't fit is in
            // the table, which scrolls.
            assert!(texts.iter().all(|t| t.1.top() < h), "{texts:?}");
            let last = texts.iter().find(|t| t.0 == "2–3 h").map(|t| t.1.bottom());
            (zero[1] - zero[0], last)
        };
        let (short, _) = drawn(380.0);
        let (tall, last) = drawn(760.0);
        // Room enough: the whole table shows.
        assert!(last.is_some_and(|b| b <= 760.0), "{last:?}");
        assert!(tall > short + 120.0, "{short} {tall}");
    }

    /// A finished run in a short pane scrolls, by the wheel, to its
    /// Amplitude chart and its last hour, and opens again at its top; in a
    /// pane with room, its one hour's row shows whole.
    #[test]
    fn a_short_panes_run_scrolls_to_its_last_hour_and_opens_at_its_top() {
        let run = |hours: u32| {
            let mut t = Test::new(Position::DialUp, true, String::new(), 0.0);
            t.update(&beats(0.0, 32.0, 0.2), 0.2, 32.0);
            t.stop();
            t.finish(|s| s).unwrap();
            if let Some(r) = t.results.as_mut() {
                r.hours = (0..hours)
                    .map(|h| power_reserve::Hour {
                        hour: h,
                        span_s: 3600.0,
                        rate_s_per_day: Some(6.5),
                        amplitude_deg: Some(281.0),
                        beat_error_ms: Some(0.4),
                        read_s: 3500.0,
                    })
                    .collect();
            }
            t
        };
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let (w, h) = (600.0, 240.0);
        let pass = |t: Option<&Test>, h: f32, wheel: bool| {
            let at = egui::pos2(w / 2.0, h / 2.0);
            let mut events = vec![egui::Event::PointerMoved(at)];
            if wheel {
                events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -300.0),
                    modifiers: Default::default(),
                });
            }
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(w, h),
                )),
                events,
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(t) = t {
                        pane(ui, Some(t), None, "Position", "Pepsi");
                    }
                });
            });
            out.shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(s) => {
                        let r = s.galley.rect.translate(s.pos.to_vec2());
                        c.clip_rect
                            .contains_rect(r)
                            .then(|| (s.galley.text().to_string(), r))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let t = run(5);
        let shown = |texts: &[(String, egui::Rect)], what: &str| texts.iter().any(|t| t.0 == what);
        // The Rate chart's own scale, "7 s/d", at the top of what scrolls.
        let rate_scale = |texts: &[(String, egui::Rect)]| {
            texts
                .iter()
                .any(|t| t.0.ends_with(" s/d") && !t.0.contains('.'))
        };
        let mut texts = pass(Some(&t), h, false);
        texts = pass(Some(&t), h, false).into_iter().chain(texts).collect();
        assert!(rate_scale(&texts), "the top first: {texts:?}");
        assert!(
            texts
                .iter()
                .any(|t| t.0.starts_with("Ran ") && t.0.contains(" s · ")),
            "one line of the result: {texts:?}"
        );
        assert!(!shown(&texts, "4–5 h"), "{texts:?}");
        for _ in 0..12 {
            texts = pass(Some(&t), h, true);
        }
        assert!(
            shown(&texts, "4–5 h"),
            "the last hour, scrolled to: {texts:?}"
        );
        // A pane with under 64 points left under its line: the last hour
        // still scrolls whole into it.
        let low = 84.0;
        let mut texts = Vec::new();
        for _ in 0..30 {
            texts = pass(Some(&t), low, true);
        }
        let row = texts.iter().find(|t| t.0 == "4–5 h");
        assert!(
            row.is_some_and(|r| r.1.bottom() <= low),
            "{row:?} {texts:?}"
        );
        for _ in 0..60 {
            pass(Some(&t), h, false);
        }
        // The wheel's glide spent, away a pass and back: at the top again.
        for _ in 0..60 {
            pass(Some(&t), h, false);
        }
        assert!(!rate_scale(&pass(Some(&t), h, false)));
        pass(None, h, false);
        pass(Some(&t), h, false);
        let texts = pass(Some(&t), h, false);
        assert!(rate_scale(&texts), "{texts:?}");
        // Room for it all: the one hour's row whole, nothing to scroll.
        let one = run(1);
        pass(Some(&one), 520.0, false);
        let texts = pass(Some(&one), 520.0, false);
        let row = texts
            .iter()
            .find(|t| t.0 == "0–1 h")
            .unwrap_or_else(|| panic!("{texts:?}"));
        assert!(row.1.bottom() <= 520.0, "{row:?}");
        assert!(shown(&texts, "280°") || shown(&texts, "281°"), "{texts:?}");
    }

    /// The pane holds no more height than it shows: one line before a run,
    /// and one line while it runs until its first hours are measured.
    #[test]
    fn the_pane_holds_no_more_than_it_shows() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let height = |t: Option<&Test>, free: Option<(u64, u64)>| {
            let mut h = 0.0;
            let mut texts = Vec::new();
            for _ in 0..2 {
                let out = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let top = ui.cursor().top();
                        pane(ui, t, free, "Position", "");
                        h = ui.cursor().top() - top - ui.spacing().item_spacing.y;
                    });
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
            (h, texts)
        };
        // Ready: the toolbar's 28 points and one line in 64.
        let (h, texts) = height(None, Some((500_000_000_000, 100_000_000_000)));
        assert!(h <= 64.0 - 28.0, "{h}");
        assert_eq!(
            texts,
            ["Wind fully, set the position on the Position card, then press Start Test."]
        );
        // Running, before its first hours are measured: one line of run
        // time and state, with the toolbar, in 64.
        let mut t = Test::new(Position::DialUp, true, String::new(), 0.0);
        t.update(&beats(0.0, 32.0, 0.2), 0.2, 32.0);
        assert!(t.running());
        let (h, texts) = height(Some(&t), None);
        assert!(h <= 64.0 - 28.0, "{h}");
        assert_eq!(texts.len(), 1, "{texts:?}");
        assert!(texts[0].starts_with("32 s · "), "{texts:?}");
        // Ended with no hours yet: its one line.
        t.stop();
        let (h, texts) = height(Some(&t), None);
        assert!(h <= 64.0 - 28.0, "{h} {texts:?}");
        assert!(texts[0].starts_with("Ran "), "{texts:?}");
    }

    /// While a run goes on, its hours show as they come: a row for each,
    /// the hour in progress read like a finished run's last, and a chart
    /// point for each whole hour only; before its first whole hour, the
    /// table alone.
    #[test]
    fn a_running_test_shows_its_hours_so_far() {
        let hour = |h: u32, span_s: f64| power_reserve::Hour {
            hour: h,
            span_s,
            rate_s_per_day: Some(6.5),
            amplitude_deg: Some(281.0),
            beat_error_ms: Some(0.4),
            read_s: span_s - 100.0,
        };
        let mut t = Test::new(Position::DialUp, true, String::new(), 0.0);
        t.update(&beats(0.0, 32.0, 0.2), 0.2, 32.0);
        assert!(t.running());
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let texts = |t: &Test| {
            let mut texts = Vec::new();
            for _ in 0..2 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700.0, 700.0),
                    )),
                    ..Default::default()
                };
                let out = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        pane(ui, Some(t), None, "Position", "Pepsi");
                    });
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
            texts
        };
        // The first hour filling: its row, no chart yet.
        t.so_far = vec![hour(0, 1588.0)];
        let shown = texts(&t);
        assert!(shown.iter().any(|s| s == "0 h + 26:28"), "{shown:?}");
        assert!(!shown.iter().any(|s| s == "0 h"), "no chart: {shown:?}");
        // Two whole hours and the third filling: three rows, and the
        // charts reach the second hour's point only.
        t.so_far = vec![hour(0, 3600.0), hour(1, 3600.0), hour(2, 1588.0)];
        let shown = texts(&t);
        for row in ["0–1 h", "1–2 h", "2 h + 26:28"] {
            assert!(shown.iter().any(|s| s == row), "{row}: {shown:?}");
        }
        assert_eq!(shown.iter().filter(|s| *s == "0 h").count(), 2, "{shown:?}");
        assert!(!shown.iter().any(|s| s == "3 h"), "{shown:?}");
        assert!(shown[0].starts_with("32 s · "), "{shown:?}");
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
                egui::CentralPanel::default().show(ctx, |ui| hour_chart(ui, &r.hours, 110.0));
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
