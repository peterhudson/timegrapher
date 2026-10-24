//! The Calibre menu on the Beat Rate card, and Manage Calibres. The menu
//! offers the calibres the app was set to lately, the short list and the
//! watchmaker's own, marked Custom. Manage Calibres is a table of every
//! calibre to sort, filter by any column and search over names, other
//! names and makers, a tickbox for the short list on each, and the one
//! selected in full: its beat rate and lift angle, each wheel's turn with where the figure comes
//! from, and the table's sources. Wheels nobody has given are left blank
//! for the watchmaker to fill in; those figures, and calibres the table
//! doesn't list, are kept in core's user calibre file in the settings
//! folder, which the command line and the MCP server read too. Share with
//! the Project only opens a pre-filled GitHub issue in the browser: the
//! app sends nothing itself.

use crate::fields;
use crate::settings;
use crate::theme;
use eframe::egui::{self, RichText, Vec2};
use std::path::PathBuf;
use std::sync::OnceLock;
use timegrapher_core::calibre_store::{
    Catalog, CustomCalibre, CustomWheel, Entry, Origin, UserCalibres,
};
use timegrapher_core::calibres::{self, Calibre};
use timegrapher_core::periodicity::Wheel;

/// How many calibres the app was set to lately the menu offers.
pub const RECENT: usize = 6;

/// The user's calibres and the menu's own state.
pub struct Calibres {
    pub user: UserCalibres,
    /// Where the user's additions are kept; `None` keeps them in memory
    /// (tests, or no settings folder).
    path: Option<PathBuf>,
    /// A failed load must never turn an empty fallback into a replacement file.
    /// Kept for this instance even if the file is repaired externally.
    blocked_load: Option<String>,
    /// Original legacy settings, retained until a catalogue save succeeds.
    pending_legacy: Vec<settings::CustomCalibre>,
    /// The calibres the app was last set to (a menu pick, a watch opened,
    /// a take opened or saved), the latest first, none twice.
    pub recent: Vec<String>,
    /// Calibres the watchmaker added to the short list, and the list's
    /// own ones they took off it.
    pub short_added: Vec<String>,
    pub short_removed: Vec<String>,
    /// Manage Calibres: asked for (the app opens Settings at Calibres),
    /// the calibre selected in it and its search.
    pub manager_open: bool,
    selected: Option<String>,
    search: String,
    /// What is typed in the Calibre menu's own search box.
    menu_search: String,
    /// The column the table is sorted on, and whether reversed.
    sort: (Col, bool),
    /// Each column's funnel: the values it shows, or `None` for all.
    funnels: [Option<std::collections::BTreeSet<String>>; 6],
    /// What is typed in the funnel open.
    funnel_search: String,
    /// Short List Only.
    short_only: bool,
    /// Name and period typed for a wheel the slots don't list.
    extra: (String, String),
    /// Why the user's calibres couldn't be read or written, if so.
    pub problem: Option<String>,
}

/// A column of Manage Calibres' table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Col {
    Maker,
    Calibre,
    BeatRate,
    LiftAngle,
    Short,
    Custom,
}

/// The table's columns, each with its heading and what it shows.
const COLS: [(Col, &str, &str); 6] = [
    (
        Col::Maker,
        "Maker",
        "Who makes the calibre; the ones you added are under Custom",
    ),
    (Col::Calibre, "Calibre", "The calibre's name"),
    (Col::BeatRate, "Beat Rate", "Beats per hour"),
    (Col::LiftAngle, "Lift Angle", "The lift angle, in degrees"),
    (
        Col::Short,
        "Short List",
        "Ticked: on your short list, which the Calibre menu offers",
    ),
    (Col::Custom, "Custom", "Marked: a calibre you added"),
];

/// The widths of the table's columns in `w`: the facts' columns as wide
/// as their headings, maker and name sharing the rest.
fn col_widths(w: f32) -> [f32; 6] {
    let facts = [80.0, 82.0, 82.0, 70.0];
    let rest = (w - facts.iter().sum::<f32>()).max(160.0);
    [
        rest * 0.42,
        rest * 0.58,
        facts[0],
        facts[1],
        facts[2],
        facts[3],
    ]
}

/// The least width the selected calibre's details need beside the table.
const DETAILS_W: f32 = 330.0;
/// A line of the table.
const ROW_H: f32 = 22.0;
/// The maker the user's own calibres are shown under.
const CUSTOM: &str = "Custom";
/// What a column shows where the list gives nothing.
const NOT_GIVEN: &str = "Not Given";

/// What Manage Calibres did that the app acts on.
#[derive(Debug, Default)]
pub struct Managed {
    /// What changed for the calibre selected, named `name` before it.
    pub change: Change,
    pub name: String,
}

const MANAGE_HINT: &str = "Every calibre in a table to search, sort and filter, with its \
     tickbox for your short list, and the one you select in full: its wheels, sources and notes, \
     what you added, and your custom calibres to edit";

pub const MANAGE_HELP: &[&str] = &[
    "Every calibre the app knows, in a table, with the ones you added under the maker Custom, \
     always at the top. Click a column's heading to sort on it, and again to reverse. The \
     funnel on each heading picks which of that column's values show; the funnels combine, a \
     tinted funnel is one in use, and Clear Filters shows everything again, clearing the \
     search and Short List Only too.",
    "Search by a calibre's number, one of its other names, or its maker under any of the \
     maker's names. Short List Only shows just your short list.",
    "The tickbox in Short List puts a calibre on your short list, which the Calibre menu \
     offers. The list starts as the common and modern calibres; Reset the Short List goes back \
     to them.",
    "The calibre selected shows in full: maker, beat rate, lift angle and escape wheel, each \
     wheel's time for one turn with where it comes from, the list's sources and notes. Type a \
     wheel's period you know into a blank and it is kept for this calibre, marked Custom; Share \
     with the Project opens a GitHub issue with what you added, to send from your browser.",
    "New Custom Calibre adds one the list doesn't have, named as typed in the search box; its \
     name, maker, beat rate and lift angle are edited here.",
];

/// What the panel changed that the app acts on.
#[derive(Debug, Default, PartialEq)]
pub struct Change {
    /// A wheel's period: the cycles Steadiness finds are named afresh.
    pub wheels: bool,
    /// A custom calibre's new name.
    pub renamed: Option<String>,
    /// A custom calibre deleted.
    pub deleted: bool,
    /// A custom calibre's lift angle, typed in its panel.
    pub lift: Option<f64>,
}

impl Calibres {
    /// The user's calibres from `path`; none yet, or an unreadable file,
    /// starts empty. A load error blocks persistence for this instance; edits
    /// remain in memory and the original path/error stay visible.
    pub fn open(path: Option<PathBuf>) -> Self {
        let mut c = Calibres::in_memory();
        if let Some(p) = &path {
            match UserCalibres::load(p) {
                Ok(u) => c.user = u,
                Err(e) => {
                    let problem = format!(
                        "Your calibres couldn't be read: {e}. Catalogue edits are only in memory; \
                         the existing file will not be overwritten. Repair or move the file, \
                         then restart the app before making changes you want to keep."
                    );
                    c.blocked_load = Some(problem.clone());
                    c.problem = Some(problem);
                }
            }
        }
        c.path = path;
        c
    }

    /// Kept only while the app runs.
    pub fn in_memory() -> Self {
        Calibres {
            user: UserCalibres::default(),
            path: None,
            blocked_load: None,
            pending_legacy: Vec::new(),
            recent: Vec::new(),
            short_added: Vec::new(),
            short_removed: Vec::new(),
            manager_open: false,
            selected: None,
            search: String::new(),
            menu_search: String::new(),
            sort: (Col::Maker, false),
            funnels: Default::default(),
            funnel_search: String::new(),
            short_only: false,
            extra: (String::new(), String::new()),
            problem: None,
        }
    }

    /// True only after a disk save. A failed load is not retried implicitly:
    /// this instance never observed the existing catalogue it would replace.
    fn save(&mut self) -> bool {
        if let Some(problem) = &self.blocked_load {
            self.problem = Some(problem.clone());
            return false;
        }
        let Some(p) = &self.path else { return false };
        match self.user.save(p) {
            Ok(()) => {
                self.pending_legacy.clear();
                self.problem = None;
                true
            }
            Err(e) => {
                self.problem = Some(format!(
                    "Your calibres couldn't be saved: {e}. Catalogue edits are only in memory."
                ));
                false
            }
        }
    }

    /// Keep the originals in app settings until their catalogue write succeeds,
    /// including when no persistent catalogue path is available.
    pub fn pending_legacy(&self) -> &[settings::CustomCalibre] {
        &self.pending_legacy
    }

