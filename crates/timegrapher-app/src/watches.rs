//! The Watches view's parts: the library's watches as a list, one watch's
//! details and takes, and the form for a watch's details. The app owns
//! the library and acts on what these return.

use crate::app::{fmt_bytes, DEFAULT_LIFT_DEG};
use crate::calibre_ui::Calibres;
use crate::fields;
use crate::photos::{Thumb, Thumbs};
use crate::theme::{self, DialogButton, Role};
use eframe::egui::{self, RichText, Sense, Vec2};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use timegrapher_core::library::{Field, FieldKind, Library, Take, Watch, WatchEntry};
use timegrapher_core::session::Position;

pub const HELP: &[&str] = &[
    "Watches is the window's second view: the switch at the top left, or View > Watches \
     (Ctrl+2, Cmd+2 on a Mac), shows it, and Bench (Ctrl+1) goes back. A take keeps running \
     behind it. Open on a take, or starting one from a watch, goes to the Bench.",
    "Your watches, each with its details and every take made of it, kept in the Watches \
     folder (Settings > General says where). Pick a watch to see its takes and to start a \
     new one; Save… then saves the take into that watch, named by when it started.",
    "New Watch… adds a watch: only its name is needed. Its calibre, lift angle and beat rate \
     set up each new take of it, and can be changed for one take without changing the watch.",
    "Add Photos… copies photos of the picked watch into its folder, each kept as it is \
     (JPEG, PNG, TIFF, or HEIC as an iPhone takes them). They show as a strip under its \
     details, and the first by name leads its row in the list. Click one to open it in your \
     photo viewer, or right-click it to show its folder or delete it.",
    "Each take is a row of the Takes table: when it started, its positions, its length, \
     what it measured when it was saved, and the space it uses. Clicking a row picks it; \
     Open, at the right of the Takes title, or a double click opens it on the Bench, and \
     the take already on show is not read again. Right-clicking it, or …, offers Keep \
     Analysis Only… (keeping what was measured and deleting the sound, to save space), \
     Move to Another Watch and Delete…. Open Recording… and Open Folder… in the File menu \
     ask whether to add a recording from outside your watches to one of them, asking for \
     anything its files don't say, or just to open it.",
    "Pick a watch, then Listen, Positions Test or Power Reserve Test: each is a new take \
     on the microphone, saved into the watch when it stops.",
    "Quick Listen (the Listen button on the Listen card, with no watch picked) keeps nothing \
     until you Save it: into a watch, or into a folder. Its sound goes without a question \
     when you quit or start another take. A take saved into Unsorted is listed at the top \
     of the watches until you file it under a watch (right-click it, then File Under a \
     Watch) or delete it.",
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
/// the notes or a field's value, ignoring case), in `sort`'s order. A
/// watch an older version marked Archived is listed like any other.
pub fn shown<'a>(lib: &'a Library, search: &str, sort: Sort) -> Vec<&'a WatchEntry> {
    let key = search.trim().to_lowercase();
    let mut v: Vec<&WatchEntry> = lib
        .watches
        .iter()
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
    below: &[String],
    hint: &str,
) -> egui::Response {
    found_row(ui, picked, lead, top, right, below, hint, "")
}

/// Where `key` is found in `text`, ignoring case, as byte ranges of
/// `text`, in order and not overlapping. Nothing for an empty key.
pub fn found_in(text: &str, key: &str) -> Vec<std::ops::Range<usize>> {
    let key: Vec<char> = key.chars().flat_map(char::to_lowercase).collect();
    if key.is_empty() {
        return Vec::new();
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        // Match char by char, each lowered, so the ranges stay on
        // `text`'s own boundaries.
        let mut lowered = Vec::new();
        let mut j = i;
        while j < chars.len() && lowered.len() < key.len() {
            lowered.extend(chars[j].1.to_lowercase());
            j += 1;
        }
        if lowered == key {
            let end = chars.get(j).map_or(text.len(), |c| c.0);
            found.push(chars[i].0..end);
            i = j;
        } else {
            i += 1;
        }
    }
    found
}

/// `text` in `font` and `color`, with what `key` finds in it in `accent`.
fn found_job(
    text: &str,
    key: &str,
    font: egui::FontId,
    color: egui::Color32,
    accent: egui::Color32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let mut at = 0;
    let add = |job: &mut egui::text::LayoutJob, s: &str, c| {
        job.append(s, 0.0, egui::TextFormat::simple(font.clone(), c));
    };
    for r in found_in(text, key) {
        add(&mut job, &text[at..r.start], color);
        add(&mut job, &text[r.clone()], accent);
        at = r.end;
    }
    add(&mut job, &text[at..], color);
    job
}

