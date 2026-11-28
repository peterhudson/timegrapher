//! The Watches pane: the watch list, a watch's page and takes, keeping takes into a watch, and the watch form.

use super::*;

/// What Keep Analysis Only reads a take from: its sound files, their
/// length in seconds, the companion files, the settings and the shape
/// stretches.
impl TimegrapherApp {
    /// The Watches view: where the sound comes from and what to do with it,
    /// then the watch itself. Before anything is open it stands beside
    /// the ways in, so the microphone and the watch can be set first.
    pub(super) fn watch_pane(&mut self, ui: &mut egui::Ui) {
        let wide = ui.available_width() >= WATCHES_SIDE_BY_SIDE;
        // The toolbar, drawn first, goes by this: drawn again at once when
        // it changes.
        if wide != self.watches_view.watches_wide {
            ui.ctx().request_repaint();
        }
        self.watches_view.watches_wide = wide;
        let scroll = |ui: &mut egui::Ui, id: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
            crate::theme::scroll(egui::ScrollArea::vertical())
                .id_salt(id)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Clear of the scroll bar.
                    ui.set_max_width(ui.available_width() - 8.0);
                    ui.spacing_mut().item_spacing.y = 8.0;
                    add(ui);
                });
        };
        if !wide {
            // One at a time: the list, or the watch with ‹ All Watches.
            let one = !self.list_shown()
                && (self.open_watch().is_some() || self.watches_view.unsorted_open);
            scroll(ui, "watch pane", &mut |ui| {
                if one {
                    self.watch_side(ui);
                } else {
                    self.watch_list_card(ui);
                }
            });
            return;
        }
        // The list beside the watch; only the watch's side scrolls with
        // it, and it stops at about 1100 pt wide.
        let full = ui.available_rect_before_wrap();
        let list = egui::Rect::from_min_size(full.min, Vec2::new(WATCH_LIST_W, full.height()));
        let side = egui::Rect::from_min_max(
            egui::pos2(list.right() + 17.0, full.top()),
            egui::pos2(
                (list.right() + 17.0 + WATCH_SIDE_MAX).min(full.right()),
                full.bottom(),
            ),
        );
        let pal = theme::pal(ui);
        ui.painter().vline(
            list.right() + 8.0,
            full.y_range(),
            egui::Stroke::new(1.0_f32, pal.separator),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
            scroll(ui, "watch list", &mut |ui| self.watch_list_card(ui));
        });
        ui.scope_builder(egui::UiBuilder::new().max_rect(side), |ui| {
            scroll(ui, "watch side", &mut |ui| self.watch_side(ui));
        });
        ui.allocate_rect(full, egui::Sense::hover());
    }

    /// Why a new take can't start or a take be opened from the Watches
    /// view: the Bench holds one take at a time.
    pub(super) fn watches_busy(&self) -> Option<&'static str> {
        (self.running() || self.bench.recording.batch.is_some())
            .then_some("Stop the take on the Bench first.")
    }

    /// The picked watch, or Unsorted Takes: its header, New Take, its
    /// takes, then its details and photos.
    pub(super) fn watch_side(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let busy = self.watches_busy();
        if let Some(e) = self.open_watch().cloned() {
            let writable = matches!(&self.library, Ok(Some(l)) if l.read_only.is_none());
            let locked = (!writable).then_some("This copy of the app can only read your watches");
            match watches::header(ui, &e, &self.cal, &mut self.watches_view.thumbs, locked) {
                Some(watches::HeaderAction::Edit) => {
                    self.watches_view.watch_form = Some(watches::Form {
                        watch: e.watch.clone(),
                        new: false,
                        error: None,
                    });
                }
                Some(watches::HeaderAction::AddPhotos) => self.add_photos(),
                None => {}
            }
            watches::title_row(ui, "New Take", None, |ui| {
                if let Some(why) = busy {
                    ui.label(RichText::new(why).small().color(pal.text_secondary));
                }
            });
            // The position card named by its title at this width.
            let hints = NEW_TAKES
                .map(|(_, _, hint, _)| hint.replace("{card}", self.bench.test.position_card_title));
            let names: Vec<(&str, &str, &str)> = NEW_TAKES
                .iter()
                .zip(&hints)
                .map(|(t, hint)| (t.1, hint.as_str(), t.3))
                .collect();
            if let Some(i) = watches::new_take_buttons(ui, &names, busy) {
                self.watches_view.view = View::Bench;
                self.new_take(NEW_TAKES[i].0, false);
            }
            self.watch_takes_section(ui, &e, busy);
            watches::title_row(ui, "Details", None, |_| {});
            watches::details_panel(ui, &e.watch);
            watches::title_row(ui, "Photos", None, |_| {});
            if e.photos.is_empty() {
                watches::panel(ui, |ui| {
                    ui.label(
                        RichText::new(
                            "None yet. Add Photos… puts them here, as a strip of thumbnails.",
                        )
                        .small()
                        .color(pal.text_secondary),
                    );
                });
            } else {
                self.watch_photos(ui, &e);
            }
        } else if let Some(e) = self.open_group() {
            watches::unsorted_details(ui);
            self.watch_takes_section(ui, &e, busy);
        } else {
            ui.add_space(24.0);
            ui.label(
                RichText::new("Pick a watch in the list to see its takes and start a new one.")
                    .color(pal.text_secondary),
            );
        }
    }

    /// The watch picked in the Watches view, as the library last read it.
    pub(super) fn open_watch(&self) -> Option<&library::WatchEntry> {
        let id = self.watches_view.watch_open.as_ref()?;
        match &self.library {
            Ok(Some(l)) => l.watches.iter().find(|e| &e.watch.id == id),
            _ => None,
        }
    }

    /// The watch picked, or Unsorted when the pane shows it.
    pub(super) fn open_group(&self) -> Option<library::WatchEntry> {
        match &self.library {
            Ok(Some(l))
                if self.watches_view.unsorted_open && self.watches_view.watch_open.is_none() =>
            {
                Some(l.unsorted.clone())
            }
            _ => self.open_watch().cloned(),
        }
    }

    /// Where Save… keeps a take: the `takes/` of the watch picked, or
    /// Unsorted's after Quick Listen or with Unsorted shown, where the
    /// library can be written.
    pub(super) fn watch_takes_dir(&self) -> Option<PathBuf> {
        match &self.library {
            Ok(Some(l)) if l.read_only.is_none() => match self.open_watch() {
                Some(e) => Some(e.dir.join(library::TAKES)),
                None if self.bench.recording.quick_listen || self.watches_view.unsorted_open => {
                    l.unsorted_dir().ok()
                }
                None => None,
            },
            _ => None,
        }
    }

    /// The library's watches, or what stands in for them.
    fn watch_list_card(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let shows = self.list_shows().map(str::to_string);
        let mut show_unsorted = false;
        let writing = self.take_writing();
        let picked = match &self.library {
            Ok(Some(l)) => {
                let shown = watches::shown(
                    l,
                    &self.watches_view.watch_search,
                    self.watches_view.watch_sort,
                );
                // Unsorted Takes first, any word about the list under it.
                let tiled = shown.iter().any(|e| !e.photos.is_empty());
                let marked = self.watches_view.watch_open.as_deref();
                if watches::unsorted_row(
                    ui,
                    &l.unsorted,
                    tiled,
                    marked.is_none() && self.watches_view.unsorted_open,
                    writing.as_deref(),
                ) {
                    show_unsorted = true;
                }
                if l.watches.is_empty() {
                    ui.label(
                        RichText::new(
                            "No watches yet. New Watch… adds one, and the takes of it you \
                             save are kept with it.",
                        )
                        .color(pal.text_secondary),
                    );
                } else if shown.is_empty() {
                    ui.label(
                        RichText::new("No watch matches the search.").color(pal.text_secondary),
                    );
                }
                let picked = watches::list(
                    ui,
                    &shown,
                    marked,
                    &mut self.watches_view.thumbs,
                    shows.as_deref(),
                    &self.watches_view.watch_search,
                    writing.as_deref(),
                );
                // How many the search hid.
                if !self.watches_view.watch_search.trim().is_empty() && !l.watches.is_empty() {
                    ui.label(
                        RichText::new(format!(
                            "{} of {} {}",
                            shown.len(),
                            l.watches.len(),
                            if l.watches.len() == 1 {
                                "watch"
                            } else {
                                "watches"
                            }
                        ))
                        .small()
                        .color(pal.text_secondary),
                    );
                }
                if let Some(why) = &l.read_only {
                    ui.label(
                        RichText::new(format!("{why}."))
                            .small()
                            .color(pal.text_secondary),
                    );
                }
                picked
            }
            Ok(None) => {
                ui.label(
                    RichText::new(
                        "No watches yet. New Watch… adds one, and the takes of it you save \
                         are kept with it.",
                    )
                    .color(pal.text_secondary),
                );
                None
            }
            Err(e) => {
                theme::alert_label(
                    ui,
                    theme::Alert::Bad,
                    &format!("Your watches can't be read: {e}"),
                    false,
                );
                None
            }
        };
        if show_unsorted {
            self.pick_unsorted();
        }
        if let Some(id) = picked {
            self.list_pick(id);
        }
    }

    /// A watch picked in the Watches view's list, or just added there.
    /// Over a Quick Listen still listening it does what the Watch line
    /// does: the Quick Listen is kept in the watch and listening goes on,
    /// with no question, since no sound is lost (#2107). Over a watch's
    /// take still listening, another is refused; its own shows its page.
    pub(super) fn list_pick(&mut self, id: String) {
        if self.running() && self.bench.recording.quick_listen {
            return self.line_pick(Some(id), false);
        }
        if self.running() && self.watches_view.watch_open.as_deref() == Some(id.as_str()) {
            self.watches_view.list_for = None;
            return;
        }
        let again = id.clone();
        if self.behind_save(move |a| a.list_pick(again)) {
            return;
        }
        self.pick_watch(&id);
    }

    /// The picked watch's photos, as a strip.
    fn watch_photos(&mut self, ui: &mut egui::Ui, e: &library::WatchEntry) {
        let writable = matches!(&self.library, Ok(Some(l)) if l.read_only.is_none());
        match watches::photo_strip(ui, e, &mut self.watches_view.thumbs, writable) {
            Some(watches::PhotoAction::Open(p)) => {
                if let Err(err) = reveal(&p) {
                    self.error(format!("Couldn't open {}: {err}", p.display()));
                }
            }
            Some(watches::PhotoAction::Show(p)) => {
                let dir = p.parent().unwrap_or(&p).to_path_buf();
                if let Err(err) = reveal(&dir) {
                    self.error(format!("Couldn't open {}: {err}", dir.display()));
                }
            }
            Some(watches::PhotoAction::Delete(photo)) => {
                self.watches_view.take_ask = Some(watches::Ask::DeletePhoto { photo });
            }
            None => {}
        }
    }

    /// Add Photos…: copy the photos picked into the open watch's.
    pub(super) fn add_photos(&mut self) {
        let Some(id) = self.open_watch().map(|e| e.watch.id.clone()) else {
            return;
        };
        let d = rfd::FileDialog::new()
            .set_title("Add Photos of the Watch")
            .add_filter(
                "Photos",
                &[
                    "jpg", "jpeg", "png", "heic", "heif", "tif", "tiff", "JPG", "JPEG", "PNG",
                    "HEIC", "HEIF", "TIF", "TIFF",
                ],
            );
        self.pick(Picker::files(d, move |a, files| {
            a.add_photo_files(&id, &files)
        }));
    }

    /// Copy `files` into the photos of the watch `id`, saying how many.
    fn add_photo_files(&mut self, id: &str, files: &[PathBuf]) {
        let Ok(Some(lib)) = &mut self.library else {
            return;
        };
        match lib.add_photos(id, files) {
            Ok(added) => self.info_msg(format!(
                "Added {}.",
                if added.len() == 1 {
                    "1 photo".to_string()
                } else {
                    format!("{} photos", added.len())
                }
            )),
            Err(e) => self.error(format!("The photos couldn't all be added: {e}")),
        }
    }

    /// The watch's takes: the title row with Open and "…", then the table.
    fn watch_takes_section(
        &mut self,
        ui: &mut egui::Ui,
        e: &library::WatchEntry,
        busy: Option<&'static str>,
    ) {
        self.take_add_row(ui, &e.watch.id);
        let (others, writable) = match &self.library {
            Ok(Some(l)) => (
                l.watches
                    .iter()
                    .filter(|w| w.watch.id != e.watch.id)
                    .map(|w| (w.watch.id.clone(), w.watch.name.clone()))
                    .collect(),
                l.read_only.is_none(),
            ),
            _ => (Vec::new(), false),
        };
        self.want_take_readings(e, ui.ctx());
        let keeping = self.bench.keeping.take_keep.as_ref().map(|(t, job)| {
            let read = f64::from_bits(job.progress.load(std::sync::atomic::Ordering::Relaxed));
            (
                t.as_path(),
                (read / (2.0 * job.total_s).max(1e-9)).clamp(0.0, 1.0) as f32,
            )
        });
        if keeping.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        let on_show = self.take_on_show();
        let held = self.take_held();
        let writing = self.take_writing();
        let picked = self.bench.keeping.take_picked.clone();
        let cx = watches::TakesContext {
            others: &others,
            writable,
            keeping,
            unsorted: e.watch.id.is_empty(),
            picked: picked.as_deref(),
            on_show: on_show.as_deref(),
            held: held.as_deref(),
            writing: writing.as_deref(),
            readings: &self.watches_view.take_readings,
        };
        let mut act = None;
        watches::title_row(ui, "Takes", Some(e.takes.len()), |ui| {
            act = watches::takes_buttons(ui, e, &cx, busy);
        });
        if let Some(a) = watches::takes_table(ui, e, &cx, busy) {
            act = Some(a);
        }
        self.take_action(act);
    }

    /// Do what was asked of a take in the Watches view's list.
    pub(super) fn take_action(&mut self, act: Option<watches::TakeAction>) {
        match act {
            Some(watches::TakeAction::Pick(dir)) => self.bench.keeping.take_picked = Some(dir),
            // The take on show is not read again.
            Some(watches::TakeAction::Open(dir)) if self.take_on_show().as_ref() == Some(&dir) => {
                self.bench.keeping.take_picked = Some(dir);
                self.watches_view.view = View::Bench;
            }
            Some(watches::TakeAction::Open(dir)) => {
                self.watches_view.view = View::Bench;
                self.bench.keeping.take_picked = Some(dir.clone());
                self.bench.recording.input = Input::File;
                self.bench.recording.file_path = dir.display().to_string();
                self.open_recording(&dir, true);
            }
            Some(watches::TakeAction::Show(dir)) => {
                if let Err(err) = reveal(&dir) {
                    self.error(format!("Couldn't open {}: {err}", dir.display()));
                }
            }
            Some(watches::TakeAction::SetWind(take, wind_h)) => self.set_take_wind(&take, wind_h),
            Some(watches::TakeAction::Delete(take)) => {
                let bytes = folder_size(&take);
                self.watches_view.take_ask = Some(watches::Ask::Delete { take, bytes });
            }
            Some(watches::TakeAction::KeepAnalysis(take)) => {
                let sound = recordings_in(&take)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|f| std::fs::metadata(f).ok())
                    .map(|m| m.len())
                    .sum();
                let bytes = folder_size(&take);
                self.watches_view.take_ask = Some(watches::Ask::Keep { take, bytes, sound });
            }
            Some(watches::TakeAction::MoveTo(take, id)) => self.move_take(&take, &id),
            Some(watches::TakeAction::CancelKeep) => self.bench.keeping.take_keep = None,
            None => {}
        }
    }

    /// Move a take into another watch (by its id): one in Unsorted is
    /// filed under it.
    pub(super) fn move_take(&mut self, take: &Path, id: &str) {
        if let Some(why) = self.take_busy(take, false) {
            return self.info_msg(why);
        }
        let mut filed = false;
        let moved = match &mut self.library {
            Ok(Some(l)) => {
                filed = l.is_unsorted(take);
                l.move_take(take, id)
            }
            _ => Err("There is no Watches folder".into()),
        };
        match moved {
            Ok(to) => {
                self.take_moved(take, &to);
                let watch = self
                    .library
                    .as_ref()
                    .ok()
                    .and_then(|l| l.as_ref())
                    .and_then(|l| l.watch_of(&to))
                    .map(|w| w.watch.clone());
                let name = watch.as_ref().map(|w| w.name.clone()).unwrap_or_default();
                let read_again =
                    match watch.map(|w| library::file_take_under(&to, &w, &self.cal.user)) {
                        Some(Err(e)) => {
                            return self.error(format!(
                                "\"{}\" is in {name} now, but its session file still says what \
                             it was recorded with: {e}",
                                file_name(&to)
                            ))
                        }
                        Some(Ok(changed)) => changed,
                        None => false,
                    };
                // The take on show is read again with its new watch's
                // settings: opened, or the session's own, stopped (#1978).
                let shown = self.take_on_show().as_deref() == Some(to.as_path())
                    || (!self.running() && self.session.saved_to() == Some(to.as_path()));
                if read_again && shown {
                    // Opened as Open opens it, so the status line names it
                    // (Walter on #1999).
                    self.bench.recording.input = Input::File;
                    self.bench.recording.file_path = to.display().to_string();
                    self.open_recording(&to, false);
                }
                if filed {
                    self.info_msg(format!("Filed \"{}\" under {name}.", file_name(&to)));
                } else {
                    self.info_msg(format!("Moved \"{}\" to {name}.", file_name(&to)));
                }
            }
            Err(err) => self.error(format!("The take couldn't be moved: {err}")),
        }
    }

    /// Whether a recording opened from disk is on show: the one test of
    /// it (#2391).
    pub(super) fn opened_recording(&self) -> bool {
        self.bench.recording.input == Input::File && !self.bench.keeping.opened_path.is_empty()
    }

    /// The recording opened, a lone file or a take, when the library can
    /// take it: outside the library by core's one import-path guard
    /// ([`library::not_already_in`], links followed).
    pub(super) fn addable_take(&self) -> Option<PathBuf> {
        if !self.opened_recording() {
            return None;
        }
        let p = PathBuf::from(&self.bench.keeping.opened_path);
        let root = match &self.library {
            Ok(Some(l)) if l.read_only.is_none() => l.root.clone(),
            // Made with the first watch.
            Ok(None) => self.library_root()?,
            _ => return None,
        };
        library::not_already_in(&p, &root).is_ok().then_some(p)
    }

    /// Keep what is on show in the watch `id`: the unsaved Quick Listen
    /// take is saved into it, or the recording opened is copied in
    /// (asking for what neither it nor the watch says) and opened whole
    /// from there.
    pub(super) fn keep_into(&mut self, id: &str) {
        if let Some(from) = self.addable_take() {
            // The pane shows the watch it goes into, until a Cancel.
            self.bench.keeping.keep_back = Some(self.watches_view.picked());
            self.watches_view.watch_open = Some(id.to_string());
            self.watches_view.unsorted_open = false;
            // A take folder with its session file goes in whole, copied
            // or moved.
            let dir = if from.is_dir() {
                from.clone()
            } else {
                from.parent().map(Path::to_path_buf).unwrap_or_default()
            };
            // One part of a take of several opened alone goes in alone,
            // with what is on show.
            let one_of_several = from.is_file() && recordings_in(&dir).is_ok_and(|r| r.len() > 1);
            if !one_of_several && library::is_take_folder(&dir) {
                self.watches_view.take_ask = Some(watches::Ask::Add {
                    from: dir,
                    watch: Some(id.to_string()),
                    keep_original: true,
                });
                return;
            }
            let Some(mut a) = self.open_ask_for(&from) else {
                return self.error(format!(
                    "\"{}\" can't be added to a watch.",
                    file_name(&from)
                ));
            };
            a.add = true;
            a.keeping = true;
            a.watch = Some(id.to_string());
            a.new_watch = false;
            // What is on show is not asked again: its position, its beat
            // rate (set or heard) and its lift angle.
            a.position = a.position.or(Some(self.bench.take.position));
            a.bph = a.bph.or(self.bench.take.bph).or(self.bph_heard());
            a.lift = self.bench.take.lift_deg;
            a.lift_set = true;
            let need = self.open_need(&a);
            if Self::open_asks(&a, need) == OpenNeed::default() {
                self.answer_open_ask(a, need);
            } else {
                self.bench.keeping.open_ask = Some(a);
            }
        } else {
            self.keep_in_watch(id);
        }
    }

    /// Why `take` can't be moved or deleted now (or, with `keep`, kept
    /// as its analysis), if it can't: it is being recorded or read, or
    /// its analysis is being kept. A stopped session saved into it is let
    /// go of first, by [`Self::answer_take_ask`].
    pub(super) fn take_busy(&self, take: &Path, keep: bool) -> Option<String> {
        let inside = |p: &Path| p.starts_with(take);
        if self.running() && self.session.saved_to().is_some_and(inside) {
            return Some("This take is still being recorded. Press Stop first.".into());
        }
        if self.running() && inside(Path::new(&self.bench.keeping.opened_path)) {
            return Some("This take is being played. Press Stop first.".into());
        }
        if self
            .bench
            .keeping
            .take_add
            .as_ref()
            .is_some_and(|a| a.from().starts_with(take))
        {
            return Some(self.adding_already());
        }
        if self
            .bench
            .keeping
            .take_keep
            .as_ref()
            .is_some_and(|(t, _)| t == take)
        {
            return Some(
                "This take's sound is already being removed. Wait for it to finish, or press \
                 Cancel."
                    .into(),
            );
        }
        if self.watches_view.take_readings.being_read(take) {
            return Some(
                "This take is being read for the Watches list. Wait for its figures, then try \
                 again."
                    .into(),
            );
        }
        if !keep
            && self.bench.recording.batch.is_some()
            && inside(Path::new(&self.bench.keeping.opened_path))
        {
            return Some(
                "This take is still being analysed. Wait for it to finish, or press Cancel.".into(),
            );
        }
        None
    }

    /// A take moved: the recording opened from it follows, and so does
    /// Save Again.
    pub(super) fn take_moved(&mut self, from: &Path, to: &Path) {
        // The take on show: opened, or the one the session saved (a Quick
        // Listen kept, then filed: #1978).
        let opened = Path::new(&self.bench.keeping.opened_path).starts_with(from)
            || self.session.saved_to().is_some_and(|s| s.starts_with(from));
        self.paths_moved(from, to);
        if opened {
            // The take on show is the watch's it went to now: the Bench's
            // card and the status line name that one (#1280).
            let went = match &self.library {
                Ok(Some(l)) => l
                    .watch_of(to)
                    .map(|e| (e.watch.id.clone(), e.watch.name.clone())),
                _ => None,
            };
            if let Some((id, name)) = went {
                self.watches_view.watch_open = Some(id);
                self.watches_view.unsorted_open = false;
                self.bench.take.watch = name;
            }
        }
    }

    /// A folder moved, from `from` to `to`: a take, or the whole Watches
    /// folder (#1646). Every path inside it follows: the take saved into
    /// it and its next Save Again, every save noted, the recording opened
    /// and the path typed.
    pub(super) fn paths_moved(&mut self, from: &Path, to: &Path) {
        let saved = self.session.saved_to().map(Path::to_path_buf);
        if let Some((take, rest)) = saved
            .as_ref()
            .and_then(|t| Some((t, t.strip_prefix(from).ok()?)))
        {
            // Its next save goes into the take where it is now.
            let now = to.join(rest);
            if let Some(r) = self.session.recorder.as_mut() {
                r.copy_moved(take, &now);
            }
        }
        self.session.saved_moved(from, to);
        if let Ok(rest) = Path::new(&self.bench.keeping.opened_path).strip_prefix(from) {
            self.bench.keeping.opened_path = to.join(rest).display().to_string();
        }
        if let Ok(rest) = Path::new(self.bench.recording.file_path.trim()).strip_prefix(from) {
            self.bench.recording.file_path = to.join(rest).display().to_string();
        }
    }

    /// The question about a take, while it is open, and what follows a yes.
    pub(super) fn take_ask_dialog(&mut self, ctx: &egui::Context) {
        let Some(ask) = self.watches_view.take_ask.as_mut() else {
            return;
        };
        let watches: Vec<(String, String)> = match &self.library {
            Ok(Some(l)) => l
                .watches
                .iter()
                .map(|w| (w.watch.id.clone(), w.watch.name.clone()))
                .collect(),
            _ => Vec::new(),
        };
        match watches::ask_dialog(ctx, ask, &watches) {
            Some(true) => {
                if let Some(ask) = self.watches_view.take_ask.take() {
                    self.answer_take_ask(ask);
                }
            }
            Some(false) => {
                self.watches_view.take_ask = None;
                self.keep_cancelled();
            }
            None => {}
        }
    }

    /// Keep in a Watch…'s question cancelled: the watch picked before it
    /// is picked again.
    pub(super) fn keep_cancelled(&mut self) {
        if let Some(p) = self.bench.keeping.keep_back.take() {
            self.watches_view.pick_again(p);
        }
    }

    pub(super) fn answer_take_ask(&mut self, ask: watches::Ask) {
        self.bench.keeping.keep_back = None;
        // The session just saved into the take, stopped, lets go of it:
        // nothing more is saved there, and what is on show goes.
        let changed = match &ask {
            watches::Ask::Delete { take, .. } | watches::Ask::Keep { take, .. } => Some(take),
            _ => None,
        };
        let saved_into = changed
            .is_some_and(|take| self.session.saved_to().is_some_and(|p| p.starts_with(take)));
        if saved_into && !self.running() {
            self.new_session();
        }
        let busy = match &ask {
            watches::Ask::Delete { take, .. } => self.take_busy(take, false),
            watches::Ask::DeletePhoto { .. } => None,
            watches::Ask::Keep { take, .. } => self.take_busy(take, true),
            watches::Ask::Add {
                from,
                keep_original,
                ..
            } => (!keep_original)
                .then(|| self.take_busy(from, false))
                .flatten(),
        };
        if let Some(why) = busy {
            return self.info_msg(why);
        }
        let Ok(Some(lib)) = &mut self.library else {
            return self.error("There is no Watches folder.".into());
        };
        match ask {
            watches::Ask::Delete { take, .. } => match lib.delete_take(&take) {
                Ok(()) => {
                    // A take on show that is deleted leaves the window
                    // empty, as at the start, rather than showing sound
                    // that is gone: set for the next take of the watch
                    // picked, as Listen sets it (#1981).
                    if !self.running()
                        && Path::new(&self.bench.keeping.opened_path).starts_with(&take)
                    {
                        self.take_setup();
                    }
                    self.info_msg(format!("Deleted \"{}\".", file_name(&take)))
                }
                Err(e) => self.error(format!("The take couldn't be deleted: {e}")),
            },
            watches::Ask::Keep { take, .. } => self.start_take_keep(take),
            watches::Ask::DeletePhoto { photo } => match lib.delete_photo(&photo) {
                Ok(()) => {
                    self.watches_view.thumbs.forget(&photo);
                    self.info_msg(format!("Deleted \"{}\".", file_name(&photo)));
                }
                Err(e) => self.error(format!("The photo couldn't be deleted: {e}")),
            },
            watches::Ask::Add {
                from,
                watch: Some(id),
                keep_original,
            } => self.start_take_add(from, id, keep_original),
            watches::Ask::Add { watch: None, .. } => {}
        }
    }

    /// How Keep Analysis Only reads a take with session file `m`: as core
    /// reads any take, its session file's beat rate, lift and escape teeth,
    /// else its calibre's, and whether that lift was supplied (#685, #1324).
    /// Core's [`timegrapher_core::analysis_take::plan_keep`] reads it so;
    /// the tests ask it here.
    #[cfg(test)]
    pub(super) fn keep_config(&self, m: &Manifest) -> timegrapher_core::stream::StreamConfig {
        timegrapher_core::analysis_take::keep_settings(m, &self.cal.user)
    }

    /// Read a saved take's sound again for its analysis, in the
    /// background, writing it beside the take; [`Self::poll_take_keep`]
    /// then puts it in the take's place. Only a take whose session file
    /// names the whole folder in each entry, as the app's own takes do.
    pub(super) fn start_take_keep(&mut self, take: PathBuf) {
        if self.bench.keeping.take_keep.is_some() {
            return;
        }
        let plan = match timegrapher_core::analysis_take::plan_keep(&take, &self.cal.user) {
            Ok(p) => p,
            Err(e) => {
                return self.error(format!(
                    "The sound couldn't be removed from this take, so nothing was changed: {e}"
                ))
            }
        };
        let total_s = plan.total_s;
        let software = report::software();
        let (tx, rx) = std::sync::mpsc::channel();
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (p2, c2) = (progress.clone(), cancel.clone());
        let at = take.clone();
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                timegrapher_core::analysis_take::write_kept(&at, &plan, software, |s| {
                    p2.store(s.to_bits(), std::sync::atomic::Ordering::Relaxed);
                    if c2.load(std::sync::atomic::Ordering::Relaxed) {
                        std::panic::resume_unwind(Box::new(Cancelled));
                    }
                })
            }));
            match r {
                Ok(r) => {
                    if let Err(std::sync::mpsc::SendError(Ok(dir))) = tx.send(r) {
                        let _ = std::fs::remove_dir_all(dir);
                    }
                }
                Err(e) if e.is::<Cancelled>() => {}
                Err(e) => std::panic::resume_unwind(e),
            }
        });
        self.bench.keeping.take_keep = Some((
            take,
            AnalysisSave {
                rx,
                progress,
                cancel,
                total_s,
            },
        ));
    }

    /// The watch the take is of, as the Calibre card's title and the
    /// status bar name it: the one open in Watches, else the one the take
    /// names itself (its session file's), which isn't in Watches (false);
    /// none for No Watch.
    pub(super) fn named_watch(&self) -> Option<(String, bool)> {
        self.open_watch()
            .map(|e| e.watch.name.clone())
            .filter(|n| !n.is_empty())
            .map(|n| (n, true))
            .or_else(|| {
                (!self.bench.take.watch.is_empty()).then(|| (self.bench.take.watch.clone(), false))
            })
    }

    /// The kept analysis, once written, put in its take's place.
    pub(super) fn poll_take_keep(&mut self) {
        let Some((_, job)) = &self.bench.keeping.take_keep else {
            return;
        };
        let done = match job.rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("it stopped part way".into()),
        };
        // Its outcome taken, the job has nothing left to clean up.
        let Some((take, _)) = self.bench.keeping.take_keep.take() else {
            return;
        };
        let before = folder_size(&take);
        let placed = match (done, &mut self.library) {
            (Ok(kept), Ok(Some(l))) => l.put_kept(&take, &kept),
            (Ok(kept), _) => {
                let _ = std::fs::remove_dir_all(&kept);
                Err("the Watches folder is no longer open".into())
            }
            (Err(e), _) => Err(e),
        };
        match placed {
            Ok(None) => self.info_msg(format!(
                "Removed the sound from \"{}\" and kept its analysis. It now uses {}, down \
                 from {}.",
                file_name(&take),
                size_text(folder_size(&take)),
                size_text(before)
            )),
            Ok(Some(why)) => self.error(why),
            Err(e) => self.error(format!(
                "The sound couldn't be removed from this take, so it still has its sound: {e}"
            )),
        }
    }

    /// Pick a watch: the pane shows it, and a new take starts with its
    /// name, calibre, lift angle and beat rate. Refused while listening,
    /// since the readings so far are of the watch already set.
    pub(super) fn pick_watch(&mut self, id: &str) {
        if self.pick_refused("another watch") {
            return;
        }
        // A take on show from another watch or Unsorted Takes is put away,
        // as Unsorted Takes and the Watch line do: the Bench's card and the
        // status line name the watch picked, so never over a take that
        // is another's (#1303). With marks not yet saved, they are asked
        // about first, as the Watch line asks, and until then the card
        // still names the take's own watch (Hugh). A recording opened from
        // outside the Watches folder stays, as it was measured, to be kept
        // in the watch picked (#1966).
        let elsewhere = self.take_on_show().is_some_and(|t| {
            let of = match &self.library {
                Ok(Some(l)) => l.watch_of(&t).map(|e| e.watch.id.clone()),
                _ => None,
            };
            of.as_deref() != Some(id) && !self.shown_outside_watches()
        });
        if elsewhere && self.needs_save_confirmation() {
            self.confirm = Some(Confirm::PickWatch(Some(id.to_string())));
            return;
        }
        self.watches_view.watch_open = Some(id.to_string());
        self.forget_quick_wind();
        self.watches_view.unsorted_open = false;
        // The watch picked shows, not the list left over from before.
        self.watches_view.list_for = None;
        self.watches_view.quick_defaults = false;
        let Some(w) = self.open_watch().map(|e| e.watch.clone()) else {
            self.watches_view.watch_open = None;
            return;
        };
        if elsewhere {
            self.new_session();
        }
        // Readings on show keep the settings they were measured with; the
        // watch's apply to its next take.
        if self.bench.recording.live.is_some() {
            return;
        }
        self.bench.recording.quick_listen = false;
        self.apply_watch(&w);
    }

    /// Whether the take on show is a recording opened from outside the
    /// Watches folder: in no watch and not in Unsorted Takes. Picking or
    /// adding a watch leaves it as it was measured (#1966).
    pub(super) fn shown_outside_watches(&self) -> bool {
        let Some(t) = self.take_on_show() else {
            return false;
        };
        match &self.library {
            Ok(Some(l)) => l.watch_of(&t).is_none() && !l.is_unsorted(&t),
            _ => true,
        }
    }

    /// Whether the pane shows the list over what is picked.
    pub(super) fn list_shown(&self) -> bool {
        self.watches_view
            .list_for
            .as_ref()
            .is_some_and(|p| self.watches_view.is_picked(p))
    }

    /// Whether picking `what` is refused, while listening: the readings
    /// so far are of the watch already set, and the status bar says so.
    /// Once a pick goes through, that refusal no longer stands (Walter).
    fn pick_refused(&mut self, what: &str) -> bool {
        let refusal = |what: &str| format!("Stop listening before picking {what}.");
        if self.running() {
            self.info_msg(refusal(what));
            return true;
        }
        let stale = ["another watch", "Unsorted Takes"].map(refusal);
        if self
            .message
            .as_ref()
            .is_some_and(|(m, _)| stale.contains(m))
        {
            self.message = None;
        }
        false
    }

    /// Unsorted Takes: Quick Listen again, with no watch, its takes on
    /// show and the settings back to the defaults: no calibre, the
    /// default lift angle and the beat rate found from the beats.
    pub(super) fn pick_unsorted(&mut self) {
        if self.pick_refused("Unsorted Takes") {
            return;
        }
        // A watch's take on show, all of it saved, is put away: Unsorted
        // Takes starts again from Quick Listen's settings.
        let watch_take = self.watches_view.watch_open.is_some()
            && self.bench.recording.live.is_some()
            && !self.needs_save_confirmation()
            && self.session.unsaved_s() < 0.5;
        self.watches_view.watch_open = None;
        self.watches_view.unsorted_open = true;
        // Its takes show, not the list left over from before.
        self.watches_view.list_for = None;
        self.bench.recording.quick_listen = false;
        if watch_take {
            self.new_session();
        }
        // Other readings on show keep the settings they were measured with.
        if self.bench.recording.live.is_some() {
            self.watches_view.quick_defaults = true;
        } else {
            self.quick_listen_defaults();
        }
    }

    /// A beat rate picked, or Auto. A take on show is read again at it,
    /// with the settings and masks as they are now, since its readings
    /// were worked out at the old one; one opening is read at it anyway.
    pub(super) fn change_bph(&mut self, bph: Option<u32>) {
        let shown = (!self.running() && !self.bench.recording.opening)
            .then(|| self.take_on_show())
            .flatten();
        if !self.set_live_bph(bph) || shown.is_some() {
            self.bench.recording.anchor = None;
            self.clear_trends();
        }
        if let Some(take) = shown {
            let masked = self.bench.take.masked.clone();
            self.bench.take.carried_positions = Some((
                self.positions_now(),
                self.bench.take.opened_positions.clone(),
            ));
            self.bench.take.reread_with = Some(self.watch_setup());
            self.analyse_whole(&take, None);
            self.bench.take.reread_with = None;
            self.put_masked(masked);
        }
    }

    /// Set the beat rate, and the live analysis's; true when the beats heard
    /// so far stand, being read at that rate already (such as a calibre's
    /// 28800 after Auto found 28800, or Auto where Auto found the rate, as
    /// core's `set_bph` says), so the strip and trends keep all heard so far.
    fn set_live_bph(&mut self, bph: Option<u32>) -> bool {
        self.put_bph(bph);
        let Some(l) = self.bench.recording.live.as_mut() else {
            return false;
        };
        l.set_bph(bph)
    }

    /// Quick Listen's settings: no watch, no calibre, the default lift
    /// angle and the beat rate found from the beats.
    fn quick_listen_defaults(&mut self) {
        self.watches_view.quick_defaults = false;
        self.bench.take.watch.clear();
        self.put_calibre(String::new());
        if self.bench.take.lift_deg != DEFAULT_LIFT_DEG {
            self.set_lift(DEFAULT_LIFT_DEG);
        }
        self.bench.take.lift_context = (DEFAULT_LIFT_DEG, false);
        if self.bench.take.bph.is_some() && !self.set_live_bph(None) {
            self.bench.recording.anchor = None;
            self.clear_trends();
        }
        self.note_settings();
    }

    /// A watch's details as this take's settings.
    pub(super) fn apply_watch(&mut self, w: &library::Watch) {
        self.bench.take.watch = w.name.clone();
        match w.calibre.clone().filter(|c| !c.is_empty()) {
            Some(c) => self.set_calibre(c),
            // Not the calibre of the watch picked before.
            None => self.put_calibre(String::new()),
        }
        // The calibre's lift angle and beat rate, else the watch's own,
        // else 52° and Auto: nothing kept from the watch picked before.
        let lift = self.cal.lift(&self.bench.take.calibre).or(w.lift);
        let l = lift.unwrap_or(DEFAULT_LIFT_DEG);
        if l != self.bench.take.lift_deg {
            self.set_lift(l);
        }
        self.bench.take.lift_context = (l, lift.is_some());
        let bph = self.cal.bph(&self.bench.take.calibre).or(w.beat_rate());
        if bph != self.bench.take.bph && !self.set_live_bph(bph) {
            self.bench.recording.anchor = None;
            self.clear_trends();
        }
        self.note_settings();
    }

    /// In a narrow window, one at a time: a watch or Unsorted Takes shown
    /// has only the way back to the list in its toolbar. Whether it did.
    fn watches_back(&mut self, ui: &mut egui::Ui) -> bool {
        if self.watches_view.watches_wide || self.list_shown() {
            return false;
        }
        if self.watches_view.unsorted_open && self.watches_view.watch_open.is_none() {
            theme::keep_next(ui);
            if theme::tool_word_button(
                ui,
                Icon::Back,
                "All Watches",
                "Back to the list of watches, Unsorted Takes marked",
                false,
                None,
            ) {
                self.watches_view.list_for = Some(watches_view::Picked::UNSORTED);
            }
            return true;
        } else if self.open_watch().is_some() {
            theme::keep_next(ui);
            if theme::tool_word_button(
                ui,
                Icon::Back,
                "All Watches",
                "Back to the list of watches, this one marked, the search kept: Listen \
                 and the tests still take it until another row is picked",
                false,
                None,
            ) {
                self.watches_view.list_for = Some(watches_view::Picked {
                    watch: self.watches_view.watch_open.clone(),
                    unsorted: false,
                });
            }
            return true;
        }
        false
    }

    /// The Watches view's toolbar: the list's search, order and New
    /// Watch…, or, with a watch picked, the way back and Edit….
    pub(super) fn watches_tools(&mut self, ui: &mut egui::Ui) {
        // One at a time in a narrow window: the watch shown has only the
        // way back to the list.
        if self.watches_back(ui) {
            return;
        }
        // The pane's main action first, in words, and never folded: the
        // empty list names it.
        let writable = !matches!(&self.library, Ok(Some(l)) if l.read_only.is_some())
            && self.library_root().is_some();
        theme::keep_next(ui);
        if theme::tool_word_button(
            ui,
            Icon::NewWatch,
            "New Watch…",
            "Add a watch: only its name is needed",
            false,
            (!writable).then_some(
                "This copy of the app can only read your watches, or there is no folder for them",
            ),
        ) {
            self.open_new_watch_form();
        }
        theme::bar_gap(ui);
        // The search stays in the row too: Sort folds first.
        theme::keep_next(ui);
        theme::tool_search(
            ui,
            "Show the watches whose name, calibre, notes or fields hold this",
            |ui| {
                // Narrower in a narrow pane, leaving room for "…" after it.
                let w = (ui.available_width() - 48.0).clamp(72.0, 120.0);
                let field = ui
                    .add(
                        egui::TextEdit::singleline(&mut self.watches_view.watch_search)
                            .hint_text("Search")
                            .desired_width(w),
                    )
                    .on_hover_text(
                        "Show the watches whose name, calibre, notes or fields hold this",
                    );
                if std::mem::take(&mut self.watches_view.watch_search_focus) {
                    // Its text selected, as Find does, so typing replaces it.
                    field.request_focus();
                    let mut state =
                        egui::TextEdit::load_state(ui.ctx(), field.id).unwrap_or_default();
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::default(),
                            egui::text::CCursor::new(
                                self.watches_view.watch_search.chars().count(),
                            ),
                        )));
                    state.store(ui.ctx(), field.id);
                }
            },
        );
        let mut sort = self.watches_view.watch_sort;
        theme::tool_field(ui, "Sort", "The order of the list", |ui| {
            egui::ComboBox::from_id_salt("watch sort")
                .selected_text(sort.words())
                .show_ui(ui, |ui| {
                    for s in watches::Sort::ALL {
                        ui.selectable_value(&mut sort, s, s.words());
                    }
                });
        });
        self.watches_view.watch_sort = sort;
        // What each row shows under the watch's name: the calibre, or one
        // of the fields added to the watches.
        let labels: Vec<String> = match &self.library {
            Ok(Some(l)) => {
                let mut v: Vec<String> = l
                    .watches
                    .iter()
                    .flat_map(|e| e.watch.fields.iter().map(|f| f.label.clone()))
                    .collect();
                v.sort();
                v.dedup();
                v
            }
            _ => Vec::new(),
        };
        // With no field added to any watch, Calibre is the only choice:
        // no drop-down of one (#2342). The choice is kept for when one is.
        if labels.is_empty() {
            return;
        }
        // A field no watch has any more shows as Calibre, the choice kept
        // for when one has it again.
        let mut shows = self.list_shows().map(str::to_string);
        let was = shows.clone();
        theme::tool_field(
            ui,
            "List Shows",
            "What each watch's row shows under its name: its calibre, or a field added to \
             the watches, such as Customer",
            |ui| {
                egui::ComboBox::from_id_salt("watch list shows")
                    .selected_text(shows.as_deref().unwrap_or("Calibre"))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut shows, None, "Calibre");
                        for l in &labels {
                            ui.selectable_value(&mut shows, Some(l.clone()), l);
                        }
                    });
            },
        );
        if shows != was {
            self.watches_view.watch_list_shows = shows;
        }
    }

    /// The field the list shows under each name, while some watch has it;
    /// `None` is the calibre.
    pub(super) fn list_shows(&self) -> Option<&str> {
        let label = self.watches_view.watch_list_shows.as_deref()?;
        match &self.library {
            Ok(Some(l)) => l
                .watches
                .iter()
                .any(|e| e.watch.fields.iter().any(|f| f.label == label))
                .then_some(label),
            _ => None,
        }
    }

    /// The form for a new watch: only its name is needed.
    pub(super) fn open_new_watch_form(&mut self) {
        self.watches_view.new_watch_on_mic = false;
        let mut w = library::Watch::new("");
        w.calibre = Some(String::new());
        w.notes = Some(String::new());
        self.watches_view.watch_form = Some(watches::Form {
            watch: w,
            new: true,
            error: None,
        });
    }

    /// The form for a watch's details, while it is open.
    pub(super) fn watch_form_dialog(&mut self, ctx: &egui::Context) {
        let Some(f) = self.watches_view.watch_form.as_mut() else {
            return;
        };
        match watches::form(ctx, f, &mut self.cal) {
            Some(watches::FormAction::Save) => self.save_watch_form(),
            Some(watches::FormAction::Cancel) => {
                self.watches_view.watch_form = None;
                self.bench.keeping.keep_after_form = false;
            }
            None => {}
        }
    }

    /// The take opened whole (or being opened), if one is on show.
    pub(super) fn take_on_show(&self) -> Option<PathBuf> {
        (matches!(
            self.session.state(),
            session::State::Analysing | session::State::Analysed
        ) && !self.bench.keeping.opened_path.is_empty())
        .then(|| PathBuf::from(&self.bench.keeping.opened_path))
    }

    /// The take the session saved into: Set Wind… there would be undone
    /// by its next save, written from memory (Felix on #1745). A take
    /// opened isn't: each write to it edits its file in place (Felix on
    /// #1947).
    pub(super) fn take_held(&self) -> Option<PathBuf> {
        self.session.saved_to().map(Path::to_path_buf)
    }

    /// The takes the Bench has in use, whose files nothing else may
    /// rewrite under it: the take on show ([`Self::take_on_show`]) and the
    /// one its session saves into ([`Self::take_held`]), recording or
    /// stopped, as Save Again writes there from memory (#1988).
    pub(super) fn takes_in_use(&self) -> Vec<PathBuf> {
        self.take_on_show()
            .into_iter()
            .chain(self.take_held())
            .collect()
    }

    /// The take the Bench is recording into now: the one it saved into,
    /// while the microphone runs. Stopped (paused), or a recording opened,
    /// it is none (Trevor on #2032).
    pub(super) fn take_writing(&self) -> Option<PathBuf> {
        self.take_held()
            .filter(|_| self.running() && self.session.mic())
    }

    /// Ask for each of `e`'s takes' own reading, as `timegrapher watches`
    /// gives it; a test's figures are its own, and a take still being
    /// recorded is read once it stops (#2027).
    pub(super) fn want_take_readings(&mut self, e: &library::WatchEntry, ctx: &egui::Context) {
        self.watches_view.take_readings.poll();
        let writing = self.take_writing();
        // Each is asked again when the library is read again or a setting
        // its reading reads changes, so an edit that keeps a take's size
        // (lift 55 to 52) shows its new reading (#2410).
        let at = match &self.library {
            Ok(Some(l)) => l.readings_revision(),
            _ => Default::default(),
        };
        for t in &e.takes {
            let dir = e.take_dir(t);
            if watches::TakeReadings::read_for(t, &dir) && writing.as_ref() != Some(&dir) {
                self.watches_view.take_readings.want(&dir, &at, ctx);
            }
        }
    }

    /// Make the library at its folder when there is none yet.
    pub(super) fn ensure_library(&mut self) {
        if matches!(self.library, Ok(None)) {
            self.library = match self.library_root() {
                Some(root) => library::Library::open_or_create(&root).map(Some),
                None => Err("there is no folder for it: choose one in Settings".into()),
            };
        }
    }

    /// Quick Listen: show Unsorted and listen to the microphone in a new
    /// session, without a watch picked, its name cleared. Stop keeps
    /// nothing until Keep in a Watch… or Save…. Unsaved sound is asked about first, unless
    /// `asked`.
    pub(super) fn quick_listen(&mut self, asked: bool) {
        if self.behind_save(move |a| a.quick_listen(asked)) {
            return;
        }
        if self.quick_listen_ready(asked) {
            self.start_microphone();
        }
    }

    /// All of Quick Listen but opening the microphone: whether to open it.
    pub(super) fn quick_listen_ready(&mut self, asked: bool) -> bool {
        if self.running() {
            self.info_msg("Stop listening first.".into());
            return false;
        }
        self.ensure_library();
        let ready = match &self.library {
            Ok(Some(l)) => match &l.read_only {
                Some(why) => Err(why.clone()),
                None => l.unsorted_dir().map(|_| ()),
            },
            Ok(None) => Err("There is no Watches folder".into()),
            Err(e) => Err(format!("Your watches can't be read: {e}")),
        };
        if let Err(e) = ready {
            self.error(format!("Quick Listen can't keep its take: {e}"));
            return false;
        }
        if !asked && self.needs_save_confirmation() {
            self.confirm = Some(Confirm::QuickListen);
            return false;
        }
        // Not even a resume: Quick Listen starts a new session.
        if self.saving_busy() {
            return false;
        }
        if self.bench.recording.live.is_some() || self.session.recorder.is_some() {
            self.new_session();
        }
        if self.watches_view.quick_defaults {
            self.quick_listen_defaults();
        }
        self.watches_view.watch_open = None;
        self.watches_view.unsorted_open = true;
        // The Watches view shows the list, Unsorted Takes marked.
        self.watches_view.list_for = Some(watches_view::Picked::UNSORTED);
        self.bench.recording.quick_listen = true;
        self.bench.recording.input = Input::Microphone;
        self.bench.take.watch.clear();
        self.note_settings();
        true
    }

    /// Write the form's watch into the library, making the library first
    /// for the first watch.
    pub(super) fn save_watch_form(&mut self) {
        let Some(f) = self.watches_view.watch_form.clone() else {
            return;
        };
        let mut w = f.watch;
        w.name = w.name.trim().to_string();
        // Empty is the same as not set.
        w.calibre = w.calibre.filter(|c| !c.trim().is_empty());
        w.notes = w.notes.filter(|n| !n.trim().is_empty());
        w.fields
            .retain(|x| !(x.label.trim().is_empty() && x.value.trim().is_empty()));
        // The app keeps no Archived mark: one an older version set goes
        // when the watch is saved, so the command line lists it too.
        w.archived = false;
        self.ensure_library();
        let done = match &mut self.library {
            Ok(Some(lib)) if f.new => lib.add(&w).map(|_| ()),
            Ok(Some(lib)) => lib.update(&w),
            Ok(None) => Err("There is no Watches folder".into()),
            Err(e) => Err(format!("Your watches can't be read: {e}")),
        };
        match done {
            Ok(()) => {
                self.watches_view.watch_form = None;
                if f.new && std::mem::take(&mut self.bench.keeping.keep_after_form) {
                    match self.bench.test.test_keep.take() {
                        Some(kind) => self.keep_test(kind, Some(&w.id)),
                        None => self.keep_into(&w.id),
                    }
                } else if f.new && std::mem::take(&mut self.watches_view.new_watch_on_mic) {
                    self.line_pick(Some(w.id.clone()), false);
                } else if f.new {
                    // As a click on it in the list picks it, while
                    // listening too (#1981).
                    self.list_pick(w.id.clone());
                } else if self.watches_view.watch_open.as_ref() == Some(&w.id)
                    && !self.running()
                    && self.bench.recording.live.is_none()
                {
                    // The take not yet started follows the watch's new details.
                    self.apply_watch(&w);
                }
            }
            Err(e) => {
                if let Some(f) = self.watches_view.watch_form.as_mut() {
                    f.error = Some(e);
                }
            }
        }
    }
}
