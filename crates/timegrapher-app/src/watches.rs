//! The Watches pane's parts: the library's watches as a list, one watch's
//! details and takes, and the form for a watch's details. The app owns
//! the library and acts on what these return.

use crate::app::fmt_bytes;
use crate::calibre_ui::Calibres;
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
    "Each take shows what it measured when it was saved, its length and the space it uses. \
     Picking one opens it as Open does. The Microphone and Recording controls below work \
     with a watch picked or without one; without one, Save… asks where.",
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

/// A row that can be clicked: `top` and its `right` on one line, `below`
/// in small grey type under it.
fn row(ui: &mut egui::Ui, top: &str, right: &str, below: &str, hint: &str) -> egui::Response {
    let pal = theme::pal(ui);
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 40.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, pal.control_hover);
    }
    let p = ui.painter_at(rect);
    let left = rect.left() + 8.0;
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

/// The list: one row per watch. The id of the watch clicked, if any.
pub fn list(ui: &mut egui::Ui, watches: &[&WatchEntry]) -> Option<String> {
    let mut picked = None;
    for e in watches {
        let w = &e.watch;
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
    if !e.photos.is_empty() {
        rows.push(("Photos".into(), count(e.photos.len(), "photo", "photos")));
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

/// What to do with a take.
#[derive(Debug, Clone, PartialEq)]
pub enum TakeAction {
    Open(PathBuf),
    Replay(PathBuf),
    Show(PathBuf),
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
    if !names.is_empty() {
        parts.push(names.join(", "));
    }
    if let Some(d) = t.duration_s {
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
pub fn takes(ui: &mut egui::Ui, e: &WatchEntry) -> Option<TakeAction> {
    let pal = theme::pal(ui);
    if e.takes.is_empty() {
        ui.label(
            RichText::new("No takes yet: Start, then Save…, keeps one here.")
                .color(pal.text_secondary),
        );
        return None;
    }
    let mut act = None;
    for t in &e.takes {
        let dir = e.take_dir(t);
        let replays = t.audio && !t.incomplete;
        let r = row(
            ui,
            &t.name,
            &fmt_bytes(t.bytes),
            &take_line(t),
            if t.incomplete {
                "Files this take lists are missing (still syncing, or copied in part): it is \
                 left as it is. Click to open what is there."
            } else {
                "Open this take, as Open does. Right-click for more."
            },
        );
        if r.clicked() {
            act = Some(TakeAction::Open(dir.clone()));
        }
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
        });
    }
    act
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
        };
        let line = take_line(&t);
        assert!(line.starts_with("Dial Up, Crown Left · "), "{line}");
        assert!(
            line.contains("+3.5 s/d · 281° · 0.4 ms · Analysis Only · Incomplete"),
            "{line}"
        );
        assert!(local_minute("2026-10-20T21:32:00Z").is_some_and(|m| m.len() == 16));
        assert_eq!(local_minute("not a time"), None);
    }
}
