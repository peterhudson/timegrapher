//! The Power Reserve pane: its settings, and the test's start, tick, end and save.

use super::*;

impl TimegrapherApp {
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
                 the {} card (or keys 1 to 6) when Start Test is pressed.",
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
                let dir = self.guided_dir();
                if let Some(m) = reserve::save_csv(&reserve::results_text(r, &watch, ','), &dir) {
                    self.info_msg(m);
                }
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

    /// Follow a running Power Reserve Test, whether its pane shows or
    /// not: read the beats every few seconds, and end it at the stop.
    pub(super) fn reserve_tick(&mut self, ctx: &egui::Context) {
        if !self.reserve_running() {
            return;
        }
        let Some(live) = self.live.as_ref() else {
            return;
        };
        let now = live.duration_s();
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        if now - self.reserve_read_s < 2.0 {
            return;
        }
        self.reserve_read_s = now;
        let beat_s = power_reserve::beat_s(live.bph());
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
        if now - self.reserve_hours_s < 60.0 {
            return;
        }
        self.reserve_hours_s = now;
        let Some((first, last, whole)) = self.reserve.as_ref().and_then(|t| {
            let whole = t.so_far.iter().take_while(|h| h.whole()).count();
            Some((t.heard.first_s?, t.heard.last_s?, whole))
        }) else {
            return;
        };
        let from = first + 3600.0 * whole as f64;
        let mut fresh = self.reserve_hours(from, last);
        for h in &mut fresh {
            h.hour += whole as u32;
        }
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

    /// The run's hours from audio second `from` to `to`, as `timegrapher
    /// session` reads each stretch, on the session's clock, breaks and
    /// masks.
    fn reserve_hours(&self, from: f64, to: f64) -> Vec<power_reserve::Hour> {
        let Some(log) = self.live.as_ref().and_then(steady::log_of) else {
            return Vec::new();
        };
        let fit = self
            .clock_track
            .as_ref()
            .and_then(|c| c.fit.clone())
            .or_else(|| self.reading_clock(log.duration_s));
        let breaks = self.reading_breaks(&log);
        power_reserve::hours(&log, fit.as_ref(), &breaks, &self.masked, from, to)
    }

    /// The test has ended: stop listening, keep the take in the watch's
    /// history with its results, and as its analysis only if so picked.
    pub(super) fn end_reserve(&mut self) {
        self.pause();
        let to_true = self.to_true();
        let Some(t) = self.reserve.as_mut() else {
            return;
        };
        if let Some(e) = t.ending {
            applog::info(&format!("power reserve test ended: {}", e.text()));
        }
        let mut results = t.finish(to_true).cloned();
        // Hour by hour, as `timegrapher session` reads each stretch.
        if let Some(r) = results.as_mut() {
            r.hours = self.reserve_hours(r.first_beat_s, r.last_beat_s);
        }
        // The results go into the take with its sound, through
        // `results_files`.
        let hours = results.as_ref().map(|r| r.hours.len());
        if let Some(t) = self.reserve.as_mut() {
            t.results = results;
        }
        if let Some(n) = hours {
            grow_row(&mut self.panes.tiles, Pane::PowerReserve, reserve::run_h(n));
        }
        // With no watch picked, asked where it goes before it is saved.
        if self.open_watch().is_none() {
            self.test_keep = Some(TestKeep::PowerReserve);
            return;
        }
        self.save_reserve();
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
        let dir = library::unused_dir(&takes, &format!("{started} Power Reserve"));
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
                let (rate, bits) = self
                    .capture
                    .as_ref()
                    .map_or((48_000, 16), |c| (c.sample_rate, c.bits));
                Some((have, reserve::bytes_needed(rate, bits)))
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
            self.position_card_title,
            &watch,
            self.ready_test() == Some(NewTake::PowerReserve),
        );
    }
}
