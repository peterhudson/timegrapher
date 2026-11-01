//! The Scope and Sound panes: the beats' waveform and the Tick Tock profiles.

use super::*;

impl TimegrapherApp {
    pub(super) fn profile_settings(&mut self, ui: &mut egui::Ui) {
        let accent = theme::pal(ui).accent;
        let o = &mut self.sound_opts;
        theme::tool_choice(
            ui,
            "Layout",
            "Tick above tock, or side by side as tg shows them",
            &mut o.side_by_side,
            &[
                (false, Icon::Stacked, "Stacked", "Tick above tock"),
                (
                    true,
                    Icon::Beside,
                    "Beside",
                    "Tick on the left, tock on the right, as tg shows them",
                ),
            ],
        );
        let mut db = o.scale == profiles::Scale::Decibels;
        theme::tool_switch(
            ui,
            Icon::Db,
            accent,
            "dB Scale",
            &mut db,
            "Show the level in decibels. Off: linear.",
        );
        o.scale = if db {
            profiles::Scale::Decibels
        } else {
            profiles::Scale::Linear
        };
        theme::bar_gap(ui);
        theme::tool_switch(
            ui,
            Icon::PalletFork,
            accent,
            "Edges",
            &mut o.edges,
            "The solid unlock, drop and peak lines: the edges amplitude and the beat error \
             are read from",
        );
        theme::tool_switch(
            ui,
            Icon::Notes,
            accent,
            "Sounds",
            &mut o.sounds,
            "The dots where the three sounds of each beat rise, Unlock Sound, Impulse \
             Sound and Drop Sound, named below the plot",
        );
        theme::bar_gap(ui);
        theme::tool_choice(
            ui,
            "Time From",
            "Where each side's time is measured from",
            &mut o.shared_clock,
            &[
                (
                    true,
                    Icon::OneClock,
                    "Shared",
                    "Tick and tock on one clock: the drops stand the beat error from the \
                     drop apart, and the unlocks the beat error from the unlock",
                ),
                (
                    false,
                    Icon::TwoClocks,
                    "Own Drop",
                    "Each side from its own drop, so both drops sit at 0 ms",
                ),
            ],
        );
        theme::bar_gap(ui);
        self.export_button(ui, Pane::Sound);
    }

    pub(super) fn sound_pane(&mut self, ui: &mut egui::Ui) {
        let end = self.latest_s();
        let latest = self.running();
        let mut note = None;
        let shown: Profiles = if latest {
            self.live
                .as_ref()
                .map_or([None, None], |l| l.tick_profiles().clone())
        } else {
            // Stopped: what the session kept nearest before the end,
            // else work it out from the file.
            let i = self.sound_history.partition_point(|h| h.0 <= end + 1e-9);
            match i.checked_sub(1).map(|i| &self.sound_history[i]) {
                Some((t, p)) if end - t <= SOUND_EVERY_S + 0.5 => {
                    note = Some(format!("At {}", length_text(*t)));
                    p.clone()
                }
                _ => {
                    self.poll_sound_job();
                    match &self.sound_at {
                        Some((t, rev, p)) if (t - end).abs() < 0.5 && *rev == self.rev.settings => {
                            note = Some(format!("At {}", length_text(*t)));
                            p.clone()
                        }
                        _ => {
                            note = Some(if self.start_sound_job(end) {
                                "Working It Out from the Recording…".to_string()
                            } else {
                                "Not Kept for This Moment".to_string()
                            });
                            [None, None]
                        }
                    }
                }
            }
        };
        let i = self.trend.partition_point(|p| p.t <= end + 1e-9);
        let drop_be = i.checked_sub(1).and_then(|i| self.trend[i].beat_error_drop);
        // The moment shown goes in the toolbar, drawn before the pane: a
        // change shows on the next frame.
        if note != self.sound_note {
            self.sound_note = note;
            ui.ctx().request_repaint();
        }
        profiles::draw(ui, &shown, self.sound_opts, drop_be);
    }

