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
use crate::strength::{self, Grid};
use crate::theme;
use eframe::egui::{self, RichText};
use std::path::PathBuf;
use timegrapher_core::beats::Beat;
use timegrapher_core::chart_scale::axis_number;
use timegrapher_core::confidence::Figure;
use timegrapher_core::figure_text::{base_digits, figure, fixed_text, true_minus};
use timegrapher_core::library::length_text;
use timegrapher_core::power_reserve::{self, Ending, Heard, Results};
use timegrapher_core::recorder::STORED_BITS;
use timegrapher_core::session::{beat_error_source_label, BeatErrorSource};

/// Bytes of sound written an hour at `sample_rate`, one channel, as the
/// recorder keeps it: at [`STORED_BITS`] whatever the microphone gives.
pub fn bytes_per_hour(sample_rate: u32) -> u64 {
    u64::from(sample_rate) * u64::from(STORED_BITS.div_ceil(8)) * 3600
}

/// The sound a run of [`power_reserve::LIMIT_S`] could write at most.
pub fn bytes_needed(sample_rate: u32) -> u64 {
    bytes_per_hour(sample_rate) * (power_reserve::LIMIT_S / 3600.0) as u64
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
    /// Its results not yet written into its take after its recording
    /// failed (the disk full, say): the take, and when the write last
    /// failed, tried again a minute later (#1380).
    pub unwritten: Option<(PathBuf, std::time::Instant)>,
    /// The beat rate it last ran at, kept once an automatic rate lets go
    /// in the silence after the watch stops, so its hours are still read
    /// at it (#1394).
    pub bph: Option<u32>,
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
            unwritten: None,
            bph: None,
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
        let ending = self.heard.ending(now_s, self.began_s)?;
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

/// The Power Reserve pane: before a run, one line of what to do; then
/// the run in a title line (#1528), a grey chip when it is read from a
/// long take, and the hour charts and table, which take the rest of the
/// pane and scroll in it, with the Info panel ("i") beside them when
/// `info`. `card` is the position card's title as drawn, `watch` the
/// watch the take is kept in (none: Unsorted), and `ready` whether a run
/// is set up, waiting for Start Test on the Listen card.
pub fn pane(
    ui: &mut egui::Ui,
    test: Option<&Test>,
    free: Option<(u64, u64)>,
    (card, watch): (&str, &str),
    ready: bool,
    info: bool,
) {
    ui.spacing_mut().item_spacing.y = 6.0;
    let Some(t) = test else {
        before_line(ui, free, card, ready);
        return;
    };
    let pal = theme::pal(ui);
    // One line, its height fixed, so nothing under it moves as it changes.
    let line = run_title(t);
    ui.add(egui::Label::new(theme::caption(&line).color(pal.text)).truncate());
    if let Some(chip) = t.results.as_ref().and_then(|r| r.source.chip()) {
        let full = t.results.as_ref().and_then(|r| r.source.text());
        let why: Vec<String> = full.map(|w| format!("{w}.")).into_iter().collect();
        theme::chip(ui, chip, chip, &why, ui.available_width());
    }
    let hours: &[power_reserve::Hour] = if t.running() {
        &t.so_far
    } else {
        t.results.as_ref().map_or(&[], |r| &r.hours)
    };
    theme::with_info(
        ui,
        info,
        egui::Id::new("reserve info"),
        |ui, h| info_panel(ui, &info_lines(t, watch, hours), h),
        |ui| {
            if !hours.is_empty() {
                hour_area(ui, hours, t.running());
            }
        },
    );
}

/// The run's title line (#1528): while it runs, the run so far and how
/// the beats stand ("4 h 12 min · Running"); once it ends, core's
/// [`power_reserve::title`] ("Ran 9 h 46 min · stopped").
fn run_title(t: &Test) -> String {
    let restarts = t.heard.pauses.len();
    if t.running() {
        let state = match t.heard.silent_s(t.now_s) {
            None => "Waiting for the first beats".to_string(),
            Some(s) if s >= power_reserve::PAUSE_S => format!("No beat for {}", length_text(s)),
            Some(_) => "Running".to_string(),
        };
        let mut line = format!(
            "{} \u{b7} {state}",
            power_reserve::run_text(t.run_so_far_s())
        );
        if restarts > 0 {
            line += &format!(" \u{b7} {}", power_reserve::restarted_text(restarts));
        }
        return line;
    }
    match t.results.as_ref() {
        Some(r) => power_reserve::title(r.run_s, r.ending, r.source, restarts),
        None => {
            let ending = t.ending.unwrap_or(power_reserve::Ending::ByHand);
            power_reserve::title(
                t.run_so_far_s(),
                ending,
                power_reserve::Source::Test,
                restarts,
            )
        }
    }
}

/// The Info panel's paragraphs, in Hugh's order (#1528): the run in
/// full, where its take is kept, the beats' pauses, then what the grey
/// cells mean. `hours` are those the pane shows.
fn info_lines(t: &Test, watch: &str, hours: &[power_reserve::Hour]) -> Vec<String> {
    let mut lines = Vec::new();
    let position = t.position.label();
    if t.running() {
        let mut run = "The run so far, from the first beat heard to now.".to_string();
        if t.heard
            .silent_s(t.now_s)
            .is_some_and(|s| s >= power_reserve::PAUSE_S)
        {
            run += &format!(
                " The run ends after {} min with no beat.",
                power_reserve::STOP_AFTER_S / 60.0
            );
        }
        lines.push(format!("Running in {position}. {run}"));
    } else {
        let sentence = t.results.as_ref().map_or_else(
            || {
                let ending = t.ending.unwrap_or(power_reserve::Ending::ByHand);
                power_reserve::sentence(
                    t.run_so_far_s(),
                    ending,
                    power_reserve::Source::Test,
                    position,
                )
            },
            |r| r.sentence(position),
        );
        lines.push(format!(
            "{sentence} The run is from the first beat heard to the last, in true time."
        ));
    }
    if t.saving {
        lines.push("Saving the take\u{2026}".to_string());
    } else if let Some(p) = &t.saved {
        let place = if watch.is_empty() { "Unsorted" } else { watch };
        let name = p
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into());
        // Read only while the panel shows: whether the sound is gone yet.
        let alone = (!t.keep_sound).then(|| timegrapher_core::analysis_take::is_analysis_only(p));
        lines.push(format!("Kept in {place}. {}.", kept_words(&name, alone)));
    }
    let n = t.heard.pauses.len();
    if n > 0 {
        lines.push(format!(
            "Beats stopped and came back {n} time{} on the way. {}.",
            timegrapher_core::measure::plural(n),
            pause_hint()
        ));
    }
    lines.extend(hour_key_text(hours));
    lines
}

