//! The Power Reserve pane: its settings, and the test's start, tick, end and save.

use super::*;

/// The Power Reserve pane's Info button (#1528).
const RESERVE_INFO_HINT: &str = "Info: the run in full, where its take is kept, the beats' \
     pauses and what the grey cells mean.";

impl TimegrapherApp {
    /// The Power Reserve pane's toolbar: its settings, its "?" naming the
    /// position card as captioned, and its Info button pinned at the right.
    pub(super) fn reserve_toolbar(&mut self, ui: &mut egui::Ui, title: &str) {
        let help = help::power_reserve(self.position_card_title);
        let help: Vec<&str> = help.iter().map(String::as_str).collect();
        let mut info = self.reserve_info;
        theme::pane_toolbar_info(ui, title, &help, (&mut info, RESERVE_INFO_HINT), |ui| {
            self.reserve_settings(ui);
        });
        self.reserve_info = info;
    }

    /// Whether the pane holds a Power Reserve Test, under way or ended,
    /// not a long take's run read as one ([`power_reserve::Source`]).
    pub(super) fn reserve_is_test(&self) -> bool {
        self.reserve
            .as_ref()
            .is_some_and(|t| t.results.as_ref().is_none_or(|r| r.source.is_test()))
    }

    /// Whether a Power Reserve Test is under way, not yet ended.
    pub(super) fn reserve_running(&self) -> bool {
        self.reserve.as_ref().is_some_and(|t| t.running())
    }

    /// Where a Power Reserve Test keeps its take: the picked watch's
    /// takes, or the library's Unsorted with none picked.
    fn reserve_takes_dir(&mut self) -> Result<PathBuf, String> {
        self.ensure_library();
        match &self.library {
            Ok(Some(l)) => {
                if let Some(why) = &l.read_only {
                    return Err(why.clone());
                }
                match self.open_watch() {
                    Some(e) => Ok(e.dir.join(library::TAKES)),
                    None => l.unsorted_dir(),
                }
            }
            Ok(None) => Err("There is no library".into()),
            Err(e) => Err(format!("The library can't be read: {e}")),
        }
    }

    /// Audio seconds to true seconds for the session: the input's clock
    /// as measured while listening, else its stored correction.
    fn to_true(&self) -> impl Fn(f64) -> f64 + use<> {
        let fit = self
            .clock_track
            .as_ref()
            .and_then(|c| c.fit.clone())
            .or_else(|| self.reading_clock(self.live.as_ref().map_or(0.0, |l| l.duration_s())));
        move |s| fit.as_ref().map_or(s, |f| f.map(s))
    }

    /// The Power Reserve pane's toolbar: the position it runs in (set on
    /// the Current Position card), the keeping, and Start Test.
    pub(super) fn reserve_settings(&mut self, ui: &mut egui::Ui) {
        let running = self.reserve_running();
        let pal = theme::pal(ui);
        let p = self.position;
        let words = if running {
            format!("Runs in {} ({})", p.name(), p.code())
        } else {
            format!(
                "Runs in {} ({}), set on the {} card",
                p.name(),
                p.code(),
                self.position_card_title
            )
        };
        ui.add(egui::Label::new(
            RichText::new(words).color(pal.text_secondary),
        ))
        .on_hover_text(if running {
            "The one position the watch runs down in, from wind to stop. Stop Test to \
                 change it."
                .to_string()
        } else {
            format!(
                "The one position the watch runs down in, from wind to stop: the one set on \
                 the {} card (or its key) when Start Test is pressed.",
                self.position_card_title
            )
        });
        theme::bar_gap(ui);
        ui.add_enabled_ui(!running, |ui| {
            theme::tool_choice(
                ui,
                "Keep",
                "What the take keeps once the run ends",
                &mut self.reserve_keep_sound,
                &[
                    (
                        true,
                        Icon::Wave,
                        "Keep Sound",
                        "Keep the run's sound in the take, to measure again later; about a \
                         third of a GB an hour",
                    ),
                    (
                        false,
                        Icon::Sheet,
                        "Analysis Only",
                        "Keep the take as its analysis only, tens of MB: every pane still \
                         reads it, but it can't be measured again. The sound is still written \
                         while the test runs and is deleted once the analysis is written.",
                    ),
                ],
            );
        });
        // Start Test and Stop Test… are on the Listen card; set up, Cancel
        // Test leaves the Bench as it was.
        if !running {
            theme::bar_gap(ui);
            if self.ready_test() == Some(NewTake::PowerReserve) {
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
                "Set a Power Reserve Test up on a clear Bench: wind the watch fully, set the \
                 position on the Position card, then press Start Test on the Listen card. It \
                 runs until the watch stops, keeping the computer awake.",
                true,
                None,
            ) {
                self.new_take(NewTake::PowerReserve, false);
            }
        }
        self.reserve_results_buttons(ui);
    }