    /// The Tick Tock profiles nearest `end`, as the profile pane shows them:
    /// the latest while listening, else what the session kept, else worked
    /// out from the recording (none until that is done).
    fn profiles_at(&mut self, end: f64) -> Profiles {
        let latest = self.running();
        let i = self.sound_history.partition_point(|h| h.0 <= end + 1e-9);
        let kept = i
            .checked_sub(1)
            .map(|i| &self.sound_history[i])
            .filter(|(t, _)| end - t <= SOUND_EVERY_S + 0.5)
            .map(|(_, p)| p.clone());
        match kept {
            _ if latest => self
                .live
                .as_ref()
                .map_or([None, None], |l| l.tick_profiles().clone()),
            Some(p) => p,
            None => {
                self.poll_sound_job();
                match &self.sound_at {
                    Some((t, rev, p)) if (t - end).abs() < 0.5 && *rev == self.rev.settings => {
                        p.clone()
                    }
                    _ => {
                        self.start_sound_job(end);
                        [None, None]
                    }
                }
            }
        }
    }

    /// The Scope pane: the latest beats' sound up to the time on show, from
    /// what the live engine still holds, else read from the recording.
    pub(super) fn scope_pane(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let view = self.scope_view;
        let profiles = self.profiles_at(self.latest_s());
        let Some(live) = &self.live else {
            ui.label(
                RichText::new("Listen to a watch or open a recording to see its sound here.")
                    .color(pal.text_tertiary),
            );
            return;
        };
        if self.no_sound {
            ui.label(
                RichText::new("This take's sound was removed, so there is no waveform to show.")
                    .color(pal.text_tertiary),
            );
            return;
        }
        let end = self.latest_s();
        // One Beat follows the watch's beat rate, 28,800 bph until known.
        let beat_s = 3600.0 / f64::from(live.bph().unwrap_or(28_800).max(1));
        let split = view.zoom.split();
        let specs: Vec<(&'static str, bool, Color32)> = if split {
            vec![("Tick", true, pal.tick), ("Tock", false, pal.tock)]
        } else {
            vec![("Tick", true, pal.text_secondary)]
        };
        let picks: Vec<Vec<timegrapher_core::beats::Beat>> = specs
            .iter()
            .map(|&(_, even, _)| {
                scope::pick(live.beats(), view.zoom, beat_s, view.ring, end, Some(even))
            })
            .collect();
        // The marks from the profile nearest the moment shown, as the
        // Tick Tock Profile shows them.
        let fs = live.sample_rate() as f64;
        let (before, after) = view.zoom.window(beat_s);
        // Each beat from the traces already drawn, else from the sound the
        // engine still holds; only the rest need reading from the recording.
        let key = |b: &timegrapher_core::beats::Beat| scope_key(b, view.zoom, beat_s);
        let known = scope_known(
            live,
            &mut self.scope_traces,
            &picks,
            (view.zoom, beat_s),
            fs,
            (before, after),
        );
        let missing: Vec<&timegrapher_core::beats::Beat> = picks
            .iter()
            .flatten()
            .filter(|b| !known.contains_key(&key(b)))
            .collect();
        let times = missing.iter().map(|b| b.time);
        // A little either side, so rounding never cuts a trace short.
        let lo = (times.clone().fold(f64::INFINITY, f64::min) - before - 0.015).max(0.0);
        let hi = times.fold(f64::NEG_INFINITY, f64::max) + after + 0.005;
        let listening = self.running();
        let (audio, note) = if lo >= hi || (listening && !known.is_empty()) {
            // Nothing more to read, or while listening a beat whose sound
            // went before it was drawn: the others carry on without it.
            (None, None)
        } else {
            self.poll_scope_job();
            self.scope_cut(lo, hi)
        };
        let rows: Vec<scope::Row> = specs
            .iter()
            .zip(&picks)
            .map(|(&(label, even, color), beats)| {
                let traces = beats
                    .iter()
                    .filter_map(|b| {
                        known.get(&key(b)).cloned().or_else(|| {
                            let (x, t0, rate) = audio.as_ref()?;
                            scope::trace(x, *rate, *t0, b, view.zoom, beat_s)
                        })
                    })
                    .collect();
                let marks = if view.zoom.marks() {
                    scope_marks(profiles[usize::from(!even)].as_ref(), pal)
                } else {
                    Vec::new()
                };
                scope::Row {
                    label: if split { label } else { "" },
                    traces,
                    color,
                    marks,
                }
            })
            .collect();
        if let Some(n) = note {
            ui.label(RichText::new(n).small().color(pal.text_tertiary));
        }
        let rate = audio.as_ref().map_or(fs, |a| a.2);
        scope::draw(ui, &rows, view.zoom, beat_s, rate);
    }

