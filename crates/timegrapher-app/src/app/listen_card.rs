//! The Listen card: the position card, Listen and Stop, the Save menu, the watch and calibre lines, rate and lift, and getting a new take ready.

use super::*;

impl TimegrapherApp {
    /// The controls on the readings' cards, each in the rows its card
    /// left for it.
    pub(super) fn card_controls(&mut self, ui: &mut egui::Ui, out: &Readouts) {
        for &(slot, rect) in &out.slots {
            // A child of its own, not a scope: a scope would move the
            // row's cursor back up to the foot of what it drew, and the
            // panes below would be laid over the cards.
            let mut ui = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("card controls", slot))
                    .max_rect(rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.spacing_mut().interact_size.y = theme::CONTROL_H;
            match slot {
                Slot::Position => self.position_card(&mut ui, out.position_compact),
                Slot::Listen => self.listen_buttons(&mut ui, out.listen_compact),
                Slot::WatchLine => self.watch_line(&mut ui, &out.calibre_limits),
                Slot::Calibre => self.calibre_control(&mut ui),
                Slot::RateAndLift => self.rate_and_lift(&mut ui),
            }
        }
    }

    /// Whether the Position card sets the position: while listening, or
    /// before anything is on show, and never during a test, which sets it
    /// itself. A take's positions are its history.
    pub(super) fn position_editable(&self) -> bool {
        // A Positions Test finished, while listening goes on, sets nothing.
        let testing = (self.running() && self.guided_running()) || self.reserve_running();
        let listening = self.running() && self.session.mic();
        !testing && (listening || self.live.is_none())
    }

