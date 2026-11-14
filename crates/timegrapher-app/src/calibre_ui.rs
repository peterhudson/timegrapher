//! The Calibre menu on the Calibre card, and Manage Calibres. The menu
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
use crate::marks::title_case;
use crate::settings;
use crate::theme;
use eframe::egui::{self, RichText, Vec2};
use std::path::PathBuf;
use std::sync::OnceLock;
use timegrapher_core::calibre_store::{
    Catalog, CustomCalibre, CustomWheel, Entry, Origin, Slot, UserCalibres,
};
use timegrapher_core::calibres::{self, Calibre};
use timegrapher_core::input_domain::LIFT_RANGE_DEG;
use timegrapher_core::periodicity::Wheel;
use timegrapher_core::train;

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
    /// and the calibre selected in it.
    pub manager_open: bool,
    selected: Option<String>,
    /// What is typed in the Calibre menu's own search box.
    menu_search: String,
    /// What is typed in the search over Manage Calibres' table.
    search: String,
    /// The column the table is sorted on, and whether reversed.
    sort: (Col, bool),
    /// Each column's funnel: the values it shows, or `None` for all.
    funnels: [Option<std::collections::BTreeSet<String>>; 5],
    /// What is typed in the funnel open.
    funnel_search: String,
    /// What the funnel open ticks, kept to the column's filter on OK.
    funnel_draft: Option<std::collections::BTreeSet<String>>,
    /// The values the funnel open lists, for its column and what is typed
    /// in its search, so a frame or a Select All doesn't gather them
    /// from every calibre again.
    funnel_listed: std::cell::RefCell<Option<(Col, String, Vec<String>)>>,
    /// The values the funnel open offers, kept the same way.
    funnel_offered: std::cell::RefCell<Option<(Col, Vec<String>)>>,
    /// The row open for a wheel the slots don't list: its name and period
    /// as typed so far.
    adding: Option<(String, String)>,
    /// Why the user's calibres couldn't be read or written, if so.
    pub problem: Option<String>,
    /// The beat rate the Calibre menu knew when it asked for Manage
    /// Calibres, and where the rate came from, for the table to start at.
    manager_rate: Option<(u32, &'static str)>,
    /// The Beat Rate funnel the table started at, and where the rate came
    /// from, said above the table while that funnel is as it was set.
    rate_preset: Option<(String, &'static str)>,
    /// The table's rows by name, kept with what they were sorted from so a
    /// frame that changes nothing doesn't sort every calibre again.
    table_rows: std::cell::RefCell<Option<(TableKey, RowNames)>>,
}

/// The table's rows by name: whether custom, and the name.
type RowNames = Vec<(bool, String)>;

/// What the table's rows depend on.
#[derive(Clone, PartialEq)]
struct TableKey {
    funnels: [Option<std::collections::BTreeSet<String>>; 5],
    search: String,
    sort: (Col, bool),
    short_added: Vec<String>,
    short_removed: Vec<String>,
    user: UserCalibres,
}

/// A column of Manage Calibres' table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Col {
    Maker,
    Calibre,
    BeatRate,
    LiftAngle,
    Short,
}

/// The table's columns, each with its heading and what it shows.
const COLS: [(Col, &str, &str); 5] = [
    (
        Col::Maker,
        "Maker",
        "Who makes the calibre; the ones you added are under Custom",
    ),
    (Col::Calibre, "Calibre", "The calibre's name"),
    (Col::BeatRate, "Beat Rate (bph)", "Beats per hour"),
    (
        Col::LiftAngle,
        "Lift Angle (°)",
        "The lift angle, in degrees",
    ),
    // Headed by a star alone, to keep the column narrow.
    (
        Col::Short,
        "Short List",
        "Short List: ticked calibres are on your short list, which the Calibre menu offers",
    ),
];

/// The widths of the table's columns in `w`: the facts' columns as wide
/// as their headings, the Short List's as its star and funnel, maker and
/// name sharing the rest.
fn col_widths(w: f32) -> [f32; 5] {
    let facts = [118.0, 108.0, 44.0];
    let rest = (w - facts.iter().sum::<f32>()).max(160.0);
    [rest * 0.5, rest * 0.5, facts[0], facts[1], facts[2]]
}