/// What the beats' pauses are.
fn pause_hint() -> String {
    format!(
        "Silences of {:.0} s or more inside the run, after which the beats came back: a watch \
         near the end can stop and start again with a knock",
        power_reserve::PAUSE_S
    )
}

/// The Info panel: its paragraphs under "Run", as [`theme::info_text`]
/// draws them.
fn info_panel(ui: &mut egui::Ui, lines: &[String], max_h: f32) {
    let id = egui::Id::new("reserve info panel");
    theme::info_text(ui, id, ("Run", lines), max_h);
}

/// Before a run: one line of what to do, with a warning when the disk
/// where the take is kept has too little room for a long run.
fn before_line(ui: &mut egui::Ui, free: Option<(u64, u64)>, card: &str, ready: bool) {
    let pal = theme::pal(ui);
    let words = if ready {
        format!("Wind fully, set the position on the {card} card, then press Start Test.")
    } else {
        "New Test sets a run up on a clear Bench.".to_string()
    };
    let mut line =
        ui.add(egui::Label::new(RichText::new(words).color(pal.text_secondary)).truncate());
    if let Some((have, need)) = free {
        let room = format!(
            "{} free where the take is kept; a run of up to {} writes up to {} of sound.",
            gb(have),
            timegrapher_core::power_reserve::limit_text(),
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
}

/// Where the run's take is kept, named `name`, for the run line's hover:
/// `alone` is none with its sound kept, else whether its beats and
/// readings are saved and the sound deleted yet (Hugh's wording, 11-04).
fn kept_words(name: &str, alone: Option<bool>) -> String {
    match alone {
        None => format!("Kept as \"{name}\". Every pane reads it like any other take"),
        Some(false) => format!(
            "Kept as \"{name}\". Saving its beats and readings; the sound is deleted when \
             that finishes"
        ),
        Some(true) => format!(
            "Kept as \"{name}\", with its beats and readings only (the sound was deleted). \
             Every pane reads it like any other take"
        ),
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
        // third of it, for a long table); when that leaves them short of a
        // readable height, all of it, the table under them. What doesn't
        // fit scrolls.
        let row_h = ui
            .text_style_height(&egui::TextStyle::Body)
            .max(ui.spacing().interact_size.y)
            + 3.0;
        let gap = ui.spacing().item_spacing.y;
        let room = viewport_h - 8.0 - 3.0 * gap;
        let table_h = ((hours.len() as f32 + 2.0) * row_h + 4.0).min(room / 3.0);
        let with_table = (room - table_h - gap) / 2.0;
        let stacked = if with_table >= CHART_READ_H {
            with_table
        } else {
            ((room - gap) / 2.0).max(CHART_MIN_H)
        };
        // Too short to stack them at a height that reads, but wide enough:
        // side by side, each the whole height.
        let side = stacked < CHART_READ_H
            && ui.available_width() >= 2.0 * SIDE_MIN_W + ui.spacing().item_spacing.x;
        // Side by side, they fill the pane, the table wholly under them.
        let chart_h = if side {
            (viewport_h - 8.0).max(CHART_MIN_H)
        } else {
            stacked
        };
        // While it runs the charts gain a point as each hour is whole.
        let whole = if so_far {
            hours.iter().take_while(|h| h.whole()).count()
        } else {
            hours.len()
        };
        let mut plot_x = None;
        if whole > 0 {
            plot_x = Some(hour_chart(ui, &hours[..whole], chart_h, side));
            ui.add_space(8.0);
        }
        hour_table(ui, hours, plot_x);
    });
}

/// The least height of each of the Power Reserve charts, points.
const CHART_MIN_H: f32 = 56.0;

/// The least width of each Power Reserve chart side by side, points.
const SIDE_MIN_W: f32 = 360.0;

/// The height each Power Reserve chart reads well at, points: its hours'
/// labels and a plot that names a few values.
const CHART_READ_H: f32 = 100.0;

/// The least height the pane's run needs under its toolbar once it has
/// charts: its line and both charts at a height that reads, the table
/// scrolling under them; none before then. The Paper Strip above gives
/// room for it, down to its own least.
pub fn least_h(test: Option<&Test>) -> Option<f32> {
    let t = test?;
    let charts = match t.results.as_ref() {
        Some(r) => !r.hours.is_empty(),
        None => t.so_far.iter().any(|h| h.whole()),
    };
    charts.then_some(22.0 + 2.0 * CHART_READ_H + 8.0 + 12.0)
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
                beat_error_source: None,
                caveats: Vec::new(),
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
    60.0 + 2.0 * 160.0 + 8.0 + (hours as f32 + 2.0) * row_h
}

/// A value written by core's figure rule `text`, or a dash; its unit is
/// in the column's title.
fn cell(v: Option<f64>, text: impl Fn(f64) -> String) -> String {
    v.map_or_else(|| "–".to_string(), text)
}

/// The hour column's words: "0–1 h", and the last part hour a range of
/// the same form, its end in tenths of an hour, "46–46.4 h", never 46.0 or
/// the next whole hour (Hugh's ruling on #1086).
fn hour_label(h: &power_reserve::Hour) -> String {
    if h.whole() {
        return format!("{}–{} h", h.hour, h.hour + 1);
    }
    let tenths = (h.span_s / 360.0).round().clamp(1.0, 9.0);
    format!(
        "{}–{} h",
        h.hour,
        fixed_text(f64::from(h.hour) + tenths / 10.0, 1)
    )
}

/// A part hour's exact length, in its label's hover; whole seconds, as the
/// run's own words count them.
fn part_hour_hover(h: &power_reserve::Hour) -> Option<(String, bool)> {
    (!h.whole()).then(|| {
        (
            format!(
                "A part hour: {} past {} h",
                length_text(power_reserve::whole_s(h.span_s)),
                h.hour
            ),
            false,
        )
    })
}

/// The run hour by hour as a table, the hour in progress read like a
/// finished run's last, part hour. Under charts, it starts at their plots'
/// left edge, the value labels alone in the margin, and is no wider than
/// the plots, scrolling sideways when it would be; `plot_x` is the plots'
/// span, none with no chart.
fn hour_table(ui: &mut egui::Ui, hours: &[power_reserve::Hour], plot_x: Option<egui::Rangef>) {
    let Some(x) = plot_x else {
        hour_grid(ui, hours);
        return;
    };
    // The plots' span across, from here down.
    let below = egui::Rect::from_x_y_ranges(x, ui.available_rect_before_wrap().y_range());
    ui.scope_builder(egui::UiBuilder::new().max_rect(below), |ui| {
        egui::ScrollArea::horizontal()
            .id_salt("reserve hours across")
            // egui shows a scroll area's content a clip margin past its
            // edge.
            .max_width(x.span() - ui.visuals().clip_rect_margin)
            .auto_shrink([true, true])
            .show(ui, |ui| hour_grid(ui, hours));
    });
}

/// What the grey beat errors mean, in their key and the Beat Error title's
/// hover: one text for both.
macro_rules! dim_key {
    () => {
        "Grey beat errors: from the drop, or source not recorded."
    };
}

/// Whether a beat error is dimmed: one not taken from the unlock (from
/// the drop, or its source not recorded). The one rule for every table
/// that shows beat errors, Power Reserve and Positions Test (#1076).
pub fn beat_error_dimmed(source: Option<BeatErrorSource>) -> bool {
    source != Some(BeatErrorSource::Unlock)
}

/// The dimming's key, over a table so it is in view with the rows, only
/// when a row is dimmed (Hugh's rulings on Walter's walks of #1079 and
/// #1081).
pub fn dim_key(ui: &mut egui::Ui, any_dimmed: bool) {
    if any_dimmed {
        ui.label(RichText::new(dim_key!()).color(theme::pal(ui).text_secondary));
    }
}

/// What a grey amplitude or beat error read from few windows in the Power
/// Reserve table means.
pub const THIN_KEY: &str =
    "Grey amplitudes and beat errors: read from few of the hour's windows; the hover says how many.";

/// The hour's coverage caveat for `figure`, when the figure is shown and
/// stands on under 60% of its windows (core's caveat, #1429, #1439): the
/// cell is dimmed and the caveat is in its hover. Amplitude and beat error
/// alike, one rule.
fn thin(h: &power_reserve::Hour, figure: Figure) -> Option<String> {
    match figure {
        Figure::Amplitude => h.amplitude_deg?,
        Figure::BeatError => h.beat_error_ms?,
        Figure::Rate => h.rate_s_per_day?,
    };
    h.caveats
        .iter()
        .find(|c| {
            matches!(
                c,
                timegrapher_core::measure::Caveat::ReadingWindows { reading, .. }
                    if *reading == figure
            )
        })
        .and_then(|c| c.long(None))
}

/// The hour's beat error's hover and whether it is grey: its source in
/// words, then core's word on why it is from the drop where it is (the
/// unlock not found, or inconsistent, #1439), then its coverage caveat
/// when thin ([`thin`]).
fn beat_error_hover(h: &power_reserve::Hour) -> (String, bool) {
    let src = h.beat_error_source;
    let mut words = vec![beat_error_source_label(src).to_string()];
    words.extend(
        h.caveats
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    timegrapher_core::measure::Caveat::BeatErrorFromDrop { .. }
                )
            })
            .filter_map(|c| c.long(None)),
    );
    let thin = thin(h, Figure::BeatError);
    let grey = thin.is_some() || beat_error_dimmed(src);
    words.extend(thin);
    (words.join(". "), grey)
}

