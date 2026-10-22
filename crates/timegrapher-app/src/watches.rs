//! The Watches pane's parts: the library's watches as a list, one watch's
//! details and takes, and the form for a watch's details. The app owns
//! the library and acts on what these return.

use crate::app::fmt_bytes;
use crate::calibre_ui::Calibres;
use crate::photos::{Thumb, Thumbs};
use crate::theme;
use eframe::egui::{self, RichText, Sense, Vec2};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use timegrapher_core::library::{Field, FieldKind, Library, Take, Watch, WatchEntry};
use timegrapher_core::session::Position;

pub const HELP: &[&str] = &[
    "Your watches, each with its details and every take made of it, kept in the library \
     folder (Settings > Library says where). Pick a watch to see its takes and to start a \
     new one; Save… then saves the take into that watch, named by when it started.",
    "New Watch… adds a watch: only its name is needed. Its calibre, lift angle and beat rate \
     set up each new take of it, and can be changed for one take without changing the watch.",
    "Add Photos… copies photos of the picked watch into its folder, each kept as it is \
     (JPEG, PNG, TIFF, or HEIC as an iPhone takes them). They show as a strip under its \
     details, and the first by name leads its row in the list. Click one to open it in your \
     photo viewer, or right-click it to show its folder or delete it.",
    "Each take shows what it measured when it was saved, its length and the space it uses. \
     Clicking one picks it and shows its Open and Replay buttons; double-clicking opens it \
     as Open does, and the take already on show is not read again. Right-clicking it \
     offers Rename…, Keep Analysis \
     Only… (keeping what was measured and deleting the sound, to save space), Move to \
     Another Watch and Delete…. A take opened from outside the library can be copied or \
     moved into a watch with Add to a Watch…, beside Open….",
    "Quick Listen listens on the microphone without picking or adding a watch first, for a \
     watch handed over the counter. Its take is saved into Unsorted when you Stop (and \
     Save Again adds to it after a Resume), and waits there, listed at the top of the \
     watches, until you file it under a watch (right-click it, then File Under a Watch) \
     or delete it.",
    "The Microphone and Recording controls below work with a watch picked or without one; \
     without one, Save… asks where.",
];

/// How the list is sorted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sort {
    #[default]
    Name,
    LastTake,
    Size,
}

impl Sort {
    pub const ALL: [Sort; 3] = [Sort::Name, Sort::LastTake, Sort::Size];

    pub fn words(self) -> &'static str {
        match self {
            Sort::Name => "Name",
            Sort::LastTake => "Last Take",
            Sort::Size => "Size",
        }
    }
}

/// The time `utc` (RFC 3339) names, in this computer's time zone, as a
/// take folder is named: `2026-10-20 14.32`.
pub fn local_minute(utc: &str) -> Option<String> {
    let t = chrono::DateTime::parse_from_rfc3339(utc).ok()?;
    Some(
        t.with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H.%M")
            .to_string(),
    )
}

/// When a watch's newest take started, in UTC as `session.json` has it.
fn last_take(e: &WatchEntry) -> Option<&str> {
    e.takes
        .iter()
        .filter_map(|t| t.started_utc.as_deref())
        .max()
}

/// The watches to list: those matching `search` (in the name, the calibre,
/// the notes or a field's value, ignoring case), archived ones only when
/// asked for, in `sort`'s order.
pub fn shown<'a>(
    lib: &'a Library,
    search: &str,
    sort: Sort,
    archived: bool,
) -> Vec<&'a WatchEntry> {
    let key = search.trim().to_lowercase();
    let mut v: Vec<&WatchEntry> = lib
        .watches
        .iter()
        .filter(|e| archived || !e.watch.archived)
        .filter(|e| {
            let w = &e.watch;
            key.is_empty()
                || std::iter::once(w.name.as_str())
                    .chain(w.calibre.as_deref())
                    .chain(w.notes.as_deref())
                    .chain(w.fields.iter().map(|f| f.value.as_str()))
                    .any(|s| s.to_lowercase().contains(&key))
        })
        .collect();
    match sort {
        // The library keeps them by name.
        Sort::Name => {}
        Sort::LastTake => v.sort_by(|a, b| last_take(b).cmp(&last_take(a))),
        Sort::Size => v.sort_by_key(|e| std::cmp::Reverse(e.bytes())),
    }
    v
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// What leads a row in the list: nothing, when no watch listed has a
/// photo; else the watch's first photo, or a tile with its initial while
/// there is none (or it is still being read, or can't be).
#[derive(Clone, Copy)]
pub enum Lead<'a> {
    None,
    Photo(Option<&'a egui::TextureHandle>, &'a str),
}

