//! Listening and saving: the microphone, the session and its unsaved state, saving takes and analysis, opening and replaying recordings, the whole-take analysis, and the poll that feeds the live view.

use super::*;
pub(super) use timegrapher_core::recording_order::order_warning;
use timegrapher_core::take_context::{EntryWindow, FilePart};

/// An ask that goes ahead once the save being written is done
/// ([`TimegrapherApp::behind_save`]).
pub(super) type AfterSave = Box<dyn FnOnce(&mut TimegrapherApp) + Send>;

impl TimegrapherApp {
    pub(super) fn live_config(&self) -> LiveConfig {
        config_for(&self.open_inputs(&[], &[]))
    }

    /// What a take opening now rests on: its positions as its session
    /// file gives them (`stretches`, `settles`), or as they were for one
    /// read again at a new beat rate.
    fn inputs_for(&self, stretches: &[(f64, Position)], settles: &[(f64, f64)]) -> OpenInputs {
        match &self.carried_positions {
            Some((now, _)) => self.open_inputs(&now.0 .0, &now.0 .1),
            None => self.open_inputs(stretches, settles),
        }
    }

    /// What a take opening now rests on, its positions beginning at
    /// `stretches` with `settles` before their readings.
    pub(super) fn open_inputs(
        &self,
        stretches: &[(f64, Position)],
        settles: &[(f64, f64)],
    ) -> OpenInputs {
        OpenInputs {
            bph: self.bph,
            lift_deg: self.lift_deg,
            lift_known: self.lift_known(),
            average_s: self.average_s,
            masked: self.masked.clone(),
            changes: stretches.iter().map(|s| s.0).collect(),
            settles: settles.to_vec(),
        }
    }

    /// Seconds of beats the tick and tock sound covers: the averaging time,
    /// so it describes the same beats as the readings, up to the engine's
    /// limit.
    pub(super) fn sound_span(&self) -> f64 {
        profile_span_s(self.average_s)
    }

    /// A new averaging time, for the readings and every pane: the readings
    /// over the whole session are worked out again, and the tick and tock
    /// sound follows it.
    pub(super) fn set_average(&mut self, seconds: f64) {
        self.average_s = seconds;
        let span = self.sound_span();
        if let Some(l) = self.live.as_mut() {
            l.set_profile_span(span);
        }
        // The sound kept so far covered the old span.
        self.sound_at = None;
        self.rebuild_trend();
    }

    /// Clear every run of readings, for a new session or a change that
    /// makes them all out of date; new readings fill them again.
    pub(super) fn clear_trends(&mut self) {
        self.strip_points = None;
        self.trend.clear();
    }

    pub(super) fn info(&self, device: &str, sample_rate: u32, bits: u16) -> SessionInfo {
        SessionInfo {
            watch: self.watch.clone(),
            position: self.position.code().into(),
            lift_deg: self.lift_deg,
            bph: self.bph,
            device: device.into(),
            sample_rate,
            bits,
            software: format!("timegrapher-app {}", env!("CARGO_PKG_VERSION")),
        }
    }

    pub(super) fn reset_session(&mut self, sample_rate: u32, label: String) {
        // Never throw away the sound a save is still copying.
        self.wait_for_save();
        self.session.discard_recording();
        self.session.clear();
        self.put_input_clock(None);
        self.live = Some(LiveAnalyzer::new(sample_rate, self.live_config()));
        self.source_label = label;
        self.gutter_fresh = true;
        self.anchor = None;
        self.clear_trends();
        self.strip_end = None;
        self.level = Level::default();
        self.clipped_at = None;
        self.message = None;
        self.sound_history.clear();
        self.no_sound = None;
        self.analysis_kept_to = None;
        self.guided = None;
        self.reserve = None;
        self.bg.reserve_live = None;
        self.bg.reserve_live_s = 0.0;
        self.guided_unused.clear();
        self.guided_measured.clear();
        // Its masks, once nothing of the last test's is left.
        self.mask_engine();
        self.rev.new_take();
    }

    /// List the inputs, once, and keep the chosen one if it is still there,
    /// else take the first.
    pub(super) fn ensure_devices(&mut self) {
        if !self.devices_listed {
            self.rescan();
        }
    }

    /// List the inputs again, after one is plugged in.
    pub(super) fn rescan(&mut self) {
        self.devices = capture::list().unwrap_or_default();
        self.devices_listed = true;
        let there = self
            .device
            .as_ref()
            .is_some_and(|d| self.devices.iter().any(|x| &x.id == d));
        if !there {
            self.device = capture::choices(&self.devices)
                .first()
                .map(|c| c.id.clone());
        }
    }

    /// Listen, the masks on the recording on show saved or let go: and
    /// begin the Positions Test when that is what asked. They stay on show
    /// until listening starts, so a microphone that fails to open leaves
    /// them to save.
    pub(super) fn listen_after_ask(&mut self) {
        self.masks_let_go = true;
        match self.guided_after_ask.take() {
            Some((kind, half)) => self.start_guided_test(kind, half),
            None => self.start_microphone(),
        }
        self.masks_let_go = false;
    }

    /// Listen to the microphone: carry on the paused session if there is
    /// one, else start a new one.
    pub(super) fn start_microphone(&mut self) {
        let let_go = std::mem::take(&mut self.masks_let_go);
        if self.refuse_faulted_recording() || self.analysis_save_busy() {
            return;
        }
        // A save being written carries on through a resume, but not a new
        // session, which throws away the sound it copies.
        if !self.can_resume() && self.saving_busy() {
            return;
        }
        // Listening replaces the recording on show: changed masks on it
        // are asked about first.
        if !let_go && self.marks_unsaved() {
            self.confirm = Some(Confirm::Microphone);
            return;
        }
        if self.can_resume() {
            self.ensure_devices();
            return self.resume_microphone();
        }
        // A take's fitted view is for the take: listening goes back to the
        // strip as it was.
        self.restore_live_strip();
        let clocks = match clockstore::ClockStore::load() {
            Ok(s) => s,
            Err(e) => return self.error(format!("Can't load the input's clock correction: {e}")),
        };
        self.ensure_devices();
        self.stop();
        if let Some(s) = self.mic_setup.take() {
            self.set_watch_setup(s);
        }
        match capture::open_input(self.device.as_deref(), None) {
            Ok(c) => {
                let label = self.device_label(&c);
                applog::info(&format!(
                    "microphone opened: {label}, {} Hz, {}-bit",
                    c.sample_rate, c.bits
                ));
                // A new take: nothing of it masked yet.
                self.put_masked(Vec::new());
                self.opened_masks = None;
                self.opened_positions = None;
                self.put_take_clock(None, None);
                self.put_take_breaks(None, None);
                self.masking = false;
                self.brush = None;
                self.reset_session(
                    c.sample_rate,
                    format!("{label} at {} Hz, {}-bit", c.sample_rate, c.bits),
                );
                self.opened_at = Instant::now();
                self.audio_at = None;
                self.gaps = None;
                self.session.listen();
                self.put_input_clock(clocks.get(&c.label).cloned());
                self.clock_track = Some(ClockTrack {
                    device: c.label.clone(),
                    shown: label.clone(),
                    tracker: timegrapher_core::clock::ClockTracker::new(c.sample_rate),
                    next_s: CLOCK_MEASURE_S,
                    fit: None,
                });
                // Every session is kept as it goes, so it can be saved at any
                // point; it is thrown away with a new session unless saved.
                let info = self.info(&c.label, c.sample_rate, c.bits);
                match Recorder::start(&scratch_dir(), info) {
                    Ok(r) => self.session.recorder = Some(r),
                    Err(e) => self.error(format!(
                        "Can't keep the sound for saving later: {e}. The readings still work."
                    )),
                }
                self.input_fault = None;
                self.capture = Some(c);
                self.apply_agc();
            }
            Err(e) => {
                let m = format!("Can't open the input. {}", capture::explain_error(&e));
                self.input_fault = Some(m.clone());
                self.error(m);
            }
        }
    }

    fn device_label(&self, c: &Capture) -> String {
        capture::choices(&self.devices)
            .into_iter()
            .find(|ch| Some(&ch.id) == self.device.as_ref())
            .map_or_else(|| c.label.clone(), |ch| ch.label)
    }

    /// A microphone session that is paused and can carry on.
    pub(super) fn can_resume(&self) -> bool {
        self.session.allows(Action::Resume) && self.live.is_some() && !self.running()
    }

    /// Let go of the input. Done on a thread of its own with a short
    /// wait: an input whose sound server died can hang as it closes, and
    /// the window must not hang with it.
    fn release_capture(&mut self) {
        if let Some(c) = self.capture.take() {
            if !drop_within(c, RELEASE_WAIT) {
                applog::warn("the input didn't close in time; carrying on without it");
            }
        }
    }

    /// The events a live input has already sent and the window hasn't
    /// taken yet, as listening takes them: what core's gap rule lets
    /// through, and why the input failed, if it did. A replayed file is
    /// left as it was.
    fn queued_blocks(&mut self) -> (Vec<Live>, Option<CaptureFailure>) {
        let mut lives = Vec::new();
        let mut failure = None;
        // Taken faster than a live input sends, so this ends.
        while let Some(cap) = self.capture.as_ref().filter(|c| !c.is_file) {
            let event = match cap.rx.try_recv() {
                Err(TryRecvError::Empty) => break,
                event => event,
            };
            let (came, ended, fail) = self.input_event(event, false);
            lives.extend(came);
            if fail.is_some() {
                failure = fail;
            }
            if ended {
                break;
            }
        }
        (lives, failure)
    }

    /// Stop listening for now, keeping everything.
    pub(super) fn pause(&mut self) {
        if self.capture.is_none() {
            return;
        }
        // The sound the input already sent is the take's too (#873), and so
        // is the sound core's gap rule still holds.
        let (queued, failure) = self.queued_blocks();
        let held = self.finish_gaps();
        if let Err(CaptureFailure::Recording(e)) = queued
            .into_iter()
            .chain(held)
            .try_for_each(|l| self.take_live(l))
        {
            return self.recording_failed(e);
        }
        self.release_capture();
        if self.session.mic() {
            applog::info("microphone stopped and released");
            let now = self.live.as_ref().map_or(0.0, |l| l.duration_s());
            if let Ok(Err(e)) = self.session.pause(now) {
                return self
                    .recording_failed(format!("Couldn't close the kept sound at the pause: {e}"));
            }
        }
        // An input that failed just before the pause says so, as listening
        // would have.
        if let Some(CaptureFailure::Input(error)) = failure {
            self.input_fault = Some(error.clone());
            self.error(error);
        }
    }

    /// Stop: pause, and with a watch picked save the take into it (or the
    /// sound since into the take saved before). A Quick Listen take isn't
    /// kept until Keep in a Watch… or Save….
    pub(super) fn stop_listening(&mut self) {
        // Stop during a Power Reserve Test ends the test, which keeps its take.
        if self.reserve_running() {
            if let Some(t) = self.reserve.as_mut() {
                t.stop();
            }
            return self.end_reserve();
        }
        self.pause();
        // A Positions Test with no watch picked: asked where it goes.
        if self.positions_take
            && self.open_watch().is_none()
            && self.session.saved_to().is_none()
            && self.session.allows(Action::Save)
        {
            self.test_keep = Some(TestKeep::Positions);
            return;
        }
        // Never a folder dialog at a Stop: only where Save… needs none.
        // A Quick Listen take kept already takes the sound since too, and
        // one whose first save is still written is kept once it is (#1216).
        let kept = |a: &mut Self| {
            if a.kept_take() {
                a.save_again();
            }
        };
        if self.behind_save(kept) {
            return;
        }
        if self.kept_take() {
            self.save_again();
        }
    }

    /// Whether the take on the microphone is kept, so Stop saves it with
    /// no question and, while listening, its sound is added to its folder
    /// as it comes ([`TimegrapherApp::stream_tick`]): a Quick Listen take
    /// once saved, or a watch's own take.
    pub(super) fn kept_take(&self) -> bool {
        let into_watch = if self.quick_listen {
            self.session.saved_to().is_some()
        } else {
            self.open_watch().is_some()
        };
        let quiet = self.session.saved_to().is_some() || self.watch_takes_dir().is_some();
        into_watch && quiet && self.session.allows(Action::Save)
    }

    /// Whether the take being heard records into its folder as it goes: a
    /// kept take ([`TimegrapherApp::kept_take`]) while the microphone is
    /// listening, a Positions or Power Reserve Test's of a watch picked
    /// too (its results are written as it ends). A test with no watch
    /// picked is asked where it goes as it ends, so it has no folder until
    /// then; its sound is kept here meanwhile, offered back after a crash.
    pub(super) fn streams(&self) -> bool {
        self.session.mic()
            && self.session.state() == session::State::Listening
            && self.session.recording_fault().is_none()
            && self.kept_take()
    }

