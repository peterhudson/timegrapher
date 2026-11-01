//! Listening and saving: the microphone, the session and its unsaved state, saving takes and analysis, opening and replaying recordings, the whole-take analysis, and the poll that feeds the live view.

use super::*;

impl TimegrapherApp {
    pub(super) fn live_config(&self) -> LiveConfig {
        let mut c = LiveConfig::default();
        c.analysis.bph = self.bph;
        c.analysis.amplitude.lift_deg = self.lift_deg;
        c.analysis.lift_known = self.lift_known();
        c.profile_s = self.sound_span();
        // Every beat of as long as the strip can show, a power reserve
        // run's limit, rather than the engine's day: a take listened to
        // for longer keeps its first hours on the strip and the charts.
        c.max_beats = c.max_beats.max(reserve::RUN_BEATS_KEPT);
        c
    }

    /// Seconds of beats the tick and tock sound covers: the averaging time,
    /// so it describes the same beats as the readings, up to the engine's
    /// limit.
    fn sound_span(&self) -> f64 {
        self.average_s.clamp(0.5, MAX_PROFILE_S)
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
        let mut live = LiveAnalyzer::new(sample_rate, self.live_config());
        live.set_masked(&self.masked);
        self.live = Some(live);
        self.source_label = label;
        self.anchor = None;
        self.clear_trends();
        self.strip_end = None;
        self.level = Level::default();
        self.clipped_at = None;
        self.message = None;
        self.sound_history.clear();
        self.no_sound = false;
        self.analysis_kept_to = None;
        self.guided = None;
        self.reserve = None;
        self.guided_unused.clear();
        self.guided_measured.clear();
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
            Some(half) => self.start_guided(half),
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
        if matches!(self.live_strip, Some((_, fitted)) if fitted == self.strip) {
            self.strip_fit = self.live_fit;
        }
        self.strip = self.live_view();
        self.live_strip = None;
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

    /// Stop listening for now, keeping everything.
    pub(super) fn pause(&mut self) {
        if self.capture.is_none() {
            return;
        }
        // The sound core's gap rule still holds is the take's too.
        let held = self.gaps.as_mut().map(GapWatch::finish);
        if let Err(CaptureFailure::Recording(e)) = held
            .into_iter()
            .flatten()
            .try_for_each(|l| self.take_live(l))
        {
            return self.recording_failed(e);
        }
        self.release_capture();
        if self.session.mic() {
            applog::info("microphone stopped and released");
            let now = self.live.as_ref().map_or(0.0, |l| l.duration_s());
            if let Ok(Err(e)) = self.session.pause(now) {
                self.recording_failed(format!("Couldn't close the kept sound at the pause: {e}"));
            }
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
        // A Quick Listen take kept already takes the sound since too.
        let into_watch = if self.quick_listen {
            self.session.saved_to().is_some()
        } else {
            self.open_watch().is_some()
        };
        let quiet = self.session.saved_to().is_some() || self.watch_takes_dir().is_some();
        if into_watch && quiet && self.session.allows(Action::Save) {
            self.save_again();
        }
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
        self.put_masked(Vec::new());
        self.opened_masks = None;
        self.opened_positions = None;
        self.put_take_clock(None, None);
        self.put_take_breaks(None, None);
        self.masking = false;
        self.brush = None;
        self.clear_trends();
        self.sound_history.clear();
        self.no_sound = false;
        self.analysis_kept_to = None;
        self.guided = None;
        self.reserve = None;
        self.positions_take = false;
        self.test_keep = None;
        self.guided_unused.clear();
        self.guided_measured.clear();
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
        let dir = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent()
                .ok_or_else(|| format!("{} has no folder", path.display()))?
                .to_path_buf()
        };
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
        if self.refuse_faulted_recording() || self.saving_busy() {
            return;
        }
        match self.session.saved_to().and_then(|p| p.parent()) {
            Some(parent) => {
                let parent = parent.to_path_buf();
                self.save_into(&parent);
                // Into a take in a watch, its row then reads what was saved.
                if let Some(s) = &mut self.saving {
                    s.rescan = true;
                }
            }
            None => match self.watch_takes_dir() {
                Some(takes) => self.save_into_watch(&takes),
                None => self.save_recording(),
            },
        }
    }

    /// Save the session as a new take in a watch's `takes`, named by when
    /// it started (in this computer's time) and its one position, if it
    /// used only one.
    fn save_into_watch(&mut self, takes: &Path) {
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
        let Some(r) = self.session.recorder.as_mut() else {
            return;
        };
        let started = watches::local_minute(r.started_utc())
            .unwrap_or_else(|| r.started_utc().replace(':', "."));
        let dir = library::unused_dir(takes, &library::take_name(&started, &positions));
        if let Some(name) = dir.file_name() {
            r.name_first_copy(name.to_owned());
        }
        self.save_into(takes);
        if let Some(s) = &mut self.saving {
            s.rescan = true;
        }
    }

    /// Save a copy of the session's sound so far, with its clock log and
    /// settings, in a folder the user picks. Listening carries on.
    pub(super) fn save_recording(&mut self) {
        if self.refuse_faulted_recording() || self.saving_busy() {
            return;
        }
        let mut d = rfd::FileDialog::new()
            .set_title("Choose Where to Save (the Take Gets Its Own Folder Inside)");
        let start = self
            .session
            .saved_to()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from(&self.save_dir));
        if start.is_dir() {
            d = d.set_directory(&start);
        }
        let Some(parent) = d.pick_folder() else {
            return;
        };
        self.save_into(&parent);
    }