    pub fn find(&self, name: &str) -> Option<Entry<'_>> {
        Catalog::new(&self.user).find(name)
    }

    /// `name` as the table calls it now, when the table has renamed it
    /// ("… vph" to "… bph"); a calibre of the user's own keeps its name.
    pub fn current_name(&self, name: &str) -> String {
        if self.user.custom.iter().any(|c| c.name == name) {
            return name.to_string();
        }
        calibres::renamed(name).unwrap_or(name).to_string()
    }

    /// The recent picks and the short list saved by an earlier version,
    /// under the table's current names, each once.
    pub fn restore(&mut self, recent: Vec<String>, added: Vec<String>, removed: Vec<String>) {
        let current = |names: Vec<String>| {
            let mut out: Vec<String> = Vec::new();
            for n in names {
                let n = self.current_name(&n);
                if !out.contains(&n) {
                    out.push(n);
                }
            }
            out
        };
        let (recent, added, removed) = (current(recent), current(added), current(removed));
        self.recent = recent;
        self.recent.truncate(RECENT);
        self.short_added = added;
        self.short_removed = removed;
    }

    /// The wheels known for `name`, the table's and the user's, for
    /// naming the cycles Steadiness finds; `None` when no calibre is set.
    pub fn wheels(&self, name: &str) -> Option<Vec<Wheel>> {
        let e = self.find(name)?;
        Some(
            Catalog::new(&self.user)
                .wheels(e)
                .into_iter()
                .filter(|w| w.period_s > 0.0 && !w.name.trim().is_empty())
                .collect(),
        )
    }

    pub fn lift(&self, name: &str) -> Option<f64> {
        self.find(name)?.lift_angle_deg()
    }

    pub fn bph(&self, name: &str) -> Option<u32> {
        self.find(name)?.bph()
    }

    /// A custom calibre's wheels, for the session file of a take.
    pub fn custom_wheels(&self, name: &str) -> Option<Vec<(String, f64)>> {
        match self.find(name)? {
            Entry::Custom(c) => Some(
                c.wheels
                    .iter()
                    .map(|w| (w.name.clone(), w.period_s))
                    .collect(),
            ),
            Entry::Table(_) => None,
        }
    }

    /// Add a calibre a take's session file names with its wheels, when
    /// neither the table nor the user has one by that name.
    pub fn adopt(&mut self, name: &str, wheels: &[(String, f64)]) {
        if self.find(name).is_some() || wheels.is_empty() {
            return;
        }
        self.user
            .put_custom(custom(name, wheels.iter().map(|(n, p)| (n.as_str(), *p))));
        self.save();
    }

    /// Whether `c` is on the short list: the list's common and modern
    /// calibres unless taken off, and any the watchmaker added.
    pub fn in_short(&self, c: &Calibre) -> bool {
        if c.common {
            !self.short_removed.contains(&c.calibre)
        } else {
            self.short_added.contains(&c.calibre)
        }
    }

    /// Put `name` on the short list, or take it off.
    pub fn set_short(&mut self, name: &str, on: bool) {
        let Some(c) = calibres::find(name) else {
            return;
        };
        let (list, add) = if c.common {
            (&mut self.short_removed, !on)
        } else {
            (&mut self.short_added, on)
        };
        list.retain(|n| n != &c.calibre);
        if add {
            list.push(c.calibre.clone());
        }
    }

    /// Whether the short list is the one the app ships with.
    fn short_is_default(&self) -> bool {
        self.short_added.is_empty() && self.short_removed.is_empty()
    }

    /// Put `name` first among the recent calibres.
    pub fn remember(&mut self, name: &str) {
        if name.is_empty() {
            return;
        }
        self.recent.retain(|r| r != name);
        self.recent.insert(0, name.to_string());
        self.recent.truncate(RECENT);
    }

    /// Calibres typed in before they were kept in the user calibre file,
    /// from the app's settings. The original settings are dropped only after
    /// an actual catalogue save, never after a blocked or memory-only migration.
    pub fn migrate(&mut self, old: Vec<settings::CustomCalibre>) {
        for c in old {
            if !self.pending_legacy.contains(&c) {
                self.pending_legacy.push(c.clone());
            }
            if self.user.custom.iter().any(|x| x.name == c.name) {
                continue;
            }
            self.user.put_custom(custom(
                &c.name,
                c.wheels.iter().map(|w| (w.name.as_str(), w.period_s)),
            ));
        }
        if !self.pending_legacy.is_empty() {
            self.save();
        }
    }

    /// A new custom calibre, named `typed` when that is free, else "My
    /// Calibre N": its name.
    fn new_custom(&mut self, typed: &str) -> String {
        let typed = typed.trim();
        let name = if !typed.is_empty() && self.find(typed).is_none() {
            typed.to_string()
        } else {
            let mut n = self.user.custom.len() + 1;
            while self.find(&format!("My Calibre {n}")).is_some() {
                n += 1;
            }
            format!("My Calibre {n}")
        };
        self.user.put_custom(custom(
            &name,
            [
                ("escape wheel", 5.0),
                ("fourth wheel", 60.0),
                ("centre wheel", 3600.0),
            ],
        ));
        self.save();
        name
    }

    /// The Calibre menu, short: Not Set, the recent picks, the short list
    /// and the user's own calibres, with Manage Calibres… pinned at its
    /// foot, below the scrolling list, for the rest. What was picked ("" for
    /// Not Set), if anything.
    pub fn picker(&mut self, ui: &mut egui::Ui, current: &str) -> Option<String> {
        let pal = theme::pal(ui);
        let shown = match self.find(current) {
            Some(e) => e.name().to_string(),
            None => "Not Set".to_string(),
        };
        let picked: std::cell::RefCell<Option<String>> = Default::default();
        let mut manage = false;
        let catalog = Catalog::new(&self.user);
        let mut typed = std::mem::take(&mut self.menu_search);
        // egui makes a menu no taller than its default area; the list
        // scrolls inside that, leaving room for the search field above and
        // Manage Calibres… below, so the menu itself never scrolls.
        let menu_h =
            (ui.ctx().content_rect().height() * 0.7).min(ui.spacing().default_area_size.y - 16.0);
        let list_h = (menu_h - 110.0).max(120.0);
        {
            egui::ComboBox::from_id_salt("calibre")
                .width(ui.available_width())
                .height(menu_h)
                .truncate()
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    let line = |ui: &mut egui::Ui, e: Entry<'_>, text: String| {
                        if ui
                            .selectable_label(e.name() == current, text)
                            .on_hover_text(hint(e))
                            .clicked()
                        {
                            *picked.borrow_mut() = Some(e.name().to_string());
                        }
                    };
                    // Every calibre the app knows, by any of its names or its
                    // maker, without opening Manage Calibres.
                    let search = ui
                        .add(
                            egui::TextEdit::singleline(&mut typed)
                                .hint_text("Search every calibre")
                                .desired_width(f32::INFINITY),
                        )
                        .on_hover_text(
                            "Type part of a calibre's name, another name for it or its maker, \
                             to list every calibre that matches",
                        );
                    // Ready to type as soon as the menu opens.
                    if typed.is_empty() && !search.has_focus() {
                        search.request_focus();
                    }
                    let q = typed.trim();
                    // The list scrolls between the search field and Manage
                    // Calibres…, so both stay in view however long the
                    // Short List grows.
                    egui::ScrollArea::vertical()
                        .id_salt("calibre list")
                        .max_height(list_h)
                        .min_scrolled_height(list_h)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            if !q.is_empty() {
                                let found = catalog.search(q);
                                if found.is_empty() {
                                    ui.label(
                                        RichText::new(
                                            "No calibre matches. Manage Calibres… can add it.",
                                        )
                                        .small()
                                        .color(pal.text_tertiary),
                                    );
                                }
                                const MOST: usize = 200;
                                for e in found.iter().take(MOST) {
                                    let text = match e {
                                        Entry::Table(c) => format!("{} · {}", c.calibre, c.maker),
                                        Entry::Custom(_) => label(*e),
                                    };
                                    line(ui, *e, text);
                                }
                                if found.len() > MOST {
                                    ui.label(
                                        RichText::new(format!(
                                            "{} more: type more of the name to narrow it down.",
                                            found.len() - MOST
                                        ))
                                        .small()
                                        .color(pal.text_tertiary),
                                    );
                                }
                                return;
                            }
                            if ui
                                .selectable_label(current.is_empty(), "Not Set")
                                .on_hover_text(
                                    "Name cycles after the wheels most calibres at this beat rate \
                             share, with the default lift angle",
                                )
                                .clicked()
                            {
                                *picked.borrow_mut() = Some(String::new());
                            }
                            let recent: Vec<Entry<'_>> =
                                self.recent.iter().filter_map(|r| catalog.find(r)).collect();
                            if !recent.is_empty() {
                                heading(ui, "Recent");
                                for e in recent {
                                    line(ui, e, label(e));
                                }
                            }
                            heading(ui, "Short List");
                            let mut any = false;
                            for (maker, _, list) in by_maker() {
                                let short: Vec<&&Calibre> =
                                    list.iter().filter(|c| self.in_short(c)).collect();
                                if short.is_empty() {
                                    continue;
                                }
                                any = true;
                                ui.label(RichText::new(*maker).small().color(pal.text_tertiary));
                                for c in short {
                                    line(ui, Entry::Table(c), c.calibre.clone());
                                }
                            }
                            if !any {
                                ui.label(
                                    RichText::new(
                                        "Empty: tick calibres in Manage Calibres to put them here.",
                                    )
                                    .small()
                                    .color(pal.text_tertiary),
                                );
                            }
                            if !self.user.custom.is_empty() {
                                heading(ui, "Custom");
                                for c in &self.user.custom {
                                    line(ui, Entry::Custom(c), c.name.clone());
                                }
                            }
                        });
                    ui.separator();
                    if ui
                        .selectable_label(false, "Manage Calibres…")
                        .on_hover_text(MANAGE_HINT)
                        .clicked()
                    {
                        manage = true;
                    }
                });
        }
        let picked = picked.into_inner();
        // A pick starts the next search afresh.
        self.menu_search = if picked.is_some() {
            String::new()
        } else {
            typed
        };
        if manage {
            self.open_manager(current);
        }
        picked
    }

    /// Open Manage Calibres on `current`, or on nothing.
    pub fn open_manager(&mut self, current: &str) {
        self.manager_open = true;
        self.selected = self.find(current).map(|e| e.name().to_string());
    }

    /// Each wheel's turn with where it comes from, blanks to fill in,
    /// another wheel by name, the table's sources and notes, and the way to
    /// share what was added. `place` keeps the boxes apart from the same
    /// calibre's shown elsewhere. True when a wheel's period changed.
    fn wheels_and_sources(
        &mut self,
        ui: &mut egui::Ui,
        name: &str,
        version: &str,
        place: &str,
    ) -> bool {
        let pal = theme::pal(ui);
        let Some(e) = self.find(name) else {
            return false;
        };
        let catalog = Catalog::new(&self.user);
        let slots = catalog.slots(e);
        let share = catalog.contribution_url(e, version);
        let table = match e {
            Entry::Table(c) => Some(c),
            Entry::Custom(_) => None,
        };
        let mut edits: Vec<(String, Option<f64>)> = Vec::new();
        ui.label(
            RichText::new(
                "Each wheel's time for one full turn. A blank one isn't known: type \
                 its period if you know it, and it is kept for this calibre.",
            )
            .small()
            .color(pal.text_tertiary),
        );
        for s in &slots {
            let tip = match s.origin {
                Origin::Shipped => format!(
                    "From the list: {}",
                    s.basis.clone().unwrap_or_else(|| "no basis given".into())
                ),
                Origin::User => match s.shipped_period_s {
                    Some(p) => format!("Custom; the list says {} s", (PERIOD.show)(p)),
                    None => match &s.basis {
                        Some(b) => format!("Custom: {b}"),
                        None => "Custom, typed in".into(),
                    },
                },
                Origin::Unknown => {
                    "Nobody has given this wheel's period yet: type it if you know it".into()
                }
            };
            let id = format!("calibre-{place}-{name}-{}", s.name);
            theme::row(ui, &title_case(&s.name), Some(&tip), |ui| {
                if let Some(v) = period_field(ui, &id, s.period_s, &tip) {
                    edits.push((s.name.clone(), v));
                }
                ui.label("s");
                if s.origin == Origin::User {
                    ui.label(RichText::new("Custom").small().color(pal.text_tertiary));
                    if ui
                        .small_button("Clear")
                        .on_hover_text(if s.shipped_period_s.is_some() {
                            "Go back to the list's period"
                        } else {
                            "Forget the period typed in"
                        })
                        .clicked()
                    {
                        edits.push((s.name.clone(), None));
                    }
                }
            });
        }
        // Another wheel by name, such as an intermediate wheel.
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.extra.0)
                    .id_salt(("another wheel", place))
                    .hint_text("Another wheel")
                    .desired_width(theme::LABEL_W),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.extra.1)
                    .id_salt(("another period", place))
                    .hint_text("period")
                    .desired_width(50.0),
            );
            ui.label("s");
            let period = fields::parse_number(&self.extra.1).filter(|p| *p > 0.0);
            if ui
                .add_enabled(
                    period.is_some() && !self.extra.0.trim().is_empty(),
                    egui::Button::new("Add"),
                )
                .on_hover_text("Add a wheel the list above doesn't name, with its period")
                .clicked()
            {
                if let Some(p) = period {
                    edits.push((self.extra.0.trim().to_lowercase(), Some(p)));
                    self.extra = (String::new(), String::new());
                }
            }
        });
        if let Some(c) = table {
            if !c.sources.is_empty() {
                ui.add_space(4.0);
                ui.label(theme::caption("Sources"));
                for s in &c.sources {
                    source_line(ui, s);
                }
            }
            ui.label(
                RichText::new(format!("How the figures are known: {}.", c.confidence))
                    .small()
                    .color(pal.text_tertiary),
            );
            if let Some(n) = &c.notes {
                ui.add(egui::Label::new(RichText::new(n).small().color(pal.text_tertiary)).wrap());
            }
        }
        ui.add_space(4.0);
        if ui
            .add_enabled(
                share.is_some(),
                egui::Button::new("Share with the Project…"),
            )
            .on_hover_text(
                "Open a GitHub issue in your browser, filled in with what you added to \
                 this calibre, to look over and send. Nothing is sent from the app.",
            )
            .on_disabled_hover_text(
                "Nothing to share yet: type a wheel's period above, or add a custom calibre",
            )
            .clicked()
        {
            if let Some(url) = &share {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        }
        if edits.is_empty() {
            return false;
        }
        for (wheel, v) in edits {
            match v {
                Some(p) => self.user.set_wheel(name, &wheel, p, None),
                None => self.user.clear_wheel(name, &wheel),
            }
        }
        self.save();
        true
    }

    /// Manage Calibres, in Settings: every calibre in a table to search,
    /// sort and filter, with each one's short-list tickbox, and the one
    /// selected in full beside it. Returns what changed for the one
    /// selected, with its name before the change.
    pub fn contents(&mut self, ui: &mut egui::Ui, current: &str, version: &str) -> Managed {
        let mut out = Managed::default();
        self.manager_body(ui, current, version, &mut out);
        out
    }

    fn manager_body(&mut self, ui: &mut egui::Ui, current: &str, version: &str, out: &mut Managed) {
        let pal = theme::pal(ui);
        // A load failure can leave no selected calibre; show the warning before
        // the details pane's early return, not only beside a selected entry.
        if let Some(problem) = &self.problem {
            ui.label(RichText::new(problem).color(pal.bad));
        }
        // Its "?" is beside the Calibres heading above.
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("New Custom Calibre…")
                    .on_hover_text(
                        "Add a calibre the list doesn't have, named as typed in the search \
                         box, with the wheels' periods you type; it is kept for next time",
                    )
                    .clicked()
                {
                    let name = self.new_custom(&self.search.clone());
                    self.search.clear();
                    // The new one shows, whatever was filtered out.
                    if !self.rows().iter().any(|e| e.name() == name) {
                        self.funnels = Default::default();
                        self.short_only = false;
                    }
                    self.selected = Some(name);
                }
                if ui
                    .add_enabled(
                        !self.short_is_default(),
                        egui::Button::new("Reset the Short List"),
                    )
                    .on_hover_text("Back to the common and modern calibres the app comes with")
                    .clicked()
                {
                    self.short_added.clear();
                    self.short_removed.clear();
                }
            });
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .id_salt("manage search")
                    .hint_text("Search calibres, makers and other names")
                    .desired_width(260.0),
            )
            .on_hover_text(
                "Every word must match: a calibre's number, one of its other names, or its \
                 maker under any of the maker's names (\"dd 3235\", \"sw200\", \"seagull st19\")",
            );
            let filtered = self.funnels.iter().any(Option::is_some)
                || !self.search.is_empty()
                || self.short_only;
            if ui
                .add_enabled(filtered, egui::Button::new("Clear Filters"))
                .on_hover_text(
                    "Show every calibre again: clears the search, Short List Only and every \
                     column's funnel",
                )
                .on_disabled_hover_text("Nothing is filtered")
                .clicked()
            {
                self.funnels = Default::default();
                self.search.clear();
                self.short_only = false;
            }
            ui.add_space(8.0);
            let hint = "Show only the calibres on your short list, which the Calibre menu offers";
            let l = ui.add(egui::Label::new("Short List Only").sense(egui::Sense::click()));
            if l.clicked() {
                self.short_only = !self.short_only;
            }
            l.on_hover_text(hint);
            theme::switch(ui, &mut self.short_only).on_hover_text(hint);
        });
        ui.add_space(6.0);
        // By name, so the table can change what it holds as it draws.
        let rows: Vec<(bool, String)> = self
            .rows()
            .iter()
            .map(|e| (e.is_custom(), e.name().to_string()))
            .collect();
        // The details beside it keep room for a wheel's row, with Clear.
        let room = ui.available_width();
        let table_w = (room * 0.62).min(room - DETAILS_W).max(440.0);
        let mut toggles: Vec<(String, bool)> = Vec::new();
        let mut select: Option<String> = None;
        ui.horizontal_top(|ui| {
            let h = ui.available_height();
            let down = egui::Layout::top_down(egui::Align::Min);
            ui.allocate_ui_with_layout(Vec2::new(table_w, h), down, |ui| {
                ui.set_min_size(Vec2::new(table_w, h));
                theme::card().fill(pal.card).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    let widths = col_widths(ui.available_width());
                    self.table_head(ui, &widths);
                    ui.separator();
                    if rows.is_empty() {
                        let empty = self.short_only
                            && self.search.trim().is_empty()
                            && !self.funnels.iter().any(Option::is_some)
                            && !calibres::all().iter().any(|c| self.in_short(c));
                        ui.label(
                            RichText::new(if empty {
                                "Empty: tick calibres with Short List Only off to put them here."
                            } else {
                                "No calibre matches."
                            })
                            .small()
                            .color(pal.text_tertiary),
                        );
                    }
                    // Rows touch, as a table's do.
                    ui.spacing_mut().item_spacing.y = 0.0;
                    egui::ScrollArea::vertical()
                        .id_salt("manage list")
                        .auto_shrink([false, false])
                        .show_rows(ui, ROW_H, rows.len(), |ui, range| {
                            for (custom, name) in &rows[range] {
                                let Some(e) = self.entry(*custom, name) else {
                                    continue;
                                };
                                if self.table_row(ui, e, &widths, current, &mut toggles) {
                                    select = Some(name.clone());
                                }
                            }
                        });
                });
            });
            ui.add_space(8.0);
            // The details never widen the window: what doesn't fit is cut
            // off at its edge.
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), h), egui::Sense::hover());
            let mut d = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(down));
            d.set_clip_rect(rect.intersect(ui.clip_rect()));
            egui::ScrollArea::vertical()
                .id_salt("manage details")
                .auto_shrink([false, false])
                .show(&mut d, |ui| self.details(ui, current, version, out));
        });
        for (name, on) in toggles {
            self.set_short(&name, on);
        }
        if select.is_some() {
            self.selected = select;
        }
    }

    /// Every calibre: the user's own, then the list's.
    fn every(&self) -> impl Iterator<Item = Entry<'_>> {
        self.user
            .custom
            .iter()
            .map(Entry::Custom)
            .chain(calibres::all().iter().map(Entry::Table))
    }

    /// The custom calibre or the list's called exactly `name`.
    fn entry(&self, custom: bool, name: &str) -> Option<Entry<'_>> {
        if custom {
            self.user
                .custom
                .iter()
                .find(|c| c.name == name)
                .map(Entry::Custom)
        } else {
            calibres::all()
                .iter()
                .find(|c| c.calibre == name)
                .map(Entry::Table)
        }
    }

    /// What a calibre shows in a column, as its funnel lists it.
    fn cell(&self, e: Entry<'_>, col: Col) -> String {
        match col {
            Col::Maker => match e {
                Entry::Custom(_) => CUSTOM.into(),
                Entry::Table(c) => c.maker.clone(),
            },
            Col::Calibre => e.name().into(),
            Col::BeatRate => e.bph().map_or(NOT_GIVEN.into(), |b| b.to_string()),
            Col::LiftAngle => e
                .lift_angle_deg()
                .map_or(NOT_GIVEN.into(), |l| format!("{}°", fields::plain(l))),
            Col::Short => match e {
                Entry::Table(c) if self.in_short(c) => "Ticked".into(),
                _ => "Not Ticked".into(),
            },
            Col::Custom => match e {
                Entry::Custom(_) => CUSTOM.into(),
                Entry::Table(_) => "Listed".into(),
            },
        }
    }

    /// The calibres the search, Short List Only and every column's filter
    /// but `skip`'s let through, in no order.
    fn passing(&self, skip: Option<Col>) -> Vec<Entry<'_>> {
        let typed = self.search.trim();
        let found: Option<std::collections::HashSet<(bool, &str)>> =
            (!typed.is_empty()).then(|| {
                Catalog::new(&self.user)
                    .search(typed)
                    .into_iter()
                    .map(|e| (e.is_custom(), e.name()))
                    .collect()
            });
        self.every()
            .filter(|e| {
                found
                    .as_ref()
                    .is_none_or(|f| f.contains(&(e.is_custom(), e.name())))
            })
            .filter(|e| !self.short_only || matches!(e, Entry::Table(c) if self.in_short(c)))
            .filter(|e| {
                COLS.iter().all(|&(col, ..)| {
                    Some(col) == skip
                        || self.funnels[col as usize]
                            .as_ref()
                            .is_none_or(|keep| keep.contains(&self.cell(*e, col)))
                })
            })
            .collect()
    }

    /// The table's rows: the calibres let through, the custom ones at the
    /// top whatever the order, the rest by the column sorted on.
    fn rows(&self) -> Vec<Entry<'_>> {
        let mut rows = self.passing(None);
        rows.sort_by(|a, b| {
            b.is_custom()
                .cmp(&a.is_custom())
                .then_with(|| self.order(a, b))
        });
        rows
    }

    /// Two calibres in the order of the column sorted on, reversed when
    /// asked, then by maker and name. A blank comes after every value,
    /// either way round.
    fn order(&self, a: &Entry<'_>, b: &Entry<'_>) -> std::cmp::Ordering {
        let maker = |e: &Entry<'_>| self.cell(*e, Col::Maker).to_lowercase();
        let short = |e: &Entry<'_>| !matches!(e, Entry::Table(c) if self.in_short(c));
        let blank = |e: &Entry<'_>| match self.sort.0 {
            Col::BeatRate => e.bph().is_none(),
            Col::LiftAngle => e.lift_angle_deg().is_none(),
            _ => false,
        };
        let by = match self.sort.0 {
            Col::Maker => maker(a).cmp(&maker(b)),
            Col::Calibre => natural(a.name(), b.name()),
            Col::BeatRate => a.bph().cmp(&b.bph()),
            Col::LiftAngle => a
                .lift_angle_deg()
                .partial_cmp(&b.lift_angle_deg())
                .unwrap_or(std::cmp::Ordering::Equal),
            Col::Short => short(a).cmp(&short(b)),
            Col::Custom => b.is_custom().cmp(&a.is_custom()),
        };
        let by = if self.sort.1 { by.reverse() } else { by };
        blank(a)
            .cmp(&blank(b))
            .then(by)
            .then_with(|| maker(a).cmp(&maker(b)))
            .then_with(|| natural(a.name(), b.name()))
    }

    /// The values a column's funnel offers: what the calibres the other
    /// filters let through show in it, in name order.
    fn funnel_values(&self, col: Col) -> Vec<String> {
        let mut v: Vec<String> = self
            .passing(Some(col))
            .into_iter()
            .map(|e| self.cell(e, col))
            .collect();
        v.sort_by(|a, b| natural(a, b));
        v.dedup();
        v
    }

    /// Show `value` in `col` or not, the other values offered staying as
    /// they were.
    fn set_funnel(&mut self, col: Col, value: &str, on: bool) {
        let offered = self.funnel_values(col);
        let keep =
            self.funnels[col as usize].get_or_insert_with(|| offered.iter().cloned().collect());
        if on {
            keep.insert(value.to_string());
        } else {
            keep.remove(value);
        }
        if offered.iter().all(|v| keep.contains(v)) {
            self.funnels[col as usize] = None;
        }
    }

    /// The headings: a click sorts on one, a second click reverses it;
    /// each has a funnel to pick the values it shows.
    fn table_head(&mut self, ui: &mut egui::Ui, widths: &[f32; 6]) {
        let pal = theme::pal(ui);
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(widths.iter().sum(), ROW_H), egui::Sense::hover());
        let mut x = rect.left();
        for (i, &(col, title, hint)) in COLS.iter().enumerate() {
            let cell =
                egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(widths[i], ROW_H));
            x += widths[i];
            let funnel = egui::Rect::from_center_size(
                egui::pos2(cell.right() - 9.0, cell.center().y),
                Vec2::splat(15.0),
            );
            let words =
                egui::Rect::from_min_max(cell.min, egui::pos2(funnel.left() - 2.0, cell.bottom()));
            let sorted = self.sort.0 == col;
            let r = ui
                .interact(
                    words,
                    ui.id().with(("calibre heading", i)),
                    egui::Sense::click(),
                )
                .on_hover_text(format!(
                    "{hint}. Click to sort on it; click again to reverse the order"
                ));
            if r.clicked() {
                self.sort = (col, sorted && !self.sort.1);
            }
            let ink = if sorted || r.hovered() {
                pal.text
            } else {
                pal.text_secondary
            };
            let galley = ui.painter().layout_no_wrap(
                title.to_string(),
                egui::FontId::proportional(11.5),
                ink,
            );
            let at = egui::pos2(cell.left() + 4.0, cell.center().y - galley.size().y / 2.0);
            let text_right = at.x + galley.size().x;
            ui.painter().with_clip_rect(words).galley(at, galley, ink);
            if sorted {
                // A small triangle: up for the order as it reads, down reversed.
                let c = egui::pos2((text_right + 6.0).min(words.right() - 3.0), cell.center().y);
                let s = 3.0;
                let pts = if self.sort.1 {
                    vec![
                        c + Vec2::new(-s, -s / 2.0),
                        c + Vec2::new(s, -s / 2.0),
                        c + Vec2::new(0.0, s),
                    ]
                } else {
                    vec![
                        c + Vec2::new(-s, s / 2.0),
                        c + Vec2::new(s, s / 2.0),
                        c + Vec2::new(0.0, -s),
                    ]
                };
                ui.painter()
                    .add(egui::Shape::convex_polygon(pts, ink, egui::Stroke::NONE));
            }
            let on = self.funnels[col as usize].is_some();
            let fr = ui
                .interact(
                    funnel,
                    ui.id().with(("calibre funnel", i)),
                    egui::Sense::click(),
                )
                .on_hover_text(if on {
                    format!("{title} is filtered: click to change which values show")
                } else {
                    format!("Pick which {title} values show")
                });
            if on {
                ui.painter()
                    .rect_filled(funnel, 3.0, pal.accent.gamma_multiply(0.18));
            } else if fr.hovered() {
                ui.painter().rect_filled(funnel, 3.0, pal.control_hover);
            }
            let ink = if on { pal.accent } else { pal.text_tertiary };
            crate::icons::draw(ui.painter(), funnel.shrink(1.5), theme::Icon::Funnel, ink);
            if fr.clicked() {
                self.funnel_search.clear();
            }
            egui::Popup::menu(&fr)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .width(220.0)
                .show(|ui| self.funnel_menu(ui, col, title));
        }
    }

    /// A column's funnel: its values, each with a tickbox, a search over
    /// them, and Select All.
    fn funnel_menu(&mut self, ui: &mut egui::Ui, col: Col, title: &str) {
        let pal = theme::pal(ui);
        ui.add(
            egui::TextEdit::singleline(&mut self.funnel_search)
                .hint_text(format!("Search {title} values"))
                .desired_width(f32::INFINITY),
        )
        .on_hover_text("List only the values holding this");
        let offered = self.funnel_values(col);
        let typed = self.funnel_search.trim().to_lowercase();
        let listed: Vec<&String> = offered
            .iter()
            .filter(|v| typed.is_empty() || v.to_lowercase().contains(&typed))
            .collect();
        let shown = |s: &Self, v: &str| {
            s.funnels[col as usize]
                .as_ref()
                .is_none_or(|k| k.contains(v))
        };
        let mut all = listed.iter().all(|v| shown(self, v));
        if ui
            .checkbox(&mut all, "Select All")
            .on_hover_text("Tick or untick every value listed")
            .changed()
        {
            if typed.is_empty() {
                self.funnels[col as usize] = (!all).then(Default::default);
            } else {
                for v in &listed {
                    self.set_funnel(col, v, all);
                }
            }
        }
        ui.separator();
        if listed.is_empty() {
            ui.label(
                RichText::new("No value matches.")
                    .small()
                    .color(pal.text_tertiary),
            );
        }
        let mut flips: Vec<(String, bool)> = Vec::new();
        egui::ScrollArea::vertical()
            .id_salt(("funnel values", col as usize))
            .max_height(260.0)
            .show_rows(ui, 22.0, listed.len(), |ui, range| {
                for v in &listed[range] {
                    let mut on = shown(self, v);
                    if ui
                        .checkbox(&mut on, v.as_str())
                        .on_hover_text(format!("Show the calibres whose {title} is {v}"))
                        .changed()
                    {
                        flips.push(((*v).clone(), on));
                    }
                }
            });
        for (v, on) in flips {
            self.set_funnel(col, &v, on);
        }
    }

    /// One calibre's row; true when it was clicked.
    fn table_row(
        &self,
        ui: &mut egui::Ui,
        e: Entry<'_>,
        widths: &[f32; 6],
        current: &str,
        toggles: &mut Vec<(String, bool)>,
    ) -> bool {
        let pal = theme::pal(ui);
        let (rect, r) =
            ui.allocate_exact_size(Vec2::new(widths.iter().sum(), ROW_H), egui::Sense::click());
        let r = r.on_hover_text(hint(e));
        let selected = self.selected.as_deref() == Some(e.name());
        if selected {
            ui.painter()
                .rect_filled(rect, 4.0, pal.accent.gamma_multiply(0.22));
        } else if r.hovered() {
            ui.painter().rect_filled(rect, 4.0, pal.control_hover);
        }
        let mut x = rect.left();
        for (i, &(col, ..)) in COLS.iter().enumerate() {
            let cell =
                egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(widths[i], ROW_H));
            x += widths[i];
            match col {
                Col::Short => {
                    if let Entry::Table(c) = e {
                        let mut on = self.in_short(c);
                        let at = egui::Rect::from_min_size(
                            egui::pos2(cell.left() + 4.0, cell.top()),
                            Vec2::new(ui.spacing().interact_size.y, ROW_H),
                        );
                        if ui
                            .put(at, egui::Checkbox::without_text(&mut on))
                            .on_hover_text(if on {
                                "On your short list: untick to keep it only in the full list"
                            } else {
                                "Tick to put it on your short list, offered in the Calibre menu"
                            })
                            .changed()
                        {
                            toggles.push((c.calibre.clone(), on));
                        }
                    }
                }
                Col::Custom => {
                    if e.is_custom() {
                        // A tick: a calibre you added.
                        let c = egui::pos2(cell.left() + 12.0, cell.center().y);
                        ui.painter().line(
                            vec![
                                c + Vec2::new(-4.0, 0.0),
                                c + Vec2::new(-1.0, 3.0),
                                c + Vec2::new(4.5, -3.5),
                            ],
                            egui::Stroke::new(1.6_f32, pal.accent),
                        );
                    }
                }
                _ => {
                    let mut text = self.cell(e, col);
                    if col == Col::Calibre {
                        text = short_name(e).to_string();
                        if e.name() == current {
                            text += "  (this watch)";
                        }
                    }
                    let ink = match col {
                        Col::Calibre => pal.text,
                        Col::Maker if e.is_custom() => pal.accent,
                        _ => pal.text_secondary,
                    };
                    // Too long for its column: cut with "…", a calibre in
                    // the middle to keep its number at the end, the whole
                    // of it in the row's hover.
                    let font = egui::FontId::proportional(12.5);
                    let room = widths[i] - 8.0;
                    let mut galley = ui.painter().layout_no_wrap(text.clone(), font.clone(), ink);
                    let chars: Vec<char> = text.chars().collect();
                    let mut keep = chars.len();
                    while galley.size().x > room && keep > 2 {
                        keep -= 1;
                        // A maker reads best from its start.
                        let tail = if col == Col::Calibre { keep / 2 } else { 0 };
                        let head = keep - tail;
                        let cut: String = chars[..head]
                            .iter()
                            .chain(std::iter::once(&'…'))
                            .chain(&chars[chars.len() - tail..])
                            .collect();
                        galley = ui.painter().layout_no_wrap(cut, font.clone(), ink);
                    }
                    let at = egui::pos2(cell.left() + 4.0, cell.center().y - galley.size().y / 2.0);
                    ui.painter()
                        .with_clip_rect(cell.shrink2(Vec2::new(2.0, 0.0)))
                        .galley(at, galley, ink);
                }
            }
        }
        r.clicked()
    }
    /// The calibre selected in Manage Calibres, in full.
    fn details(&mut self, ui: &mut egui::Ui, current: &str, version: &str, out: &mut Managed) {
        let pal = theme::pal(ui);
        let Some(name) = self.selected.clone().filter(|n| self.find(n).is_some()) else {
            ui.label(
                RichText::new(
                    "Select a calibre in the table to see its wheels, sources and notes, and \
                     add what you know.",
                )
                .color(pal.text_secondary),
            );
            return;
        };
        let Some(e) = self.find(&name) else { return };
        let custom = e.is_custom();
        let mut title = match e {
            Entry::Table(c) => with_maker_name(&c.maker, &c.calibre),
            Entry::Custom(_) => label(e),
        };
        if name == current {
            title += "  (this watch)";
        }
        ui.label(RichText::new(title).strong().size(15.0));
        ui.label(RichText::new(summary(e)).small().color(pal.text_secondary));
        if let Entry::Table(c) = e {
            if !c.family.is_empty() {
                ui.label(
                    RichText::new(format!("Also known as {}", c.family.join(", ")))
                        .small()
                        .color(pal.text_tertiary),
                );
            }
        }
        ui.add_space(6.0);
        if custom {
            out.change = self.custom_editor(ui, &name, "manager");
            ui.add_space(6.0);
        }
        if !out.change.deleted {
            out.change.wheels |= self.wheels_and_sources(ui, &name, version, "manager");
        }
        if out.change.deleted {
            self.selected = None;
        } else if let Some(n) = &out.change.renamed {
            self.selected = Some(n.clone());
        }
        out.name = name;
    }

    /// A custom calibre's name, maker, beat rate and lift angle, and the
    /// way to delete it.
    fn custom_editor(&mut self, ui: &mut egui::Ui, name: &str, place: &str) -> Change {
        let mut change = Change::default();
        let Some(i) = self.user.custom.iter().position(|c| c.name == name) else {
            return change;
        };
        let before = self.user.custom[i].clone();
        let c = &mut self.user.custom[i];
        theme::row(
            ui,
            "Name",
            Some("What the calibre is called in the menu"),
            |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut c.name)
                        .id_salt(("custom name", place))
                        .desired_width(f32::INFINITY),
                );
            },
        );
        let mut maker = c.maker.clone().unwrap_or_default();
        theme::row(
            ui,
            "Maker",
            Some("Who made the movement, to search by"),
            |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut maker)
                        .id_salt(("custom maker", place))
                        .desired_width(f32::INFINITY),
                );
            },
        );
        c.maker = (!maker.trim().is_empty()).then(|| maker.trim().to_string());
        theme::row(
            ui,
            "Beat Rate",
            Some("Beats per hour, as the maker gives it"),
            |ui| {
                let mut b = c.bph.map_or(0.0, f64::from);
                if fields::entry(
                    ui,
                    &format!("custom-bph-{place}-{i}"),
                    &mut b,
                    0.0..=72000.0,
                    &BPH,
                    70.0,
                ) {
                    c.bph = (b > 0.0).then_some(b.round() as u32);
                }
                ui.label("bph");
            },
        );
        theme::row(
            ui,
            "Lift Angle",
            Some("The lift angle the maker gives, used for the amplitude when picked"),
            |ui| {
                let mut l = c.lift_angle_deg.unwrap_or(0.0);
                if fields::entry(
                    ui,
                    &format!("custom-lift-{place}-{i}"),
                    &mut l,
                    0.0..=90.0,
                    &LIFT,
                    50.0,
                ) {
                    c.lift_angle_deg = (l > 0.0).then_some(l);
                    change.lift = c.lift_angle_deg;
                }
                ui.label("°");
            },
        );
        let renamed = c.name.trim().to_string();
        if ui
            .button("Delete Calibre")
            .on_hover_text("Forget this calibre and the wheels typed for it")
            .clicked()
        {
            self.user.remove_custom(name);
            change.deleted = true;
        } else if renamed != before.name {
            // A name another calibre has, or none, isn't taken.
            let free = !renamed.is_empty()
                && Catalog::new(&self.user)
                    .find(&renamed)
                    .is_none_or(|e| e.is_custom() && e.name() == self.user.custom[i].name);
            if free {
                self.user.custom[i].name = renamed.clone();
                change.renamed = Some(renamed);
            } else {
                self.user.custom[i].name = before.name.clone();
            }
        }
        if change.deleted || self.user.custom.get(i) != Some(&before) {
            self.save();
        }
        change
    }
}

