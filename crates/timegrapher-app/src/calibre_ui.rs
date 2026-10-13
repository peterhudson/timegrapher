//! The Calibre menu under Watch and the panel beneath it. The menu offers
//! the calibres picked lately and the common ones, every calibre by maker
//! behind Show All Calibres, and a search over names, other names and
//! makers. The panel shows what is known about the calibre picked: its
//! beat rate and lift angle, each wheel's turn with where the figure comes
//! from, and the table's sources. Wheels nobody has given are left blank
//! for the watchmaker to fill in; those figures, and calibres the table
//! doesn't list, are kept in core's user calibre file in the settings
//! folder, which the command line and the MCP server read too. Share with
//! the Project only opens a pre-filled GitHub issue in the browser: the
//! app sends nothing itself.

use crate::fields;
use crate::settings;
use crate::theme;
use eframe::egui::{self, RichText};
use std::path::PathBuf;
use std::sync::OnceLock;
use timegrapher_core::calibre_store::{
    Catalog, CustomCalibre, CustomWheel, Entry, Origin, UserCalibres,
};
use timegrapher_core::calibres::{self, Calibre};
use timegrapher_core::periodicity::Wheel;

/// How many calibres picked lately the menu offers.
pub const RECENT: usize = 6;
/// How many search results the menu lists before asking for more letters.
const SHOWN: usize = 150;