/// A square tile `rect` holding `tex` cropped to a square, or the
/// initial of `name` on a plain tile.
fn tile(ui: &egui::Ui, rect: egui::Rect, tex: Option<&egui::TextureHandle>, name: &str) {
    let pal = theme::pal(ui);
    let p = ui.painter_at(rect);
    match tex {
        Some(t) => {
            let [w, h] = t.size().map(|v| v as f32);
            let (cw, ch) = if w > h { (h / w, 1.0) } else { (1.0, w / h) };
            let uv = egui::Rect::from_center_size(egui::pos2(0.5, 0.5), Vec2::new(cw, ch));
            egui::Image::new((t.id(), rect.size()))
                .uv(uv)
                .corner_radius(rect.width().min(rect.height()) * 0.15)
                .paint_at(ui, rect);
        }
        None => {
            p.rect_filled(rect, rect.width() * 0.15, pal.control);
            let initial: String = name.trim().chars().take(1).collect();
            p.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                initial.to_uppercase(),
                theme::semibold(rect.height() * 0.45),
                pal.text_secondary,
            );
        }
    }
}

/// A row that can be clicked: `top` and its `right` on one line, `below`
/// in small grey type under it, after its `lead`.
fn row(
    ui: &mut egui::Ui,
    picked: bool,
    lead: Lead,
    top: &str,
    right: &str,
    below: &str,
    hint: &str,
) -> egui::Response {
    let pal = theme::pal(ui);
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 40.0), Sense::click());
    if picked {
        ui.painter().rect_filled(rect, 6.0, pal.control_active);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, pal.control_hover);
    }
    let mut left = rect.left() + 8.0;
    if let Lead::Photo(tex, name) = lead {
        let r = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 4.0, rect.top() + 4.0),
            Vec2::splat(32.0),
        );
        tile(ui, r, tex, name);
        left = r.right() + 8.0;
    }
    let p = ui.painter_at(rect);
    p.text(
        egui::pos2(left, rect.top() + 12.0),
        egui::Align2::LEFT_CENTER,
        top,
        theme::semibold(13.0),
        pal.text,
    );
    p.text(
        egui::pos2(rect.right() - 8.0, rect.top() + 12.0),
        egui::Align2::RIGHT_CENTER,
        right,
        egui::FontId::proportional(12.0),
        pal.text_secondary,
    );
    p.text(
        egui::pos2(left, rect.top() + 29.0),
        egui::Align2::LEFT_CENTER,
        below,
        egui::FontId::proportional(11.5),
        pal.text_secondary,
    );
    resp.on_hover_text(hint)
}

/// The list: one row per watch, led by its first photo when any watch
/// listed has one. The id of the watch clicked, if any.
pub fn list(ui: &mut egui::Ui, watches: &[&WatchEntry], thumbs: &mut Thumbs) -> Option<String> {
    let mut picked = None;
    let photos = watches.iter().any(|e| !e.photos.is_empty());
    for e in watches {
        let w = &e.watch;
        let tex = e
            .photos
            .first()
            .and_then(|p| match thumbs.get(ui.ctx(), p) {
                Thumb::Ready(t) => Some(t.clone()),
                _ => None,
            });
        let lead = if photos {
            Lead::Photo(tex.as_ref(), &w.name)
        } else {
            Lead::None
        };
        let mut below = Vec::new();
        if let Some(c) = w.calibre.as_deref().filter(|c| !c.is_empty()) {
            below.push(c.to_string());
        }
        below.push(count(e.takes.len(), "take", "takes"));
        if let Some(t) = last_take(e).and_then(local_minute) {
            below.push(format!("last {}", &t[..10]));
        }
        if w.archived {
            below.push("Archived".into());
        }
        if row(
            ui,
            false,
            lead,
            &w.name,
            &fmt_bytes(e.bytes()),
            &below.join(" · "),
            "Show this watch's details and takes",
        )
        .clicked()
        {
            picked = Some(w.id.clone());
        }
    }
    picked
}

/// Unsorted's row, at the top of the list (led by a tile when the
/// watches' rows have photos): whether it was clicked.
pub fn unsorted_row(ui: &mut egui::Ui, e: &WatchEntry, tiled: bool) -> bool {
    row(
        ui,
        false,
        if tiled {
            Lead::Photo(None, &e.watch.name)
        } else {
            Lead::None
        },
        &e.watch.name,
        &fmt_bytes(e.bytes()),
        &format!(
            "{}, made with Quick Listen",
            count(e.takes.len(), "take", "takes")
        ),
        "Show the takes not yet filed under a watch",
    )
    .clicked()
}

/// What stands for a watch's details above Unsorted's takes.
pub fn unsorted_details(ui: &mut egui::Ui) {
    let pal = theme::pal(ui);
    ui.label(RichText::new(timegrapher_core::library::UNSORTED).font(theme::semibold(16.0)));
    ui.label(
        RichText::new(
            "Takes made with Quick Listen, kept here until you file each under a watch or \
             delete it.",
        )
        .color(pal.text_secondary),
    );
}