const PERIOD: fields::Format = fields::Format {
    show: |v| {
        let s = format!("{v:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    },
    parse: fields::parse_number,
};
const BPH: fields::Format = fields::Format {
    show: |v| {
        if v > 0.0 {
            format!("{v:.0}")
        } else {
            String::new()
        }
    },
    parse: fields::parse_number,
};
const LIFT: fields::Format = fields::Format {
    show: |v| {
        if v > 0.0 {
            fields::plain(v)
        } else {
            String::new()
        }
    },
    parse: fields::parse_number,
};

/// A custom calibre from wheel names and periods, names tidied and
/// unnamed or unusable wheels left out.
pub(crate) fn custom<'a>(
    name: &str,
    wheels: impl IntoIterator<Item = (&'a str, f64)>,
) -> CustomCalibre {
    CustomCalibre {
        name: name.trim().to_string(),
        maker: None,
        aliases: Vec::new(),
        bph: None,
        lift_angle_deg: None,
        escape_teeth: None,
        wheels: wheels
            .into_iter()
            .filter(|(n, p)| !n.trim().is_empty() && *p > 0.0)
            .map(|(n, p)| CustomWheel {
                name: n.trim().to_lowercase(),
                period_s: p,
            })
            .collect(),
        notes: None,
    }
}

/// Every table calibre by maker: the common makers first, then the rest,
/// each in name order, calibres in name order.
fn by_maker() -> &'static [(&'static str, bool, Vec<&'static Calibre>)] {
    static LIST: OnceLock<Vec<(&'static str, bool, Vec<&'static Calibre>)>> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut map: std::collections::BTreeMap<&str, Vec<&Calibre>> = Default::default();
        for c in calibres::all() {
            map.entry(c.maker.as_str()).or_default().push(c);
        }
        let common = |m: &str| {
            calibres::makers().iter().any(|x| x.maker == m && x.common)
                || map.get(m).is_some_and(|l| l.iter().any(|c| c.common))
        };
        let mut out: Vec<(&str, bool, Vec<&Calibre>)> = map
            .iter()
            .map(|(m, l)| {
                let mut l = l.clone();
                l.sort_by(|a, b| natural(&a.calibre, &b.calibre));
                (*m, common(m), l)
            })
            .collect();
        out.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
        });
        out
    })
}