    /// The Power Reserve settings' Save as CSV… and Copy, for the run that
    /// ended last.
    fn reserve_results_buttons(&mut self, ui: &mut egui::Ui) {
        let done = self.reserve.as_ref().and_then(|t| t.results.clone());
        let nothing = done.is_none().then_some("No run ended yet");
        theme::bar_gap(ui);
        let watch = self.watch.trim().to_string();
        if theme::tool_button(
            ui,
            Icon::SaveTable,
            "Save as CSV…",
            "Save the watch, the run and its hour-by-hour table as a CSV file a spreadsheet \
             opens",
            nothing,
        ) {
            if let Some(r) = &done {
                let d = reserve::csv_dialog(&self.guided_dir());
                let text = reserve::results_text(r, &watch, ',');
                self.save_text_as(d, "the Power Reserve Test", text);
            }
        }
        if theme::tool_word_button(
            ui,
            Icon::Copy,
            "Copy",
            "Copy the watch, the run and its hour-by-hour table with tabs between the columns, \
             to paste into a spreadsheet or a message",
            false,
            nothing,
        ) {
            if let Some(r) = &done {
                ui.ctx().copy_text(reserve::results_text(r, &watch, '\t'));
                self.info_msg("Copied the Power Reserve Test".to_string());
            }
        }
    }

    /// Begin a Power Reserve Test: a new microphone session in the
    /// Current Position card's position, kept in the watch's takes when it
    /// ends.
    #[cfg(test)]
    pub(super) fn start_reserve(&mut self) {
        self.start_reserve_asked(false);
    }

    /// [`Self::start_reserve`], `asked` once the question about unsaved
    /// sound has been answered.
    pub(super) fn start_reserve_asked(&mut self, asked: bool) {
        if let Err(e) = self.reserve_takes_dir() {
            return self.error(format!("A Power Reserve Test can't keep its take: {e}"));
        }
        // Quick Listen's sound too, listening or not: a test of a watch
        // isn't its sound. The same question as a new take's; listening
        // stops once it is answered.
        if !asked && self.line_pick_asks() {
            self.confirm = Some(Confirm::ReserveStart);
            return;
        }
        if self.running() {
            self.pause();
        }
        // Whatever the status line said before is past.
        self.message = None;
        self.reserve_session();
        self.start_microphone();
        if !self.running() || self.session.recorder.is_none() {
            return;
        }
        self.begin_reserve();
    }

    /// A fresh microphone session for a Power Reserve Test, in the
    /// position the card and the pane named, even where it came from a
    /// take opened over listening's own.
    pub(super) fn reserve_session(&mut self) {
        let pos = self.position;
        if self.live.is_some() || self.session.recorder.is_some() {
            self.new_session();
        }
        self.position = pos;
        self.input = Input::Microphone;
        // Never Quick Listen, whose sound goes without a question: a test
        // keeps its take.
        self.quick_listen = false;
    }