/// The least width the selected calibre's details need beside the table, in
/// their card: room for a wheel's row with Add and Cancel.
const DETAILS_W: f32 = 360.0;
/// The least width the table takes beside the details.
const TABLE_MIN_W: f32 = 440.0;
/// A line of the table.
const ROW_H: f32 = 28.0;
/// A number's right edge, this far in from its column's, under the end
/// of its heading.
const NUMBER_PAD: f32 = 22.0;
/// The maker the user's own calibres are shown under.
const CUSTOM: &str = "Custom";
/// What a column's filter lists where the list gives nothing; its cell
/// shows a dim "—".
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
     always at the top.",
    "## The Table",
    "- The search over the table finds a calibre by its maker and name together, with the \
     count beside it.",
    "- Click a column's heading to sort on it, and again to reverse.",
    "- The ▾ beside each heading sorts on that column, clears its filter, and picks which of \
     its values show, ticked and applied with OK.",
    "- The filters combine. A column filtered shows a funnel for its ▾, and its Clear Filter \
     shows that column's values again.",
    "- A dim \"—\" is a figure the list doesn't give.",
    "## Filters",
    "- A column filter's search lists the values holding what you type.",
    "- In Maker and Calibre it also finds a calibre by one of its other names, or its maker \
     under any of the maker's names.",
    "- With a search typed, (Select All Search Results) picks just the values it lists.",
    "- The star column's funnel shows just your short list (Starred).",
    "## Short List",
    "- Click the star in a calibre's row to put it on your short list, which the Calibre menu \
     offers; a filled star is on it.",
    "- The list starts as the common and modern calibres. Reset the Short List, in the star \
     column's funnel, goes back to them.",
    "## The Calibre Selected",
    "- It shows in full: maker, beat rate, lift angle and escape wheel, each wheel's time for \
     one turn with where it comes from, the list's sources and notes.",
    "- Type a wheel's period you know into a blank and it is kept for this calibre, marked \
     Custom.",
    "- Add another wheel by name and period with Add a Wheel… at the foot of the wheels, then \
     Add.",
    "- Share with the Project opens a GitHub issue with what you added, to send from your \
     browser.",
    "## Custom Calibres",
    "- New Custom Calibre…, at the right of the search, adds one the list doesn't have, with \
     no wheels assumed.",
    "- Its name, maker, beat rate and lift angle are edited here, and its balance's period \
     follows the beat rate typed. Delete Calibre is at the foot.",
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
            menu_search: String::new(),
            search: String::new(),
            sort: (Col::Maker, false),
            funnels: Default::default(),
            funnel_search: String::new(),
            funnel_draft: None,
            funnel_listed: Default::default(),
            funnel_offered: Default::default(),
            adding: None,
            problem: None,
            manager_rate: None,
            rate_preset: None,
            table_rows: Default::default(),
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

    /// The meshes known for `name`, marked beside its wheels
    /// ([`Catalog::meshes`]); none when no calibre is set.
    pub fn meshes(&self, name: &str) -> Vec<Wheel> {
        self.find(name)
            .map(|e| Catalog::new(&self.user).meshes(e))
            .unwrap_or_default()
    }

    pub fn lift(&self, name: &str) -> Option<f64> {
        self.find(name)?.lift_angle_deg()
    }

    pub fn bph(&self, name: &str) -> Option<u32> {
        self.find(name)?.bph()
    }

    /// The lift angle a take of `w` starts with: its calibre's where the
    /// table gives one, else the watch's own.
    pub fn watch_lift(&self, w: &timegrapher_core::library::Watch) -> Option<f64> {
        self.lift(w.calibre.as_deref().unwrap_or_default())
            .or(w.lift.filter(|l| LIFT_RANGE_DEG.contains(l)))
    }

    /// The beat rate a take of `w` starts with, as [`Self::watch_lift`].
    pub fn watch_bph(&self, w: &timegrapher_core::library::Watch) -> Option<u32> {
        self.bph(w.calibre.as_deref().unwrap_or_default()).or(w.bph)
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
    /// Calibre N", with nothing else known: its name.
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
        // Nothing assumed: a custom calibre is often obscure or vintage, so
        // its wheels start not known.
        self.user.put_custom(custom(&name, []));
        self.save();
        name
    }

    /// The Calibre menu, short: Not Set, the recent picks, the short list
    /// and the user's own calibres, with Manage Calibres… pinned at its
    /// foot, below the scrolling list, for the rest. `rate` is the beat rate
    /// known, with where it came from: Manage Calibres… then opens on the
    /// calibres at that rate. What was picked ("" for Not Set), if anything.
    pub fn picker(
        &mut self,
        ui: &mut egui::Ui,
        current: &str,
        rate: Option<(u32, &'static str)>,
    ) -> Option<String> {
        self.picker_with(ui, current, rate, "Not Set", None)
    }

    /// [`Self::picker`], reading `unset` while no calibre is picked, its
    /// menu starting with what `before` draws.
    pub fn picker_with(
        &mut self,
        ui: &mut egui::Ui,
        current: &str,
        rate: Option<(u32, &'static str)>,
        unset: &str,
        mut before: Option<&mut dyn FnMut(&mut egui::Ui)>,
    ) -> Option<String> {
        let shown = match self.find(current) {
            Some(e) => e.name().to_string(),
            None => unset.to_string(),
        };
        let mut picked = None;
        let mut manage = false;
        let mut typed = std::mem::take(&mut self.menu_search);
        // egui makes a menu no taller than its default area; the list
        // scrolls inside that, leaving room for the search field above and
        // Manage Calibres… below, so the menu itself never scrolls.
        let menu_h =
            (ui.ctx().content_rect().height() * 0.7).min(ui.spacing().default_area_size.y - 16.0);
        let list_h = (menu_h - 110.0).max(120.0);
        egui::ComboBox::from_id_salt("calibre")
            .width(ui.available_width())
            .height(menu_h)
            .truncate()
            .selected_text(shown)
            // A click in its search field never closes it.
            .close_behavior(theme::SEARCH_MENU_CLOSE)
            .show_ui(ui, |ui| {
                // The search field first, where the caret is: what the
                // menu adds above the list comes under it.
                let list = Picked {
                    keys: search_field(ui, &mut typed).into(),
                    name: Default::default(),
                };
                if let Some(before) = before.as_mut() {
                    before(ui);
                }
                let q = typed.trim();
                // The list scrolls between the search field and Manage
                // Calibres…, so both stay in view however long the
                // Short List grows. It keeps one height, searching or not:
                // egui sizes a menu from its last frame, so a list shrunk
                // to a short search's hits would leave the menu that small.
                egui::ScrollArea::vertical()
                    .id_salt("calibre list")
                    .max_height(list_h)
                    .min_scrolled_height(list_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let catalog = Catalog::new(&self.user);
                        if q.is_empty() {
                            self.short_list_items(ui, &catalog, current, &list);
                        } else {
                            search_hits(ui, &catalog, q, current, &list);
                        }
                    });
                ui.separator();
                if ui
                    .selectable_label(false, "Manage Calibres…")
                    .on_hover_text(MANAGE_HINT)
                    .clicked()
                {
                    manage = true;
                    ui.close();
                }
                let Picked { name, keys } = list;
                keys.into_inner().finish(ui);
                picked = name.into_inner();
            });
        // A pick starts the next search afresh.
        self.menu_search = if picked.is_some() {
            String::new()
        } else {
            typed
        };
        if manage {
            self.open_manager(current);
            self.manager_rate = rate;
        }
        picked
    }

    /// The menu with nothing searched: Not Set, then Recent, the Short List
    /// and Custom.
    fn short_list_items(
        &self,
        ui: &mut egui::Ui,
        catalog: &Catalog<'_>,
        current: &str,
        picked: &Picked,
    ) {
        let pal = theme::pal(ui);
        picked.item(
            ui,
            current.is_empty(),
            "Not Set".into(),
            "Name cycles after the wheels most calibres at this beat rate share, with the \
             default lift angle",
            "",
        );
        let recent: Vec<Entry<'_>> = self.recent.iter().filter_map(|r| catalog.find(r)).collect();
        if !recent.is_empty() {
            heading(ui, "Recent");
            for e in recent {
                menu_line(ui, e, label(e), current, picked);
            }
        }
        heading(ui, "Short List");
        let mut any = false;
        for (maker, _, list) in by_maker() {
            let short: Vec<&&Calibre> = list.iter().filter(|c| self.in_short(c)).collect();
            if short.is_empty() {
                continue;
            }
            any = true;
            ui.label(RichText::new(*maker).small().color(pal.text_tertiary));
            for c in short {
                menu_line(ui, Entry::Table(c), c.calibre.clone(), current, picked);
            }
        }
        if !any {
            ui.label(
                RichText::new("Empty: tick calibres in Manage Calibres to put them here.")
                    .small()
                    .color(pal.text_tertiary),
            );
        }
        if !self.user.custom.is_empty() {
            heading(ui, "Custom");
            for c in &self.user.custom {
                menu_line(ui, Entry::Custom(c), c.name.clone(), current, picked);
            }
        }
    }

    /// Open Manage Calibres on `current`, or on nothing.
    pub fn open_manager(&mut self, current: &str) {
        self.manager_open = true;
        self.manager_rate = None;
        self.selected = self.find(current).map(|e| e.name().to_string());
    }

    /// Settings opening: the table loses the Beat Rate funnel an earlier
    /// Manage Calibres… set, if still as it was set, so the gear opens it
    /// unfiltered.
    pub fn drop_rate_preset(&mut self) {
        if let Some((rate, _)) = self.rate_preset.take() {
            if self.rate_funnel_is(&rate) {
                self.funnels[Col::BeatRate as usize] = None;
            }
        }
    }

    /// Manage Calibres… from the Calibre menu with a beat rate known: the
    /// table starts at the calibres at that rate.
    pub fn apply_rate_preset(&mut self) {
        if let Some((bph, from)) = self.manager_rate.take() {
            let rate = bph.to_string();
            self.funnels[Col::BeatRate as usize] = Some([rate.clone()].into());
            self.rate_preset = Some((rate, from));
        }
    }

    /// Whether the Beat Rate funnel shows only `rate`.
    fn rate_funnel_is(&self, rate: &str) -> bool {
        self.funnels[Col::BeatRate as usize]
            .as_ref()
            .is_some_and(|keep| keep.len() == 1 && keep.contains(rate))
    }

    /// Each wheel's turn with where it comes from, blanks to fill in,
    /// another wheel by name, and the table's sources and notes. `place`
    /// keeps the boxes apart from the same calibre's shown elsewhere. True
    /// when a wheel's period changed.
    fn wheels_and_sources(&mut self, ui: &mut egui::Ui, name: &str, place: &str) -> bool {
        let pal = theme::pal(ui);
        let Some(e) = self.find(name) else {
            return false;
        };
        let catalog = Catalog::new(&self.user);
        let slots = catalog.slots(e);
        let train = catalog.train(e);
        let counts = catalog.train_input(e);
        let table = match e {
            Entry::Table(c) => Some(c),
            Entry::Custom(_) => None,
        };
        let mut edits: Vec<(String, Option<f64>)> = Vec::new();
        let adding = &mut self.adding;
        theme::group(ui, "Wheels", None, |g| {
            for s in &slots {
                let worked = train.arbors.iter().find(|a| a.name == s.name);
                let tip = slot_tip(s, worked);
                let id = format!("calibre-{place}-{name}-{}", s.name);
                let teeth = worked.and_then(|w| w.caption());
                let caption = match (&teeth, s.origin) {
                    (Some(t), _) => theme::Caption::Text(t),
                    (None, Origin::User) => theme::Caption::Text("Custom"),
                    _ => theme::Caption::None,
                };
                g.row(&title_case(&s.name), caption, &tip, |ui| {
                    slot_period(ui, s, &id, &tip, &mut edits);
                });
            }
            // Another wheel by name, such as an intermediate wheel: a row
            // of its own, kept once it has both.
            g.whole(|ui| add_wheel_row(ui, adding, place, &mut edits));
        });
        ui.add_space(-8.0);
        ui.add(
            egui::Label::new(
                RichText::new(
                    "Each wheel's time for one full turn. Type one you know; it is kept for \
                     this calibre.",
                )
                .size(11.5)
                .color(pal.text_secondary),
            )
            .wrap(),
        );
        ui.add_space(12.0);
        let counted = teeth_and_leaves(ui, &counts, place, name);
        if let Some(c) = table {
            sources_and_notes(ui, c);
        }
        if edits.is_empty() && counted.is_empty() {
            return false;
        }
        for (arbor, teeth, leaves) in counted {
            let had = self
                .user
                .counts
                .iter()
                .find(|c| c.calibre == name && c.arbor == arbor);
            let (t0, l0) = had.map_or((None, None), |c| (c.teeth, c.leaves));
            let teeth = teeth.unwrap_or(t0);
            let leaves = leaves.unwrap_or(l0);
            self.user.set_count(name, arbor, teeth, leaves);
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
        // A load failure can leave no selected calibre; show the warning before
        // the details pane's early return, not only beside a selected entry.
        if let Some(problem) = &self.problem {
            crate::theme::alert_label(ui, crate::theme::Alert::Bad, problem, false);
        }
        self.rate_preset_line(ui);
        ui.add_space(4.0);
        // One search over the table with the count beside it, and the
        // page's one action at the right. The column filters stay for
        // picking values.
        let rows = self.search_bar(ui);
        ui.add_space(6.0);
        // The details beside it keep room for a wheel's row, with Clear.
        let room = ui.available_width();
        // Too narrow for both side by side, the table fills the room and
        // the calibre selected takes its place, with the way back.
        let narrow = room < TABLE_MIN_W + DETAILS_W;
        // A calibre the search or a filter hides isn't shown beside the
        // rows that are.
        let shown = self
            .selected
            .as_ref()
            .is_some_and(|n| rows.iter().any(|(_, r)| r == n));
        let chosen = shown
            && self
                .selected
                .as_ref()
                .is_some_and(|n| self.find(n).is_some());
        let table_w = if narrow {
            room
        } else {
            (room * 0.62).min(room - DETAILS_W).max(TABLE_MIN_W)
        };
        if narrow && chosen {
            if ui
                .button("‹ All Calibres")
                .on_hover_text("Back to the table of calibres")
                .clicked()
            {
                self.selected = None;
                return;
            }
            ui.add_space(4.0);
            let h = ui.available_height();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(room, h), egui::Sense::hover());
            self.details_card(ui, rect, (current, version, shown), out);
            return;
        }
        let (toggles, select) = ui
            .horizontal_top(|ui| {
                let h = ui.available_height();
                let down = egui::Layout::top_down(egui::Align::Min);
                let picks = ui
                    .allocate_ui_with_layout(Vec2::new(table_w, h), down, |ui| {
                        ui.set_min_size(Vec2::new(table_w, h));
                        self.table_card(ui, &rows, current)
                    })
                    .inner;
                if narrow {
                    return picks;
                }
                ui.add_space(8.0);
                // The details sit in a card level with the table's, and never
                // widen the window: what doesn't fit is cut off at its edge.
                let (rect, _) = ui
                    .allocate_exact_size(Vec2::new(ui.available_width(), h), egui::Sense::hover());
                self.details_card(ui, rect, (current, version, shown), out);
                picks
            })
            .inner;
        for (name, on) in toggles {
            self.set_short(&name, on);
        }
        if select.is_some() {
            self.selected = select;
        }
    }

    /// The rate the table started at, while its funnel is as it was set,
    /// with the way to every rate.
    fn rate_preset_line(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let Some((rate, from)) = self.rate_preset.clone() else {
            return;
        };
        if !self.rate_funnel_is(&rate) {
            return;
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Showing {rate} bph, {from}")).color(pal.text_secondary),
            );
            if ui
                .link("Show All")
                .on_hover_text("Show the calibres at every beat rate")
                .clicked()
            {
                self.funnels[Col::BeatRate as usize] = None;
                self.rate_preset = None;
            }
        });
    }

    /// One search over the table with the count beside it, and the
    /// page's one action at the right; the rows it leaves, by name, so the
    /// table can change what it holds as it draws.
    fn search_bar(&mut self, ui: &mut egui::Ui) -> RowNames {
        let pal = theme::pal(ui);
        let mut rows = Vec::new();
        ui.horizontal(|ui| {
            let (icon, _) = ui.allocate_exact_size(Vec2::splat(16.0), egui::Sense::hover());
            crate::icons::draw(ui.painter(), icon, theme::Icon::Search, pal.text_tertiary);
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .id_salt("calibre table search")
                    .hint_text("Search Makers and Calibres")
                    .desired_width(240.0),
            )
            .on_hover_text(
                "Show the calibres whose maker or name holds what you type, or one of their \
                 other names",
            );
            rows = self.row_names();
            ui.label(
                RichText::new(shown_of(rows.len(), self.every().count())).color(pal.text_secondary),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.new_custom_button(ui);
            });
        });
        rows
    }

    /// The selected calibre in full, in a card filling `rect` and cut off
    /// at its edge; `shown` is whether the table shows it.
    fn details_card(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        (current, version, shown): (&str, &str, bool),
        out: &mut Managed,
    ) {
        let pal = theme::pal(ui);
        let down = egui::Layout::top_down(egui::Align::Min);
        let mut d = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(down));
        d.set_clip_rect(rect.intersect(ui.clip_rect()));
        theme::card().fill(pal.card).show(&mut d, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::vertical()
                .id_salt("manage details")
                .auto_shrink([false, false])
                .show(ui, |ui| self.details(ui, current, version, shown, out));
        });
    }

    /// The table in its card: its head, then `rows`, drawn only where they
    /// show. Returns the short-list ticks changed and the row clicked.
    fn table_card(
        &mut self,
        ui: &mut egui::Ui,
        rows: &RowNames,
        current: &str,
    ) -> (Vec<(String, bool)>, Option<String>) {
        let pal = theme::pal(ui);
        let mut toggles: Vec<(String, bool)> = Vec::new();
        let mut select: Option<String> = None;
        theme::card().fill(pal.card).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            let widths = col_widths(ui.available_width());
            self.table_head(ui, &widths);
            ui.separator();
            if rows.is_empty() {
                ui.label(
                    RichText::new(
                        "No calibre matches the filters. Search with the funnel beside Calibre.",
                    )
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
        (toggles, select)
    }

    /// New Custom Calibre…, beside the Calibres heading: one shown and
    /// selected whatever the filters hid, to name and fill in.
    pub fn new_custom_button(&mut self, ui: &mut egui::Ui) {
        if ui
            .button("New Custom Calibre…")
            .on_hover_text(
                "Add a calibre the list doesn't have, with the wheels' periods you type; it is \
                 kept for next time",
            )
            .clicked()
        {
            let name = self.new_custom("");
            // The new one shows, whatever was searched for or filtered out.
            if !self.rows().iter().any(|e| e.name() == name) {
                self.funnels = Default::default();
                self.search.clear();
            }
            self.selected = Some(name);
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
                .map_or(NOT_GIVEN.into(), |l| fields::plain(l).to_string()),
            Col::Short => match e {
                Entry::Table(c) if self.in_short(c) => "Starred".into(),
                _ => "Not Starred".into(),
            },
        }
    }

    /// The calibres every column's filter but `skip`'s lets through, in no
    /// order.
    fn passing(&self, skip: Option<Col>) -> Vec<Entry<'_>> {
        let found = self.searched();
        self.every()
            .filter(|e| found.as_ref().is_none_or(|f| f.contains(e.name())))
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

    /// The calibres the search over the table finds, by name: its words
    /// in the maker or the calibre, or one of the calibre's other names,
    /// or its maker under another of the maker's names, as the Calibre
    /// menu's search finds them. `None` while nothing is typed.
    fn searched(&self) -> Option<std::collections::HashSet<String>> {
        let typed = self.search.trim();
        if typed.is_empty() {
            return None;
        }
        let low = typed.to_lowercase();
        let mut found: std::collections::HashSet<String> = Catalog::new(&self.user)
            .search(typed)
            .into_iter()
            .map(|e| e.name().to_string())
            .collect();
        for e in self.every() {
            let maker = self.cell(e, Col::Maker).to_lowercase();
            let name = e.name().to_lowercase();
            if low
                .split_whitespace()
                .all(|w| maker.contains(w) || name.contains(w))
            {
                found.insert(e.name().to_string());
            }
        }
        Some(found)
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

    /// The table's rows by name (custom or not, and the name), sorted
    /// again only when a filter, the sort or the calibres
    /// themselves changed since the last frame.
    fn row_names(&self) -> RowNames {
        let key = TableKey {
            funnels: self.funnels.clone(),
            search: self.search.trim().to_string(),
            sort: self.sort,
            short_added: self.short_added.clone(),
            short_removed: self.short_removed.clone(),
            user: self.user.clone(),
        };
        let mut cache = self.table_rows.borrow_mut();
        if let Some((k, rows)) = cache.as_ref() {
            if *k == key {
                return rows.clone();
            }
        }
        let rows: Vec<(bool, String)> = self
            .rows()
            .iter()
            .map(|e| (e.is_custom(), e.name().to_string()))
            .collect();
        *cache = Some((key, rows.clone()));
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

    /// The values the funnel open offers: [`Self::funnel_values`], kept
    /// while the menu is open, since the other columns' filters can't
    /// change then.
    fn funnel_offered(&self, col: Col) -> Vec<String> {
        if let Some((c, v)) = self.funnel_offered.borrow().as_ref() {
            if *c == col {
                return v.clone();
            }
        }
        let v = self.funnel_values(col);
        *self.funnel_offered.borrow_mut() = Some((col, v.clone()));
        v
    }

    /// The values the funnel open lists: those offered that hold what is
    /// typed in its search. For Maker and Calibre the search also finds a
    /// calibre by one of its other names, or its maker under any of the
    /// maker's names ("dd 3235", "sw200", "seagull st19"), as the Calibre
    /// menu's does. Kept until the column or the search changes.
    fn funnel_listed(&self, col: Col) -> Vec<String> {
        let typed = self.funnel_search.trim().to_string();
        if let Some((c, t, v)) = self.funnel_listed.borrow().as_ref() {
            if *c == col && *t == typed {
                return v.clone();
            }
        }
        let offered = self.funnel_offered(col);
        let low = typed.to_lowercase();
        let found: std::collections::HashSet<String> =
            if !typed.is_empty() && matches!(col, Col::Maker | Col::Calibre) {
                Catalog::new(&self.user)
                    .search(&typed)
                    .into_iter()
                    .map(|e| self.cell(e, col))
                    .collect()
            } else {
                Default::default()
            };
        let listed: Vec<String> = offered
            .into_iter()
            .filter(|v| low.is_empty() || v.to_lowercase().contains(&low) || found.contains(v))
            .collect();
        *self.funnel_listed.borrow_mut() = Some((col, typed, listed.clone()));
        listed
    }

    /// Show `value` in `col` or not, the other values offered staying as
    /// they were.
    #[cfg(test)]
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
    fn table_head(&mut self, ui: &mut egui::Ui, widths: &[f32; 5]) {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(widths.iter().sum(), ROW_H), egui::Sense::hover());
        let mut x = rect.left();
        for (i, w) in widths.iter().enumerate() {
            let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(*w, ROW_H));
            x += w;
            self.head_cell(ui, i, cell);
        }
    }

    /// Column `i`'s heading in `cell`: its title to sort on, the order
    /// it is sorted in, and its funnel.
    fn head_cell(&mut self, ui: &mut egui::Ui, i: usize, cell: egui::Rect) {
        let pal = theme::pal(ui);
        let (col, title, hint) = COLS[i];
        let sorted = self.sort.0 == col;
        let sort_w = if sorted { 12.0 } else { 0.0 };
        let (words, funnel, title_x) = head_layout(ui, col, title, cell, sort_w);
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
        let text_right = if col == Col::Short {
            // A star for the short list, as narrow as a heading gets.
            let c = egui::pos2(cell.left() + 11.0, cell.center().y);
            star(ui.painter(), c, 6.0, ink);
            c.x + 6.0
        } else {
            let galley = ui.painter().layout_no_wrap(
                title.to_string(),
                egui::FontId::proportional(11.5),
                ink,
            );
            let at = egui::pos2(title_x, cell.center().y - galley.size().y / 2.0);
            let right = at.x + galley.size().x;
            ui.painter().with_clip_rect(words).galley(at, galley, ink);
            right
        };
        if sorted {
            // Up for the order as it reads, down reversed.
            let c = egui::pos2((text_right + 6.0).min(words.right() - 3.0), cell.center().y);
            sort_mark(ui.painter(), c, self.sort.1, ink);
        }
        self.funnel_button(ui, i, funnel);
    }

    /// Column `i`'s funnel at `funnel`: a ▾, or the funnel itself in the
    /// accent while the column is filtered; it opens the column's menu.
    fn funnel_button(&mut self, ui: &mut egui::Ui, i: usize, funnel: egui::Rect) {
        let pal = theme::pal(ui);
        let (col, title, _) = COLS[i];
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
        // A ▾ opens the column's filter; while the filter is on, a
        // funnel in the accent colour stands in its place.
        if on {
            crate::icons::draw(
                ui.painter(),
                funnel.shrink(1.5),
                theme::Icon::Funnel,
                pal.accent,
            );
        } else {
            let c = funnel.center();
            let ink = if fr.hovered() {
                pal.text
            } else {
                pal.text_tertiary
            };
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    c + Vec2::new(-4.0, -2.0),
                    c + Vec2::new(4.0, -2.0),
                    c + Vec2::new(0.0, 2.5),
                ],
                ink,
                egui::Stroke::NONE,
            ));
        }
        if fr.clicked() {
            // What the menu ticks starts as the filter in force, and
            // changes it only on OK, as a spreadsheet's does.
            self.funnel_search.clear();
            self.funnel_draft = self.funnels[col as usize].clone();
            self.funnel_listed.take();
            self.funnel_offered.take();
        }
        egui::Popup::menu(&fr)
            .close_behavior(theme::SEARCH_MENU_CLOSE)
            .width(230.0)
            .show(|ui| self.funnel_menu(ui, col, title));
    }

    /// A column's funnel, laid out as a spreadsheet's column filter: sort
    /// either way, clear the column's filter, then its values, each with a
    /// tickbox, under a search over them and Select All, kept only on OK.
    fn funnel_menu(&mut self, ui: &mut egui::Ui, col: Col, title: &str) {
        self.funnel_sorts(ui, col, title);
        ui.separator();
        let enter = self.funnel_ticks(ui, col, title);
        ui.add_space(4.0);
        self.funnel_ok(ui, col, title, enter);
    }

    /// The funnel menu's top: sort either way on `col`, clear its filter,
    /// and on the short list's, reset it.
    fn funnel_sorts(&mut self, ui: &mut egui::Ui, col: Col, title: &str) {
        let (up, down) = match col {
            Col::BeatRate | Col::LiftAngle => {
                ("Sort Smallest to Largest", "Sort Largest to Smallest")
            }
            Col::Short => ("Sort Starred First", "Sort Starred Last"),
            _ => ("Sort A to Z", "Sort Z to A"),
        };
        for (text, reversed) in [(up, false), (down, true)] {
            let on = self.sort == (col, reversed);
            if ui
                .add(egui::Button::selectable(on, text).frame_when_inactive(false))
                .on_hover_text(format!("Sort the table on {title}"))
                .clicked()
            {
                self.sort = (col, reversed);
                ui.close();
            }
        }
        let filtered = self.funnels[col as usize].is_some();
        if ui
            .add_enabled(
                filtered,
                egui::Button::new(format!("Clear Filter From \u{201c}{title}\u{201d}"))
                    .frame_when_inactive(false),
            )
            .on_hover_text(format!("Show every {title} value again"))
            .on_disabled_hover_text(format!("{title} isn't filtered"))
            .clicked()
        {
            self.funnels[col as usize] = None;
            ui.close();
        }
        if col == Col::Short
            && ui
                .add_enabled(
                    !self.short_is_default(),
                    egui::Button::new("Reset the Short List").frame_when_inactive(false),
                )
                .on_hover_text("Back to the common and modern calibres the app comes with")
                .on_disabled_hover_text("The short list is as the app comes with it")
                .clicked()
        {
            self.short_added.clear();
            self.short_removed.clear();
            ui.close();
        }
    }

    /// The funnel menu's values, each with a tickbox, under a search over
    /// them and Select All.
    /// Whether Enter was pressed, which is OK.
    fn funnel_ticks(&mut self, ui: &mut egui::Ui, col: Col, title: &str) -> bool {
        let pal = theme::pal(ui);
        let keys = theme::MenuSearch::field(
            ui,
            &mut self.funnel_search,
            &format!("Search {title} values"),
            "List only the values holding this",
        );
        let enter = keys.enter;
        keys.finish(ui);
        let offered = self.funnel_offered(col);
        let listed = self.funnel_listed(col);
        let listed: Vec<&String> = listed.iter().collect();
        let searching = !self.funnel_search.trim().is_empty();
        let shown = |s: &Self, v: &str| s.funnel_draft.as_ref().is_none_or(|k| k.contains(v));
        // The list keeps one height however few values a search leaves, so
        // the menu doesn't shrink as you type.
        let list_h = 240.0;
        egui::Frame::new()
            .stroke(egui::Stroke::new(1.0_f32, pal.separator))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(6, 4))
            .show(ui, |ui| {
                let mut all = !listed.is_empty() && listed.iter().all(|v| shown(self, v));
                // As a spreadsheet's: with a search typed, it picks only
                // the values the search lists.
                let (text, hint) = if searching {
                    (
                        "(Select All Search Results)",
                        "Tick or untick every value the search lists",
                    )
                } else {
                    ("(Select All)", "Tick or untick every value")
                };
                if ui.checkbox(&mut all, text).on_hover_text(hint).changed() {
                    for v in &listed {
                        self.draft_set(&offered, v, all);
                    }
                }
                let mut flips: Vec<(String, bool)> = Vec::new();
                egui::ScrollArea::vertical()
                    .id_salt(("funnel values", col as usize))
                    .min_scrolled_height(list_h)
                    .max_height(list_h)
                    .auto_shrink([false, false])
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
                        if listed.is_empty() {
                            ui.label(
                                RichText::new("No value matches.")
                                    .small()
                                    .color(pal.text_tertiary),
                            );
                        }
                    });
                for (v, on) in flips {
                    self.draft_set(&offered, &v, on);
                }
            });
        enter
    }

    /// Cancel, and OK to keep the values ticked as `col`'s filter: Enter
    /// is OK.
    fn funnel_ok(&mut self, ui: &mut egui::Ui, col: Col, title: &str, enter: bool) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button("Cancel")
                .on_hover_text("Close, leaving the filter as it was")
                .clicked()
            {
                ui.close();
            }
            let kept = self.funnel_kept(col);
            let none = kept.as_ref().is_some_and(|k| k.is_empty());
            if ui
                .add_enabled(!none, egui::Button::new("OK"))
                .on_hover_text(format!(
                    "Show only the {title} values ticked, of those the search lists"
                ))
                .on_disabled_hover_text("Tick at least one value")
                .clicked()
                || (enter && !none)
            {
                self.funnels[col as usize] = kept;
                self.funnel_draft = None;
                ui.close();
            }
        });
    }

    /// What OK keeps as `col`'s filter: the values ticked, and with a
    /// search typed only those of them it lists, as a spreadsheet's filter
    /// keeps its search results. Every value kept is no filter.
    fn funnel_kept(&self, col: Col) -> Option<std::collections::BTreeSet<String>> {
        let draft = self.funnel_draft.as_ref();
        if self.funnel_search.trim().is_empty() {
            return draft.cloned();
        }
        let kept: std::collections::BTreeSet<String> = self
            .funnel_listed(col)
            .into_iter()
            .filter(|v| draft.is_none_or(|k| k.contains(v)))
            .collect();
        let offered = self.funnel_offered(col);
        (!offered.iter().all(|v| kept.contains(v))).then_some(kept)
    }

    /// Tick or untick `value` in the funnel menu open, the other values
    /// `offered` staying as they are; every one ticked is no filter.
    fn draft_set(&mut self, offered: &[String], value: &str, on: bool) {
        let keep = self
            .funnel_draft
            .get_or_insert_with(|| offered.iter().cloned().collect());
        if on {
            keep.insert(value.to_string());
        } else {
            keep.remove(value);
        }
        if offered.iter().all(|v| keep.contains(v)) {
            self.funnel_draft = None;
        }
    }

    /// One calibre's row; true when it was clicked.
    fn table_row(
        &self,
        ui: &mut egui::Ui,
        e: Entry<'_>,
        widths: &[f32; 5],
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
            match (col, e) {
                (Col::Short, Entry::Table(c)) => self.short_star(ui, c, cell, toggles),
                (Col::Short, Entry::Custom(_)) => {}
                (Col::BeatRate | Col::LiftAngle, _) => number_cell(ui, e, col, cell),
                _ => self.text_cell(ui, e, col, cell, current),
            }
        }
        r.clicked()
    }

    /// Calibre `c`'s short-list star in `cell`: filled on the short list,
    /// an outline off it; a click asks for the other in `toggles`.
    fn short_star(
        &self,
        ui: &mut egui::Ui,
        c: &Calibre,
        cell: egui::Rect,
        toggles: &mut Vec<(String, bool)>,
    ) {
        let pal = theme::pal(ui);
        let on = self.in_short(c);
        let at = egui::Rect::from_min_size(
            egui::pos2(cell.left() + 2.0, cell.top()),
            Vec2::new(18.0, ROW_H),
        );
        let r = ui
            .interact(
                at,
                ui.id().with(("short", &c.calibre)),
                egui::Sense::click(),
            )
            .on_hover_text(if on {
                "On your short list: click to keep it only in the full list"
            } else {
                "Click to put it on your short list, offered in the Calibre menu"
            });
        // A filled star on the short list, an outline off it.
        let ink = if on {
            pal.accent
        } else if r.hovered() {
            pal.text_secondary
        } else {
            pal.text_tertiary
        };
        let c0 = egui::pos2(at.left() + 9.0, at.center().y);
        if on {
            star(ui.painter(), c0, 6.5, ink);
        } else {
            star_outline(ui.painter(), c0, 6.5, ink);
        }
        if r.clicked() {
            toggles.push((c.calibre.clone(), !on));
        }
    }

    /// Column `col`'s words for `e` in `cell`; too long for it, cut at
    /// its end with "…", the whole of it in the row's hover.
    fn text_cell(
        &self,
        ui: &mut egui::Ui,
        e: Entry<'_>,
        col: Col,
        cell: egui::Rect,
        current: &str,
    ) {
        let pal = theme::pal(ui);
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
        // Too long for its column: cut at its end with "…",
        // the whole of it in the row's hover.
        let font = egui::FontId::proportional(12.5);
        let room = cell.width() - 8.0;
        let mut galley = ui.painter().layout_no_wrap(text.clone(), font.clone(), ink);
        let chars: Vec<char> = text.chars().collect();
        let mut keep = chars.len();
        while galley.size().x > room && keep > 2 {
            keep -= 1;
            let cut: String = chars[..keep].iter().chain(std::iter::once(&'…')).collect();
            galley = ui.painter().layout_no_wrap(cut, font.clone(), ink);
        }
        let at = egui::pos2(cell.left() + 4.0, cell.center().y - galley.size().y / 2.0);
        ui.painter()
            .with_clip_rect(cell.shrink2(Vec2::new(2.0, 0.0)))
            .galley(at, galley, ink);
    }

    /// The calibre selected in Manage Calibres, in full, read top to
    /// bottom: what it is, its own facts when it is yours, its wheels, where
    /// the figures come from, and at the foot what can be done with it.
    /// `shown` says the calibre selected is among the rows shown.
    fn details(
        &mut self,
        ui: &mut egui::Ui,
        current: &str,
        version: &str,
        shown: bool,
        out: &mut Managed,
    ) {
        let pal = theme::pal(ui);
        let Some(name) = self
            .selected
            .clone()
            .filter(|n| shown && self.find(n).is_some())
        else {
            ui.label(
                RichText::new(
                    "Select a calibre in the table to see its wheels, sources and notes, and \
                     add what you know. New Custom Calibre… adds one the list doesn't have.",
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
        // The name alone: every fact in the row stays in the row.
        ui.label(RichText::new(title).strong().size(15.0));
        if let Entry::Table(c) = e {
            if !c.family.is_empty() {
                ui.label(
                    RichText::new(format!("Also known as {}", c.family.join(", ")))
                        .small()
                        .color(pal.text_tertiary),
                );
            }
        }
        let share = Catalog::new(&self.user).contribution_url(e, version);
        ui.add_space(10.0);
        if custom {
            ui.label(theme::caption("Calibre"));
            out.change = self.custom_editor(ui, &name, "manager");
            ui.add_space(10.0);
        }
        out.change.wheels |= self.wheels_and_sources(ui, &name, "manager");
        // What can be done with it, at the foot.
        ui.add_space(12.0);
        ui.separator();
        ui.horizontal(|ui| {
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
                    "Nothing to share yet: type a wheel's period or its teeth above, or add a custom calibre",
                )
                .clicked()
            {
                if let Some(url) = &share {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            }
            if custom {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("Delete Calibre")
                        .on_hover_text("Forget this calibre and the wheels typed for it")
                        .clicked()
                    {
                        self.user.remove_custom(&name);
                        self.save();
                        out.change.deleted = true;
                    }
                });
            }
        });
        if out.change.deleted {
            self.selected = None;
        } else if let Some(n) = &out.change.renamed {
            self.selected = Some(n.clone());
        }
        out.name = name;
    }

    /// A custom calibre's name, maker, beat rate and lift angle.
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
                    let was = c.bph;
                    c.bph = (b > 0.0).then_some(b.round() as u32);
                    follow_balance(c, was);
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
                    // 0 leaves it unset; any other lift is held to the range.
                    0.0..=*LIFT_RANGE_DEG.end(),
                    &LIFT,
                    50.0,
                ) {
                    c.lift_angle_deg =
                        (l > 0.0).then(|| l.clamp(*LIFT_RANGE_DEG.start(), *LIFT_RANGE_DEG.end()));
                    change.lift = c.lift_angle_deg;
                }
                ui.label("°");
            },
        );
        let renamed = c.name.trim().to_string();
        if renamed != before.name {
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
    show: train::period_text,
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

/// The balance's period, the time for one full swing and back: two beats.
fn balance_period(bph: u32) -> f64 {
    train::balance_period_s(bph)
}

/// After a custom calibre's beat rate changed from `was`: the balance's
/// period follows it, unless one was typed that the old rate didn't give.
fn follow_balance(c: &mut CustomCalibre, was: Option<u32>) {
    let at = c.wheels.iter().position(|w| w.name == "balance");
    let derived = |p: f64| was.is_some_and(|b| (balance_period(b) - p).abs() < 1e-9);
    let free = at.is_none_or(|i| derived(c.wheels[i].period_s));
    if !free {
        return;
    }
    match (c.bph, at) {
        (Some(b), Some(i)) => c.wheels[i].period_s = balance_period(b),
        (Some(b), None) => c.wheels.insert(
            0,
            CustomWheel {
                name: "balance".into(),
                period_s: balance_period(b),
            },
        ),
        (None, Some(i)) => {
            c.wheels.remove(i);
        }
        (None, None) => {}
    }
}

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

/// A beat rate or lift angle in `cell`, right-aligned so the digits line
/// up; a dim dash where the list gives nothing.
fn number_cell(ui: &egui::Ui, e: Entry<'_>, col: Col, cell: egui::Rect) {
    let pal = theme::pal(ui);
    let value = match col {
        Col::BeatRate => e.bph().map(|b| b.to_string()),
        _ => e.lift_angle_deg().map(|l| fields::plain(l).to_string()),
    };
    let (text, ink) = match value {
        Some(v) => (v, pal.text_secondary),
        None => ("—".to_string(), pal.text_tertiary),
    };
    ui.painter().text(
        egui::pos2(cell.right() - NUMBER_PAD, cell.center().y),
        egui::Align2::RIGHT_CENTER,
        text,
        egui::FontId::proportional(12.5),
        ink,
    );
}

/// Where column `col`'s heading sits in `cell`: the part a click sorts
/// on, its funnel, and where its title starts; `sort_w` keeps room for
/// the sort mark.
fn head_layout(
    ui: &egui::Ui,
    col: Col,
    title: &str,
    cell: egui::Rect,
    sort_w: f32,
) -> (egui::Rect, egui::Rect, f32) {
    let pal = theme::pal(ui);
    // The funnel follows its own heading, so it is never read as
    // the next column's.
    let title_w = if col == Col::Short {
        14.0
    } else {
        ui.painter()
            .layout_no_wrap(
                title.to_string(),
                egui::FontId::proportional(11.5),
                pal.text,
            )
            .size()
            .x
    };
    // A number's heading ends over its numbers, its ▾ just after.
    let numeric = matches!(col, Col::BeatRate | Col::LiftAngle);
    let after = cell.left() + 4.0 + title_w + sort_w;
    let funnel_x = if numeric {
        cell.right() - 9.0
    } else {
        (after + 11.0).min(cell.right() - 9.0)
    };
    let funnel =
        egui::Rect::from_center_size(egui::pos2(funnel_x, cell.center().y), Vec2::splat(15.0));
    let title_x = if numeric {
        (funnel.left() - 2.0 - sort_w - title_w).max(cell.left() + 4.0)
    } else {
        cell.left() + 4.0
    };
    let words = egui::Rect::from_min_max(cell.min, egui::pos2(funnel.left() - 2.0, cell.bottom()));
    (words, funnel, title_x)
}

/// A small triangle at `c`: up for the order as it reads, down when
/// `reversed`.
fn sort_mark(painter: &egui::Painter, c: egui::Pos2, reversed: bool, ink: egui::Color32) {
    let s = 3.0;
    let pts = if reversed {
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
    painter.add(egui::Shape::convex_polygon(pts, ink, egui::Stroke::NONE));
}

/// Where a wheel's period comes from, for its hover; `worked` is the
/// wheel worked out from its teeth, where the train counts it.
fn slot_tip(s: &Slot, worked: Option<&train::Worked>) -> String {
    if let Some(how) = worked
        .filter(|w| w.differs().is_some())
        .and_then(|w| w.how())
    {
        let given = slot_tip(s, None);
        return format!("{}. The teeth give {how}", given.trim_end_matches('.'));
    }
    match s.origin {
        Origin::Counts => s.basis.clone().unwrap_or_default(),
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
        Origin::Unknown if worked.and_then(|w| w.clash.as_ref()).is_some() => {
            worked.and_then(|w| w.clash_text()).unwrap_or_default()
        }
        Origin::Unknown => match worked.and_then(|w| w.needs_text()) {
            Some(needs) => format!(
                "Nobody has given this wheel's period yet: type it if you know it. \
                 From its teeth: {}",
                lower_first(&needs)
            ),
            None => "Nobody has given this wheel's period yet: type it if you know it".into(),
        },
    }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_lowercase().chain(c).collect())
        .unwrap_or_default()
}

/// The arbors' short names in Teeth and Leaves, as watchmakers say them.
fn arbor_label(arbor: &str) -> String {
    title_case(arbor.strip_suffix(" wheel").unwrap_or(arbor))
}

/// A count typed in Teeth and Leaves: the arbor, then `Some(new)` for each
/// of its teeth and leaves changed, `Some(None)` for one cleared.
type CountEdit = (&'static str, Option<Option<u32>>, Option<Option<u32>>);

/// Teeth and Leaves: each arbor's wheel teeth and pinion leaves, closed
/// until opened. Returns the counts typed this frame: the arbor, then
/// `Some(new)` for each count changed (`Some(None)` when cleared).
fn teeth_and_leaves(
    ui: &mut egui::Ui,
    train: &[train::Arbor],
    place: &str,
    name: &str,
) -> Vec<CountEdit> {
    let pal = theme::pal(ui);
    // Not under the parent's id: the panel lays out afresh on a resize.
    let open_id = egui::Id::new(("teeth and leaves open", place));
    let mut open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
    let mut out = Vec::new();
    let head = disclosure(ui, "Teeth and Leaves", open).on_hover_text(if open {
        "Hide the teeth and leaves"
    } else {
        "Type the teeth and leaves you count, and the wheels' periods follow"
    });
    if head.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(open_id, open));
    }
    if !open {
        ui.add_space(12.0);
        return out;
    }
    let group = |g: &mut theme::Group<'_>| {
        count_heads(g.ui);
        for &arbor in &train::ARBORS {
            let a = train.iter().find(|a| a.name == arbor);
            let pinion = train::pinion_name(arbor);
            let hint = if arbor == "barrel" {
                "The barrel's teeth. It has no pinion".to_string()
            } else {
                format!("The {arbor}'s teeth, and the leaves of the {pinion} on its arbor")
            };
            g.row(&arbor_label(arbor), theme::Caption::None, &hint, |ui| {
                // Both boxes' width from the first frame, so the row never
                // squeezes the Leaves box while it learns its width.
                ui.set_min_width(2.0 * COUNT_BOX_W + ui.spacing().item_spacing.x);
                let id = format!("calibre-{place}-{name}-{arbor}");
                let teeth =
                    count_field(ui, &format!("{id}-teeth"), a.and_then(|a| a.teeth), "Teeth");
                let leaves = if arbor == "barrel" {
                    ui.add_sized(Vec2::new(COUNT_BOX_W, 20.0), egui::Label::new("\u{2013}"));
                    None
                } else {
                    count_field(
                        ui,
                        &format!("{id}-leaves"),
                        a.and_then(|a| a.leaves),
                        "Leaves",
                    )
                };
                if teeth.is_some() || leaves.is_some() {
                    out.push((arbor, teeth, leaves));
                }
            });
        }
    };
    // Rows of their own: Wheels has a Barrel row too, and a row keeps its
    // width by its name.
    ui.push_id("teeth and leaves", |ui| theme::group_panel(ui, group));
    ui.add_space(-8.0);
    ui.add(
        egui::Label::new(
            RichText::new(
                "Type each wheel's teeth and its pinion's leaves. The periods above follow.",
            )
            .size(11.5)
            .color(pal.text_secondary),
        )
        .wrap(),
    );
    ui.add_space(12.0);
    out
}