/// A watch's details, as rows.
pub fn details(ui: &mut egui::Ui, e: &WatchEntry) {
    let pal = theme::pal(ui);
    let w = &e.watch;
    ui.label(RichText::new(&w.name).font(theme::semibold(16.0)));
    let mut rows: Vec<(String, String)> = Vec::new();
    if let Some(c) = &w.calibre {
        rows.push(("Calibre".into(), c.clone()));
    }
    if let Some(l) = w.lift {
        rows.push(("Lift Angle".into(), format!("{l}°")));
    }
    if let Some(b) = w.bph {
        rows.push(("Beat Rate".into(), format!("{b} bph")));
    }
    for f in &w.fields {
        rows.push((f.label.clone(), f.value.clone()));
    }
    for (label, value) in rows {
        theme::row(ui, &label, None, |ui| ui.label(value));
    }
    if let Some(n) = w.notes.as_deref().filter(|n| !n.trim().is_empty()) {
        ui.label(RichText::new(n).color(pal.text_secondary));
    }
    if w.archived {
        ui.label(RichText::new("Archived: hidden from the list").color(pal.text_secondary));
    }
}

/// What to do with one of a watch's photos.
#[derive(Debug, Clone, PartialEq)]
pub enum PhotoAction {
    /// In the computer's own viewer.
    Open(PathBuf),
    /// Its folder, in the file manager.
    Show(PathBuf),
    Delete(PathBuf),
}

/// A watch's photos as a row of tiles, in name order (the first leads
/// the watch's row in the list); one that can't be read says so, with
/// its name, instead of a picture.
pub fn photo_strip(
    ui: &mut egui::Ui,
    e: &WatchEntry,
    thumbs: &mut Thumbs,
    writable: bool,
) -> Option<PhotoAction> {
    let mut act = None;
    let pal = theme::pal(ui);
    egui::ScrollArea::horizontal()
        .id_salt("photo strip")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for p in &e.photos {
                    let name = folder_label(p);
                    let (tex, failed) = match thumbs.get(ui.ctx(), p) {
                        Thumb::Ready(t) => (Some(t.clone()), None),
                        Thumb::Loading => (None, None),
                        Thumb::Failed(why) => (None, Some(why.clone())),
                    };
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(88.0), Sense::click());
                    let hint = match (&tex, &failed) {
                        (Some(_), _) => format!("{name}. Click to open it in your photo viewer."),
                        (None, Some(why)) => format!(
                            "{name} can't be shown here ({why}). Click to open it in your \
                             photo viewer."
                        ),
                        (None, None) => format!("{name}, being read"),
                    };
                    if tex.is_some() {
                        tile(ui, rect, tex.as_ref(), &name);
                    } else {
                        ui.painter().rect_filled(rect, 88.0 * 0.15, pal.control);
                        if failed.is_some() {
                            let galley = ui.painter().layout(
                                format!("{name}\nCan't be shown"),
                                egui::FontId::proportional(11.0),
                                pal.text_secondary,
                                rect.width() - 12.0,
                            );
                            let at = rect.center() - galley.size() / 2.0;
                            ui.painter().galley(at, galley, pal.text_secondary);
                        } else {
                            let r = egui::Rect::from_center_size(rect.center(), Vec2::splat(16.0));
                            ui.put(r, egui::Spinner::new().size(16.0).color(pal.text_secondary));
                        }
                    }
                    let resp = resp.on_hover_text(hint);
                    if resp.clicked() {
                        act = Some(PhotoAction::Open(p.clone()));
                    }
                    resp.context_menu(|ui| {
                        if ui
                            .button("Open")
                            .on_hover_text("Open this photo in your photo viewer")
                            .clicked()
                        {
                            act = Some(PhotoAction::Open(p.clone()));
                        }
                        if ui
                            .button(crate::settings_window::show_in_words())
                            .on_hover_text("Open the folder holding this watch's photos")
                            .clicked()
                        {
                            act = Some(PhotoAction::Show(p.clone()));
                        }
                        ui.separator();
                        if ui
                            .add_enabled(writable, egui::Button::new("Delete…"))
                            .on_hover_text("Delete this photo for good. It asks first.")
                            .on_disabled_hover_text(
                                "This copy of the app can only read the library",
                            )
                            .clicked()
                        {
                            act = Some(PhotoAction::Delete(p.clone()));
                        }
                    });
                }
            });
        });
    act
}