    /// A save the user asked for being written: one of a take recording
    /// into its folder as it goes doesn't count, as it is waited for.
    pub(super) fn saving_asked(&self) -> bool {
        self.saving
            .as_ref()
            .is_some_and(|s| s.streamed == Streamed::No)
    }

    /// Where a take recording into its folder is kept, as the Save icon
    /// says it: Unsorted Takes, the watch's name, or the folder's.
    pub(super) fn kept_place(&self) -> String {
        if let Some(e) = self.open_watch() {
            return e.watch.name.clone();
        }
        let parent = self.session.saved_to().and_then(Path::parent);
        let unsorted = match &self.library {
            Ok(Some(l)) => l.unsorted_dir().ok(),
            _ => None,
        };
        match parent {
            Some(p) if unsorted.as_deref() == Some(p) => "Unsorted Takes".to_string(),
            Some(p) => p.file_name().map_or_else(
                || p.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            ),
            None => String::new(),
        }
    }

    /// While a kept take is heard, add its sound since the last save to
    /// its folder every [`STREAM_EVERY_S`] of sound, quietly, so a crash
    /// or a power cut costs only seconds. A save that fails says so, and
    /// the next is tried a minute later.
    pub(super) fn stream_tick(&mut self) {
        if self.saving.is_some()
            || self.stream_wait.is_some_and(|t| t.elapsed() < STREAM_RETRY)
            || !self.streams()
            || self.session.unsaved_s() < STREAM_EVERY_S
        {
            return;
        }
        self.stream_wait = None;
        self.save_again_as(Streamed::Yes);
    }

    /// Carry on listening after a pause, in the same session.
    pub(super) fn resume_microphone(&mut self) {
        if self.refuse_faulted_recording() || self.analysis_save_busy() {
            return;
        }
        let rate = self.live.as_ref().map(|l| l.sample_rate());
        match capture::open_input(self.device.as_deref(), rate) {
            Ok(c) => {
                let shown = self.device_label(&c);
                if let Some(why) = self.resume_refusal(&c.label, &shown, c.sample_rate) {
                    return self.error(why);
                }
                let paused = match self.session.resume(&c.label) {
                    Ok(p) => p,
                    Err(e) => return self.error(e.to_string()),
                };
                if let Some(l) = self.live.as_mut() {
                    l.skip(paused);
                    self.session.resumed_at(l.duration_s());
                }
                // A settling begun at Resume is left out, too.
                self.mask_engine();
                self.opened_at = Instant::now();
                self.audio_at = None;
                self.gaps = None;
                self.strip_end = None;
                self.message = None;
                applog::info(&format!("microphone resumed at {} Hz", c.sample_rate));
                self.input_fault = None;
                self.capture = Some(c);
                self.apply_agc();
            }
            Err(e) => {
                let m = format!("Can't open the input. {}", capture::explain_error(&e));
                self.input_fault = Some(m.clone());
                self.error(m);
            }
        }
    }

    /// Why the paused session can't carry on with the input `label` (shown
    /// in the microphone menu as `shown`) opened at `sample_rate`: another
    /// sample rate, or another input, whose clock correction and clock
    /// measurement aren't this session's.
    pub(super) fn resume_refusal(
        &self,
        label: &str,
        shown: &str,
        sample_rate: u32,
    ) -> Option<String> {
        let rate = self.live.as_ref().map(|l| l.sample_rate());
        if Some(sample_rate) != rate {
            return Some(format!(
                "The microphone opened at {sample_rate} Hz this time, not the {} Hz this session \
                 was listening at, so it can't carry on. Press New Session to start again.",
                rate.unwrap_or_default()
            ));
        }
        let track = self.clock_track.as_ref()?;
        let first = &track.shown;
        (track.device != label).then(|| {
            format!(
                "This session was listening to {first}, and its sound is timed by that input's \
                 clock, so it can't carry on with {shown}. Choose {first} to carry on, or press \
                 New Session to start again with {shown}."
            )
        })
    }

    /// Clear everything for the next watch or position. Listening to the
    /// microphone carries on in the new session.
    pub(super) fn new_session(&mut self) {
        if self.analysis_save_busy() || self.saving_busy() {
            return;
        }
        let listening = self.running() && self.session.mic();
        self.message = None;
        self.stop();
        // A failed close leaves files behind: say so, and don't start
        // listening again over it.
        let close_error = self
            .session
            .recording_fault()
            .and_then(|_| self.message.clone());
        self.session.discard_recording();
        self.session.clear();
        self.live = None;
        self.restore_live_strip();
        self.put_masked(Vec::new());
        self.opened_masks = None;
        self.opened_positions = None;
        self.put_take_clock(None, None);
        self.put_take_breaks(None, None);
        self.masking = false;
        self.brush = None;
        self.clear_trends();
        self.sound_history.clear();
        self.no_sound = None;
        self.analysis_kept_to = None;
        self.guided = None;
        self.reserve = None;
        self.bg.reserve_live = None;
        self.bg.reserve_live_s = 0.0;
        self.positions_take = false;
        self.guided_saved = None;
        self.complication_saved = None;
        self.test_keep = None;
        self.guided_unused.clear();
        self.guided_measured.clear();
        self.mask_engine();
        self.source_label.clear();
        // The recording opened goes too: what follows is a new take.
        if self.input == Input::File {
            self.input = Input::Microphone;
            self.opened_path.clear();
            if let Some(s) = self.mic_setup.take() {
                self.set_watch_setup(s);
            }
        }
        if listening && close_error.is_none() {
            self.start_microphone();
        } else if !self.quick_listen {
            // The next take is of the watch picked.
            if let Some(w) = self.open_watch().map(|e| e.watch.clone()) {
                self.apply_watch(&w);
            }
        }
        if close_error.is_some() {
            self.message = close_error;
        }
    }

    /// Unsaved sound that a new session would throw away, seconds.
    pub(super) fn unsaved_s(&self) -> f64 {
        self.session.unsaved_s()
    }

    /// Memory-only saved-edit inputs. Continuing audio must not make a short
    /// tail look like a settings edit by extending a clipped position range.
    pub(super) fn save_metadata(&self) -> session::Metadata {
        let guided_ranges = self
            .guided
            .as_ref()
            .map_or_else(Vec::new, |t| t.stretches().to_vec());
        let guided_wind_h = (!guided_ranges.is_empty())
            .then(|| self.guided.as_ref().and_then(|t| t.wind_h()))
            .flatten();
        let mut about = self.guided_about();
        // Auto detection may evolve with sound. Track the selected setting,
        // while save_into still writes the effective BPH as before.
        about.bph = self.bph;
        session::Metadata {
            // session.json retains the raw name; manifest/results trim theirs.
            watch: self.watch.clone(),
            calibre: self.calibre.clone(),
            position: self.position.code().into(),
            bph: self.bph,
            lift_deg: self.lift_deg,
            lift_known: self.lift_known(),
            wheels: self
                .cal
                .custom_wheels(&self.calibre)
                .unwrap_or_default()
                .into_iter()
                .collect(),
            guided_ranges,
            guided_wind_h,
            full_wind: self.guided.as_ref().and_then(|t| t.kept(&about)),
            masked: self.masked.clone(),
        }
    }

    pub(super) fn metadata_unsaved(&self) -> bool {
        self.session.saved_to().is_some() && self.session.metadata_changed(&self.save_metadata())
    }

    /// Keep the existing sound threshold; metadata edits have no duration.
    pub(super) fn needs_save_confirmation(&self) -> bool {
        self.session.recording_fault().is_some()
            || !self.quick_unsaved() && (self.unsaved_s() > 5.0 || self.metadata_unsaved())
            || self.marks_unsaved()
    }

    /// Unsaved sound from Quick Listen goes without a question: listening
    /// again, opening a recording or quitting lets it go. Save keeps it. A
    /// take of a watch, or picking one, asks.
    pub(super) fn quick_unsaved(&self) -> bool {
        self.quick_listen && self.session.mic() && self.session.saved_to().is_none()
    }

    /// A recording opened from disk whose masked stretches or positions
    /// have changed since it was opened or last saved to.
    pub(super) fn marks_unsaved(&self) -> bool {
        self.masks_changed() || self.positions_changed()
    }

    /// A recording opened from disk whose masked stretches have changed
    /// since it was opened or last saved to.
    fn masks_changed(&self) -> bool {
        !self.session.mic()
            && self
                .opened_masks
                .as_ref()
                .is_some_and(|m| *m != self.masked)
    }

    /// A recording opened from disk whose positions have been painted
    /// since it was opened or last saved to.
    fn positions_changed(&self) -> bool {
        !self.session.mic()
            && self
                .opened_positions
                .as_ref()
                .is_some_and(|p| *p != self.positions_now())
    }

    /// The positions of the recording opened from disk as its session
    /// file lists them: one entry per stretch in one position, from the
    /// end of its settling to the next change, in seconds into the
    /// recording; one with no times while the watch stayed put.
    fn opened_entries(&self) -> Entries {
        entries_of(
            &self.session.position_spans(self.position),
            &self.session.settles(),
        )
    }

    /// The positions shown and their settling, and those recorded under
    /// any painting, as the session has them now.
    pub(super) fn positions_now(&self) -> Layers {
        (
            (self.session.stretches(), self.session.settles()),
            self.session.recorded(),
        )
    }

    /// The positions as recorded under the painting of the recording
    /// opened from disk, as [`Self::opened_entries`] lists those shown;
    /// empty while nothing painted shows.
    fn opened_recorded(&self) -> Vec<RecordedPosition> {
        let Some((stretches, settles)) = self.session.recorded() else {
            return Vec::new();
        };
        let spans: Vec<(f64, f64, Position)> = (0..stretches.len())
            .map(|i| {
                let (a, p) = stretches[i];
                let b = stretches.get(i + 1).map_or(f64::INFINITY, |s| s.0);
                (if i == 0 { f64::NEG_INFINITY } else { a }, b, p)
            })
            .collect();
        entries_of(&spans, &settles)
            .into_iter()
            .map(|(start_s, end_s, position)| RecordedPosition {
                position,
                start_s,
                end_s,
            })
            .collect()
    }

    /// Save the masked stretches of the recording opened from disk to its
    /// session file, so they come back when it is opened again. A take's
    /// session file keeps everything else it says, comments too, and only
    /// its `masked` changes on the entries for what was opened. A lone file,
    /// or a folder with no session file, gets a new one, as a take
    /// recorded here with the same settings would have, which makes its
    /// folder a take; refused where the folder holds other recordings.
    pub(super) fn save_marks(&mut self) {
        let path = PathBuf::from(&self.opened_path);
        let masked: Vec<[f64; 2]> = self.masked.iter().map(|&(a, b)| [a, b]).collect();
        let positions = self.positions_changed().then(|| self.opened_entries());
        let what = match (self.masks_changed(), positions.is_some()) {
            (true, true) => "the masked stretches and the positions",
            (false, true) => "the positions",
            _ => "the masked stretches",
        };
        let saved = match read_manifest(&path) {
            Ok(Some(_)) => {
                save_marks_to_take(&path, &masked, positions.as_ref(), &self.opened_recorded())
            }
            Ok(None) => self.new_take_file(&path, &masked),
            Err(e) => Err(e),
        };
        match saved {
            Ok(file) => {
                self.opened_masks = Some(self.masked.clone());
                self.opened_positions = Some(self.positions_now());
                self.info_msg(format!("Saved {what} to {}.", file.display()));
                // A take in a watch: its row in the Watches view reads what
                // was saved.
                if let Ok(Some(l)) = &mut self.library {
                    l.scan();
                }
            }
            Err(e) => self.error(format!("Couldn't save {what}: {e}")),
        }
    }

    /// A new session file for `path`, a lone file or a folder with none,
    /// listing it with `masked`: what saving a take recorded here writes.
    fn new_take_file(&self, path: &Path, masked: &[[f64; 2]]) -> Result<PathBuf, String> {
        // The whole folder, as the recorder lists a take's sound: a lone
        // file is the whole of its folder's take (see `lists`).
        let dir = take_dir(path)
            .ok_or_else(|| format!("{} has no folder", path.display()))?
            .to_path_buf();
        let file = ".".to_string();
        let target = dir.join(MANIFEST);
        if target.exists() {
            return Err(format!(
                "{} doesn't list {}, so it is left as it is.",
                target.display(),
                capture_label(path)
            ));
        }
        if !path.is_dir() {
            let others = recordings_in(&dir)?
                .into_iter()
                .filter(|f| f.file_name() != path.file_name())
                .count();
            if others > 0 {
                return Err(format!(
                    "saving them makes {} a take of {} alone, but it holds {others} other {}. \
                     Move the recording into a folder of its own first.",
                    dir.display(),
                    capture_label(path),
                    if others == 1 {
                        "recording"
                    } else {
                        "recordings"
                    }
                ));
            }
        }
        let mut m = self.manifest_header();
        m.recordings = self
            .opened_entries()
            .into_iter()
            .map(|(start_s, end_s, code)| RecordingEntry {
                file: file.clone(),
                position: Some(code),
                start_s,
                end_s,
                settle_s: Some(0.0),
                masked: masked.to_vec(),
                ..Default::default()
            })
            .collect();
        m.recorded = self.opened_recorded();
        let text = toml::to_string_pretty(&m).map_err(|e| e.to_string())?;
        write_replacing(&target, &text)?;
        Ok(target)
    }