    /// Save the session's sound so far into a take in `parent`. The copy
    /// is written on a thread of its own ([`Saving`]): listening carries
    /// on, and the window with it.
    pub(super) fn save_into(&mut self, parent: &Path) {
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
            let kept = self.results_files()?;
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
        let pending = match r.begin_save(parent, &self.calibre, result.as_ref(), &files) {
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
                })
            }
            Err(e) => self.error(format!("Saving the recording failed: {e}")),
        }
    }

    /// The take's results files, written beside its session.toml by both
    /// Save… and Save Analysis Only…: the positions test's and the power
    /// reserve test's, when there are.
    fn results_files(&self) -> Result<Vec<(&'static str, Vec<u8>)>, String> {
        let about = self.guided_about();
        let mut files = Vec::new();
        if let Some(k) = self.guided.as_ref().and_then(|t| t.kept(&about)) {
            let text = serde_json::to_string_pretty(&k)
                .map_err(|e| format!("Couldn't prepare {}: {e}", guided::RESULTS_FILE))?;
            files.push((guided::RESULTS_FILE, (text + "\n").into_bytes()));
        }
        if let Some(r) = self.reserve.as_ref().and_then(|t| t.results.as_ref()) {
            files.push((power_reserve::FILE, r.to_json().into_bytes()));
        }
        Ok(files)
    }

    /// While a save is being written, say so and refuse what would start
    /// another or throw the sound it is copying away.
    pub(super) fn saving_busy(&mut self) -> bool {
        if self.saving.is_none() {
            return false;
        }
        self.info_msg(
            "Saving: the sound so far is being copied into its take, so wait until it is done."
                .into(),
        );
        true
    }

    /// The save being written, once it is done.
    fn poll_saving(&mut self) {
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
                }
                self.info_msg(format!(
                    "Saved {} of sound in {}",
                    length_text(s.secs),
                    copy.dir.display()
                ));
                self.save_dir = s.parent.display().to_string();
                self.session.mark_saved(s.secs, copy.dir, s.metadata);
                if s.rescan {
                    if let Ok(Some(l)) = &mut self.library {
                        l.scan();
                    }
                }
            }
            Ok(Err(e)) => self.error(format!("Saving the recording failed: {e}")),
            Err(_) => self.error("Saving the recording failed: it stopped part way".into()),
        }
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
        let start = self
            .session
            .saved_to()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from(&self.save_dir));
        if start.is_dir() {
            d = d.set_directory(&start);
        }
        let Some(parent) = d.pick_folder() else {
            return;
        };
        self.start_analysis_save(&parent);
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
        let mut cfg = StreamConfig::default();
        cfg.analysis.bph = self.bph;
        cfg.analysis.amplitude.lift_deg = self.lift_deg;
        cfg.analysis.lift_known = self.lift_known();
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
            });
        }
        let path = PathBuf::from(self.file_path.trim());
        if self.file_path.trim().is_empty() || !path.exists() {
            return None;
        }
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
        let test: Vec<RecordingEntry> = self
            .guided
            .as_ref()
            .map_or(&[][..], |t| t.stretches())
            .iter()
            .filter_map(|&(pos, from, to)| {
                let (start_s, end_s) = (self.session.audio_of(from)?, self.session.audio_of(to)?);
                Some(RecordingEntry {
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
        let dir = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        self.positions_take = timegrapher_core::library::is_positions_test(dir);
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
        // What the take masked out; its seconds are the strip's.
        let masked: Vec<[f64; 2]> = entries.iter().flat_map(|e| e.masked.clone()).collect();
        self.put_masked(timegrapher_core::session::masked_spans(&masked));
        self.opened_masks = Some(self.masked.clone());
        // Each entry read once the watch has settled, from core's window
        // for it, as `session` reads it: the app's own takes say
        // `settle_s = 0`, a recorded one its settling, and a take shorter
        // than twice its settling settles to halfway. Its length is known
        // here for a take of one sound file; for one of several, each
        // entry runs to its end.
        let one_file = entries.windows(2).all(|w| w[0].file == w[1].file);
        let length = duration_s.filter(|_| one_file).unwrap_or(f64::INFINITY);
        let (out, settles) = positions_of(entries.iter().filter_map(|e| {
            let p = Position::from_name(e.position.as_deref()?)?;
            let start = e.start_s.unwrap_or(0.0);
            let (from, to) = match timegrapher_core::take::reading_window(e, m, None, length) {
                Ok(w) => (w.from_s, w.to_s),
                // An entry session reads nothing of settles to its end.
                Err(_) => {
                    let end = e.end_s.unwrap_or(start);
                    (end, end)
                }
            };
            Some((start, from, to.is_finite().then_some(to), p))
        }));
        // What was recorded under any painting, loaded with the positions.
        self.pending_recorded = (!m.recorded.is_empty()).then(|| {
            positions_of(m.recorded.iter().filter_map(|r| {
                let p = Position::from_name(&r.position)?;
                let start = r.start_s.unwrap_or(0.0);
                Some((start, start, r.end_s, p))
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
        if self.analysis_save_busy() || self.saving_busy() {
            return;
        }
        if ask && (self.session.mic() || self.marks_unsaved()) && self.needs_save_confirmation() {
            self.confirm = Some(Confirm::Open {
                path: path.to_path_buf(),
            });
        } else {
            self.start_batch(path);
        }
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
                        self.apply_manifest(&m, &entries, None)
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

    pub(super) fn set_watch_setup(&mut self, s: WatchSetup) {
        self.watch = s.watch;
        self.set_calibre(s.calibre);
        self.put_lift(s.lift_deg);
        self.lift_context = s.lift_context;
        self.put_bph(s.bph);
        self.position = s.position;
    }

    pub(super) fn start_batch(&mut self, path: &Path) {
        self.analyse_whole(path, None);
    }

    /// Analyse `path` whole. Without a session file it starts from
    /// `carried`, else from the defaults.
    pub(super) fn analyse_whole(&mut self, path: &Path, carried: Option<WatchSetup>) {
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
        // What core opens: the whole recording, on the clock log the
        // take's first entry names, as the app has always read it.
        let opening = take_entry(path, manifest.as_ref());
        let (stretches, settles) = match manifest {
            Some((m, entries)) => {
                let length = info.frames.map(|f| f as f64 / f64::from(info.sample_rate));
                let out = self.apply_manifest(&m, &entries, length.filter(|_| files.len() == 1));
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
        open_in_background(
            opening,
            settings,
            tx,
            progress.clone(),
            cancel.clone(),
            None,
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
        let opening = take_entry(path, Some(&(m, entries)));
        let settings = self.entry_settings(&opening.1);
        let (tx, rx) = std::sync::mpsc::channel();
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let duration_s = header.duration_s;
        open_in_background(
            opening,
            settings,
            tx,
            progress.clone(),
            cancel.clone(),
            Some(duration_s),
        );
        self.opened_path = path.display().to_string();
        self.reset_session(header.sample_rate, label.clone());
        self.load_reserve(path);
        self.no_sound = true;
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

    pub(super) fn error(&mut self, m: String) {
        applog::error(&m);
        self.message = Some((plain_error(&m), true));
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
        if let Some(fault) = self.session.recording_fault() {
            self.error(fault.description());
        }
    }

    /// Take in whatever audio has arrived.
    pub(super) fn poll(&mut self) {
        self.poll_for(FRAME_BUDGET);
    }

    /// One block of a capture, or a break core's gap rule found before the
    /// next: saved and read.
    fn take_live(&mut self, l: Live) -> Result<(), CaptureFailure> {
        let (Some(cap), Some(live)) = (self.capture.as_ref(), self.live.as_mut()) else {
            return Ok(());
        };
        let b = match l {
            Live::Block(b) => b,
            Live::Break(gap) => {
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
                }
                if let Some(t) = self.clock_track.as_mut() {
                    t.restart(live.sample_rate());
                }
                let m = format!(
                    "The sound's timestamps jumped by {}, as they do when the \
                     computer sleeps. The jump is left out of the take as a \
                     break, and listening carries on.",
                    fields::duration(gap.round())
                );
                applog::warn(&m);
                self.message = Some((m, false));
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
                self.sound_history.push((r.time_s, p.clone()));
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
        self.refresh_trend();
        let (taken, ended, failure) = self.take_blocks(started, budget);
        self.capture_outcome(ended, failure);

        self.note_lock();

        self.poll_analysis_save();
        self.poll_saving();
        self.poll_batch();

        self.hold_anchor();
        taken
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
            // A live input's blocks go through core's gap rule, which lets
            // each through once no break before it is in doubt (#689).
            let came = match cap.rx.try_recv() {
                Ok(Event::Audio(b)) => {
                    taken += 1;
                    // Heard as it arrives, though core's gap rule may hold
                    // it a while.
                    self.audio_at = Some(Instant::now());
                    if is_file {
                        vec![Live::Block(b)]
                    } else {
                        let rate = self.live.as_ref().map_or(1, |l| l.sample_rate());
                        self.gaps.get_or_insert_with(|| GapWatch::new(rate)).push(b)
                    }
                }
                Ok(Event::Error(e)) if is_file => {
                    failure = Some(CaptureFailure::Input(format!(
                        "Input stopped. {}",
                        capture::explain_error(&e)
                    )));
                    continue;
                }
                Ok(Event::Error(e)) => {
                    failure = Some(CaptureFailure::Input(format!(
                        "Input stopped. {}",
                        capture::explain_error(&e)
                    )));
                    ended = true;
                    self.gaps.as_mut().map(GapWatch::finish).unwrap_or_default()
                }
                Ok(Event::End) | Err(TryRecvError::Disconnected) => {
                    ended = true;
                    self.gaps.as_mut().map(GapWatch::finish).unwrap_or_default()
                }
                Err(TryRecvError::Empty) => break,
            };
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
                        log,
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
                    self.show_log(log, label, stretches, settles);
                    if let Some((_, opened)) = carried {
                        self.opened_positions = opened;
                    }
                    // A take with no sound shows the profiles it kept.
                    if let Some(k) = profiles {
                        self.sound_history = k.into_iter().map(|p| (p.end_s, p.sides)).collect();
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

    pub(super) fn show_log(
        &mut self,
        log: BeatLog,
        label: String,
        stretches: Vec<(f64, Position)>,
        settles: Vec<(f64, f64)>,
    ) {
        let mut live = LiveAnalyzer::from_log(log, self.live_config());
        self.session.load(&[], &[]);
        self.session.analysed();
        // No reading fits across one of the take's breaks.
        let mut breaks: Vec<f64> = self
            .take_breaks
            .iter()
            .flatten()
            .map(|b| b.audio_s)
            .collect();
        // A take of one position settles too, unless its session file
        // says it was read from its start.
        if stretches.len() > 1 || !settles.is_empty() {
            // Nor across a move, nor across a settling's end.
            breaks.extend(
                stretches
                    .iter()
                    .skip(1)
                    .map(|s| s.0)
                    .chain(settles.iter().map(|s| s.1)),
            );
            self.session.load(&stretches, &settles);
        }
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        live.set_breaks(breaks);
        if let Some(&(_, p)) = self.session.stretches().last() {
            self.position = p;
        }
        if let Some((s, t)) = self.pending_recorded.take() {
            self.session.load_recorded(&s, &t, self.position);
        }
        self.opened_positions = Some(self.positions_now());
        live.set_masked(&self.masked);
        // The new engine's first sight of the take's clock; after this only
        // put_take_clock gives it one.
        live.set_rate_clock(self.take_rate_clock());
        self.live = Some(live);
        self.rev.new_take();
        self.refresh_trend();
        self.source_label = label;
        self.anchor = None;
        self.strip_end = None;
        self.fit_take();
        let done = if self.no_sound {
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
