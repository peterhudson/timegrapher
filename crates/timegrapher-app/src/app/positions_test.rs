//! The Positions Test pane: its settings, and the test's start, tick and end.

use super::*;

impl TimegrapherApp {
    /// Begin the Positions Test, listening first if need be, with the
    /// watch dial up: just after winding, or as the 24 hour half after the
    /// full-wind test `half`.
    pub(super) fn start_guided(&mut self, half: Option<guided::FullWind>) {
        // One test to a take: after one, a new take (asking first about
        // sound not saved), never the stopped one carried on.
        if self.guided.is_some() && !self.running() {
            return self.new_take(NewTake::Positions, false);
        }
        // A test is never Quick Listen: its sound is saved or asked about,
        // not let go, even when it follows Quick Listen or runs on from it.
        self.quick_listen = false;
        if !self.running() {
            if self.can_resume() {
                self.resume_microphone();
            } else {
                self.start_microphone();
                if self.confirm == Some(Confirm::Microphone) {
                    // The test begins once the masks are answered for.
                    self.guided_after_ask = Some(half);
                    return;
                }
            }
        }
        if !self.running() {
            return;
        }
        if self.position != guided::ORDER[0] {
            self.change_position(guided::ORDER[0]);
        }
        let test = match half {
            Some(full) => guided::Test::after(full),
            None => guided::Test::new(),
        };
        self.guided = Some(test.timed(self.guided_settle_s, self.guided_measure_s));
        self.positions_take = true;
        self.guided_unused.clear();
        self.guided_measured.clear();
        self.reveal_pane(Pane::PositionsTest);
    }

    /// Stop Test… answered Stop Test: the test ends now, keeping what it
    /// measured, as its natural end does.
    pub(super) fn stop_test(&mut self) {
        self.stop_test_ask = false;
        let now = self.live.as_ref().map_or(0.0, |l| l.duration_s());
        if self.reserve_running() {
            if let Some(t) = self.reserve.as_mut() {
                t.stop();
            }
            self.end_reserve();
        } else if self.guided_running() {
            self.end_guided(now, false);
        }
    }