    /// Save Again: the sound so far into the take saved before, with no
    /// question. The first time, Save… saves into the watch picked in the
    /// Watches view, else asks where.
    pub(super) fn save_again(&mut self) {
        self.save_again_as(Streamed::No);
    }

    /// [`TimegrapherApp::save_again`], or a save of a take recording into
    /// its folder as it goes, which says nothing when it is done.
    fn save_again_as(&mut self, streamed: Streamed) {
        if self.behind_save(move |a| a.save_again_as(streamed)) {
            return;
        }
        if self.refuse_faulted_recording() || self.saving_busy() {
            return;
        }
        match self.session.saved_to().and_then(|p| p.parent()) {
            Some(parent) => {
                let parent = parent.to_path_buf();
                self.save_into_as(&parent, streamed);
                // Into a take in a watch, its row then reads what was saved
                // (a take recording into its folder: at the end).
                if let Some(s) = &mut self.saving {
                    s.rescan = streamed == Streamed::No;
                }
            }
            None => match self.watch_takes_dir() {
                Some(takes) => self.save_into_watch(&takes, streamed),
                None => self.save_recording(),
            },
        }
    }

    /// Save the session as a new take in a watch's `takes`, named by when
    /// it started (in this computer's time) and its one position, if it
    /// used only one.
    fn save_into_watch(&mut self, takes: &Path, streamed: Streamed) {
        let positions: Vec<timegrapher_core::session::Position> = self
            .take_manifest()
            .recordings
            .iter()
            .filter_map(|r| {
                r.position
                    .as_deref()
                    .and_then(timegrapher_core::session::Position::parse)
            })
            .collect();
        let test = self.reserve_is_test();
        let Some(r) = self.session.recorder.as_mut() else {
            return;
        };
        let started = watches::local_minute(r.started_utc())
            .unwrap_or_else(|| r.started_utc().replace(':', "."));
        // A Power Reserve Test's take is named as the test names it at
        // its end, so that save lands in the same folder.
        let name = if test {
            power_reserve_pane::reserve_take_name(&started)
        } else {
            library::take_name(&started, &positions)
        };
        let dir = library::unused_dir(takes, &name);
        if let Some(name) = dir.file_name() {
            r.name_first_copy(name.to_owned());
        }
        self.save_into_as(takes, streamed);
        if let Some(s) = &mut self.saving {
            s.rescan = true;
        }
    }

    /// Save a copy of the session's sound so far, with its clock log and
    /// settings, in a folder the user picks. Listening carries on.
    pub(super) fn save_recording(&mut self) {
        if self.behind_save(Self::save_recording) {
            return;
        }
        if self.refuse_faulted_recording() || self.saving_busy() {
            return;
        }
        let mut d = rfd::FileDialog::new()
            .set_title("Choose Where to Save (the Take Gets Its Own Folder Inside)");
        let start = self.save_start();
        if start.is_dir() {
            d = d.set_directory(&start);
        }
        self.pick(Picker::one(d, Pick::Folder, |a, parent| {
            a.save_into(&parent)
        }));
    }

    /// Save the session's sound so far into a take in `parent`. The copy
    /// is written on a thread of its own ([`Saving`]): listening carries
    /// on, and the window with it.
    pub(super) fn save_into(&mut self, parent: &Path) {
        self.save_into_as(parent, Streamed::No);
    }

    fn save_into_as(&mut self, parent: &Path, streamed: Streamed) {
        if self.refuse_faulted_recording() || self.saving_busy() {
            return;
        }
        let metadata = self.session.metadata_snapshot(self.save_metadata());
        // Prepare all required metadata before starting the saved-copy update.
        let prepared = (|| -> Result<_, String> {
            let result = self
                .reading()
                .map(serde_json::to_value)
                .transpose()
                .map_err(|e| format!("Couldn't prepare the saved reading: {e}"))?;
            let manifest = toml::to_string_pretty(&self.take_manifest())
                .map_err(|e| format!("Couldn't prepare {MANIFEST}: {e}"))?;
            // A test's results are written as it ends, never part way:
            // a take recording into its folder holds none until then.
            let kept = match streamed {
                Streamed::No => self.results_files()?,
                Streamed::Yes => Vec::new(),
            };
            Ok((result, manifest, kept))
        })();
        let (result, manifest, kept) = match prepared {
            Ok(p) => p,
            Err(e) => return self.error(e),
        };
        // The watch's name as it is now, for session.json and the folder.
        self.note_settings();
        let Some(r) = self.session.recorder.as_mut() else {
            return;
        };
        let secs = r.duration_s();
        let mut files = vec![(MANIFEST, manifest.as_bytes())];
        files.extend(kept.iter().map(|(n, b)| (*n, b.as_slice())));
        // With no new full-wind result (including a 24-hour half), preserve
        // the earlier result file for comparison, as Save Again always has.
        let begun = match streamed {
            Streamed::No => r.begin_save(parent, &self.calibre, result.as_ref(), &files),
            Streamed::Yes => r.begin_stream_save(parent, &self.calibre, result.as_ref(), &files),
        };
        let pending = match begun {
            Ok(p) => p,
            Err(e) => return self.error(format!("Saving the recording failed: {e}")),
        };
        // A saved take's calibre is among the recent ones.
        let c = self.calibre.clone();
        self.cal.remember(&c);
        let dest = pending.dest().to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("saving".into())
            .spawn(move || pending.write());
        match spawned {
            Ok(thread) => {
                self.saving = Some(Saving {
                    thread,
                    dest,
                    parent: parent.to_path_buf(),
                    secs,
                    metadata,
                    rescan: false,
                    streamed,
                })
            }
            Err(e) => self.error(format!("Saving the recording failed: {e}")),
        }
    }

    /// The take's results files, written beside its session.toml by both
    /// Save… and Save Analysis Only…: the positions test's and the power
    /// reserve test's, when there are.
    pub(super) fn results_files(&self) -> Result<Vec<(&'static str, Vec<u8>)>, String> {
        let about = self.guided_about();
        let mut files = Vec::new();
        if let Some(k) = self.guided.as_ref().and_then(|t| t.kept(&about)) {
            let text = serde_json::to_string_pretty(&k)
                .map_err(|e| format!("Couldn't prepare {}: {e}", guided::RESULTS_FILE))?;
            files.push((guided::RESULTS_FILE, (text + "\n").into_bytes()));
        }
        // A long take's run read as one is never written into the take.
        if let Some(r) = self
            .reserve
            .as_ref()
            .and_then(|t| t.results.as_ref())
            .filter(|r| r.source.is_test())
        {
            files.push((power_reserve::FILE, r.to_json().into_bytes()));
        }
        Ok(files)
    }

    /// While a save is being written, say so and refuse what would start
    /// another or throw the sound it is copying away.
    pub(super) fn saving_busy(&mut self) -> bool {
        // Asks waiting on a save go first: this one never jumps them.
        if !self.after_save.is_empty() {
            self.info_msg("Saving into its take: wait until it is written.".into());
            return true;
        }
        // A take recording into its folder: its few seconds are written
        // first, then what was asked for goes ahead.
        if self
            .saving
            .as_ref()
            .is_some_and(|s| s.streamed == Streamed::Yes)
        {
            self.wait_for_save();
        }
        if self.saving.is_none() {
            return false;
        }
        self.info_msg(
            "Saving: the sound so far is being copied into its take, so wait until it is done."
                .into(),
        );
        true
    }

    /// Whether `then` waits for a save into a take that is still being
    /// written (the first can copy a whole hour), the user's own or one of
    /// a take recording into its folder: it goes ahead once that is done,
    /// in the order asked, and the window carries on meanwhile rather than
    /// waiting on the copy, or refusing (#1216).
    pub(super) fn behind_save(&mut self, then: impl FnOnce(&mut Self) + Send + 'static) -> bool {
        if self.after_save.is_empty() && self.saving.is_none() {
            return false;
        }
        self.after_save.push(Box::new(then));
        self.info_msg("Saving into its take: this goes ahead once it is written.".into());
        true
    }

    /// Whether the window closes now, once a Stop's save into the take
    /// and the asks waiting on it are all done.
    pub(super) fn closes_after_save(&self) -> bool {
        self.close_after_save && self.saving.is_none() && self.after_save.is_empty()
    }

    /// How soon the window draws again with no input: while listening or
    /// reading a take, and while work on its own thread is under way, so
    /// what it gives shows (and a take kept as its analysis only lets its
    /// sound go, #1221) without waiting for the pointer to move.
    pub(super) fn repaint_every(&self) -> Option<std::time::Duration> {
        let working = self.sound_job.is_some()
            || self.scope_job.is_some()
            || self.steady_job.is_some()
            || self.saving.is_some()
            || self.take_keep.is_some()
            || self.analysis_save.is_some();
        if self.running() || self.batch.is_some() {
            Some(std::time::Duration::from_millis(40))
        } else {
            working
                .then_some(std::time::Duration::from_millis(100))
                .or_else(|| self.reserve_retry_in())
        }
    }

    /// When the window draws next: as [`Self::repaint_every`] says, and
    /// once more at once on the frame the work ended, so what it left (a
    /// message, the take's files) shows with no input (Walter on #1232).
    pub(super) fn draw_next(&self, was_working: bool) -> Option<std::time::Duration> {
        self.repaint_every()
            .or(was_working.then_some(std::time::Duration::ZERO))
    }

    /// The save being written, once it is done.
    pub(super) fn poll_saving(&mut self) {
        if self.saving.as_ref().is_some_and(|s| s.thread.is_finished()) {
            self.wait_for_save();
        }
    }

    /// Wait for the save being written, if there is one, and say how it
    /// went.
    pub(super) fn wait_for_save(&mut self) {
        let Some(s) = self.saving.take() else {
            return;
        };
        match s.thread.join() {
            Ok(Ok(copy)) => {
                if let Some(r) = self.session.recorder.as_mut() {
                    r.note_saved(&copy);
                    // Where the take is recording into, for the next start
                    // after a crash: the sound left here is there already.
                    if s.streamed == Streamed::Yes {
                        let note = r.dir().join(timegrapher_core::recorder::STREAMED_TO);
                        let _ = std::fs::write(note, copy.dir.to_string_lossy().as_bytes());
                    }
                }
                if s.streamed == Streamed::No {
                    self.info_msg(format!(
                        "Saved {} of sound in {}",
                        length_text(s.secs),
                        copy.dir.display()
                    ));
                }
                self.save_dir = s.parent.display().to_string();
                // A take saved with listening stopped is whole: the command
                // the user set runs on it. A take saving into its folder as
                // it goes isn't yet.
                if s.streamed == Streamed::No && !self.running() {
                    self.run_after_save_command(&copy.dir);
                }
                self.session.mark_saved(s.secs, copy.dir, s.metadata);
                if s.rescan {
                    if let Ok(Some(l)) = &mut self.library {
                        l.scan();
                    }
                }
            }
            Ok(Err(e)) => self.save_failed(s.streamed, &s.dest, e.to_string()),
            Err(_) => self.save_failed(s.streamed, &s.dest, "it stopped part way".into()),
        }
    }

    /// Settings > General > Run After Saving a Take, on the take saved in
    /// `dir`. Only a command that can't be started at all (no shell) says
    /// so here; one that fails, a mistyped one included, shows only in the
    /// app's log.
    fn run_after_save_command(&mut self, dir: &Path) {
        if let Err(e) = crate::after_save::run(&self.after_save_command, dir) {
            self.error(format!(
                "The command set to run after saving couldn't be started: {e}"
            ));
        }
    }

    /// What was asked while a take's save into its folder was written,
    /// in the order asked, once no save is. One that starts a save of its
    /// own (a Stop's) holds the rest until that is written too.
    pub(super) fn run_after_save(&mut self) {
        while self.saving.is_none() && !self.after_save.is_empty() {
            let mut rest = std::mem::take(&mut self.after_save);
            rest.remove(0)(self);
            rest.append(&mut self.after_save);
            self.after_save = rest;
        }
    }

