//! The Scope and Sound panes: the beats' waveform and the Tick Tock profiles.

use super::*;

// The kept-sound rule is core's (#483).
#[cfg(test)]
pub(super) use timegrapher_core::profile::kept_sound;
pub(super) use timegrapher_core::profile::{sound_at, thin_sound, SoundAt};

/// The Tick Tock profiles kept for a moment, and the seconds of beats
/// before it they were made over.
#[derive(Debug, Clone)]
pub(super) struct Kept {
    pub(super) span_s: f64,
    pub(super) sides: Profiles,
}

/// What a sound pane shows for a moment: the profiles, with a line for
/// their hover; or one line in their place.
pub(super) struct ShownSound {
    pub(super) profiles: Profiles,
    pub(super) line: Option<String>,
    pub(super) hover: Option<String>,
    /// What the band is ([`profiles::BAND_BEATS`] or
    /// [`profiles::BAND_AVERAGED`]).
    pub(super) band: &'static str,
    /// The moment shown is masked out, by core's rule for the sound kept
    /// there ([`timegrapher_core::profile::sound_at`]): the Scope draws no
    /// beats there either.
    pub(super) masked: bool,
}

/// One line in a sound pane's plot's place.
fn not_shown(line: String, masked: bool) -> ShownSound {
    ShownSound {
        profiles: [None, None],
        line: Some(line),
        hover: None,
        band: profiles::BAND_BEATS,
        masked,
    }
}

/// The moment `at` is masked out ([`SoundAt::Masked`]).
fn masked_at(at: &str) -> ShownSound {
    not_shown(format!("Masked at {at}"), true)
}

/// No beats were heard at `at` ([`SoundAt::NoBeats`]).
fn no_beats_at(at: &str) -> ShownSound {
    not_shown(format!("No beats at {at}"), false)
}

/// No sound was kept for `at` ([`SoundAt::NotKept`]).
fn none_kept_at(at: &str) -> ShownSound {
    not_shown(format!("No sound kept from {at}"), false)
}

