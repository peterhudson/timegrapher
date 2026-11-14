//! The Positions Test pane: its settings, and the test's start, tick and end.

use super::*;

impl TimegrapherApp {
    /// Begin the Positions Test, listening first if need be, with the
    /// watch dial up: just after winding, or as the 24 hour half after the
    /// full-wind test `half`.
    pub(super) fn start_guided(&mut self, half: Option<guided::FullWind>) {
        self.start_guided_test(guided::Kind::Positions, half);
    }

    /// Begin the Dynamic Poise test, as the Positions Test begins, from
    /// the first of its eight upright stops, with the card at 10.
    pub(super) fn start_poise(&mut self) {
        self.ten_positions = true;
        self.start_guided_test(guided::Kind::Poise, None);
    }

    /// Begin the Complication Test, as the Positions Test begins, in the
    /// position the card shows, with the complication off.
    pub(super) fn start_complication(&mut self) {
        self.start_guided_test(guided::Kind::Complication, None);
    }

    /// Begin the guided test of `kind`; `half` is the Positions Test's
    /// full-wind test for its 24 hour half.
    pub(super) fn start_guided_test(&mut self, kind: guided::Kind, half: Option<guided::FullWind>) {
        // One test to a take: after one, a new take (asking first about
        // sound not saved), never the stopped one carried on.
        if self.guided.is_some() && !self.running() {
            return self.new_take(NewTake::of(kind), false);
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
                    self.guided_after_ask = Some((kind, half));
                    return;
                }
            }
        }
        if !self.running() {
            return;
        }
        // The Complication Test stays where the card is.
        if let Some(&first) = kind.order().first() {
            if self.position != first {
                self.change_position(first);
            }
        }
        let test = match (kind, half) {
            (guided::Kind::Poise, _) => guided::Test::poise(),
            (guided::Kind::Complication, _) => guided::Test::complication(self.position),
            (_, Some(full)) => guided::Test::after(full),
            (_, None) => guided::Test::new(),
        };
        self.guided = Some(test.timed(self.guided_settle_s, self.guided_measure_s));
        self.positions_take = true;
        self.guided_unused.clear();
        self.guided_measured.clear();
        self.mask_engine();
        self.reveal_pane(match kind {
            guided::Kind::Positions => Pane::PositionsTest,
            guided::Kind::Poise => Pane::DynamicPoise,
            guided::Kind::Complication => Pane::Complication,
        });
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
                "Stop the Power Reserve Test?".to_string(),
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
            let name = self
                .guided
                .as_ref()
                .map_or("Positions Test", |t| t.kind.name());
            (format!("Stop the {name}?"), format!("{what}{kept}."))
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
                    test.bph_set = self.live.as_ref().and_then(|l| l.config().analysis.bph);
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
        // The test's settlings leave the readings as a move's do.
        self.mask_engine();
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
            test.bph_set = self.live.as_ref().and_then(|l| l.config().analysis.bph);
            test.finish(&log, clock.as_ref(), &self.masked);
        }
        self.guided_unused = test.unused(now);
        self.guided_measured = test.measured(now);
        self.guided = Some(test);
        self.mask_engine();
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

    /// A Positions Test's take opened from `dir`: a 24 hour half's saved
    /// results, so its pane shows the run's table and comparison again.
    pub(super) fn load_saved_test(&mut self, dir: &Path) {
        self.guided_saved = std::fs::read_to_string(dir.join(guided::RESULTS_FILE))
            .ok()
            .and_then(|t| guided::FullWind::from_json(&t).ok())
            .filter(|f| f.against.is_some());
    }

    /// The Positions Test, if that is the guided test on the Bench.
    pub(super) fn positions_test(&self) -> Option<&guided::Test> {
        self.guided
            .as_ref()
            .filter(|t| t.kind == guided::Kind::Positions)
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
        self.positions_lengths(ui, running, "Each Position");
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
    /// The Settle and the measuring time every guided test shares: one
    /// value each, so a change in one pane changes it in all (Peter, #1577).
    /// `each` names the measuring time in this pane: "Each Position", or the
    /// Complication Test's "Each Stretch".
    pub(super) fn positions_lengths(&mut self, ui: &mut egui::Ui, running: bool, each: &str) {
        theme::tool_field(
            ui,
            "Settle",
            "Seconds the watch is left to settle after every change of position, in any take \
             and in every guided test, before its beats are read. They show faint on the \
             strip and are left out of every reading. Every guided test shares it.",
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
            each,
            "Used by the guided tests only: how long each measures a position, or a \
             Complication Test's stretch. Every guided test shares it. Longer brings the \
             rate's ± down; a minute gives about half a second a day.",
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

    /// The Positions Test settings' Save as CSV…, Save as Text…, Copy and
    /// Save Results for 24 Hours…. The first three write the table the
    /// pane shows: the test's, or an opened take's This Take (#1660).
    fn positions_results(&mut self, ui: &mut egui::Ui) {
        let kept = self.positions_kept(ui.ctx());
        let nothing = match self.positions_test() {
            Some(t) => t.readings_so_far().is_empty(),
            None => kept.is_none(),
        }
        .then_some("Nothing measured yet");
        theme::bar_gap(ui);
        let about = self.guided_about();
        let text = |t: Option<&guided::Test>, sep: char| match (t, &kept) {
            (Some(t), _) => Some(guided::export_text(t, &about, sep)),
            (None, Some(k)) => Some(guided::kept_rows_text(
                k,
                guided::Kind::Positions,
                &about,
                sep,
            )),
            _ => None,
        };
        if theme::tool_button(
            ui,
            Icon::SaveTable,
            "Save as CSV…",
            "Save the watch, its calibre, the table, the means and the differences as a CSV \
             file a spreadsheet opens",
            nothing,
        ) {
            if let Some(text) = text(self.positions_test(), ',') {
                let d = guided::csv_dialog(&self.guided_dir());
                self.save_text_as(d, "the Positions Test", text);
            }
        }
        if theme::tool_button(
            ui,
            Icon::SaveTable,
            "Save as Text…",
            "Save the watch, how it was read, the table and the notes under it as a plain-text \
             report to keep, print or send",
            nothing,
        ) {
            self.save_test_text(&about, kept.as_deref());
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
            if let Some(text) = text(self.positions_test(), '\t') {
                ui.ctx().copy_text(text);
                self.info_msg("Copied the Positions Test table".to_string());
            }
        }
        // Only a full-wind test's results are for the 24 hour half.
        let kept = self.guided.as_ref().and_then(|t| t.kept(&about));
        if let Some(k) = kept.filter(|k| k.wind_h.is_none()) {
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
                let d = guided::results_dialog(&k, &self.guided_dir());
                self.save_text_as(d, "the Positions Test's results", k.to_json());
            }
        }
    }

    /// An opened take's six positions as the pane's This Take shows them,
    /// with no test on the Bench and no saved 24 hour half on show: None
    /// when none was read.
    pub(super) fn positions_kept(
        &self,
        ctx: &egui::Context,
    ) -> Option<Vec<timegrapher_core::session::Reading>> {
        if self.guided.is_some() || self.guided_saved.is_some() {
            return None;
        }
        self.free_table_shown(ctx)
            .map(|t| t.six.0)
            .filter(|k| !k.is_empty())
    }

    /// Save as Text…: the plain-text report (core `text_report`) of the
    /// test, or of an opened take's `kept` positions, to a file the person
    /// picks.
    fn save_test_text(
        &mut self,
        about: &guided::About,
        kept: Option<&[timegrapher_core::session::Reading]>,
    ) {
        let dir = self.guided_dir();
        // The take's name, which starts with when it began: as saved, or
        // as opened; before it is saved, the test's own start, named the
        // same way.
        let saved = self
            .session
            .saved_to()
            .map(Path::to_path_buf)
            .or_else(|| self.take_on_show())
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
        let lift_known = self.lift_known();
        let text = match (self.positions_test(), kept) {
            (Some(t), _) => {
                let take = saved.unwrap_or_else(|| guided::began_name(t));
                guided::report_text(t, about, lift_known, Some(&take))
            }
            (None, Some(k)) => guided::kept_report_text(
                k,
                guided::Kind::Positions,
                about,
                lift_known,
                saved.as_deref(),
            ),
            _ => None,
        };
        let Some(text) = text else { return };
        let d = guided::text_dialog(guided::Kind::Positions, &about.watch, &dir);
        self.save_text_as(d, "the text report", text);
    }

    /// The Positions Test pane: the banner and the tables, and what its
    /// buttons ask for.
    /// The pane's toolbar, with the Info toggle pinned at its right.
    pub(super) fn positions_tools(&mut self, ui: &mut egui::Ui, title: &str) {
        let mut info = self.positions_info;
        let hint = "Info: how the table for a take is read";
        theme::pane_toolbar_info(ui, title, &help::POSITIONS_TEST, (&mut info, hint), |ui| {
            self.positions_settings(ui);
            self.export_button(ui, Pane::PositionsTest);
        });
        self.positions_info = info;
    }

    pub(super) fn positions_pane(&mut self, ui: &mut egui::Ui) {
        self.results_rects[0] = Some(ui.max_rect());
        let now = self.live.as_ref().map(|l| l.duration_s());
        // A Dynamic Poise test is its own pane's: this one shows only how
        // the Positions Test runs.
        let ours = |t: &guided::Test| t.kind == guided::Kind::Positions;
        let phase = match (self.guided.as_mut().filter(|t| ours(t)), now) {
            (Some(t), Some(now)) => Some(t.tick(now)),
            _ => None,
        };
        let free = if self.guided.is_none() {
            self.free_table_shown(ui.ctx())
        } else {
            None
        };
        // A take on show being read again: that alone, not the words for
        // a pane with no take.
        if self.guided.is_none() && free.is_none() && self.free_table_reading() {
            let pal = theme::pal(ui);
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(12.0).color(pal.text_secondary));
                ui.label(
                    RichText::new("Reading the take…")
                        .small()
                        .color(pal.text_secondary),
                );
            });
            return;
        }
        let live = match phase {
            Some(guided::Phase::Measuring(_)) => self.guided_live(ui.ctx()),
            _ => None,
        };
        // Set up as the 24 hours after half with no full-wind test to set
        // it against: said here, and Start Test waits.
        let note = self.start_test_why_not();
        let action = guided::pane(
            ui,
            self.guided.as_ref().filter(|t| ours(t)),
            phase,
            self.guided_earlier.as_ref(),
            self.guided_saved.as_ref(),
            free.as_ref(),
            live.as_ref(),
            self.guided_settle_s,
            self.open_watch().is_some(),
            note.as_deref(),
            (self.position_card_title, self.positions_info),
        );
        let Some(action) = action else { return };
        if action == guided::Action::ClearEarlier {
            self.guided_earlier = None;
            self.guided_earlier_file = false;
            self.guided_after = false;
            return;
        }
        let Some(now) = now else { return };
        self.apply_guided_action(action, now);
    }

    /// A guided test's banner button, from either test's pane (#1563):
    /// the step it asks for, then the strip's not-used and measured
    /// stretches and the mask follow the test.
    pub(super) fn apply_guided_action(&mut self, action: guided::Action, now: f64) {
        let Some(mut test) = self.guided.take() else {
            return;
        };
        match action {
            // Noted in the take like a position picked by hand, so its
            // session.json follows the test. After the last stop there is
            // no Next: the test ends by itself.
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
        self.mask_engine();
    }
}