/// What to do with a take.
#[derive(Debug, Clone, PartialEq)]
pub enum TakeAction {
    /// Picked in the list, not opened.
    Pick(PathBuf),
    Open(PathBuf),
    Replay(PathBuf),
    Show(PathBuf),
    Rename(PathBuf),
    KeepAnalysis(PathBuf),
    /// To the watch of this id.
    MoveTo(PathBuf, String),
    Delete(PathBuf),
    /// Stop keeping a take as its analysis.
    CancelKeep,
}

/// What the takes list needs besides the watch: the other watches (id,
/// name) to move a take to, whether the library can be changed, the
/// take being kept as its analysis with how far along it is (0 to 1),
/// and whether these are Unsorted's takes, to be filed under a watch.
pub struct TakesContext<'a> {
    pub others: &'a [(String, String)],
    pub writable: bool,
    pub keeping: Option<(&'a std::path::Path, f32)>,
    pub unsorted: bool,
    /// The take picked in the list, and the one on show (open, or
    /// opening).
    pub picked: Option<&'a std::path::Path>,
    pub on_show: Option<&'a std::path::Path>,
}

/// What a take measured and is, in a line: its positions, length, rate,
/// amplitude, beat error (its size) and marks.
pub fn take_line(t: &Take) -> String {
    let mut parts: Vec<String> = Vec::new();
    let names: Vec<String> = t
        .positions
        .iter()
        .map(|p| Position::parse(p).map_or_else(|| p.clone(), |x| x.name().to_string()))
        .collect();
    if let Some(r) = &t.power_reserve {
        parts.push(format!(
            "Power Reserve {}{}",
            timegrapher_core::power_reserve::run_text(r.run_s),
            match r.ending {
                timegrapher_core::power_reserve::Ending::Stopped => "",
                _ => " (still running at the end)",
            }
        ));
    }
    if !names.is_empty() {
        parts.push(names.join(", "));
    }
    if let Some(d) = t.duration_s.filter(|_| t.power_reserve.is_none()) {
        parts.push(crate::strip::fmt_time(d));
    }
    if let Some(a) = &t.at_end {
        if let Some(r) = a.rate_s_per_day {
            parts.push(format!("{r:+.1} s/d"));
        }
        if let Some(x) = a.amplitude_deg {
            parts.push(format!("{x:.0}°"));
        }
        if let Some(b) = a.beat_error_ms {
            parts.push(format!("{:.1} ms", b.abs()));
        }
    }
    if !t.audio {
        parts.push("Analysis Only".into());
    }
    if t.incomplete {
        parts.push("Incomplete".into());
    }
    parts.join(" · ")
}