    /// A save that failed says so. A take recording into its folder tries
    /// again a minute later, its sound kept here meanwhile; a quit waiting
    /// on the save stays open. Once the take is stopped, its Stop's save
    /// waiting its turn holds all its sound, so it alone answers for the
    /// take and the quit: the pause can take the hour from under a copy
    /// still under way (#1230).
    fn save_failed(&mut self, streamed: Streamed, dest: &Path, e: String) {
        if streamed == Streamed::Yes && !self.running() && !self.after_save.is_empty() {
            return applog::info(&format!(
                "a save into {} failed after the Stop, whose save follows: {e}",
                dest.display()
            ));
        }
        self.close_after_save = false;
        if streamed == Streamed::Yes {
            // The promise holds only while the take records: once its
            // recording has failed, nothing is tried again (#1380).
            if self.session.recording_fault().is_some() {
                return self.error(format!(
                    "Couldn't add the sound to {}: {e}. The sound not yet added is kept in \
                     the recovery folder.",
                    dest.display()
                ));
            }
            self.stream_wait = Some(Instant::now());
            return self.error(format!(
                "Couldn't add the sound to {}: {e}. It is kept here, and tried again in a \
                 minute.",
                dest.display()
            ));
        }
        self.error(format!("Saving the recording failed: {e}"))
    }

    /// [`Self::save_failed`] of a save into the take's folder, for tests.
    #[cfg(test)]
    pub(super) fn save_failed_for_test(&mut self, dest: &Path, e: String) {
        self.save_failed(Streamed::Yes, dest, e);
    }

    /// While the session is being kept as its analysis alone, say so and
    /// refuse what would throw its sound away or add to it.
    pub(super) fn analysis_save_busy(&mut self) -> bool {
        if self.analysis_save.is_none() {
            return false;
        }
        self.info_msg(
            "Keeping the analysis of this session: it is reading the session's sound and \
             writing its analysis, so wait until it is done, or press Cancel beside its \
             progress."
                .into(),
        );
        true
    }

    /// Whether the session can be kept as its analysis alone now: a
    /// microphone session with sound, not listening.
    pub(super) fn can_save_analysis(&self) -> bool {
        self.session.allows(Action::Save)
            && !self.running()
            && self.analysis_save.is_none()
            && self.session.recorder.is_some()
    }

    /// Keep the session as its analysis alone, in a new folder inside one
    /// the user picks.
    pub(super) fn save_analysis_only(&mut self) {
        if self.refuse_faulted_recording() || !self.can_save_analysis() {
            return;
        }
        let mut d = rfd::FileDialog::new()
            .set_title("Choose Where to Keep the Analysis (the Take Gets Its Own Folder Inside)");
        let start = self.save_start();
        if start.is_dir() {
            d = d.set_directory(&start);
        }
        self.pick(Picker::one(d, Pick::Folder, |a, parent| {
            if a.can_save_analysis() {
                a.start_analysis_save(&parent);
            }
        }));
    }