/// A group's title that opens and closes it: a disclosure triangle, right
/// when closed and down when open, then the title, both clickable.
fn disclosure(ui: &mut egui::Ui, title: &str, open: bool) -> egui::Response {
    let pal = theme::pal(ui);
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.spacing_mut().item_spacing.x = 4.0;
        let (r, icon) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::click());
        let c = r.center();
        let pts = if open {
            [(-4.0, -2.0), (4.0, -2.0), (0.0, 3.0)]
        } else {
            [(-2.0, -4.0), (3.0, 0.0), (-2.0, 4.0)]
        };
        ui.painter().add(egui::Shape::convex_polygon(
            pts.iter().map(|&(x, y)| c + Vec2::new(x, y)).collect(),
            pal.text_secondary,
            egui::Stroke::NONE,
        ));
        let label = ui.add(
            egui::Label::new(RichText::new(title).size(12.0).color(pal.text_secondary))
                .sense(egui::Sense::click()),
        );
        icon.union(label)
    })
    .inner
}

/// Over the count boxes, once at the top of Teeth and Leaves: which
/// column is which, as a table's head.
fn count_heads(ui: &mut egui::Ui) {
    let pal = theme::pal(ui);
    let w = ui.available_width();
    ui.allocate_ui_with_layout(
        Vec2::new(w, 24.0),
        egui::Layout::right_to_left(egui::Align::Max),
        |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for head in ["Leaves", "Teeth"] {
                ui.add_sized(
                    Vec2::new(COUNT_BOX_W, 16.0),
                    egui::Label::new(RichText::new(head).size(11.5).color(pal.text_secondary)),
                );
            }
        },
    );
}

