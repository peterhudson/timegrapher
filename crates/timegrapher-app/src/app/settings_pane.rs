//! The Settings window and the input level controls: the gear, the level meter and gain, the microphone, general and calibre pages, and the take settings they write.

use super::*;

impl TimegrapherApp {
    /// The gear at the end of the icon bar, in both views: Settings.
    pub(super) fn settings_gear(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        if theme::icon_toggle(
            ui,
            theme::Icon::Settings,
            self.settings_tab.is_some(),
            pal.accent,
        )
        .on_hover_text("Settings: how the app looks, the calibres and each input's kept clock")
        .clicked()
        {
            self.open_settings(self.settings_last);
        }
    }

    pub(super) fn clipping(&self) -> bool {
        self.clipped_at
            .is_some_and(|t| t.elapsed().as_secs_f64() < 3.0)
    }

    /// The level meter: a grey bar to the latest peak over a grey band
    /// between the plain reference marks at -10 and -6 dBFS. Clipping is
    /// said in words, not by colour.
    fn meter(&self, ui: &mut egui::Ui, width: f32, height: f32) -> egui::Response {
        let pal = theme::pal(ui);
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
        let r = height / 2.0;
        let p = ui.painter();
        p.rect_filled(rect, r, pal.control);
        // The band between the reference marks, shaded.
        let at = |db: f64| rect.left() + rect.width() * ((db + 60.0) / 60.0) as f32;
        p.rect_filled(
            egui::Rect::from_x_y_ranges(
                at(REFERENCE_DBFS[0])..=at(REFERENCE_DBFS[1]),
                rect.y_range(),
            ),
            0.0,
            pal.text_tertiary.gamma_multiply(0.35),
        );
        if self.running() {
            let peak = self.level.peak_dbfs.max(-60.0);
            let frac = ((peak + 60.0) / 60.0).clamp(0.0, 1.0) as f32;
            let mut fill = rect;
            fill.set_width((rect.width() * frac).max(height));
            p.rect_filled(fill, r, pal.text_secondary);
        }
        for db in REFERENCE_DBFS {
            let x = rect.left() + rect.width() * ((db + 60.0) / 60.0) as f32;
            p.line_segment(
                [
                    egui::pos2(x, rect.top() - 2.0),
                    egui::pos2(x, rect.bottom() + 2.0),
                ],
                egui::Stroke::new(1.5_f32, pal.text_tertiary),
            );
        }
        resp
    }

    /// Find the selected input's level control and read it.
    fn read_gain(&mut self) {
        self.gain_device = self.device.clone();
        // Before the inputs are listed, the system default's level control:
        // reading a mixer does not open the input.
        let id = match (&self.device, self.devices_listed) {
            (Some(d), _) => Some(d.as_str()),
            (None, false) => Some("default"),
            (None, true) => None,
        };
        self.gain = id.and_then(InputGain::for_device);
        self.gain_state = self.gain.as_ref().and_then(|g| g.read());
        self.gain_read_at = Instant::now();
        self.agc_read = self.input_agc(None).and_then(Result::ok);
    }

    /// The key Settings keeps the selected input's automatic gain under.
    fn agc_key(&self) -> String {
        self.device.clone().unwrap_or_else(|| "default".into())
    }

