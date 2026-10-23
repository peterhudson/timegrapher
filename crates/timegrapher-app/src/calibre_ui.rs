//! The Calibre menu on the Beat Rate card, and Manage Calibres. The menu
//! offers the calibres picked lately, the short list and the watchmaker's
//! own. Manage Calibres lists every calibre by maker with a search over
//! names, other names and makers, a tickbox for the short list on each,
//! and the one selected in full: its beat rate and lift angle, each wheel's turn with where the figure comes
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

/// How many calibres picked lately the menu offers.
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
    /// Calibre names picked lately, the latest first.
    pub recent: Vec<String>,
    /// Calibres the watchmaker added to the short list, and the list's
    /// own ones they took off it.
    pub short_added: Vec<String>,
    pub short_removed: Vec<String>,
    /// Manage Calibres: asked for (the app opens Settings at Calibres),
    /// the calibre selected in it, its search and which calibres its list
    /// shows.
    pub manager_open: bool,
    selected: Option<String>,
    search: String,
    /// What is typed in the Calibre menu's own search box.
    menu_search: String,
    filter: Filter,
    /// Name and period typed for a wheel the slots don't list.
    extra: (String, String),
    /// Why the user's calibres couldn't be read or written, if so.
    pub problem: Option<String>,
}

/// Which calibres Manage Calibres lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Short,
    Yours,
}

/// A line in Manage Calibres' list: a maker's heading (with Tick All and
/// Untick All when true), or a calibre by name (with its maker in front
/// when true, in search results).
#[derive(Debug, Clone, PartialEq)]
enum Row {
    Maker(String, bool),
    Calibre(String, bool),
}

/// What Manage Calibres did that the app acts on.
#[derive(Debug, Default)]
pub struct Managed {
    /// A calibre to use for the watch.
    pub picked: Option<String>,
    /// What changed for the calibre selected, named `name` before it.
    pub change: Change,
    pub name: String,
}

const MANAGE_HINT: &str = "Every calibre in a list to search, with its tickbox for your short \
     list, and the one you select in full: its wheels, sources and notes, what you added, and \
     your own calibres to edit";