/// A list row, what the search found in its name and the line under it
/// in the accent colour.
#[allow(clippy::too_many_arguments)]
fn found_row(
    ui: &mut egui::Ui,
    picked: bool,
    lead: Lead,
    top: &str,
    right: &str,
    below: &[String],
    hint: &str,
    key: &str,
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
    let size = p.text(
        egui::pos2(rect.right() - 8.0, rect.top() + 12.0),
        egui::Align2::RIGHT_CENTER,
        right,
        egui::FontId::proportional(12.0),
        pal.text_secondary,
    );
    // Nothing runs off the row: the name stops short of the size, cut with
    // "…", and the line under it keeps only the parts that fit, in order.
    let name_room = (size.left() - 8.0 - left).max(0.0);
    let name = cut(
        ui,
        found_job(top, key, theme::semibold(13.0), pal.text, pal.accent),
        name_room,
    );
    p.galley(
        egui::pos2(left, rect.top() + 12.0 - name.size().y / 2.0),
        name,
        pal.text,
    );
    let room = (rect.right() - 8.0 - left).max(0.0);
    let small = egui::FontId::proportional(11.5);
    let line = fitted(ui, below, &small, room);
    let line = cut(
        ui,
        found_job(&line, key, small, pal.text_secondary, pal.accent),
        room,
    );
    p.galley(
        egui::pos2(left, rect.top() + 29.0 - line.size().y / 2.0),
        line,
        pal.text_secondary,
    );
    resp.on_hover_text(hint)
}

/// `job` on one line no wider than `width`, cut short with "…".
fn cut(ui: &egui::Ui, mut job: egui::text::LayoutJob, width: f32) -> std::sync::Arc<egui::Galley> {
    job.wrap = egui::text::TextWrapping {
        max_width: width,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.fonts_mut(|f| f.layout_job(job))
}

/// `parts` joined by " · ", keeping as many from the first as fit in
/// `width` whole; the first is kept always, to be cut short if need be.
fn fitted(ui: &egui::Ui, parts: &[String], font: &egui::FontId, width: f32) -> String {
    let wide = |t: &str| {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(t.to_owned(), font.clone(), egui::Color32::WHITE)
                .size()
                .x
        })
    };
    let mut line = String::new();
    for (i, part) in parts.iter().enumerate() {
        let next = if i == 0 {
            part.clone()
        } else {
            format!("{line} · {part}")
        };
        if i > 0 && wide(&next) > width {
            break;
        }
        line = next;
    }
    line
}

/// A watch's size in the list, once it is a megabyte or more: an empty
/// folder's "0 bytes" says nothing.
fn list_size(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        fmt_bytes(bytes)
    } else {
        String::new()
    }
}

/// The list: one row per watch, led by its first photo when any watch
/// listed has one. The id of the watch clicked, if any.
pub fn list(
    ui: &mut egui::Ui,
    watches: &[&WatchEntry],
    marked: Option<&str>,
    thumbs: &mut Thumbs,
    shows: Option<&str>,
    search: &str,
) -> Option<String> {
    let key = search.trim();
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
        let shown = match shows {
            Some(label) => w
                .fields
                .iter()
                .find(|f| f.label == label && !f.value.trim().is_empty())
                .map(|f| format!("{label}: {}", f.value)),
            None => w.calibre.clone().filter(|c| !c.is_empty()),
        };
        if let Some(c) = shown {
            below.push(c);
        }
        below.push(count(e.takes.len(), "take", "takes"));
        if let Some(t) = last_take(e).and_then(local_minute) {
            below.push(format!("last {}", &t[..10]));
        }
        if found_row(
            ui,
            marked == Some(w.id.as_str()),
            lead,
            &w.name,
            &list_size(e.bytes()),
            &below,
            "Show this watch's details and takes",
            key,
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
pub fn unsorted_row(ui: &mut egui::Ui, e: &WatchEntry, tiled: bool, marked: bool) -> bool {
    row(
        ui,
        marked,
        if tiled {
            Lead::Photo(None, UNSORTED_TAKES)
        } else {
            Lead::None
        },
        UNSORTED_TAKES,
        &list_size(e.bytes()),
        &[
            "Quick Listen, no watch".to_string(),
            count(e.takes.len(), "take", "takes"),
        ],
        "Quick Listen again: no watch, no calibre, the default lift angle and the beat \
         rate found from the beats. Shows the takes not yet filed under a watch.",
    )
    .clicked()
}

/// The list's row for Quick Listen and the takes it kept.
pub const UNSORTED_TAKES: &str = "Unsorted Takes";

/// What stands for a watch's details above Unsorted's takes.
pub fn unsorted_details(ui: &mut egui::Ui) {
    let pal = theme::pal(ui);
    ui.label(RichText::new(UNSORTED_TAKES).font(theme::semibold(16.0)));
    ui.label(
        RichText::new(
            "Quick Listen: no watch, no calibre, the default lift angle and the beat rate \
             found from the beats. Its takes are kept here until you file each under a \
             watch or delete it.",
        )
        .color(pal.text_secondary),
    );
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
                                "This copy of the app can only read your watches",
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
    Show(PathBuf),
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

/// Whether the take in `dir` is a Positions Test's: it keeps the test's
/// results beside its sound.
pub fn is_positions_test(dir: &std::path::Path) -> bool {
    dir.join(crate::guided::RESULTS_FILE).is_file()
}

/// What a take measured and is, in a line: a test's kind, its positions,
/// length, rate, amplitude, beat error (its size) and marks.
pub fn take_line(t: &Take, positions_test: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    if positions_test {
        parts.push("Positions Test".into());
    }
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
        if let Some(r) = a.rate() {
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

/// The captions under the three New Take buttons: Listen, Positions Test, Power Reserve Test.
pub const NEW_TAKE_CAPTIONS: [&str; 3] = [
    "As long as you like",
    "Six positions, guided",
    "From full wind to a stop",
];

/// The words under a watch's name in its header: its calibre, beat rate
/// and lift angle, then its added fields, each named. The beat rate and
/// lift angle are the ones its takes start with: the calibre's where the
/// table gives them, else the watch's own ([`Calibres::watch_bph`]).
pub fn subline(w: &Watch, cal: &Calibres) -> String {
    let mut parts: Vec<String> = Vec::new();
    match w.calibre.as_deref().filter(|c| !c.is_empty()) {
        Some(c) => parts.push(c.to_string()),
        None => parts.push("Calibre not set".into()),
    }
    if let Some(b) = cal.watch_bph(w) {
        parts.push(format!("{b} bph"));
    }
    if let Some(l) = cal.watch_lift(w) {
        parts.push(format!("lift {}°", fields::plain(l)));
    }
    for f in w.fields.iter().filter(|f| !f.value.trim().is_empty()) {
        parts.push(format!("{}: {}", f.label, f.value));
    }
    parts.join(" · ")
}

/// What was asked in a watch's header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderAction {
    Edit,
    AddPhotos,
}

/// A watch's header: its first photo (or its initial), its name and
/// [`subline`], and Edit… and Add Photos… at the right, greyed out with
/// `locked`'s reason when the library can't be changed.
pub fn header(
    ui: &mut egui::Ui,
    e: &WatchEntry,
    cal: &Calibres,
    thumbs: &mut Thumbs,
    locked: Option<&str>,
) -> Option<HeaderAction> {
    let pal = theme::pal(ui);
    let w = &e.watch;
    let tex = e
        .photos
        .first()
        .and_then(|p| match thumbs.get(ui.ctx(), p) {
            Thumb::Ready(t) => Some(t.clone()),
            _ => None,
        });
    let mut act = None;
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
        tile(ui, rect, tex.as_ref(), &w.name);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(&w.name).font(theme::semibold(16.0)));
            ui.add(
                egui::Label::new(
                    RichText::new(subline(w, cal))
                        .small()
                        .color(pal.text_secondary),
                )
                .truncate(),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ok = locked.is_none();
            let why = locked.unwrap_or_default();
            if ui
                .add_enabled(ok, theme::secondary(ui, "Add Photos…"))
                .on_hover_text(
                    "Copy photos of this watch into its folder (JPEG, PNG, HEIC from an \
                     iPhone, or TIFF). The first by name leads its row in the list.",
                )
                .on_disabled_hover_text(why)
                .clicked()
            {
                act = Some(HeaderAction::AddPhotos);
            }
            if ui
                .add_enabled(ok, theme::secondary(ui, "Edit…"))
                .on_hover_text("Change this watch's details")
                .on_disabled_hover_text(why)
                .clicked()
            {
                act = Some(HeaderAction::Edit);
            }
        });
    });
    act
}