    /// Read the session's sound again for its analysis, in the background,
    /// then write it as a take with no sound inside `parent`.
    pub(super) fn start_analysis_save(&mut self, parent: &Path) {
        let mut manifest = self.take_manifest();
        manifest.audio = Some(false);
        let prepared = (|| -> Result<_, String> {
            let result = self
                .reading()
                .map(serde_json::to_value)
                .transpose()
                .map_err(|e| format!("Couldn't prepare the saved reading: {e}"))?;
            let text = toml::to_string_pretty(&manifest)
                .map_err(|e| format!("Couldn't prepare {MANIFEST}: {e}"))?;
            let kept = self.results_files()?;
            Ok((result, text, kept))
        })();
        let (result, text, kept) = match prepared {
            Ok(p) => p,
            Err(e) => return self.error(e),
        };
        self.note_settings();
        let Some(r) = self.session.recorder.as_mut() else {
            return;
        };
        let total_s = r.duration_s();
        let mut required = vec![(MANIFEST, text.as_bytes())];
        required.extend(kept.iter().map(|(n, b)| (*n, b.as_slice())));
        let (name, companions) =
            match r.analysis_copy_files(&self.calibre, result.as_ref(), &required) {
                Ok(c) => c,
                Err(e) => return self.error(format!("Keeping the analysis failed: {e}")),
            };
        let files = r.segment_paths();
        let dest = parent.join(name);
        // Read as the take on show is read: its beat rate and lift, and
        // the escape teeth core gives its session file and calibre (#1324).
        let cfg = self.entry_settings(&manifest).stream;
        let shapes = timegrapher_core::analysis_take::shape_stretches(&manifest, total_s);
        let software = report::software();
        let (tx, rx) = std::sync::mpsc::channel();
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (p2, c2) = (progress.clone(), cancel.clone());
        std::thread::spawn(move || {
            let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let stored = timegrapher_core::analysis_take::measure(
                    &paths,
                    &cfg,
                    &shapes,
                    software,
                    |s| {
                        p2.store(s.to_bits(), std::sync::atomic::Ordering::Relaxed);
                        if c2.load(std::sync::atomic::Ordering::Relaxed) {
                            std::panic::resume_unwind(Box::new(Cancelled));
                        }
                    },
                )
                .map_err(|e| e.to_string())?;
                let cancelled = || c2.load(std::sync::atomic::Ordering::Relaxed);
                if cancelled() {
                    std::panic::resume_unwind(Box::new(Cancelled));
                }
                let dir = timegrapher_core::analysis_take::save(&dest, &companions, &stored)
                    .map_err(|e| e.to_string())?;
                // Cancelled while it was being written: the folder is this
                // job's own (the save refuses one already there), so it goes.
                if cancelled() {
                    let _ = std::fs::remove_dir_all(&dir);
                    std::panic::resume_unwind(Box::new(Cancelled));
                }
                Ok(dir)
            }));
            match r {
                Ok(r) => {
                    // No one left to tell (cancelled after its last check):
                    // the folder it wrote goes too.
                    if let Err(std::sync::mpsc::SendError(Ok(dir))) = tx.send(r) {
                        let _ = std::fs::remove_dir_all(dir);
                    }
                }
                Err(e) if e.is::<Cancelled>() => {}
                Err(e) => std::panic::resume_unwind(e),
            }
        });
        self.save_dir = parent.display().to_string();
        self.analysis_save = Some(AnalysisSave {
            rx,
            progress,
            cancel,
            total_s,
        });
    }

    /// The analysis being kept, once it is written.
    fn poll_analysis_save(&mut self) {
        let Some(job) = &self.analysis_save else {
            return;
        };
        let done = match job.rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("it stopped part way".into()),
        };
        let Some(job) = self.analysis_save.take() else {
            return;
        };
        match done {
            Ok(dir) => {
                let size = folder_size(&dir);
                self.info_msg(format!(
                    "Kept the analysis of {} of sound in {}, without the sound ({}).",
                    length_text(job.total_s),
                    dir.display(),
                    size_text(size)
                ));
                self.analysis_kept_to = Some((dir, job.total_s));
            }
            Err(e) => self.error(format!("Keeping the analysis failed: {e}")),
        }
    }

    /// Open Report a Problem with the sound the app has: the live
    /// session's, or the recording open.
    pub(super) fn open_report(&mut self) {
        let shown = report::Shown {
            bph: self.bph,
            lift_deg: self.lift_deg,
            app: serde_json::json!({
                "reading": self.reading(),
                "showing": self.source_label,
                "watch": self.watch.trim(),
                "calibre": self.calibre,
                "position": self.position.code(),
                "bph_set": self.bph,
                "lift_deg": self.lift_deg,
                "averaging_s": self.average_s,
                "at_s": self.latest_s(),
                // Stretches masked out on the strip, in the session's
                // seconds, so an analysis of the excerpt can leave them out too.
                "masked": self.masked,
            }),
        };
        let source = self.report_source();
        self.report.show(report::Kind::Bug, source, shown);
    }

    fn report_source(&mut self) -> Option<report::Source> {
        if self.session.mic() {
            let r = self.session.recorder.as_mut()?;
            let _ = r.flush();
            let total = r.duration_s();
            let about = std::fs::read_to_string(r.dir().join("session.json"))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default();
            return (total >= 1.0).then(|| report::Source {
                files: r.segment_paths(),
                total_s: total,
                name: "this session".into(),
                live: true,
                end_s: total,
                about,
                cfg: None,
                kept: None,
            });
        }
        let path = PathBuf::from(self.file_path.trim());
        if self.file_path.trim().is_empty() || !path.exists() {
            return None;
        }
        let manifest = read_manifest(&path).ok().flatten();
        if analysis_take::is_analysis_only(&path) {
            return Some(report::Source {
                files: Vec::new(),
                total_s: 0.0,
                name: capture_label(&path),
                live: false,
                end_s: 0.0,
                about: serde_json::Value::Null,
                cfg: None,
                kept: Some(path),
            });
        }
        // How the take was read, so its beats come from what was found.
        let cfg = take_entry(&path, manifest.as_ref())
            .ok()
            .map(|o| self.entry_settings(&o.1).stream);
        let files = recordings_in(&path).ok()?;
        let mut total = 0.0;
        for f in &files {
            let i = timegrapher_core::audio::info(f).ok()?;
            total += i.frames? as f64 / i.sample_rate as f64;
        }
        // The five minutes up to the strip's newest edge, when the strip
        // has been moved back; else the start.
        let end = match self.strip_end {
            Some(_) if self.live.is_some() => self.strip_end_s(),
            _ => report::SPAN_S,
        }
        .clamp(report::SPAN_S.min(total), total);
        let about = std::fs::read_to_string(
            if path.is_dir() {
                path.clone()
            } else {
                path.parent()?.to_path_buf()
            }
            .join(MANIFEST),
        )
        .map(serde_json::Value::String)
        .unwrap_or_default();
        Some(report::Source {
            files,
            total_s: total,
            name: capture_label(&path),
            live: false,
            end_s: end,
            about,
            cfg,
            kept: None,
        })
    }

    /// The session file for the take being kept: the watch, its calibre
    /// (with the wheels of a custom one), the lift angle and beat rate, and
    /// one entry per stretch in one position, in seconds into the sound
    /// saved, which `timegrapher session` reads as well. Each entry starts
    /// once its position has settled, and says `settle_s = 0` so that
    /// nothing is skipped twice: the command line reads the seconds the
    /// app's table read.
    pub(super) fn take_manifest(&self) -> Manifest {
        let mut m = self.manifest_header();
        // The stretches masked out, in the saved sound's own time. Every
        // entry lists them all, so each comes back when the take is
        // opened, even one in a settling gap; `timegrapher session`
        // counts only the part inside an entry's stretch.
        let masked: Vec<[f64; 2]> = self
            .masked
            .iter()
            .filter_map(|&(a, b)| Some([self.session.audio_of(a)?, self.session.audio_of(b)?]))
            .filter(|m| m[1] > m[0])
            .collect();
        // The positions in the saved sound's own time (the sound leaves
        // out the pauses the session's clock counts).
        m.recordings = self
            .session
            .take_ranges()
            .into_iter()
            .map(|(start_s, end_s, code)| RecordingEntry {
                file: ".".into(),
                position: Some(code),
                clock: Some("clock.csv".into()),
                start_s,
                end_s,
                settle_s: Some(0.0),
                masked: masked.clone(),
                ..Default::default()
            })
            .collect();
        // A Positions Test's own stretches (after its settling, for its
        // measuring time), so `timegrapher session` reads the seconds its
        // table showed.
        let wind_h = self.guided.as_ref().and_then(|t| t.wind_h());
        // A Complication Test's stretches say which is Off and which On,
        // so its results come back when the take is opened (#1620).
        let step = |i: usize| {
            self.guided
                .as_ref()
                .filter(|t| t.kind == guided::Kind::Complication)
                .and_then(|_| timegrapher_core::complication::ORDER.get(i))
                .map(|s| s.saved_name().to_string())
        };
        let test: Vec<RecordingEntry> = self
            .guided
            .as_ref()
            .map_or(&[][..], |t| t.stretches())
            .iter()
            .enumerate()
            .filter_map(|(i, &(pos, from, to))| {
                let (start_s, end_s) = (self.session.audio_of(from)?, self.session.audio_of(to)?);
                Some(RecordingEntry {
                    complication: step(i),
                    file: ".".into(),
                    position: Some(pos.code().to_string()),
                    wind_h,
                    clock: Some("clock.csv".into()),
                    start_s: Some(start_s),
                    end_s: Some(end_s),
                    settle_s: Some(0.0),
                    masked: masked.clone(),
                    ..Default::default()
                })
            })
            .collect();
        if test.is_empty() {
            // What was recorded under any painting, in the sound's time.
            m.recorded = self
                .session
                .recorded_ranges()
                .unwrap_or_default()
                .into_iter()
                .map(|(start_s, end_s, position)| RecordedPosition {
                    position,
                    start_s,
                    end_s,
                })
                .collect();
        } else {
            m.recordings = test;
        }
        m
    }

    /// What a take's session file says about the watch: its name, calibre
    /// (with the wheels of a custom one), beat rate and lift angle, and
    /// whether that lift was chosen or assumed.
    pub(super) fn manifest_header(&self) -> Manifest {
        let mut m = Manifest {
            watch: (!self.watch.trim().is_empty()).then(|| self.watch.trim().to_string()),
            calibre: (!self.calibre.is_empty()).then(|| self.calibre.clone()),
            bph: self.bph.or(self.live.as_ref().and_then(|l| l.bph())),
            lift: Some(self.lift_deg),
            lift_known: Some(self.lift_known()),
            ..Default::default()
        };
        if let Some(w) = self.cal.custom_wheels(&self.calibre) {
            m.wheels = w.into_iter().collect();
        }
        m
    }

    /// A take opened: a Positions Test's known by `positions-test.json`
    /// beside it, and a Power Reserve Test's results back from
    /// `power-reserve.json`, so its pane shows the run.
    pub(super) fn load_reserve(&mut self, path: &Path) {
        let dir = take_dir(path).unwrap_or(path);
        self.positions_take = timegrapher_core::library::is_positions_test(dir);
        self.load_saved_test(dir);
        self.load_saved_complication(dir);
        // A test's take opens on its results.
        if self.positions_take {
            self.reveal_pane(Pane::PositionsTest);
        }
        let Ok(text) = std::fs::read_to_string(dir.join(power_reserve::FILE)) else {
            return;
        };
        let r = match power_reserve::Results::from_json(&text) {
            Ok(r) => r,
            Err(e) => {
                return self.error(format!(
                    "This take's Power Reserve results can't be read: {e}"
                ))
            }
        };
        let position = Position::from_name(&r.position).unwrap_or(self.position);
        let mut t = reserve::Test::new(position, r.sound_kept, r.started_utc.clone(), 0.0);
        t.ending = Some(r.ending);
        t.saved = Some(dir.to_path_buf());
        let hours = r.hours.len();
        t.results = Some(r);
        self.reserve = Some(t);
        self.reveal_pane(Pane::PowerReserve);
        // The run's charts and table, not the line before a test.
        grow_row(
            &mut self.panes.tiles,
            Pane::PowerReserve,
            reserve::run_h(hours),
        );
    }

    /// Take what a session file says about a recording: the watch, its
    /// calibre (a custom one is added to the custom calibres from its wheels),
    /// the lift angle and beat rate. Returns the positions it lists through
    /// the recording, oldest first, and the settling before each: a gap
    /// between one position's end and the next one's start.
    pub(super) fn apply_manifest(
        &mut self,
        m: &Manifest,
        entries: &[RecordingEntry],
        duration_s: Option<f64>,
    ) -> (Stretches, Vec<(f64, f64)>) {
        self.apply_manifest_placed(m, entries, duration_s, None)
    }

    /// [`Self::apply_manifest`] for a take opened from a folder of several
    /// sound files, `layout` (core's [`folder_layout`], one window per
    /// entry) placing each on the strip: an entry for one of them gives its
    /// positions, settling and masks in seconds into its own file, so each
    /// is moved to where that file starts (#1154).
    #[expect(
        clippy::disallowed_methods,
        reason = "the strip shows the session file's positions as the take starts to open, before its TakeContext arrives, laid by core's folder layout; and the positions recorded under painting are the app's own record"
    )]
    pub(super) fn apply_manifest_placed(
        &mut self,
        m: &Manifest,
        entries: &[RecordingEntry],
        duration_s: Option<f64>,
        layout: Option<&[EntryWindow]>,
    ) -> (Stretches, Vec<(f64, f64)>) {
        if let Some(w) = &m.watch {
            self.watch = w.clone();
        }
        if let Some(c) = &m.calibre {
            let wheels: Vec<(String, f64)> =
                m.wheels.iter().map(|(n, &p)| (n.clone(), p)).collect();
            self.cal.adopt(c, &wheels);
            self.set_calibre(c.clone());
        }
        if let Some(l) = m.lift {
            self.set_lift(l);
            // Whether that lift was supplied, by core's one rule for
            // session files (#685): older ones did not record it.
            let cal = m
                .calibre
                .as_deref()
                .and_then(|n| timegrapher_core::take::calibre_named(n, &self.cal.user));
            let known = timegrapher_core::take::file_lift_known(m, cal.as_ref());
            self.lift_context = (l, known.unwrap_or(false));
        }
        if m.bph.is_some() {
            self.put_bph(m.bph);
        }
        // What the take masked out; its seconds are the strip's, each
        // entry's moved to where its file starts.
        let offset = |i: usize| layout.map_or(0.0, |w| w[i].offset_s);
        let masked =
            timegrapher_core::take_context::masks_in_take(entries, (0..entries.len()).map(offset));
        self.put_masked(timegrapher_core::session::masked_spans(&masked));
        self.opened_masks = Some(self.masked.clone());
        // Each entry read once the watch has settled, from core's window
        // for it, as `session` reads it: the app's own takes say
        // `settle_s = 0`, a recorded one its settling, and a take shorter
        // than twice its settling settles to halfway. Its length is known
        // here for a take of one sound file, and for each file of a folder
        // take laid out by core; otherwise each entry runs to its end.
        let listed = match layout {
            Some(windows) => entries
                .iter()
                .zip(windows)
                .filter_map(|(e, w)| {
                    timegrapher_core::take::listed_position(
                        e,
                        m,
                        w.window.as_ref().ok().copied(),
                        w.offset_s,
                    )
                })
                .collect(),
            None => {
                let one_file = entries.windows(2).all(|w| w[0].file == w[1].file);
                let length = duration_s.filter(|_| one_file).unwrap_or(f64::INFINITY);
                timegrapher_core::take::listed_positions(m, entries, length)
            }
        };
        let (out, settles) = timegrapher_core::take::positions_of(listed.into_iter().filter_map(
            |(start, from, to, p, carried)| Some((start, from, to, Position::of_core(p)?, carried)),
        ));
        // What was recorded under any painting, loaded with the positions.
        self.pending_recorded = (!m.recorded.is_empty()).then(|| {
            timegrapher_core::take::positions_of(m.recorded.iter().filter_map(|r| {
                let p = Position::from_name(&r.position)?;
                let start = r.start_s.unwrap_or(0.0);
                Some((start, start, r.end_s, p, false))
            }))
        });
        if let Some(&(_, p)) = out.first() {
            self.position = p;
        }
        (out, settles)
    }

    /// A take in a watch whose session file names no calibre (one kept
    /// from Quick Listen by an older app) opens with the watch's details.
    pub(super) fn watch_fills_in(&mut self, take: &Path, m: &Manifest) {
        if m.calibre.as_deref().is_some_and(|c| !c.is_empty()) {
            return;
        }
        let w = match &self.library {
            Ok(Some(l)) => l.watch_of(take).map(|e| e.watch.clone()),
            _ => None,
        };
        if let Some(w) = w.filter(|w| w.calibre.as_deref().is_some_and(|c| !c.is_empty())) {
            self.take_watch_details(&w);
        }
    }

    /// Open a recording, analysed whole. With `ask`, unsaved sound from a
    /// microphone session is not thrown away without asking first.
    pub(super) fn open_recording(&mut self, path: &Path, ask: bool) {
        self.open_recording_as(path, ask, false);
    }

    /// Open a recording as `open_recording` does; with `no_watch`, the
    /// watch picked is let go once it opens, not before, so a take still
    /// heard keeps its watch if the open is called off (Just Open It).
    pub(super) fn open_recording_as(&mut self, path: &Path, ask: bool, no_watch: bool) {
        let p = path.to_path_buf();
        if self.behind_save(move |a| a.open_recording_as(&p, ask, no_watch)) {
            return;
        }
        if self.analysis_save_busy() || self.saving_busy() {
            return;
        }
        if ask && (self.session.mic() || self.marks_unsaved()) && self.needs_save_confirmation() {
            self.confirm = Some(Confirm::Open {
                path: path.to_path_buf(),
                no_watch,
            });
            return;
        }
        if no_watch {
            // Not one of a watch's takes, so no watch stays picked: the
            // bar, the cards and Watches say so alike, and the next
            // Listen is a Quick Listen (Hugh).
            self.watch_open = None;
            self.unsorted_open = false;
            self.mic_setup = Some(WatchSetup::default());
        }
        self.start_batch(path);
    }

    /// Play a recording through as if live, at its own speed. The app
    /// offers no replay; tests use it to feed the live path from a file
    /// without opening an input.
    #[cfg(test)]
    pub(super) fn start_replay(&mut self, path: &Path) {
        self.start_replay_at(path, 1.0);
    }

    /// Play a recording through as if live, `speed` times its own. A
    /// folder plays its files one after the other in the order it is
    /// analysed in, with the watch from its session file when it has one.
    #[cfg(test)]
    pub(super) fn start_replay_at(&mut self, path: &Path, speed: f32) {
        // Its settings read nothing again, as an open's don't (#1241).
        let outer = std::mem::replace(&mut self.opening, true);
        self.replay_at(path, speed);
        self.opening = outer;
    }

    #[cfg(test)]
    fn replay_at(&mut self, path: &Path, speed: f32) {
        // Refused before anything on show is stopped or its masks cleared.
        if let Some(why) = no_analysis_to_read(path) {
            return self.open_failed(why);
        }
        if let Ok(inner) = take_folder(path) {
            if analysis_take::is_analysis_only(&inner) {
                return self.open_failed(format!(
                    "{} has had its sound removed, so it can't be played again. Open shows \
                     its readings.",
                    capture_label(&inner)
                ));
            }
            if let Some(why) = no_analysis_to_read(&inner) {
                return self.open_failed(why);
            }
        }
        self.stop();
        self.pending_recorded = None;
        self.put_take_clock(None, None);
        self.put_take_breaks(None, None);
        self.put_masked(Vec::new());
        self.masking = false;
        self.brush = None;
        let inner = match take_folder(path) {
            Ok(p) => p,
            Err(e) => return self.open_failed(e),
        };
        let path = inner.as_path();
        // Reject a relevant invalid manifest before opening audio or a worker.
        let manifest = match read_manifest(path) {
            Ok(manifest) => manifest,
            Err(e) => return self.open_failed(e),
        };
        let files = match recordings_in(path) {
            Ok(f) => f,
            Err(e) => return self.open_failed(e),
        };
        let started = if path.is_dir() {
            let label = match files.len() {
                1 => format!("{} (1 file)", capture_label(path)),
                n => format!("{} ({n} files)", capture_label(path)),
            };
            capture::replay_files(&files, label)
        } else {
            capture::replay_file(path)
        };
        match started {
            Ok(c) => {
                let (stretches, settles) = match manifest {
                    Some((m, entries)) => {
                        self.keep_mic_setup();
                        self.set_watch_setup(WatchSetup::default());
                        self.open_note = None;
                        let layout = folder_layout(path, &m, &entries);
                        let windows = layout.as_ref().map(|l| l.1.as_slice());
                        self.apply_manifest_placed(&m, &entries, None, windows)
                    }
                    None => {
                        self.fresh_setup(path, &files);
                        Default::default()
                    }
                };
                if let Some(n) = order_warning(&files) {
                    self.open_note = Some(match self.open_note.take() {
                        Some(o) => format!("{o} {n}"),
                        None => n,
                    });
                }
                if self.file_path.trim() != path.display().to_string() {
                    self.file_path = path.display().to_string();
                }
                self.reset_session(c.sample_rate, c.label.clone());
                self.session.replay();
                self.session.load(&stretches, &settles);
                if let Some((s, t)) = self.pending_recorded.take() {
                    self.session.load_recorded(&s, &t, self.position);
                }
                if let (Some(live), Some(breaks)) =
                    (self.live.as_mut(), self.session.replay_breaks())
                {
                    live.set_breaks(breaks);
                }
                self.mask_engine();
                self.opened_path = path.display().to_string();
                c.set_speed(speed);
                self.capture = Some(c);
                if let Some(n) = self.open_note.take() {
                    self.info_msg(n);
                }
            }
            Err(e) => self.open_failed(format!("Can't read {}: {e}", path.display())),
        }
    }

    /// A recording couldn't be opened: say why, and put the path box back
    /// to the one still shown.
    fn open_failed(&mut self, e: String) {
        self.error(e);
        if !self.opened_path.is_empty() {
            self.file_path = self.opened_path.clone();
        }
    }

    /// The microphone's watch set aside, the first time a recording is
    /// opened over it.
    fn keep_mic_setup(&mut self) {
        if self.mic_setup.is_none() {
            self.mic_setup = Some(self.watch_setup());
        }
    }

    /// A recording with no session file starts from the defaults, never
    /// the last take's watch, with its position from the file's name when
    /// it carries one (reverso_DU_…), and a note saying so.
    fn fresh_setup(&mut self, path: &Path, files: &[PathBuf]) {
        self.opened_masks = Some(Vec::new());
        self.keep_mic_setup();
        self.set_watch_setup(WatchSetup::default());
        let from_name =
            position_in_name(path).or_else(|| files.first().and_then(|f| position_in_name(f)));
        if let Some(p) = from_name {
            self.position = p;
        }
        self.open_note = Some(match from_name {
            Some(p) => format!(
                "No watch settings in the file: Calibre Not Set, Lift {DEFAULT_LIFT_DEG}°, \
                 {} from the file name.",
                p.label()
            ),
            None => format!(
                "No watch settings in the file: Calibre Not Set, Lift {DEFAULT_LIFT_DEG}°, \
                 Dial Up."
            ),
        });
    }

    pub(super) fn watch_setup(&self) -> WatchSetup {
        WatchSetup {
            watch: self.watch.clone(),
            calibre: self.calibre.clone(),
            lift_deg: self.lift_deg,
            lift_context: self.lift_context,
            bph: self.bph,
            position: self.position,
        }
    }

    /// Put a whole watch setup in place. Its beat rate is put with it, so
    /// a calibre in it reads nothing on show again (#1241).
    pub(super) fn set_watch_setup(&mut self, s: WatchSetup) {
        self.watch = s.watch;
        let outer = std::mem::replace(&mut self.opening, true);
        self.set_calibre(s.calibre);
        self.opening = outer;
        self.put_lift(s.lift_deg);
        self.lift_context = s.lift_context;
        self.put_bph(s.bph);
        self.position = s.position;
    }

    pub(super) fn start_batch(&mut self, path: &Path) {
        self.analyse_whole(path, None);
    }

    /// Analyse `path` whole. Without a session file it starts from
    /// `carried`, else from the defaults. The settings it puts in place
    /// for it read nothing again, since it is about to be read with them.
    pub(super) fn analyse_whole(&mut self, path: &Path, carried: Option<WatchSetup>) {
        let outer = std::mem::replace(&mut self.opening, true);
        self.open_whole(path, carried);
        self.opening = outer;
    }

    fn open_whole(&mut self, path: &Path, carried: Option<WatchSetup>) {
        // Refused before anything on show is stopped.
        if let Some(why) = no_analysis_to_read(path).or_else(|| {
            take_folder(path)
                .ok()
                .and_then(|inner| no_analysis_to_read(&inner))
        }) {
            return self.open_failed(why);
        }
        let carried_masks = self.clear_for_open(carried.is_some());
        let inner = match self.take_inside(path) {
            Ok(p) => p,
            Err(e) => return self.open_failed(e),
        };
        let path = inner.as_path();
        // Reject a relevant invalid manifest before opening audio or a worker.
        let manifest = match read_manifest(path) {
            Ok(manifest) => manifest,
            Err(e) => return self.open_failed(e),
        };
        if self.file_path.trim() != path.display().to_string() {
            self.file_path = path.display().to_string();
        }
        if analysis_take::is_analysis_only(path) {
            return self.open_analysis_only(path, manifest);
        }
        if let Some(why) = no_analysis_to_read(path) {
            return self.open_failed(why);
        }
        let (files, info) = match recordings_info(path) {
            Ok(f) => f,
            Err(e) => return self.open_failed(e),
        };
        let order_note = order_warning(&files);
        // What the take's session file says: the watch, calibre, lift
        // angle, beat rate and positions, before the analysis uses them.
        // Nothing carries over from the take before.
        self.open_setup(manifest.is_some(), carried, path, &files);
        // What core opens: the whole recording, as the command line reads
        // it whole.
        let opening = match take_entry(path, manifest.as_ref()) {
            Ok(o) => o,
            Err(e) => return self.open_failed(e),
        };
        let (stretches, settles) = match manifest {
            Some((m, entries)) => {
                let length = info.frames.map(|f| f as f64 / f64::from(info.sample_rate));
                let layout = folder_layout(path, &m, &entries);
                let length = length.filter(|_| files.len() == 1);
                let windows = layout.as_ref().map(|l| l.1.as_slice());
                let out = self.apply_manifest_placed(&m, &entries, length, windows);
                self.watch_fills_in(path, &m);
                out
            }
            None => Default::default(),
        };
        // Read again for a new beat rate: the settings as they were, not
        // as the take saved them.
        if let Some(s) = self.reread_with.take() {
            self.set_watch_setup(s);
        }
        if let Some(m) = carried_masks {
            self.put_masked(m);
        }
        if let Some(n) = &order_note {
            self.add_open_note(n);
        }
        let label = take_label(path, files.len());
        let settings = self.entry_settings(&opening.1);
        let (tx, rx) = std::sync::mpsc::channel();
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let name = |p: &PathBuf| capture_label(p);
        let (count, first, last) = (files.len(), name(&files[0]), name(&files[files.len() - 1]));
        let inputs = self.inputs_for(&stretches, &settles);
        open_in_background(
            opening,
            settings,
            tx,
            progress.clone(),
            cancel.clone(),
            None,
            inputs,
        );
        self.opened_path = path.display().to_string();
        self.reset_session(info.sample_rate, label.clone());
        self.load_reserve(path);
        self.session.analyse();
        self.live = None;
        // Nothing of the take before's clock carries over, whichever
        // way this one is kept; a take that fails to open leaves it.
        self.put_take_clock(None, None);
        self.put_take_breaks(None, None);
        self.batch = Some(Batch {
            rx,
            progress,
            cancel,
            duration_s: info.frames.map(|f| f as f64 / info.sample_rate as f64),
            label,
            started: Instant::now(),
            files: count,
            first,
            last,
            stretches,
            settles,
            order_note,
        });
    }

    /// Stop what is on show and clear what the take before marked, before
    /// opening another. Masks carried with the settings (`carried`) come
    /// back once it opens; they are returned.
    fn clear_for_open(&mut self, carried: bool) -> Option<Vec<(f64, f64)>> {
        self.stop();
        if self.reread_with.is_none() {
            self.carried_positions = None;
        }
        self.pending_recorded = None;
        // Masks carried with the settings stay; anything else opened
        // starts with what its session file masks.
        let carried_masks = carried.then(|| self.masked.clone());
        self.put_masked(Vec::new());
        if carried_masks.is_none() {
            self.masking = false;
            self.brush = None;
        }
        carried_masks
    }

    /// The take to open at `path`: the take's own folder when `path` is a
    /// folder a take was saved into, saying so.
    fn take_inside(&mut self, path: &Path) -> Result<PathBuf, String> {
        // A folder a take was saved into holds the take's own folder.
        let inner = take_folder(path)?;
        if inner != path {
            self.info_msg(format!(
                "Opened the take saved inside {}: {}",
                capture_label(path),
                capture_label(&inner)
            ));
        }
        Ok(inner)
    }

    /// The watch setup an opened take starts from: what its session file
    /// says (applied after this), else `carried`, else fresh defaults.
    fn open_setup(
        &mut self,
        has_manifest: bool,
        carried: Option<WatchSetup>,
        path: &Path,
        files: &[PathBuf],
    ) {
        if !has_manifest {
            match carried {
                Some(s) => {
                    self.keep_mic_setup();
                    self.set_watch_setup(s);
                    self.open_note = None;
                }
                None => self.fresh_setup(path, files),
            }
        } else {
            self.keep_mic_setup();
            self.set_watch_setup(WatchSetup::default());
            self.open_note = None;
        }
    }

    /// Add `n` to the note shown about the take just opened.
    fn add_open_note(&mut self, n: &str) {
        self.open_note = Some(match self.open_note.take() {
            Some(o) => format!("{o} {n}"),
            None => n.to_string(),
        });
    }

    /// Open a take kept as its analysis alone: its beats, amplitude
    /// windows and profiles read from `analysis/` in the background, then
    /// shown as an analysed recording is.
    fn open_analysis_only(
        &mut self,
        path: &Path,
        manifest: Option<(Manifest, Vec<RecordingEntry>)>,
    ) {
        let name = capture_label(path);
        if recordings_in(path).is_ok() {
            return self.open_failed(format!(
                "{name} holds both sound and an {} folder: a take is kept either with its \
                 sound or as its analysis alone. Move one of them out to open the other.",
                analysis_take::DIR
            ));
        }
        let Some((m, entries)) = manifest.filter(|(m, _)| m.audio == Some(false)) else {
            return self.open_failed(format!(
                "{name} has an {} folder, but its {MANIFEST} doesn't say audio = false, so \
                 it isn't known to be a take kept as its analysis alone.",
                analysis_take::DIR
            ));
        };
        let header_file = path.join(analysis_take::DIR).join(analysis_take::HEADER);
        let header = std::fs::read_to_string(&header_file)
            .ok()
            .and_then(|t| serde_json::from_str::<analysis_take::Header>(&t).ok())
            .filter(|h| h.format == analysis_take::FORMAT);
        let Some(header) = header else {
            let why = match analysis_take::read(path) {
                Err(e) => e.to_string(),
                Ok(_) => format!("{} can't be read", header_file.display()),
            };
            return self.open_failed(format!("Can't open {name}: {why}"));
        };
        self.keep_mic_setup();
        self.set_watch_setup(WatchSetup::default());
        self.open_note = None;
        let (stretches, settles) = self.apply_manifest(&m, &entries, Some(header.duration_s));
        self.watch_fills_in(path, &m);
        let label = format!("{name} (analysis only)");
        // Its beats, its own clock log with its USB lines' figure as
        // measured when it was kept, and the breaks its session.json
        // lists, read by core as `timegrapher session` reads such a take.
        let opening = match take_entry(path, Some(&(m, entries))) {
            Ok(o) => o,
            Err(e) => return self.open_failed(e),
        };
        let settings = self.entry_settings(&opening.1);
        let (tx, rx) = std::sync::mpsc::channel();
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let duration_s = header.duration_s;
        let inputs = self.inputs_for(&stretches, &settles);
        open_in_background(
            opening,
            settings,
            tx,
            progress.clone(),
            cancel.clone(),
            Some(duration_s),
            inputs,
        );
        self.opened_path = path.display().to_string();
        self.reset_session(header.sample_rate, label.clone());
        self.load_reserve(path);
        self.no_sound = Some(Vec::new());
        self.session.analyse();
        self.live = None;
        // Nothing of the take before's clock carries over, whichever
        // way this one is kept; a take that fails to open leaves it.
        self.put_take_clock(None, None);
        self.put_take_breaks(None, None);
        self.batch = Some(Batch {
            rx,
            progress,
            cancel,
            duration_s: Some(duration_s),
            label,
            started: Instant::now(),
            files: 0,
            first: String::new(),
            last: String::new(),
            stretches,
            settles,
            order_note: None,
        });
    }

    /// Stop listening, and any analysis under way. A
    /// microphone session pauses, so it can carry on or be saved.
    pub(super) fn stop(&mut self) {
        if self.capture.as_ref().is_some_and(|c| !c.is_file) {
            self.pause();
        } else if matches!(
            self.session.state(),
            session::State::Replaying | session::State::Analysing
        ) {
            let now = self.live.as_ref().map_or(0.0, |l| l.duration_s());
            let _ = self.session.stop(now);
        }
        self.release_capture();
        self.batch = None;
    }

    /// Cancel the opening under way: nothing is opened, and the window no
    /// longer says Opening (#1220).
    pub(super) fn cancel_open(&mut self) {
        // Nothing of the take is left: the watch's setup from before the
        // open comes back, and there is no take to save (#1238).
        self.input = Input::File;
        self.new_session();
    }

    pub(super) fn error(&mut self, m: String) {
        applog::error(&m);
        self.message = Some((plain_error(&m), true));
    }

    /// A note or setting the recorder kept in memory but couldn't write
    /// to session.json; `finish` writes it at the latest (#1639).
    pub(super) fn session_write_failed(&mut self, e: &dyn std::fmt::Display) {
        self.error(format!(
            "The take's session file couldn't be written ({e}). The change is kept, \
             and goes into the file with the next change or when the take stops."
        ));
    }

    pub(super) fn info_msg(&mut self, m: String) {
        applog::info(&m);
        self.message = Some((m, false));
    }

    pub(super) fn refuse_faulted_recording(&mut self) -> bool {
        if let Some(fault) = self.session.recording_fault() {
            self.error(fault.description());
            true
        } else {
            false
        }
    }

    /// The recording failed (a full disk, say): listening stops, its sound
    /// is left for recovery, and the take can't be saved again. A Power
    /// Reserve Test running ends there, its results kept with that ending;
    /// a save into the take's folder that failed before isn't tried again,
    /// and the take's row in Watches reads what its folder holds (#1380).
    pub(super) fn recording_failed(&mut self, mut reason: String) {
        // Release the input before attempting any remaining close/flush work.
        self.release_capture();
        let now = self.live.as_ref().map_or(0.0, |l| l.duration_s());
        if self.session.state() == session::State::Listening {
            if let Ok(Err(e)) = self.session.pause(now) {
                reason.push_str(&format!("; closing the kept sound also failed: {e}"));
            }
        }
        self.session.recording_failed(now, reason);
        self.stream_wait = None;
        self.reserve_failed();
        if let Ok(Some(l)) = &mut self.library {
            l.scan();
        }
        if let Some(fault) = self.session.recording_fault() {
            self.error(fault.description());
        }
    }

    /// Take in whatever audio has arrived.
    pub(super) fn poll(&mut self) {
        self.poll_for(FRAME_BUDGET);
        self.poll_wav_keep();
        // Asks waiting on a save go ahead on the window's frame, never on
        // the listening thread, which polls too.
        self.run_after_save();
    }

    /// One block of a capture, or a break core's gap rule found before the
    /// next: saved and read.
    fn take_live(&mut self, l: Live) -> Result<(), CaptureFailure> {
        let taken = self.take_block(l);
        self.follow_rebuild();
        taken
    }

    fn take_block(&mut self, l: Live) -> Result<(), CaptureFailure> {
        let (Some(cap), Some(live)) = (self.capture.as_ref(), self.live.as_mut()) else {
            return Ok(());
        };
        // A cut ends a backlog culled after a break: a break with no time
        // lost, said with the break before it.
        let gap = l.lost_s();
        let said = match l {
            Live::Break(_) => Some(break_text(gap, false)),
            Live::Dropped(_) => Some(break_text(gap, true)),
            Live::Glitch => Some(timegrapher_core::breaks::GLITCH_TEXT.to_string()),
            Live::Block(_) | Live::Cut => None,
        };
        let b = match l {
            Live::Block(b) => b,
            Live::Break(_) | Live::Dropped(_) | Live::Cut | Live::Glitch => {
                // The computer slept or the input stalled: carry on, as
                // after a pause, with the time between out.
                if let Ok(Err(e)) = self.session.pause(live.duration_s()) {
                    return Err(CaptureFailure::Recording(e.to_string()));
                }
                if self.session.resume(&cap.label).is_ok() {
                    if let Some(r) = self.session.recorder.as_mut() {
                        r.sound_lost(gap);
                    }
                    live.skip(gap);
                    self.session.resumed_at(live.duration_s());
                    live.set_masked(&super::masks::engine_masks(
                        &self.masked,
                        &self.session,
                        &self.guided_unused,
                    ));
                }
                if let Some(t) = self.clock_track.as_mut() {
                    t.restart(live.sample_rate());
                }
                if let Some(m) = said {
                    applog::warn(&m);
                    self.message = Some((m, false));
                }
                return Ok(());
            }
        };
        if let Some(r) = self.session.recorder.as_mut() {
            // An error stops the queued sound being processed, and no
            // later input error replaces it.
            r.write(&b)
                .map_err(|e| CaptureFailure::Recording(e.to_string()))?;
        }
        if let Some(t) = self.clock_track.as_mut() {
            t.push(b.samples.len(), b.at);
        }
        let l = Level::of(&b.samples);
        if self.level_at.elapsed().as_secs_f64() > 0.5 || l.peak_dbfs > self.level.peak_dbfs {
            self.level = l;
            self.level_at = Instant::now();
        }
        if l.clipped_fraction > 0.0 {
            self.clipped_at = Some(Instant::now());
        }
        if live.push(&b.samples) {
            // A jump the beats' timing shows is found a second
            // or two after it: the points read since reached
            // across it, so they go.
            let jumps = live.jumps();
            if jumps != self.jumps_seen.as_slice() {
                let recent = live.duration_s() - 10.0;
                let new = jumps
                    .iter()
                    .copied()
                    .filter(|j| *j > recent && !self.jumps_seen.contains(j))
                    .reduce(f64::min);
                if let Some(j) = new {
                    self.trend.retain(|p| p.t <= j);
                    self.sound_history.retain(|h| h.0 <= j);
                }
                self.jumps_seen = jumps.to_vec();
            }
            // While the watch settles after a move nothing is
            // read; then the readings start afresh.
            match self.session.settling_now(live.duration_s()) {
                Some(true) => return Ok(()),
                Some(false) => {
                    live.new_stretch();
                    return Ok(());
                }
                None => {}
            }
            let r = live.reading(self.average_s);
            if !self.session.replay_reading_allowed(r.time_s) {
                return Ok(());
            }
            self.trend.push(TrendPoint::of(r.time_s, &r));
            let p = live.tick_profiles();
            let due = self
                .sound_history
                .last()
                .is_none_or(|h| r.time_s - h.0 >= SOUND_EVERY_S);
            if due && p.iter().any(Option::is_some) {
                let kept = scope_panes::Kept {
                    span_s: profile_span_s(self.average_s),
                    sides: p.clone(),
                };
                self.sound_history.push((r.time_s, kept));
                scope_panes::thin_sound(&mut self.sound_history);
            }
        }
        Ok(())
    }

    /// Save and read the sound that has come in, for up to `budget`, so a
    /// backlog never holds up the window for long. Returns the blocks of
    /// sound taken.
    pub(super) fn poll_for(&mut self, budget: Duration) -> usize {
        let started = Instant::now();
        // After a while with nothing run, the input gets the usual grace
        // before it counts as quiet.
        if self.polled_at.elapsed() > Duration::from_secs(2) && self.audio_at.is_some() {
            self.audio_at = Some(started);
        }
        self.polled_at = started;
        // While listening the charts are read here; a take on show's in
        // the background, from the frame ([`Self::refresh_trend_shown`]).
        if self.running() {
            self.refresh_trend();
        }
        let (taken, ended, failure) = self.take_blocks(started, budget);
        self.capture_outcome(ended, failure);

        self.note_lock();

        self.poll_analysis_save();
        self.poll_saving();
        self.stream_tick();
        self.poll_batch();

        self.hold_anchor();
        taken
    }

    /// One event from the input, as listening takes it: what core's gap
    /// rule lets through, whether the input ended, and why it failed, if it
    /// did. A live input's blocks go through the gap rule, which lets each
    /// through once no break before it is in doubt (#689).
    fn input_event(
        &mut self,
        event: Result<Event, TryRecvError>,
        is_file: bool,
    ) -> (Vec<Live>, bool, Option<CaptureFailure>) {
        let stopped = |e: &str| {
            Some(CaptureFailure::Input(format!(
                "Input stopped. {}",
                capture::explain_error(e)
            )))
        };
        match event {
            Ok(Event::Audio(b)) if is_file => (vec![Live::Block(b)], false, None),
            Ok(Event::Audio(b)) => {
                let rate = self.live.as_ref().map_or(1, |l| l.sample_rate());
                let lives = self.gaps.get_or_insert_with(|| GapWatch::new(rate)).push(b);
                (lives, false, None)
            }
            // Only a live input drops sound (#483): a break, never spliced.
            Ok(Event::Lost(frames)) => {
                let rate = self.live.as_ref().map_or(1, |l| l.sample_rate());
                let lives = self
                    .gaps
                    .get_or_insert_with(|| GapWatch::new(rate))
                    .lost(frames);
                (lives, false, None)
            }
            // A glitch the input carries on after (#1046): a break, never
            // a stop.
            Ok(Event::Glitch) => {
                let rate = self.live.as_ref().map_or(1, |l| l.sample_rate());
                let lives = self
                    .gaps
                    .get_or_insert_with(|| GapWatch::new(rate))
                    .glitch();
                (lives, false, None)
            }
            Ok(Event::Error(e)) if is_file => (Vec::new(), false, stopped(&e)),
            Ok(Event::Error(e)) => (self.finish_gaps(), true, stopped(&e)),
            Ok(Event::End) | Err(_) => (self.finish_gaps(), true, None),
        }
    }

    /// What core's gap rule still holds, let through at the input's end.
    fn finish_gaps(&mut self) -> Vec<Live> {
        self.gaps.as_mut().map(GapWatch::finish).unwrap_or_default()
    }

    /// Take the blocks the input has sent, for up to `budget` once one is
    /// taken, through core's gap rule for a live input. Returns how many were
    /// taken, whether the input ended and why it failed, if it did.
    fn take_blocks(
        &mut self,
        started: Instant,
        budget: Duration,
    ) -> (usize, bool, Option<CaptureFailure>) {
        let mut taken = 0;
        let mut ended = false;
        let mut failure = None;
        while self.live.is_some() {
            let Some(cap) = self.capture.as_ref() else {
                break;
            };
            if taken > 0 && started.elapsed() > budget {
                break;
            }
            let is_file = cap.is_file;
            let event = match cap.rx.try_recv() {
                Err(TryRecvError::Empty) => break,
                event => event,
            };
            if matches!(event, Ok(Event::Audio(_))) {
                taken += 1;
                // Heard as it arrives, though core's gap rule may hold it a
                // while.
                self.audio_at = Some(Instant::now());
            }
            let (came, end, fail) = self.input_event(event, is_file);
            ended |= end;
            if fail.is_some() {
                failure = fail;
            }
            if let Err(e) = came.into_iter().try_for_each(|l| self.take_live(l)) {
                failure = Some(e);
                break;
            }
            if ended {
                break;
            }
        }
        // A microphone that opened but sends nothing.
        if let Some(cap) = &self.capture {
            let quiet = self.audio_at.unwrap_or(self.opened_at).elapsed();
            if !cap.is_file && quiet.as_secs_f64() > 3.0 && failure.is_none() {
                failure = Some(CaptureFailure::Input(
                    "No sound is arriving from this input.".to_string(),
                ));
                ended = true;
            }
        }
        (taken, ended, failure)
    }

    /// Stop when the input ended and say why it failed, if it did.
    fn capture_outcome(&mut self, ended: bool, failure: Option<CaptureFailure>) {
        if let Some(CaptureFailure::Recording(error)) = failure {
            self.recording_failed(error);
        } else {
            let was_file = self.capture.as_ref().is_some_and(|c| c.is_file);
            if ended {
                self.stop();
                if was_file {
                    self.info_msg("End of the recording.".into());
                }
            }
            if let Some(CaptureFailure::Input(error)) = failure {
                // Settings > Microphone says why listening stopped.
                self.input_fault = Some(error.clone());
                // A close error during stop is a recording fault in its own
                // right, even when the input failure triggered the stop.
                let message = match self.session.recording_fault() {
                    Some(fault) => format!("{}\n{error}", fault.description()),
                    None => error,
                };
                self.error(message);
            }
        }
    }

    /// Beat lock gained or lost, for the log.
    fn note_lock(&mut self) {
        let bph = self.live.as_ref().and_then(|l| l.bph());
        if bph != self.locked_bph && self.capture.is_some() {
            match bph {
                Some(b) => applog::info(&format!("locked on to the beat at {b} beats per hour")),
                None => applog::warn("lost the beat"),
            }
        }
        self.locked_bph = bph;
    }

    /// Show a recording opened whole once its analysis is done.
    fn poll_batch(&mut self) {
        let mut done = None;
        if let Some(b) = &self.batch {
            match b.rx.try_recv() {
                Ok(r) => done = Some(r),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => done = Some(Ok(None)),
            }
        }
        if let Some(r) = done {
            let (label, stretches, settles) = self
                .batch
                .take()
                .map(|b| (b.label.clone(), b.stretches.clone(), b.settles.clone()))
                .unwrap_or_default();
            match r {
                Ok(Some(o)) => {
                    let Opened {
                        mut live,
                        inputs,
                        trend,
                        clock,
                        clock_source,
                        breaks,
                        unlisted,
                        profiles,
                    } = o;
                    self.put_take_clock(clock, clock_source);
                    self.put_take_breaks(Some(breaks), unlisted);
                    // Read again at a new beat rate: the positions as they were.
                    let carried = self.carried_positions.take();
                    let (stretches, settles) = match &carried {
                        Some((now, _)) => {
                            self.pending_recorded = now.1.clone();
                            now.0.clone()
                        }
                        None => (stretches, settles),
                    };
                    // The readings readied with the engine, unless what
                    // they rest on changed while the take opened.
                    let now = self.open_inputs(&stretches, &settles);
                    let breaks = self.take_breaks.clone().unwrap_or_default();
                    let trend = if now == inputs {
                        Some(trend)
                    } else {
                        retune(&mut live, &inputs, &now, &breaks);
                        None
                    };
                    self.show_engine(live, trend, label, stretches, settles);
                    if let Some((_, opened)) = carried {
                        self.opened_positions = opened;
                    }
                    // A take with no sound shows the profiles it kept,
                    // averaged over the Average Over window (#979).
                    if let Some(k) = profiles {
                        self.no_sound = Some(k);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    // Nothing opened: end the opening, so the window
                    // isn't left saying Opening.
                    let _ = self.session.stop(0.0);
                    self.error(format!("Analysis failed: {e}"));
                }
            }
        }
    }

    /// Keep the strip anchored: centre on the first beats, then hold.
    fn hold_anchor(&mut self) {
        if self.anchor.is_none() {
            if let Some(live) = &self.live {
                if let Some(bph) = live.bph() {
                    let beats = live.beats();
                    if beats.len() >= 16 {
                        self.anchor = Anchor::centre_on_true(
                            &beats[..16],
                            3600.0 / bph as f64,
                            16,
                            self.take_breaks.as_deref().unwrap_or_default(),
                            Self::rate_clock(&self.take_clock),
                        );
                    }
                }
            }
        }
    }

    /// Show a take's beats, `log`, read on the window's thread.
    #[cfg(test)]
    pub(super) fn show_log(
        &mut self,
        log: BeatLog,
        label: String,
        stretches: Vec<(f64, Position)>,
        settles: Vec<(f64, f64)>,
    ) {
        self.land_log(log, label, stretches, settles);
        // Read here what the window reads in the background.
        self.refresh_trend();
        self.bg.reserve = None;
        self.reserve_from_long_take();
    }

    /// Show a take's beats, `log`, as an opened take lands whose readings
    /// weren't readied with its engine: what rests on them is left to
    /// the window's next frames.
    #[cfg(test)]
    pub(super) fn land_log(
        &mut self,
        log: BeatLog,
        label: String,
        stretches: Vec<(f64, Position)>,
        settles: Vec<(f64, f64)>,
    ) {
        let inputs = self.open_inputs(&stretches, &settles);
        let breaks = self.take_breaks.as_deref().unwrap_or_default();
        let live = ready_engine(log, &inputs, breaks, self.take_rate_clock());
        self.show_engine(live, None, label, stretches, settles);
    }

    /// Show an opened take's engine, `live` (readied by [`ready_engine`]),
    /// with its readings at the averaging time when they were readied
    /// with it (`trend`), else read here.
    fn show_engine(
        &mut self,
        live: LiveAnalyzer,
        trend: Option<Vec<TrendPoint>>,
        label: String,
        stretches: Vec<(f64, Position)>,
        settles: Vec<(f64, f64)>,
    ) {
        self.session.load(&[], &[]);
        self.session.analysed();
        // A take of one position settles too, unless its session file
        // says it was read from its start.
        if stretches.len() > 1 || !settles.is_empty() {
            self.session.load(&stretches, &settles);
        }
        if let Some(&(_, p)) = self.session.stretches().last() {
            self.position = p;
        }
        if let Some((s, t)) = self.pending_recorded.take() {
            self.session.load_recorded(&s, &t, self.position);
        }
        self.opened_positions = Some(self.positions_now());
        self.live = Some(live);
        // The opened take's labels, not those drawn while it opened
        // (Walter's HOLD on #1239).
        self.gutter_fresh = true;
        self.rev.new_take();
        // Readings not readied with the engine are read in the
        // background by the next frame (#1014).
        match trend {
            Some(points) => self.put_trend(points),
            None => (self.trend, self.strip_points) = (Vec::new(), None),
        }
        self.source_label = label;
        self.anchor = None;
        self.strip_end = None;
        self.fit_take();
        // A long take that isn't a test: its run, read as one in the
        // background.
        self.bg.reserve = Some(None);
        let done = if self.no_sound.is_some() {
            "Opened the analysis kept without its sound: the readings, charts and tests are as \
             they were measured, and the Scope needs the sound. Drag the strip to look \
             back."
        } else {
            "Analysed the whole recording. Drag the strip to look back."
        };
        // The note first: the status bar cuts a long message at its end.
        let m = match self.open_note.take() {
            Some(n) => format!("{n} {done}"),
            None => done.to_string(),
        };
        self.info_msg(m);
    }
}