/// The user's calibres and the menu's own state.
pub struct Calibres {
    pub user: UserCalibres,
    /// Where the user's additions are kept; `None` keeps them in memory
    /// (tests, or no settings folder).
    path: Option<PathBuf>,
    /// Calibre names picked lately, the latest first.
    pub recent: Vec<String>,
    /// Calibres the watchmaker added to the short list, and the list's
    /// own ones they took off it.
    pub short_added: Vec<String>,
    pub short_removed: Vec<String>,
    search: String,
    show_all: bool,
    /// Name and period typed for a wheel the slots don't list.
    extra: (String, String),
    /// Why the user's calibres couldn't be read or written, if so.
    pub problem: Option<String>,
}

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
    /// starts empty (and says why for the latter).
    pub fn open(path: Option<PathBuf>) -> Self {
        let mut c = Calibres::in_memory();
        if let Some(p) = &path {
            match UserCalibres::load(p) {
                Ok(u) => c.user = u,
                Err(e) => c.problem = Some(format!("Your calibres couldn't be read: {e}")),
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
            recent: Vec::new(),
            short_added: Vec::new(),
            short_removed: Vec::new(),
            search: String::new(),
            show_all: false,
            extra: (String::new(), String::new()),
            problem: None,
        }
    }

    fn save(&mut self) {
        if let Some(p) = &self.path {
            if let Err(e) = self.user.save(p) {
                self.problem = Some(format!("Your calibres couldn't be saved: {e}"));
            }
        }
    }

    pub fn find(&self, name: &str) -> Option<Entry<'_>> {
        Catalog::new(&self.user).find(name)
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
    /// from the app's settings: moved there, once, and dropped from the
    /// settings.
    pub fn migrate(&mut self, old: Vec<settings::CustomCalibre>) {
        let mut moved = false;
        for c in old {
            if self.user.custom.iter().any(|x| x.name == c.name) {
                continue;
            }
            self.user.put_custom(custom(
                &c.name,
                c.wheels.iter().map(|w| (w.name.as_str(), w.period_s)),
            ));
            moved = true;
        }
        if moved {
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

    /// The Calibre menu: what was picked ("" for Not Set), if anything.
    pub fn picker(&mut self, ui: &mut egui::Ui, current: &str) -> Option<String> {
        let pal = theme::pal(ui);
        let shown = match self.find(current) {
            Some(e) => e.name().to_string(),
            None => "Not Set".to_string(),
        };
        let mut picked: Option<String> = None;
        let mut new_custom: Option<String> = None;
        let mut typed = std::mem::take(&mut self.search);
        let mut show_all = self.show_all;
        let mut set_short: Vec<(String, bool)> = Vec::new();
        let mut reset = false;
        egui::ComboBox::from_id_salt("calibre")
            .width(ui.available_width())
            .height(460.0)
            .truncate()
            // Typing in the search box mustn't close the menu; a pick
            // closes it below.
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .selected_text(shown)
            .show_ui(ui, |ui| {
                let search = ui
                    .add(
                        egui::TextEdit::singleline(&mut typed)
                            .hint_text("Search calibres, makers and other names")
                            .desired_width(f32::INFINITY),
                    )
                    .on_hover_text(
                        "Every word must match: a calibre's number, one of its other names, or \
                     its maker under any of the maker's names (\"dd 3235\", \"sw200\", \
                     \"seagull st19\")",
                    );
                if !search.has_focus() && typed.is_empty() {
                    search.request_focus();
                }
                let toggles: std::cell::RefCell<Vec<(String, bool)>> = Default::default();
                let chosen: std::cell::RefCell<Option<String>> = Default::default();
                // A calibre's line: its tickbox for the short list (table
                // calibres only) and its name, which picks it.
                let line = |ui: &mut egui::Ui, e: Entry<'_>, text: String| {
                    ui.horizontal(|ui| {
                        if let Entry::Table(c) = e {
                            let mut on = self.in_short(c);
                            if ui
                                .checkbox(&mut on, "")
                                .on_hover_text(if on {
                                    "On your short list: untick to keep it only in the full list"
                                } else {
                                    "Tick to put it on your short list, offered first"
                                })
                                .changed()
                            {
                                toggles.borrow_mut().push((c.calibre.clone(), on));
                            }
                        }
                        let picked_now = ui
                            .selectable_label(e.name() == current, text)
                            .on_hover_text(hint(e))
                            .clicked();
                        if picked_now {
                            *chosen.borrow_mut() = Some(e.name().to_string());
                            ui.close();
                        }
                    });
                };
                let catalog = Catalog::new(&self.user);
                if !typed.trim().is_empty() {
                    let hits = catalog.search(&typed);
                    if hits.is_empty() {
                        ui.label(
                            RichText::new("No calibre matches.")
                                .small()
                                .color(pal.text_tertiary),
                        );
                    }
                    for &e in hits.iter().take(SHOWN) {
                        line(ui, e, label(e));
                    }
                    if hits.len() > SHOWN {
                        ui.label(
                            RichText::new(format!(
                                "{} more: type more of the name to narrow the list",
                                hits.len() - SHOWN
                            ))
                            .small()
                            .color(pal.text_tertiary),
                        );
                    }
                    ui.separator();
                    if ui
                        .selectable_label(false, format!("New Custom Calibre “{}”…", typed.trim()))
                        .on_hover_text(
                            "Add a calibre by this name, with the wheels' periods you type; \
                             it is kept for next time",
                        )
                        .clicked()
                    {
                        new_custom = Some(typed.clone());
                        ui.close();
                    }
                } else {
                    if ui
                        .selectable_label(current.is_empty(), "Not Set")
                        .on_hover_text(
                            "Name cycles after the wheels most calibres at this beat rate \
                             share, with the default lift angle",
                        )
                        .clicked()
                    {
                        picked = Some(String::new());
                        ui.close();
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
                                "Empty: tick calibres in Show All Calibres to put them here.",
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
                        .selectable_label(false, "New Custom Calibre…")
                        .on_hover_text(
                            "Add a calibre the list doesn't have, with the wheels' periods you \
                             type; it is kept for next time",
                        )
                        .clicked()
                    {
                        new_custom = Some(String::new());
                        ui.close();
                    }
                    let all = calibres::all().len();
                    if ui
                        .selectable_label(
                            show_all,
                            if show_all {
                                "Hide the Full List".to_string()
                            } else {
                                format!("Show All Calibres ({all})…")
                            },
                        )
                        .on_hover_text(
                            "Every calibre in the list by maker, the common and modern makers \
                             first, then the uncommon and vintage ones. Tick a calibre to put \
                             it on your short list.",
                        )
                        .clicked()
                    {
                        show_all = !show_all;
                    }
                    if show_all {
                        let mut vintage = false;
                        for (maker, common, list) in by_maker() {
                            if !common && !vintage {
                                vintage = true;
                                heading(ui, "Uncommon and Vintage");
                            }
                            let ticked = list.iter().filter(|c| self.in_short(c)).count();
                            let title = match ticked {
                                0 => format!("{maker} ({})", list.len()),
                                t => format!("{maker} ({}, {t} on the short list)", list.len()),
                            };
                            egui::CollapsingHeader::new(title)
                                .id_salt(("maker", *maker))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        if ui
                                            .small_button("Tick All")
                                            .on_hover_text(format!(
                                                "Put every {maker} calibre on your short list"
                                            ))
                                            .clicked()
                                        {
                                            toggles.borrow_mut().extend(
                                                list.iter().map(|c| (c.calibre.clone(), true)),
                                            );
                                        }
                                        if ui
                                            .small_button("Untick All")
                                            .on_hover_text(format!(
                                                "Take every {maker} calibre off your short list"
                                            ))
                                            .clicked()
                                        {
                                            toggles.borrow_mut().extend(
                                                list.iter().map(|c| (c.calibre.clone(), false)),
                                            );
                                        }
                                    });
                                    for c in list {
                                        line(ui, Entry::Table(c), c.calibre.clone());
                                    }
                                });
                        }
                    }
                    if !self.short_is_default()
                        && ui
                            .selectable_label(false, "Reset the Short List")
                            .on_hover_text(
                                "Back to the common and modern calibres the app comes with",
                            )
                            .clicked()
                    {
                        reset = true;
                    }
                }
                set_short = toggles.into_inner();
                if let Some(c) = chosen.into_inner() {
                    picked = Some(c);
                }
            });
        self.search = typed;
        self.show_all = show_all;
        for (name, on) in set_short {
            self.set_short(&name, on);
        }
        if reset {
            self.short_added.clear();
            self.short_removed.clear();
        }
        if let Some(t) = new_custom {
            picked = Some(self.new_custom(&t));
        }
        if picked.is_some() {
            self.search.clear();
        }
        picked
    }

    /// What is known about the calibre `current`, under the menu: a
    /// summary line and, folded away until opened, each wheel's turn with
    /// where it comes from, blanks to fill in, the table's sources and the
    /// way to share what was added. A custom calibre is edited here.
    pub fn panel(&mut self, ui: &mut egui::Ui, current: &str, version: &str) -> Change {
        let pal = theme::pal(ui);
        let mut change = Change::default();
        let Some(e) = self.find(current) else {
            return change;
        };
        let name = e.name().to_string();
        let custom = e.is_custom();
        ui.label(RichText::new(summary(e)).small().color(pal.text_secondary));
        let catalog = Catalog::new(&self.user);
        let slots = catalog.slots(e);
        let blanks = slots.iter().filter(|s| s.period_s.is_none()).count();
        let share = catalog.contribution_url(e, version);
        let table = match e {
            Entry::Table(c) => Some(c),
            Entry::Custom(_) => None,
        };
        let mut edits: Vec<(String, Option<f64>)> = Vec::new();
        let header = if blanks > 0 {
            format!("Wheels and Sources ({blanks} not known)")
        } else {
            "Wheels and Sources".to_string()
        };
        egui::CollapsingHeader::new(header)
            .id_salt("calibre panel")
            .show(ui, |ui| {
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
                            "Nobody has given this wheel's period yet: type it if you know it"
                                .into()
                        }
                    };
                    let id = format!("calibre-{name}-{}", s.name);
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
                            .hint_text("Another wheel")
                            .desired_width(theme::LABEL_W - 16.0),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.extra.1)
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
                        ui.add(
                            egui::Label::new(RichText::new(n).small().color(pal.text_tertiary))
                                .wrap(),
                        );
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
                        "Nothing to share yet: type a wheel's period above, or add a custom \
                         calibre",
                    )
                    .clicked()
                {
                    if let Some(url) = &share {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    }
                }
            });
        if custom {
            change = self.custom_editor(ui, &name);
        }
        if !edits.is_empty() {
            for (wheel, v) in edits {
                match v {
                    Some(p) => self.user.set_wheel(&name, &wheel, p, None),
                    None => self.user.clear_wheel(&name, &wheel),
                }
            }
            self.save();
            change.wheels = true;
        }
        if let Some(p) = &self.problem {
            ui.label(RichText::new(p).small().color(pal.bad));
        }
        change
    }

    /// A custom calibre's name, maker, beat rate and lift angle, and the
    /// way to delete it.
    fn custom_editor(&mut self, ui: &mut egui::Ui, name: &str) -> Change {
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
                ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(f32::INFINITY));
            },
        );
        let mut maker = c.maker.clone().unwrap_or_default();
        theme::row(
            ui,
            "Maker",
            Some("Who made the movement, to search by"),
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut maker).desired_width(f32::INFINITY));
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
                    &format!("custom-bph-{i}"),
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
                    &format!("custom-lift-{i}"),
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_blank_period_box_reads_typed_numbers() {
        assert_eq!(title_case("escape wheel"), "Escape Wheel");
        let e = Entry::Table(&calibres::all()[0]);
        assert!(summary(e).contains("bph") || summary(e).contains("beat rate not given"));
    }
}