/// A section's title row, always one control tall so nothing moves when
/// what is at its right comes and goes: `title`, a `count` after it, and
/// `right` laid out from the right edge.
pub fn title_row(
    ui: &mut egui::Ui,
    title: &str,
    count: Option<usize>,
    right: impl FnOnce(&mut egui::Ui),
) {
    let pal = theme::pal(ui);
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), theme::CONTROL_H),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(Vec2::new(ui.available_width(), theme::CONTROL_H));
            ui.label(RichText::new(title).font(theme::semibold(13.0)));
            if let Some(n) = count {
                ui.label(RichText::new(n.to_string()).color(pal.text_secondary));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
        },
    );
}

/// A rounded panel for a section's content.
pub fn panel<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let pal = theme::pal(ui);
    egui::Frame::new()
        .fill(pal.raised.gamma_multiply(0.6))
        .corner_radius(egui::CornerRadius::same(theme::CONTROL_RADIUS + 2))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// New Take's three buttons, equal, each with its caption under it; Listen,
/// the default, is the blue one. Which was pressed, if any. While `why`
/// says so, they are greyed out with that reason.
pub fn new_take_buttons(
    ui: &mut egui::Ui,
    names: [(&str, &str); 3],
    why: Option<&str>,
) -> Option<usize> {
    let pal = theme::pal(ui);
    let mut pressed = None;
    let gap = 10.0;
    let w = ((ui.available_width() - 2.0 * gap) / 3.0).max(80.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (i, (name, hint)) in names.into_iter().enumerate() {
            ui.allocate_ui_with_layout(
                Vec2::new(w, 64.0),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.set_width(w);
                    panel(ui, |ui| {
                        let b = if i == 0 {
                            theme::primary(ui, name)
                        } else {
                            theme::secondary(ui, name)
                        };
                        let r = ui
                            .add_enabled(
                                why.is_none(),
                                b.min_size(Vec2::new(ui.available_width(), 26.0)),
                            )
                            .on_hover_text(hint)
                            .on_disabled_hover_text(why.unwrap_or_default());
                        if r.clicked() {
                            pressed = Some(i);
                        }
                        // One line, so the three buttons stay level.
                        ui.vertical_centered(|ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(NEW_TAKE_CAPTIONS[i])
                                        .small()
                                        .color(pal.text_secondary),
                                )
                                .truncate(),
                            );
                        });
                    });
                },
            );
        }
    });
    pressed
}

/// A take's start in its row: `22 Oct 2026 09:10`, in this computer's
/// time zone; its folder's name when `session.json` doesn't say.
pub fn started_words(t: &Take) -> String {
    t.started_utc
        .as_deref()
        .and_then(|u| chrono::DateTime::parse_from_rfc3339(u).ok())
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%-d %b %Y %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| t.name.clone())
}