/// Names with numbers compared as numbers.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    crate::app::natural_order(a, b)
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.separator();
    ui.label(theme::caption(text).color(theme::pal(ui).text_secondary));
}

/// A calibre's line in the search results: its name, and "Custom" for one
/// the user added.
fn label(e: Entry<'_>) -> String {
    match e {
        Entry::Table(c) => c.calibre.clone(),
        Entry::Custom(c) => format!("{} · {CUSTOM}", c.name),
    }
}

/// What the list says about a calibre, for its line in the menu.
/// A calibre's name for the table's Calibre column: without its maker in
/// front when the Maker column already says it ("970" for A. Schild 970).
fn short_name<'a>(e: Entry<'a>) -> &'a str {
    match e {
        Entry::Table(c) => c
            .calibre
            .strip_prefix(c.maker.as_str())
            .and_then(|rest| rest.strip_prefix(' '))
            .filter(|rest| !rest.trim().is_empty())
            .unwrap_or(&c.calibre),
        Entry::Custom(_) => e.name(),
    }
}

fn hint(e: Entry<'_>) -> String {
    // The name first: the table may cut it short.
    let mut t = format!("{}\n{}", e.name(), summary(e));
    if let Entry::Table(c) = e {
        if !c.family.is_empty() {
            t += &format!("\nAlso: {}", c.family.join(", "));
        }
        for w in &c.wheels {
            t += &format!("\n{}: {} s", title_case(&w.name), (PERIOD.show)(w.period_s));
        }
    }
    t
}