    /// The test itself, once its session is listening: in the position the
    /// card has now, which it then holds.
    pub(super) fn begin_reserve(&mut self) {
        // A Power Reserve Test begins at full wind.
        self.started_from_full_wind();
        let pos = self.position;
        let started = self
            .session
            .recorder
            .as_ref()
            .map_or_else(String::new, |r| r.started_utc().to_string());
        let now = self.live.as_ref().map_or(0.0, |l| l.duration_s());
        if let Some(l) = self.live.as_mut() {
            let n = l.config().max_beats.max(reserve::RUN_BEATS_KEPT);
            l.set_max_beats(n);
        }
        self.reserve = Some(reserve::Test::new(
            pos,
            self.reserve_keep_sound,
            started,
            now,
        ));
        self.reserve_read_s = now;
        self.reserve_hours_s = f64::NEG_INFINITY;
        applog::info(&format!(
            "power reserve test started in {}, {}",
            pos.code(),
            if self.reserve_keep_sound {
                "keeping the sound"
            } else {
                "keeping the analysis only"
            }
        ));
        self.reveal_pane(Pane::PowerReserve);
    }

    /// Keep listening while the window isn't drawing: save and read the
    /// sound that has come in, and follow a Power Reserve Test to its
    /// end. Called from [`crate::pump`]'s thread.
    pub(crate) fn pump(&mut self) -> usize {
        if self.capture.is_none() {
            return 0;
        }
        let taken = self.poll_for(super::PUMP_BUDGET);
        self.reserve_follow();
        taken
    }

    /// [`Self::reserve_follow`] from a frame of the window, which draws
    /// again each second while the test runs.
    pub(super) fn reserve_tick(&mut self, ctx: &egui::Context) {
        self.reserve_follow();
        if self.reserve_running() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }

    /// Follow a running Power Reserve Test, whether its pane shows or
    /// not: read the beats every few seconds, and end it at the stop or
    /// the time limit. Called from the window's frames and from the
    /// listening thread's ([`Self::pump`]), so a window that isn't drawn
    /// (minimised, a locked screen) still ends the test (#1647).
    pub(super) fn reserve_follow(&mut self) {
        self.reserve_write_again();
        if !self.reserve_running() {
            return;
        }
        let Some(live) = self.live.as_ref() else {
            return;
        };
        let now = live.duration_s();
        if now - self.reserve_read_s < 2.0 {
            return;
        }
        self.reserve_read_s = now;
        let beat_s = power_reserve::beat_s(self.reserve_bph());
        let Some(live) = self.live.as_ref() else {
            return;
        };
        let ended = self
            .reserve
            .as_mut()
            .and_then(|t| t.update(live.beats(), beat_s, now));
        if ended.is_some() {
            self.end_reserve();
            return;
        }
        // The hours so far, about once a minute: the whole hours kept,
        // the newest measured again.
        if now - self.reserve_hours_s < power_reserve::READ_AGAIN_S {
            return;
        }
        self.reserve_hours_s = now;
        let Some((first, last, whole)) = self.reserve.as_ref().and_then(|t| {
            let whole = t.so_far.iter().take_while(|h| h.whole()).count();
            Some((t.heard.first_s?, t.heard.last_s?, whole))
        }) else {
            return;
        };
        // On core's grid of true hours, so a pause or a card running
        // fast or slow doesn't shift the hours measured again (#642).
        let fresh = self.reserve_hours(first, last, whole as u32);
        let mut first_chart = false;
        if let Some(t) = self.reserve.as_mut() {
            t.so_far.truncate(whole);
            t.so_far.extend(fresh);
            // The first whole hour brings the charts: room for them, once.
            first_chart = whole == 0 && t.so_far.iter().any(|h| h.whole());
        }
        if first_chart {
            let n = self.reserve.as_ref().map_or(0, |t| t.so_far.len());
            grow_row(&mut self.panes.tiles, Pane::PowerReserve, reserve::run_h(n));
        }
    }