/// A take's row in the takes table: Started, Position, Length, Rate (s/d,
/// on the take's clock log where it has one), Amplitude (°), Beat Error
/// (ms, its size) and Size (MB).
pub fn take_cells(t: &Take, positions_test: bool) -> [String; 7] {
    let codes: Vec<String> = t
        .positions
        .iter()
        .map(|p| Position::parse(p).map_or_else(|| p.clone(), |x| x.common().to_string()))
        .collect();
    let position = if t.power_reserve.is_some() {
        "Power Reserve".to_string()
    } else if positions_test {
        // The test's kind; its positions are in the row's hover.
        "Positions Test".to_string()
    } else if codes.len() == 6 {
        "All 6".to_string()
    } else {
        codes.join(", ")
    };
    let length = match (&t.power_reserve, t.duration_s) {
        // One format down the column, a Power Reserve Test's run included.
        (Some(r), _) => crate::strip::fmt_time(r.run_s),
        (None, Some(d)) => crate::strip::fmt_time(d),
        _ => String::new(),
    };
    let a = t.at_end.as_ref();
    [
        started_words(t),
        position,
        length,
        a.and_then(|a| a.rate())
            .map_or_else(String::new, |r| format!("{r:+.1}").replace('-', "−")),
        a.and_then(|a| a.amplitude_deg)
            .map_or_else(String::new, |x| format!("{x:.0}")),
        a.and_then(|a| a.beat_error_ms)
            .map_or_else(String::new, |b| format!("{:.1}", b.abs())),
        format!("{:.1}", t.bytes as f64 / 1e6),
    ]
}

/// The takes table's headings, in [`take_cells`]' order.
pub const TAKE_HEADS: [&str; 7] = [
    "Started",
    "Position",
    "Length",
    "Rate (s/d)",
    "Amplitude (°)",
    "Beat Error (ms)",
    "Size (MB)",
];

/// The take picked in `e`'s takes, if it is still there.
pub fn picked_take<'a>(e: &'a WatchEntry, cx: &TakesContext) -> Option<&'a Take> {
    e.takes
        .iter()
        .find(|t| cx.picked == Some(e.take_dir(t).as_path()))
}

/// The Takes title row's Open and "…", acting on the picked take: Open
/// greyed out while `busy` says why (a take running on the Bench) or none
/// is picked.
pub fn takes_buttons(
    ui: &mut egui::Ui,
    e: &WatchEntry,
    cx: &TakesContext,
    busy: Option<&str>,
) -> Option<TakeAction> {
    let mut act = None;
    let picked = picked_take(e, cx);
    let r = ui.add_enabled(picked.is_some(), theme::secondary(ui, "…"));
    let r = r
        .on_hover_text("More for the picked take: Show in the folder, Keep Analysis Only…, move it, or delete it")
        .on_disabled_hover_text("Pick a take first");
    if let Some(t) = picked {
        egui::Popup::menu(&r)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
            .show(|ui| {
                if let Some(a) = take_menu(ui, e, t, cx, busy) {
                    act = Some(a);
                }
            });
    }
    let why = busy.or(picked.is_none().then_some("Pick a take first"));
    if ui
        .add_enabled(why.is_none(), theme::secondary(ui, "Open"))
        .on_hover_text(
            "Analyse the whole picked take on the Bench; a long take takes a while to read",
        )
        .on_disabled_hover_text(why.unwrap_or_default())
        .clicked()
    {
        if let Some(t) = picked {
            act = Some(TakeAction::Open(e.take_dir(t)));
        }
    }
    act
}