/// The recordings in `path` in the order they are read, and their sound's
/// format with the frames of all of them together.
fn recordings_info(
    path: &Path,
) -> Result<(Vec<PathBuf>, timegrapher_core::audio::AudioInfo), String> {
    let files = recordings_in(path)?;
    let mut info = match timegrapher_core::audio::info(&files[0]) {
        Ok(i) => i,
        Err(e) => return Err(format!("Can't read {}: {e}", files[0].display())),
    };
    for f in &files[1..] {
        match timegrapher_core::audio::info(f) {
            Ok(i) => info.frames = info.frames.zip(i.frames).map(|(a, b)| a + b),
            Err(e) => return Err(format!("Can't read {}: {e}", f.display())),
        }
    }
    Ok((files, info))
}

/// The take kept sound in `dir` was recording into as it went, which
/// holds all of it but its last few seconds. A take moved or deleted
/// since leaves this the only copy, kept as sound never saved, and the
/// note goes so a move takes none with it.
pub(super) fn streamed_to(dir: &Path) -> Option<PathBuf> {
    let note = dir.join(timegrapher_core::recorder::STREAMED_TO);
    let take = PathBuf::from(std::fs::read_to_string(&note).ok()?.trim());
    if take.is_dir() {
        return Some(take);
    }
    let _ = std::fs::remove_file(&note);
    None
}