    /// The beat rate the running test reads its beats at: the session's
    /// while locked, kept in the test, else the rate it last ran at
    /// ([`reserve::Test::bph`]).
    fn reserve_bph(&mut self) -> Option<u32> {
        let now = self.live.as_ref().and_then(|l| l.bph());
        let t = self.reserve.as_mut()?;
        if now.is_some() {
            t.bph = now;
        }
        t.bph
    }

    /// The run's hours from its first beat at audio second `first` to
    /// `to`, from hour `from_hour` on ([`power_reserve::hours_from`]), as
    /// `timegrapher session` reads each stretch, on the session's clock,
    /// breaks and masks.
    fn reserve_hours(&self, first: f64, to: f64, from_hour: u32) -> Vec<power_reserve::Hour> {
        let bph = self.reserve.as_ref().and_then(|t| t.bph);
        let Some(log) = self
            .live
            .as_ref()
            .and_then(|l| steady::log_of_at(l, l.bph().or(bph)))
        else {
            return Vec::new();
        };
        let fit = self
            .clock_track
            .as_ref()
            .and_then(|c| c.fit.clone())
            .or_else(|| self.reading_clock(log.duration_s));
        let breaks = self.reading_breaks(&log);
        power_reserve::hours_from(
            &log,
            fit.as_ref(),
            &breaks,
            &self.masked,
            first,
            to,
            from_hour,
        )
    }

    /// The Power Reserve pane's row as the pane is first shown: at its
    /// content's height, no less than a pane's least, the Paper Strip
    /// keeping the rest until dragged; with a run already read (a long
    /// take's, read before the pane showed), grown to show its charts and
    /// table, as an opened run's take is.
    pub(super) fn fit_reserve_row(&mut self) {
        let pane = Pane::PowerReserve;
        match self.reserve.as_ref().and_then(|t| t.results.as_ref()) {
            Some(r) => grow_row(&mut self.panes.tiles, pane, reserve::run_h(r.hours.len())),
            None => fit_row(&mut self.panes.tiles, pane, MIN_PANE_H),
        }
    }

    /// A take opened whole that isn't a Power Reserve Test: when core
    /// reads it as a run ([`power_reserve::from_long_take`]: six hours or
    /// more of the watch heard, in one position), the pane shows that run
    /// as it shows a test's, saying where it came from. Nothing is written
    /// into the take, and the pane stays where it is. Read here and now,
    /// as the tests read it; the window reads it in the background
    /// ([`Self::long_reserve_shown`], #1014).
    #[cfg(test)]
    pub(super) fn reserve_from_long_take(&mut self) {
        if let Some(read) = self.long_take_reader() {
            self.reserve = read();
        }
    }