/// Whether any of the hour's amplitude or beat error is thin ([`thin`]).
fn any_thin(h: &power_reserve::Hour) -> bool {
    [Figure::Amplitude, Figure::BeatError]
        .into_iter()
        .any(|f| thin(h, f).is_some())
}

/// The Power Reserve table's key, one line, naming each kind of grey cell
/// its rows hold; none when no cell is grey.
fn hour_key_text(hours: &[power_reserve::Hour]) -> Option<String> {
    let parts: Vec<&str> = [
        (
            hours.iter().any(|h| beat_error_dimmed(h.beat_error_source)),
            dim_key!(),
        ),
        (hours.iter().any(any_thin), THIN_KEY),
    ]
    .into_iter()
    .filter_map(|(on, t)| on.then_some(t))
    .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// The key's words, for tests that look for it as drawn.
#[cfg(test)]
pub const DIM_KEY_TEXT: &str = dim_key!();

/// The Beat Error column's hover ends with the key.
pub fn beat_error_hint(lead: &str) -> String {
    format!("{lead} {}", dim_key!())
}

/// The table's titles: each name, its unit under it (none for Hours),
/// and what the column holds (Hugh's ruling on #1086).
pub(crate) const HOUR_COLUMNS: [(&str, &str, &str); 5] = [
    ("Hours", "", "Hours since the first beat heard"),
    (
        "Rate",
        "s/d",
        "The hour's rate: seconds a day gained (+) or lost (−)",
    ),
    ("Amplitude", "°", "The hour's median amplitude"),
    (
        "Beat Error",
        "ms",
        concat!(
            "The hour's beat error as a size, from the unlock, or from the drop in an hour \
             where the unlock isn't found. ",
            dim_key!()
        ),
    ),
    (
        "Read",
        "%",
        "How much of the hour its whole 10 s readings cover",
    ),
];

/// A title as drawn: its words, its hover, the cell, and where its name
/// and its unit lie.
struct Title {
    name: &'static str,
    unit: &'static str,
    hint: &'static str,
    cell: egui::Rect,
    name_at: egui::Pos2,
    unit_at: egui::Pos2,
}

/// Why an hour's Read (%) is a dash: its times can't belong to a real
/// hour (core `Hour::read_share`).
const NO_READ_SHARE: &str = "Not shown: this hour's reading time is longer than the hour";

/// A table cell: its text, and the words of its hover with whether it is
/// dimmed (a beat error's source; why a share is missing).
type Cell = (String, Option<(String, bool)>);

/// The table itself: the hour named at the left, the figures right-aligned
/// under their titles.
fn hour_grid(ui: &mut egui::Ui, hours: &[power_reserve::Hour]) {
    let pal = theme::pal(ui);
    // Each cell: its text, and the words of its source with whether it is
    // dimmed, for a beat error not from the unlock.
    let plain = |t: String| (t, None);
    let rows: Vec<[Cell; 5]> = hours
        .iter()
        .map(|h| {
            [
                (hour_label(h), part_hour_hover(h)),
                plain(cell(h.rate_s_per_day, |v| {
                    true_minus(&figure(v, None, base_digits::RATE, true))
                })),
                (
                    cell(h.amplitude_deg, |v| {
                        figure(v, None, base_digits::AMPLITUDE, false)
                    }),
                    thin(h, Figure::Amplitude).map(|w| (w, true)),
                ),
                (
                    cell(h.beat_error_ms.map(f64::abs), |v| {
                        figure(v, None, base_digits::BEAT_ERROR, false)
                    }),
                    // Its source in words, why it is from the drop
                    // where it is, and its coverage caveat when thin
                    // (#1439).
                    Some(beat_error_hover(h)),
                ),
                // A dash says why in its hover (Walter's walk of #1084).
                (
                    cell(h.read_share(), |v| fixed_text(100.0 * v, 0)),
                    h.read_share()
                        .is_none()
                        .then(|| (NO_READ_SHARE.into(), false)),
                ),
            ]
        })
        .collect();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let wide = |t: &str| {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(t.to_string(), font.clone(), egui::Color32::WHITE)
                .size()
                .x
        })
    };
    // Each column as wide as its title or its widest figure, a point to
    // spare for the bold title.
    let widths: Vec<f32> = HOUR_COLUMNS
        .iter()
        .enumerate()
        .map(|(c, (head, unit, _))| {
            rows.iter()
                .map(|r| wide(&r[c].0))
                .fold((wide(head) + 1.0).max(wide(unit)), f32::max)
        })
        .collect();
    let row_h = ui.spacing().interact_size.y;
    let put = |ui: &mut egui::Ui, c: usize, text: RichText| {
        if c == 0 {
            return ui.label(text);
        }
        // One cell, its column's width, the figure at its right.
        ui.allocate_ui_with_layout(
            egui::vec2(widths[c], row_h),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.set_min_width(widths[c]);
                ui.label(text)
            },
        )
        .inner
    };
    let mut titles = Vec::new();
    let grid = egui::Grid::new("reserve hours")
        .striped(true)
        .spacing([18.0, GRID_GAP])
        .show(ui, |ui| {
            for (c, (name, unit, hint)) in HOUR_COLUMNS.into_iter().enumerate() {
                titles.push(title(ui, (name, unit, hint), widths[c], c > 0));
            }
            ui.end_row();
            for row in &rows {
                for (c, (text, source)) in row.iter().enumerate() {
                    // A beat error not from the unlock and a thin
                    // amplitude dimmed, and every beat error's source in
                    // words.
                    let colour = if source.as_ref().is_some_and(|s| s.1) {
                        pal.text_secondary
                    } else {
                        pal.text
                    };
                    let r = put(ui, c, RichText::new(text).color(colour));
                    if let Some((words, _)) = source {
                        r.on_hover_text(words.as_str());
                    }
                }
                ui.end_row();
            }
        });
    pin_titles(ui, &titles, grid.response.rect, rows.len());
}