/// A count box's width with its margins, points.
const COUNT_BOX_W: f32 = 54.0;

/// A count's box: `Some(new)` when it lost focus changed, `Some(None)`
/// when emptied; a count that is not a whole number above 0 is ignored.
fn count_field(
    ui: &mut egui::Ui,
    id_salt: &str,
    value: Option<u32>,
    hint: &str,
) -> Option<Option<u32>> {
    let shown = value.map(|v| v.to_string()).unwrap_or_default();
    blank_field(
        ui,
        id_salt,
        shown,
        hint,
        hint,
        COUNT_BOX_W - 8.0,
        |t| match train::parse_count(t) {
            Ok((Some(v), None)) => Some(v),
            _ => None,
        },
    )
}

/// A wheel's period box, with Clear on one typed in; what changed goes
/// in `edits`, `None` for back to the list's.
fn slot_period(
    ui: &mut egui::Ui,
    s: &Slot,
    id: &str,
    tip: &str,
    edits: &mut Vec<(String, Option<f64>)>,
) {
    if s.origin == Origin::User
        && ui
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
    if let Some(v) = period_field(ui, id, s.period_s, tip) {
        edits.push((s.name.clone(), v));
    }
    ui.label("s");
}

/// Another wheel by name, such as an intermediate wheel: a row of its
/// own, kept once it has both.
fn add_wheel_row(
    ui: &mut egui::Ui,
    adding: &mut Option<(String, String)>,
    place: &str,
    edits: &mut Vec<(String, Option<f64>)>,
) {
    let pal = theme::pal(ui);
    let Some((wheel, period)) = adding.as_mut() else {
        if ui
            .add(
                egui::Label::new(RichText::new("Add a Wheel…").color(pal.accent))
                    .sense(egui::Sense::click()),
            )
            .on_hover_text(
                "Add a wheel the list above doesn't name, such as an intermediate \
                 wheel: type its name and its period",
            )
            .clicked()
        {
            *adding = Some((String::new(), String::new()));
        }
        return;
    };
    let mut close = false;
    let mut keep = false;
    // Add and Cancel go under the fields where the details are
    // too narrow for them beside.
    ui.horizontal_wrapped(|ui| {
        let n = ui.add(
            egui::TextEdit::singleline(wheel)
                .id_salt(("another wheel", place))
                .hint_text("Wheel name")
                .desired_width(theme::LABEL_W - ui.spacing().item_spacing.x),
        );
        let p = ui.add(
            egui::TextEdit::singleline(period)
                .id_salt(("another period", place))
                .hint_text("period")
                .desired_width(period_box_w(ui)),
        );
        ui.label("s");
        let whole =
            !wheel.trim().is_empty() && fields::parse_number(period).is_some_and(|v| v > 0.0);
        let added = ui
            .add_enabled(whole, egui::Button::new("Add"))
            .on_hover_text("Add this wheel to the calibre (or press Return)")
            .on_disabled_hover_text("Type the wheel's name and its period in seconds")
            .clicked();
        if ui
            .button("Cancel")
            .on_hover_text("Forget this row without adding a wheel")
            .clicked()
        {
            close = true;
        }
        let entered =
            (n.lost_focus() || p.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
        keep = whole && (added || entered);
    });
    if keep {
        if let Some(v) = fields::parse_number(period) {
            edits.push((wheel.trim().to_lowercase(), Some(v)));
        }
        close = true;
    }
    if close {
        *adding = None;
    }
}

/// The table's sources for a calibre, each linked where it can be, and
/// its notes.
fn sources_and_notes(ui: &mut egui::Ui, c: &Calibre) {
    let pal = theme::pal(ui);
    if !c.sources.is_empty() {
        let title = format!("Sources · {}", title_case(&c.confidence));
        theme::group(ui, &title, None, |g| {
            for s in &c.sources {
                let (name, url, says) = source_parts(s);
                g.whole(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    match &url {
                        Some(u) => {
                            ui.hyperlink_to(format!("{name} ↗"), u).on_hover_text(u);
                        }
                        None => {
                            ui.add(egui::Label::new(name.as_str()).wrap());
                        }
                    }
                    if !says.is_empty() {
                        ui.add(
                            egui::Label::new(
                                RichText::new(says.as_str())
                                    .size(11.5)
                                    .color(pal.text_secondary),
                            )
                            .wrap(),
                        );
                    }
                });
            }
        });
    }
    if let Some(n) = &c.notes {
        ui.add(egui::Label::new(RichText::new(n).small().color(pal.text_tertiary)).wrap());
    }
}

