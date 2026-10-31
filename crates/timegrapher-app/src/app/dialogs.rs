//! The dialogs and notices: confirmations, the setup sheet, keeping a test, what Open asks, and the update, kept, crash and error notices.

use super::*;

impl TimegrapherApp {
    /// The question before throwing away unsaved sound.
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    pub(super) fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(what) = self.confirm.clone() else {
            return;
        };
        // A save is being written (its Save… started it, or one before):
        // wait for it. Once nothing is left unsaved, carry on; otherwise
        // (it failed, and the status bar says why, or there is sound since)
        // ask as before.
        if self.saving.is_some() {
            return self.confirm_saving_dialog(ctx);
        }
        let ask = match what {
            Confirm::PickWatch(_) => self.line_pick_asks(),
            Confirm::NewTake(_) => self.new_take_asks(),
            Confirm::ReserveStart => self.line_pick_asks(),
            _ => self.needs_save_confirmation(),
        };
        if !ask {
            self.confirm = None;
            return self.confirmed(what, ctx);
        }
        let mut choice = None;
        let unsaved = length_text(self.unsaved_s());
        let metadata = self.metadata_unsaved();
        let masks = self.marks_unsaved();
        let fault = self.session.recording_fault().map(|f| f.description());
        egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(
                RichText::new(match what {
                    Confirm::NewSession => "Start a new session?",
                    Confirm::NewTake(_) | Confirm::ReserveStart => {
                        "Start a new take without saving?"
                    }
                    // A watch picked on the Watch line is the next take's.
                    Confirm::PickWatch(_) => "Start a new take without saving?",
                    Confirm::Quit => "Quit without saving?",
                    Confirm::Open { .. } => "Open the recording without saving?",
                    Confirm::Microphone | Confirm::QuickListen => {
                        "Start listening without saving?"
                    }
                })
                .font(theme::semibold(15.0)),
            );
            ui.add_space(4.0);
            ui.label(if let Some(fault) = &fault {
                fault.clone()
            } else if masks {
                "Changes to the masked stretches or positions haven't been saved to the \
                 recording's session file and will be thrown away."
                    .to_string()
            } else if metadata && self.unsaved_s() <= 0.0 {
                "Changes to this session's settings or position history haven't been saved and will be thrown away.".to_string()
            } else if metadata {
                format!("This session's {unsaved} of sound and changes to its settings or position history haven't been saved and will be thrown away.")
            } else {
                format!("This session's {unsaved} of sound hasn't been saved and will be thrown away.")
            });
            ui.add_space(8.0);
            {
                let save = if masks { "Save" } else { "Save…" };
                let go = match (&fault, &what) {
                    (Some(_), Confirm::Quit) => "Leave Files and Quit",
                    (Some(_), _) => "Leave Files and Continue",
                    (
                        None,
                        Confirm::NewSession
                        | Confirm::NewTake(_)
                        | Confirm::ReserveStart
                        | Confirm::PickWatch(_)
                        | Confirm::Open { .. }
                        | Confirm::Microphone
                        | Confirm::QuickListen,
                    ) => "Don't Save",
                    (None, Confirm::Quit) => "Quit Without Saving",
                };
                // With the files at fault there is nothing to save:
                // carrying on is the default.
                let buttons: Vec<DialogButton> = if fault.is_none() {
                    vec![
                        DialogButton::new(Role::Default, save, ""),
                        DialogButton::new(Role::Destructive, go, ""),
                        DialogButton::new(Role::Cancel, "Cancel", ""),
                    ]
                } else {
                    vec![
                        DialogButton::new(Role::Default, go, ""),
                        DialogButton::new(Role::Cancel, "Cancel", ""),
                    ]
                };
                let skip = usize::from(fault.is_some());
                choice = theme::dialog_buttons(ui, &buttons).map(|i| i + skip);
            }
        });
        match choice {
            Some(0) if masks => {
                self.save_marks();
                if self.needs_save_confirmation() {
                    return; // saving failed
                }
            }
            Some(0) => {
                // Saved before: the rest goes to the same copy.
                self.save_again();
                if self.saving.is_some() {
                    return; // asked again, or carried on, once it is written
                }
                if self.needs_save_confirmation() {
                    return; // the folder dialog was cancelled, or saving failed
                }
            }
            Some(1) => {}
            Some(_) => {
                self.confirm = None;
                self.guided_after_ask = None;
                return;
            }
            None => return,
        }
        self.confirm = None;
        self.confirmed(what, ctx);
    }

    /// The question before throwing away unsaved sound, while a save is
    /// written.
    fn confirm_saving_dialog(&mut self, ctx: &egui::Context) {
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(RichText::new("Saving…").font(theme::semibold(15.0)));
            });
            ui.add_space(4.0);
            ui.label(
                "The sound so far is being copied into its take. This carries on once it is saved.",
            );
            ui.add_space(8.0);
            cancel = theme::dialog_buttons(
                ui,
                &[DialogButton::new(
                    Role::Cancel,
                    "Cancel",
                    "Stay here; the save carries on",
                )],
            )
            .is_some();
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        if cancel {
            self.confirm = None;
            self.guided_after_ask = None;
        }
    }

    /// Do what the question before throwing away unsaved sound asked about.
    pub(super) fn confirmed(&mut self, what: Confirm, ctx: &egui::Context) {
        match what {
            Confirm::NewSession => self.new_session(),
            Confirm::NewTake(kind) => self.new_take(kind, true),
            Confirm::ReserveStart => self.start_reserve_asked(true),
            Confirm::PickWatch(id) => {
                // Asked over a Quick Listen still listening: it stops now.
                self.pause();
                self.line_pick(id, true)
            }
            Confirm::Open { path } => self.open_recording(&path, false),
            Confirm::Microphone => self.listen_after_ask(),
            Confirm::QuickListen => self.quick_listen(true),
            Confirm::Quit => {
                self.may_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// Whether the window's go button can act: a recording just opened,
    /// or one going into a watch with everything `need` asks given.
    pub(super) fn open_ready(a: &OpenAsk, need: OpenNeed) -> bool {
        let into =
            (a.watch.is_some() && !a.new_watch) || (a.new_watch && !a.new_name.trim().is_empty());
        !a.add
            || (into
                && (!need.bph || a.bph.is_some())
                && (!need.position || a.position.is_some())
                && (!need.started || parse_local_minute(&a.started).is_some()))
    }

    /// The window Open asks in, while it is open.
    /// Set Up the Microphone, at the first launch: which microphone, and
    /// its level set against the watch on the stand, in three numbered
    /// steps, listening all the while so the meter moves. Done keeps
    /// listening; Not Now stops, and the sheet doesn't come back by itself.
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    pub(super) fn setup_sheet_dialog(&mut self, ctx: &egui::Context) {
        if !self.setup_sheet || self.open_ask.is_some() || self.confirm.is_some() {
            return;
        }
        // Listen as soon as the sheet shows, once: a failure says why in
        // its status line, and choosing another microphone tries again.
        if !self.setup_tried && !self.running() {
            self.setup_tried = true;
            self.quick_listen(true);
        }
        self.refresh_gain();
        let before = self.device.clone();
        let (mut done, mut later) = (false, false);
        let modal = egui::Modal::new(egui::Id::new("set up the microphone")).show(ctx, |ui| {
            ui.set_width(SHEET_W);
            let pal = theme::pal(ui);
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(RichText::new("Set Up the Microphone").font(theme::semibold(16.0)));
            ui.add_space(4.0);
            sheet_step(ui, 1, "Choose the microphone the watch sits on");
            sheet_indent(ui, |ui| {
                self.device_menu(ui, ui.available_width(), false);
                let hint =
                    "List every device the system offers in the microphone menu, with its id";
                theme::switch_row(ui, "Show Every Input", &mut self.all_inputs, hint);
            });
            ui.add_space(4.0);
            sheet_step(ui, 2, "Put a ticking watch on it");
            sheet_indent(ui, |ui| {
                ui.label(
                    RichText::new("The Peak meter below moves with each tick.")
                        .size(12.0)
                        .color(pal.text_secondary),
                );
            });
            ui.add_space(4.0);
            sheet_step(
                ui,
                3,
                "Set the Input Level so the peaks sit in the shaded band",
            );
            sheet_indent(ui, |ui| {
                sheet_field(ui, "Input Level", LEVEL_HINT, |ui| {
                    let w = ui.available_width();
                    self.level_control(ui, w);
                });
                if self.agc_read.is_some() {
                    sheet_field(ui, "Automatic Gain", AGC_HINT, |ui| {
                        ui.horizontal_top(|ui| {
                            self.agc_control(ui);
                            // Always there, so the switch moves nothing.
                            theme::alert_label(ui, theme::Alert::Warning, AGC_WARNING, true);
                        });
                    });
                }
                sheet_field(ui, "Peak", PEAK_HINT, |ui| {
                    let w = ui.available_width();
                    self.peak_control(ui, w);
                });
                sheet_field(ui, "", PEAK_HINT, |ui| {
                    ui.label(
                        RichText::new(BAND_WORDS)
                            .size(12.0)
                            .color(pal.text_secondary),
                    );
                });
            });
            ui.add_space(4.0);
            self.sheet_status(ui);
            ui.add_space(8.0);
            let w = ui.available_width();
            ui.allocate_ui_with_layout(
                Vec2::new(w, 34.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    // Narrower than a dialog's, so the notes beside them
                    // keep to a line each.
                    let narrow = Vec2::new(64.0, 26.0);
                    done = ui
                        .add(theme::primary(ui, "Done").min_size(narrow))
                        .on_hover_text("Keep listening to this microphone, with no watch picked")
                        .clicked();
                    later = ui
                        .add(theme::secondary(ui, "Not Now").min_size(narrow))
                        .on_hover_text(
                            "Stop listening and close this. Set up the microphone later in \
                             Settings > Microphone.",
                        )
                        .clicked();
                    // What follows, in what is left at the left.
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for line in [
                            "From now on the app listens as soon as it opens.",
                            "Change these in Settings > Microphone.",
                        ] {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("• {line}"))
                                        .size(11.0)
                                        .color(pal.text_secondary),
                                )
                                .wrap(),
                            );
                        }
                    });
                },
            );
        });
        // Return presses Done, the default; Esc presses Not Now.
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) && ctx.memory(|m| m.focused().is_none()) {
            done = true;
        }
        if modal.should_close() {
            later = true;
        }
        if self.device != before && self.running() && self.session.mic() {
            // Another microphone chosen: listen to that one instead.
            self.stop_listening();
            self.quick_listen(true);
        }
        if done {
            self.setup_sheet = false;
            self.mic_set_up = true;
            if !self.running() {
                self.quick_listen(true);
            }
        } else if later {
            self.setup_sheet = false;
            self.setup_declined = true;
            self.message = None;
            if self.running() && self.session.mic() {
                self.stop_listening();
            }
        }
    }

    /// The sheet's status line, two lines tall whatever it says: a
    /// failure, a warning such as clipping, the Live dot while listening,
    /// or "Not listening.".
    fn sheet_status(&self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let failure = match &self.message {
            Some((m, true)) => Some(m.clone()),
            _ => self.gain_error.clone(),
        };
        let listening = self.running() && self.session.mic();
        let w = ui.available_width();
        ui.allocate_ui_with_layout(
            Vec2::new(w, SHEET_STATUS_H),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_size(Vec2::new(w, SHEET_STATUS_H));
                if let Some(m) = failure {
                    // Two lines at most, the whole of it on hover.
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 5.0;
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
                        theme::alert_icon(ui.painter(), rect, theme::Alert::Bad, pal);
                        let mut job = egui::text::LayoutJob::simple(
                            m.clone(),
                            egui::FontId::proportional(13.0),
                            pal.text,
                            ui.available_width(),
                        );
                        job.wrap.max_rows = 2;
                        job.wrap.overflow_character = Some('…');
                        // Laid out here, so the label keeps the two rows.
                        let galley = ui.fonts_mut(|f| f.layout_job(job));
                        ui.add(egui::Label::new(galley)).on_hover_text(m);
                    });
                } else if listening && self.clipping() {
                    theme::alert_label(ui, theme::Alert::Warning, CLIPPING_WORDS, false);
                } else if let Some(m) = self.agc_message() {
                    theme::alert_label(ui, theme::Alert::Warning, &m, false);
                } else if listening {
                    ui.horizontal(|ui| {
                        theme::dot(ui, pal.live);
                        ui.label(
                            RichText::new(format!("Listening to {}.", self.picked_label()))
                                .color(pal.text_secondary),
                        );
                    });
                } else {
                    ui.label(RichText::new("Not listening.").color(pal.text_secondary));
                }
            },
        );
    }

    /// A test that ended with no watch picked: Keep in Unsorted, Keep in a
    /// Watch… or Throw Away.
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    pub(super) fn test_keep_dialog(&mut self, ctx: &egui::Context) {
        let Some(kind) = self.test_keep else {
            return;
        };
        if self.watch_form.is_some() || self.confirm.is_some() {
            return;
        }
        let watches: Vec<(String, String)> = match &self.library {
            Ok(Some(l)) if l.read_only.is_none() => l
                .watches
                .iter()
                .map(|e| (e.watch.id.clone(), e.watch.name.clone()))
                .collect(),
            _ => Vec::new(),
        };
        let name = match kind {
            TestKeep::Positions => "Positions Test",
            TestKeep::PowerReserve => "Power Reserve Test",
        };
        let mut answer: Option<Option<Option<String>>> = None;
        let mut new = false;
        // At the top or foot of the window, whichever is clear of the
        // test's pane, over a light backdrop, so its results stay in view
        // while it asks.
        let id = egui::Id::new("keep the test");
        let pane = self.results_rects[match kind {
            TestKeep::Positions => 0,
            TestKeep::PowerReserve => 1,
        }];
        let screen = ctx.content_rect();
        let anchor = match pane {
            Some(r) if r.center().y < screen.center().y => {
                (egui::Align2::CENTER_BOTTOM, [0.0, -48.0])
            }
            _ => (egui::Align2::CENTER_TOP, [0.0, 48.0]),
        };
        let area = egui::Modal::default_area(id).anchor(anchor.0, anchor.1);
        egui::Modal::new(id)
            .area(area)
            .backdrop_color(egui::Color32::from_black_alpha(40))
            .show(ctx, |ui| {
                ui.set_width(420.0);
                let pal = theme::pal(ui);
                ui.label(RichText::new(format!("Keep This {name}?")).font(theme::semibold(15.0)));
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "No watch was picked, so this {name} isn't kept anywhere yet. Unsorted \
                     holds it until you file it under a watch or delete it."
                    ))
                    .color(pal.text_secondary),
                );
                ui.add_space(8.0);
                // As a dialog's button row (see theme::dialog_buttons), with
                // a menu on Keep in a Watch…: the default at the right,
                // Throw Away at the far left.
                let w = ui.available_width();
                ui.allocate_ui_with_layout(
                    Vec2::new(w, 26.0),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if ui
                            .add(theme::primary(ui, "Keep in Unsorted"))
                            .on_hover_text(
                                "Keep the take in Unsorted, in the Watches view, to file under \
                                 a watch later",
                            )
                            .clicked()
                        {
                            answer = Some(Some(None));
                        }
                        let keep = ui
                            .add(theme::secondary(ui, "Keep in a Watch…"))
                            .on_hover_text(
                                "Keep the take in one of your watches. It keeps the settings it \
                                 ran with; the watch's own apply to its next take.",
                            );
                        egui::Popup::menu(&keep).show(|ui| {
                            for (id, name) in &watches {
                                if ui.button(name).clicked() {
                                    answer = Some(Some(Some(id.clone())));
                                }
                            }
                            if !watches.is_empty() {
                                ui.separator();
                            }
                            if ui
                                .button("New Watch…")
                                .on_hover_text(
                                    "Add a watch, and keep the take in it when you add it",
                                )
                                .clicked()
                            {
                                new = true;
                            }
                        });
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if ui
                                .add(theme::destructive(ui, "Throw Away"))
                                .on_hover_text("Delete the take: its sound and its readings")
                                .clicked()
                            {
                                answer = Some(None);
                            }
                        });
                    },
                );
                // Return keeps it in Unsorted, the default.
                if answer.is_none()
                    && theme::dialog_keys(ui, &[DialogButton::new(Role::Default, "", "")]).is_some()
                {
                    answer = Some(Some(None));
                }
            });
        if new {
            self.open_new_watch_form();
            self.keep_after_form = true;
            return;
        }
        match answer {
            Some(Some(watch)) => {
                self.test_keep = None;
                self.keep_test(kind, watch.as_deref());
            }
            Some(None) => {
                self.test_keep = None;
                self.new_session();
            }
            None => {}
        }
    }

    /// Keep the test's take in the watch `watch`, or in Unsorted with none.
    pub(super) fn keep_test(&mut self, kind: TestKeep, watch: Option<&str>) {
        self.test_keep = None;
        match (kind, watch) {
            (TestKeep::Positions, Some(id)) => self.keep_in_watch(id),
            (TestKeep::Positions, None) => {
                self.watch_open = None;
                self.unsorted_open = true;
                self.save_again();
            }
            (TestKeep::PowerReserve, watch) => {
                self.watch_open = watch.map(str::to_string);
                self.unsorted_open = watch.is_none();
                // The take is named for the watch it goes into.
                if let Some(e) = self.open_watch() {
                    self.watch = e.watch.name.clone();
                }
                self.save_reserve();
            }
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    pub(super) fn open_ask_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut a) = self.open_ask.take() else {
            return;
        };
        let watches: Vec<(String, String)> = match &self.library {
            Ok(Some(l)) => l
                .watches
                .iter()
                .map(|e| (e.watch.id.clone(), e.watch.name.clone()))
                .collect(),
            _ => Vec::new(),
        };
        let need = self.open_need(&a);
        let asks = Self::open_asks(&a, need);
        let started = parse_local_minute(&a.started);
        let ready = Self::open_ready(&a, need);
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::new("open question")).show(ctx, |ui| {
            ui.set_width(440.0);
            let pal = theme::pal(ui);
            let title = if a.keeping {
                format!("Keep \"{}\" in a Watch", file_name(&a.path))
            } else {
                format!("Open \"{}\"", file_name(&a.path))
            };
            ui.label(RichText::new(title).font(theme::semibold(15.0)));
            // Already on show: only keeping it is asked about.
            if !a.keeping {
                theme::segmented(
                    ui,
                    &mut a.add,
                    &[
                        (
                            true,
                            "Add to a Watch",
                            "Copy the recording into one of your watches, where it is kept with \
                         the watch's takes. The original stays where it is.",
                        ),
                        (
                            false,
                            "Just Open It",
                            "Open the recording where it is. It is not added to your watches: \
                         Save, on the Listen card, keeps it in a watch later.",
                        ),
                    ],
                );
            }
            if a.add {
                let shown = if a.new_watch {
                    "New Watch…"
                } else {
                    a.watch
                        .as_ref()
                        .and_then(|id| watches.iter().find(|(w, _)| w == id))
                        .map_or("Choose a Watch", |(_, n)| n.as_str())
                };
                theme::row(
                    ui,
                    "Watch",
                    Some("The watch the recording is added to"),
                    |ui| {
                        egui::ComboBox::from_id_salt("open into watch")
                            .width(ui.available_width())
                            .selected_text(shown)
                            .show_ui(ui, |ui| {
                                for (id, name) in &watches {
                                    let on = !a.new_watch && a.watch.as_ref() == Some(id);
                                    if ui.selectable_label(on, name).clicked() {
                                        a.watch = Some(id.clone());
                                        a.new_watch = false;
                                    }
                                }
                                if ui.selectable_label(a.new_watch, "New Watch…").clicked() {
                                    a.new_watch = true;
                                }
                            });
                    },
                );
                if a.new_watch {
                    theme::row(
                        ui,
                        "Name",
                        Some(
                            "The new watch's name. Its other details can be added later \
                             with Edit… in the Watches view.",
                        ),
                        |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut a.new_name)
                                    .desired_width(f32::INFINITY),
                            );
                        },
                    );
                }
                if asks == OpenNeed::default() {
                    ui.label(
                        RichText::new("Its files and the watch say everything a take needs.")
                            .color(pal.text_secondary),
                    );
                } else {
                    ui.label(
                        RichText::new(
                            "Neither its files nor the watch say these, so they are saved \
                             with it:",
                        )
                        .color(pal.text_secondary),
                    );
                }
                if asks.bph {
                    theme::row(ui, "Beat Rate", Some("Beats per hour"), |ui| {
                        egui::ComboBox::from_id_salt("open bph")
                            .width(ui.available_width())
                            .selected_text(match a.bph {
                                Some(b) => format!("{b} bph"),
                                None => "Choose a Beat Rate".into(),
                            })
                            .show_ui(ui, |ui| {
                                for b in STANDARD_BPH {
                                    ui.selectable_value(&mut a.bph, Some(b), format!("{b} bph"));
                                }
                            });
                    });
                }
                if asks.lift {
                    let changed = theme::row(
                        ui,
                        "Lift Angle",
                        Some(
                            "The calibre's lift angle in degrees: type it and press Enter, or \
                             pick a common one from the menu. Amplitude depends on it.",
                        ),
                        |ui| {
                            let x = fields::entry(
                                ui,
                                "open lift",
                                &mut a.lift,
                                timegrapher_core::input_domain::LIFT_RANGE_DEG,
                                &MS,
                                44.0,
                            );
                            ui.label("°");
                            let y = fields::presets(
                                ui,
                                "open lift presets",
                                &mut a.lift,
                                &LIFTS,
                                |v| format!("{}°", fields::plain(v)),
                            );
                            x || y
                        },
                    );
                    a.lift_set |= changed;
                }
                if asks.position {
                    theme::row(
                        ui,
                        "Position",
                        Some("The position the watch was in"),
                        |ui| {
                            egui::ComboBox::from_id_salt("open position")
                                .width(ui.available_width())
                                .selected_text(
                                    a.position.map_or("Choose a Position", |p| p.label()),
                                )
                                .show_ui(ui, |ui| {
                                    for p in Position::ALL {
                                        ui.selectable_value(&mut a.position, Some(p), p.label());
                                    }
                                });
                        },
                    );
                }
                if asks.started {
                    theme::row(
                        ui,
                        "Started",
                        Some(
                            "When the recording started, in this computer's time, as \
                             2026-10-22 14:30. Filled in from when its file was last written, \
                             less its length: correct it if the file was copied since.",
                        ),
                        |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut a.started)
                                    .desired_width(f32::INFINITY),
                            );
                        },
                    );
                    if started.is_none() {
                        ui.label(
                            RichText::new("Write it as 2026-10-22 14:30.")
                                .small()
                                .color(pal.text_secondary),
                        );
                    }
                }
            }
            if let Some(e) = &a.error {
                theme::alert_label(ui, theme::Alert::Bad, e, false);
            }
            ui.add_space(6.0);
            let go = match (a.keeping, a.add) {
                (true, _) => "Keep",
                (false, true) => "Add and Open",
                (false, false) => "Open",
            };
            let cancel_hint = if a.keeping {
                "Close without keeping it"
            } else {
                "Close without opening anything"
            };
            if let Some(i) = theme::dialog_buttons(
                ui,
                &[
                    DialogButton::new(Role::Default, go, "")
                        .enabled_unless(ready, "Fill in what is asked for first"),
                    DialogButton::new(Role::Cancel, "Cancel", cancel_hint),
                ],
            ) {
                answer = Some(i == 0);
            }
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(false);
        }
        match answer {
            Some(true) => self.answer_open_ask(a, need),
            Some(false) => self.keep_cancelled(),
            None => self.open_ask = Some(a),
        }
    }

    /// Open `a`'s recording where it is, or add it to its watch first and
    /// open the take it makes there. A failed add leaves the question open,
    /// saying why.
    pub(super) fn answer_open_ask(&mut self, mut a: OpenAsk, need: OpenNeed) {
        if !a.add {
            self.keep_back = None;
            return self.open_recording(&a.path, true);
        }
        match self.add_opened(&a, need) {
            Ok((take, id, fill_error)) => {
                self.keep_back = None;
                self.watch_open = Some(id);
                self.unsorted_open = false;
                match fill_error {
                    Some(e) => self.error(e),
                    None => self.info_msg(format!(
                        "Added \"{}\" to the watch's takes.",
                        file_name(&take)
                    )),
                }
                self.open_recording(&take, true);
            }
            Err(e) => {
                a.error = Some(e);
                self.open_ask = Some(a);
            }
        }
    }

    /// Copy `a`'s recording into its watch (a new one made first, when
    /// asked for), with what `need` says is missing written into its
    /// session files. Returns the take, the watch's id, and why what was
    /// missing couldn't be written, when the take is in but that failed.
    fn add_opened(
        &mut self,
        a: &OpenAsk,
        need: OpenNeed,
    ) -> Result<(PathBuf, String, Option<String>), String> {
        let inner = take_folder(&a.path)?;
        let files = recordings_in(&inner)?;
        let started = if need.started {
            Some(recorder::utc(
                parse_local_minute(&a.started)
                    .ok_or("Write when it started as 2026-10-22 14:30.")?,
            ))
        } else {
            None
        };
        let fill = Fill {
            bph: if need.bph { a.bph } else { None },
            lift: need.lift.then_some((a.lift, a.lift_set)),
            position: if need.position { a.position } else { None },
            started_utc: started,
            duration_s: files.iter().filter_map(|f| audio_length(f)).sum(),
        };
        let Ok(Some(lib)) = &mut self.library else {
            return Err("There is no Watches folder.".into());
        };
        let id = if a.new_watch {
            let w = library::Watch::new(&a.new_name);
            lib.add(&w)?;
            w.id
        } else {
            a.watch.clone().ok_or("Choose a watch first.")?
        };
        let watch = lib
            .watches
            .iter()
            .find(|e| e.watch.id == id)
            .map(|e| e.watch.clone())
            .ok_or("That watch is no longer in your watches")?;
        if a.has.manifest {
            let dir = if inner.is_dir() {
                inner.clone()
            } else {
                inner
                    .parent()
                    .ok_or("The recording has no folder")?
                    .to_path_buf()
            };
            let (take, _) = lib.add_take(&dir, &id, true)?;
            let filled = fill_take(&take, &watch, &fill).err().map(|e| {
                format!("The take is in the watch, but what it was missing couldn't be saved with it: {e}")
            });
            lib.scan();
            return Ok((take, id, filled));
        }
        let named =
            position_in_name(&inner).or_else(|| files.first().and_then(|f| position_in_name(f)));
        let position = fill.position.or(named);
        let lift = fill.lift.or(watch.lift.map(|l| (l, true)));
        let m = Manifest {
            watch: Some(watch.name.clone()),
            calibre: watch.calibre.clone().filter(|c| !c.is_empty()),
            bph: fill.bph.or(watch.bph),
            lift: lift.map(|l| l.0),
            lift_known: lift.map(|l| l.1),
            recordings: vec![RecordingEntry {
                file: ".".into(),
                position: position.map(|p| p.code().into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let toml = toml::to_string_pretty(&m).map_err(|e| e.to_string())?;
        let dir_json = inner.is_dir().then(|| inner.join("session.json"));
        let json = with_start(dir_json.as_deref(), &fill)?;
        let started = fill
            .started_utc
            .clone()
            .or_else(|| started_in(&inner))
            .unwrap_or_default();
        let minute = watches::local_minute(&started).unwrap_or_else(|| started.replace(':', "."));
        let name = library::take_name(
            &minute,
            &position
                .and_then(|p| timegrapher_core::session::Position::parse(p.code()))
                .into_iter()
                .collect::<Vec<_>>(),
        );
        let take = lib.add_recording(
            &inner,
            &id,
            &name,
            &[(MANIFEST, toml), ("session.json", json)],
        )?;
        Ok((take, id, None))
    }

    /// A newer release is published: its version and a button to its page.
    pub(super) fn update_notice(&mut self, ui: &mut egui::Ui) {
        let Some(r) = self.updates.notice.clone() else {
            return;
        };
        let (mut download, mut dismiss) = (false, false);
        notice(
            ui,
            None,
            &format!("Timegrapher {} is available", r.version),
            |ui| {
                dismiss = ui
                    .add(egui::Button::new("Dismiss").frame_when_inactive(false))
                    .on_hover_text("Hide this until the app starts again")
                    .clicked();
                download = ui
                    .add(theme::primary(ui, "Download"))
                    .on_hover_text(
                        "Open the release's page in your browser, to download it from. \
                     Nothing installs itself.",
                    )
                    .clicked();
            },
        );
        if download {
            ui.ctx()
                .open_url(egui::OpenUrl::new_tab(crate::update::page(&r)));
        }
        if download || dismiss {
            self.updates.notice = None;
        }
        ui.add_space(theme::GAP - ui.spacing().item_spacing.y);
    }

    /// Sound kept from an earlier session that was never saved: Open,
    /// Move To… or Delete…, one kept recording at a time.
    pub(super) fn kept_notice(&mut self, ui: &mut egui::Ui) {
        let Some(k) = self.kept.first().cloned() else {
            return;
        };
        let (mut open, mut move_to, mut delete, mut later) = (false, false, false, false);
        let what = format!(
            "Sound from an earlier session wasn't saved{}{}, {}. Open it, move it into a \
             folder of your own, or delete it?",
            k.started
                .as_ref()
                .map_or(String::new(), |s| format!(": started {s}")),
            k.duration_s
                .map_or(String::new(), |d| format!(", {} long", length_text(d))),
            size_text(k.bytes),
        );
        notice(ui, Some(theme::Alert::Warning), &what, |ui| {
            later = ui
                .add(egui::Button::new("Later").frame_when_inactive(false))
                .on_hover_text("Leave it where it is: it is offered again next time")
                .clicked();
            delete = ui
                .add(egui::Button::new("Delete…"))
                .on_hover_text("Delete this sound for good. It asks first.")
                .clicked();
            move_to = ui
                .add(egui::Button::new("Move To…"))
                .on_hover_text(
                    "Move the recording's folder into a folder you pick, to open again \
                     with Open Folder… whenever you like",
                )
                .clicked();
            open = ui
                .add(theme::primary(ui, "Open"))
                .on_hover_text(
                    "Analyse it here. It stays where it is and is offered again next time \
                     until it is moved or deleted.",
                )
                .clicked();
        });
        if open {
            self.kept.remove(0);
            self.input = Input::File;
            self.file_path = k.dir.display().to_string();
            self.open_recording(&k.dir, true);
        } else if move_to {
            if let Some(parent) = rfd::FileDialog::new()
                .set_title("Move the Unsaved Sound Into a Folder")
                .pick_folder()
            {
                match move_kept(&k.dir, &parent) {
                    Ok(to) => {
                        self.kept.remove(0);
                        self.info_msg(format!("Moved the unsaved sound to {}.", to.display()));
                    }
                    Err(e) => self.error(e),
                }
            }
        } else if delete {
            self.kept_delete_ask = Some(k.dir.clone());
        } else if later {
            self.kept.clear();
        }
    }

    /// Delete… on kept sound asks first: it can't be got back.
    pub(super) fn kept_delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(dir) = self.kept_delete_ask.clone() else {
            return;
        };
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("delete kept sound")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new("Delete the unsaved sound?").font(theme::semibold(15.0)));
            ui.add_space(4.0);
            ui.label("It was never saved anywhere else, so it can't be got back.");
            ui.add_space(8.0);
            // Nothing that can't be got back is the default: Return does
            // nothing, and the button that throws it away is far from Cancel.
            if let Some(i) = theme::dialog_buttons(
                ui,
                &[
                    DialogButton::new(Role::Destructive, "Delete", ""),
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
            self.kept_delete_ask = None;
            if go {
                timegrapher_core::recorder::remove_stale_lock(&dir);
                match std::fs::remove_dir_all(&dir) {
                    Ok(()) => self.kept.retain(|k| k.dir != dir),
                    Err(e) => self.error(format!("Couldn't delete {}: {e}", dir.display())),
                }
            }
        }
    }

    /// After the app closed unexpectedly: an offer to report it. The report
    /// is a GitHub issue the user opens with the crash record pasted in;
    /// nothing is sent from the app.
    pub(super) fn crash_notice(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.crash.clone() else {
            return;
        };
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let (mut report, mut show, mut dismiss) = (false, false, false);
        notice(
            ui,
            Some(theme::Alert::Warning),
            "The timegrapher closed unexpectedly last time. Make a report?",
            |ui| {
                dismiss = ui
                    .add(egui::Button::new("Dismiss").frame_when_inactive(false))
                    .on_hover_text("Don't ask about this crash again")
                    .clicked();
                show = ui
                    .add(egui::Button::new(if self.crash_shown {
                        "Hide the Record"
                    } else {
                        "Show the Record"
                    }))
                    .on_hover_text("What the app wrote down as it closed")
                    .clicked();
                report = ui
                    .add(theme::primary(ui, "Make a Report…"))
                    .on_hover_text(
                        "Copy the crash record and open a new bug report on GitHub in \
                     your browser, to paste it into. Nothing is sent until you submit it.",
                    )
                    .clicked();
            },
        );
        if self.crash_shown {
            egui::ScrollArea::vertical()
                .id_salt("crash-record")
                .max_height(200.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(&text).monospace().small()).selectable(true),
                    );
                });
        }
        if show {
            self.crash_shown = !self.crash_shown;
        }
        if report {
            ui.ctx().copy_text(text);
            ui.ctx().open_url(egui::OpenUrl::new_tab(BUG_REPORT_URL));
            self.info_msg(
                "The crash record is on the clipboard: paste it into the report that opened \
                 in your browser."
                    .into(),
            );
        }
        if report || dismiss {
            applog::mark_seen(&path);
            self.crash = None;
            self.crash_shown = false;
        }
    }

    /// A problem, where it can't be missed, with a way to put it away.
    pub(super) fn error_notice(&mut self, ui: &mut egui::Ui) {
        if self.setup_sheet {
            // The sheet says it.
            return;
        }
        let fault = self.session.recording_fault().map(|f| f.description());
        let m = match (&fault, &self.message) {
            (Some(fault), Some((message, true))) => {
                let fault = plain_error(fault);
                if message.starts_with(&fault) {
                    message.clone()
                } else {
                    format!("{fault}\n{message}")
                }
            }
            (Some(fault), _) => fault.clone(),
            (None, Some((message, true))) => message.clone(),
            _ => return,
        };
        let mut dismiss = false;
        notice(ui, Some(theme::Alert::Bad), &m, |ui| {
            dismiss = fault.is_none()
                && ui
                    .add(egui::Button::new("Dismiss").frame_when_inactive(false))
                    .on_hover_text("Hide this message")
                    .clicked();
        });
        if dismiss {
            self.message = None;
        }
    }
}