/// One title's cell, `w` wide: its name, and its unit on its own line
/// under it in plain secondary text, at the right over figures (`right`)
/// and at the left over the hours.
fn title(
    ui: &mut egui::Ui,
    (name, unit, hint): (&'static str, &'static str, &'static str),
    w: f32,
    right: bool,
) -> Title {
    let pal = theme::pal(ui);
    let align = if right {
        egui::Align::Max
    } else {
        egui::Align::Min
    };
    let r = ui.allocate_ui_with_layout(
        egui::vec2(w, ui.spacing().interact_size.y),
        egui::Layout::top_down(align),
        |ui| {
            ui.set_min_width(w);
            ui.spacing_mut().item_spacing.y = 0.0;
            let n = ui.label(RichText::new(name).strong().color(pal.text_secondary));
            // An empty line where there is no unit, so every name sits at
            // the cell's top.
            let u = ui.label(RichText::new(unit).color(pal.text_secondary));
            (n.rect.left_top(), u.rect.left_top())
        },
    );
    let (name_at, unit_at) = r.inner;
    let cell = r.response.on_hover_text(hint).rect;
    Title {
        name,
        unit,
        hint,
        cell,
        name_at,
        unit_at,
    }
}

/// The table's titles kept in view: once they scroll above the pane's
/// top while rows still show, drawn again there over the rows, so each
/// figure keeps its unit in sight (Walter's walk of #1055). The band under
/// them reaches down to the first row that shows whole, so no row shows
/// cut, and they sit at its foot, tight over that row as in their own
/// place. The pinned titles keep their hovers. What the grey cells mean
/// is in the Info panel (#1528). `table` is the whole table, titles
/// and `rows` rows of hours.
fn pin_titles(ui: &mut egui::Ui, titles: &[Title], table: egui::Rect, rows: usize) {
    let top = ui.clip_rect().top();
    let Some(head) = titles.iter().map(|t| t.cell).reduce(|a, b| a.union(b)) else {
        return;
    };
    // The first hour's row, and from one hour's top to the next's, the
    // gap between included.
    let first_row = head.bottom() + GRID_GAP;
    let pitch = (table.bottom() - first_row + GRID_GAP) / rows.max(1) as f32;
    let next = ((top + head.height() + GRID_GAP - first_row) / pitch)
        .ceil()
        .max(0.0);
    let band_bottom = first_row + next * pitch;
    if head.top() >= top || table.bottom() <= band_bottom {
        return;
    }
    // The titles at the band's foot.
    let titles_top = band_bottom - GRID_GAP - head.height();
    let dy = titles_top - head.top();
    let pal = theme::pal(ui);
    let painter = ui.painter();
    painter.rect_filled(
        // As wide and as deep as egui's row stripes, which reach 2 pt past
        // the table at each side and half the gap above and below a row,
        // so no hidden row's stripe edges show round it.
        egui::Rect::from_x_y_ranges(
            table.x_range().expand(2.0),
            top..=band_bottom - GRID_GAP / 2.0,
        ),
        0.0,
        pal.card,
    );
    let font = egui::TextStyle::Body.resolve(ui.style());
    let down = egui::vec2(0.0, dy);
    for t in titles {
        for (words, at) in [(t.name, t.name_at), (t.unit, t.unit_at)] {
            painter.text(
                at + down,
                egui::Align2::LEFT_TOP,
                words,
                font.clone(),
                pal.text_secondary,
            );
        }
    }
    // Over the rows they cover, so their hovers win.
    for t in titles {
        ui.interact(
            t.cell.translate(down),
            ui.id().with(("pinned title", t.name)),
            egui::Sense::hover(),
        )
        .on_hover_text(t.hint);
    }
}

/// The gap between the table's rows, points.
const GRID_GAP: f32 = 3.0;

/// The steps, in hours, the Power Reserve charts' hours go up in.
const HOUR_STEPS: [f64; 7] = [1.0, 2.0, 3.0, 6.0, 12.0, 24.0, 48.0];

/// The Power Reserve charts' hour marks: none before the run, in the
/// margin left of 0 h.
fn hour_marks(g: egui_plot::GridInput) -> Vec<egui_plot::GridMark> {
    // 80 points apart at least: egui_plot names hours only from 60 apart,
    // and fades them in up to 80.
    let mut marks = theme::even_grid(g, 80.0, &HOUR_STEPS);
    marks.retain(|m| m.value >= 0.0);
    marks
}

/// The least width of the Power Reserve charts' value axis, points.
const GUTTER: f32 = 64.0;

/// The Power Reserve charts' two figures: name, unit and reading.
type Series = (
    &'static str,
    &'static str,
    fn(&power_reserve::Hour) -> Option<f64>,
);

/// Rate, then amplitude.
const SERIES: [Series; 2] = [
    ("rate", " s/d", |h| h.rate_s_per_day),
    ("amplitude", "°", |h| h.amplitude_deg),
];

/// One width for both charts' value axes, so their plots start at the
/// same x: the least gutter, or the widest label either chart may draw,
/// read from each figure's whole-number ends with a decimal to spare.
fn shared_gutter(ui: &egui::Ui, hours: &[power_reserve::Hour]) -> f32 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let wide = |t: String| {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(t, font.clone(), egui::Color32::WHITE)
                .size()
                .x
        })
    };
    let mut w = GUTTER;
    for (_, unit, get) in SERIES {
        for v in hours.iter().filter_map(get) {
            for end in [v.floor(), v.ceil()] {
                w = w.max(wide(format!("{end:.1}{unit}")) + 8.0);
            }
        }
    }
    w
}