/// Maker, beat rate, lift angle and escape wheel in one line, saying
/// which the list doesn't give.
fn summary(e: Entry<'_>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(m) = e.maker() {
        parts.push(m.to_string());
    }
    parts.push(match e.bph() {
        Some(b) => format!("{b} bph"),
        None => "beat rate not given".into(),
    });
    parts.push(match e.lift_angle_deg() {
        Some(l) => format!("lift angle {}°", fields::plain(l)),
        None => "lift angle not given".into(),
    });
    if let Some(t) = e.escape_teeth() {
        parts.push(format!("{t}-tooth escape wheel"));
    }
    parts.join(" · ")
}

/// "escape wheel" as "Escape Wheel".
fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// One source: a link when it starts with an address, the rest of the
/// line after it as its text.
fn source_line(ui: &mut egui::Ui, s: &str) {
    let (url, rest) = s.split_once(' ').unwrap_or((s, ""));
    if url.starts_with("http") {
        let host = url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or(url);
        let text = match rest.trim() {
            "" => host.to_string(),
            r => format!("{host} {r}"),
        };
        ui.hyperlink_to(RichText::new(text).small(), url)
            .on_hover_text(url);
    } else {
        ui.add(egui::Label::new(RichText::new(s).small()).wrap());
    }
}