    /// The reading of [`Self::reserve_from_long_take`], ready to run here
    /// or in the background: none when the pane already has a test or
    /// there is no take.
    pub(super) fn long_take_reader(
        &self,
    ) -> Option<impl FnOnce() -> Option<reserve::Test> + Send + 'static> {
        if self.reserve_is_test() {
            return None;
        }
        let live = self.live.clone()?;
        let (take_clock, opened) = (self.take_clock.clone(), self.take_breaks.is_some());
        let (ppm, take_breaks) = (self.clock.as_ref().map(|c| c.ppm), self.take_breaks.clone());
        let codes: Vec<&'static str> = self
            .session
            .stretches()
            .iter()
            .map(|(_, p)| p.code())
            .collect();
        let codes = if codes.is_empty() {
            vec![self.position.code()]
        } else {
            codes
        };
        // Its settlings left out as a mask is, as session reads the run
        // (#1233); the run still starts at its first beat, so the log
        // leaves out only the masks by hand.
        let (by_hand, masked) = (self.masked.clone(), self.engine_masked());
        let (position, sound) = (self.position, self.no_sound.is_none());
        Some(move || {
            let log = steady::log_masked(&live, &by_hand)?;
            let fit = reading_clock(take_clock.as_ref(), opened, ppm, log.duration_s);
            let breaks = steady::reading_breaks(take_breaks.unwrap_or_default(), &log);
            let r = power_reserve::from_long_take(
                &log,
                fit.as_ref(),
                &breaks,
                &masked,
                &codes,
                "",
                sound,
            )?;
            let position = Position::from_name(&r.position).unwrap_or(position);
            let mut t = reserve::Test::new(position, r.sound_kept, String::new(), 0.0);
            t.ending = Some(r.ending);
            t.results = Some(r);
            Some(t)
        })
    }

    /// The test has ended: stop listening, keep the take in the watch's
    /// history with its results, and as its analysis only if so picked.
    pub(super) fn end_reserve(&mut self) {
        self.pause();
        // The results go into the take with its sound, through
        // `results_files`.
        self.finish_reserve();
        // With no watch picked, asked where it goes before it is saved.
        if self.open_watch().is_none() {
            self.test_keep = Some(TestKeep::PowerReserve);
            return;
        }
        self.save_reserve();
    }

    /// The ended test's results, hour by hour as `timegrapher session`
    /// reads each stretch, with room in the pane for its charts and table.
    fn finish_reserve(&mut self) {
        let to_true = self.to_true();
        let Some(t) = self.reserve.as_mut() else {
            return;
        };
        if let Some(e) = t.ending {
            applog::info(&format!("power reserve test ended: {}", e.text()));
        }
        let mut results = t.finish(to_true).cloned();
        if let Some(r) = results.as_mut() {
            r.hours = self.reserve_hours(r.first_beat_s, r.last_beat_s, 0);
        }
        let hours = results.as_ref().map(|r| r.hours.len());
        if let Some(t) = self.reserve.as_mut() {
            t.results = results;
        }
        if let Some(n) = hours {
            grow_row(&mut self.panes.tiles, Pane::PowerReserve, reserve::run_h(n));
        }
    }

    /// The recording failed while a Power Reserve Test ran: the run ends
    /// there, read to its last beat heard, at least that long and never a
    /// stop of the watch ([`power_reserve::Ending::RecordingFailed`]). Its
    /// results go into the take it was recording into, when it was; the
    /// take keeps its sound, as no save can follow a failed recording
    /// (#1380).
    pub(super) fn reserve_failed(&mut self) {
        if !self.reserve_running() {
            return;
        }
        // The beats heard up to the failure, as a tick reads them: a stop
        // found there is the watch's own.
        let beat_s = power_reserve::beat_s(self.reserve_bph());
        if let (Some(live), Some(t)) = (self.live.as_ref(), self.reserve.as_mut()) {
            t.update(live.beats(), beat_s, live.duration_s());
        }
        if let Some(t) = self.reserve.as_mut() {
            if t.running() {
                t.ending = Some(power_reserve::Ending::RecordingFailed);
            }
            // Nothing follows to keep it as its analysis alone: the take
            // keeps its sound, and says so.
            t.keep_sound = true;
        }
        self.finish_reserve();
        if let Some(take) = self.session.saved_to().map(Path::to_path_buf) {
            self.write_reserve_into(take);
        }
    }

    /// Write the ended run's results into its take `take`; when that
    /// fails, tried again a minute later ([`Self::reserve_write_again`]).
    fn write_reserve_into(&mut self, take: PathBuf) {
        let Some(t) = self.reserve.as_mut() else {
            return;
        };
        t.unwritten = None;
        let Some(r) = t.results.as_ref() else {
            return;
        };
        match power_reserve::write(&take, r) {
            Ok(()) => {
                t.saved = Some(take);
                if let Ok(Some(l)) = &mut self.library {
                    l.scan();
                }
            }
            Err(e) => {
                applog::error(&format!(
                    "the power reserve results couldn't be written into {}: {e}",
                    take.display()
                ));
                t.unwritten = Some((take, Instant::now()));
            }
        }
    }

    /// Results whose write failed, written again once a minute until
    /// they are; the window draws for each try with no input
    /// ([`Self::reserve_retry_in`], #1405).
    fn reserve_write_again(&mut self) {
        let Some((take, at)) = self.reserve.as_ref().and_then(|t| t.unwritten.clone()) else {
            return;
        };
        if at.elapsed() >= STREAM_RETRY {
            self.write_reserve_into(take);
        }
    }

    /// How long until the next try at results whose write failed, for
    /// the window to draw then on its own.
    pub(super) fn reserve_retry_in(&self) -> Option<std::time::Duration> {
        let (_, at) = self.reserve.as_ref()?.unwritten.as_ref()?;
        Some(STREAM_RETRY.saturating_sub(at.elapsed()))
    }

    /// Save the ended Power Reserve Test's take, with its results, into
    /// the picked watch's takes or Unsorted.
    pub(super) fn save_reserve(&mut self) {
        let takes = match self.reserve_takes_dir() {
            Ok(d) => d,
            Err(e) => return self.error(format!("The test's take couldn't be kept: {e}")),
        };
        let Some(r) = self.session.recorder.as_mut() else {
            return;
        };
        let started = watches::local_minute(r.started_utc())
            .unwrap_or_else(|| r.started_utc().replace(':', "."));
        let dir = library::unused_dir(&takes, &reserve_take_name(&started));
        if let Some(name) = dir.file_name() {
            r.name_first_copy(name.to_owned());
        }
        self.save_into(&takes);
        if let Some(t) = self.reserve.as_mut() {
            t.saving = self.saving.is_some();
        }
        self.reserve_saved();
    }

    /// Once the test's take is written: note where, and keep it as its
    /// analysis only if so picked.
    pub(super) fn reserve_saved(&mut self) {
        if self.saving.is_some() {
            return;
        }
        let Some(t) = self.reserve.as_mut().filter(|t| t.saving) else {
            return;
        };
        t.saving = false;
        let keep_sound = t.keep_sound;
        let Some(saved) = self.session.saved_to().map(Path::to_path_buf) else {
            return;
        };
        if let Some(t) = self.reserve.as_mut() {
            t.saved = Some(saved.clone());
        }
        if let Ok(Some(l)) = &mut self.library {
            l.scan();
        }
        if !keep_sound {
            self.start_take_keep(saved);
        }
    }

    /// The Power Reserve pane: the run so far, or its result.
    pub(super) fn reserve_pane(&mut self, ui: &mut egui::Ui) {
        let free = if self.reserve.is_none() {
            self.reserve_takes_dir().ok().and_then(|d| {
                let have = reserve::free_bytes(&d)?;
                let rate = self.capture.as_ref().map_or(48_000, |c| c.sample_rate);
                Some((have, reserve::bytes_needed(rate)))
            })
        } else {
            None
        };
        let watch = self.watch.trim().to_string();
        // Its charts at a readable height, never a sliver under the strip.
        if let Some(least) = reserve::least_h(self.reserve.as_ref()) {
            let above = ui.cursor().top() - self.pane_top;
            self.pane_need.insert(Pane::PowerReserve, above + least);
        } else {
            self.pane_need.remove(&Pane::PowerReserve);
        }
        reserve::pane(
            ui,
            self.reserve.as_ref(),
            free,
            (self.position_card_title, &watch),
            self.ready_test() == Some(NewTake::PowerReserve),
            self.reserve_info,
        );
    }
}

/// The folder name of a Power Reserve Test's take started at `started`
/// (local time, as a watch's takes are named).
pub(super) fn reserve_take_name(started: &str) -> String {
    format!("{started} Power Reserve")
}