    /// Why the Position card's buttons can't be pressed now.
    pub(super) fn position_lock_words(&self) -> &'static str {
        if self.reserve_running() {
            "A Power Reserve Test runs in this one position: Stop Test to change it."
        } else if self.test_listening() {
            "The Positions Test sets the position."
        } else {
            "Shown, not set here: a take's positions are kept as they were, and during a \
             Positions Test the test sets them."
        }
    }

    /// The position the Position card shows: the one set while it can be
    /// set, else the take's at the moment the readings show.
    fn shown_position(&self) -> Position {
        if self.position_editable() || self.running() {
            return self.position;
        }
        let at = self.latest_s();
        self.session
            .stretches()
            .into_iter()
            .take_while(|&(t, _)| t <= at + 1e-6)
            .last()
            .map_or(self.position, |(_, p)| p)
    }

    /// The Current Position card, in `ui`'s rect inside its padding: the
    /// watch as it lies at the left, its caption and name beside it, and
    /// the six buttons, DU to CR, across the foot, the current one teal,
    /// each with a bar filling as that position is heard. On a compact
    /// card, the picture smaller, the two-letter code beside it, and one pop-up of
    /// the six instead of the buttons. The buttons set the position in a
    /// free live take; on a take on show or during a test they only show
    /// it.
    pub(super) fn position_card(&mut self, ui: &mut egui::Ui, compact: bool) {
        self.position_card_title = if compact {
            "Position"
        } else {
            "Current Position"
        };
        let pal = theme::pal(ui);
        let here = self.shown_position();
        let editable = self.position_editable();
        let rows = CardRows::of(ui.max_rect().expand(f32::from(theme::CARD_PADDING)));
        let settle = format!(
            "Each change of position settles for {} s (Settle, on the Positions Test pane), \
             and those seconds are left out of the readings.",
            fields::plain(self.guided_settle_s)
        );
        let codes = format!("{} · {}", here.code(), here.witschi());
        let side = if compact {
            POSITION_PIC_COMPACT
        } else {
            POSITION_PIC
        };
        let pic = if compact {
            egui::Rect::from_min_size(
                egui::pos2(rows.figure.left(), rows.figure.center().y - side / 2.0),
                Vec2::splat(side),
            )
        } else {
            // Across the title and figure rows, centred in their height.
            let top = rows.title.top();
            let h = rows.figure.bottom() - top;
            egui::Rect::from_min_size(
                egui::pos2(rows.inner.left(), top + (h - side) / 2.0),
                Vec2::splat(side),
            )
        };
        crate::icons::watch(
            ui.painter(),
            pic,
            here.lying(),
            pal.positions,
            pal.text,
            pal.card,
        );
        ui.interact(pic, ui.id().with("position picture"), egui::Sense::hover())
            .on_hover_text(if editable {
                format!("{}. {POSITION_HINT} {settle}", here.label())
            } else {
                format!("{}: {} {settle}", here.label(), self.position_lock_words())
            });
        let painter = ui.painter().clone();
        if compact {
            return self.position_card_compact(ui, &rows, pic, &codes, &settle);
        }
        let x = pic.right() + theme::GAP;
        let beside = |r: egui::Rect| egui::Rect::from_min_max(egui::pos2(x, r.top()), r.max);
        position_caption(ui, beside(rows.title), self.position_card_title);
        // The name, and both its codes under it, inside the figure row.
        let fig = beside(rows.figure);
        let clip = painter.with_clip_rect(fig);
        clip.text(
            egui::pos2(fig.left(), fig.center().y + 3.0),
            egui::Align2::LEFT_BOTTOM,
            here.name(),
            theme::semibold(17.0),
            pal.text,
        );
        clip.text(
            egui::pos2(fig.left(), fig.center().y + 4.0),
            egui::Align2::LEFT_TOP,
            &codes,
            egui::FontId::proportional(12.0),
            pal.text_secondary,
        );
        // The six buttons, a segmented control across the card.
        let track = rows.controls;
        painter.rect_filled(
            track,
            egui::CornerRadius::same(theme::CONTROL_RADIUS),
            pal.control,
        );
        let picked = self.position_buttons(ui, track, &settle);
        if let Some(p) = picked {
            if p != self.position {
                self.change_position(p);
            }
        }
    }

    /// The Current Position card on a compact card: the two-letter code
    /// beside the picture `pic`, and one pop-up of the six where the
    /// position can be set.
    fn position_card_compact(
        &mut self,
        ui: &mut egui::Ui,
        rows: &CardRows,
        pic: egui::Rect,
        codes: &str,
        settle: &str,
    ) {
        let pal = theme::pal(ui);
        let here = self.shown_position();
        let editable = self.position_editable();
        let painter = ui.painter().clone();
        position_caption(ui, rows.title, self.position_card_title);
        // The two-letter code beside the picture, the name in its
        // hover: a name doesn't fit a compact card whole.
        let x = pic.right() + 8.0;
        let code = painter.with_clip_rect(rows.figure).text(
            egui::pos2(x, rows.figure.center().y),
            egui::Align2::LEFT_CENTER,
            here.code(),
            theme::semibold(20.0),
            pal.text,
        );
        ui.interact(code, ui.id().with("position code"), egui::Sense::hover())
            .on_hover_text(format!("{} ({codes})", here.name()));
        // The six in one pop-up; keys 1 to 6 still pick them. Where
        // the position can't be set (a take on show, a test running),
        // no pop-up: the code beside the picture says it all.
        if !editable {
            return;
        }
        let mut picked = here;
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("position menu")
                .max_rect(rows.controls)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.spacing_mut().interact_size.y = theme::CONTROL_H;
        child
            .add_enabled_ui(editable, |ui| {
                // The code is beside the picture already: the pop-up
                // says what it does instead.
                egui::ComboBox::from_id_salt("position")
                    .width(ui.available_width())
                    .selected_text("Set")
                    .show_ui(ui, |ui| {
                        for (n, p) in Position::ALL.into_iter().enumerate() {
                            ui.selectable_value(
                                &mut picked,
                                p,
                                format!("{} ({}) · key {}", p.name(), p.code(), n + 1),
                            );
                        }
                    })
                    .response
            })
            .inner
            .on_hover_text(if editable {
                format!("{codes}. {POSITION_HINT} {settle}")
            } else {
                format!("{codes}: {}", self.position_lock_words())
            });
        if picked != here && picked != self.position {
            self.change_position(picked);
        }
    }

    /// The six position buttons across the Current Position card's
    /// `track`, each with a bar filling as that position is heard; the one
    /// clicked, where the position can be set.
    fn position_buttons(
        &self,
        ui: &mut egui::Ui,
        track: egui::Rect,
        settle: &str,
    ) -> Option<Position> {
        let pal = theme::pal(ui);
        let here = self.shown_position();
        let editable = self.position_editable();
        let painter = ui.painter().clone();
        let w = (track.width() - 4.0) / 6.0;
        let mut picked = None;
        for (n, p) in Position::ALL.into_iter().enumerate() {
            let r = egui::Rect::from_min_size(
                egui::pos2(track.left() + 2.0 + w * n as f32, track.top() + 2.0),
                Vec2::new(w, track.height() - 4.0),
            )
            .shrink2(Vec2::new(1.0, 0.0));
            let sense = if editable {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            };
            let resp = ui.interact(r, ui.id().with(("position", n)), sense);
            let on = p == here;
            let fill = if on {
                pal.positions
            } else if editable && resp.hovered() {
                pal.control_hover
            } else {
                Color32::TRANSPARENT
            };
            painter.rect_filled(r, egui::CornerRadius::same(theme::CONTROL_RADIUS - 1), fill);
            let ink = if on {
                theme::ON_TEAL
            } else if editable {
                pal.text
            } else {
                pal.text_tertiary
            };
            painter.text(
                r.center() - Vec2::new(0.0, 1.0),
                egui::Align2::CENTER_CENTER,
                p.code(),
                egui::FontId::proportional(12.0),
                ink,
            );
            // A bar along the foot, filling as the position is heard, full
            // at the Positions Test's Measure time.
            let heard = self.recorded_in(p);
            let full = heard / self.guided_measure_s;
            if self.live.is_some() {
                // A faint full-length track, so a part fill reads as progress.
                let bar = egui::Rect::from_min_max(
                    egui::pos2(r.left() + 4.0, r.bottom() - 5.5),
                    egui::pos2(r.right() - 4.0, r.bottom() - 2.0),
                );
                let fill_ink = if on { ink } else { pal.positions };
                painter.rect_filled(bar, 1.5, fill_ink.gamma_multiply(0.2));
                let mut done = bar;
                done.set_width(bar.width() * full.min(1.0) as f32);
                painter.rect_filled(done, 1.5, fill_ink);
            }
            let tip = format!(
                "{} ({}, {}) · key {}. {} heard, settled and not masked; the bar is full at \
                 Each Position's {} (Positions Test)",
                p.name(),
                p.code(),
                p.witschi(),
                n + 1,
                fields::duration(heard.floor()),
                fields::duration(self.guided_measure_s)
            );
            let resp = if editable {
                resp.on_hover_text(format!("{tip}. {settle}"))
            } else {
                resp.on_hover_text(format!("{tip}. {}", self.position_lock_words()))
            };
            if resp.clicked() {
                picked = Some(p);
            }
        }
        picked
    }

    /// The Listen card's rows under its caption: Listen or Stop and Save,
    /// with the take's length and state beside them, then Average Over. On
    /// a compact card, the length goes in Listen's hover and Average Over
    /// loses its label to its hover.
    pub(super) fn listen_buttons(&mut self, ui: &mut egui::Ui, compact: bool) {
        let pal = theme::pal(ui);
        let area = ui.max_rect();
        let length = length_text(self.live.as_ref().map_or(0.0, |l| l.duration_s()));
        let state = self.state_words();
        let top = egui::Rect::from_min_size(area.min, Vec2::new(area.width(), theme::FIGURE_ROW_H));
        let foot = egui::Rect::from_min_max(
            egui::pos2(area.left(), area.bottom() - theme::CONTROL_H),
            area.max,
        );
        let mut buttons = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("listen buttons")
                .max_rect(top)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        buttons.spacing_mut().item_spacing.x = 8.0;
        let ui_b = &mut buttons;
        let length_hint = format!("\n\n{length} · {state}");
        let word = |ui: &mut egui::Ui, b: egui::Button<'_>| {
            ui.add(b.min_size(Vec2::new(0.0, LISTEN_BUTTON)))
        };
        let was_listen = self.listen_tap.take();
        if let Some(kind) = self.ready_test() {
            self.listen_start_test(ui_b, kind, compact.then_some(length_hint.as_str()));
        } else if self.test_listening() {
            let mut hint = "End the test now, keeping what it measured: asks first".to_string();
            if compact {
                hint += &length_hint;
            }
            if word(ui_b, theme::secondary(ui_b, "Stop Test…"))
                .on_hover_text(hint)
                .clicked()
            {
                self.stop_test_ask = true;
            }
        } else if self.running() && self.session.mic() {
            let mut hint = STOP_HINT.to_string();
            if compact {
                hint += &length_hint;
            }
            if theme::icon_button(ui_b, Icon::Stop, LISTEN_BUTTON, 8, pal.accent, true, false)
                .on_hover_text(hint)
                .clicked()
            {
                self.stop_listening();
            }
        } else {
            // With a watch picked, a take of it; else Quick Listen.
            let watch = self.open_watch().map(|e| e.watch.name.clone());
            let mut hint = match &watch {
                Some(name) => format!(
                    "Listen to {name}: a new take, saved into it at Stop. For a Quick \
                     Listen with no watch, pick No Watch (Quick Listen) on the Calibre \
                     card's title."
                ),
                None => QUICK_LISTEN_HINT.to_string(),
            };
            if compact {
                hint += &length_hint;
            }
            let (now, pressed) = ui_b.input(|i| (i.time, i.pointer.any_pressed()));
            let tap = listen_tap(was_listen, now, pressed);
            self.listen_tap = Some(tap);
            if tap.0.is_infinite() {
                ui_b.ctx().request_repaint(); // so the count starts unprompted
            }
            let armed = tap.1;
            if ui_b
                .add_enabled_ui(!self.running() && self.batch.is_none(), |ui| {
                    theme::icon_button(ui, Icon::Listen, LISTEN_BUTTON, 8, pal.accent, true, false)
                })
                .inner
                .on_hover_text(hint)
                .clicked()
                && armed
            {
                if watch.is_some() {
                    self.new_take(NewTake::Listen, false);
                } else {
                    // Asks only about what isn't Quick Listen's: changed
                    // masks on a recording opened, a watch's unsaved
                    // details.
                    self.quick_listen(false);
                }
            }
        }
        // A watch's take saves itself at Stop, and a test's at its end.
        let testing = self.ready_test().is_some() || self.test_listening();
        if (!self.session.mic() || self.quick_listen) && !testing {
            self.save_menu(ui_b, LISTEN_BUTTON);
        }
        if !compact {
            listen_length(ui_b, top, &length, state);
        }
        self.listen_average(ui, foot, compact);
    }

    /// The Listen card's Start Test, for the test of `kind` ready to
    /// start; `length_hint` adds the take's length to its hover on a
    /// compact card.
    fn listen_start_test(&mut self, ui_b: &mut egui::Ui, kind: NewTake, length_hint: Option<&str>) {
        // The one place a take starts, test or not.
        let name = if kind == NewTake::Positions {
            "Positions Test"
        } else {
            "Power Reserve Test"
        };
        let why = self.start_test_why_not();
        let mut hint = if kind == NewTake::Positions {
            "Start the Positions Test, from Dial Up".to_string()
        } else {
            format!(
                "Start the {name} in the position on the {} card and run until the watch \
                 stops, keeping the computer awake. A closed lid or Sleep from a menu still \
                 stops the computer; a laptop is best left on mains power.",
                self.position_card_title
            )
        };
        if let Some(h) = length_hint {
            hint += h;
        }
        let b = theme::primary(ui_b, "Start Test");
        let r = ui_b
            .add_enabled_ui(why.is_none(), |ui| {
                ui.add(b.min_size(Vec2::new(0.0, LISTEN_BUTTON)))
            })
            .inner;
        if r.on_hover_text(hint)
            .on_disabled_hover_text(why.unwrap_or_default())
            .clicked()
        {
            self.start_ready_test();
        }
    }

    /// The Listen card's Average Over, in its `foot` row; on a compact
    /// card without its label.
    fn listen_average(&mut self, ui: &mut egui::Ui, foot: egui::Rect, compact: bool) {
        let pal = theme::pal(ui);
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("listen average")
                .max_rect(foot)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        row.spacing_mut().item_spacing.x = 8.0;
        row.spacing_mut().interact_size.y = theme::CONTROL_H;
        if !compact {
            row.label(
                RichText::new("Average Over")
                    .size(13.0)
                    .color(pal.text_secondary),
            );
        }
        let mut avg = self.average_s;
        egui::ComboBox::from_id_salt("card average")
            .width(row.available_width())
            // Compact, "Avg" says it isn't a timer.
            .selected_text(if compact {
                format!("Avg {}", fields::duration(avg))
            } else {
                fields::duration(avg)
            })
            .show_ui(&mut row, |ui| {
                for a in AVERAGES {
                    ui.selectable_value(&mut avg, a, fields::duration(a));
                }
            })
            .response
            .on_hover_text(format!("Average Over: {AVERAGE_REACH}"));
        if avg != self.average_s {
            self.set_average(avg);
        }
    }

    /// Save: what is on show kept in a watch, or as a take in a folder.
    /// Whether the Save menu offers Unsorted Takes: a Quick Listen not
    /// saved yet goes there, as File > Save puts it.
    pub(super) fn saves_into_unsorted(&self) -> bool {
        self.quick_listen && self.session.saved_to().is_none() && self.watch_takes_dir().is_some()
    }

    pub(super) fn save_menu(&mut self, ui: &mut egui::Ui, size: f32) {
        let pal = theme::pal(ui);
        let can = self.session.allows(Action::Save) && self.saving.is_none();
        // The watch with the newest take first.
        let watches: Vec<(String, String)> = match &self.library {
            Ok(Some(l)) if l.read_only.is_none() => self.recent_watches(),
            _ => Vec::new(),
        };
        let into_watch = self.addable_take().is_some() || self.quick_listen;
        let into_unsorted = can && self.saves_into_unsorted();
        let listening = self.running();
        // A take opened with painting or masks changed: kept in its folder.
        let changes = self.marks_unsaved();
        let mut picked = None;
        let (mut new, mut folder, mut unsorted, mut marks) = (false, false, false, false);
        ui.add_enabled_ui(can || changes || self.addable_take().is_some(), |ui| {
            let button = theme::icon_button(ui, Icon::Save, size, 8, pal.accent, false, false);
            let saved = self.session.saved_to().is_some();
            egui::Popup::menu(&button)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
                .show(|ui| {
                    if changes {
                        if ui
                            .button("Save Changes")
                            .on_hover_text(SAVE_MARKS_HINT)
                            .clicked()
                        {
                            marks = true;
                        }
                        // Only above an item: the menu never ends on a line.
                        if into_unsorted || into_watch || can {
                            ui.separator();
                        }
                    }
                    if into_unsorted {
                        if ui
                            .button("Unsorted Takes")
                            .on_hover_text(
                                "Keep this in Unsorted Takes, in the Watches view, to file \
                                 under a watch later",
                            )
                            .clicked()
                        {
                            unsorted = true;
                        }
                        ui.separator();
                    }
                    if into_watch {
                        save_menu_watches(ui, &watches, !listening, can, &mut picked, &mut new);
                    }
                    if can
                        && ui
                            .button(if saved {
                                "Save Again"
                            } else {
                                "Save in a Folder…"
                            })
                            .on_hover_text(SAVE_HINT)
                            .clicked()
                    {
                        folder = true;
                    }
                });
            button
                .on_hover_text(
                    "Save: keep this in one of your watches, as a take of it, or as a take \
                     in a folder you choose",
                )
                .on_disabled_hover_text("Nothing to save yet, or a save is under way");
        });
        if marks {
            self.save_marks();
        }
        if let Some(id) = picked {
            self.keep_into(&id);
        }
        if unsorted {
            self.save_again();
        }
        if new {
            self.open_new_watch_form();
            self.keep_after_form = true;
        }
        if folder {
            // Save Again into the folder saved to before; else ask where.
            if self.session.saved_to().is_some() {
                self.save_again();
            } else {
                self.save_recording();
            }
        }
    }

    /// The Calibre card's title: the watch on the microphone, or No Watch
    /// (Quick Listen), with ▾, the one place on the Bench that says which
    /// watch a take is of. Its menu picks No Watch, a recent watch, a new
    /// one, or finds one in Watches. The card's limits and its "?" (still
    /// "Calibre") are at the right.
    pub(super) fn watch_line(&mut self, ui: &mut egui::Ui, limits: &[(String, String)]) {
        let pal = theme::pal(ui);
        // The card at its narrowest (800 × 600): the title a step smaller,
        // No Watch without its "(Quick Listen)".
        let narrow = ui.available_width() < NARROW_TITLE_W;
        // A watch with no calibre whose Beat Rate field is hidden for
        // width: the beat rates head this menu.
        let rates_here = ui.available_width() < TWO_FIELDS_W
            && self.watch_sets_here()
            && !self.bph_set_by_calibre();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            theme::help(ui, "Calibre", &help::CALIBRE);
            if let Some(first) = limits.first() {
                let sentences: Vec<String> = limits.iter().map(|l| l.1.clone()).collect();
                // The watch's name keeps the room; the chip says how many.
                let text = if limits.len() > 1 || ui.available_width() < 180.0 {
                    limits.len().to_string()
                } else {
                    chip_words(&first.0)
                };
                theme::chip(ui, &text, "Calibre", &sentences, ui.available_width());
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                // Level with the other cards' captions: no padding, and
                // only the title row's height, not a control's.
                ui.spacing_mut().button_padding = Vec2::new(0.0, 0.0);
                ui.spacing_mut().interact_size.y = theme::TITLE_ROW_H;
                let name = self
                    .open_watch()
                    .map_or_else(|| NO_WATCH.to_string(), |e| e.watch.name.clone());
                let title = match self.open_watch() {
                    None if narrow => NO_WATCH_SHORT,
                    _ => name.as_str(),
                };
                // The name is cut, never the ⏷.
                let font = theme::semibold(if narrow { 12.0 } else { 13.0 });
                let title = elide(ui, title, " ⏷", &font, ui.available_width());
                let button = ui
                    .add(
                        egui::Button::new(
                            RichText::new(title).font(font).color(pal.text_secondary),
                        )
                        .frame_when_inactive(false),
                    )
                    .on_hover_text(format!(
                        "Calibre: the watch on the microphone, {name}. Click to pick \
                         another, No Watch for a Quick Listen, or find one in Watches."
                    ));
                egui::Popup::menu(&button)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
                    .show(|ui| self.watch_line_menu(ui, rates_here));
            });
        });
    }

    /// The Watch line's menu: No Watch (Quick Listen), up to five recent
    /// watches, New Watch… and Find in Watches…. Another watch can't be
    /// picked while listening: the readings so far are of this one.
    pub(super) fn watch_line_menu(&mut self, ui: &mut egui::Ui, rates_here: bool) {
        let pal = theme::pal(ui);
        // As wide as its longest line, not the title's button.
        ui.set_min_width(WATCH_MENU_W);
        if rates_here {
            ui.label(
                RichText::new("Beat Rate (hidden at this width)")
                    .small()
                    .color(pal.text_tertiary),
            )
            .on_hover_text(self.bph_hint());
            // Short, as the menu is the watch's: the rate set, Auto, and
            // the other standard rates one level down.
            let mut bph = self.bph;
            let item = |ui: &mut egui::Ui, bph: &mut Option<u32>, v: Option<u32>, words: &str| {
                let r = theme::menu_item(ui, v == self.bph, words);
                if r.clicked() {
                    *bph = v;
                }
                r
            };
            if let Some(b) = self.bph {
                item(ui, &mut bph, Some(b), &format!("{b} bph"));
            }
            item(ui, &mut bph, None, "Auto").on_hover_text("Found from the first seconds of beats");
            ui.menu_button("Other Rates", |ui| {
                for b in STANDARD_BPH.into_iter().filter(|&b| Some(b) != self.bph) {
                    item(ui, &mut bph, Some(b), &format!("{b} bph"));
                }
            });
            if bph != self.bph {
                self.change_bph(bph);
                self.save_card_to_watch();
            }
            ui.separator();
        }
        let free = !self.running();
        if !free {
            // Said once, above what it dims, so no hint covers a row.
            ui.label(
                RichText::new("Stop listening to pick another watch")
                    .small()
                    .color(pal.text_tertiary),
            );
        }
        let none = self.open_watch().is_none();
        if ui
            .add_enabled_ui(free, |ui| theme::menu_item(ui, none, NO_WATCH))
            .inner
            .on_hover_text(QUICK_LISTEN_HINT)
            .clicked()
        {
            self.line_pick(None, false);
        }
        let recent = self.recent_watches();
        if !recent.is_empty() {
            ui.separator();
            ui.label(RichText::new("Recent").small().color(pal.text_tertiary));
        }
        for (id, name) in recent.into_iter().take(RECENT_WATCHES) {
            let on = self.watch_open.as_deref() == Some(id.as_str());
            if ui
                .add_enabled_ui(free, |ui| theme::menu_item(ui, on, &name))
                .inner
                .on_hover_text("Listen and the tests take this watch, saved into it")
                .clicked()
            {
                self.line_pick(Some(id), false);
            }
        }
        ui.separator();
        if ui
            .button("New Watch…")
            .on_hover_text("Add a watch: its name, calibre, lift angle and beat rate")
            .clicked()
        {
            self.open_new_watch_form();
            self.new_watch_on_mic = true;
        }
        if ui
            .button("Find in Watches…")
            .on_hover_text("Show the Watches view with its search field ready to type in")
            .clicked()
        {
            self.find_in_watches();
        }
    }

    /// A watch, or No Watch (`None`), picked on the Watch line: the watch
    /// on the microphone, so a take on show is put away first and the card
    /// shows the watch picked at once. Unsaved sound is asked about first,
    /// Quick Listen's too: sound never goes without a question here.
    pub(super) fn line_pick(&mut self, id: Option<String>, asked: bool) {
        if self.running() || self.saving_busy() {
            return;
        }
        if self.live.is_some() || self.session.recorder.is_some() {
            if !asked && self.line_pick_asks() {
                self.confirm = Some(Confirm::PickWatch(id));
                return;
            }
            self.new_session();
        }
        match id {
            Some(id) => self.pick_watch(&id),
            None => self.pick_unsorted(),
        }
    }

    /// Whether the Watch line asks before putting the take on show away:
    /// what any new take asks about, and Quick Listen's unsaved sound too.
    pub(super) fn line_pick_asks(&self) -> bool {
        self.needs_save_confirmation() || self.quick_unsaved() && self.unsaved_s() > 5.0
    }

    /// The library's watches, the one with the newest take first.
    fn recent_watches(&self) -> Vec<(String, String)> {
        match &self.library {
            Ok(Some(l)) => watches::shown(l, "", watches::Sort::LastTake)
                .into_iter()
                .map(|e| (e.watch.id.clone(), e.watch.name.clone()))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The Calibre card's figure row: the Calibre pop-up, or a watch's
    /// calibre named where the watch sets it. Where the controls row is
    /// too narrow for the beat rate beside the lift, the beat rates head
    /// the Calibre menu, or for a watch with no calibre the title's menu.
    pub(super) fn calibre_control(&mut self, ui: &mut egui::Ui) {
        if !self.take_shows_rate_and_lift() {
            return self.calibre_line(ui);
        }
        let pal = theme::pal(ui);
        if self.open_watch().is_some() {
            // A watch's calibre is the watch's to set.
            calibre_figure(ui, "No Calibre", pal.text_tertiary).on_hover_text(
                "This watch has no calibre the table knows. Edit… on the watch sets it, and \
                 with it the beat rate and lift angle.",
            );
            return;
        }
        let heard = self.live.as_ref().and_then(|l| l.bph());
        // The rate Manage Calibres… starts its table at.
        let rate = match self.bph {
            Some(b) => Some((b, "the beat rate set on the Calibre card")),
            None => heard.map(|b| (b, "the rate found from the beats")),
        };
        let current = self.calibre.clone();
        let mut bph = self.bph;
        let picked = if ui.available_width() >= TWO_FIELDS_W || self.bph_set_by_calibre() {
            self.cal.picker(ui, &current, rate)
        } else {
            // Too narrow for both under the picker: the lift keeps the
            // controls row, since Auto finds the beat rate and nothing
            // finds the lift, and the beat rates head this menu.
            let how = self.bph_hint();
            let mut first = |ui: &mut egui::Ui| {
                let found = match (bph, heard) {
                    (None, Some(h)) => format!("Beat Rate: Auto, {h} found"),
                    (Some(b), _) => format!("Beat Rate: {b} bph"),
                    _ => "Beat Rate: Auto".to_string(),
                };
                ui.label(RichText::new(found).small().color(pal.text_tertiary))
                    .on_hover_text(how);
                bph_choices(ui, &mut bph);
                ui.separator();
            };
            self.cal
                .picker_with(ui, &current, rate, "Not Set", Some(&mut first))
        };
        if let Some(p) = picked {
            self.set_calibre(p);
        } else if bph != self.bph {
            self.change_bph(bph);
        }
    }

    /// Whether the calibre sets the beat rate: a watch's calibre the
    /// table knows, or a calibre picked whose beat rate it gives.
    pub(super) fn bph_set_by_calibre(&self) -> bool {
        !self.take_shows_rate_and_lift() || self.cal.bph(&self.calibre).is_some()
    }

    /// What the beat rate's pop-up is, for its hover.
    fn bph_hint(&self) -> &'static str {
        let heard = self.live.as_ref().and_then(|l| l.bph());
        match (self.bph, heard) {
            (Some(_), _) => {
                "Beats per hour, set here, so the readings use it rather than finding it from \
                 the beats. Auto finds it again."
            }
            (None, Some(_)) => {
                "Beats per hour: Auto, found from the first seconds of beats as the nearest \
                 standard rate. Pick a rate to set it, for a watch that beats at an unusual \
                 rate or runs far off."
            }
            (None, None) => {
                "Beats per hour: Auto finds it from the first few seconds of beats. Pick a \
                 rate to set it."
            }
        }
    }

    /// The beat rate in the Calibre card's row, in words that say whether
    /// it was found or set (`bph_set`, by the calibre); `heard` is the rate
    /// heard.
    fn bph_shown(&self, bph_set: bool, heard: Option<u32>) -> String {
        if bph_set {
            self.bph
                .or_else(|| self.cal.bph(&self.calibre))
                .or(heard)
                .map_or_else(|| "Found When Heard".to_string(), |b| format!("{b} bph"))
        } else {
            match (self.bph, heard) {
                (Some(b), _) => format!("{b} bph"),
                (None, Some(h)) => format!("Auto {h}"),
                _ => "Auto".to_string(),
            }
        }
    }

    /// The Calibre card's controls row: the lift angle, then the beat
    /// rate, each as text where the calibre sets it. Where the row is too
    /// narrow for both, the lift alone (nothing finds it, while Auto finds
    /// the beat rate), the beat rates heading the Calibre menu.
    pub(super) fn rate_and_lift(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let heard = self.live.as_ref().and_then(|l| l.bph());
        let both = ui.available_width() >= TWO_FIELDS_W;
        let watch_sets = !self.take_shows_rate_and_lift();
        let calibre = self.calibre.clone();
        let set_by = move |what: &str| {
            if watch_sets {
                format!("The watch's calibre sets the {what}. Edit… on the watch changes it.")
            } else {
                format!(
                    "The {calibre} calibre sets the {what}. Set Calibre to Not Set to set another."
                )
            }
        };
        let words_w = |ui: &mut egui::Ui, words: &str| {
            ui.fonts_mut(|f| {
                f.layout_no_wrap(
                    words.to_string(),
                    egui::FontId::proportional(13.0),
                    pal.text,
                )
                .size()
                .x
            })
        };
        // The beat rate in words that say whether it was found or set, and
        // the room it takes at the end of the row: as wide as its words.
        let bph_set = self.bph_set_by_calibre();
        let shown = self.bph_shown(bph_set, heard);
        let text_w = words_w(ui, &shown);
        let sp = ui.spacing();
        // ComboBox's width leaves out its padding and ▾.
        let chrome = 2.0 * sp.button_padding.x + sp.icon_width + sp.icon_spacing;
        let bph_w = if bph_set {
            text_w + 4.0
        } else {
            text_w + 8.0 + chrome
        };
        let lift_w = if both {
            ui.available_width() - bph_w - sp.item_spacing.x
        } else {
            ui.available_width()
        };
        // A watch with no calibre keeps what is set here.
        let saved_to = match self.open_watch() {
            Some(e) if self.watch_sets_here() => format!(" Saved to {}.", e.watch.name),
            _ => String::new(),
        };
        if self.lift_settable() {
            let mut lift = self.lift_deg;
            let (resp, changed) = fields::combo(
                ui,
                "lift",
                &mut lift,
                timegrapher_core::input_domain::LIFT_RANGE_DEG,
                &DEGREES,
                &LIFTS,
                show_degrees,
                "Lift",
                lift_w,
            );
            let mut hint = if self.lift_known() {
                "The lift angle: type it, or pick one with ⏷.".to_string()
            } else {
                LIFT_HINT.to_string()
            };
            hint += &saved_to;
            if !both && !self.bph_set_by_calibre() && self.watch_sets_here() {
                hint += " Beat Rate is hidden at this width: it heads the menu on the card's \
                         title.";
            }
            resp.on_hover_text(hint);
            if changed {
                self.set_lift(lift);
                self.save_card_to_watch();
            }
        } else {
            card_fact(
                ui,
                "Lift",
                format!("{}°", fields::plain(self.lift_deg)),
                &set_by("lift angle"),
                lift_w,
            );
        }
        if !both {
            return;
        }
        if bph_set {
            card_fact(ui, "", shown, &set_by("beat rate"), ui.available_width());
            return;
        }
        let mut bph = self.bph;
        egui::ComboBox::from_id_salt("bph")
            // The rest of the row, so it ends on the card's right edge as
            // the Calibre pop-up does (its width is the outer width).
            .width(ui.available_width())
            // Tall enough for every rate, as in the menus that list them.
            .height(480.0)
            .selected_text(shown)
            .show_ui(ui, |ui| bph_choices(ui, &mut bph))
            .response
            .on_hover_text(format!("{}{saved_to}", self.bph_hint()));
        if bph != self.bph {
            self.change_bph(bph);
            self.save_card_to_watch();
        }
    }

    /// Whether the card's Lift and Beat Rate belong to the watch picked: a
    /// watch with no calibre the table knows, so what is set here is
    /// saved to it, as Edit Watch would.
    pub(super) fn watch_sets_here(&self) -> bool {
        self.open_watch().is_some() && self.cal.find(&self.calibre).is_none()
    }

    /// The card's lift angle and beat rate saved to the watch picked, when
    /// they are its to keep (see [`Self::watch_sets_here`]).
    pub(super) fn save_card_to_watch(&mut self) {
        if !self.watch_sets_here() {
            return;
        }
        let Some(mut w) = self.open_watch().map(|e| e.watch.clone()) else {
            return;
        };
        if w.lift == Some(self.lift_deg) && w.bph == self.bph {
            return;
        }
        w.lift = Some(self.lift_deg);
        w.bph = self.bph;
        let saved = match &mut self.library {
            Ok(Some(lib)) => lib.update(&w),
            _ => Err("there is no Watches folder".into()),
        };
        if let Err(e) = saved {
            self.error(format!("Couldn't save to {}: {e}", w.name));
        }
    }

    /// A new take of the watch picked: a new session on the microphone,
    /// asking first when there is unsaved sound, then listening, or the
    /// Positions Test or the Power Reserve Test.
    pub(super) fn new_take(&mut self, kind: NewTake, asked: bool) {
        // A test is set up over a Quick Listen still listening, which
        // stops once its sound is asked about; never over a test.
        let over = kind == NewTake::Listen || self.test_listening() || !self.quick_listen;
        if self.saving_busy() || (self.running() && over) {
            return;
        }
        if !asked && self.new_take_asks() {
            self.confirm = Some(Confirm::NewTake(kind));
            return;
        }
        if self.running() {
            self.pause();
        }
        self.take_setup();
        match kind {
            NewTake::Listen => {
                self.leave_ready_test();
                self.start_microphone();
            }
            NewTake::Positions | NewTake::PowerReserve => self.get_ready(kind),
        }
    }

    /// Set a test up on the clear Bench: its pane shown, the Position
    /// card live, waiting for Start Test on the Listen card.
    fn get_ready(&mut self, kind: NewTake) {
        self.test_ready = Some(kind);
        self.message = None;
        self.view = View::Bench;
        if kind == NewTake::Positions {
            // Where the test begins.
            self.set_position(guided::ORDER[0]);
            self.guided_earlier_for = None;
            self.find_full_wind();
            self.reveal_pane(Pane::PositionsTest);
        } else {
            self.reveal_pane(Pane::PowerReserve);
        }
    }

    /// Leave the test set up, with nothing started: the full-wind test
    /// picked or loaded for it goes too, so it isn't shown on the Bench
    /// after.
    pub(super) fn leave_ready_test(&mut self) {
        self.test_ready = None;
        self.guided_earlier = None;
        self.guided_earlier_file = false;
        self.guided_choices.clear();
        self.guided_earlier_for = None;
    }

    /// The test set up and waiting for Start Test, while the Bench stays
    /// clear.
    pub(super) fn ready_test(&self) -> Option<NewTake> {
        self.test_ready
            .filter(|_| self.live.is_none() && !self.running() && self.batch.is_none())
    }

    /// Whether a test is under way and listening: the Listen card's Stop
    /// asks first.
    pub(super) fn test_listening(&self) -> bool {
        self.running() && (self.guided_running() || self.reserve_running())
    }

    /// Why Start Test can't start the test set up, if it can't.
    pub(super) fn start_test_why_not(&self) -> Option<String> {
        let w = self.open_watch()?;
        if self.ready_test() != Some(NewTake::Positions)
            || !self.guided_after
            || self.guided_earlier.is_some()
        {
            return None;
        }
        Some(if self.guided_choices.is_empty() {
            format!(
                "{} has no Full Wind Positions Test from {}.",
                w.watch.name,
                guided::full_wind_ago()
            )
        } else {
            "Pick the full-wind test to compare with, in Compare With.".to_string()
        })
    }

    /// Start the test set up, from the Listen card's Start Test.
    fn start_ready_test(&mut self) {
        let Some(kind) = self.ready_test() else {
            return;
        };
        self.find_full_wind();
        if self.start_test_why_not().is_some() {
            return;
        }
        self.test_ready = None;
        match kind {
            NewTake::Positions => self.start_guided(self.guided_earlier.clone()),
            _ => self.start_reserve_asked(true),
        }
    }

    /// For a watch set up for 24 Hours After, the full-wind tests from
    /// [`guided::FULL_WIND_HOURS`] ago kept in its takes, for the next
    /// Positions Test to be set against the one picked. Searched again
    /// only when the watch, Wind, the minute or its takes have changed
    /// since; a pick no longer among them is dropped. With no watch, the
    /// half is what Load Full-Wind Results… loads.
    pub(super) fn find_full_wind(&mut self) {
        self.find_full_wind_at(guided::unix_now());
    }

    /// [`Self::find_full_wind`] at `now`, Unix seconds.
    pub(super) fn find_full_wind_at(&mut self, now: f64) {
        if self.test_ready != Some(NewTake::Positions) {
            return;
        }
        let dir = self.open_watch().map(|e| e.dir.join(library::TAKES));
        let changed = dir
            .as_ref()
            .and_then(|d| std::fs::metadata(d).ok())
            .and_then(|m| m.modified().ok());
        let mut takes: Vec<std::ffi::OsString> = dir
            .as_ref()
            .and_then(|d| std::fs::read_dir(d).ok())
            .map(|r| r.flatten().map(|e| e.file_name()).collect())
            .unwrap_or_default();
        takes.sort();
        let key = (
            self.watch_open.clone(),
            self.guided_after,
            (now / 60.0).floor() as i64,
            changed,
            takes,
        );
        if self.guided_earlier_for.as_ref() == Some(&key) {
            return;
        }
        // Another watch, or Wind changed: nothing picked carries over.
        let same = self
            .guided_earlier_for
            .as_ref()
            .is_some_and(|k| (&k.0, k.1) == (&key.0, key.1));
        self.guided_earlier_for = Some(key);
        self.guided_choices = match dir {
            Some(dir) if self.guided_after => guided::full_winds_in(&dir, now),
            _ => Vec::new(),
        };
        let from_file = self.guided_earlier_file;
        let listed = |e: &guided::FullWind| self.guided_choices.contains(e);
        if !same || !(from_file || self.guided_earlier.as_ref().is_some_and(listed)) {
            self.guided_earlier = None;
            self.guided_earlier_file = false;
        }
    }

    /// How long until the full-wind search should run again, at the next
    /// minute after `now` (Unix seconds), while a Positions Test waits on
    /// one: an idle Bench draws nothing, so without this a full-wind test
    /// reaching its 12 hour mark, or a take added or removed, is seen only
    /// when the mouse moves.
    pub(super) fn full_wind_recheck(&self, now: f64) -> Option<Duration> {
        (self.ready_test() == Some(NewTake::Positions)
            && self.guided_after
            && self.open_watch().is_some())
        .then(|| Duration::from_secs_f64(60.0 - now.rem_euclid(60.0) + 0.05))
    }

    /// Whether a new take asks first: a take of a watch asks about Quick
    /// Listen's sound too, as picking a watch does, as it isn't that
    /// watch's sound.
    pub(super) fn new_take_asks(&self) -> bool {
        if self.open_watch().is_some() {
            self.line_pick_asks()
        } else {
            self.needs_save_confirmation()
        }
    }

    /// What a new take starts from: an empty session and the open watch's
    /// details. Opens no input, so tests can check it.
    pub(super) fn take_setup(&mut self) {
        if self.live.is_some() || self.session.recorder.is_some() {
            self.new_session();
        }
        if let Some(w) = self.open_watch().map(|e| e.watch.clone()) {
            // The watch's details over what the microphone had before a
            // recording was opened.
            if let Some(s) = self.mic_setup.take() {
                self.set_watch_setup(s);
            }
            self.quick_listen = false;
            self.apply_watch(&w);
        }
        self.input = Input::Microphone;
    }

    /// Save the unsaved Quick Listen take into the watch `id`, as a take of
    /// it: the watch is picked, and the take named after it.
    pub(super) fn keep_in_watch(&mut self, id: &str) {
        if self.running() {
            return;
        }
        let Some(name) = (match &self.library {
            Ok(Some(l)) => l.watches.iter().find(|e| e.watch.id == id),
            _ => None,
        })
        .map(|e| e.watch.name.clone()) else {
            return;
        };
        self.watch_open = Some(id.to_string());
        self.unsorted_open = false;
        self.quick_listen = false;
        self.watch = name;
        if let Some(w) = self.open_watch().map(|e| e.watch.clone()) {
            self.take_watch_details(&w);
        }
        self.note_settings();
        self.save_again();
    }

    /// A take kept in, or opened from, the watch `w` without a calibre of
    /// its own takes the watch's:
    /// - the calibre, and with it the calibre's lift angle and wheels;
    /// - the watch's own lift angle, where the calibre gives none;
    /// - the calibre's beat rate, else the watch's, unless the take was
    ///   heard at another.
    ///
    /// The readings on show are read again at the new lift, not cleared.
    pub(super) fn take_watch_details(&mut self, w: &library::Watch) {
        if let Some(c) = w.calibre.clone().filter(|c| !c.is_empty()) {
            self.set_calibre(c);
        }
        if let Some(l) = self.cal.lift(&self.calibre).or(w.lift) {
            if l != self.lift_deg {
                self.set_lift(l);
            }
            self.lift_context = (l, true);
        }
        let bph = self.cal.bph(&self.calibre).or(w.bph);
        let heard = self.live.as_ref().and_then(|l| l.bph());
        if bph.is_some() && heard.is_none_or(|h| Some(h) == bph) {
            self.put_bph(bph);
        }
    }

    /// Whether the cards offer Beat Rate and Lift Angle: when the
    /// take's watch has no calibre the table knows, or no watch is picked.
    /// Whether the lift angle can be typed: not when a watch's calibre or
    /// a calibre picked in Quick Listen sets it.
    pub(super) fn lift_settable(&self) -> bool {
        self.take_shows_rate_and_lift() && self.cal.lift(&self.calibre).is_none()
    }

    pub(super) fn take_shows_rate_and_lift(&self) -> bool {
        self.open_watch().is_none() || self.cal.find(&self.calibre).is_none()
    }

    /// One line for a watch's calibre and what it sets: the beat rate and
    /// the lift angle.
    fn calibre_line(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let heard = self.live.as_ref().and_then(|l| l.bph());
        let bph = match (self.bph, heard) {
            (Some(b), _) => format!("{b} bph"),
            (None, Some(b)) => format!("{b} bph, found"),
            (None, None) => "beat rate found when heard".to_string(),
        };
        // The beat rate and lift show on their own cards: the name is enough,
        // on one line cut short where the card is narrow, so the card keeps
        // its height. A calibre whose beat rate isn't the one heard says its
        // own beside the name, the whole sentence in the hover.
        let other = heard
            .zip(self.cal.bph(&self.calibre))
            .filter(|&(b, cb)| b != cb);
        let line = match other {
            Some((_, cb)) => format!("{} is {cb} bph", self.calibre),
            None => self.calibre.clone(),
        };
        let mut hover = format!(
            "{}: {bph}, lift {}°. The watch's calibre sets the beat rate and lift angle. \
             Edit… on the watch changes them.",
            self.calibre,
            fields::plain(self.lift_deg)
        );
        if let Some((b, cb)) = other {
            hover = format!(
                "The {} beats at {cb} bph, but this watch beats at {b}.\n\n{hover}",
                self.calibre
            );
        }
        calibre_figure(ui, &line, pal.text).on_hover_text(hover);
    }

    /// The position picked: a new stretch, noted in the recording.
    pub(super) fn change_position(&mut self, pos: Position) {
        if self.refuse_faulted_recording() {
            return;
        }
        if self.reserve_running() && pos != self.position {
            self.info_msg(
                "A Power Reserve Test runs in one position: stop the test to change it.".into(),
            );
            return;
        }
        self.set_position(pos);
        if let Some(r) = self.session.recorder.as_mut() {
            // note appends in memory before writing scratch metadata, so the
            // edit remains unsaved even if that scratch write fails.
            let _ = r.note(&format!("position {}", pos.code()));
            self.session.note_metadata_change();
        }
        self.note_settings();
    }

    /// Put the watch in a new position. Listening (or paused), the session
    /// carries on with a new stretch: the strip keeps its beats and marks
    /// the stretch, and the readings, the profile, the distributions and the
    /// steadiness tests take only the beats in the new position.
    pub(super) fn set_position(&mut self, pos: Position) {
        let old = self.position;
        applog::info(&format!("position {} to {}", old.code(), pos.code()));
        self.position = pos;
        let going = self.session.allows(Action::Move);
        if let (Some(l), true) = (self.live.as_mut(), going) {
            l.new_stretch();
            let at = l.duration_s();
            let _ = self.session.move_to(at, old, pos);
            self.rev.breaks += 1;
        }
    }

    /// Seconds of settling still to come now.
    pub(super) fn settling_left(&self) -> Option<f64> {
        self.session.settling_left(self.latest_s())
    }

    /// When stopped inside the settle after a move: when that move was,
    /// the end of the position before it.
    pub(super) fn stopped_in_settle(&self) -> Option<f64> {
        if self.running() {
            return None;
        }
        let t = self.latest_s();
        self.session
            .settles()
            .into_iter()
            .find(|s| t >= s.0 && t < s.1)
            .map(|s| s.0)
    }

    /// The stretches in the position the watch is in now, which the
    /// profile, the distributions and the steadiness tests are made over.
    pub(super) fn current_spans(&self) -> Vec<(f64, f64)> {
        self.session.current_spans(self.position)
    }

    /// The seconds heard so far in position `p`, settled and not masked,
    /// in a live take or the take on show: what each position button's
    /// bar fills with.
    pub(super) fn recorded_in(&self, p: Position) -> f64 {
        if self.live.is_none() {
            return 0.0;
        }
        let end = self.latest_s();
        let masked = timegrapher_core::session::masked_spans(
            &self.masked.iter().map(|&(a, b)| [a, b]).collect::<Vec<_>>(),
        );
        self.session
            .settled_spans(self.position, p)
            .into_iter()
            .map(|(a, b)| (a.max(0.0), b.min(end)))
            .filter(|(a, b)| a < b)
            .map(|(a, b)| b - a - timegrapher_core::session::masked_within(&masked, a, b))
            .sum::<f64>()
            .max(0.0)
    }

    /// Whether a reading ending at `t` comes from the position the watch is
    /// in now.
    pub(super) fn in_position(&self, t: f64) -> bool {
        self.session.in_position(t, self.position)
    }

    /// The stretches to mark on the strip and the charts, once there is
    /// more than one or a Positions Test has begun: start, end and the
    /// position's name.
    pub(super) fn position_bands(&self) -> Vec<(f64, f64, &'static str)> {
        let bands = self.session.position_bands(self.position);
        if bands.is_empty() && self.in_test() {
            return self
                .session
                .position_spans(self.position)
                .into_iter()
                .map(|(a, b, p)| (a.max(0.0), b, p.label()))
                .collect();
        }
        bands
    }

    /// Whether the strip and charts mark a Positions Test's phases: from
    /// its start until the next test or a new session.
    pub(super) fn in_test(&self) -> bool {
        !self.guided_unused.is_empty() || !self.guided_measured.is_empty()
    }

    /// What is wrong with keeping the computer awake or taking sound in
    /// the background, for Keep Awake's caption.
    /// One plain line for the caption, and what the system said, for the
    /// row's hover.
    pub(super) fn awake_words(&self) -> Option<(String, String)> {
        if let Some(e) = &self.pump_fault {
            return Some((
                "Not kept awake, and sound is taken only while the window shows.".into(),
                format!("The thread that does both didn't start: {e}"),
            ));
        }
        match (self.keep_awake, &self.awake_fault) {
            (true, Some(e)) => Some((
                "Not being kept awake: the system didn't allow it.".into(),
                format!("What the system said: {e}"),
            )),
            _ => None,
        }
    }

    /// The train the Steadiness tests name cycles after, when a calibre is
    /// picked; `None` leaves them to the beat rate's usual wheels.
    pub(super) fn calibre_wheels(&self) -> Option<Vec<Wheel>> {
        self.cal.wheels(&self.calibre)
    }

    /// Pick a calibre: its lift angle, where the list or the user gives
    /// one, and its wheels for naming the cycles Steadiness finds.
    pub(super) fn set_calibre(&mut self, name: String) {
        // A take saved before the table renamed its calibre.
        let name = self.cal.current_name(&name);
        // Recent is what the app was last set to, even set again.
        self.cal.remember(&name);
        if name == self.calibre {
            return;
        }
        self.put_calibre(name);
        if let Some(l) = self.cal.lift(&self.calibre) {
            self.set_lift(l);
            self.lift_context = (l, true);
        } else if self.calibre.is_empty() && self.lift_deg != DEFAULT_LIFT_DEG {
            // No calibre: the default lift angle, as Quick Listen starts.
            self.set_lift(DEFAULT_LIFT_DEG);
        }
        // In Quick Listen, the calibre's beat rate, else Auto again with
        // none picked; a watch's own beat rate stays the watch's.
        let bph = self.cal.bph(&self.calibre);
        if self.open_watch().is_none()
            && (bph.is_some() || self.calibre.is_empty())
            && bph != self.bph
        {
            self.change_bph(bph);
        }
    }
}