const MANAGE_HELP: &[&str] = &[
    "Every calibre the app knows, by maker, with the ones you added. Search by a calibre's \
     number, one of its other names, or its maker under any of the maker's names. All, Short \
     List and Yours choose which calibres the list shows.",
    "The tickbox beside a calibre puts it on your short list, which the Calibre menu offers; \
     Tick All and Untick All beside a maker do the same for all of that maker's calibres. The \
     list starts as the common and modern calibres; Reset the Short List goes back to them.",
    "The calibre selected shows in full: maker, beat rate, lift angle and escape wheel, each \
     wheel's time for one turn with where it comes from, the list's sources and notes. Type a \
     wheel's period you know into a blank and it is kept for this calibre, marked yours; Share \
     with the Project opens a GitHub issue with what you added, to send from your browser. Use \
     for This Watch sets it on the Beat Rate card.",
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
            filter: Filter::All,
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
    /// and the user's own calibres, with Manage Calibres… at its foot for
    /// the rest. What was picked ("" for Not Set), if anything.
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
        {
            egui::ComboBox::from_id_salt("calibre")
                .width(ui.available_width())
                .height(ui.ctx().content_rect().height() * 0.7)
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
                    if !q.is_empty() {
                        let found = catalog.search(q);
                        if found.is_empty() {
                            ui.label(
                                RichText::new("No calibre matches. Manage Calibres… can add it.")
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
                        ui.separator();
                        if ui
                            .selectable_label(false, "Manage Calibres…")
                            .on_hover_text(MANAGE_HINT)
                            .clicked()
                        {
                            manage = true;
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
                        heading(ui, "Your Calibres");
                        for c in &self.user.custom {
                            line(ui, Entry::Custom(c), c.name.clone());
                        }
                    }
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
                    Some(p) => format!("Yours; the list says {} s", (PERIOD.show)(p)),
                    None => match &s.basis {
                        Some(b) => format!("Yours: {b}"),
                        None => "Yours, typed in".into(),
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
                    ui.label(RichText::new("yours").small().color(pal.text_tertiary));
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
                    .desired_width(theme::LABEL_W - 16.0),
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

    /// Manage Calibres, in Settings: every calibre in a list to search,
    /// with each one's short-list tickbox, and the one selected in full
    /// beside it. Returns a calibre to use for the watch, and what changed
    /// for the one selected, with its name before the change.
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
        ui.horizontal(|ui| {
            theme::help(ui, "Calibres", MANAGE_HELP);
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
                    self.selected = Some(name);
                    self.search.clear();
                    self.filter = Filter::Yours;
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
                    .desired_width(320.0),
            )
            .on_hover_text(
                "Every word must match: a calibre's number, one of its other names, or its \
                 maker under any of the maker's names (\"dd 3235\", \"sw200\", \"seagull st19\")",
            );
            let short = calibres::all().iter().filter(|c| self.in_short(c)).count();
            let all = calibres::all().len() + self.user.custom.len();
            theme::segmented(
                ui,
                &mut self.filter,
                &[
                    (
                        Filter::All,
                        &format!("All ({all})"),
                        "Every calibre, by maker",
                    ),
                    (
                        Filter::Short,
                        &format!("Short List ({short})"),
                        "The calibres the Calibre menu offers",
                    ),
                    (
                        Filter::Yours,
                        &format!("Yours ({})", self.user.custom.len()),
                        "The calibres you added",
                    ),
                ],
            );
        });
        ui.add_space(6.0);
        let rows = self.rows();
        let list_w = (ui.available_width() * 0.38).clamp(260.0, 420.0);
        let mut toggles: Vec<(String, bool)> = Vec::new();
        let mut select: Option<String> = None;
        ui.horizontal_top(|ui| {
            let h = ui.available_height();
            let down = egui::Layout::top_down(egui::Align::Min);
            ui.allocate_ui_with_layout(Vec2::new(list_w, h), down, |ui| {
                ui.set_min_size(Vec2::new(list_w, h));
                theme::card().fill(pal.card).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    if rows.is_empty() {
                        ui.label(
                            RichText::new(match self.filter {
                                Filter::Yours if self.search.trim().is_empty() => {
                                    "None yet: New Custom Calibre… adds one."
                                }
                                Filter::Short if self.search.trim().is_empty() => {
                                    "Empty: tick calibres under All to put them here."
                                }
                                _ => "No calibre matches.",
                            })
                            .small()
                            .color(pal.text_tertiary),
                        );
                    }
                    let row_h = 22.0;
                    egui::ScrollArea::vertical()
                        .id_salt("manage list")
                        .auto_shrink([false, false])
                        .show_rows(ui, row_h, rows.len(), |ui, range| {
                            for row in &rows[range] {
                                let w = ui.available_width();
                                ui.allocate_ui_with_layout(Vec2::new(w, row_h), down, |ui| {
                                    if let Some(n) = self.list_row(ui, row, current, &mut toggles) {
                                        select = Some(n);
                                    }
                                });
                            }
                        });
                });
            });
            ui.add_space(8.0);
            let rest = Vec2::new(ui.available_width(), h);
            ui.allocate_ui_with_layout(rest, down, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("manage details")
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.details(ui, current, version, out));
            });
        });
        for (name, on) in toggles {
            self.set_short(&name, on);
        }
        if select.is_some() {
            self.selected = select;
        }
    }

    /// The list's lines under the filter and the search.
    fn rows(&self) -> Vec<Row> {
        let typed = self.search.trim();
        let catalog = Catalog::new(&self.user);
        let wanted = |e: &Entry<'_>| match (self.filter, e) {
            (Filter::All, _) => true,
            (Filter::Short, Entry::Table(c)) => self.in_short(c),
            (Filter::Short, Entry::Custom(_)) => false,
            (Filter::Yours, e) => e.is_custom(),
        };
        if !typed.is_empty() {
            return catalog
                .search(typed)
                .into_iter()
                .filter(wanted)
                .map(|e| Row::Calibre(e.name().to_string(), true))
                .collect();
        }
        let mut rows = Vec::new();
        if self.filter != Filter::Short {
            if !self.user.custom.is_empty() {
                rows.push(Row::Maker("Your Calibres".into(), false));
                rows.extend(
                    self.user
                        .custom
                        .iter()
                        .map(|c| Row::Calibre(c.name.clone(), false)),
                );
            }
            if self.filter == Filter::Yours {
                return rows;
            }
        }
        for (maker, _, list) in by_maker() {
            let shown: Vec<&&Calibre> = list
                .iter()
                .filter(|c| self.filter == Filter::All || self.in_short(c))
                .collect();
            if shown.is_empty() {
                continue;
            }
            rows.push(Row::Maker(maker.to_string(), self.filter == Filter::All));
            rows.extend(
                shown
                    .into_iter()
                    .map(|c| Row::Calibre(c.calibre.clone(), false)),
            );
        }
        rows
    }

    /// One line of the list; the calibre clicked, if any.
    fn list_row(
        &self,
        ui: &mut egui::Ui,
        row: &Row,
        current: &str,
        toggles: &mut Vec<(String, bool)>,
    ) -> Option<String> {
        let mut clicked = None;
        let pal = theme::pal(ui);
        match row {
            Row::Maker(maker, ticks) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(maker).strong().color(pal.text_secondary));
                    if !*ticks {
                        return;
                    }
                    let list = by_maker()
                        .iter()
                        .find(|m| m.0 == maker)
                        .map_or(&[][..], |m| &m.2[..]);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("Untick All")
                            .on_hover_text(format!(
                                "Take every {maker} calibre off your short list"
                            ))
                            .clicked()
                        {
                            toggles.extend(list.iter().map(|c| (c.calibre.clone(), false)));
                        }
                        if ui
                            .small_button("Tick All")
                            .on_hover_text(format!("Put every {maker} calibre on your short list"))
                            .clicked()
                        {
                            toggles.extend(list.iter().map(|c| (c.calibre.clone(), true)));
                        }
                    });
                });
            }
            Row::Calibre(name, with_maker) => {
                let e = self.find(name)?;
                ui.horizontal(|ui| {
                    match e {
                        Entry::Table(c) => {
                            let mut on = self.in_short(c);
                            if ui
                                .checkbox(&mut on, "")
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
                        // Lined up with the tickboxes.
                        Entry::Custom(_) => ui.add_space(ui.spacing().icon_width + 4.0),
                    }
                    let mut text = e.name().to_string();
                    if *with_maker {
                        if let Some(m) = e.maker() {
                            text = with_maker_name(m, &text);
                        }
                    }
                    if e.name() == current {
                        text += "  (this watch)";
                    }
                    let selected = self.selected.as_deref() == Some(e.name());
                    let r = ui.selectable_label(selected, text).on_hover_text(hint(e));
                    if r.clicked() {
                        clicked = Some(e.name().to_string());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let facts = [
                            e.bph().map(|b| format!("{b}")),
                            e.lift_angle_deg().map(|l| format!("{}°", fields::plain(l))),
                        ]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" · ");
                        ui.label(RichText::new(facts).small().color(pal.text_tertiary));
                    });
                });
            }
        }
        clicked
    }

    /// The calibre selected in Manage Calibres, in full.
    fn details(&mut self, ui: &mut egui::Ui, current: &str, version: &str, out: &mut Managed) {
        let pal = theme::pal(ui);
        let Some(name) = self.selected.clone().filter(|n| self.find(n).is_some()) else {
            ui.label(
                RichText::new(
                    "Select a calibre in the list to see its wheels, sources and notes, add \
                     what you know, or use it for this watch.",
                )
                .color(pal.text_secondary),
            );
            return;
        };
        let Some(e) = self.find(&name) else { return };
        let custom = e.is_custom();
        ui.horizontal(|ui| {
            ui.label(RichText::new(label(e)).strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let here = name == current;
                if ui
                    .add_enabled(!here, theme::primary(ui, "Use for This Watch"))
                    .on_hover_text(
                        "Set this calibre on the Beat Rate card, with its lift angle and wheels",
                    )
                    .on_disabled_hover_text("This watch's calibre already")
                    .clicked()
                {
                    out.picked = Some(name.clone());
                }
            });
        });
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
            "Calibre Name",
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
            "Calibre Beat Rate",
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
            "Calibre Lift Angle",
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