/// A box for a wheel's period that may be blank. Enter or clicking away
/// reads it: `Some(Some(v))` for a new period, `Some(None)` when emptied,
/// `None` when nothing changed.
fn period_field(
    ui: &mut egui::Ui,
    id_salt: &str,
    value: Option<f64>,
    tip: &str,
) -> Option<Option<f64>> {
    let id = ui.make_persistent_id(id_salt);
    let buf = id.with("text");
    let shown = value.map(|v| (PERIOD.show)(v)).unwrap_or_default();
    let mut text = ui
        .data_mut(|d| d.get_temp::<String>(buf))
        .unwrap_or_else(|| shown.clone());
    let resp = ui
        .add(
            egui::TextEdit::singleline(&mut text)
                .id(id)
                .hint_text("not known")
                .desired_width(66.0),
        )
        .on_hover_text(tip);
    let mut out = None;
    if resp.lost_focus() {
        if text.trim() != shown {
            out = match fields::parse_number(&text) {
                Some(v) if v > 0.0 => Some(Some(v)),
                _ if text.trim().is_empty() && value.is_some() => Some(None),
                _ => None,
            };
        }
        ui.data_mut(|d| d.remove::<String>(buf));
    } else if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(buf, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(buf));
    }
    out
}

/// A calibre's name with its maker in front, unless the name already
/// starts with the maker under one of its names ("Rolex 3235", not "Rolex
/// Rolex 3235"; "Seagull ST36", not "Tianjin Seagull Seagull ST36").
fn with_maker_name(maker: &str, name: &str) -> String {
    // A clone maker's marker is already in a name such as "Patek 240 Clone",
    // so only the maker itself goes in front.
    if let Some(outer) = maker.strip_suffix("(clone)").map(str::trim_end) {
        if name
            .split(|ch: char| !ch.is_alphanumeric())
            .any(|word| word.eq_ignore_ascii_case("clone"))
        {
            return match outer {
                "" => name.to_string(),
                outer => format!("{outer} {name}"),
            };
        }
    }
    let starts_with = |word: &str| {
        !word.is_empty()
            && name
                .get(..word.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(word))
            && name[word.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric())
    };
    if maker_words(maker).iter().any(|w| starts_with(w)) {
        // The maker goes, but not what it says about the calibre.
        match maker.strip_suffix(" (clone)") {
            Some(_) => format!("{name} (clone)"),
            None => name.to_string(),
        }
    } else {
        format!("{maker} {name}")
    }
}

