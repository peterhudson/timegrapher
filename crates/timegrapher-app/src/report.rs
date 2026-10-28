//! Report a Problem: a zip a person can open, check and attach to a GitHub
//! issue, in the same `timegrapher.report/1` shape `timegrapher report`
//! writes, so a maintainer reads and replays both alike (docs/reports.md).
//!
//! The sound comes from what the app already has: the live session, which
//! is kept on disk as it goes, or the recording that was opened. Up to five
//! minutes of it go in, cut without changing a sample. The person plays it
//! back, trims it with two handles and says they listened before Save is
//! offered. Nothing is sent anywhere: the app opens the GitHub form and the
//! person attaches the zip.

use crate::theme::{DialogButton, Role};
use crate::{applog, theme};
use eframe::egui::{self, RichText, Sense, Vec2};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use timegrapher_core::analysis::{analyze, AnalysisConfig, Summary};
use timegrapher_core::library::length_text;
use timegrapher_core::redact::{os_version, recent_files, Redactor};
use timegrapher_core::{archive, audio, capture, clockstore, excerpt, recorder};

/// The most sound a report holds: five minutes of FLAC is about 18 MB,
/// under GitHub's 25 MB limit for attachments.
pub const SPAN_S: f64 = 300.0;
/// The shortest stretch the handles leave.
const MIN_S: f64 = 2.0;
/// Columns in the waveform's outline.
const COLUMNS: usize = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bug,
    Reading,
    Setup,
    Idea,
}

impl Kind {
    fn code(self) -> &'static str {
        match self {
            Kind::Bug => "bug",
            Kind::Reading => "reading",
            Kind::Setup => "setup",
            Kind::Idea => "idea",
        }
    }

    /// The GitHub issue form the report goes with.
    fn form(self) -> &'static str {
        match self {
            Kind::Reading => "bad_reading.yml",
            Kind::Idea => "feature.yml",
            Kind::Bug | Kind::Setup => "bug.yml",
        }
    }
}

/// Where the sound for a report comes from.
#[derive(Clone, Debug)]
pub struct Source {
    /// The audio files, joined end to end.
    pub files: Vec<PathBuf>,
    /// Their length, seconds.
    pub total_s: f64,
    /// The recording's name, or "this session".
    pub name: String,
    /// A live session (whose last minutes are offered), not a file.
    pub live: bool,
    /// Where the offered five minutes end, seconds.
    pub end_s: f64,
    /// What the recording's own files say about it (the session file of a
    /// take, or a sidecar), put beside the excerpt.
    pub about: Value,
}

/// What the app shows and is set to when the report is made.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    pub bph: Option<u32>,
    pub lift_deg: f64,
    /// The app's readings and settings, as JSON.
    pub app: Value,
}

/// The stretch of sound being looked at, in memory.
struct Clip {
    from_s: f64,
    rate: u32,
    /// Mono, in [-1, 1].
    mono: Arc<Vec<f32>>,
    /// Lowest and highest sample in each column.
    outline: Vec<(f32, f32)>,
}

impl Clip {
    fn len_s(&self) -> f64 {
        self.mono.len() as f64 / self.rate as f64
    }
}

fn cut_clip(files: &[PathBuf], from_s: f64) -> Result<Clip, String> {
    let pcm = excerpt::cut(files, from_s, SPAN_S).map_err(|e| e.to_string())?;
    if pcm.frames() == 0 {
        return Err("There is no sound there.".into());
    }
    let full = (1u64 << (pcm.bits_per_sample.max(1) - 1)) as f32;
    let ch = pcm.channels.max(1) as usize;
    let mono: Vec<f32> = pcm
        .samples
        .chunks(ch)
        .map(|f| f.iter().map(|&s| s as f32).sum::<f32>() / (ch as f32 * full))
        .collect();
    let per = mono.len().div_ceil(COLUMNS).max(1);
    let outline = mono
        .chunks(per)
        .map(|c| {
            c.iter()
                .fold((0.0f32, 0.0f32), |(lo, hi), &s| (lo.min(s), hi.max(s)))
        })
        .collect();
    Ok(Clip {
        from_s,
        rate: pcm.sample_rate,
        mono: Arc::new(mono),
        outline,
    })
}

/// What saving came to.
enum Saved {
    Done { zip: PathBuf, bytes: u64 },
    Failed(String),
}

#[derive(Default)]
pub struct ReportDialog {
    pub open: bool,
    kind: Option<Kind>,
    note: String,
    include: bool,
    listened: bool,
    cc0: bool,
    source: Option<Source>,
    shown: Shown,
    clip: Option<Result<Clip, String>>,
    /// Where the five minutes end, while it is being chosen.
    end_s: f64,
    /// The handles, seconds into the clip.
    trim: (f64, f64),
    dragging: Option<usize>,
    player: Option<capture::Playback>,
    player_error: Option<String>,
    saving: Option<Receiver<Saved>>,
    result: Option<Saved>,
    save_dir: Option<PathBuf>,
}