/// The Current Position card's caption and its "?" in a title row.
fn position_caption(ui: &mut egui::Ui, rect: egui::Rect, caption: &str) {
    let pal = theme::pal(ui);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("position title")
            .max_rect(rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    theme::help(&mut child, caption, &help::POSITION);
    child.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.add(
            egui::Label::new(
                RichText::new(caption)
                    .font(theme::semibold(13.0))
                    .color(pal.text_secondary),
            )
            .truncate(),
        );
    });
}

/// The Listen card's take length, live while listening, and its state,
/// after the buttons in `top`.
fn listen_length(ui_b: &mut egui::Ui, top: egui::Rect, length: &str, state: &str) {
    let pal = theme::pal(ui_b);
    // The take's length, live while listening, and its state.
    ui_b.add_space(4.0);
    let at = ui_b.cursor().min.x;
    let words = egui::Rect::from_min_max(egui::pos2(at, top.top()), top.max);
    let clip = ui_b.painter().with_clip_rect(words);
    clip.text(
        egui::pos2(at, top.center().y + 1.0),
        egui::Align2::LEFT_BOTTOM,
        length,
        egui::FontId::proportional(15.0),
        pal.text,
    );
    clip.text(
        egui::pos2(at, top.center().y + 2.0),
        egui::Align2::LEFT_TOP,
        state,
        egui::FontId::proportional(12.0),
        pal.text_secondary,
    );
    ui_b.interact(words, ui_b.id().with("take length"), egui::Sense::hover())
        .on_hover_text(
            "The take's length: the sound heard so far while listening, or the whole \
             take opened",
        );
}