    /// The question Stop Test… on the Listen card asks once: Keep
    /// Measuring, the default, or Stop Test, keeping what was measured.
    pub(super) fn stop_test_dialog(&mut self, ctx: &egui::Context) {
        if !self.stop_test_ask {
            return;
        }
        if !self.test_listening() {
            self.stop_test_ask = false;
            return;
        }
        let kept = match self.open_watch() {
            Some(e) => format!(" in {}", e.watch.name),
            None => String::new(),
        };
        let (title, body) = if self.reserve_running() {
            let run = self.reserve.as_ref().map_or(0.0, |t| t.run_so_far_s());
            (
                "Stop the Power Reserve Test?",
                format!(
                    "The run so far, {} from the first beat heard, is kept{kept}. Its run time \
                     then says it was stopped by hand.",
                    power_reserve::run_text(run)
                ),
            )
        } else {
            // Each position measured, read or not: what Stop Test keeps.
            let n = self.guided.as_ref().map_or(0, |t| t.stretches().len());
            let what = match n {
                0 => "The take so far is kept, with no position measured yet".to_string(),
                1 => "The 1 position measured so far is kept".to_string(),
                n => format!("The {n} positions measured so far are kept"),
            };
            ("Stop the Positions Test?", format!("{what}{kept}."))
        };
        let mut choice = None;
        egui::Modal::new(egui::Id::new("stop test")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(RichText::new(title).font(theme::semibold(15.0)));
            ui.add_space(4.0);
            ui.label(body);
            ui.add_space(8.0);
            choice = theme::dialog_buttons(
                ui,
                &[
                    DialogButton::new(Role::Default, "Keep Measuring", ""),
                    DialogButton::new(Role::Destructive, "Stop Test", ""),
                ],
            );
        });
        match choice {
            Some(0) => self.stop_test_ask = false,
            Some(_) => self.stop_test(),
            None => {}
        }
    }

    /// Move the Positions Test on with the sound, whether its pane shows
    /// or not: read each position as it is measured, and keep what the
    /// strip marks.
    pub(super) fn guided_tick(&mut self, ctx: &egui::Context) {
        let Some(now) = self.live.as_ref().map(|l| l.duration_s()) else {
            return;
        };
        let Some(test) = self.guided.as_mut() else {
            return;
        };
        let phase = test.tick(now);
        if test.needs_reading() {
            if let Some(log) = self.live.as_ref().and_then(steady::log_of) {
                let clock = self.reading_clock(log.duration_s);
                if let Some(test) = self.guided.as_mut() {
                    test.read_new(&log, clock.as_ref(), &self.masked);
                }
            }
        }
        let Some(test) = self.guided.as_mut() else {
            return;
        };
        self.guided_unused = test.unused(now);
        self.guided_measured = test.measured(now);
        if matches!(
            phase,
            guided::Phase::Settling(_) | guided::Phase::Measuring(_) | guided::Phase::Paused
        ) {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        // The sixth position measured and read: the test ends by itself,
        // stops listening and keeps its take, as Stop does.
        let done =
            phase == guided::Phase::Turn && test.next_position().is_none() && !test.needs_reading();
        if done && self.running() {
            self.end_guided(now, true);
        }
    }

    /// The Positions Test ended at `now`, all six measured (`whole`) or
    /// stopped part way: the positions measured stay in the table, read
    /// together, and listening stops as at Stop, keeping the take.
    fn end_guided(&mut self, now: f64, whole: bool) {
        let Some(mut test) = self.guided.take() else {
            return;
        };
        if whole {
            test.advance(now);
        } else {
            test.stop(now);
        }
        if let Some(log) = self.live.as_ref().and_then(steady::log_of) {
            let clock = self.reading_clock(log.duration_s);
            test.finish(&log, clock.as_ref(), &self.masked);
        }
        self.guided_unused = test.unused(now);
        self.guided_measured = test.measured(now);
        self.guided = Some(test);
        self.stop_listening();
    }

    /// The watch and how it is read, for the test's saved and copied
    /// results.
    pub(super) fn guided_about(&self) -> guided::About {
        guided::About {
            watch: self.watch.trim().to_string(),
            calibre: self.calibre.clone(),
            bph: self.bph.or(self.live.as_ref().and_then(|l| l.bph())),
            lift_angle_deg: self.lift_deg,
        }
    }

    /// Whether a Positions Test is under way, not yet finished.
    pub(super) fn guided_running(&self) -> bool {
        self.guided.as_ref().is_some_and(|t| t.readings.is_none())
    }

    /// Where the test's files are offered first: beside the saved take,
    /// or the folder Save offers.
    pub(super) fn guided_dir(&self) -> PathBuf {
        self.session
            .saved_to()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from(&self.save_dir))
    }

    /// The Positions Test pane's toolbar: start the test, load the
    /// full-wind results for the 24 hour half, the times in each
    /// position, and saving or copying the results.
    pub(super) fn positions_settings(&mut self, ui: &mut egui::Ui) {
        let running = self.guided_running();
        // While it runs, the step panel and the Listen card carry its
        // buttons; set up, Cancel Test leaves the Bench as it was.
        if !running {
            theme::keep_next(ui);
            if self.ready_test() == Some(NewTake::Positions) {
                if theme::tool_word_button(
                    ui,
                    Icon::Back,
                    "Cancel Test",
                    "Leave the test set up, with nothing started or kept",
                    false,
                    None,
                ) {
                    self.leave_ready_test();
                }
            } else if theme::tool_word_button(
                ui,
                Icon::Listen,
                "New Test",
                "Set a Positions Test up on a clear Bench: wind the watch, then press Start \
                 Test on the Listen card. It begins in Dial Up and leads the watch through the \
                 six positions.",
                true,
                None,
            ) {
                self.new_take(NewTake::Positions, false);
            }
            theme::bar_gap(ui);
        }
        self.positions_wind(ui, running);
        theme::bar_gap(ui);
        self.positions_lengths(ui, running);
        self.positions_results(ui);
    }

    /// The Positions Test settings' Wind choice (Full Wind or 24 Hours After)
    /// with Compare With, or, with no watch open, Load Full-Wind Results….
    fn positions_wind(&mut self, ui: &mut egui::Ui, running: bool) {
        if self.open_watch().is_some() {
            let was = self.guided_after;
            ui.add_enabled_ui(!running, |ui| {
                theme::tool_choice(
                    ui,
                    "Wind",
                    &format!(
                        "When the test is run: just after a full wind, or 24 hours after it, \
                         set against the watch's full-wind test from {}",
                        guided::full_wind_ago()
                    ),
                    &mut self.guided_after,
                    &[
                        (
                            false,
                            Icon::BarrelIn,
                            "Full Wind",
                            "Just after winding the watch fully",
                        ),
                        (
                            true,
                            Icon::Barrel24,
                            "24 Hours After",
                            "24 hours after the full-wind test, the watch neither wound nor \
                             worn since: set against that test, picked in Compare With",
                        ),
                    ],
                );
            })
            .response
            .on_disabled_hover_text(FIXED_IN_TEST);
            if self.guided_after != was {
                self.find_full_wind();
            }
            if self.guided_after && !running {
                theme::bar_gap(ui);
                self.compare_with(ui);
            }
        } else if !running {
            let load_hint = format!(
                "For the test 24 hours after winding: load the results saved from the test just \
                 after it ({} in a saved session's folder, or the file Save Results for 24 \
                 Hours… wrote). The next test is then set against them.",
                guided::RESULTS_FILE
            );
            if theme::tool_word_button(
                ui,
                Icon::BarrelIn,
                "Load Full-Wind Results…",
                &load_hint,
                false,
                None,
            ) {
                self.load_earlier();
            }
        }
    }

    /// The Positions Test settings' Settle and Each Position fields.
    fn positions_lengths(&mut self, ui: &mut egui::Ui, running: bool) {
        theme::tool_field(
            ui,
            "Settle",
            "Seconds the watch is left to settle after every change of position, in any take \
             and in the guided test, before its beats are read. They show faint on the strip \
             and are left out of every reading.",
            |ui| {
                ui.add_enabled_ui(!running, |ui| {
                    let mut v = self.guided_settle_s;
                    let a =
                        fields::entry(ui, "settle", &mut v, guided::SETTLE_RANGE, &DURATION, 48.0);
                    let b =
                        fields::presets(ui, "settle-presets", &mut v, &SETTLES, fields::duration);
                    if a || b {
                        self.guided_settle_s = v;
                        // Every settle band redraws, and the table with it.
                        self.session.resettle(v);
                        self.positions_moved(self.position);
                    }
                })
                .response
                .on_disabled_hover_text(FIXED_IN_TEST);
            },
        );
        theme::tool_field(
            ui,
            "Each Position",
            "Used by the guided test only: how long it measures each position. Longer brings \
             the rate's ± down; a minute gives about half a second a day.",
            |ui| {
                ui.add_enabled_ui(!running, |ui| {
                    let mut v = self.guided_measure_s;
                    let a = fields::entry(
                        ui,
                        "measure",
                        &mut v,
                        guided::MEASURE_RANGE,
                        &DURATION,
                        48.0,
                    );
                    let b =
                        fields::presets(ui, "measure-presets", &mut v, &MEASURES, fields::duration);
                    if a || b {
                        self.guided_measure_s = v;
                    }
                })
                .response
                .on_disabled_hover_text(FIXED_IN_TEST);
            },
        );
    }

    /// The Positions Test settings' Save as CSV…, Copy and Save Results for 24
    /// Hours….
    fn positions_results(&mut self, ui: &mut egui::Ui) {
        let has = self
            .guided
            .as_ref()
            .is_some_and(|t| !t.readings_so_far().is_empty());
        let nothing = (!has).then_some("Nothing measured yet");
        theme::bar_gap(ui);
        let about = self.guided_about();
        if theme::tool_button(
            ui,
            Icon::SaveTable,
            "Save as CSV…",
            "Save the watch, its calibre, the table, the means and the differences as a CSV \
             file a spreadsheet opens",
            nothing,
        ) {
            if let Some(t) = &self.guided {
                let text = guided::export_text(t, &about, ',');
                if let Some(m) = guided::save_csv(&text, &self.guided_dir()) {
                    self.info_msg(m);
                }
            }
        }
        if theme::tool_word_button(
            ui,
            Icon::Copy,
            "Copy",
            "Copy the watch, its calibre and the table with tabs between the columns, to paste \
             into a spreadsheet or a message",
            false,
            nothing,
        ) {
            if let Some(t) = &self.guided {
                ui.ctx().copy_text(guided::export_text(t, &about, '\t'));
                self.info_msg("Copied the Positions Test table".to_string());
            }
        }
        if let Some(k) = self.guided.as_ref().and_then(|t| t.kept(&about)) {
            let hint = format!(
                "Save these results to load when you run the test again 24 hours after winding. \
                 Saving the session keeps them too, as {} in its folder.",
                guided::RESULTS_FILE
            );
            if theme::tool_word_button(
                ui,
                Icon::Barrel24,
                "Save Results for 24 Hours…",
                &hint,
                false,
                None,
            ) {
                if let Some(m) = guided::save_results(&k, &self.guided_dir()) {
                    self.info_msg(m);
                }
            }
        }
    }

    /// The Positions Test pane: the banner and the tables, and what its
    /// buttons ask for.
    pub(super) fn positions_pane(&mut self, ui: &mut egui::Ui) {
        self.results_rects[0] = Some(ui.max_rect());
        let now = self.live.as_ref().map(|l| l.duration_s());
        let phase = match (self.guided.as_mut(), now) {
            (Some(t), Some(now)) => Some(t.tick(now)),
            _ => None,
        };
        let free = if self.guided.is_none() {
            self.free_table(ui.ctx())
        } else {
            None
        };
        let live = match phase {
            Some(guided::Phase::Measuring(_)) => self.guided_live(ui.ctx()),
            _ => None,
        };
        // Set up as the 24 hours after half with no full-wind test to set
        // it against: said here, and Start Test waits.
        let note = self.start_test_why_not();
        let action = guided::pane(
            ui,
            self.guided.as_ref(),
            phase,
            self.guided_earlier.as_ref(),
            free.as_ref(),
            live.as_ref(),
            self.guided_settle_s,
            self.guided_measure_s,
            self.open_watch().is_some(),
            note.as_deref(),
            self.position_card_title,
        );
        let Some(action) = action else { return };
        if action == guided::Action::ClearEarlier {
            self.guided_earlier = None;
            self.guided_earlier_file = false;
            self.guided_after = false;
            return;
        }
        let Some(now) = now else { return };
        let Some(mut test) = self.guided.take() else {
            return;
        };
        match action {
            // Noted in the take like a position picked by hand, so its
            // session.json follows the test. After the sixth there is no
            // Next: the test ends by itself.
            guided::Action::Next => {
                if let Some(p) = test.advance(now) {
                    self.change_position(p);
                }
            }
            guided::Action::Redo => test.redo(now),
            guided::Action::Pause => test.pause(now),
            guided::Action::Resume => test.resume(now),
            guided::Action::ClearEarlier => {}
        }
        self.guided_unused = test.unused(now);
        self.guided_measured = test.measured(now);
        self.guided = Some(test);
    }
}
