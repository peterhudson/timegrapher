//! The menu bar: Watches, File, Edit, View and Help, and what File's Open asks before it opens.

use super::*;

impl TimegrapherApp {
    /// The menus across the very top, File, Edit, View and Help, and beside
    /// them an icon for each pane that shows or hides it.
    pub(super) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| self.file_menu(ui));
            ui.menu_button("Edit", |ui| self.edit_menu(ui));
            ui.menu_button("View", |ui| self.view_menu(ui));
            ui.menu_button("Watches", |ui| self.watches_menu(ui));
            ui.menu_button("Help", |ui| self.help_menu(ui));
            // The pane icons are the Bench's: in Watches they hide and
            // leave their room, so the gear stays where it is.
            ui.add_space(16.0);
            self.pane_icons(ui, self.view == View::Bench);
        });
    }

    /// The Watches menu: New Watch… and Find in Watches…. Going to the
    /// Watches view is View ▸ Watches (Cmd+2); No Watch (Quick Listen) is
    /// on the Calibre card's Watch line.
    pub(super) fn watches_menu(&mut self, ui: &mut egui::Ui) {
        if ui
            .button("New Watch…")
            .on_hover_text("Add a watch: its name, calibre, lift angle and beat rate")
            .clicked()
        {
            ui.close();
            self.open_new_watch_form();
        }
        if ui
            .button("Find in Watches…")
            .on_hover_text("Show the Watches view with its search field ready to type in")
            .clicked()
        {
            ui.close();
            self.find_in_watches();
        }
    }

    /// The Watches view's list, its search field ready to type in.
    pub(super) fn find_in_watches(&mut self) {
        self.view = View::Watches;
        if !self.list_shown() {
            self.list_for = Some((self.watch_open.clone(), self.unsorted_open));
        }
        self.watch_search_focus = true;
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        if ui
            .add(
                egui::Button::new("Open Recording…")
                    .shortcut_text(ui.ctx().format_shortcut(&command(egui::Key::O))),
            )
            .on_hover_text(
                "Choose a WAV or FLAC recording and analyse it all. From outside your \
                 watches, it asks first whether to add it to one of them.",
            )
            .clicked()
        {
            ui.close();
            self.open_from_menu(false);
        }
        if ui
            .button("Open Folder…")
            .on_hover_text(FOLDER_RULE)
            .clicked()
        {
            ui.close();
            self.open_from_menu(true);
        }
        ui.separator();
        self.file_menu_save(ui);
        // Anything shown, from the microphone or a recording, can be
        // cleared, listening or not.
        if ui
            .add_enabled(self.live.is_some(), egui::Button::new("New Session"))
            .on_hover_text(NEW_SESSION_HINT)
            .on_disabled_hover_text(
                "Nothing to clear yet: listen to a watch or open a recording first",
            )
            .clicked()
        {
            ui.close();
            if self.needs_save_confirmation() {
                self.confirm = Some(Confirm::NewSession);
            } else {
                self.new_session();
            }
        }
        ui.separator();
        if ui
            .add(
                egui::Button::new("Quit")
                    .shortcut_text(ui.ctx().format_shortcut(&command(egui::Key::Q))),
            )
            .clicked()
        {
            ui.close();
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// The File menu's Save, Save Again, Save As and Save Analysis Only
    /// items.
    fn file_menu_save(&mut self, ui: &mut egui::Ui) {
        let saved = self.session.saved_to().is_some();
        let can_save = self.session.allows(Action::Save);
        let saving = self.saving.is_some();
        if self.marks_unsaved() {
            if ui
                .add(
                    egui::Button::new("Save")
                        .shortcut_text(ui.ctx().format_shortcut(&command(egui::Key::S))),
                )
                .on_hover_text(SAVE_MARKS_HINT)
                .clicked()
            {
                ui.close();
                self.save_marks();
            }
        } else if ui
            .add_enabled(
                can_save && !saving,
                egui::Button::new(if saved { "Save Again" } else { "Save…" })
                    .shortcut_text(ui.ctx().format_shortcut(&command(egui::Key::S))),
            )
            .on_hover_text(SAVE_HINT)
            .on_disabled_hover_text(if saving {
                SAVING_HINT
            } else {
                "Nothing to save yet: listen to a watch first"
            })
            .clicked()
        {
            ui.close();
            self.save_again();
        }
        if ui
            .add_enabled(
                can_save && saved && !saving,
                egui::Button::new("Save As…")
                    .shortcut_text(ui.ctx().format_shortcut(&save_as_shortcut())),
            )
            .on_hover_text(SAVE_AS_HINT)
            .on_disabled_hover_text(if saving {
                SAVING_HINT
            } else {
                "Save once first; Save… asks where"
            })
            .clicked()
        {
            ui.close();
            self.save_recording();
        }
        if ui
            .add_enabled(
                self.can_save_analysis(),
                egui::Button::new("Save Analysis Only…"),
            )
            .on_hover_text(ANALYSIS_ONLY_HINT)
            .on_disabled_hover_text(if self.analysis_save.is_some() {
                "The analysis is being kept now"
            } else if can_save {
                "Stop listening first: the whole take's sound is read again"
            } else {
                "Nothing to save yet: listen to a watch first"
            })
            .clicked()
        {
            ui.close();
            self.save_analysis_only();
        }
    }

    fn edit_menu(&mut self, ui: &mut egui::Ui) {
        let none = self.masked.is_empty();
        if ui
            .add_enabled(!none, egui::Button::new("Remove All Masks…"))
            .on_hover_text("Bring every masked stretch back into the readings, after asking")
            .on_disabled_hover_text("Nothing is masked")
            .clicked()
        {
            ui.close();
            self.remove_masks_asked = true;
        }
        ui.separator();
        if ui
            .add(
                egui::Button::new("Settings…")
                    .shortcut_text(ui.ctx().format_shortcut(&settings_shortcut())),
            )
            .on_hover_text(
                "How the app looks, the calibres and each input's kept clock, in a window \
                 of their own",
            )
            .clicked()
        {
            ui.close();
            self.open_settings(self.settings_last);
        }
    }

    fn view_menu(&mut self, ui: &mut egui::Ui) {
        for (view, name, hint) in VIEWS {
            if ui
                .add(
                    egui::Button::selectable(self.view == view, name)
                        .shortcut_text(ui.ctx().format_shortcut(&view_shortcut(view))),
                )
                .on_hover_text(hint)
                .clicked()
            {
                ui.close();
                self.view = view;
            }
        }
        ui.separator();
        // The panes are the Bench's: dimmed in Watches.
        let bench = self.view == View::Bench;
        ui.add_enabled_ui(bench, |ui| self.view_menu_panes(ui));
    }

    fn view_menu_panes(&mut self, ui: &mut egui::Ui) {
        let off = "The panes are on the Bench: View > Bench first";
        ui.checkbox(&mut self.show_readings, "Readings")
            .on_hover_text("The rate, amplitude and beat error cards")
            .on_disabled_hover_text(off);
        let mut pane_box = |ui: &mut egui::Ui, pane: Pane| {
            let mut on = pane_visible(&self.panes.tiles, pane);
            if ui
                .checkbox(&mut on, pane.title())
                .on_hover_text(pane.hint())
                .on_disabled_hover_text(off)
                .changed()
            {
                self.show_pane(pane, on);
            }
        };
        // In the icon bar's order: its panes, Analysis, then Tests.
        for pane in Pane::CORE {
            pane_box(ui, pane);
        }
        ui.menu_button("Analysis", |ui| {
            for pane in Pane::ANALYSIS {
                pane_box(ui, pane);
            }
        });
        ui.separator();
        for pane in Pane::TESTS {
            pane_box(ui, pane);
        }
        ui.separator();
        if ui
            .button("Reset Panes")
            .on_hover_text("Put the panes back as the app starts: the readings, the Paper Strip and the Tick Tock Profile, every other pane hidden")
            .on_disabled_hover_text(off)
            .clicked()
        {
            ui.close();
            self.show_readings = true;
            self.panes = default_layout(self.strip.horizontal);
            self.panes_dragged.clear();
        }
    }

    fn help_menu(&mut self, ui: &mut egui::Ui) {
        let checking = self.updates.checking();
        if ui
            .add_enabled(
                !checking,
                egui::Button::new(if checking {
                    "Checking for Updates…"
                } else {
                    "Check for Updates…"
                }),
            )
            .on_hover_text(
                "Ask GitHub whether a newer Timegrapher is published. Nothing downloads or \
                 installs itself.",
            )
            .clicked()
        {
            ui.close();
            self.updates.start(ui.ctx(), true);
        }
        ui.separator();
        if ui
            .button("Report a Problem…")
            .on_hover_text(
                "Make a zip of what went wrong, with up to five minutes of the sound, to \
                 attach to a GitHub issue. The app sends nothing itself.",
            )
            .clicked()
        {
            ui.close();
            self.open_report();
        }
        if cfg!(target_os = "linux") {
            ui.separator();
            if ui
                .button("Add to Applications Menu")
                .on_hover_text(
                    "Add Timegrapher to this computer's applications menu, with its icon, \
                     so the desktop shows the icon on the window and in the task bar. It \
                     writes one entry and one icon in your home folder.",
                )
                .clicked()
            {
                ui.close();
                match crate::desktop::install() {
                    Ok(p) => self.info_msg(format!(
                        "Added to the applications menu ({}). The icon shows from the next \
                         start.",
                        p.display()
                    )),
                    Err(e) => self.error(format!("Couldn't add it to the menu: {e}")),
                }
            }
        }
    }

    /// Open a recording, or a folder of segments, chosen in a dialog.
    pub(super) fn open_from_menu(&mut self, folder: bool) {
        let picked = if folder {
            self.open_folder_dialog()
        } else {
            self.open_file_dialog()
        };
        if let Some(p) = picked {
            self.input = Input::File;
            match self.open_ask_for(&p) {
                Some(a) => self.open_ask = Some(a),
                None => self.open_recording(&p, true),
            }
        }
    }

    /// The question Open asks for `path`, when there are watches to add it
    /// to and it isn't in them already. None means open it straight away
    /// (a recording that can't be read says why when opened).
    pub(super) fn open_ask_for(&self, path: &Path) -> Option<OpenAsk> {
        let Ok(Some(l)) = &self.library else {
            return None;
        };
        let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        if l.read_only.is_some() || canon(path).starts_with(canon(&l.root)) {
            return None;
        }
        let inner = take_folder(path).ok()?;
        // A folder's session file speaks for all of it, each recording
        // listed or not; a lone file's only through an entry listing it.
        let manifest = if inner.is_dir() && inner.join(MANIFEST).is_file() {
            let text = std::fs::read_to_string(inner.join(MANIFEST)).ok()?;
            let m: Manifest = toml::from_str(&text).ok()?;
            let entries = m.recordings.clone();
            Some((m, entries))
        } else {
            read_manifest(&inner).ok()?
        };
        let files = recordings_in(&inner).unwrap_or_default();
        let named =
            position_in_name(&inner).or_else(|| files.first().and_then(|f| position_in_name(f)));
        let dir = if inner.is_dir() {
            inner.clone()
        } else {
            inner.parent()?.to_path_buf()
        };
        // A lone file's folder speaks for it only through a session file
        // listing it.
        let started = (manifest.is_some() || inner.is_dir()) && started_in(&dir).is_some();
        let has = match &manifest {
            Some((m, entries)) => OpenHas {
                manifest: true,
                bph: m.bph,
                lift: m.lift,
                calibre: m.calibre.clone().filter(|c| !c.is_empty()),
                position: named.is_some() || entries.iter().all(|e| e.position.is_some()),
                started,
            },
            None => OpenHas {
                position: named.is_some(),
                started,
                ..Default::default()
            },
        };
        let watch = self
            .watch_open
            .clone()
            .filter(|id| l.watches.iter().any(|e| &e.watch.id == id));
        Some(OpenAsk {
            path: path.to_path_buf(),
            add: watch.is_some(),
            new_watch: l.watches.is_empty(),
            watch,
            new_name: String::new(),
            has,
            bph: None,
            lift: DEFAULT_LIFT_DEG,
            lift_set: false,
            position: named,
            started: started_guess(&files)
                .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default(),
            error: None,
            keeping: false,
        })
    }

    /// What adding `a` to its watch asks to be filled in: what neither the
    /// recording's files nor the watch and its calibre say.
    pub(super) fn open_need(&self, a: &OpenAsk) -> OpenNeed {
        let w = match (&self.library, &a.watch) {
            (Ok(Some(l)), Some(id)) if !a.new_watch => l
                .watches
                .iter()
                .find(|e| &e.watch.id == id)
                .map(|e| &e.watch),
            _ => None,
        };
        let calibre = a
            .has
            .calibre
            .clone()
            .or_else(|| w.and_then(|w| w.calibre.clone()))
            .filter(|c| !c.is_empty());
        let of_calibre = |f: &dyn Fn(&str) -> bool| calibre.as_deref().is_some_and(f);
        OpenNeed {
            bph: a.has.bph.is_none()
                && w.and_then(|w| w.bph).is_none()
                && !of_calibre(&|c| self.cal.bph(c).is_some()),
            lift: a.has.lift.is_none()
                && w.and_then(|w| w.lift).is_none()
                && !of_calibre(&|c| self.cal.lift(c).is_some()),
            position: !a.has.position,
            started: !a.has.started,
        }
    }

    /// Of what `need` says is missing, what the window asks for: all of
    /// it, except, when keeping the recording on show, what the take on
    /// show already says. It is still written with the take.
    pub(super) fn open_asks(a: &OpenAsk, need: OpenNeed) -> OpenNeed {
        if !a.keeping {
            return need;
        }
        OpenNeed {
            bph: need.bph && a.bph.is_none(),
            lift: need.lift && !a.lift_set,
            position: need.position && a.position.is_none(),
            started: need.started,
        }
    }
}

/// A shortcut of the platform's command key (Ctrl, or ⌘ on a Mac) and
/// `key`.
fn command(key: egui::Key) -> egui::KeyboardShortcut {
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, key)
}