/// The Save menu's Keep in a Watch: each of `watches`, then New Watch…,
/// greyed unless the take `keeps` (it has ended), and a line under them
/// when Save in a Folder… follows (`can`). Sets `picked` or `new`.
fn save_menu_watches(
    ui: &mut egui::Ui,
    watches: &[(String, String)],
    keeps: bool,
    can: bool,
    picked: &mut Option<String>,
    new: &mut bool,
) {
    let pal = theme::pal(ui);
    ui.label(
        RichText::new("Keep in a Watch")
            .small()
            .color(pal.text_secondary),
    );
    // A take goes into a watch once it ends: greyed,
    // saying so, while it is heard.
    for (id, name) in watches {
        if ui
            .add_enabled(keeps, egui::Button::new(name))
            .on_disabled_hover_text(KEEP_AFTER_STOP)
            .clicked()
        {
            *picked = Some(id.clone());
        }
    }
    if ui
        .add_enabled(keeps, egui::Button::new("New Watch…"))
        .on_hover_text("Add a watch, and keep this in it when you add it")
        .on_disabled_hover_text(KEEP_AFTER_STOP)
        .clicked()
    {
        *new = true;
    }
    // Only above an item: the menu never ends on a line.
    if can {
        ui.separator();
    }
}

/// A fact in the Calibre card's controls row, `w` wide: its `label`
/// (none when empty) and `value`, both with `why` in their hover.
fn card_fact(ui: &mut egui::Ui, label: &str, value: String, why: &str, w: f32) {
    let pal = theme::pal(ui);
    ui.allocate_ui_with_layout(
        Vec2::new(w, theme::CONTROL_H),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.set_min_width(w);
            if !label.is_empty() {
                ui.label(RichText::new(label).size(13.0).color(pal.text_secondary))
                    .on_hover_text(why);
            }
            ui.add(egui::Label::new(RichText::new(value).size(13.0)).truncate())
                .on_hover_text(why);
        },
    );
}