/// A take opened from the folder `path` of several sound files, with its
/// session file `m` and that file's `entries` for the folder, laid out by
/// core (`take_context::entry_layout`): each sound file's place on the
/// strip, and each entry's window there, one per entry (#1154). None for
/// a lone file, a folder of one, one core can't lay out, or one with an
/// entry for the whole folder, which reads over the take whole as before.
pub(super) fn folder_layout(
    path: &Path,
    m: &Manifest,
    entries: &[RecordingEntry],
) -> Option<(Vec<FilePart>, Vec<EntryWindow>)> {
    let whole = entries
        .iter()
        .any(|e| timegrapher_core::take::names_whole_folder(&e.file));
    if !path.is_dir() || whole {
        return None;
    }
    let listed = Manifest {
        recordings: entries.to_vec(),
        ..m.clone()
    };
    let (files, windows) =
        timegrapher_core::take_context::entry_layout(path, &listed, None).ok()?;
    (files.len() > 1 && windows.len() == entries.len()).then_some((files, windows))
}

/// The name an opened take shows with: a folder says how many files.
fn take_label(path: &Path, count: usize) -> String {
    if path.is_dir() {
        match count {
            1 => format!("{} (1 file)", capture_label(path)),
            n => format!("{} ({n} files)", capture_label(path)),
        }
    } else {
        capture_label(path)
    }
}