/// A calibre's line in the search results: its name, and "yours" for one
/// the user added.
fn label(e: Entry<'_>) -> String {
    match e {
        Entry::Table(c) => c.calibre.clone(),
        Entry::Custom(c) => format!("{} (yours)", c.name),
    }
}

/// What the list says about a calibre, for its line in the menu.
fn hint(e: Entry<'_>) -> String {
    let mut t = summary(e);
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
                .desired_width(70.0),
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

    /// Manage Calibres' list: All has every calibre under its maker, the
    /// short list only the ticked ones, Yours only the user's, and a search
    /// keeps to the filter.
    #[test]
    fn the_manager_lists_by_filter_and_search() {
        let mut cal = Calibres::in_memory();
        let calibre = |r: &Row| match r {
            Row::Calibre(n, _) => Some(n.clone()),
            Row::Maker(..) => None,
        };
        let all: Vec<String> = cal.rows().iter().filter_map(calibre).collect();
        assert_eq!(all.len(), calibres::all().len());
        assert!(matches!(cal.rows()[0], Row::Maker(..)));
        cal.filter = Filter::Short;
        let short: Vec<String> = cal.rows().iter().filter_map(calibre).collect();
        let common = calibres::all().iter().filter(|c| cal.in_short(c)).count();
        assert_eq!(short.len(), common);
        assert!(short.len() < all.len());
        let mine = cal.new_custom("Bench Special");
        cal.filter = Filter::Yours;
        let yours: Vec<String> = cal.rows().iter().filter_map(calibre).collect();
        assert_eq!(yours, vec![mine.clone()]);
        cal.search = "bench".into();
        assert_eq!(cal.rows(), vec![Row::Calibre(mine, true)]);
        cal.search = "3235".into();
        assert!(cal.rows().is_empty(), "a listed calibre isn't one of yours");
        cal.filter = Filter::All;
        assert!(!cal.rows().is_empty());
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
}