/// The watch's takes, newest first. What was asked for, if anything.
pub fn takes(ui: &mut egui::Ui, e: &WatchEntry, cx: &TakesContext) -> Option<TakeAction> {
    let pal = theme::pal(ui);
    if e.takes.is_empty() {
        ui.label(
            RichText::new(if cx.unsorted {
                "No takes here: each take Quick Listen makes waits here until it is filed."
            } else {
                "No takes yet: Start, then Save…, keeps one here."
            })
            .color(pal.text_secondary),
        );
        return None;
    }
    let mut act = None;
    for t in &e.takes {
        let dir = e.take_dir(t);
        let replays = t.audio && !t.incomplete;
        let on_show = cx.on_show == Some(dir.as_path());
        let picked = cx.picked == Some(dir.as_path());
        let r = row(
            ui,
            picked,
            Lead::None,
            &t.name,
            &if on_show {
                format!("On Show · {}", fmt_bytes(t.bytes))
            } else {
                fmt_bytes(t.bytes)
            },
            &take_line(t),
            if on_show {
                "This take is on show. Right-click for more."
            } else if t.incomplete {
                "Files this take lists are missing (still syncing, or copied in part): it is \
                 left as it is. Double-click to open what is there."
            } else {
                "Click to pick this take, double-click to open it. Right-click for more."
            },
        );
        if r.double_clicked() {
            act = Some(TakeAction::Open(dir.clone()));
        } else if r.clicked() {
            act = Some(TakeAction::Pick(dir.clone()));
        }
        // The picked take's own buttons: nothing opens until asked.
        if picked && !on_show {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                if ui
                    .button("Open")
                    .on_hover_text("Analyse the whole take; a long take takes a while to read")
                    .clicked()
                {
                    act = Some(TakeAction::Open(dir.clone()));
                }
                if ui
                    .add_enabled(replays, egui::Button::new("Replay"))
                    .on_hover_text("Play the take through as if live")
                    .on_disabled_hover_text("This take has no sound to replay")
                    .clicked()
                {
                    act = Some(TakeAction::Replay(dir.clone()));
                }
            });
        }
        if let Some((_, done)) = cx.keeping.filter(|(k, _)| *k == dir) {
            ui.horizontal(|ui| {
                ui.add(
                    egui::ProgressBar::new(done)
                        .desired_width(ui.available_width() - 70.0)
                        .text("Keeping its analysis"),
                )
                .on_hover_text(
                    "Reading the take's sound again for its analysis. Its sound is deleted \
                     only once the analysis is written and reads back.",
                );
                if ui
                    .button("Cancel")
                    .on_hover_text("Stop, and keep the take with its sound as it was")
                    .clicked()
                {
                    act = Some(TakeAction::CancelKeep);
                }
            });
        }
        let busy = cx.keeping.is_some_and(|(k, _)| k == dir);
        let changes = cx.writable && !busy;
        let why = if busy {
            "Wait until its analysis is kept, or Cancel"
        } else {
            "This copy of the app can only read the library"
        };
        r.context_menu(|ui| {
            if ui
                .button("Open")
                .on_hover_text("Analyse the whole take")
                .clicked()
            {
                act = Some(TakeAction::Open(dir.clone()));
            }
            if ui
                .add_enabled(replays, egui::Button::new("Replay"))
                .on_hover_text("Play the take through as if live")
                .on_disabled_hover_text("This take has no sound to replay")
                .clicked()
            {
                act = Some(TakeAction::Replay(dir.clone()));
            }
            if ui
                .button(crate::settings_window::show_in_words())
                .on_hover_text("Open the take's folder")
                .clicked()
            {
                act = Some(TakeAction::Show(dir.clone()));
            }
            ui.separator();
            if ui
                .add_enabled(changes, egui::Button::new("Rename…"))
                .on_hover_text("Give the take's folder another name")
                .on_disabled_hover_text(why)
                .clicked()
            {
                act = Some(TakeAction::Rename(dir.clone()));
            }
            if ui
                .add_enabled(
                    changes && t.audio && !t.incomplete && cx.keeping.is_none(),
                    egui::Button::new("Keep Analysis Only…"),
                )
                .on_hover_text(
                    "Keep what was measured and delete the sound, to save space. It asks \
                     first and says what is lost.",
                )
                .on_disabled_hover_text(if !t.audio {
                    "This take is already kept as its analysis alone"
                } else if t.incomplete {
                    "Files this take lists are missing"
                } else if cx.keeping.is_some() {
                    "Another take's analysis is being kept"
                } else {
                    why
                })
                .clicked()
            {
                act = Some(TakeAction::KeepAnalysis(dir.clone()));
            }
            let (move_words, move_hint, no_watch) = if cx.unsorted {
                (
                    "File Under a Watch",
                    "Move the take into a watch's takes",
                    "There is no watch yet: New Watch… adds one",
                )
            } else {
                (
                    "Move to Another Watch",
                    "Move the take into another watch's takes",
                    "There is no other watch to move it to",
                )
            };
            ui.add_enabled_ui(changes && !cx.others.is_empty(), |ui| {
                ui.menu_button(move_words, |ui| {
                    for (id, name) in cx.others {
                        if ui.button(name).clicked() {
                            act = Some(TakeAction::MoveTo(dir.clone(), id.clone()));
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text(move_hint)
                .on_disabled_hover_text(if cx.others.is_empty() { no_watch } else { why });
            });
            if ui
                .add_enabled(changes, theme::destructive(ui, "Delete…"))
                .on_hover_text("Delete the take and its sound. It asks first.")
                .on_disabled_hover_text(why)
                .clicked()
            {
                act = Some(TakeAction::Delete(dir.clone()));
            }
        });
    }
    act
}

/// A question about a take, in a window of its own.
#[derive(Debug, Clone)]
pub enum Ask {
    Rename {
        take: PathBuf,
        name: String,
    },
    Delete {
        take: PathBuf,
        bytes: u64,
    },
    /// Keep as its analysis alone: the take's size and its sound's.
    Keep {
        take: PathBuf,
        bytes: u64,
        sound: u64,
    },
    /// One of a watch's photos.
    DeletePhoto {
        photo: PathBuf,
    },
    /// A take from outside: into which watch (by id), copied or moved.
    Add {
        from: PathBuf,
        watch: Option<String>,
        keep_original: bool,
    },
}

fn folder_label(p: &std::path::Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The window asking `ask`: `Some(true)` when the user goes ahead,
/// `Some(false)` when they cancel.
pub fn ask_dialog(
    ctx: &egui::Context,
    ask: &mut Ask,
    watches: &[(String, String)],
) -> Option<bool> {
    let mut answer = None;
    let modal = egui::Modal::new(egui::Id::new("take question")).show(ctx, |ui| {
        ui.set_width(420.0);
        let pal = theme::pal(ui);
        let (go, ready, red) = match ask {
            Ask::Rename { take, name } => {
                ui.label(
                    RichText::new(format!("Rename \"{}\"", folder_label(take)))
                        .font(theme::semibold(15.0)),
                );
                ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY))
                    .on_hover_text("The take's new folder name; nothing relies on it");
                ("Rename", !name.trim().is_empty(), false)
            }
            Ask::Delete { take, bytes } => {
                ui.label(
                    RichText::new(format!("Delete \"{}\"?", folder_label(take)))
                        .font(theme::semibold(15.0)),
                );
                ui.label(format!(
                    "Its folder and everything in it, {}, are deleted. This can't be undone.",
                    fmt_bytes(*bytes)
                ));
                ("Delete", true, true)
            }
            Ask::DeletePhoto { photo } => {
                ui.label(
                    RichText::new(format!("Delete \"{}\"?", folder_label(photo)))
                        .font(theme::semibold(15.0)),
                );
                ui.label("The photo is deleted from the watch's photos. This can't be undone.");
                ("Delete", true, true)
            }
            Ask::Keep { take, bytes, sound } => {
                ui.label(
                    RichText::new(format!(
                        "Keep \"{}\" as Its Analysis Only?",
                        folder_label(take)
                    ))
                    .font(theme::semibold(15.0)),
                );
                ui.label(format!(
                    "The take uses {} now, {} of it its sound. Its sound is read once more \
                     for its analysis, which is written beside it and checked, and only then \
                     is the sound deleted.",
                    fmt_bytes(*bytes),
                    fmt_bytes(*sound)
                ));
                ui.label(
                    RichText::new(
                        "Without its sound the take can't be measured again (by a later \
                         version of the app, or with other settings), and Scope and Replay \
                         don't work on it. The readings, charts, Steadiness and the Positions \
                         Test still do. This can't be undone.",
                    )
                    .color(pal.text_secondary),
                );
                ("Keep Analysis Only", true, true)
            }
            Ask::Add {
                from,
                watch,
                keep_original,
            } => {
                ui.label(
                    RichText::new(format!("Add \"{}\" to a Watch", folder_label(from)))
                        .font(theme::semibold(15.0)),
                );
                let shown = watch
                    .as_ref()
                    .and_then(|id| watches.iter().find(|(w, _)| w == id))
                    .map_or("Choose a Watch", |(_, n)| n.as_str());
                theme::row(ui, "Watch", Some("The watch the take is added to"), |ui| {
                    egui::ComboBox::from_id_salt("add to watch")
                        .width(ui.available_width())
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            for (id, name) in watches {
                                ui.selectable_value(watch, Some(id.clone()), name);
                            }
                        });
                });
                theme::row(ui, "", None, |ui| {
                    theme::segmented(
                        ui,
                        keep_original,
                        &[
                            (
                                true,
                                "Copy",
                                "Copy the take in; the original stays where it is",
                            ),
                            (
                                false,
                                "Move",
                                "Move the take in; the original is gone from where it was",
                            ),
                        ],
                    );
                });
                ("Add", watch.is_some(), false)
            }
        };
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let b = if red {
                theme::destructive(ui, go)
            } else {
                theme::primary(ui, go)
            };
            if ui.add_enabled(ready, b).on_hover_text("Go ahead").clicked() {
                answer = Some(true);
            }
            if ui
                .add(theme::secondary(ui, "Cancel"))
                .on_hover_text("Close without changing anything")
                .clicked()
            {
                answer = Some(false);
            }
        });
    });
    if modal.should_close() && answer.is_none() {
        answer = Some(false);
    }
    answer
}