    /// The sound from `lo` to `hi` s from the recording read in the
    /// background, with the line to show over the Scope while it is read,
    /// or when it can't be.
    fn scope_cut(&mut self, lo: f64, hi: f64) -> ScopeSound {
        match &self.scope_audio {
            // What was asked for covers these beats: use what was read,
            // even when the recording ended before it, and never ask
            // again for a stretch that couldn't be read.
            Some((a, b, got)) if *a <= lo && *b >= hi => match got {
                Ok(sp) => {
                    let i0 =
                        (((lo - sp.t0) * sp.rate).floor().max(0.0) as usize).min(sp.mono.len());
                    let i1 = (((hi - sp.t0) * sp.rate).ceil().max(0.0) as usize)
                        .clamp(i0, sp.mono.len());
                    let cut = (
                        sp.mono[i0..i1].to_vec(),
                        sp.t0 + i0 as f64 / sp.rate,
                        sp.rate,
                    );
                    (Some(cut), None)
                }
                Err(e) => (None, Some(format!("Couldn't read the sound here: {e}."))),
            },
            _ => {
                let note = Some(
                    if self.start_scope_job(lo, hi) {
                        "Reading the sound from the recording…"
                    } else {
                        "The sound of these beats isn't kept: the Scope shows the last \
                         few seconds while listening, and any moment of a recording."
                    }
                    .to_string(),
                );
                (None, note)
            }
        }
    }