    /// The selected input's automatic gain switch, read back, after
    /// turning it on or off first with `set`. `None` when the input has no
    /// switch the app can reach: the sound card's mixer on Linux, the USB
    /// microphone itself on macOS.
    fn input_agc(&self, set: Option<bool>) -> Option<Result<bool, String>> {
        if cfg!(windows) {
            // The input by the name the system gives it, the system
            // default's when none is picked; Windows sets the one input.
            let name = self
                .devices
                .iter()
                .find(|d| match &self.device {
                    Some(id) => &d.id == id,
                    None => d.is_default,
                })
                .map(|d| d.name.clone())?;
            let first =
                |r: Result<Vec<mixer::EndpointAgc>, String>| r.map(|v| v.into_iter().next());
            let found = match first(mixer::endpoint_agc(Some(&name), None)) {
                Ok(Some(f)) => f,
                Ok(None) => return None,
                Err(e) => return Some(Err(e)),
            };
            return Some(match set {
                Some(want) if found.before != Some(want) => {
                    match first(mixer::endpoint_agc(Some(&name), Some(want))) {
                        Ok(Some(mixer::EndpointAgc { error: Some(e), .. })) | Err(e) => Err(e),
                        Ok(Some(mixer::EndpointAgc { after: Some(v), .. })) => Ok(v),
                        _ => Err("the input didn't say whether it changed".into()),
                    }
                }
                _ => found.before.ok_or_else(|| {
                    found
                        .error
                        .unwrap_or_else(|| "the input didn't answer".into())
                }),
            });
        }
        if cfg!(target_os = "macos") {
            let label = capture::choices(&self.devices)
                .into_iter()
                .find(|c| Some(&c.id) == self.device.as_ref())
                .map(|c| c.label);
            let read = |list: Result<Vec<mixer::UsbAgc>, String>| {
                usb_agc_for(list.ok()?, label.as_deref())
            };
            let found = read(mixer::usb_agc(None))?;
            let now = found.before;
            return Some(match set {
                // Over USB the request reaches every USB microphone's
                // switch, so it goes only when this input's needs it.
                Some(want) if now != Some(want) => match read(mixer::usb_agc(Some(want))) {
                    Some(mixer::UsbAgc { error: Some(e), .. }) => Err(e),
                    Some(mixer::UsbAgc { after: Some(v), .. }) => Ok(v),
                    _ => Err("the microphone didn't say whether it changed".into()),
                },
                _ => now.ok_or_else(|| {
                    found
                        .error
                        .unwrap_or_else(|| "the microphone didn't answer".into())
                }),
            });
        }
        let g = self.gain.as_ref()?;
        self.gain_state.as_ref()?.agc?;
        if let Some(want) = set {
            if let Err(e) = g.set_agc(want) {
                return Some(Err(e));
            }
        }
        g.read()?.agc.map(Ok)
    }

    /// Turn the selected input's automatic gain off, or leave it on where
    /// Settings says so: each time the app opens the input, since a stand
    /// turns it back on when it is plugged in.
    pub(super) fn apply_agc(&mut self) {
        let want = self.agc_on.contains(&self.agc_key());
        if self.gain_device != self.device {
            self.read_gain();
        }
        match self.input_agc(Some(want)) {
            Some(Ok(now)) => {
                self.agc_read = Some(now);
                applog::info(&format!(
                    "automatic gain {}",
                    if now { "on" } else { "off" }
                ));
            }
            Some(Err(e)) => {
                self.agc_read = None;
                self.gain_error = Some(format!(
                    "Couldn't turn the microphone's automatic gain {}: {e}",
                    if want { "on" } else { "off" }
                ));
            }
            None => self.agc_read = None,
        }
    }

    /// Read the picked input's level afresh when it changed or has not
    /// been read for a while.
    pub(super) fn refresh_gain(&mut self) {
        let every = if self.running() { 2.0 } else { 10.0 };
        if self.gain_device != self.device || self.gain_read_at.elapsed().as_secs_f64() > every {
            self.read_gain();
        }
    }