impl ReportDialog {
    /// Open the dialog for `source` (none when there is no sound) as the
    /// app shows `shown`.
    pub fn show(&mut self, kind: Kind, source: Option<Source>, shown: Shown) {
        self.player = None;
        self.player_error = None;
        self.result = None;
        self.kind = Some(kind);
        self.include = source.is_some();
        self.listened = false;
        self.cc0 = false;
        self.end_s = source.as_ref().map_or(0.0, |s| s.end_s);
        self.source = source;
        self.shown = shown;
        self.clip = None;
        self.open = true;
    }

    fn load(&mut self) {
        let Some(s) = &self.source else { return };
        let from = (self.end_s - SPAN_S).max(0.0);
        self.player = None;
        self.clip = Some(cut_clip(&s.files, from));
        let len = match &self.clip {
            Some(Ok(c)) => c.len_s(),
            _ => 0.0,
        };
        self.trim = (0.0, len);
    }

    pub fn ui(&mut self, ctx: &egui::Context) -> Option<String> {
        if !self.open {
            return None;
        }
        let mut message = None;
        if let Some(rx) = &self.saving {
            if let Ok(r) = rx.try_recv() {
                self.saving = None;
                if let Saved::Done { zip, .. } = &r {
                    let kind = self.kind.unwrap_or(Kind::Bug);
                    if let Some(dir) = zip.parent() {
                        ctx.open_url(egui::OpenUrl::new_tab(format!("file://{}", dir.display())));
                    }
                    ctx.open_url(egui::OpenUrl::new_tab(format!(
                        "https://github.com/peterhudson/timegrapher/issues/new?template={}",
                        kind.form()
                    )));
                    applog::info(&format!("report saved: {}", zip.display()));
                    message = Some(format!(
                        "Saved the report as {}. Attach it to the GitHub form that opened.",
                        zip.display()
                    ));
                }
                self.result = Some(r);
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
        if self.include && self.clip.is_none() && self.source.is_some() {
            self.load();
        }
        if self.player.as_ref().is_some_and(|p| p.finished()) {
            self.player = None;
        }
        if self.player.is_some() {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        let mut open = self.open;
        egui::Window::new("Report a Problem")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(620.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| self.contents(ui));
        if !open {
            self.close();
        }
        message
    }

    fn close(&mut self) {
        self.open = false;
        self.player = None;
        self.clip = None;
    }

    fn contents(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        ui.set_width(620.0);
        ui.add(
            egui::Label::new(
                RichText::new(
                    "This makes a zip file to attach to a GitHub issue: what you write here, \
                     the app's version, readings and log, and the sound if you include it. \
                     Open it to see everything it holds. The app sends nothing anywhere.",
                )
                .color(pal.text_secondary),
            )
            .wrap(),
        );
        ui.add_space(8.0);
        let mut kind = self.kind.unwrap_or(Kind::Bug);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Kind").font(theme::semibold(13.0)));
            theme::segmented(
                ui,
                &mut kind,
                &[
                    (Kind::Bug, "Bug", "Something went wrong in the app"),
                    (
                        Kind::Reading,
                        "Reading",
                        "A rate, amplitude or beat error that looks wrong",
                    ),
                    (
                        Kind::Setup,
                        "Setup",
                        "Trouble with the microphone, the level or the stand",
                    ),
                    (Kind::Idea, "Idea", "Something the app could do"),
                ],
            );
        });
        self.kind = Some(kind);
        ui.add_space(6.0);
        ui.label(RichText::new("What Happened").font(theme::semibold(13.0)));
        ui.add(
            egui::TextEdit::multiline(&mut self.note)
                .hint_text("What you did, what you saw, and what you expected to see")
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        ui.add_space(8.0);
        match self.source.clone() {
            None => {
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "There is no sound to include: listen to a watch or open a \
                             recording first, and the report can carry up to five minutes \
                             of it.",
                        )
                        .color(pal.text_secondary),
                    )
                    .wrap(),
                );
            }
            Some(src) => {
                if theme::switch_row(
                    ui,
                    "Include the Sound",
                    &mut self.include,
                    "Up to five minutes of the sound, unchanged, so a maintainer can hear \
                     and analyse what the app heard",
                ) && !self.include
                {
                    self.player = None;
                }
                if self.include {
                    self.sound(ui, &src);
                }
            }
        }
        ui.add_space(10.0);
        let busy = self.saving.is_some();
        let ready = !busy && (!self.has_sound() || self.listened);
        let w = ui.available_width();
        ui.allocate_ui_with_layout(
            egui::vec2(w, 26.0),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                let buttons = [
                    DialogButton::new(
                        Role::Default,
                        "Save Report…",
                        "Choose where to save the zip. Its folder and the GitHub form open \
                         next, so you can drag it in.",
                    )
                    .enabled_unless(
                        ready,
                        if busy {
                            "Saving…"
                        } else {
                            "Listen to the sound, then tick the box above first"
                        },
                    ),
                    DialogButton::new(Role::Cancel, "Cancel", ""),
                ];
                match theme::dialog_buttons_right(ui, &buttons) {
                    Some(0) => self.save(),
                    Some(_) => self.close(),
                    None => {}
                }
                // While it saves, at the left of the row.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if busy {
                        ui.spinner();
                        ui.label(RichText::new("Saving…").color(pal.text_secondary));
                    }
                });
            },
        );
        match &self.result {
            Some(Saved::Done { zip, bytes }) => {
                let big = *bytes > 25_000_000;
                let saved = format!("Saved {} ({:.1} MB).", zip.display(), *bytes as f64 / 1e6);
                if big {
                    theme::alert_label(
                        ui,
                        theme::Alert::Warning,
                        &format!(
                            "{saved} That is over GitHub's 25 MB limit: trim the sound and save \
                             again."
                        ),
                        false,
                    );
                } else {
                    ui.add(egui::Label::new(RichText::new(saved).color(pal.text_secondary)).wrap());
                }
            }
            Some(Saved::Failed(e)) => {
                theme::alert_label(
                    ui,
                    theme::Alert::Bad,
                    &format!("Couldn't save the report: {e}"),
                    false,
                );
            }
            None => {}
        }
    }

    fn has_sound(&self) -> bool {
        self.include && matches!(self.clip, Some(Ok(_)))
    }

    /// The waveform with its handles, Play, and the consent boxes.
    fn sound(&mut self, ui: &mut egui::Ui, src: &Source) {
        let pal = theme::pal(ui);
        let span = src.total_s.min(SPAN_S);
        ui.add(
            egui::Label::new(
                RichText::new(if src.live {
                    format!(
                        "The last {} of this session, from what the app kept as it listened.",
                        length_text(span)
                    )
                } else {
                    format!("{} of {}.", length_text(span), src.name)
                })
                .color(pal.text_secondary),
            )
            .wrap(),
        );
        if src.total_s > SPAN_S + 1.0 {
            ui.horizontal(|ui| {
                ui.label("Ends At");
                ui.spacing_mut().slider_width = (ui.available_width() - 90.0).max(120.0);
                let r = ui
                    .add(
                        egui::Slider::new(&mut self.end_s, SPAN_S..=src.total_s)
                            .custom_formatter(|v, _| length_text(v))
                            .show_value(true),
                    )
                    .on_hover_text(
                        "Which five minutes of the recording: the time they end at. The \
                         handles below then trim them.",
                    );
                if r.drag_stopped() || (r.changed() && !r.dragged()) {
                    self.listened = false;
                    self.load();
                }
            });
        }
        let clip = match &self.clip {
            Some(Ok(c)) => c,
            Some(Err(e)) => {
                theme::alert_label(
                    ui,
                    theme::Alert::Bad,
                    &format!("Can't read the sound: {e}"),
                    false,
                );
                return;
            }
            None => return,
        };
        let len = clip.len_s();
        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 96.0),
            Sense::click_and_drag(),
        );
        let x_of = |t: f64| rect.left() + (t / len) as f32 * rect.width();
        let t_of = |x: f32| (((x - rect.left()) / rect.width()) as f64 * len).clamp(0.0, len);
        if resp.drag_started() {
            if let Some(p) = resp.interact_pointer_pos() {
                let d0 = (p.x - x_of(self.trim.0)).abs();
                let d1 = (p.x - x_of(self.trim.1)).abs();
                self.dragging = Some(if d0 <= d1 { 0 } else { 1 });
            }
        }
        if let (Some(h), Some(p)) = (self.dragging, resp.interact_pointer_pos()) {
            if resp.dragged() {
                let t = t_of(p.x);
                if h == 0 {
                    self.trim.0 = t.min(self.trim.1 - MIN_S).max(0.0);
                } else {
                    self.trim.1 = t.max(self.trim.0 + MIN_S).min(len);
                }
                self.player = None;
                self.listened = false;
            }
        }
        if resp.drag_stopped() {
            self.dragging = None;
        }
        let near = resp.hover_pos().is_some_and(|p| {
            (p.x - x_of(self.trim.0)).abs() < 8.0 || (p.x - x_of(self.trim.1)).abs() < 8.0
        });
        if near || self.dragging.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        resp.on_hover_text(
            "Drag the handles at either end to keep only the part that shows the problem",
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, theme::CARD_RADIUS, pal.control);
        let mid = rect.center().y;
        let half = rect.height() * 0.46;
        let n = clip.outline.len().max(1);
        for (i, &(lo, hi)) in clip.outline.iter().enumerate() {
            let x = rect.left() + (i as f32 + 0.5) / n as f32 * rect.width();
            let t = (i as f64 + 0.5) / n as f64 * len;
            let kept = t >= self.trim.0 && t <= self.trim.1;
            let c = if kept {
                pal.text_secondary
            } else {
                pal.text_tertiary.gamma_multiply(0.5)
            };
            painter.line_segment(
                [
                    egui::pos2(x, mid - hi.clamp(-1.0, 1.0) * half),
                    egui::pos2(x, mid - lo.clamp(-1.0, 1.0) * half + 0.5),
                ],
                egui::Stroke::new(1.0_f32, c),
            );
        }
        for t in [self.trim.0, self.trim.1] {
            let x = x_of(t);
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(2.0_f32, pal.accent),
            );
            let grip = egui::Rect::from_center_size(egui::pos2(x, mid), Vec2::new(8.0, 22.0));
            painter.rect_filled(grip, 3.0, pal.accent);
        }
        if let Some(p) = &self.player {
            let x = x_of(self.trim.0 + p.position_s());
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.5_f32, pal.tock),
            );
        }
        ui.horizontal_wrapped(|ui| {
            let playing = self.player.is_some();
            if ui
                .add(theme::secondary(ui, if playing { "Stop" } else { "Play" }))
                .on_hover_text("Listen to the part between the handles")
                .clicked()
            {
                if playing {
                    self.player = None;
                } else {
                    let rate = clip.rate as f64;
                    let a = (self.trim.0 * rate) as usize;
                    let b = ((self.trim.1 * rate) as usize).min(clip.mono.len());
                    match capture::play(Arc::new(clip.mono[a..b].to_vec()), clip.rate) {
                        Ok(p) => {
                            self.player = Some(p);
                            self.player_error = None;
                        }
                        Err(e) => self.player_error = Some(e),
                    }
                }
            }
            let kept = self.trim.1 - self.trim.0;
            ui.label(
                RichText::new(format!(
                    "{} kept, {} to {} · about {:.0} MB",
                    length_text(kept),
                    length_text(clip.from_s + self.trim.0),
                    length_text(clip.from_s + self.trim.1),
                    // FLAC of a watch on the stand: about 60 kB a second.
                    kept * 0.06,
                ))
                .color(pal.text_secondary),
            );
        });
        if let Some(e) = &self.player_error {
            theme::alert_label(
                ui,
                theme::Alert::Warning,
                &format!("Can't play it here: {e}"),
                false,
            );
        }
        ui.add_space(4.0);
        // The boxes ringed, so they show on the card in the dark theme.
        let ring = egui::Stroke::new(1.0_f32, pal.text_tertiary);
        ui.visuals_mut().widgets.inactive.bg_stroke = ring;
        ui.checkbox(
            &mut self.listened,
            "I listened to this sound, and it holds nothing private, such as voices",
        )
        .on_hover_text("Save Report is offered once this is ticked");
        ui.checkbox(
            &mut self.cc0,
            "Offer it to the public test corpus, free for anyone to use (CC0)",
        )
        .on_hover_text(
            "Recordings in the corpus test every change to the timegrapher. Only the \
             person who made the recording should tick this.",
        );
    }

    fn save(&mut self) {
        let created = recorder::utc(SystemTime::now());
        let stamp: String = created
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == 'T')
            .take(15)
            .collect();
        let name = format!("timegrapher-report-{stamp}Z.zip");
        let mut d = rfd::FileDialog::new()
            .set_title("Save the report as…")
            .set_file_name(&name);
        if let Some(dir) = self.save_dir.clone().or_else(dirs_download) {
            d = d.set_directory(dir);
        }
        let Some(zip) = d.save_file() else { return };
        self.save_dir = zip.parent().map(Path::to_path_buf);
        let sound = match (&self.clip, self.has_sound()) {
            (Some(Ok(c)), true) => Some(Excerpt {
                from_s: c.from_s + self.trim.0,
                span_s: self.trim.1 - self.trim.0,
            }),
            _ => None,
        };
        let job = Job {
            zip,
            created,
            kind: self.kind.unwrap_or(Kind::Bug),
            note: self.note.trim().to_string(),
            source: self.source.clone(),
            sound,
            listened: self.listened,
            cc0: self.cc0,
            shown: self.shown.clone(),
        };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let r = match job.write() {
                Ok(bytes) => Saved::Done {
                    zip: job.zip.clone(),
                    bytes,
                },
                Err(e) => Saved::Failed(e),
            };
            let _ = tx.send(r);
        });
        self.result = None;
        self.saving = Some(rx);
    }
}