/// A watch's details being written: a new watch or one being edited.
#[derive(Debug, Clone)]
pub struct Form {
    pub watch: Watch,
    pub new: bool,
    pub error: Option<String>,
}

/// What the form asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormAction {
    Save,
    Cancel,
}

/// What a suggested field holds.
pub fn suggested_kind(label: &str) -> FieldKind {
    match label {
        "Year" => FieldKind::Number,
        "Purchased" | "Received" | "Returned" => FieldKind::Date,
        "Purchase Price" => FieldKind::Money,
        _ => FieldKind::Text,
    }
}

fn kind_words(k: FieldKind) -> &'static str {
    match k {
        FieldKind::Text => "Text",
        FieldKind::Date => "Date",
        FieldKind::Number => "Number",
        FieldKind::Money => "Money",
    }
}

/// The standard fields that can be removed and added back.
#[derive(Clone, Copy)]
enum Standard {
    Calibre,
    LiftAngle,
    BeatRate,
    Notes,
}

impl Standard {
    const ALL: [Standard; 4] = [
        Standard::Calibre,
        Standard::LiftAngle,
        Standard::BeatRate,
        Standard::Notes,
    ];

    fn words(self) -> &'static str {
        match self {
            Standard::Calibre => "Calibre",
            Standard::LiftAngle => "Lift Angle",
            Standard::BeatRate => "Beat Rate",
            Standard::Notes => "Notes",
        }
    }

    fn shown(self, w: &Watch) -> bool {
        match self {
            Standard::Calibre => w.calibre.is_some(),
            Standard::LiftAngle => w.lift.is_some(),
            Standard::BeatRate => w.bph.is_some(),
            Standard::Notes => w.notes.is_some(),
        }
    }

    fn add(self, w: &mut Watch) {
        match self {
            Standard::Calibre => w.calibre = Some(String::new()),
            Standard::LiftAngle => w.lift = Some(52.0),
            Standard::BeatRate => w.bph = Some(28800),
            Standard::Notes => w.notes = Some(String::new()),
        }
    }

    fn remove(self, w: &mut Watch) {
        match self {
            Standard::Calibre => w.calibre = None,
            Standard::LiftAngle => w.lift = None,
            Standard::BeatRate => w.bph = None,
            Standard::Notes => w.notes = None,
        }
    }
}