/// Where the run's hours end: the last hour's end, an hour at least, so a
/// short run reads 0 h to 1 h rather than the one hour over and over.
fn run_end_h(hours: &[power_reserve::Hour]) -> f64 {
    hours
        .iter()
        .map(|h| f64::from(h.hour) + h.span_s / 3600.0)
        .fold(1.0, f64::max)
}

/// Rate and amplitude against hours since the first beat, one above the
/// other on the same hours, or `side` by side in a short, wide pane: both
/// span 0 h to the run's end, on one width of value axis, so an hour sits
/// at the same x in each when stacked. Gives the plots' span across.
fn hour_chart(
    ui: &mut egui::Ui,
    hours: &[power_reserve::Hour],
    height: f32,
    side: bool,
) -> egui::Rangef {
    let end = run_end_h(hours);
    // A 5% margin each side, as egui's own, the same in both, so 0 h
    // clears the value axis.
    let x_range = (-end * 0.05, end * 1.05);
    let gutter = shared_gutter(ui, hours);
    // Room at the right for half the last hour's label, which centres on
    // the plot's edge, clear of the pane's scroll bar.
    ui.set_max_width(ui.available_width() - 18.0);
    let plots: Vec<egui::Rect> = if side {
        ui.columns(2, |cols| {
            cols.iter_mut()
                .zip(SERIES)
                .map(|(col, series)| one_chart(col, hours, series, height, x_range, gutter))
                .collect()
        })
    } else {
        SERIES
            .into_iter()
            .map(|series| one_chart(ui, hours, series, height, x_range, gutter))
            .collect()
    };
    plots
        .iter()
        .map(|r| r.x_range())
        .reduce(|a, b| egui::Rangef::new(a.min.min(b.min), a.max.max(b.max)))
        .unwrap_or(egui::Rangef::NOTHING)
}

/// One of the Power Reserve charts: `series` against hours since the first
/// beat, `height` tall, over `x_range` hours, its value axis `gutter` wide.
/// Gives its plot's frame.
fn one_chart(
    ui: &mut egui::Ui,
    hours: &[power_reserve::Hour],
    (name, unit, get): Series,
    height: f32,
    (x_lo, x_hi): (f64, f64),
    gutter: f32,
) -> egui::Rect {
    use egui_plot::{Line, Plot, PlotPoints};
    let pal = theme::pal(ui);
    // Its series' name over it, as the Distributions' plots have (#1528),
    // in the chart's height.
    use timegrapher_core::steadiness::SeriesKind;
    let kind = if name == "rate" {
        SeriesKind::Rate
    } else {
        SeriesKind::Amplitude
    };
    let title = crate::steady::title(kind);
    let named = ui.label(theme::caption(title).color(pal.text_secondary));
    let height = (height - named.rect.height() - ui.spacing().item_spacing.y).max(CHART_MIN_H);
    let x = |h: &power_reserve::Hour| f64::from(h.hour) + h.span_s / 7200.0;
    let colour = if name == "rate" {
        pal.trace_rate
    } else {
        pal.trace_amplitude
    };
    let pts: Vec<[f64; 2]> = hours
        .iter()
        .filter_map(|h| get(h).map(|v| [x(h), v]))
        .collect();
    let mut grid = Grid::new(hour_marks, |g| theme::even_grid(g, 24.0, &[]));
    if name == "rate" {
        grid = grid.zero_y();
    }
    grid.on(Plot::new(format!("reserve {name}")))
        .height(height)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .default_x_bounds(x_lo, x_hi)
        .y_axis_min_width(gutter)
        .x_axis_formatter(|m, _| format!("{} h", axis_number(m.value, m.step_size)))
        // The point under the pointer in words: hours into the run and
        // the hour's figure.
        .label_formatter(move |name, v| {
            if name.is_empty() {
                return String::new();
            }
            format!(
                "{} into the run\n{:.d$}{unit}",
                length_text((v.x * 3600.0).max(0.0)),
                v.y,
                d = usize::from(unit != "°")
            )
        })
        // Never "-0 s/d" on the zero line.
        .y_axis_formatter(move |m, _| {
            true_minus(&format!("{}{unit}", axis_number(m.value, m.step_size)))
        })
        .show(ui, |p| {
            grid.draw(p, pal);
            p.line(
                Line::new(name, PlotPoints::from(pts))
                    .color(colour)
                    .width(strength::DATA_W),
            );
        })
        .response
        .rect
}