/// The menu's list as it is drawn: the calibre picked this frame, if any
/// ("" for Not Set), and the keys over the list (↑/↓, Enter).
struct Picked {
    name: std::cell::RefCell<Option<String>>,
    keys: std::cell::RefCell<theme::MenuSearch>,
}

impl Picked {
    /// One item, `on` ticked: picked by a click or Enter, as `name`.
    fn item(&self, ui: &mut egui::Ui, on: bool, text: String, hover: &str, name: &str) {
        let (r, picked) = self.keys.borrow_mut().item(ui, on, text);
        r.on_hover_text(hover);
        if picked {
            *self.name.borrow_mut() = Some(name.to_string());
        }
    }
}

/// One calibre in the menu, ticked when it is `current`.
fn menu_line(ui: &mut egui::Ui, e: Entry<'_>, text: String, current: &str, picked: &Picked) {
    picked.item(ui, e.name() == current, text, &hint(e), e.name());
}

/// Every calibre the app knows, by any of its names or its maker, without
/// opening Manage Calibres; ready to type as soon as the menu opens.
fn search_field(ui: &mut egui::Ui, typed: &mut String) -> theme::MenuSearch {
    theme::MenuSearch::field(
        ui,
        typed,
        "Search every calibre",
        "Type part of a calibre's name, another name for it or its maker, to list every \
         calibre that matches",
    )
}