/// A small "×" that removes a field.
fn remove_button(ui: &mut egui::Ui, what: &str) -> bool {
    ui.small_button("×")
        .on_hover_text(format!("Remove {what} from this watch"))
        .clicked()
}

/// The form for a watch's details, in a window of its own.
pub fn form(ctx: &egui::Context, f: &mut Form, cal: &mut Calibres) -> Option<FormAction> {
    let mut act = None;
    let modal = egui::Modal::new(egui::Id::new("watch form")).show(ctx, |ui| {
        ui.set_width(440.0);
        let pal = theme::pal(ui);
        ui.label(
            RichText::new(if f.new { "New Watch" } else { "Edit Watch" })
                .font(theme::semibold(15.0)),
        );
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .max_height(420.0)
            .show(ui, |ui| {
                ui.push_id("watch form fields", |ui| fields(ui, &mut f.watch, cal))
            });
        if let Some(e) = &f.error {
            ui.label(RichText::new(e).color(pal.bad));
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !f.watch.name.trim().is_empty(),
                    theme::primary(ui, if f.new { "Add Watch" } else { "Save" }),
                )
                .on_hover_text("Write these details into the watch's folder")
                .on_disabled_hover_text("A watch needs a name")
                .clicked()
            {
                act = Some(FormAction::Save);
            }
            if ui
                .add(theme::secondary(ui, "Cancel"))
                .on_hover_text("Close without changing anything")
                .clicked()
            {
                act = Some(FormAction::Cancel);
            }
        });
    });
    if modal.should_close() && act.is_none() {
        act = Some(FormAction::Cancel);
    }
    act
}