/// One line in place of a sound pane's plot, as a pane too short for its
/// plot says so.
fn no_sound_line(ui: &mut egui::Ui, line: &str) {
    let pal = theme::pal(ui);
    ui.centered_and_justified(|ui| {
        ui.label(RichText::new(line).color(pal.text_secondary));
    });
}

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
            "The rings where the three sounds of each beat rise, Unlock Sound, Impulse \
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
        let end = self.strip_end_s();
        let shown = self.profiles_at(end);
        if let Some(line) = &shown.line {
            return no_sound_line(ui, line);
        }
        let i = self.trend.partition_point(|p| p.t <= end + 1e-9);
        let drop_be = i.checked_sub(1).and_then(|i| self.trend[i].beat_error_ms);
        // Moved back, the moment first: the strip says it, and the hover
        // says where it was set.
        let note = match (self.moment_hover(), shown.hover) {
            (Some(m), Some(h)) => Some(format!("{m}\n{h}")),
            (m, h) => m.or(h),
        };
        profiles::draw(
            ui,
            &shown.profiles,
            self.sound_opts,
            drop_be,
            note.as_deref(),
            shown.band,
        );
    }

    /// The Tick Tock Profile's or the Scope's toolbar. The moment they
    /// show is said once, on the strip that sets it (#1068).
    pub(super) fn sound_tools(&mut self, ui: &mut egui::Ui, pane: Pane) {
        let help = if pane == Pane::Sound {
            help::PROFILE
        } else {
            help::SCOPE
        };
        theme::pane_toolbar(ui, pane.title(), help, |ui| {
            if pane == Pane::Sound {
                self.profile_settings(ui);
            } else {
                scope::settings(ui, &mut self.scope_view);
                self.export_button(ui, pane);
            }
        });
    }

    /// When the strip has been moved off the newest beats, the time at its
    /// newest edge, which the sound panes show.
    pub(super) fn sound_moment(&self) -> Option<String> {
        self.strip_end
            .is_some()
            .then(|| format!("at {}", length_text(self.strip_end_s())))
    }

    /// For a pane's hover when the strip is moved back: the moment, and
    /// that the strip set it.
    pub(super) fn moment_hover(&self) -> Option<String> {
        Some(format!("{}, set by the Paper Strip.", self.sound_moment()?))
    }

    /// The Tick Tock profiles for the window ending at `end`, the strip's
    /// newest edge: the latest while listening at the newest beats; else
    /// what the session kept for it ([`timegrapher_core::profile::sound_at`]),
    /// else worked out from the recording (none until that is done); or
    /// the line to show in their place.
    pub(super) fn profiles_at(&mut self, end: f64) -> ShownSound {
        let at = length_text(end);
        let shown = |profiles: Profiles, hover: Option<String>| ShownSound {
            profiles,
            line: None,
            hover,
            band: profiles::BAND_BEATS,
            masked: false,
        };
        let line = |l: String| not_shown(l, false);
        if self.running() && self.strip_end.is_none() {
            let p = self
                .live
                .as_ref()
                .map_or([None, None], |l| l.tick_profiles().clone());
            return shown(p, None);
        }
        if let Some(kept) = &self.no_sound {
            return self.kept_average(kept, end);
        }
        let beats = self.live.as_ref().map_or(&[][..], |l| l.beats());
        let window = (end, self.average_s);
        match sound_at(&self.sound_history, window, &self.masked, beats, |k| {
            k.span_s
        }) {
            SoundAt::Kept { coarse, sound, .. } => {
                let mut hover = Vec::new();
                if coarse {
                    hover.push("Shapes from then are kept once a minute".to_string());
                }
                if sound.span_s != self.sound_span() {
                    hover.push(format!(
                        "Made over the {} s Average Over set then",
                        sound.span_s
                    ));
                }
                let p = sound.sides.clone();
                shown(p, (!hover.is_empty()).then(|| hover.join("\n")))
            }
            SoundAt::Masked => masked_at(&at),
            SoundAt::NoBeats => no_beats_at(&at),
            SoundAt::NotKept => {
                self.poll_sound_job();
                let worked_out = match &self.sound_at {
                    Some((t, rev, p)) if (t - end).abs() < 0.5 && *rev == self.rev.settings => {
                        Some(p.clone())
                    }
                    _ => None,
                };
                match worked_out {
                    Some(p) => shown(p, None),
                    None if self.start_sound_job(end) => {
                        line("Working it out from the recording…".to_string())
                    }
                    // At the newest beats, before any sound is kept: the
                    // empty plots.
                    None if self.strip_end.is_none() => shown([None, None], None),
                    None => none_kept_at(&at),
                }
            }
        }
    }

    /// For a take kept without its sound: the Tick Tock Profile over the
    /// Average Over window ending at `end`, averaged from the 2 s profiles
    /// it kept (`kept`; [`timegrapher_core::profile::profile_at`], #979),
    /// with what it is made of for the hover; or the line in its place.
    fn kept_average(
        &self,
        kept: &[timegrapher_core::analysis_take::StretchProfiles],
        end: f64,
    ) -> ShownSound {
        use timegrapher_core::profile::{profile_at, ProfileAt};
        let at = length_text(end);
        let Some(live) = self.live.as_ref() else {
            return no_beats_at(&at);
        };
        let osc_period_s = 2.0 * timegrapher_core::power_reserve::beat_s(live.bph());
        let sides = match profile_at(
            kept,
            (end, self.average_s),
            &self.masked,
            self.take_breaks.as_deref().unwrap_or_default(),
            live.beats(),
            osc_period_s,
            &live.config().analysis.amplitude,
        ) {
            ProfileAt::Averaged(sides) => *sides,
            ProfileAt::Masked => return masked_at(&at),
            ProfileAt::NoBeats => return no_beats_at(&at),
            ProfileAt::NotKept => return none_kept_at(&at),
        };
        let mut hover: Vec<String> = ["Tick", "Tock"]
            .iter()
            .zip(&sides)
            .map(|(name, s)| match s {
                Some(s) => {
                    let what = match s.stretches {
                        1 => "the 2 s profile".to_string(),
                        n => format!("average of {n} 2 s profiles"),
                    };
                    let (a, b) = (length_text(s.from_s), length_text(s.to_s));
                    format!("{name}: {what}, {a} to {b}")
                }
                None => format!("{name}: none kept"),
            })
            .collect();
        hover.push(format!("Band: {}", profiles::BAND_AVERAGED));
        hover.push("This take kept no sound".to_string());
        ShownSound {
            profiles: sides.map(|s| s.map(|s| s.profile)),
            line: None,
            hover: Some(hover.join("\n")),
            band: profiles::BAND_AVERAGED,
            masked: false,
        }
    }

    /// The Scope pane: the latest beats' sound up to the time on show, from
    /// what the live engine still holds, else read from the recording.
    pub(super) fn scope_pane(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let view = self.scope_view;
        let end = self.strip_end_s();
        let shown = self.profiles_at(end);
        // Masked out where the profile says so, by the same rule.
        if let (true, Some(line)) = (shown.masked, &shown.line) {
            return no_sound_line(ui, line);
        }
        let profiles = shown.profiles;
        let Some(live) = self.live.as_ref().filter(|_| self.no_sound.is_none()) else {
            let words = if self.live.is_none() {
                "Listen to a watch or open a recording to see its sound here."
            } else {
                "This take's sound was removed, so there is no waveform to show."
            };
            ui.label(RichText::new(words).color(pal.text_tertiary));
            return;
        };
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
            match self.scope_cut(lo, hi) {
                Some(cut) => cut,
                None if known.is_empty() => {
                    return no_sound_line(ui, &format!("No sound kept from {}", length_text(end)));
                }
                // Some beats drawn from what is held, the rest not kept:
                // said over the Scope.
                None => (
                    None,
                    Some(format!("No sound kept from {}", length_text(end))),
                ),
            }
        };
        let moment = self.moment_hover();
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
                let side = profiles[usize::from(!even)].as_ref();
                let marks = if view.zoom.marks() {
                    scope_marks(side, pal, moment.as_deref())
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
    /// or when it can't be read; None when no sound of it is kept.
    fn scope_cut(&mut self, lo: f64, hi: f64) -> Option<ScopeSound> {
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
                    Some((Some(cut), None))
                }
                Err(e) => Some((None, Some(format!("Couldn't read the sound here: {e}.")))),
            },
            _ => self.start_scope_job(lo, hi).then(|| {
                (
                    None,
                    Some("Reading the sound from the recording…".to_string()),
                )
            }),
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
    /// side) from the recording open, or while listening from the take
    /// being recorded ([`Self::heard_files`]).
    /// False when there is no recording to read it from.
    pub(super) fn start_scope_job(&mut self, lo: f64, hi: f64) -> bool {
        if self.scope_job.is_some() {
            return true;
        }
        let (files, shift) = if self.session.mic() {
            match self.heard_files(lo, hi) {
                Some(heard) => heard,
                None => return false,
            }
        } else if self.input == Input::File {
            match recordings_in(&PathBuf::from(self.file_path.trim())) {
                Ok(files) => (files, 0.0),
                Err(_) => return false,
            }
        } else {
            return false;
        };
        let (a, b) = ((lo - 2.0).max(0.0), hi + 2.0);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // Read in the sound's own seconds, kept in the take's.
            let got = crate::span::read(&files, (a - shift).max(0.0), b - a).map(|mut sp| {
                sp.t0 += shift;
                sp
            });
            let _ = tx.send((a, b, got));
        });
        self.scope_job = Some(rx);
        true
    }

    /// While listening, the sound files recorded so far, written out so
    /// they can be read, and how far the take's time from `lo` to `hi`
    /// runs ahead of the sound kept (the pauses and breaks before it).
    /// The engine holds only the last few seconds, so a moment further
    /// back is read from these. None with no recording, or with a pause
    /// or break between `lo` and `hi`: the sound either side of it isn't
    /// one stretch.
    fn heard_files(&mut self, lo: f64, hi: f64) -> Option<(Vec<PathBuf>, f64)> {
        let shift = self.session.sound_shift(lo, hi)?;
        let r = self.session.recorder.as_mut()?;
        r.flush().ok()?;
        Some((r.segment_paths(), shift))
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
/// shown; their hovers start with `moment` when the strip is moved back.
fn scope_marks(
    profile: Option<&TickProfile>,
    pal: &theme::Palette,
    moment: Option<&str>,
) -> Vec<scope::Mark> {
    let mut marks = Vec::new();
    let Some(p) = profile else {
        return marks;
    };
    if let Some(ms) = p.unlock_ms {
        marks.push(scope::Mark {
            ms,
            color: pal.unlock,
            name: "Unlock",
            moment: moment.map(str::to_string),
        });
    }
    if let Some(ms) = p.drop_ms {
        marks.push(scope::Mark {
            ms,
            color: pal.drop,
            name: "Drop",
            moment: moment.map(str::to_string),
        });
    }
    marks
}