/// The calibres matching `q`, the first few hundred of them.
fn search_hits(ui: &mut egui::Ui, catalog: &Catalog<'_>, q: &str, current: &str, picked: &Picked) {
    let pal = theme::pal(ui);
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
        menu_line(ui, *e, text, current, picked);
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

/// "4,537 calibres", "1 calibre".
fn count_words(n: usize) -> String {
    format!(
        "{} {}",
        grouped(n),
        if n == 1 { "calibre" } else { "calibres" }
    )
}

/// The table's count: "4,537 calibres" with all shown, else "6 of 4,537
/// calibres", as the Watches list counts its watches.
fn shown_of(shown: usize, all: usize) -> String {
    if shown == all {
        count_words(all)
    } else {
        format!("{} of {}", grouped(shown), count_words(all))
    }
}

/// `n` with its thousands apart by commas.
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut grouped = String::new();
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(d);
    }
    grouped
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

/// A source as its name, its address when it has one, and what it says.
/// "https://watchguy.co.uk/… (A. Schild 1017, 51°)" is watchguy.co.uk
/// saying "51°" (the calibre's own name, first, is dropped: the panel
/// is headed by it); a source with a description before what it says,
/// "https://docs.google.com/… (lift angle sheet linked from
/// watchrepairtalk.com/topic/…) (…)", is named "Lift angle sheet
/// (watchrepairtalk.com)".
pub fn source_parts(s: &str) -> (String, Option<String>, String) {
    let (url, rest) = match s.split_once(' ') {
        Some((u, r)) if u.starts_with("http") => (Some(u.to_string()), r.trim()),
        None if s.starts_with("http") => (Some(s.to_string()), ""),
        _ => (None, s.trim()),
    };
    let host = |u: &str| {
        u.trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("www.")
            .split('/')
            .next()
            .unwrap_or(u)
            .to_string()
    };
    let Some(url) = url else {
        return (rest.to_string(), None, String::new());
    };
    let groups = paren_groups(rest);
    let (name, says) = match groups.as_deref() {
        Some([one]) => (host(&url), one.clone()),
        Some([first, .., last]) => {
            let name = match first.split_once(" linked from ") {
                Some((what, from)) => format!("{} ({})", capitalised(what), host(from)),
                None => capitalised(first),
            };
            (name, last.clone())
        }
        _ => (host(&url), rest.to_string()),
    };
    (name, Some(url), drop_calibre_name(&says))
}