fn fields(ui: &mut egui::Ui, w: &mut Watch, cal: &mut Calibres) {
    let pal = theme::pal(ui);
    theme::row(
        ui,
        "Name",
        Some("Shown everywhere; no two watches share one"),
        |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut w.name)
                    .hint_text("Make and model, or what you call it")
                    .desired_width(f32::INFINITY),
            );
        },
    );
    if let Some(current) = w.calibre.clone() {
        let mut gone = false;
        let picked = theme::row(
            ui,
            "Calibre",
            Some("The movement: it fills the lift angle and beat rate below"),
            |ui| {
                ui.horizontal(|ui| {
                    gone = remove_button(ui, "the calibre");
                    cal.picker(ui, &current)
                })
                .inner
            },
        );
        if let Some(p) = picked {
            if let Some(l) = cal.lift(&p) {
                w.lift = Some(l);
            }
            if let Some(b) = cal.bph(&p) {
                w.bph = Some(b);
            }
            w.calibre = Some(p);
        }
        if gone {
            Standard::Calibre.remove(w);
        }
    }
    if let Some(mut lift) = w.lift {
        let mut gone = false;
        theme::row(
            ui,
            "Lift Angle",
            Some("Degrees; each new take of this watch starts with it"),
            |ui| {
                gone = remove_button(ui, "the lift angle");
                ui.add(
                    egui::DragValue::new(&mut lift)
                        .range(10.0..=90.0)
                        .suffix("°"),
                );
            },
        );
        w.lift = (!gone).then_some(lift);
    }
    if let Some(mut bph) = w.bph {
        let mut gone = false;
        theme::row(
            ui,
            "Beat Rate",
            Some("Beats per hour; each new take of this watch starts with it"),
            |ui| {
                gone = remove_button(ui, "the beat rate");
                egui::ComboBox::from_id_salt("form bph")
                    .selected_text(format!("{bph} bph"))
                    .show_ui(ui, |ui| {
                        for b in timegrapher_core::beats::STANDARD_BPH {
                            ui.selectable_value(&mut bph, b, format!("{b} bph"));
                        }
                    });
            },
        );
        w.bph = (!gone).then_some(bph);
    }
    let mut remove = None;
    let mut up = None;
    for (i, f) in w.fields.iter_mut().enumerate() {
        ui.push_id(("field", i), |ui| {
            ui.horizontal(|ui| {
                if remove_button(ui, "this field") {
                    remove = Some(i);
                }
                ui.add(
                    egui::TextEdit::singleline(&mut f.label)
                        .hint_text("Label")
                        .desired_width(110.0),
                )
                .on_hover_text("The field's name");
                ui.add(
                    egui::TextEdit::singleline(&mut f.value)
                        .hint_text(match f.kind {
                            FieldKind::Text => "Value",
                            FieldKind::Date => "2026-10-21",
                            FieldKind::Number => "Number",
                            FieldKind::Money => "Amount and currency",
                        })
                        .desired_width(160.0),
                )
                .on_hover_text("Kept as typed");
                egui::ComboBox::from_id_salt("kind")
                    .width(70.0)
                    .selected_text(kind_words(f.kind))
                    .show_ui(ui, |ui| {
                        for k in [
                            FieldKind::Text,
                            FieldKind::Date,
                            FieldKind::Number,
                            FieldKind::Money,
                        ] {
                            ui.selectable_value(&mut f.kind, k, kind_words(k));
                        }
                    })
                    .response
                    .on_hover_text("What the field holds");
                if i > 0
                    && ui
                        .small_button("↑")
                        .on_hover_text("Move this field up")
                        .clicked()
                {
                    up = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        w.fields.remove(i);
    }
    if let Some(i) = up {
        w.fields.swap(i - 1, i);
    }
    if let Some(mut notes) = w.notes.clone() {
        let mut gone = false;
        ui.horizontal(|ui| {
            gone = remove_button(ui, "the notes");
            ui.label("Notes");
        });
        ui.add(
            egui::TextEdit::multiline(&mut notes)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        )
        .on_hover_text("Anything about the watch, kept as typed");
        w.notes = (!gone).then_some(notes);
    }
    ui.horizontal(|ui| {
        ui.menu_button("Add Field", |ui| {
            let gone: Vec<Standard> = Standard::ALL.into_iter().filter(|s| !s.shown(w)).collect();
            for s in gone {
                if ui.button(s.words()).clicked() {
                    s.add(w);
                    ui.close();
                }
            }
            for label in timegrapher_core::library::SUGGESTED_FIELDS {
                if w.fields.iter().any(|f| f.label == label) {
                    continue;
                }
                if ui.button(label).clicked() {
                    w.fields.push(Field {
                        label: label.into(),
                        kind: suggested_kind(label),
                        value: String::new(),
                    });
                    ui.close();
                }
            }
            ui.separator();
            if ui
                .button("Your Own…")
                .on_hover_text("A blank field to name yourself")
                .clicked()
            {
                w.fields.push(Field {
                    label: String::new(),
                    kind: FieldKind::Text,
                    value: String::new(),
                });
                ui.close();
            }
        })
        .response
        .on_hover_text("Add a field: a suggested one, or one of your own");
        let mut hidden = w.archived;
        if ui
            .checkbox(&mut hidden, "Archived")
            .on_hover_text("Hide this watch from the list without deleting it")
            .changed()
        {
            w.archived = hidden;
        }
    });
    ui.label(
        RichText::new("Only the name is needed; every other field can be removed.")
            .small()
            .color(pal.text_secondary),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_take_line_says_what_it_measured_with_beat_error_as_a_size() {
        let t = Take {
            name: "2026-10-20 14.32 Dial Up".into(),
            bytes: 1_900_000,
            audio: false,
            incomplete: true,
            started_utc: Some("2026-10-20T21:32:00Z".into()),
            duration_s: Some(120.0),
            positions: vec!["DU".into(), "CL".into()],
            at_end: Some(timegrapher_core::library::AtEnd {
                rate_s_per_day: Some(3.5),
                amplitude_deg: Some(281.0),
                beat_error_ms: Some(-0.4),
            }),
            power_reserve: None,
        };
        let line = take_line(&t);
        assert!(line.starts_with("Dial Up, Crown Left · "), "{line}");
        assert!(
            line.contains("+3.5 s/d · 281° · 0.4 ms · Analysis Only · Incomplete"),
            "{line}"
        );
        // A Power Reserve take leads with its run, not its length.
        let r = Take {
            positions: vec!["CL".into()],
            power_reserve: Some(timegrapher_core::power_reserve::Summary {
                run_s: 166_320.0,
                ending: timegrapher_core::power_reserve::Ending::Stopped,
                position: "CL".into(),
            }),
            ..t.clone()
        };
        let line = take_line(&r);
        assert!(
            line.starts_with("Power Reserve 46 h 12 min · Crown Left · +3.5"),
            "{line}"
        );
        assert!(local_minute("2026-10-20T21:32:00Z").is_some_and(|m| m.len() == 16));
        assert_eq!(local_minute("not a time"), None);
    }
}