    /// The Input Level slider and its value, `w` wide; where the system
    /// sets the level, "Not adjustable here" (or, on macOS and Windows,
    /// the way to the system's own page), at the same height.
    pub(super) fn level_control(&mut self, ui: &mut egui::Ui, w: f32) {
        let pal = theme::pal(ui);
        let h = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            Vec2::new(w, h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(Vec2::new(w, h));
                match (self.gain.clone(), self.gain_state.clone()) {
                    (Some(g), Some(st)) => {
                        let mut pct = st.level * 100.0;
                        ui.spacing_mut().slider_width = (w - 64.0).max(60.0);
                        let r = ui
                            .add(
                                egui::Slider::new(&mut pct, 0.0..=100.0)
                                    .show_value(false)
                                    .step_by(1.0),
                            )
                            .on_hover_text(LEVEL_HINT);
                        ui.label(RichText::new(&st.text).color(pal.text_secondary));
                        // Set it once the drag ends (or on a click or key),
                        // not on every pixel.
                        if (r.changed() && !r.dragged()) || r.drag_stopped() {
                            self.gain_error = g.set_level(pct / 100.0).err();
                            applog::info(&format!("input level set to {pct:.0}%"));
                            self.read_gain();
                        } else if r.changed() {
                            if let Some(s) = self.gain_state.as_mut() {
                                s.level = pct / 100.0;
                            }
                        }
                    }
                    _ if cfg!(any(target_os = "macos", target_os = "windows")) => {
                        // The system sets it: open the system's own page.
                        if ui
                            .button("Open Sound Settings…")
                            .on_hover_text(SOUND_SETTINGS_HINT)
                            .clicked()
                        {
                            self.gain_error = open_sound_settings()
                                .err()
                                .map(|e| format!("Couldn't open the sound settings: {e}"));
                        }
                    }
                    _ => {
                        ui.label(RichText::new("Not adjustable here").color(pal.text_secondary))
                            .on_hover_text(LEVEL_HINT);
                    }
                }
            },
        );
    }

    /// The Automatic Gain switch, for an input whose own automatic gain
    /// the app can reach; false (and nothing drawn) for any other.
    pub(super) fn agc_control(&mut self, ui: &mut egui::Ui) -> bool {
        if self.agc_read.is_none() {
            return false;
        }
        let key = self.agc_key();
        let mut on = self.agc_on.contains(&key);
        if theme::switch(ui, &mut on).on_hover_text(AGC_HINT).changed() {
            self.agc_on.retain(|k| k != &key);
            if on {
                self.agc_on.push(key);
            }
            self.apply_agc();
        }
        true
    }

    /// What the microphone says of its automatic gain, where it isn't
    /// what was asked.
    pub(super) fn agc_message(&self) -> Option<String> {
        let now = self.agc_read?;
        let on = self.agc_on.contains(&self.agc_key());
        (now != on).then(|| {
            format!(
                "The microphone reports its automatic gain {}.",
                if now { "on" } else { "off" }
            )
        })
    }

    /// The Peak meter, `w` wide, and its dBFS after it.
    pub(super) fn peak_control(&self, ui: &mut egui::Ui, w: f32) {
        let pal = theme::pal(ui);
        ui.horizontal(|ui| {
            self.meter(ui, (w - 72.0).max(60.0), 8.0)
                .on_hover_text(PEAK_HINT);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(64.0, 18.0), egui::Sense::hover());
            ui.painter().text(
                rect.right_center(),
                egui::Align2::RIGHT_CENTER,
                self.peak_words(),
                egui::FontId::proportional(13.0),
                pal.text_secondary,
            );
        });
    }

    /// The latest peak, "−11 dBFS", or a dash while nothing is heard.
    fn peak_words(&self) -> String {
        if self.running() {
            let peak = self.level.peak_dbfs.max(-99.0);
            format!("{peak:.0} dBFS").replace('-', "−")
        } else {
            "–".into()
        }
    }

    /// The Settings window, when open, over everything else. Manage
    /// Calibres… opens it at Calibres.
    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.cal.manager_open) {
            self.open_settings(Tab::Calibres);
            self.cal.apply_rate_preset();
        }
        self.poll_library_move(ctx);
        let Some(mut tab) = self.settings_tab else {
            return;
        };
        let screen = ctx.content_rect();
        let size = Vec2::new(
            (screen.width() - 24.0).clamp(320.0, 1100.0),
            (screen.height() - 64.0).clamp(240.0, 760.0),
        );
        let mut done = false;
        let modal = egui::Modal::new(egui::Id::new("settings")).show(ctx, |ui| {
            ui.set_width(size.x);
            ui.set_height(size.y);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Settings").strong().size(16.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(theme::primary(ui, "Done"))
                        .on_hover_text("Close Settings")
                        .clicked()
                    {
                        done = true;
                    }
                });
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                let h = ui.available_height();
                let down = egui::Layout::top_down(egui::Align::Min);
                ui.allocate_ui_with_layout(Vec2::new(SETTINGS_SIDEBAR_W, h), down, |ui| {
                    ui.set_min_size(Vec2::new(SETTINGS_SIDEBAR_W, h));
                    settings_window::sidebar(ui, &mut tab);
                });
                ui.separator();
                let rest = Vec2::new(ui.available_width(), h);
                ui.allocate_ui_with_layout(rest, down, |ui| {
                    ui.set_min_size(rest);
                    self.settings_category(ui, tab);
                });
            });
        });
        self.settings_last = tab;
        self.settings_tab = (!done && !modal.should_close()).then_some(tab);
        self.forget_dialog(ctx);
        self.library_move_dialog(ctx);
    }

    /// The category picked in Settings, under its name.
    fn settings_category(&mut self, ui: &mut egui::Ui, tab: Tab) {
        let name = settings_window::TABS
            .iter()
            .find(|t| t.0 == tab)
            .map_or("", |t| t.2);
        ui.horizontal(|ui| {
            ui.label(RichText::new(name).strong().size(16.0));
            match tab {
                Tab::Microphone => {
                    theme::help(ui, "Microphone", help::MICROPHONE);
                }
                Tab::WatchesFolder => {
                    theme::help(ui, "Watches Folder", settings_window::LIBRARY_HELP);
                }
                Tab::Calibres => {
                    // New Custom Calibre…, the page's one action, is at the
                    // right of the search over the table.
                    theme::help(ui, "Calibres", calibre_ui::MANAGE_HELP);
                }
                Tab::General => {}
            }
        });
        ui.add_space(6.0);
        match tab {
            Tab::General => {
                egui::ScrollArea::vertical()
                    .id_salt("general settings")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        self.general_settings(ui);
                    });
            }
            Tab::Microphone => {
                egui::ScrollArea::vertical()
                    .id_salt("microphone settings")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        self.microphone_page(ui);
                        let listening = self.clock_track.as_ref().map(|t| t.device.clone());
                        let timing = self.clock_timing();
                        match settings_window::quartz_section(
                            ui,
                            &self.kept_clocks,
                            listening.as_deref(),
                            timing.as_ref(),
                        ) {
                            Some(settings_window::QuartzAct::Forget(device)) => {
                                self.forget_ask = Some(device);
                            }
                            Some(settings_window::QuartzAct::Keep) => {
                                self.keep_clock();
                                self.kept_clocks = clockstore::ClockStore::load();
                            }
                            None => {}
                        }
                    });
            }
            Tab::WatchesFolder => {
                egui::ScrollArea::vertical()
                    .id_salt("watches folder settings")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let root = self.library_root();
                        let act = settings_window::library_section(
                            ui,
                            root.as_deref(),
                            &self.library,
                            self.library_moving.as_ref().map(|(to, _)| to.as_path()),
                            self.library_dir.is_some(),
                        );
                        if let Some(act) = act {
                            self.library_action(act);
                        }
                    });
            }
            Tab::Calibres => {
                let current = self.calibre.clone();
                let out = self.cal.contents(ui, &current, env!("CARGO_PKG_VERSION"));
                let name = out.name.clone();
                self.calibre_changed(&name, out.change);
            }
        }
    }

    /// Settings > Microphone's groups: the microphone, then its level.
    /// Quartz Correction follows.
    pub(super) fn microphone_page(&mut self, ui: &mut egui::Ui) {
        self.refresh_gain();
        let input_fault = self.input_fault.clone();
        theme::group(ui, "Microphone", None, |g| {
            let hint = "The microphone to listen to. Show Every Input lists every device the \
                        system offers.";
            // The menu on its own line, the group's width, whatever the
            // device: a long name moves nothing.
            let caption = match &input_fault {
                Some(f) => theme::Caption::Alert(theme::Alert::Warning, f),
                None => theme::Caption::Text("The input the watch sits on."),
            };
            g.row_under("Microphone", caption, hint, |ui| {
                self.device_menu(ui, ui.available_width(), true);
            });
            let hint = "List every device the system offers in the microphone menu, with its id";
            g.row(
                "Show Every Input",
                theme::Caption::Text("Lists every device the system offers, with its id."),
                hint,
                |ui| theme::switch(ui, &mut self.all_inputs).on_hover_text(hint),
            );
        });
        let gain_error = self.gain_error.clone();
        let agc = self.agc_message();
        let clipping = self.running() && self.clipping();
        theme::group(ui, "Level", None, |g| {
            g.row(
                "Input Level",
                match &gain_error {
                    Some(e) => theme::Caption::Alert(theme::Alert::Bad, e),
                    None => theme::Caption::Text("Set it so the peaks sit in the shaded band."),
                },
                LEVEL_HINT,
                |ui| {
                    self.level_control(ui, 240.0);
                },
            );
            if self.agc_read.is_some() {
                g.row(
                    "Automatic Gain",
                    theme::Caption::Alert(
                        theme::Alert::Warning,
                        agc.as_deref().unwrap_or(AGC_WARNING),
                    ),
                    AGC_HINT,
                    |ui| {
                        self.agc_control(ui);
                    },
                );
            }
            g.row(
                "Peak",
                if clipping {
                    theme::Caption::Alert(theme::Alert::Warning, CLIPPING_WORDS)
                } else {
                    theme::Caption::Text(BAND_WORDS)
                },
                PEAK_HINT,
                |ui| {
                    self.peak_control(ui, 240.0);
                },
            );
        });
    }

    /// Forget asks first, saying what is lost: the kept clock took 20
    /// minutes or more of listening to measure.
    fn forget_dialog(&mut self, ctx: &egui::Context) {
        let Some(device) = self.forget_ask.clone() else {
            return;
        };
        let kept = self
            .kept_clocks
            .as_ref()
            .ok()
            .and_then(|s| s.get(&device))
            .cloned();
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("forget clock")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(
                RichText::new(match &kept {
                    Some(c) => format!(
                        "Forget the clock kept for \"{device}\", {}, measured {}?",
                        settings_window::ppm_words(c.ppm),
                        day_of(&c.measured_utc)
                    ),
                    None => format!("Forget the clock kept for \"{device}\"?"),
                })
                .font(theme::semibold(15.0)),
            );
            ui.add_space(4.0);
            ui.label(
                "Rates from this input are then read on its own clock, uncorrected, here, on \
                 the command line and in the MCP server. In the app, getting it back takes 20 \
                 minutes or more of listening and Keep.",
            );
            ui.add_space(8.0);
            // Nothing that can't be got back is the default: Return does
            // nothing, and the button that throws it away is far from Cancel.
            if let Some(i) = theme::dialog_buttons(
                ui,
                &[
                    DialogButton::new(Role::Destructive, "Forget", ""),
                    DialogButton::new(Role::Cancel, "Cancel", ""),
                ],
            ) {
                choice = Some(i == 0);
            }
        });
        if choice.is_none() && modal.should_close() {
            choice = Some(false);
        }
        if let Some(go) = choice {
            self.forget_ask = None;
            if go {
                self.forget_clock(&device);
            }
        }
    }

    /// Settings' General section: how the app looks, listening, and
    /// whether it asks about updates, as grouped rows.
    pub(super) fn general_settings(&mut self, ui: &mut egui::Ui) {
        let mut t = ui.ctx().options(|o| o.theme_preference);
        theme::group(ui, "Appearance", None, |g| {
            g.row(
                "Appearance",
                theme::Caption::Text("Auto follows the system's light or dark mode."),
                "How the app looks: light, dark, or as the system is set",
                |ui| {
                    theme::segmented(
                        ui,
                        &mut t,
                        &[
                            (
                                egui::ThemePreference::System,
                                "Auto",
                                "Follow the system's light or dark setting",
                            ),
                            (egui::ThemePreference::Light, "Light", "Always light"),
                            (egui::ThemePreference::Dark, "Dark", "Always dark"),
                        ],
                    );
                },
            );
        });
        if t != ui.ctx().options(|o| o.theme_preference) {
            ui.ctx().set_theme(t);
            self.theme = t;
        }
        let awake = self.awake_words();
        theme::group(ui, "Listening", None, |g| {
            let hint = "Start listening to the microphone as soon as the app opens, for the \
                        watch on the stand. Off, Listen on the Listen card starts it.";
            g.row(
                "Listen When the App Opens",
                theme::Caption::Text("Off: press Listen on the Listen card to start."),
                hint,
                |ui| theme::switch(ui, &mut self.listen_at_open).on_hover_text(hint),
            );
            let mut hint = "While listening, stop the computer going to sleep on its own, so \
                            a long take isn't cut short. The screen can still lock and switch \
                            off."
                .to_string();
            if let Some((_, said)) = &awake {
                hint = format!("{hint}\n\n{said}");
            }
            let hint = hint.as_str();
            g.row(
                "Keep Awake",
                match &awake {
                    Some((w, _)) => theme::Caption::Alert(theme::Alert::Warning, w),
                    None => theme::Caption::Text(
                        "While listening, the computer doesn't sleep on its own. The screen \
                         can still lock.",
                    ),
                },
                hint,
                |ui| theme::switch(ui, &mut self.keep_awake).on_hover_text(hint),
            );
        });
        theme::group(ui, "Updates", None, |g| {
            let hint = "Ask GitHub whether a newer Timegrapher is published each time the app \
                        starts, and say so if there is. It sends no account or identifier, and \
                        nothing installs itself. Help > Check for Updates… asks now.";
            g.row(
                "Check for Updates at Start",
                theme::Caption::Text(
                    "Asks GitHub once per start. Sends no account or identifier; installs \
                     nothing.",
                ),
                hint,
                |ui| theme::switch(ui, &mut self.check_at_start).on_hover_text(hint),
            );
        });
    }

    /// Forget the kept clock of `device`: its rates are read on its own
    /// clock again, here and for the command line and the MCP server.
    pub(super) fn forget_clock(&mut self, device: &str) {
        let forgot = clockstore::ClockStore::load().and_then(|mut s| {
            s.remove(device);
            s.save().map(|_| s)
        });
        match forgot {
            Ok(s) => {
                // The microphone session in hand, listening or stopped,
                // loses its True Clock too.
                if self.clock.as_ref().is_some_and(|c| c.device == device) {
                    self.put_input_clock(None);
                }
                self.kept_clocks = Ok(s);
                self.info_msg(format!(
                    "Forgot the clock kept for \"{device}\": its rates are read on its own \
                     clock again."
                ));
            }
            Err(e) => {
                self.kept_clocks = clockstore::ClockStore::load();
                self.error(format!(
                    "The clock kept for \"{device}\" couldn't be forgotten: {e}"
                ));
            }
        }
    }

    /// Act on what was changed for the calibre `name`: only this watch's
    /// calibre moves anything outside the list.
    fn calibre_changed(&mut self, name: &str, change: calibre_ui::Change) {
        if name.is_empty() || name != self.calibre {
            return;
        }
        if let Some(n) = change.renamed {
            self.put_calibre(n.clone());
            self.cal.recent.retain(|r| r != name);
            self.cal.remember(&n);
        }
        if change.deleted {
            self.cal.recent.retain(|r| r != name);
            self.set_calibre(String::new());
        }
        if let Some(l) = change.lift {
            self.set_lift(l);
            self.lift_context = (l, true);
        }
        // The calibre's wheels changed: the steadiness tests name them
        // afresh.
        if change.wheels {
            self.rev.settings += 1;
        }
    }

    /// How core reads a take opened with session file `m`: at the beat
    /// rate and lift angle on show (the session file's, or what Read Again
    /// was asked for), whether that lift is known, and the escape teeth
    /// as `take::stream_settings` gives them.
    pub(super) fn entry_settings(&self, m: &Manifest) -> timegrapher_core::take::EntrySettings {
        use timegrapher_core::take;
        let given = take::Given {
            bph: self.bph,
            lift_deg: Some(self.lift_deg),
        };
        // The take's calibre, from the user's calibres as the Watches list
        // and `session` find it, for what its session file doesn't give.
        let cal = m
            .calibre
            .as_deref()
            .and_then(|n| take::calibre_named(n, &self.cal.user));
        let mut stream = take::stream_settings(m, cal.as_ref(), given);
        stream.analysis.lift_known = self.lift_known();
        take::EntrySettings {
            stream,
            card_clock: false,
            settle_s: None,
        }
    }

    /// Restore known/assumed context only for the value it described. Numeric
    /// edits and reverts keep the existing default-angle inference; this does
    /// not introduce a new way to explicitly confirm an unchanged 52° in UI.
    pub(super) fn lift_known(&self) -> bool {
        if self.lift_deg == self.lift_context.0 {
            self.lift_context.1
        } else {
            self.lift_deg != DEFAULT_LIFT_DEG
        }
    }

    pub(super) fn set_lift(&mut self, lift: f64) {
        self.put_lift(lift);
        if let Some(l) = self.live.as_mut() {
            l.set_lift(lift);
        }
        // Changed by hand: the charts follow at once.
        self.refresh_trend();
        self.note_settings();
    }

    pub(super) fn put_lift(&mut self, lift: f64) {
        if lift != self.lift_deg {
            self.lift_deg = lift;
            self.rev.settings += 1;
        }
    }

    pub(super) fn put_calibre(&mut self, name: String) {
        if name != self.calibre {
            self.calibre = name;
            self.rev.settings += 1;
        }
    }

    pub(super) fn put_bph(&mut self, bph: Option<u32>) {
        if bph != self.bph {
            self.bph = bph;
            self.rev.settings += 1;
        }
    }

    /// The take's own clock and where it came from, in core's words.
    pub(super) fn put_take_clock(
        &mut self,
        clock: Option<timegrapher_core::clock::ClockFit>,
        source: Option<String>,
    ) {
        self.take_clock = clock;
        self.take_clock_source = source;
        if let Some(l) = self.live.as_mut() {
            l.set_rate_clock(Self::rate_clock(&self.take_clock));
        }
        self.rev.clock += 1;
    }

    /// The clock kept for the input listened to.
    pub(super) fn put_input_clock(&mut self, clock: Option<clockstore::DeviceClock>) {
        self.clock = clock;
        self.rev.clock += 1;
    }

    pub(super) fn put_take_breaks(
        &mut self,
        breaks: Option<Vec<timegrapher_core::breaks::Break>>,
        unlisted: Option<timegrapher_core::breaks::Unlisted>,
    ) {
        self.take_breaks = breaks;
        self.take_unlisted = unlisted;
        self.rev.breaks += 1;
    }

    pub(super) fn put_masked(&mut self, masked: Vec<(f64, f64)>) {
        if masked != self.masked {
            self.masked = masked;
            self.rev.masks += 1;
        }
    }

    pub(super) fn note_settings(&mut self) {
        if self.session.recording_fault().is_some() {
            return;
        }
        if let Some(r) = self.session.recorder.as_mut() {
            let info = SessionInfo {
                watch: self.watch.clone(),
                position: self.position.code().into(),
                lift_deg: self.lift_deg,
                bph: self.bph,
                device: String::new(),
                sample_rate: 0,
                bits: 0,
                software: String::new(),
            };
            let _ = r.set_info(info);
        }
    }
}