/// The names a maker goes by: its own, its other names, and the parts of
/// a name like "Peacock (Dandong)".
fn maker_words(maker: &str) -> Vec<&str> {
    let mut words = calibres::maker_names(maker);
    if let Some((outer, inner)) = maker.split_once(" (") {
        words.push(outer);
        words.push(inner.trim_end_matches(')'));
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_maker_is_named_once() {
        assert_eq!(with_maker_name("Rolex", "Rolex 3235"), "Rolex 3235");
        assert_eq!(with_maker_name("Rolex", "rolex 3235"), "rolex 3235");
        assert_eq!(with_maker_name("ETA", "2824-2"), "ETA 2824-2");
        // A name that only begins with the same letters is a different word.
        assert_eq!(
            with_maker_name("Seiko", "SeikoMatic 6218"),
            "Seiko SeikoMatic 6218"
        );
        // Under another of the maker's names.
        assert_eq!(
            with_maker_name("Tianjin Seagull", "Seagull ST36"),
            "Seagull ST36"
        );
        // A clone stays marked as one.
        assert_eq!(
            with_maker_name("APS (clone)", "APS 4401"),
            "APS 4401 (clone)"
        );
        for name in ["Patek 240 Clone", "Patek 240 (clone)", "Patek 240 CLONE"] {
            assert_eq!(with_maker_name("(clone)", name), name);
        }
        assert_eq!(
            with_maker_name("Shanghai (clone)", "Patek 240 Clone"),
            "Shanghai Patek 240 Clone"
        );
        // Do not lose the clone marker when it is absent or only a substring.
        assert_eq!(with_maker_name("(clone)", "240"), "(clone) 240");
        assert_eq!(
            with_maker_name("(clone)", "Clonecraft 240"),
            "(clone) Clonecraft 240"
        );
    }

    #[test]
    fn saved_picks_follow_the_tables_renames() {
        let c = calibres::all()
            .iter()
            .find(|c| !c.former_names.is_empty())
            .expect("a renamed calibre in the table");
        let (old, new) = (c.former_names[0].clone(), c.calibre.clone());
        let mut cal = Calibres::in_memory();
        assert_eq!(cal.current_name(&old), new);
        assert_eq!(cal.current_name("Not in the table"), "Not in the table");
        // Saved under both names: one row, and on the short list either way.
        cal.restore(
            vec![old.clone(), new.clone()],
            vec![old.clone()],
            vec![old.clone()],
        );
        assert_eq!(cal.recent, vec![new.clone()]);
        assert_eq!(cal.short_added, vec![new.clone()]);
        assert_eq!(cal.short_removed, vec![new.clone()]);
        assert!(cal.find(&new).is_some_and(|e| e.name() == new));
    }

    #[test]
    fn no_row_in_the_table_names_its_maker_twice() {
        let mut twice = Vec::new();
        for c in calibres::all() {
            let row = with_maker_name(&c.maker, &c.calibre);
            let words: Vec<String> = row
                .split(|ch: char| !ch.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .map(str::to_lowercase)
                .collect();
            let repeated = maker_words(&c.maker).iter().any(|name| {
                name.split(|ch: char| !ch.is_alphanumeric())
                    .filter(|w| w.len() > 2)
                    .any(|w| words.iter().filter(|x| **x == w.to_lowercase()).count() > 1)
            });
            if repeated {
                twice.push(row);
            }
        }
        assert!(
            twice.is_empty(),
            "{} rows: {:?}",
            twice.len(),
            &twice[..twice.len().min(10)]
        );
    }

    fn store_fixture(label: &str) -> (PathBuf, PathBuf) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "tg-calibre-protection-{label}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("calibres.json");
        (dir, path)
    }

    fn legacy_calibre() -> settings::CustomCalibre {
        settings::CustomCalibre {
            name: "Legacy Bench".into(),
            wheels: vec![settings::CustomWheel {
                name: " Third Wheel ".into(),
                period_s: 450.0,
            }],
        }
    }

    #[test]
    fn unreadable_catalogue_is_never_replaced_by_fallback_edits() {
        for bytes in [
            b"{ invalid catalogue, preserve me".as_slice(),
            br#"{"schema":"timegrapher.user-calibres/future","custom":[],"wheels":[]}"#,
        ] {
            let (dir, path) = store_fixture("blocked");
            std::fs::write(&path, bytes).unwrap();
            let mut c = Calibres::open(Some(path.clone()));
            assert!(c.user.custom.is_empty());
            let problem = c.problem.clone().expect("load failure is visible");
            assert!(problem.contains(&path.display().to_string()));
            assert!(c.find("Rolex 4130").is_some(), "built-ins remain usable");
            c.new_custom("Temporary New");
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            c.adopt("Temporary Imported", &[("third wheel".into(), 123.0)]);
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            c.migrate(vec![legacy_calibre()]);
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert_eq!(c.pending_legacy(), &[legacy_calibre()]);
            // Wheel changes and custom editor updates share this save boundary.
            c.user.set_wheel("Rolex 4130", "third wheel", 400.0, None);
            c.user.remove_custom("Temporary New");
            c.save();
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert_eq!(c.problem.as_ref(), Some(&problem));
            assert!(problem.contains("only in memory"));
            assert!(!path.with_extension("json.tmp").exists());
            // Repairing externally does not authorize this empty-fallback
            // instance to overwrite a catalogue it has never loaded.
            let mut repaired = UserCalibres::default();
            repaired.put_custom(custom("Repaired Original", [("third wheel", 321.0)]));
            repaired.save(&path).unwrap();
            let repaired_bytes = std::fs::read(&path).unwrap();
            c.new_custom("Still Temporary");
            assert_eq!(std::fs::read(&path).unwrap(), repaired_bytes);
            let reopened = Calibres::open(Some(path));
            assert!(reopened.problem.is_none());
            assert!(reopened.find("Repaired Original").is_some());
            assert!(reopened.find("Still Temporary").is_none());
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn unreadable_directory_catalogue_stays_blocked_after_path_repair() {
        let (dir, path) = store_fixture("directory");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("original"), b"keep").unwrap();
        let mut c = Calibres::open(Some(path.clone()));
        c.new_custom("Temporary");
        assert_eq!(std::fs::read(path.join("original")).unwrap(), b"keep");
        std::fs::remove_dir_all(&path).unwrap();
        c.new_custom("Still Temporary");
        assert!(
            !path.exists(),
            "a repaired path does not unblock this instance"
        );
        assert!(c.problem.as_ref().unwrap().contains("only in memory"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn legacy_settings_survive_failed_save_until_successful_retry() {
        let (dir, path) = store_fixture("retry");
        let mut c = Calibres::open(Some(path.clone()));
        // Deterministic rename failure, without permission assumptions.
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("original"), b"keep").unwrap();
        let old = vec![legacy_calibre()];
        c.migrate(old.clone());
        assert_eq!(c.pending_legacy(), old);
        assert!(c.find("Legacy Bench").is_some());
        assert!(c.problem.as_ref().unwrap().contains("couldn't be saved"));
        assert_eq!(std::fs::read(path.join("original")).unwrap(), b"keep");
        std::fs::remove_dir_all(&path).unwrap();
        // A normal later edit retries the save, including the migrated entry.
        c.adopt("Later Entry", &[("fourth wheel".into(), 60.0)]);
        assert!(c.pending_legacy().is_empty());
        assert!(c.problem.is_none());
        let reopened = Calibres::open(Some(path));
        assert!(reopened.find("Legacy Bench").is_some());
        assert!(reopened.find("Later Entry").is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_valid_and_memory_only_catalogues_keep_their_contracts() {
        let (dir, path) = store_fixture("valid");
        let mut c = Calibres::open(Some(path.clone()));
        assert!(c.problem.is_none());
        c.new_custom("First Entry");
        let mut c = Calibres::open(Some(path.clone()));
        c.adopt("Second Entry", &[("third wheel".into(), 123.0)]);
        c.migrate(vec![legacy_calibre()]);
        assert!(c.pending_legacy().is_empty());
        let reopened = Calibres::open(Some(path));
        for name in ["First Entry", "Second Entry", "Legacy Bench"] {
            assert!(reopened.find(name).is_some(), "{name}");
        }
        let mut memory = Calibres::in_memory();
        memory.new_custom("Memory Entry");
        memory.migrate(vec![legacy_calibre()]);
        assert!(memory.find("Memory Entry").is_some());
        assert!(memory.find("Legacy Bench").is_some());
        assert_eq!(memory.pending_legacy(), &[legacy_calibre()]);
        assert!(memory.problem.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn blocked_catalogue_warning_is_painted_without_a_selection() {
        let (dir, path) = store_fixture("warning");
        std::fs::write(&path, b"invalid JSON").unwrap();
        let mut c = Calibres::open(Some(path));
        assert!(c.selected.is_none());
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                c.manager_body(ui, "", "test", &mut Managed::default());
            });
        });
        let text = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("only in memory"), "{text}");
        assert!(text.contains("will not be overwritten"), "{text}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn settings_calibres_move_to_the_user_file_once() {
        let dir = std::env::temp_dir().join(format!("tg-calibres-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("calibres.json");
        let mut c = Calibres::open(Some(path.clone()));
        assert!(c.problem.is_none());
        let old = vec![settings::CustomCalibre {
            name: "Bench Watch".into(),
            wheels: vec![
                settings::CustomWheel {
                    name: " Third Wheel ".into(),
                    period_s: 450.0,
                },
                settings::CustomWheel {
                    name: String::new(),
                    period_s: 10.0,
                },
            ],
        }];
        c.migrate(old.clone());
        let again = Calibres::open(Some(path.clone()));
        assert_eq!(again.user.custom.len(), 1);
        assert_eq!(
            again.user.custom[0].wheels.len(),
            1,
            "the unnamed wheel is left out"
        );
        assert_eq!(again.user.custom[0].wheels[0].name, "third wheel");
        // Moving them again changes nothing.
        let mut again = again;
        again.migrate(old);
        assert_eq!(Calibres::open(Some(path)).user.custom.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wheels_typed_for_a_listed_calibre_name_its_cycles() {
        let mut c = Calibres::in_memory();
        let name = &calibres::common().next().unwrap().calibre;
        let before = c.wheels(name).unwrap().len();
        c.user.set_wheel(name, "third wheel", 450.0, None);
        let after = c.wheels(name).unwrap();
        assert!(after
            .iter()
            .any(|w| w.name == "third wheel" && w.period_s == 450.0));
        assert!(after.len() >= before);
        assert!(c.wheels("").is_none());
    }

    #[test]
    fn the_short_list_starts_common_and_follows_the_ticks() {
        let mut c = Calibres::in_memory();
        let common = calibres::common().next().unwrap();
        let rare = calibres::all().iter().find(|x| !x.common).unwrap();
        assert!(c.in_short(common) && !c.in_short(rare));
        c.set_short(&common.calibre, false);
        c.set_short(&rare.calibre, true);
        assert!(!c.in_short(common) && c.in_short(rare));
        assert_eq!(c.short_removed, std::slice::from_ref(&common.calibre));
        assert_eq!(c.short_added, std::slice::from_ref(&rare.calibre));
        c.set_short(&common.calibre, true);
        c.set_short(&rare.calibre, true);
        assert!(c.short_removed.is_empty() && c.short_added.len() == 1);
        c.set_short("no such calibre", true);
        assert_eq!(c.short_added.len(), 1);
    }

    #[test]
    fn recent_calibres_put_the_latest_first() {
        let mut c = Calibres::in_memory();
        for n in ["A", "B", "C", "A", "D", "E", "F", "G"] {
            c.remember(n);
        }
        assert_eq!(c.recent, ["G", "F", "E", "D", "A", "C"]);
        c.remember("");
        assert_eq!(c.recent.len(), RECENT);
    }

    #[test]
    fn a_new_custom_calibre_takes_the_typed_name_when_free() {
        let mut c = Calibres::in_memory();
        assert_eq!(c.new_custom("Bench 1"), "Bench 1");
        assert_eq!(c.new_custom("Bench 1"), "My Calibre 2");
        let listed = &calibres::all()[0].calibre;
        assert_eq!(c.new_custom(listed), "My Calibre 3");
        assert_eq!(c.wheels("Bench 1").unwrap().len(), 3);
    }

    #[test]
    fn every_calibre_is_under_one_maker_common_makers_first() {
        let list = by_maker();
        let n: usize = list.iter().map(|m| m.2.len()).sum();
        assert_eq!(n, calibres::all().len());
        let first_rare = list.iter().position(|m| !m.1).unwrap_or(list.len());
        assert!(list[first_rare..].iter().all(|m| !m.1));
    }

    #[test]
    fn titles_and_summaries_read_plainly() {
        assert_eq!(title_case("escape wheel"), "Escape Wheel");
        let e = Entry::Table(&calibres::all()[0]);
        assert!(summary(e).contains("bph") || summary(e).contains("beat rate not given"));
    }

    /// Manage Calibres' table: every calibre, Short List Only the ticked
    /// ones, the custom ones under the maker Custom, and a search keeps to
    /// what else is filtered.
    #[test]
    fn the_manager_lists_by_search_and_short_list() {
        let mut cal = Calibres::in_memory();
        let names = |c: &Calibres| -> Vec<String> {
            c.rows().iter().map(|e| e.name().to_string()).collect()
        };
        assert_eq!(names(&cal).len(), calibres::all().len());
        cal.short_only = true;
        let common = calibres::all().iter().filter(|c| cal.in_short(c)).count();
        assert_eq!(names(&cal).len(), common);
        assert!(common < calibres::all().len());
        cal.short_only = false;
        let mine = cal.new_custom("Bench Special");
        assert_eq!(names(&cal).len(), calibres::all().len() + 1);
        let e = cal.find(&mine).unwrap();
        assert_eq!(cal.cell(e, Col::Maker), "Custom");
        assert_eq!(cal.cell(e, Col::Custom), "Custom");
        cal.search = "bench".into();
        assert_eq!(names(&cal), vec![mine.clone()]);
        cal.funnels[Col::Maker as usize] = Some(["Custom".to_string()].into());
        cal.search = "3235".into();
        assert!(cal.rows().is_empty(), "a listed calibre isn't under Custom");
        cal.funnels = Default::default();
        assert!(!cal.rows().is_empty());
    }

    /// Each column's funnel keeps the values ticked; the funnels combine,
    /// each offers the values the others let through, and ticking every
    /// value again is no filter.
    #[test]
    fn the_funnels_combine_and_offer_what_the_others_let_through() {
        let mut cal = Calibres::in_memory();
        let makers = cal.funnel_values(Col::Maker);
        assert!(makers.len() > 10 && makers.windows(2).all(|w| w[0] != w[1]));
        let maker = calibres::all()
            .iter()
            .find(|c| c.bph.is_some() && c.maker == "ETA")
            .map_or_else(|| calibres::all()[0].maker.clone(), |c| c.maker.clone());
        // One maker: the rows are that maker's, the Beat Rate funnel offers
        // only that maker's rates.
        for m in &makers {
            if *m != maker {
                cal.set_funnel(Col::Maker, m, false);
            }
        }
        assert_eq!(
            cal.funnels[Col::Maker as usize],
            Some([maker.clone()].into())
        );
        let theirs: Vec<&Calibre> = calibres::all()
            .iter()
            .filter(|c| c.maker == maker)
            .collect();
        assert_eq!(cal.rows().len(), theirs.len());
        let rates = cal.funnel_values(Col::BeatRate);
        for r in &rates {
            assert!(
                theirs
                    .iter()
                    .any(|c| cal.cell(Entry::Table(c), Col::BeatRate) == *r),
                "{r} is not a rate of {maker}"
            );
        }
        // The Maker funnel still offers every maker: its own filter doesn't
        // narrow what it lists.
        assert_eq!(cal.funnel_values(Col::Maker), makers);
        // A second funnel narrows further: both must match.
        let rate = rates[0].clone();
        for r in &rates[1..] {
            cal.set_funnel(Col::BeatRate, r, false);
        }
        let both = theirs
            .iter()
            .filter(|c| cal.cell(Entry::Table(c), Col::BeatRate) == rate)
            .count();
        assert_eq!(cal.rows().len(), both);
        assert!(cal
            .rows()
            .iter()
            .all(|e| cal.cell(*e, Col::Maker) == maker && cal.cell(*e, Col::BeatRate) == rate));
        // Ticking every rate offered again is no filter on Beat Rate.
        for r in &rates {
            cal.set_funnel(Col::BeatRate, r, true);
        }
        assert!(cal.funnels[Col::BeatRate as usize].is_none());
        assert_eq!(cal.rows().len(), theirs.len());
        // Short List Only combines with the funnels too.
        cal.short_only = true;
        assert!(cal
            .rows()
            .iter()
            .all(|e| matches!(e, Entry::Table(c) if cal.in_short(c) && c.maker == maker)));
        // Unticking everything shows nothing; Clear Filters is every value.
        cal.short_only = false;
        cal.set_funnel(Col::Maker, &maker, false);
        assert!(cal.rows().is_empty());
        cal.funnels = Default::default();
        assert_eq!(cal.rows().len(), calibres::all().len());
    }

    /// The Calibre column leaves out the maker the Maker column already
    /// shows, so rows of one maker differ in what is left: their numbers.
    #[test]
    fn the_calibre_column_drops_the_makers_name() {
        let cal = Calibres::in_memory();
        let rows = cal.rows();
        let schild = rows
            .iter()
            .find(|e| e.name() == "A. Schild 970")
            .expect("A. Schild 970 listed");
        assert_eq!(short_name(*schild), "970");
        for e in &rows {
            let short = short_name(*e);
            assert!(!short.trim().is_empty(), "{}", e.name());
            assert!(e.name().ends_with(short), "{} -> {short}", e.name());
        }
    }

    /// The custom calibres stay at the top whatever column the table is
    /// sorted on, either way round; the rest follow the sort.
    #[test]
    fn custom_calibres_stay_at_the_top_whatever_the_sort() {
        let mut cal = Calibres::in_memory();
        let a = cal.new_custom("Zeta Bench");
        let b = cal.new_custom("Alpha Bench");
        for (col, ..) in COLS {
            for reverse in [false, true] {
                cal.sort = (col, reverse);
                let rows = cal.rows();
                let top: Vec<&str> = rows[..2].iter().map(|e| e.name()).collect();
                assert!(
                    top.contains(&a.as_str()) && top.contains(&b.as_str()),
                    "{col:?} {reverse}: {top:?}"
                );
                assert!(rows[2..].iter().all(|e| !e.is_custom()));
                // The listed ones in the column's order.
                for w in rows[2..].windows(2) {
                    assert_ne!(
                        cal.order(&w[0], &w[1]),
                        std::cmp::Ordering::Greater,
                        "{col:?} {reverse}: {} before {}",
                        w[0].name(),
                        w[1].name()
                    );
                }
            }
        }
        cal.sort = (Col::Calibre, false);
        let names: Vec<&str> = cal.rows()[..2].iter().map(|e| e.name()).collect();
        assert_eq!(names, [b.as_str(), a.as_str()]);
        cal.sort = (Col::Calibre, true);
        let names: Vec<&str> = cal.rows()[..2].iter().map(|e| e.name()).collect();
        assert_eq!(names, [a.as_str(), b.as_str()]);
        // A calibre with no beat rate comes after every rate, either way
        // round.
        for reverse in [false, true] {
            cal.sort = (Col::BeatRate, reverse);
            let rows = cal.rows();
            let first_blank = rows[2..].iter().position(|e| e.bph().is_none());
            if let Some(i) = first_blank {
                assert!(rows[2 + i..].iter().all(|e| e.bph().is_none()));
            }
            assert!(rows[2].bph().is_some());
        }
    }
}

#[cfg(test)]
mod menu_search_tests {
    use super::*;

    /// Shapes' words, drawn by one frame.
    fn texts(out: &egui::FullOutput) -> Vec<String> {
        let mut t = Vec::new();
        for s in &out.shapes {
            if let egui::Shape::Text(x) = &s.shape {
                t.push(x.galley.text().to_string());
            }
        }
        t
    }

    #[test]
    fn the_calibre_menu_searches_every_calibre() {
        let mut c = Calibres::in_memory();
        // One the short list doesn't hold, so only a search finds it.
        let rare = calibres::all().iter().find(|x| !x.common).unwrap();
        assert!(!c.in_short(rare));
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 800.0));
        let frame = |c: &mut Calibres, events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.set_width(300.0);
                        c.picker(ui, "");
                    });
                },
            )
        };
        frame(&mut c, vec![]);
        // The menu's button, at the top of the panel.
        let at = egui::pos2(60.0, 18.0);
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(&mut c, vec![egui::Event::PointerMoved(at), press(true)]);
        frame(&mut c, vec![press(false)]);
        let open = frame(&mut c, vec![]);
        assert!(
            texts(&open).iter().any(|t| t == "Search every calibre"),
            "{:?}",
            texts(&open)
        );
        frame(&mut c, vec![egui::Event::Text(rare.calibre.clone())]);
        let found = frame(&mut c, vec![]);
        let want = format!("{} · {}", rare.calibre, rare.maker);
        assert!(
            texts(&found).contains(&want),
            "{want} not in {:?}",
            texts(&found)
        );
    }

    #[test]
    fn manage_calibres_stays_in_view_under_a_long_short_list() {
        let mut c = Calibres::in_memory();
        for x in calibres::all() {
            c.set_short(&x.calibre, true);
        }
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 600.0));
        let frame = |c: &mut Calibres, events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.set_width(300.0);
                        c.picker(ui, "");
                    });
                },
            )
        };
        frame(&mut c, vec![]);
        let at = egui::pos2(60.0, 18.0);
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(&mut c, vec![egui::Event::PointerMoved(at), press(true)]);
        frame(&mut c, vec![press(false)]);
        for _ in 0..10 {
            frame(&mut c, vec![]);
        }
        let open = frame(&mut c, vec![]);
        // Drawn, and inside what its clip lets the eye see.
        let seen = open.shapes.iter().any(|s| match &s.shape {
            egui::Shape::Text(x) if x.galley.text() == "Manage Calibres…" => {
                let r = x.visual_bounding_rect();
                s.clip_rect.contains_rect(r) && screen.contains_rect(r)
            }
            _ => false,
        });
        assert!(seen, "Manage Calibres… scrolled out of view");
    }
}