/// `s` as nothing but bracketed groups, "(a) (b (c))", each group's
/// inside; `None` for anything else.
fn paren_groups(s: &str) -> Option<Vec<String>> {
    let mut groups = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '(' => {
                if depth > 0 {
                    cur.push(ch);
                }
                depth += 1;
            }
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
                if depth == 0 {
                    groups.push(std::mem::take(&mut cur));
                } else {
                    cur.push(ch);
                }
            }
            c if depth == 0 => {
                if !c.is_whitespace() {
                    return None;
                }
            }
            c => cur.push(c),
        }
    }
    (depth == 0 && !groups.is_empty()).then_some(groups)
}

/// What a source says, without the calibre's name in front: "A. Schild
/// 1017, 51°, 18000 vph" is "51°, 18000 vph". A first part holding a
/// figure (a degree sign or vph) is kept.
fn drop_calibre_name(says: &str) -> String {
    let mut depth = 0;
    for (i, ch) in says.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                let first = &says[..i];
                if first.contains('°') || first.contains("vph") || first.contains("bph") {
                    return says.to_string();
                }
                return says[i + 1..].trim().to_string();
            }
            _ => {}
        }
    }
    says.to_string()
}

/// "lift angle sheet" as "Lift angle sheet".
fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// The widest period a box shows whole: six figures of seconds, about
/// eleven days, to the thousandth (`train::period_text`). A longer one
/// drops decimals to fit (`train::period_text_within`).
const WIDEST_PERIOD: &str = "000000.000";