/// The Downloads folder, where a browser looks first for a file to attach.
fn dirs_download() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    let d = home.join("Downloads");
    Some(if d.is_dir() { d } else { home })
}

struct Excerpt {
    from_s: f64,
    span_s: f64,
}

/// Everything saving needs, to run away from the window.
struct Job {
    zip: PathBuf,
    created: String,
    kind: Kind,
    note: String,
    source: Option<Source>,
    sound: Option<Excerpt>,
    listened: bool,
    cc0: bool,
    shown: Shown,
}

impl Job {
    /// Write the zip; its size in bytes.
    fn write(&self) -> Result<u64, String> {
        let mut red = Redactor::from_env();
        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        let mut commands = Vec::new();
        let mut left_out: Vec<String> = vec![
            "the list of sound inputs".into(),
            "environment variables".into(),
            "an analysis of the whole recording: the app's own readings are in \
             results/app.json instead"
                .into(),
        ];
        let mut recording = Value::Null;
        if let Some(src) = &self.source {
            let mut excerpt_info = Value::Null;
            match &self.sound {
                None => left_out.push("the recording (not included)".into()),
                Some(x) => {
                    let pcm =
                        excerpt::cut(&src.files, x.from_s, x.span_s).map_err(|e| e.to_string())?;
                    let flac = excerpt::encode_flac(&pcm).map_err(|e| e.to_string())?;
                    excerpt_info = json!({
                        "from_s": x.from_s,
                        "span_s": pcm.duration_s(),
                        "sample_rate": pcm.sample_rate,
                        "channels": pcm.channels,
                        "bits_per_sample": pcm.bits_per_sample,
                    });
                    let side = json!({
                        "made_by": "timegrapher-app",
                        "source": src.about,
                        "excerpt": {
                            "of": src.name,
                            "from_s": x.from_s,
                            "span_s": pcm.duration_s(),
                        },
                    });
                    // The analysis `report --replay` runs again: `analyze` on
                    // the excerpt with the command line's settings.
                    let mut args = vec![
                        "analyze".to_string(),
                        "audio/excerpt.flac".into(),
                        "--json".into(),
                    ];
                    if let Some(b) = self.shown.bph {
                        args.extend(["--bph".into(), b.to_string()]);
                    }
                    args.extend(["--lift".into(), self.shown.lift_deg.to_string()]);
                    let doc = analyze_like_the_cli(&flac, self.shown.bph, self.shown.lift_deg)?;
                    entries.push(("audio/excerpt.flac".into(), flac));
                    entries.push((
                        "audio/excerpt.flac.json".into(),
                        red.apply(&pretty(&side)).into_bytes(),
                    ));
                    entries.push((
                        "results/analyze-excerpt.json".into(),
                        red.apply(&pretty(&doc)).into_bytes(),
                    ));
                    commands.push(json!({
                        "args": args,
                        "cwd": "bundle",
                        "output": "results/analyze-excerpt.json",
                        "replayable": true,
                    }));
                }
            }
            recording = json!({
                "name": red.apply(&src.name),
                "files": src.files.len(),
                "excerpt": excerpt_info,
            });
        }
        entries.push((
            "results/app.json".into(),
            red.apply(&pretty(&self.shown.app)).into_bytes(),
        ));
        if let Some(p) = clockstore::path() {
            if let Ok(t) = std::fs::read_to_string(&p) {
                entries.push(("env/clock.json".into(), red.apply(&t).into_bytes()));
            }
        }
        if let Some(dir) = clockstore::config_dir() {
            let mut logs = recent_files(&dir.join("logs"), |_| true);
            logs.extend(recent_files(&dir, |n| {
                n.starts_with("crash-") && n.ends_with(".txt")
            }));
            for f in logs {
                if let Ok(t) = std::fs::read_to_string(&f) {
                    let name = f
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    entries.push((
                        format!("logs/{}", red.apply(&name)),
                        red.apply(&t).into_bytes(),
                    ));
                }
            }
        }
        let note = (!self.note.is_empty()).then(|| red.apply(&self.note));
        let files: Vec<Value> = entries
            .iter()
            .map(|(p, d)| json!({ "path": p, "bytes": d.len(), "sha256": archive::sha256_hex(d) }))
            .collect();
        let audio = self.sound.is_some() && self.source.is_some();
        let manifest = json!({
            "schema": "timegrapher.report/1",
            "software": software(),
            "input": {},
            "created_utc": self.created,
            "kind": self.kind.code(),
            "note": note,
            "os_version": os_version().map(|v| red.apply(&v)),
            "made_with": "app",
            "recording": recording,
            "commands": commands,
            "files": files,
            "consent": {
                "audio_included": audio,
                "listened_back": audio && self.listened,
                "corpus_cc0": audio && self.cc0,
            },
            "redacted": red.summary(),
            "left_out": left_out,
        });
        let mut all = vec![("manifest.json".to_string(), pretty(&manifest).into_bytes())];
        all.extend(entries);
        archive::write_zip(&self.zip, &all)?;
        Ok(std::fs::metadata(&self.zip).map(|m| m.len()).unwrap_or(0))
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

/// The configuration of the explicit excerpt replay command.
fn excerpt_analysis_config(bph: Option<u32>, lift_deg: f64) -> AnalysisConfig {
    let mut cfg = AnalysisConfig {
        bph,
        // Job::write always emits --lift, including a numeric value of 52.
        // This describes replay arguments, not the original app's provenance.
        lift_known: true,
        escape_teeth: 15,
        ..Default::default()
    };
    cfg.amplitude.lift_deg = lift_deg;
    cfg.envelope.notch_hz = Vec::new();
    cfg.envelope.highpass_hz = 1500.0;
    cfg
}

/// What `timegrapher analyze audio/excerpt.flac --json` prints, with the
/// command line's own defaults, so `report --replay` compares like with
/// like. The FLAC is read back as the command line reads it.
fn analyze_like_the_cli(flac: &[u8], bph: Option<u32>, lift_deg: f64) -> Result<Value, String> {
    let tmp = std::env::temp_dir().join(format!("timegrapher-report-{}.flac", std::process::id()));
    std::fs::write(&tmp, flac).map_err(|e| e.to_string())?;
    let loaded = audio::load(&tmp);
    let _ = std::fs::remove_file(&tmp);
    let audio = loaded.map_err(|e| e.to_string())?;
    let cfg = excerpt_analysis_config(bph, lift_deg);
    let a = analyze(&audio, &cfg);
    excerpt_document(&a.summary, &cfg)
}

/// The same raw-summary plus correction-wrapper contract as CLI analyze.
fn excerpt_document(summary: &Summary, cfg: &AnalysisConfig) -> Result<Value, String> {
    let mut doc = json!({
        "schema": "timegrapher.analyze/2",
        "software": software(),
        "input": {
            "files": [{ "path": "audio/excerpt.flac" }],
            "settings": {
                "bph": cfg.bph,
                "lift_deg": cfg.amplitude.lift_deg,
                "notch_hz": cfg.envelope.notch_hz,
                "highpass_hz": cfg.envelope.highpass_hz,
                "escape_teeth": cfg.escape_teeth,
            },
        },
    });
    let mut body = serde_json::to_value(summary).map_err(|e| e.to_string())?;
    if let Some(usb) = summary.usb_clock {
        let (ppm, from) = usb.correction();
        body["clock"] = clockstore::correction_json(
            ppm,
            &from,
            summary.overall.as_ref().map(|f| f.rate_s_per_day),
        );
    }
    if let (Some(d), Value::Object(b)) = (doc.as_object_mut(), body) {
        d.extend(b);
    }
    Ok(doc)
}

/// The same `software` block the command line's documents carry.
pub(crate) fn software() -> Value {
    let dirty = match env!("TIMEGRAPHER_GIT_DIRTY") {
        "true" => json!(true),
        "false" => json!(false),
        _ => Value::Null,
    };
    json!({
        "name": "timegrapher",
        "version": env!("CARGO_PKG_VERSION"),
        "git_hash": env!("TIMEGRAPHER_GIT_HASH"),
        "git_dirty": dirty,
        "build_date": env!("TIMEGRAPHER_BUILD_DATE"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use timegrapher_core::redact::replace_word;
    use timegrapher_core::synth::{generate, SynthConfig};

    fn assert_explicit_replay_lift(bph: Option<u32>, lift: f64) {
        let cfg = excerpt_analysis_config(bph, lift);
        assert_eq!(cfg.bph, bph, "Auto remains automatic");
        assert_eq!(cfg.amplitude.lift_deg, lift);
        assert_eq!(cfg.escape_teeth, 15);
        assert_eq!(cfg.envelope.highpass_hz, 1500.0);
        assert!(cfg.envelope.notch_hz.is_empty());
        // Exercise the actual assessment rule without generating sound.
        // The real bundle test below covers the analysis integration.
        let assessment = timegrapher_core::measure::assess(
            &[],
            &[],
            &timegrapher_core::measure::Context {
                lift_known: cfg.lift_known,
                ..Default::default()
            },
        );
        assert!(!assessment.doubts.lift_unknown, "{bph:?}, {lift}");
        assert!(assessment
            .caveats
            .iter()
            .all(|c| !matches!(c, timegrapher_core::measure::Caveat::LiftUnknown)));
    }

    #[test]
    fn excerpt_replay_fixed_default_lift() {
        assert_explicit_replay_lift(Some(28800), 52.0);
    }

    #[test]
    fn excerpt_replay_fixed_custom_lift() {
        assert_explicit_replay_lift(Some(28800), 49.5);
    }

    #[test]
    fn excerpt_replay_auto_default_lift() {
        assert_explicit_replay_lift(None, 52.0);
    }

    #[test]
    fn excerpt_replay_auto_custom_lift() {
        assert_explicit_replay_lift(None, 49.5);
    }

    // Supplied measurement records: these tests exercise document construction,
    // not USB detection, acoustic fitting, or new clock estimates.
    fn clock_summary(ppm: Option<f64>, fitted: bool) -> Summary {
        use timegrapher_core::{
            analysis::Clipping, confidence::Doubts, timing::TimingFit, usbclock::UsbClock,
        };
        Summary {
            duration_s: 60.0,
            sample_rate: 48000,
            bph: 28800,
            bph_heard: None,
            beats_found: 480,
            beats_found_fraction: 1.0,
            watch_heard: fitted,
            overall: fitted.then_some(TimingFit {
                rate_s_per_day: 12.0,
                beat_error_ms: 0.2,
                period_s: 0.125,
                jitter_us: 10.0,
                beats_used: 480,
            }),
            beat_error_unlock_ms: None,
            rate_p05: None,
            rate_p95: None,
            jitter_us: None,
            timing_scatter_fraction: None,
            amplitude_deg: None,
            amplitude_even_deg: None,
            amplitude_odd_deg: None,
            amplitude_se_deg: None,
            amplitude_even_se_deg: None,
            amplitude_odd_se_deg: None,
            lift_deg: 52.0,
            timing_periods: vec![],
            amplitude_periods: vec![],
            clipping: Clipping {
                samples: 0,
                beats: 0,
                beat_fraction: 0.0,
                peak_dbfs: -10.0,
            },
            usb_clock: ppm.map(|ppm| UsbClock {
                ppm,
                lines: 3,
                spread_ppm: 0.125,
            }),
            rate_s_per_day: fitted.then_some(12.0),
            rate_corrected_s_per_day: fitted
                .then(|| ppm.map(|p| clockstore::correct_rate(12.0, p)))
                .flatten(),
            doubts: Doubts {
                clipped: false,
                beats_missing: false,
                lift_unknown: false,
                clock_unknown: ppm.is_none(),
            },
            confidence: None,
            caveats: vec![],
            shape: None,
        }
    }

    fn assert_clock_document(ppm: Option<f64>, fitted: bool) {
        let summary = clock_summary(ppm, fitted);
        let raw = serde_json::to_value(&summary).unwrap();
        let doc = excerpt_document(&summary, &excerpt_analysis_config(Some(28800), 52.0)).unwrap();
        for (key, value) in raw.as_object().unwrap() {
            assert_eq!(
                &doc[key], value,
                "summary field {key} must remain unchanged"
            );
        }
        match ppm {
            Some(ppm) => {
                assert_eq!(doc["clock"]["ppm"], ppm);
                assert_eq!(
                    doc["clock"]["from"],
                    "from 3 USB lines in the recording, 0.1 ppm apart"
                );
                assert_eq!(doc["clock"]["rate_error_s_per_day"], ppm * 1e-6 * 86400.0);
                assert_eq!(
                    doc["clock"]["rate_s_per_day"],
                    fitted
                        .then(|| (1.0 + 12.0 / 86400.0) / (1.0 + ppm * 1e-6) * 86400.0 - 86400.0)
                        .map_or(Value::Null, |v| json!(v))
                );
            }
            None => assert!(doc.get("clock").is_none()),
        }
    }

    #[test]
    fn excerpt_clock_positive() {
        assert_clock_document(Some(100.0), true);
    }
    #[test]
    fn excerpt_clock_negative() {
        assert_clock_document(Some(-100.0), true);
    }
    #[test]
    fn excerpt_clock_zero() {
        assert_clock_document(Some(0.0), true);
    }
    #[test]
    fn excerpt_clock_without_rate() {
        assert_clock_document(Some(100.0), false);
    }
    #[test]
    fn excerpt_clock_absent() {
        assert_clock_document(None, true);
    }

    /// A report from a recording holds the excerpt cut where asked, the
    /// analysis `report --replay` runs again, the app's readings and the
    /// consent, all listed in the manifest with their checksums.
    #[test]
    fn a_report_holds_the_excerpt_and_its_analysis() {
        let dir = std::env::temp_dir().join(format!("tg-report-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("take.wav");
        let cfg = SynthConfig {
            duration_s: 40.0,
            rate_s_per_day: 10.0,
            ..Default::default()
        };
        let a = generate(&cfg, |_| 280.0, |_| 0.0);
        audio::write_wav(&wav, &a).unwrap();
        let src = Source {
            files: vec![wav.clone()],
            total_s: 40.0,
            name: "take.wav".into(),
            live: false,
            end_s: 40.0,
            about: Value::Null,
        };
        let clip = cut_clip(&src.files, 0.0).unwrap();
        assert!((clip.len_s() - 40.0).abs() < 0.01 && clip.outline.len() <= COLUMNS);
        let job = Job {
            zip: dir.join("r.zip"),
            created: "2026-10-10T23:00:00.000Z".into(),
            kind: Kind::Reading,
            note: "rate jumps".into(),
            source: Some(src),
            sound: Some(Excerpt {
                from_s: 5.0,
                span_s: 30.0,
            }),
            listened: true,
            cc0: false,
            shown: Shown {
                bph: Some(28800),
                lift_deg: 52.0,
                app: json!({ "rate_s_per_day": 10.0, "doubts": { "lift_unknown": true } }),
            },
        };
        job.write().unwrap();
        let files = archive::read_zip(&job.zip).unwrap();
        let get = |n: &str| &files.iter().find(|(p, _)| p == n).unwrap().1;
        let m: Value = serde_json::from_slice(get("manifest.json")).unwrap();
        assert_eq!(m["schema"], "timegrapher.report/1");
        assert_eq!(m["kind"], "reading");
        assert_eq!(m["consent"]["listened_back"], true);
        assert_eq!(m["consent"]["corpus_cc0"], false);
        assert_eq!(
            m["commands"][0]["args"],
            json!([
                "analyze",
                "audio/excerpt.flac",
                "--json",
                "--bph",
                "28800",
                "--lift",
                "52"
            ])
        );
        for f in m["files"].as_array().unwrap() {
            let d = get(f["path"].as_str().unwrap());
            assert_eq!(f["sha256"], archive::sha256_hex(d));
        }
        let x = excerpt::read(&{
            let p = dir.join("x.flac");
            std::fs::write(&p, get("audio/excerpt.flac")).unwrap();
            p
        })
        .unwrap();
        assert!((x.duration_s() - 30.0).abs() < 0.01);
        let r: Value = serde_json::from_slice(get("results/analyze-excerpt.json")).unwrap();
        assert_eq!(r["doubts"]["lift_unknown"], false);
        assert!(r["caveats"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["code"] != "lift_unknown"));
        let captured: Value = serde_json::from_slice(get("results/app.json")).unwrap();
        assert_eq!(
            captured, job.shown.app,
            "original app capture remains independent"
        );
        let rate = r["overall"]["rate_s_per_day"].as_f64().unwrap();
        assert!((rate - 10.0).abs() < 1.0, "{rate}");
        assert!(
            r.get("clock").is_none(),
            "ordinary fixture has no USB correction"
        );

        // A second bundle, sequentially: the temporary decoder filename is shared
        // within this process. Keep the original fixture and assertions above.
        let mut usb_audio = a.clone();
        for (i, sample) in usb_audio.samples.iter_mut().enumerate() {
            for f in [5000.0, 6000.0, 7000.0] {
                *sample += (0.01
                    * (std::f64::consts::TAU * f * (1.0 + 20e-6) * i as f64
                        / usb_audio.sample_rate as f64)
                        .sin()) as f32;
            }
        }
        audio::write_wav(&wav, &usb_audio).unwrap();
        let usb_job = Job {
            zip: dir.join("usb.zip"),
            ..job
        };
        usb_job.write().unwrap();
        let usb_files = archive::read_zip(&usb_job.zip).unwrap();
        let usb_get = |name: &str| &usb_files.iter().find(|(p, _)| p == name).unwrap().1;
        let usb_doc: Value =
            serde_json::from_slice(usb_get("results/analyze-excerpt.json")).unwrap();
        assert!(
            usb_doc["usb_clock"].is_object(),
            "USB fixture must yield an estimate"
        );
        assert_eq!(usb_doc["clock"]["ppm"], usb_doc["usb_clock"]["ppm"]);
        assert_eq!(
            usb_doc["clock"]["rate_s_per_day"],
            usb_doc["rate_corrected_s_per_day"]
        );
        assert_eq!(
            serde_json::from_slice::<Value>(usb_get("results/app.json")).unwrap(),
            usb_job.shown.app
        );
        let usb_manifest: Value = serde_json::from_slice(usb_get("manifest.json")).unwrap();
        assert_eq!(
            usb_manifest["commands"], m["commands"],
            "same replay settings"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_are_removed_as_whole_words() {
        assert_eq!(replace_word("peter's peterson", "peter", "<user>").1, 1);
    }
}