/// The dialog for saving the run as a CSV file, starting in `dir`.
pub fn csv_dialog(dir: &std::path::Path) -> rfd::FileDialog {
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
    d
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
    if !r.started_utc.is_empty() {
        out += &line(&["Started (UTC)".into(), r.started_utc.clone()]);
    }
    out += &line(&["Position".into(), r.position.clone()]);
    out += &line(&[r.run_label().into(), r.run_words()]);
    out += &line(&["Ended".into(), r.ending.text()]);
    if let Some(w) = r.source.text() {
        out += &line(&["Read from".into(), w.into()]);
    }
    out += "\n";
    out += &line(&[
        "Hour".into(),
        "Length (s)".into(),
        "Rate (s/d)".into(),
        "Amplitude (deg)".into(),
        "Beat error (ms)".into(),
        "Beat error from".into(),
        "Read (s)".into(),
    ]);
    // Core's figure rule, plain: no plus, an ASCII minus, never "-0".
    let num = |v: Option<f64>, d: usize| v.map_or_else(String::new, |x| fixed_text(x, d));
    for h in &r.hours {
        out += &line(&[
            h.hour.to_string(),
            fixed_text(h.span_s, 0),
            num(h.rate_s_per_day, base_digits::RATE),
            num(h.amplitude_deg, base_digits::AMPLITUDE),
            num(h.beat_error_ms.map(f64::abs), base_digits::BEAT_ERROR),
            // Empty when none is recorded, as for a figure not measured.
            h.beat_error_source
                .map_or_else(String::new, |s| s.to_string()),
            fixed_text(h.read_s, 0),
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
        assert_eq!(gb(bytes_needed(48_000)), "27.6 GB");
        // A 24-bit microphone's sound is kept at 16 bits too.
        assert_eq!(bytes_per_hour(48_000), 48_000 * 2 * 3600);
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
            beat_error_source: None,
            caveats: Vec::new(),
            read_s: 90.0,
        }];
        let text = results_text(&r, "Pepsi, 1972", ',');
        assert!(text.starts_with("Watch,\"Pepsi, 1972\"\n"), "{text}");
        assert!(
            text.contains("Ended,Stopped by hand while the watch still ran"),
            "{text}"
        );
        assert!(
            // Beat error as its size, as every text gives it, and where it
            // came from: empty, as none is recorded for this hour.
            text.ends_with("0,100,4.2,281,0.40,,90\n")
                || text.ends_with("0,100,4.3,281,0.40,,90\n"),
            "{text}"
        );
        // Whole seconds, as the run's own words count them: 99.8 s is
        // "1 min 39 s", never a second over.
        assert_eq!(hour_label(&r.hours[0]), "0–0.1 h");
        assert_eq!(
            part_hour_hover(&r.hours[0]).map(|h| h.0).as_deref(),
            Some("A part hour: 1:39 past 0 h")
        );
        // A run ending 22 min into its 47th hour reads "46–46.4 h"; one
        // ending a minute short of 47 h is never "46–47.0 h".
        let mut part = r.hours[0].clone();
        part.hour = 46;
        for (span, label) in [
            (1320.0, "46–46.4 h"),
            (3540.0, "46–46.9 h"),
            (60.0, "46–46.1 h"),
        ] {
            part.span_s = span;
            assert_eq!(hour_label(&part), label);
        }
    }

    /// The run line's hover says only what is true of the take now: kept
    /// with its sound, its beats and readings being saved, or kept with
    /// them alone (Hugh's wording, Walter's walk of #1055).
    #[test]
    fn the_run_lines_hover_says_how_the_take_is_kept() {
        let name = "2026-10-25 23.17 Power Reserve";
        assert_eq!(
            kept_words(name, None),
            "Kept as \"2026-10-25 23.17 Power Reserve\". Every pane reads it like any other take"
        );
        assert_eq!(
            kept_words(name, Some(false)),
            "Kept as \"2026-10-25 23.17 Power Reserve\". Saving its beats and readings; the \
             sound is deleted when that finishes"
        );
        assert_eq!(
            kept_words(name, Some(true)),
            "Kept as \"2026-10-25 23.17 Power Reserve\", with its beats and readings only (the \
             sound was deleted). Every pane reads it like any other take"
        );
    }

    /// A run's hours whose beat error came from the drop: in the table
    /// that hour's beat error alone is dimmed and its hover says so, and
    /// its CSV row alone says "drop"; figures are written by core's rule,
    /// a rate that rounds to zero never "-0.0" (#1056).
    #[test]
    fn an_hour_from_the_drop_is_marked_and_says_so() {
        use timegrapher_core::session::BeatErrorSource;
        let mut t = finished_run(3);
        if let Some(r) = t.results.as_mut() {
            for h in &mut r.hours {
                h.beat_error_source = Some(BeatErrorSource::Unlock);
                h.rate_s_per_day = Some(-0.04);
            }
            r.hours[1].beat_error_source = Some(BeatErrorSource::Drop);
            r.hours[1].beat_error_ms = Some(-0.47);
        }
        let r = t.results.clone().unwrap();
        let csv = results_text(&r, "Pepsi", ',');
        let rows: Vec<&str> = csv
            .lines()
            .filter(|l| l.starts_with(|c: char| c.is_ascii_digit()))
            .collect();
        assert_eq!(rows.len(), 3, "{csv}");
        assert!(rows[0].ends_with(",0.0,281,0.40,unlock,3500"), "{csv}");
        assert!(rows[1].ends_with(",0.0,281,0.47,drop,3500"), "{csv}");
        assert!(!csv.contains("-0.0"), "{csv}");
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut ink = (egui::Color32::TRANSPARENT, egui::Color32::TRANSPARENT);
        let mut cells = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 900.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ink = (theme::pal(ui).text, theme::pal(ui).text_secondary);
                    pane(ui, Some(&t), None, ("Position", "Pepsi"), true, false);
                });
            });
            cells = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(s) => Some((
                        s.galley.text().to_string(),
                        s.galley.job.sections.first().map(|f| f.format.color),
                        s.pos.y,
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
        }
        let colour =
            |text: &str| -> Vec<_> { cells.iter().filter(|c| c.0 == text).map(|c| c.1).collect() };
        assert_eq!(colour("0.47"), vec![Some(ink.1)], "{cells:?}");
        assert_eq!(colour("0.40"), vec![Some(ink.0); 2], "{cells:?}");
        // The rate written as the app writes a rate: zero takes no sign.
        assert_eq!(colour("0.0").len(), 3, "{cells:?}");
        // The dimming's key is in the Info panel, not over the table
        // (#1528): drawn only with the panel open.
        let key = "Grey beat errors: from the drop, or source not recorded.";
        assert!(colour(key).is_empty(), "{cells:?}");
        let open = drawn_texts(&ctx, &t, true);
        assert_eq!(
            open.iter()
                .filter(|c| c.0 == key)
                .map(|c| c.1)
                .collect::<Vec<_>>(),
            vec![Some(ink.1)],
            "{open:?}"
        );
    }

    /// An hour whose amplitude and beat error stand on under 60% of
    /// their windows (core's coverage caveats, #1429, #1439): those two
    /// alone are grey, the key says what grey means, and each hover is
    /// core's caveat (the beat error's after its source).
    #[test]
    fn thin_figures_are_grey_and_keyed() {
        use timegrapher_core::confidence::Figure;
        use timegrapher_core::measure::Caveat;
        use timegrapher_core::session::BeatErrorSource;
        let mut t = finished_run(3);
        let caveat = |reading, windows_used| Caveat::ReadingWindows {
            reading,
            windows_used,
            windows_in_span: 1800,
        };
        let (amp, be) = (
            caveat(Figure::Amplitude, 400),
            caveat(Figure::BeatError, 445),
        );
        if let Some(r) = t.results.as_mut() {
            for h in &mut r.hours {
                h.beat_error_source = Some(BeatErrorSource::Unlock);
            }
            r.hours[2].amplitude_deg = Some(262.0);
            r.hours[2].beat_error_ms = Some(5.93);
            r.hours[2].caveats = vec![amp, be];
        }
        let r = t.results.as_ref().unwrap();
        assert_eq!(thin(&r.hours[0], Figure::Amplitude), None);
        assert_eq!(thin(&r.hours[0], Figure::BeatError), None);
        assert_eq!(thin(&r.hours[2], Figure::Amplitude), amp.long(None));
        assert_eq!(thin(&r.hours[2], Figure::BeatError), be.long(None));
        assert_eq!(hour_key_text(&r.hours).as_deref(), Some(THIN_KEY));
        // No figure, nothing grey.
        let mut none = r.hours[2].clone();
        none.amplitude_deg = None;
        none.beat_error_ms = None;
        assert_eq!(thin(&none, Figure::Amplitude), None);
        assert_eq!(thin(&none, Figure::BeatError), None);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut ink = (egui::Color32::TRANSPARENT, egui::Color32::TRANSPARENT);
        let mut cells = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 900.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ink = (theme::pal(ui).text, theme::pal(ui).text_secondary);
                    pane(ui, Some(&t), None, ("Position", "Pepsi"), true, false);
                });
            });
            cells = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(s) => Some((
                        s.galley.text().to_string(),
                        s.galley.job.sections.first().map(|f| f.format.color),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
        }
        let colour =
            |text: &str| -> Vec<_> { cells.iter().filter(|c| c.0 == text).map(|c| c.1).collect() };
        assert_eq!(colour("262"), vec![Some(ink.1)], "{cells:?}");
        assert_eq!(colour("281"), vec![Some(ink.0); 2], "{cells:?}");
        assert_eq!(colour("5.93"), vec![Some(ink.1)], "{cells:?}");
        assert_eq!(colour("0.40"), vec![Some(ink.0); 2], "{cells:?}");
        // Its key is in the Info panel (#1528).
        assert!(colour(THIN_KEY).is_empty(), "{cells:?}");
        let open = drawn_texts(&ctx, &t, true);
        assert_eq!(
            open.iter()
                .filter(|c| c.0 == THIN_KEY)
                .map(|c| c.1)
                .collect::<Vec<_>>(),
            vec![Some(ink.1)],
            "{open:?}"
        );
    }

    /// The texts the pane draws for `t` at 900 x 900, with the Info panel
    /// open (`info`) or not, and each one's colour.
    fn drawn_texts(
        ctx: &egui::Context,
        t: &Test,
        info: bool,
    ) -> Vec<(String, Option<egui::Color32>)> {
        let mut cells = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 900.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    pane(ui, Some(t), None, ("Position", "Pepsi"), true, info);
                });
            });
            cells = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(s) => Some((
                        s.galley.text().to_string(),
                        s.galley.job.sections.first().map(|f| f.format.color),
                    )),
                    _ => None,
                })
                .collect();
        }
        cells
    }

    /// The run line's parts in their homes (#1528): the run and its
    /// ending in short in the title, a long take's source in a grey chip,
    /// and the run in full, where it's kept, the pauses and the grey keys
    /// in the Info panel, in that order, drawn only with it open.
    #[test]
    fn the_run_line_moves_into_its_homes() {
        let mut t = finished_run(2);
        assert_eq!(run_title(&t), "Ran 31 s \u{b7} stopped by hand");
        if let Some(r) = t.results.as_mut() {
            r.source = power_reserve::Source::LongTake;
            r.hours[1].beat_error_source = Some(BeatErrorSource::Drop);
        }
        t.heard.pauses.push(power_reserve::Pause {
            from_s: 10.0,
            to_s: 20.0,
        });
        assert_eq!(
            run_title(&t),
            "Ran 31 s \u{b7} stopped by hand \u{b7} restarted once"
        );
        let hours = t.results.as_ref().unwrap().hours.clone();
        let lines = info_lines(&t, "Pepsi", &hours);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[0].starts_with("Ran 31 s in "), "{lines:?}");
        assert!(lines[0].contains("Read from a long take"), "{lines:?}");
        assert!(
            lines[1].starts_with("Beats stopped and came back 1 time "),
            "{lines:?}"
        );
        assert_eq!(lines[2], DIM_KEY_TEXT);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let shut = drawn_texts(&ctx, &t, false);
        let has =
            |cells: &[(String, Option<egui::Color32>)], w: &str| cells.iter().any(|c| c.0 == w);
        let chip = power_reserve::Source::LongTake.chip().unwrap();
        assert!(has(&shut, &run_title(&t)) && has(&shut, chip), "{shut:?}");
        assert!(
            !shut.iter().any(|c| c.0.starts_with("Beats stopped")),
            "{shut:?}"
        );
        let open = drawn_texts(&ctx, &t, true);
        for l in &lines {
            assert!(has(&open, l), "{l}: {open:?}");
        }
    }

    /// A running run's title says how the beats stand: waiting for the
    /// first, running, or no beat for core's pause or longer, with the
    /// restarts so far; and its Info says when the run will end only
    /// once the beats have gone quiet (Trevor's review of #1538).
    #[test]
    fn a_running_runs_title_and_info_follow_the_beats() {
        let mut t = Test::new(Position::DialUp, true, String::new(), 0.0);
        assert!(t.running());
        assert!(
            run_title(&t).ends_with(" \u{b7} Waiting for the first beats"),
            "{}",
            run_title(&t)
        );
        let beats: Vec<Beat> = (0..160)
            .map(|i| Beat {
                index: i,
                time: i as f64 * 0.2,
                quality: 1.0,
            })
            .collect();
        t.update(&beats, 0.2, 32.0);
        assert!(t.running());
        let steady = run_title(&t);
        assert!(steady.ends_with(" \u{b7} Running"), "{steady}");
        let ends = format!(
            "The run ends after {} min with no beat.",
            power_reserve::STOP_AFTER_S / 60.0
        );
        let info = info_lines(&t, "", &[]);
        assert!(!info[0].contains(&ends), "{info:?}");
        // Quiet for the pause and more, short of the stop.
        t.now_s = 31.8 + power_reserve::PAUSE_S + 60.0;
        assert!(t.running());
        let quiet = run_title(&t);
        assert!(quiet.contains(" \u{b7} No beat for "), "{quiet}");
        let info = info_lines(&t, "", &[]);
        assert!(
            info[0].starts_with("Running in ") && info[0].ends_with(&ends),
            "{info:?}"
        );
        // Just short of the pause: still running.
        t.now_s = 31.8 + power_reserve::PAUSE_S - 1.0;
        assert!(
            run_title(&t).ends_with(" \u{b7} Running"),
            "{}",
            run_title(&t)
        );
        assert!(!info_lines(&t, "", &[])[0].contains(&ends));
        // A restart shows while it runs.
        t.heard.pauses.push(power_reserve::Pause {
            from_s: 10.0,
            to_s: 20.0,
        });
        assert!(
            run_title(&t).ends_with(" \u{b7} Running \u{b7} restarted once"),
            "{}",
            run_title(&t)
        );
    }

    /// A saved run's Info says where its take is kept, in the watch or
    /// in Unsorted, after the run and before the pauses and the keys; and
    /// each chart is titled with its series, over it (Hugh, #1528).
    #[test]
    fn a_saved_runs_info_says_where_it_is_kept() {
        let mut t = finished_run(2);
        t.saved = Some(PathBuf::from("/takes/2026-11-13 08.00 Power Reserve"));
        t.heard.pauses.push(power_reserve::Pause {
            from_s: 10.0,
            to_s: 20.0,
        });
        let hours = t.results.as_ref().unwrap().hours.clone();
        let lines = info_lines(&t, "Pepsi", &hours);
        // The run, where it's kept, the pauses, then the grey key: these
        // hours' beat errors have no recorded source.
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert_eq!(lines[3], DIM_KEY_TEXT);
        assert_eq!(
            lines[1],
            "Kept in Pepsi. Kept as \"2026-11-13 08.00 Power Reserve\". Every pane reads it \
             like any other take."
        );
        assert!(lines[2].starts_with("Beats stopped"), "{lines:?}");
        assert!(info_lines(&t, "", &hours)[1].starts_with("Kept in Unsorted. "));
        t.saving = true;
        assert_eq!(
            info_lines(&t, "Pepsi", &hours)[1],
            "Saving the take\u{2026}"
        );
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let drawn = drawn_texts(&ctx, &finished_run(3), false);
        // Each name twice: over its chart and over its column.
        for name in ["Rate", "Amplitude"] {
            assert_eq!(drawn.iter().filter(|c| c.0 == name).count(), 2, "{drawn:?}");
        }
    }

    /// An hour whose times can't belong to a real hour shows a dash in
    /// Read (%), and the dash's hover says why (Walter's walk of #1084).
    #[test]
    fn a_dashed_share_says_why_on_hover() {
        let mut t = finished_run(2);
        if let Some(r) = t.results.as_mut() {
            r.hours[1].read_s = 3700.0;
        }
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.style_mut(|s| {
            s.interaction.tooltip_delay = 0.0;
            s.interaction.show_tooltips_only_when_still = false;
        });
        let mut texts = Vec::new();
        let mut events = Vec::new();
        for pass in 0..5 {
            let out = ctx.run(
                egui::RawInput {
                    events: std::mem::take(&mut events),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        hour_table(ui, &t.results.as_ref().unwrap().hours, None);
                    });
                },
            );
            texts = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(s) => Some((
                        s.galley.text().to_string(),
                        s.galley.rect.translate(s.pos.to_vec2()),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if pass == 1 {
                let dash = texts.iter().filter(|t| t.0 == "–").map(|t| t.1);
                let dash = dash.max_by(|a, b| a.left().total_cmp(&b.left()));
                events.push(egui::Event::PointerMoved(dash.expect("dash").center()));
            }
        }
        assert!(texts.iter().any(|t| t.0 == NO_READ_SHARE), "{texts:?}");
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
                    beat_error_source: None,
                    caveats: Vec::new(),
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
                        pane(ui, Some(&t), None, ("Position", "Pepsi"), true, false);
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
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
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
                        beat_error_source: None,
                        caveats: Vec::new(),
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
                        pane(ui, Some(t), None, ("Position", "Pepsi"), true, false);
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
        // Its unit in the column's title (Hugh's table rules).
        assert!(
            shown(&texts, "Amplitude") && shown(&texts, "°"),
            "{texts:?}"
        );
        assert!(shown(&texts, "280") || shown(&texts, "281"), "{texts:?}");
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
                        pane(ui, t, free, ("Position", ""), true, false);
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
            beat_error_source: None,
            caveats: Vec::new(),
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
                        pane(ui, Some(t), None, ("Position", "Pepsi"), true, false);
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
        assert!(shown.iter().any(|s| s == "0–0.4 h"), "{shown:?}");
        assert!(!shown.iter().any(|s| s == "0 h"), "no chart: {shown:?}");
        // Two whole hours and the third filling: three rows, and the
        // charts reach the second hour's point only.
        t.so_far = vec![hour(0, 3600.0), hour(1, 3600.0), hour(2, 1588.0)];
        let shown = texts(&t);
        for row in ["0–1 h", "1–2 h", "2–2.4 h"] {
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
            beat_error_source: None,
            caveats: Vec::new(),
            read_s: 30.0,
        }];
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut texts = Vec::new();
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| hour_chart(ui, &r.hours, 110.0, false));
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

    /// An 8.5 h run's two charts put each hour at the same x, though the
    /// rate's labels are wider than the amplitude's and the last part hour
    /// has a rate but no amplitude.
    #[test]
    fn the_two_charts_put_each_hour_at_the_same_x() {
        let rate = [3.2, 2.1, 1.4, 0.6, -0.8, -12.3, -44.1, -96.8, -150.5];
        let hours: Vec<_> = (0..9u32)
            .map(|h| power_reserve::Hour {
                hour: h,
                span_s: if h == 8 { 1800.0 } else { 3600.0 },
                rate_s_per_day: Some(rate[h as usize]),
                amplitude_deg: (h < 8).then(|| 292.0 - 9.0 * f64::from(h)),
                beat_error_ms: Some(0.4),
                beat_error_source: None,
                caveats: Vec::new(),
                read_s: 3500.0,
            })
            .collect();
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut texts = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 600.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| hour_chart(ui, &hours, 200.0, false));
            });
            texts = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some((
                        t.galley.text().to_string(),
                        t.galley.rect.translate(t.pos.to_vec2()).center().x,
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
        }
        for h in 0..=8 {
            let name = format!("{h} h");
            let xs: Vec<f32> = texts.iter().filter(|t| t.0 == name).map(|t| t.1).collect();
            assert_eq!(xs.len(), 2, "{name}: {texts:?}");
            assert!((xs[0] - xs[1]).abs() < 0.5, "{name}: {xs:?}");
        }
    }

    /// A long run's margin left of 0 h, wider than an hour step, marks no
    /// hour before the run.
    #[test]
    fn no_hour_is_marked_before_the_run() {
        let marks = hour_marks(egui_plot::GridInput {
            bounds: (-2.3, 48.8),
            base_step_size: 0.2,
        });
        assert!(!marks.is_empty());
        assert!(marks.iter().all(|m| m.value >= 0.0), "{marks:?}");
        assert_eq!(marks[0].value, 0.0);
    }
}