/// A take opened by core's `TakeContext::open`, for the window: its
/// engine readied from its beats on what the window had when the opening
/// started (`inputs`), with the readings over it, its clock and breaks,
/// and for a take kept as its analysis alone, the Tick Tock profiles it
/// kept. Readied off the window's thread, as reading a long take's
/// readings takes a while.
#[derive(Debug)]
pub(super) struct Opened {
    live: LiveAnalyzer,
    inputs: OpenInputs,
    trend: Vec<TrendPoint>,
    clock: Option<timegrapher_core::clock::ClockFit>,
    /// Where the clock came from, in core's words.
    clock_source: Option<String>,
    breaks: Vec<timegrapher_core::breaks::Break>,
    unlisted: Option<timegrapher_core::breaks::Unlisted>,
    profiles: Option<Vec<analysis_take::StretchProfiles>>,
}

/// What an opened take's engine and first readings rest on: taken from
/// the window when the opening starts, so they can be readied off its
/// thread, and again when the take arrives, to check they still hold.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct OpenInputs {
    bph: Option<u32>,
    lift_deg: f64,
    lift_known: bool,
    average_s: f64,
    masked: Vec<(f64, f64)>,
    /// Where each position began, and the settling before each one's
    /// readings.
    changes: Vec<f64>,
    settles: Vec<(f64, f64)>,
}

/// The engine for settings `i`.
fn config_for(i: &OpenInputs) -> LiveConfig {
    let mut c = LiveConfig::default();
    c.analysis.bph = i.bph;
    c.analysis.amplitude.lift_deg = i.lift_deg;
    c.analysis.lift_known = i.lift_known;
    c.profile_s = profile_span_s(i.average_s);
    // Every beat of as long as the strip can show, a power reserve
    // run's limit, rather than the engine's day: a take listened to
    // for longer keeps its first hours on the strip and the charts.
    c.max_beats = c.max_beats.max(reserve::RUN_BEATS_KEPT);
    c
}

/// An opened take's engine, from its beat log on `i`, with the take's
/// `breaks` and the `clock` that corrects its rates: no reading fits
/// across one of the take's breaks, a move or a settling's end (core's
/// cuts, as the CLI's trend reads them). The same off the window's thread
/// and on it.
fn ready_engine(
    log: BeatLog,
    i: &OpenInputs,
    breaks: &[timegrapher_core::breaks::Break],
    clock: Option<&timegrapher_core::clock::ClockFit>,
) -> LiveAnalyzer {
    let mut live = LiveAnalyzer::from_log(log, config_for(i));
    live.set_breaks(timegrapher_core::take::reading_cuts(
        breaks, &i.changes, &i.settles,
    ));
    live.set_masked(&engine_masked(i));
    // The new engine's first sight of the take's clock; after this only
    // put_take_clock gives it one.
    live.set_rate_clock(clock);
    live
}

/// `live`, readied on `was`, brought to what changed by `now` while its
/// take opened, as a change just after it opened would be: each setting
/// that changed, by the engine's own setter.
fn retune(
    live: &mut LiveAnalyzer,
    was: &OpenInputs,
    now: &OpenInputs,
    breaks: &[timegrapher_core::breaks::Break],
) {
    if now.bph != was.bph {
        live.set_bph(now.bph);
    }
    if now.lift_deg != was.lift_deg {
        live.set_lift(now.lift_deg);
    }
    if now.lift_known != was.lift_known {
        // The window says the clock every frame.
        live.set_known(now.lift_known, false);
    }
    if now.average_s != was.average_s {
        live.set_profile_span(profile_span_s(now.average_s));
    }
    if (&now.changes, &now.settles) != (&was.changes, &was.settles) {
        live.set_breaks(timegrapher_core::take::reading_cuts(
            breaks,
            &now.changes,
            &now.settles,
        ));
    }
    if (&now.masked, &now.settles) != (&was.masked, &was.settles) {
        live.set_masked(&engine_masked(now));
    }
}

/// An opened take's engine masks: its masks by hand with each settling
/// left out as a mask is, as `TimegrapherApp::engine_masked` gives them
/// once the take is open (#1233).
fn engine_masked(i: &OpenInputs) -> Vec<(f64, f64)> {
    timegrapher_core::session::with_holes(&i.masked, &i.settles)
}

/// Open the take `opening` ([`take_entry`]) with core's
/// `TakeContext::open`, its masks kept as data for the window to cut, on a
/// thread of its own, sending what it read to `tx`: `progress` holds
/// the seconds of sound read (for a take kept as its analysis alone, its
/// length `kept_s` once read) and setting `cancel` stops it, sending
/// nothing. Its engine and readings are readied there too, on `inputs`.
fn open_in_background(
    opening: (PathBuf, Manifest),
    settings: timegrapher_core::take::EntrySettings,
    tx: std::sync::mpsc::Sender<Result<Option<Opened>, String>>,
    progress: std::sync::Arc<std::sync::atomic::AtomicU64>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    kept_s: Option<f64>,
    inputs: OpenInputs,
) {
    use std::sync::atomic::Ordering::Relaxed;
    use timegrapher_core::take;
    std::thread::spawn(move || {
        let (base, m) = opening;
        // A cancel unwinds out of the analysis from its progress report
        // (resume_unwind runs no panic hook, so nothing is printed).
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            timegrapher_core::take_context::TakeContext::open(
                &base,
                &m,
                settings,
                timegrapher_core::take_context::Masks::Kept,
                take::usb,
                |first| take::correction_for(&m, first, None, None),
                |s, _| {
                    progress.store(s.to_bits(), Relaxed);
                    if cancel.load(Relaxed) {
                        std::panic::resume_unwind(Box::new(Cancelled));
                    }
                },
            )
        }));
        match r {
            Ok(r) => {
                if let Some(s) = kept_s {
                    progress.store(s.to_bits(), Relaxed);
                }
                let _ = tx.send(r.map(|(o, log)| {
                    let clock = TimegrapherApp::rate_clock(&o.clock);
                    let live = ready_engine(log, &inputs, &o.breaks, clock);
                    let trend = live.trend(inputs.average_s);
                    Some(Opened {
                        live,
                        inputs,
                        trend,
                        clock: o.clock,
                        clock_source: o.clock_source,
                        breaks: o.breaks,
                        unlisted: o.unlisted,
                        profiles: o.profiles,
                    })
                }));
            }
            Err(e) if e.is::<Cancelled>() => {}
            Err(e) => std::panic::resume_unwind(e),
        }
    });
}

/// What listening says about a break core's gap rule found: sound the
/// input dropped (#483), or a jump in its timestamps.
fn break_text(gap: f64, dropped: bool) -> String {
    if dropped {
        let lost = timegrapher_core::breaks::lost_text(gap);
        format!(
            "{lost} of sound was lost while the app was busy. It is left \
             out as a break."
        )
    } else {
        format!(
            "The sound's timestamps jumped by {}, as they do when the \
             computer sleeps. The jump is left out of the take as a \
             break, and listening carries on.",
            fields::duration(gap.round())
        )
    }
}