    fn poll_scope_job(&mut self) {
        if let Some(rx) = &self.scope_job {
            match rx.try_recv() {
                Ok(cut) => {
                    self.scope_audio = Some(cut);
                    self.scope_job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.scope_job = None;
                    self.scope_audio = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
    }

    /// Read the sound from `lo` to `hi` seconds (and two seconds either
    /// side) from the recording open.
    /// False when there is no recording to read it from.
    fn start_scope_job(&mut self, lo: f64, hi: f64) -> bool {
        if self.scope_job.is_some() {
            return true;
        }
        if self.input != Input::File || self.session.mic() {
            return false;
        }
        let Ok(files) = recordings_in(&PathBuf::from(self.file_path.trim())) else {
            return false;
        };
        let (a, b) = ((lo - 2.0).max(0.0), hi + 2.0);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send((a, b, crate::span::read(&files, a, b - a)));
        });
        self.scope_job = Some(rx);
        true
    }

    /// Start working out the sound at `end` from the file, unless one is
    /// already under way. False when there is no file to read.
    fn start_sound_job(&mut self, end: f64) -> bool {
        // One at a time: while the strip is dragged, the next starts when
        // this one is done.
        if self.sound_job.is_some() {
            return true;
        }
        let path = PathBuf::from(self.file_path.trim());
        let Some(live) = &self.live else { return false };
        let Some(bph) = live.bph() else { return false };
        if self.input != Input::File || self.session.mic() {
            return false;
        }
        let Ok(files) = recordings_in(&path) else {
            return false;
        };
        let span = live.config().profile_s.max(0.5);
        let from = (end - span).max(0.0);
        // A little audio either side of the beats for the template windows.
        let (a0, a1) = (from - 0.05, end + 0.05);
        let beats: Vec<timegrapher_core::beats::Beat> = live
            .beats()
            .iter()
            .filter(|b| b.time >= from && b.time < end)
            .map(|b| timegrapher_core::beats::Beat {
                time: b.time - a0.max(0.0),
                ..*b
            })
            .collect();
        let cfg = live.config().analysis.clone();
        let guides = live.profile_guides_at(end);
        let period = 7200.0 / bph as f64;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // Only this stretch is read, however long the recording; when
            // it can't be, the answer is no profile, not another try.
            let a0 = a0.max(0.0);
            let p = match crate::span::read(&files, a0, a1 - a0) {
                Ok(sp) => {
                    let beats: Vec<_> = beats
                        .iter()
                        .map(|b| timegrapher_core::beats::Beat {
                            time: b.time + a0 - sp.t0,
                            ..*b
                        })
                        .collect();
                    timegrapher_core::profile::profiles_from_audio(
                        &sp.mono, sp.rate, &beats, period, &cfg, guides,
                    )
                }
                Err(_) => [None, None],
            };
            let _ = tx.send(p);
        });
        self.sound_job = Some((end, self.rev.settings, rx));
        true
    }

    fn poll_sound_job(&mut self) {
        // Profiles read at settings changed since are read again.
        if self
            .sound_job
            .as_ref()
            .is_some_and(|j| j.1 != self.rev.settings)
        {
            self.sound_job = None;
        }
        if let Some((t, rev, rx)) = &self.sound_job {
            match rx.try_recv() {
                Ok(p) => {
                    self.sound_at = Some((*t, *rev, p));
                    self.sound_job = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.sound_at = Some((*t, *rev, [None, None]));
                    self.sound_job = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
    }
}

/// Sound read for the Scope (samples, start s, rate) and the line to show
/// over it.
type ScopeSound = (Option<(Vec<f32>, f64, f64)>, Option<String>);

/// Where a beat's trace is kept: its index and time at this zoom and beat.
fn scope_key(b: &timegrapher_core::beats::Beat, zoom: scope::Zoom, beat_s: f64) -> ScopeKey {
    (b.index, b.time.to_bits(), zoom, beat_s.to_bits())
}

/// Each beat's trace from the traces already drawn, else from the sound
/// the engine still holds, keeping the newest `SCOPE_KEPT`. The rest need
/// reading from the recording.
fn scope_known(
    live: &LiveAnalyzer,
    kept: &mut std::collections::VecDeque<(ScopeKey, Vec<f32>)>,
    picks: &[Vec<timegrapher_core::beats::Beat>],
    (zoom, beat_s): (scope::Zoom, f64),
    fs: f64,
    (before, after): (f64, f64),
) -> std::collections::HashMap<ScopeKey, Vec<f32>> {
    let mut known: std::collections::HashMap<ScopeKey, Vec<f32>> = Default::default();
    for b in picks.iter().flatten() {
        let k = scope_key(b, zoom, beat_s);
        if let Some((_, t)) = kept.iter().find(|(c, _)| *c == k) {
            known.insert(k, t.clone());
            continue;
        }
        let (a, z) = (b.time - before - 0.015, b.time + after + 0.005);
        if let Some(t) = live
            .sound(a, z - a)
            .and_then(|x| scope::trace(x, fs, a, b, zoom, beat_s))
        {
            kept.push_back((k, t.clone()));
            known.insert(k, t);
        }
    }
    while kept.len() > SCOPE_KEPT {
        kept.pop_front();
    }
    known
}

/// The Unlock and Drop marks from the Tick Tock Profile nearest the moment
/// shown.
fn scope_marks(profile: Option<&TickProfile>, pal: &theme::Palette) -> Vec<scope::Mark> {
    let mut marks = Vec::new();
    let Some(p) = profile else {
        return marks;
    };
    if let Some(ms) = p.unlock_ms {
        marks.push(scope::Mark {
            ms,
            color: pal.unlock,
            name: "Unlock",
        });
    }
    if let Some(ms) = p.drop_ms {
        marks.push(scope::Mark {
            ms,
            color: pal.drop,
            name: "Drop",
        });
    }
    marks
}