/// Room the cursor takes beside the text in a period box.
const CURSOR_ROOM: f32 = 4.0;

/// `t`'s width in a text box's font.
fn box_text_w(ui: &egui::Ui, t: &str) -> f32 {
    let font = egui::FontSelection::default().resolve(ui.style());
    ui.fonts_mut(|f| {
        f.layout_no_wrap(t.into(), font, egui::Color32::WHITE)
            .size()
            .x
    })
}

/// A period box's inner width: room for [`WIDEST_PERIOD`] and the
/// cursor, so no digit sits under the box's edge (#1586).
fn period_box_w(ui: &egui::Ui) -> f32 {
    (box_text_w(ui, WIDEST_PERIOD) + CURSOR_ROOM).ceil()
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
    let w = period_box_w(ui);
    let (shown, tip) = period_shown(ui, value, tip);
    blank_field(ui, id_salt, shown, "Not Known", &tip, w, |t| {
        fields::parse_number(t).filter(|v| *v > 0.0)
    })
}

/// What a period box shows, decimals dropped to fit its width, and its
/// hover, which gives the full value when any were dropped (#1586).
fn period_shown(ui: &egui::Ui, value: Option<f64>, tip: &str) -> (String, String) {
    let w = period_box_w(ui);
    let fits = |t: &str| box_text_w(ui, t) + CURSOR_ROOM <= w;
    let Some(v) = value else {
        return (String::new(), tip.to_owned());
    };
    match train::period_text_within(v, fits) {
        (shown, true) => (shown, format!("{tip}\n\nIn full: {} s", (PERIOD.show)(v))),
        (shown, false) => (shown, tip.to_owned()),
    }
}

/// A box that may be blank, showing `shown`. Enter or clicking away reads
/// it with `parse`: `Some(Some(v))` for a new value, `Some(None)` when
/// emptied, `None` when nothing changed or it doesn't read.
#[allow(clippy::too_many_arguments)]
fn blank_field<T>(
    ui: &mut egui::Ui,
    id_salt: &str,
    shown: String,
    hint: &str,
    tip: &str,
    width: f32,
    parse: impl FnOnce(&str) -> Option<T>,
) -> Option<Option<T>> {
    let id = ui.make_persistent_id(id_salt);
    let buf = id.with("text");
    let mut text = ui
        .data_mut(|d| d.get_temp::<String>(buf))
        .unwrap_or_else(|| shown.clone());
    let resp = ui
        .add(
            egui::TextEdit::singleline(&mut text)
                .id(id)
                .hint_text(hint)
                .desired_width(width),
        )
        .on_hover_text(tip);
    let mut out = None;
    if resp.lost_focus() {
        if text.trim() != shown {
            out = if text.trim().is_empty() {
                Some(None)
            } else {
                parse(&text).map(Some)
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

/// A five-pointed star of radius `r` at `c`, filled with `ink`.
fn star_points(c: egui::Pos2, r: f32) -> Vec<egui::Pos2> {
    (0..10)
        .map(|i| {
            let a = std::f32::consts::PI * (i as f32 / 5.0 - 0.5);
            let d = if i % 2 == 0 { r } else { r * 0.45 };
            c + Vec2::new(a.cos(), a.sin()) * d
        })
        .collect()
}

/// A five-pointed star's outline.
fn star_outline(painter: &egui::Painter, c: egui::Pos2, r: f32, ink: egui::Color32) {
    painter.add(egui::Shape::closed_line(
        star_points(c, r),
        egui::Stroke::new(1.2_f32, ink),
    ));
}

fn star(painter: &egui::Painter, c: egui::Pos2, r: f32, ink: egui::Color32) {
    let pts = star_points(c, r);
    // Not convex: drawn as five triangles from the centre and the rim.
    for i in 0..10 {
        let (a, b) = (pts[i], pts[(i + 1) % 10]);
        painter.add(egui::Shape::convex_polygon(
            vec![c, a, b],
            ink,
            egui::Stroke::NONE,
        ));
    }
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
#[path = "calibre_ui_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "calibre_ui_menu_tests.rs"]
mod menu_search_tests;