/// A take's menu, for its right click and the Takes row's "…".
fn take_menu(
    ui: &mut egui::Ui,
    e: &WatchEntry,
    t: &Take,
    cx: &TakesContext,
    busy: Option<&str>,
) -> Option<TakeAction> {
    let dir = e.take_dir(t);
    let mut act = None;
    let removing = cx.keeping.is_some_and(|(k, _)| k == dir);
    let changes = cx.writable && !removing;
    let why = if removing {
        "This take's sound is being removed. Wait for it to finish, or press Cancel."
    } else {
        "This copy of the app can only read your watches"
    };
    if ui
        .add_enabled(busy.is_none(), egui::Button::new("Open"))
        .on_hover_text("Analyse the whole take")
        .on_disabled_hover_text(busy.unwrap_or_default())
        .clicked()
    {
        act = Some(TakeAction::Open(dir.clone()));
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
        .add_enabled(
            changes && t.audio && !t.incomplete && cx.keeping.is_none(),
            egui::Button::new("Keep Analysis Only…"),
        )
        .on_hover_text(
            "Delete the take's sound to save space and keep its analysis. You are asked \
             first, and told what is lost.",
        )
        .on_disabled_hover_text(if !t.audio {
            "This take's sound has already been removed"
        } else if t.incomplete {
            "Files this take lists are missing"
        } else if cx.keeping.is_some() {
            "Another take's sound is being removed. Wait for it to finish."
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
    act
}

/// The watch's takes as a table, newest first: a click picks a row, a
/// double click opens it (unless `busy` says why not), a right click
/// offers the rest. Nothing is inserted between rows.
pub fn takes_table(
    ui: &mut egui::Ui,
    e: &WatchEntry,
    cx: &TakesContext,
    busy: Option<&str>,
) -> Option<TakeAction> {
    let pal = theme::pal(ui);
    if e.takes.is_empty() {
        panel(ui, |ui| {
            ui.label(
                RichText::new(if cx.unsorted {
                    "No takes here: a Quick Listen take saved with File > Save… waits here until it is filed."
                } else {
                    "No takes yet: Listen, Positions Test or Power Reserve Test, above, each keeps one here."
                })
                .color(pal.text_secondary),
            );
        });
        return None;
    }
    let mut act = None;
    panel(ui, |ui| {
        let w = ui.available_width();
        // Each column as wide as its heading or its widest cell; Started
        // takes what the others leave.
        let rows: Vec<[String; 7]> = e
            .takes
            .iter()
            .map(|t| take_cells(t, is_positions_test(&e.take_dir(t))))
            .collect();
        let text_w = |t: &str, size: f32| {
            ui.fonts_mut(|f| {
                f.layout_no_wrap(t.to_string(), egui::FontId::proportional(size), pal.text)
                    .size()
                    .x
            })
        };
        let natural: Vec<f32> = (0..7)
            .map(|i| {
                rows.iter()
                    .map(|r| text_w(&r[i], 12.5))
                    .fold(text_w(TAKE_HEADS[i], 11.5), f32::max)
                    + 18.0
            })
            .collect();
        let others: f32 = natural[1..].iter().sum();
        let first = (w - others).max(natural[0]);
        let widths: Vec<f32> = std::iter::once(first)
            .chain(natural[1..].iter().copied())
            .collect();
        let row_h = 24.0;
        let font = egui::FontId::proportional(12.5);
        let draw = |ui: &egui::Ui,
                    rect: egui::Rect,
                    cells: &[String],
                    color: egui::Color32,
                    font: &egui::FontId| {
            let p = ui.painter_at(rect);
            let mut x = rect.left() + 6.0;
            for (i, (c, w)) in cells.iter().zip(&widths).enumerate() {
                // Started and Position read from the left, the figures from the right.
                let (pos, align) = if i < 2 {
                    (egui::pos2(x, rect.center().y), egui::Align2::LEFT_CENTER)
                } else {
                    (
                        egui::pos2(x + w - 12.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                    )
                };
                p.text(pos, align, c, font.clone(), color);
                x += w;
            }
        };
        let (head, _) = ui.allocate_exact_size(Vec2::new(w, row_h), Sense::hover());
        draw(
            ui,
            head,
            &TAKE_HEADS.map(String::from),
            pal.text_secondary,
            &egui::FontId::proportional(11.5),
        );
        for t in &e.takes {
            let dir = e.take_dir(t);
            let on_show = cx.on_show == Some(dir.as_path());
            let picked = cx.picked == Some(dir.as_path());
            let test = is_positions_test(&dir);
            let (rect, r) = ui.allocate_exact_size(Vec2::new(w, row_h), Sense::click());
            if picked {
                ui.painter().rect_filled(rect, 4.0, pal.control_active);
            } else if r.hovered() {
                ui.painter().rect_filled(rect, 4.0, pal.control_hover);
            }
            let cells = take_cells(t, test);
            draw(ui, rect, &cells, pal.text, &font);
            // Its state after the date, in small grey words.
            let mut tags = Vec::new();
            if on_show {
                tags.push("On Show");
            }
            if !t.audio {
                tags.push("Analysis Only");
            }
            if t.incomplete {
                tags.push("Incomplete");
            }
            if !tags.is_empty() {
                let date_w = ui.fonts_mut(|f| {
                    f.layout_no_wrap(cells[0].clone(), font.clone(), pal.text)
                        .size()
                        .x
                });
                ui.painter_at(rect).text(
                    egui::pos2(rect.left() + 6.0 + date_w + 10.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    tags.join(" · "),
                    egui::FontId::proportional(11.0),
                    pal.text_secondary,
                );
            }
            let r = r.on_hover_text(format!(
                "{}\n\n{}",
                take_line(t, test),
                if on_show {
                    "This take is on show on the Bench. Right-click for more."
                } else if t.incomplete {
                    "Files this take lists are missing (still syncing, or copied in part): it \
                     is left as it is. Double-click to open what is there."
                } else {
                    "Click to pick this take, double-click to open it. Right-click for more."
                }
            ));
            if r.double_clicked() && busy.is_none() {
                act = Some(TakeAction::Open(dir.clone()));
            } else if r.clicked() || r.secondary_clicked() {
                // A right-click picks the row too, so the menu acts on the
                // take that is marked.
                act = Some(TakeAction::Pick(dir.clone()));
            }
            r.context_menu(|ui| {
                if let Some(a) = take_menu(ui, e, t, cx, busy) {
                    act = Some(a);
                }
            });
        }
    });
    // A take whose sound is being removed: under the table, never between
    // its rows.
    if let Some((k, done)) = cx.keeping {
        if e.takes.iter().any(|t| e.take_dir(t) == k) {
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
    }
    act
}

/// A watch's added fields and notes, as rows in a panel.
pub fn details_panel(ui: &mut egui::Ui, w: &Watch) {
    let pal = theme::pal(ui);
    let mut rows: Vec<(String, String)> = w
        .fields
        .iter()
        .map(|f| (f.label.clone(), f.value.clone()))
        .collect();
    if let Some(n) = w.notes.as_deref().filter(|n| !n.trim().is_empty()) {
        rows.push(("Notes".into(), n.to_string()));
    }
    panel(ui, |ui| {
        if rows.is_empty() {
            ui.label(
                RichText::new(
                    "None yet. Edit… adds notes, a customer, a serial number or any field.",
                )
                .small()
                .color(pal.text_secondary),
            );
        }
        for (label, value) in rows {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(value).color(pal.text_secondary));
                });
            });
        }
    });
}

/// A question about a take, in a window of its own.
#[derive(Debug, Clone)]
pub enum Ask {
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
            Ask::Delete { take, bytes } => {
                ui.label(
                    RichText::new(format!("Delete \"{}\"?", folder_label(take)))
                        .font(theme::semibold(15.0)),
                );
                ui.label(format!(
                    "The take's folder and everything in it ({}) will be deleted. You can't \
                     undo this.",
                    fmt_bytes(*bytes)
                ));
                ("Delete", true, true)
            }
            Ask::DeletePhoto { photo } => {
                ui.label(
                    RichText::new(format!("Delete \"{}\"?", folder_label(photo)))
                        .font(theme::semibold(15.0)),
                );
                ui.label("The photo will be deleted from the watch's photos. You can't undo this.");
                ("Delete", true, true)
            }
            Ask::Keep { take, bytes, sound } => {
                ui.label(
                    RichText::new(format!(
                        "Delete the Sound of \"{}\" and Keep Its Analysis?",
                        folder_label(take)
                    ))
                    .font(theme::semibold(15.0)),
                );
                ui.label(format!(
                    "This take uses {}, and {} of that is sound. The app reads the sound one \
                     last time to save the analysis, checks what it saved, and then deletes \
                     the sound.",
                    fmt_bytes(*bytes),
                    fmt_bytes(*sound)
                ));
                ui.label(
                    RichText::new(
                        "Without its sound, this take can't be analysed again, by a later \
                         version of the app or with other settings, and the Scope won't work \
                         on it. The readings, charts, Steadiness and Positions Test will still \
                         work. You can't undo this.",
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
        // What throws a take away is never the default: it sits at the far
        // left, and Return does nothing.
        let role = if red {
            Role::Destructive
        } else {
            Role::Default
        };
        if let Some(i) = theme::dialog_buttons(
            ui,
            &[
                DialogButton::new(role, go, "Go ahead")
                    .enabled_unless(ready, "Choose a watch first"),
                DialogButton::new(Role::Cancel, "Cancel", "Close without changing anything"),
            ],
        ) {
            answer = Some(i == 0);
        }
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
            theme::alert_label(ui, theme::Alert::Bad, e, false);
        }
        ui.add_space(6.0);
        match theme::dialog_buttons(
            ui,
            &[
                DialogButton::new(
                    Role::Default,
                    if f.new { "Add Watch" } else { "Save" },
                    "Write these details into the watch's folder",
                )
                .enabled_unless(!f.watch.name.trim().is_empty(), "A watch needs a name"),
                DialogButton::new(Role::Cancel, "Cancel", "Close without changing anything"),
            ],
        ) {
            Some(0) => act = Some(FormAction::Save),
            Some(_) => act = Some(FormAction::Cancel),
            None => {}
        }
    });
    if modal.should_close() && act.is_none() {
        act = Some(FormAction::Cancel);
    }
    act
}

/// The hover of a figure the calibre sets.
const FROM_CALIBRE: &str =
    "Set by the calibre: pick another calibre, or change it in Manage Calibres…";

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
    // Name, Calibre, Lift Angle, Beat Rate and Notes are always kept:
    // a watch in a collection has them, set or not. Only the fields the
    // user adds can be removed.
    let current = w.calibre.clone().unwrap_or_default();
    let picked = theme::row(
        ui,
        "Calibre",
        Some("The movement: it fills the lift angle and beat rate below"),
        |ui| cal.picker(ui, &current, w.bph.map(|b| (b, "the watch's beat rate"))),
    );
    if let Some(p) = picked {
        if let Some(l) = cal.lift(&p) {
            w.lift = Some(l);
        }
        if let Some(b) = cal.bph(&p) {
            w.bph = Some(b);
        }
        w.calibre = (!p.is_empty()).then_some(p);
    }
    // Either the calibre or the watch: a figure the calibre gives is the
    // calibre's, and only one it doesn't give is set here.
    let calibre = w.calibre.clone().unwrap_or_default();
    let (cal_lift, cal_bph) = (cal.lift(&calibre), cal.bph(&calibre));
    theme::row(
        ui,
        "Lift Angle",
        Some(match cal_lift {
            Some(_) => "Degrees, from the calibre. Pick no calibre to set it here.",
            None => "Degrees; each new take of this watch starts with it.",
        }),
        |ui| match cal_lift {
            Some(l) => {
                w.lift = Some(l);
                ui.label(format!("{}° from the calibre", fields::plain(l)))
                    .on_hover_text(FROM_CALIBRE);
            }
            None => {
                let mut lift = w.lift.unwrap_or(DEFAULT_LIFT_DEG);
                let field = ui.add(
                    egui::DragValue::new(&mut lift)
                        .range(10.0..=90.0)
                        .suffix("°"),
                );
                if field.changed() {
                    w.lift = Some(lift);
                }
                if w.lift.is_none() {
                    ui.label(RichText::new("(default)").color(theme::pal(ui).text_tertiary));
                    field.on_hover_text(
                        "The app's starting figure, not this watch's: set the lift angle its \
                         movement has",
                    );
                }
            }
        },
    );
    theme::row(
        ui,
        "Beat Rate",
        Some(match cal_bph {
            Some(_) => "Beats per hour, from the calibre. Pick no calibre to set it here.",
            None => "Beats per hour; each new take of this watch starts with it. Auto finds it.",
        }),
        |ui| match cal_bph {
            Some(b) => {
                w.bph = Some(b);
                ui.label(format!("{b} bph from the calibre"))
                    .on_hover_text(FROM_CALIBRE);
            }
            None => {
                egui::ComboBox::from_id_salt("form bph")
                    .selected_text(w.bph.map_or("Auto".to_string(), |b| format!("{b} bph")))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut w.bph, None, "Auto");
                        for b in timegrapher_core::beats::STANDARD_BPH {
                            ui.selectable_value(&mut w.bph, Some(b), format!("{b} bph"));
                        }
                    });
            }
        },
    );
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
    let mut notes = w.notes.clone().unwrap_or_default();
    ui.label("Notes");
    ui.add(
        egui::TextEdit::multiline(&mut notes)
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    )
    .on_hover_text("Anything about the watch, kept as typed");
    w.notes = (!notes.is_empty()).then_some(notes);
    ui.horizontal(|ui| {
        ui.menu_button("Add Field", |ui| {
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
    });
    ui.label(
        RichText::new(
            "Only the name is needed. Name, Calibre, Lift Angle, Beat Rate and Notes are always \
             kept; the fields you add can be removed.",
        )
        .small()
        .color(pal.text_secondary),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// New Take's captions are whole under their buttons in an 800 point
    /// window, where the watch's side is narrowest.
    #[test]
    fn the_new_take_captions_fit_an_800_window() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        // The window less the list, its gap and the margins.
        let side = 800.0 - crate::app::WATCH_LIST_W - 17.0 - 2.0 * 12.0 - 16.0;
        let tile = (side - 2.0 * 10.0) / 3.0 - 16.0;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for c in NEW_TAKE_CAPTIONS {
                    let font = egui::TextStyle::Small.resolve(ui.style());
                    let w = ui
                        .painter()
                        .layout_no_wrap(c.to_string(), font, egui::Color32::WHITE)
                        .size()
                        .x;
                    assert!(w <= tile, "{c}: {w} in {tile}");
                }
            });
        });
    }

    /// A watch's Calibre, Lift Angle, Beat Rate and Notes are always in its
    /// form, set or not; only a field the user added has a × to remove it.
    #[test]
    fn a_watchs_standard_fields_are_fixed_and_added_ones_removable() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut cal = Calibres::in_memory();
        let draw = |w: &mut Watch, cal: &mut Calibres| {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| fields(ui, w, cal));
            });
            let mut texts = Vec::new();
            for c in &out.shapes {
                if let egui::Shape::Text(t) = &c.shape {
                    texts.push(t.galley.text().to_string());
                }
            }
            texts
        };
        let mut w = Watch::new("Pepsi");
        let bare = draw(&mut w, &mut cal);
        for row in ["Calibre", "Lift Angle", "Beat Rate", "Notes"] {
            assert!(bare.iter().any(|t| t == row), "{row}: {bare:?}");
        }
        assert!(
            !bare.iter().any(|t| t == "×"),
            "nothing to remove: {bare:?}"
        );
        w.fields.push(Field {
            label: "Serial".into(),
            kind: FieldKind::Text,
            value: String::new(),
        });
        let added = draw(&mut w, &mut cal);
        assert_eq!(added.iter().filter(|t| *t == "×").count(), 1, "{added:?}");
    }

    /// The header names the beat rate and lift angle a take starts with:
    /// the calibre's where the table gives them, else the watch's own.
    #[test]
    fn the_header_takes_its_beat_rate_and_lift_from_the_calibre_first() {
        let cal = Calibres::in_memory();
        let mut w = Watch::new("Diver");
        w.calibre = Some("ETA 2824-2".into());
        let line = subline(&w, &cal);
        let (bph, lift) = (
            cal.bph("ETA 2824-2").unwrap(),
            cal.lift("ETA 2824-2").unwrap(),
        );
        assert!(line.contains(&format!("{bph} bph")), "{line}");
        assert!(
            line.contains(&format!("lift {}°", fields::plain(lift))),
            "{line}"
        );
        let mut own = Watch::new("Own");
        own.calibre = Some("Not in the table".into());
        own.bph = Some(19800);
        own.lift = Some(56.0);
        assert_eq!(
            subline(&own, &cal),
            "Not in the table · 19800 bph · lift 56°"
        );
    }

    /// A list row's second line keeps whole parts that fit, in order.
    #[test]
    fn a_rows_second_line_keeps_the_parts_that_fit() {
        let ctx = egui::Context::default();
        let mut got = Vec::new();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let font = egui::FontId::proportional(11.5);
                let parts = [
                    "Audemars Piguet 2121".to_string(),
                    "3 takes".to_string(),
                    "last 2026-10-22".to_string(),
                ];
                got.push(fitted(ui, &parts, &font, 1000.0));
                // Just wide enough for two parts, not three.
                let two = ui.fonts_mut(|f| {
                    f.layout_no_wrap(
                        "Audemars Piguet 2121 · 3 takes".into(),
                        font.clone(),
                        egui::Color32::WHITE,
                    )
                    .size()
                    .x
                });
                got.push(fitted(ui, &parts, &font, two + 1.0));
                got.push(fitted(ui, &parts, &font, 10.0));
            });
        });
        assert_eq!(got[0], "Audemars Piguet 2121 · 3 takes · last 2026-10-22");
        assert_eq!(got[1], "Audemars Piguet 2121 · 3 takes");
        assert_eq!(
            got[2], "Audemars Piguet 2121",
            "the first is kept, to be cut"
        );
    }

    /// The search's matches, ignoring case, on the text's own char
    /// boundaries, each once.
    #[test]
    fn the_list_marks_what_the_search_found() {
        assert_eq!(found_in("Pepsi GMT", "gmt"), vec![6..9]);
        assert_eq!(
            found_in("Pepsi GMT", ""),
            Vec::<std::ops::Range<usize>>::new()
        );
        assert_eq!(found_in("aaa", "aa"), vec![0..2]);
        assert_eq!(found_in("Ébauche ÉTA", "é"), vec![0..2, 9..11]);
        assert_eq!(found_in("Seiko", "x"), Vec::<std::ops::Range<usize>>::new());
    }

    #[test]
    fn a_takes_row_has_its_seven_columns() {
        let t = Take {
            name: "2026-10-22 09.10".into(),
            bytes: 112_600_000,
            audio: false,
            incomplete: false,
            started_utc: None,
            duration_s: Some(1800.0),
            positions: vec!["DU".into()],
            at_end: Some(timegrapher_core::library::AtEnd {
                rate_s_per_day: Some(9.8),
                amplitude_deg: Some(287.2),
                beat_error_ms: Some(-0.21),
                rate_corrected_s_per_day: None,
            }),
            power_reserve: None,
        };
        assert_eq!(
            take_cells(&t, false),
            [
                "2026-10-22 09.10",
                "DU",
                "30:00",
                "+9.8",
                "287",
                "0.2",
                "112.6"
            ]
            .map(String::from)
        );
        // The rate on the take's clock log where it has one, as the Bench
        // reads it.
        let mut c = t.clone();
        c.at_end.as_mut().unwrap().rate_corrected_s_per_day = Some(21.5);
        assert_eq!(take_cells(&c, false)[3], "+21.5");
        let all = Take {
            positions: ["DU", "DD", "CU", "CD", "CL", "CR"]
                .map(String::from)
                .to_vec(),
            ..t
        };
        assert_eq!(take_cells(&all, false)[1], "All 6");
        // A Power Reserve Test's run in the same format as the others.
        let pr = Take {
            power_reserve: Some(timegrapher_core::power_reserve::Summary {
                run_s: 166_320.0,
                ending: timegrapher_core::power_reserve::Ending::Stopped,
                position: "CL".into(),
            }),
            ..all.clone()
        };
        let cells = take_cells(&pr, false);
        assert_eq!(cells[1], "Power Reserve");
        assert_eq!(cells[2], crate::strip::fmt_time(166_320.0));
        assert_eq!(TAKE_HEADS.len(), 7);
    }

    /// A Positions Test's take names its kind in the table's Position
    /// column and leads its line with it, its positions after.
    #[test]
    fn a_positions_test_take_names_its_kind() {
        let dir = std::env::temp_dir().join(format!("tg-app-kind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_positions_test(&dir));
        std::fs::write(dir.join(crate::guided::RESULTS_FILE), "{}").unwrap();
        assert!(is_positions_test(&dir));
        let t = Take {
            name: "2026-10-26 12.00".into(),
            bytes: 1_000_000,
            audio: true,
            incomplete: false,
            started_utc: Some("2026-10-26T19:00:00Z".into()),
            duration_s: Some(153.0),
            positions: vec!["DU".into()],
            at_end: None,
            power_reserve: None,
        };
        assert_eq!(take_cells(&t, false)[1], "DU");
        assert_eq!(take_cells(&t, true)[1], "Positions Test");
        let line = take_line(&t, true);
        assert!(line.starts_with("Positions Test · Dial Up · "), "{line}");
        let _ = std::fs::remove_dir_all(&dir);
    }

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
                rate_corrected_s_per_day: None,
            }),
            power_reserve: None,
        };
        let line = take_line(&t, false);
        assert!(line.starts_with("Dial Up, Crown Left · "), "{line}");
        assert!(
            line.contains("+3.5 s/d · 281° · 0.4 ms · Analysis Only · Incomplete"),
            "{line}"
        );
        // A take with a clock log shows its rate on that log, as the
        // Bench reads it.
        let mut c = t.clone();
        c.at_end.as_mut().unwrap().rate_corrected_s_per_day = Some(15.0);
        assert!(
            take_line(&c, false).contains("+15.0 s/d · 281°"),
            "{}",
            take_line(&c, false)
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
        let line = take_line(&r, false);
        assert!(
            line.starts_with("Power Reserve 46 h 12 min · Crown Left · +3.5"),
            "{line}"
        );
        assert!(local_minute("2026-10-20T21:32:00Z").is_some_and(|m| m.len() == 16));
        assert_eq!(local_minute("not a time"), None);
    }
}
