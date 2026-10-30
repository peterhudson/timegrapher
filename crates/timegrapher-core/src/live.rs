//! Live analysis: audio arrives a block at a time and the readings follow.
//!
//! Every half second the last few seconds of audio are run through the
//! same envelope, template and beat tracker as a recording, and the beats
//! that have settled since the previous pass are added to a running beat
//! log. The template is built once, from the first seconds of signal, so
//! the beat's reference point stays put and the trace doesn't jump. The
//! readings (rate, beat error, amplitude) are fits over the last few
//! seconds of that log, so the live screen and an analysis of the saved
//! recording read from the same kind of per-beat records.

use crate::amplitude::{self, AmplitudeWindow};
use crate::analysis::{AnalysisConfig, Clipping};
use crate::beats::{self, Beat};
use crate::breaks::Break;
use crate::clean::Cuts;
use crate::dsp::{envelope, median_f32};
use crate::measure::{self, Caveat, NoWatchReason};
use crate::profile::{self, Side, TickProfile};
use std::collections::VecDeque;

pub use crate::clean::GAP_S;

/// Longest stretch the tick and tock profiles can cover, seconds.
pub const MAX_PROFILE_S: f64 = 60.0;

/// How far back the rate's ± looks for its 10 s windows, seconds: the
/// latest unbroken run of beats, up to this long. The averaging time
/// holds too few windows to judge the rate on its own.
pub const RATE_RUN_S: f64 = 300.0;

use crate::stream::BeatLog;
use crate::timing;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct LiveConfig {
    /// Beat rate (`None` guesses it), lift angle, filters and the amplitude
    /// window length.
    pub analysis: AnalysisConfig,
    /// Audio analysed on each pass, seconds.
    pub window_s: f64,
    /// Time between passes, seconds.
    pub update_s: f64,
    /// Beats louder than this many times the envelope's median count as
    /// signal; below it the watch is taken to be off the microphone.
    pub min_snr: f32,
    /// Beats kept; the oldest are dropped beyond this (about a day at
    /// 28,800 bph).
    pub max_beats: usize,
    /// Settled beats behind the tick and tock profiles, seconds, up to
    /// `MAX_PROFILE_S`. Each beat's sound is kept for that long (about 5 kB
    /// a beat at 48 kHz, 2.6 MB at 60 s and 28,800 bph).
    pub profile_s: f64,
}

impl Default for LiveConfig {
    fn default() -> Self {
        LiveConfig {
            analysis: AnalysisConfig::default(),
            window_s: 4.0,
            update_s: 0.5,
            min_snr: 4.0,
            max_beats: 700_000,
            profile_s: 3.0,
        }
    }
}

/// The figures on the live screen, over the last `average_s` seconds.
#[derive(Debug, Clone, Serialize)]
pub struct LiveReading {
    /// Audio time of the reading, seconds from the start.
    pub time_s: f64,
    pub bph: Option<u32>,
    pub rate_s_per_day: Option<f64>,
    /// Signed beat error from the fit to the beat times (near the drop), ms.
    pub beat_error_ms: Option<f64>,
    /// Signed beat error measured from the unlock, the figure checked
    /// against tg's beat error in regress: median over the amplitude
    /// windows, ms. Positive means Tick comes late; each take starts
    /// positive ([`crate::beats::SideCheck`]).
    pub beat_error_unlock_ms: Option<f64>,
    pub amplitude_deg: Option<f64>,
    /// Roughly how far `amplitude_deg` could be off from the scatter of
    /// the 2 s windows alone, as `confidence::standard_error` gives it for a
    /// recording: precision, not accuracy, since a wrong lift angle moves
    /// every amplitude together.
    pub amplitude_error_deg: Option<f64>,
    pub amplitude_even_deg: Option<f64>,
    pub amplitude_odd_deg: Option<f64>,
    /// Timing scatter of the fit, microseconds.
    pub jitter_us: Option<f64>,
    /// `jitter_us` as a share of the beat period at `bph`, 0 to 1 (see
    /// `beats::timing_scatter_fraction`); what `watch_heard` is decided on.
    pub timing_scatter_fraction: Option<f64>,
    pub beats_used: usize,
    /// Seconds the readings actually cover: from the first beat of the
    /// latest unbroken run in the averaging time to the reading's time.
    /// Shorter than the averaging time early in a session or after a gap.
    pub span_s: f64,
    /// Whether the averaging time is a whole bin ([`Cuts::whole`]): it
    /// reaches into no break or masked stretch, and each 2 s of it holds
    /// a beat. A reading that is not is over less than its averaging time.
    pub whole: bool,
    /// Beat peak over the envelope's median on the latest pass.
    pub snr: Option<f32>,
    /// Beats in the reading with a clipped sample in their sound, counted as
    /// `analysis::Clipping` counts them for a recording.
    pub clipped_beats: usize,
    /// `clipped_beats` as a fraction of the beats in the reading.
    pub clipped_fraction: Option<f64>,
    /// False when a regular beat was looked for and not found
    /// (`measure::watch_heard` on the fit's jitter, assessed once the
    /// averaging time holds 10 s of sound). Then every reading is `None`,
    /// as `analyze` leaves them out; `jitter_us`,
    /// `timing_scatter_fraction` and `bph` stay.
    pub watch_heard: bool,
    /// The rate's ±, from `measure::assess` as `analyze` and `session`
    /// give theirs: the scatter of 10 s windows over the run so far,
    /// scaled to the averaging time. `None` until about 30 s of unbroken
    /// beats, and when no regular beat is found.
    pub rate_error_s_per_day: Option<f64>,
    /// `beat_error_unlock_ms`'s ±: the standard error of its 2 s windows
    /// in the averaging time.
    pub beat_error_unlock_error_ms: Option<f64>,
    /// What limits the readings, in `measure`'s order.
    pub caveats: Vec<Caveat>,
}

#[derive(Debug, Clone)]
pub struct LiveAnalyzer {
    fs: f64,
    cfg: LiveConfig,
    /// The most recent audio; `buf[0]` is sample `buf0` of the stream.
    buf: Vec<f32>,
    buf0: u64,
    total: u64,
    next_pass: u64,
    /// Beats before this time (seconds) are final.
    settled_to: f64,
    bph: Option<u32>,
    /// Whether `bph` is the rate Auto found, not one set.
    found_bph: bool,
    template: Option<Vec<f32>>,
    beats: Vec<Beat>,
    /// Times of the settled beats whose sound clipped, oldest first.
    clipped: Vec<f64>,
    amp: Vec<AmplitudeWindow>,
    next_amp_s: Option<f64>,
    /// Tick and tock sounds over the latest settled beats.
    profiles: [Option<TickProfile>; 2],
    /// Each settled beat's envelope window for the profiles: time, even
    /// side, samples. Oldest first.
    snippets: VecDeque<(f64, bool, Vec<f32>)>,
    snr: Option<f32>,
    /// Passes in a row with no signal.
    quiet_passes: u32,
    /// For display only: the peaks found in passes too quiet to log, each
    /// numbered from the first by the rate guessed then, the newest
    /// [`CANDIDATES_KEPT`]. Nothing is measured from them.
    candidates: VecDeque<Beat>,
    /// The beat rate the logged beats were numbered at, kept when a long
    /// silence lets a guessed rate go, so readings back in the log still
    /// fit.
    logged_bph: Option<u32>,
    /// Known jumps in an imported beat log. Replacing position/settling
    /// boundaries must not erase these intrinsic discontinuities.
    intrinsic_breaks: Vec<f64>,
    /// The jumps of an imported beat log whose sides were named by the beat
    /// count alone ([`BeatLog::by_count`]): a reading after one says so.
    by_count: Vec<f64>,
    /// Intrinsic jumps and caller-supplied stretch boundaries, oldest first:
    /// readings never fit across one.
    breaks: Vec<f64>,
    /// Stretches masked out of the readings by hand, start and end in
    /// seconds, sorted and apart: cut like breaks, and nothing read in
    /// them.
    masked: Vec<(f64, f64)>,
    /// Whether the sound card's clock is corrected, for the caveats.
    clock_known: bool,
    /// The take's clock the rates are corrected on
    /// ([`Self::set_rate_clock`]); none, the rates as heard.
    rate_clock: Option<crate::clock::ClockFit>,
    /// Tick and Tock told apart across each jump by their sound, as a
    /// whole file's reading tells them. Readings that name a side are left
    /// out until it has, and in a stretch it never could.
    sides: beats::SideCheck,
}

/// Good beats looked at for a step in their timing ([`beats::jumps`]) on
/// each pass: enough for its typical intervals, and the beats around a
/// step.
const JUMP_LOOK: usize = 4 * beats::MIN_SIDE_BEATS + beats::JUMP_CONTEXT;

/// How many candidate peaks [`LiveAnalyzer::candidates`] keeps: about four
/// minutes at 28,800 beats per hour, more than a strip shows.
const CANDIDATES_KEPT: usize = 2000;

impl LiveAnalyzer {
    pub fn new(sample_rate: u32, cfg: LiveConfig) -> Self {
        LiveAnalyzer {
            fs: sample_rate as f64,
            bph: cfg.analysis.bph,
            found_bph: false,
            cfg,
            buf: Vec::new(),
            buf0: 0,
            total: 0,
            next_pass: 0,
            settled_to: 0.0,
            template: None,
            beats: Vec::new(),
            clipped: Vec::new(),
            amp: Vec::new(),
            next_amp_s: None,
            profiles: [None, None],
            snippets: VecDeque::new(),
            sides: beats::SideCheck::default(),
            snr: None,
            quiet_passes: 0,
            candidates: VecDeque::new(),
            logged_bph: None,
            intrinsic_breaks: Vec::new(),
            by_count: Vec::new(),
            breaks: Vec::new(),
            masked: Vec::new(),
            clock_known: false,
            rate_clock: None,
        }
    }

    /// Readings over a whole recording analysed at once (`stream::analyze_file`),
    /// so a file can be looked at on the same screen as a live watch.
    pub fn from_log(log: BeatLog, cfg: LiveConfig) -> Self {
        let mut a = LiveAnalyzer::new(log.sample_rate, cfg);
        a.cfg.analysis.amplitude.lift_deg = log.lift_deg;
        a.total = (log.duration_s * a.fs).round() as u64;
        a.settled_to = log.duration_s;
        a.bph = Some(log.bph);
        a.logged_bph = Some(log.bph);
        // The clean beats' sound the log kept, each with its side, for the
        // check on a stretch under 10 s.
        let mut kept = log.snippets.into_iter().peekable();
        for b in &log.beats {
            while kept.next_if(|s| s.0 < b.time - 1e-9).is_some() {}
            if let Some(s) = kept.next_if(|s| (s.0 - b.time).abs() < 1e-9) {
                a.snippets.push_back((s.0, b.index.rem_euclid(2) == 0, s.1));
            }
        }
        a.beats = log.beats;
        a.amp = log.amplitude_windows;
        a.clipped = log.clipped_beats;
        a.intrinsic_breaks = log.jumps;
        a.by_count = log.by_count;
        a.set_breaks(Vec::new());
        a
    }

    pub fn sample_rate(&self) -> u32 {
        self.fs as u32
    }

    pub fn config(&self) -> &LiveConfig {
        &self.cfg
    }

    /// Seconds of audio received.
    pub fn duration_s(&self) -> f64 {
        self.total as f64 / self.fs
    }

    /// The beat rate in use, once known.
    pub fn bph(&self) -> Option<u32> {
        self.bph
    }

    /// Every settled beat; times are seconds from the start of the audio.
    pub fn beats(&self) -> &[Beat] {
        &self.beats
    }

    /// The peaks found while the sound was too quiet or irregular to log
    /// as beats, oldest first, for showing what was heard before a regular
    /// beat is found. They are not beats: no reading uses them.
    pub fn candidates(&self) -> Vec<Beat> {
        self.candidates.iter().copied().collect()
    }

    /// The sound of the tick and the tock (even and odd beats) over the last
    /// `profile_s` seconds of settled beats, with the unlock, drop and sound
    /// marks the engine measured on them. Empty for a file analysed at once.
    pub fn tick_profiles(&self) -> &[Option<TickProfile>; 2] {
        &self.profiles
    }

    /// Set how many seconds of settled beats the profiles cover (clamped to
    /// 0.5..=`MAX_PROFILE_S`), e.g. to follow the screen's averaging time.
    /// Beats already kept are reused, so a longer span fills in as beats
    /// arrive and a shorter one applies on the next pass.
    pub fn set_profile_span(&mut self, seconds: f64) {
        self.cfg.profile_s = seconds;
    }

    fn profile_span(&self) -> f64 {
        self.cfg.profile_s.clamp(0.5, MAX_PROFILE_S)
    }

    /// Times of the settled beats whose sound clipped, oldest first.
    pub fn clipped_beats(&self) -> &[f64] {
        &self.clipped
    }

    pub fn amplitude_windows(&self) -> &[AmplitudeWindow] {
        &self.amp
    }

    /// Where Tick's and Tock's unlock sat, ms from the beat time, in the
    /// latest amplitude window to end by `t` seconds that read it (or the
    /// first after, before any has): the guide for a profile of the sound
    /// up to `t` ([`profile::from_snippets`]).
    pub fn profile_guides_at(&self, t: f64) -> [Option<f64>; 2] {
        let at = |side: fn(&AmplitudeWindow) -> Option<f64>| {
            let by = self.amp.partition_point(|w| w.end_s <= t + 1e-9);
            self.amp[..by]
                .iter()
                .rev()
                .find_map(side)
                .or_else(|| self.amp[by..].iter().find_map(side))
        };
        [at(|w| w.even_unlock_ms), at(|w| w.odd_unlock_ms)]
    }

    /// Forget the beats and the template, keeping the audio clock running
    /// (for a new watch or a new position).
    pub fn restart(&mut self) {
        self.template = None;
        self.bph = self.cfg.analysis.bph;
        self.found_bph = false;
        self.logged_bph = self.bph;
        self.beats.clear();
        self.clipped.clear();
        self.amp.clear();
        self.next_amp_s = None;
        self.profiles = [None, None];
        self.snippets.clear();
        self.settled_to = self.duration_s();
        self.quiet_passes = 0;
        self.candidates.clear();
        self.intrinsic_breaks.clear();
        self.by_count.clear();
        self.breaks.clear();
        self.sides = beats::SideCheck::default();
    }

    /// Start a new stretch now, for a new position: the beats so far are
    /// kept, the sound still being worked on (the watch being handled) is
    /// let go, the profiles start again, and no reading fits across the
    /// break.
    pub fn new_stretch(&mut self) {
        let now = self.duration_s();
        self.breaks.push(now);
        // The count across the handling is a guess: the sides are checked
        // by their sound once the watch is heard again.
        self.check_sides_from(now);
        self.profiles = [None, None];
        self.snippets.clear();
        self.next_amp_s = None;
        self.settled_to = now;
        self.next_pass = self.total;
    }

    /// Known jumps and stretch boundaries, seconds, oldest first.
    pub fn breaks(&self) -> &[f64] {
        &self.breaks
    }

    /// The jumps of an imported beat log whose sides were named by the
    /// beat count alone, seconds, oldest first ([`BeatLog::by_count`]);
    /// none for a session heard live.
    pub fn by_count(&self) -> &[f64] {
        &self.by_count
    }

    /// The jumps in the beats alone, seconds, oldest first: those found
    /// listening, or in the beat log a recording was read into.
    pub fn jumps(&self) -> &[f64] {
        &self.intrinsic_breaks
    }

    /// Whether Tick and Tock are told apart at `t` seconds: false from a
    /// jump until the beats after it have been compared with those before
    /// by their sound, and in a stretch between two jumps too close for
    /// that. Readings that name a side are left out there.
    pub fn sides_known_at(&self, t: f64) -> bool {
        self.sides.known_at(t)
    }

    /// A jump in the beats at `t` seconds: no reading or amplitude window
    /// reaches across it, and the sides after it are checked by their
    /// sound. One found by its timing within a second of a break already
    /// made is that break, as a whole file's reading has it.
    fn jump_at(&mut self, t: f64, by_timing: bool) {
        let near = if by_timing { 1.0 } else { 1e-9 };
        if self.breaks.iter().any(|&b| (b - t).abs() < near) {
            return;
        }
        for v in [&mut self.intrinsic_breaks, &mut self.breaks] {
            v.push(t);
            v.sort_by(f64::total_cmp);
        }
        // The amplitude's windows start again from it.
        self.amp.retain(|w| !(w.start_s < t && w.end_s > t));
        // A grid not yet started (after a new position) starts no earlier
        // than the settled beats, so no window reaches back across that
        // break either.
        match self.next_amp_s {
            None => self.next_amp_s = Some(t.max(self.settled_to)),
            Some(s) if s < t => self.next_amp_s = Some(t),
            Some(_) => {}
        }
        self.check_sides_from(t);
    }

    /// Check the sides of the beats from `t` against those before, once
    /// enough of them are heard ([`beats::SideCheck::jump`]). A stretch
    /// that can never be told apart has its sounds join neither side's
    /// profile.
    fn check_sides_from(&mut self, t: f64) {
        if let Some((a, b)) = self.sides.jump(t) {
            self.snippets.retain(|s| !(a <= s.0 && s.0 < b));
        }
    }

    /// Once the beats after a jump have been compared with those before
    /// ([`beats::SideCheck::settle`]) and they match the other way round,
    /// number every beat from the jump on one further, which swaps the
    /// sides back, as a whole file is read. Too alike to tell, the count
    /// stands.
    fn settle_sides(&mut self) {
        let Some((t, beats::SideCall::Swapped)) = self.sides.settle() else {
            return;
        };
        for b in self.beats.iter_mut().filter(|b| b.time >= t) {
            b.index += 1;
        }
        for s in self.snippets.iter_mut().filter(|s| s.0 >= t) {
            s.1 = !s.1;
        }
        for w in self.amp.iter_mut().filter(|w| w.start_s >= t) {
            w.swap_sides();
        }
    }

    /// Replace caller-supplied position/settling boundaries, retaining known
    /// jumps from the imported beat log. Distinct timestamps remain distinct;
    /// a position boundary is not evidence that a nearby jump is the same cut.
    pub fn set_breaks(&mut self, mut breaks: Vec<f64>) {
        breaks.extend_from_slice(&self.intrinsic_breaks);
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        self.breaks = breaks;
    }

    /// Stretches masked out of the readings, start and end in seconds.
    pub fn masked(&self) -> &[(f64, f64)] {
        &self.masked
    }

    /// Replace the stretches masked out of the readings (any order;
    /// overlapping ones are joined). The beats in them are kept, for the
    /// strip, but no reading uses them or reaches across one.
    pub fn set_masked(&mut self, spans: &[(f64, f64)]) {
        let pairs: Vec<[f64; 2]> = spans.iter().map(|&(a, b)| [a, b]).collect();
        self.masked = crate::session::masked_spans(&pairs);
    }

    /// Move the audio clock on by `seconds` with no audio, for a pause in
    /// listening: the beat log, template and readings are kept, the audio
    /// buffer starts afresh, and the beats after the pause carry on in time
    /// after a gap (which the readings don't fit across).
    pub fn skip(&mut self, seconds: f64) {
        let n = (seconds.max(0.0) * self.fs).round() as u64;
        self.total += n;
        self.buf.clear();
        self.buf0 = self.total;
        self.next_pass = self.total;
        self.settled_to = self.duration_s();
        self.next_amp_s = None;
        self.quiet_passes = 0;
    }

    /// Fix the beat rate, or `None` to guess it. Starts the beat log again
    /// unless the beats are read at that rate already (or it is `None` and
    /// Auto found that rate). True when beats were heard and stand.
    pub fn set_bph(&mut self, bph: Option<u32>) -> bool {
        let heard = self.bph.is_some();
        if bph != self.cfg.analysis.bph {
            self.cfg.analysis.bph = bph;
            // The rate the beats are already read at, or Auto where Auto
            // found that rate: the beats so far stand. Auto after a rate
            // set by hand listens for the rate afresh.
            let stands = match bph {
                Some(b) => Some(b) == self.bph,
                None => self.found_bph,
            };
            if !stands {
                self.restart();
                return false;
            }
        }
        heard
    }

    /// Say whether the lift angle is the calibre's and the sound card's
    /// clock is corrected: each unknown is a caveat.
    pub fn set_known(&mut self, lift_known: bool, clock_known: bool) {
        self.cfg.analysis.lift_known = lift_known;
        self.clock_known = clock_known;
    }

    /// Correct the rates read on the recording's own clock, `clock` (its
    /// log, or its USB lines' figure), as `timegrapher session` reads them:
    /// each reading on the clock's mapping over the reading's own span, so
    /// a clock that drifts corrects each reading by its error then. None
    /// leaves them as heard, on the card's clock. A corrected clock is a
    /// known one.
    pub fn set_rate_clock(&mut self, clock: Option<&crate::clock::ClockFit>) {
        // Set every frame, so copied only when it changes.
        let same = |a: &crate::clock::ClockFit, b: &crate::clock::ClockFit| {
            let key = |c: &crate::clock::ClockFit| {
                (c.ppm, c.points, c.span_s, c.breaks.len(), c.map(c.span_s))
            };
            key(a) == key(b)
        };
        match (&self.rate_clock, clock) {
            (Some(a), Some(b)) if same(a, b) => {}
            (None, None) => {}
            _ => self.rate_clock = clock.cloned(),
        }
    }

    /// A clock's error over audio `from..to`, ppm slow: its mapping's
    /// slope there.
    fn clock_ppm(c: &crate::clock::ClockFit, from: f64, to: f64) -> f64 {
        if to - from < 1e-6 {
            return c.ppm;
        }
        ((c.map(to) - c.map(from)) / (to - from) - 1.0) * 1e6
    }

    /// Change the lift angle. Amplitude is proportional to the lift angle
    /// for a given unlock-to-drop time, so the log is rescaled in place.
    pub fn set_lift(&mut self, lift_deg: f64) {
        let old = self.cfg.analysis.amplitude.lift_deg;
        if lift_deg > 0.0 && lift_deg != old {
            let k = lift_deg / old;
            for w in &mut self.amp {
                w.even_deg = w.even_deg.map(|a| a * k);
                w.odd_deg = w.odd_deg.map(|a| a * k);
            }
            for p in self.profiles.iter_mut().flatten() {
                p.amplitude_deg = p.amplitude_deg.map(|a| a * k);
            }
            self.cfg.analysis.amplitude.lift_deg = lift_deg;
        }
    }

    /// Keep up to `n` beats from now on, the oldest dropped beyond them.
    pub fn set_max_beats(&mut self, n: usize) {
        self.cfg.max_beats = n;
    }

    /// The sound as it came in, from `from_s` for `len_s` seconds, while it
    /// is still held (the last few seconds).
    pub fn sound(&self, from_s: f64, len_s: f64) -> Option<&[f32]> {
        if from_s < 0.0 || len_s <= 0.0 {
            return None;
        }
        let i0 = (from_s * self.fs).round() as u64;
        let i1 = ((from_s + len_s) * self.fs).round() as u64;
        if i0 < self.buf0 || i1 > self.buf0 + self.buf.len() as u64 {
            return None;
        }
        Some(&self.buf[(i0 - self.buf0) as usize..(i1 - self.buf0) as usize])
    }

    /// Add audio. Returns true when a pass ran (the readings may have changed).
    pub fn push(&mut self, samples: &[f32]) -> bool {
        self.buf.extend_from_slice(samples);
        self.total += samples.len() as u64;
        let keep = (self.cfg.window_s * self.fs).ceil() as usize;
        let mut ran = false;
        if self.total >= self.next_pass {
            self.pass();
            self.next_pass = self.total + (self.cfg.update_s * self.fs).round().max(1.0) as u64;
            ran = true;
        }
        if self.buf.len() > 2 * keep {
            let cut = self.buf.len() - keep;
            self.buf.drain(..cut);
            self.buf0 += cut as u64;
        }
        ran
    }

    fn pass(&mut self) {
        // The audio is lent out for the pass, which changes the rest.
        let buf = std::mem::take(&mut self.buf);
        self.pass_on(&buf);
        self.buf = buf;
    }

    /// Keep this pass's peaks between `from` and `to` as candidates,
    /// numbered on from the last kept at the `beat` guessed now.
    fn keep_candidates(&mut self, local: &[Beat], off: f64, from: f64, to: f64, beat: f64) {
        for b in local {
            let t = b.time + off;
            if t < from || t >= to {
                continue;
            }
            let index = match self.candidates.back() {
                Some(last) => last.index + ((t - last.time) / beat).round().max(1.0) as i64,
                None => 0,
            };
            self.candidates.push_back(Beat {
                index,
                time: t,
                quality: b.quality,
            });
            if self.candidates.len() > CANDIDATES_KEPT {
                self.candidates.pop_front();
            }
        }
    }

    fn pass_on(&mut self, buf: &[f32]) {
        let fs = self.fs;
        let win = ((self.cfg.window_s * fs).ceil() as usize).min(buf.len());
        // Wait for most of a window before the first pass.
        if (win as f64) < 0.75 * self.cfg.window_s * fs {
            return;
        }
        let x = &buf[buf.len() - win..];
        let off = (self.buf0 + (buf.len() - win) as u64) as f64 / fs;
        let end = self.total as f64 / fs;
        let a = &self.cfg.analysis.clone();
        let env = envelope(x, fs, &a.envelope);

        let (bph, local) = match (&self.template, self.bph) {
            (Some(t), Some(bph)) => (bph, beats::detect_with_template(&env, fs, bph, t)),
            _ => {
                let bph = a.bph.unwrap_or_else(|| beats::guess_bph(&env, fs));
                let (b, t) = beats::detect(&env, fs, bph);
                let heard =
                    heard_over(snr(&env, fs, &b), self.cfg.min_snr) || clipped_out(x, fs, &b);
                if heard && b.len() >= 8 {
                    self.template = Some(t);
                    self.bph = Some(bph);
                    self.found_bph = a.bph.is_none();
                    self.logged_bph = Some(bph);
                }
                (bph, b)
            }
        };
        let beat = 3600.0 / bph as f64;
        // Beats near the end of the window may not be complete yet; they
        // settle on a later pass. The start of the window is skipped while
        // the filters settle.
        let guard = (0.6 * beat).max(0.25);
        let from = self.settled_to.max(off + 0.25);
        let to = end - guard;
        if to <= from {
            return;
        }
        let s = snr(&env, fs, &local);
        self.snr = s;
        let heard = heard_over(s, self.cfg.min_snr) || clipped_out(x, fs, &local);
        if !heard || self.template.is_none() {
            self.keep_candidates(&local, off, from, to, beat);
            self.quiet_passes += 1;
            // A long silence with a guessed rate: start over on the next
            // watch, which may beat at another rate.
            if self.quiet_passes as f64 * self.cfg.update_s >= 5.0
                && a.bph.is_none()
                && self.template.is_some()
            {
                self.template = None;
                self.bph = None;
                self.found_bph = false;
            }
            self.settled_to = to;
            self.next_amp_s = None;
            return;
        }
        // Number this pass's beats on from the last settled beat.
        let Some(anchor) = local.iter().position(|b| {
            let t = b.time + off;
            t >= from && t < to
        }) else {
            self.settled_to = to;
            return;
        };
        let resumed = self.quiet_passes > 0;
        self.quiet_passes = 0;
        let anchor_t = local[anchor].time + off;
        let (numbered, count_off) =
            beats::number_on(&local, anchor, anchor_t, self.beats.last(), beat);
        // Off the count by more than a quarter of a beat, or after a
        // stretch too quiet to follow: the watch's phase moved across a
        // hole (a dropout, a knock that cost a beat), so the count is a
        // guess. It is a jump.
        if self.beats.last().is_some() && (resumed || count_off.abs() > 0.25) {
            self.jump_at(anchor_t, false);
        }
        // One scan of the window finds whether anything clipped; only then
        // is each new beat looked at on its own.
        let any_clipped = Clipping::measure(x, fs, &[]).samples > 0;
        for b in &numbered[anchor..] {
            let t = b.time + off;
            if t >= to {
                break;
            }
            if let Some(last) = self.beats.last() {
                if t - last.time < 0.5 * beat || b.index <= last.index {
                    continue;
                }
            }
            self.beats.push(Beat { time: t, ..*b });
            if any_clipped && Clipping::measure(x, fs, std::slice::from_ref(b)).beats > 0 {
                self.clipped.push(t);
            }
            if b.is_good() {
                if let Some(w) = profile::snippet(&env, fs, b.time) {
                    let even = b.index.rem_euclid(2) == 0;
                    self.snippets.push_back((t, even, w.to_vec()));
                    self.sides.beat(t, b.index, b.quality, w);
                }
            }
        }
        // A step in the beats' timing within a quarter of a beat of the
        // count (a hole that cost close to a whole number of beats) is a
        // jump too, found as a whole file's reading finds one.
        let tail = beats::good_tail(&self.beats, JUMP_LOOK);
        let found: Vec<f64> = beats::jumps_among_good(tail, &[])
            .into_iter()
            .map(|p| tail[p].time)
            .collect();
        for t in found {
            self.jump_at(t, true);
        }
        if self.beats.len() > self.cfg.max_beats {
            let cut = self.beats.len() - self.cfg.max_beats;
            self.beats.drain(..cut);
            let first = self.beats[0].time;
            let gone = self.clipped.partition_point(|&t| t < first);
            self.clipped.drain(..gone);
        }

        // Amplitude on a grid of windows that have settled.
        let w = a.amplitude_window_s;
        let start = self
            .next_amp_s
            .unwrap_or(from)
            .max(off + beats::PRE_S + 0.01);
        let mut stop = start;
        while stop + w <= to {
            stop += w;
        }
        if stop > start {
            let wins = amplitude::windows_between(
                &env,
                fs,
                &numbered,
                2.0 * beat,
                w,
                start - off,
                stop - off,
                &a.amplitude,
            );
            self.amp.extend(wins.into_iter().map(|mut win| {
                win.start_s += off;
                win.end_s += off;
                win
            }));
            // A side's edge marked on the wrong sound, as in a whole file;
            // only the latest windows can change.
            let span = amplitude::OUTLIER_SPAN_S;
            let recent = self.amp.partition_point(|w| w.end_s < to - 2.0 * span);
            amplitude::reject_side_outliers(&mut self.amp[recent..], span);
            self.next_amp_s = Some(stop);
        } else if self.next_amp_s.is_none() {
            self.next_amp_s = Some(start);
        }
        // After the windows, so those of this pass follow a swap too.
        self.settle_sides();
        let span = self.profile_span();
        while self.snippets.front().is_some_and(|f| f.0 < to - span) {
            self.snippets.pop_front();
        }
        // Until the sides are told apart there is no Tick or Tock to show.
        let sided = self.sides.pending().is_none();
        let guides = self.profile_guides_at(to);
        self.profiles = [Side::A, Side::B].map(|side| {
            let even = side == Side::A;
            let w: Vec<&[f32]> = self
                .snippets
                .iter()
                .filter(|s| s.1 == even)
                .map(|s| s.2.as_slice())
                .collect();
            let guide = guides[usize::from(!even)];
            profile::from_snippets(&w, fs, side, 2.0 * beat, &a.amplitude, guide).filter(|_| sided)
        });
        self.settled_to = to;
    }

    /// Readings over the last `average_s` seconds of settled beats.
    pub fn reading(&self, average_s: f64) -> LiveReading {
        self.reading_at(self.settled_to, average_s)
    }

    /// The cuts in what has been heard: every break, as a hole of no
    /// width, and every masked stretch. Live time is true time.
    pub fn cuts(&self) -> Cuts<'static> {
        let breaks: Vec<Break> = self
            .breaks
            .iter()
            .map(|&audio_s| Break {
                audio_s,
                gap_s: 0.0,
            })
            .collect();
        Cuts::new(None, &breaks, &self.masked)
    }

    /// Readings over the `average_s` seconds before `end_s`.
    pub fn reading_at(&self, end_s: f64, average_s: f64) -> LiveReading {
        let window_from = end_s - average_s;
        let masked_s = crate::session::masked_within(&self.masked, window_from, end_s);
        // Breaks and masked stretches cut the readings: they start again
        // at the latest one's end, and inside a masked one there is nothing
        // to read. So does a pause between beats (clean::GAP_S).
        let holes = self.cuts();
        // The sound heard over the averaging time, since the start or the
        // latest break.
        let since = holes.stretch_from(end_s).max(0.0);
        let listened_s = (end_s - window_from.max(since).max(0.0)).max(0.0);
        // Only the latest unbroken run of beats.
        let cut = holes.run_cut(&self.beats, window_from, end_s);
        let from = window_from.max(cut);
        // The stretch's beats and windows as a saved reading takes them.
        let std::ops::Range { start: lo, end: hi } =
            crate::session::stretch_beats(&self.beats, from, end_s);
        let span_s = if lo < hi {
            end_s - self.beats[lo].time.max(from)
        } else {
            0.0
        };
        let clipped_beats = self.clipped.partition_point(|&t| t <= end_s)
            - self
                .clipped
                .partition_point(|&t| t < self.beats.get(lo).map_or(end_s, |b| b.time));
        let bph = self.bph.or(self.logged_bph);
        let fit = bph.and_then(|bph| timing::fit(&self.beats[lo..hi], bph));
        // `from` is at or after the latest cut, so no window reaches back
        // across one: a jump in a recording read whole falls inside one.
        let wins = crate::session::stretch_windows(&holes, &self.amp, from, end_s);
        // One unbroken run of beats: its beat error's sign does not turn.
        let figures = crate::session::window_figures(&wins, false);
        let amplitude_error_deg = crate::confidence::standard_error(
            &wins.iter().filter_map(|w| w.mean()).collect::<Vec<_>>(),
        );
        // Beats scattered like noise are no regular beat: no readings, as
        // `analyze` gives none. Under 10 s of sound it is not assessed.
        let jitter_us = fit.map(|f| f.jitter_us);
        let timing_scatter_fraction =
            bph.and_then(|b| crate::beats::timing_scatter_fraction(jitter_us, b));
        // Where the timing can't say, the beats' averaged sound, from the
        // beats kept for the profiles, says whether a watch is heard: the
        // same stretch as a saved reading judges ([`BeatLog::beat_sound`]).
        let beat_sound = match (bph, &self.beats[lo..hi]) {
            (Some(b), [first, .., last])
                if measure::beat_sound_needed(jitter_us, b, listened_s) =>
            {
                let snips: Vec<(f64, bool, &[f32])> = self
                    .snippets
                    .iter()
                    .filter(|s| s.0 >= first.time && s.0 <= last.time)
                    .map(|s| (s.0, s.1, s.2.as_slice()))
                    .collect();
                measure::stretch_beat_sound(
                    &snips,
                    first.time,
                    last.time,
                    self.fs,
                    2.0 * 3600.0 / b as f64,
                    &self.cfg.analysis.amplitude,
                )
            }
            _ => None,
        };
        let no_watch = match bph {
            Some(b) => measure::no_watch_reason(jitter_us, b, listened_s, beat_sound),
            None => measure::regularity_assessed(listened_s).then_some(NoWatchReason::Scatter),
        };
        let watch_heard = no_watch.is_none();
        let heard = |v: Option<f64>| v.filter(|_| watch_heard);
        // Figures that name a side wait until the sides are told apart.
        let sided = self.sides_known_at(end_s);
        let sided = |v: Option<f64>| heard(v).filter(|_| sided);
        let clipped_fraction = (hi > lo).then(|| clipped_beats as f64 / (hi - lo) as f64);
        // The rate's ± from 10 s windows over the run so far (the latest
        // unbroken run of beats, back to a break, up to RATE_RUN_S), scaled
        // to the averaging time by `measure`.
        let rate_windows = bph.map_or_else(Vec::new, |b| {
            let floor = from.min(end_s - RATE_RUN_S);
            holes.windows(
                &self.beats[..hi],
                b,
                floor.max(holes.run_cut(&self.beats, floor, end_s)),
                end_s,
                10.0,
                5.0,
                GAP_S,
            )
        });
        let assessment = measure::assess(
            &rate_windows,
            &wins,
            &measure::Context {
                watch_heard,
                no_watch_reason: no_watch.unwrap_or_default(),
                bph_heard: None,
                lift_known: self.cfg.analysis.lift_known,
                clock_known: self.clock_known || self.rate_clock.is_some(),
                beats: hi - lo,
                expected: bph.map_or(0.0, |b| b as f64 / 3600.0 * span_s),
                clipped_fraction: clipped_fraction.unwrap_or(0.0),
                // The time the beats stand for, each one beat long: a full
                // 10 s average is 10 s of beats, though its first and last
                // beats are one beat less apart.
                span_s: span_s + bph.map_or(0.0, |b| 3600.0 / b as f64),
                listened_s,
                breaks: 0,
                // Tick and Tock here are named across every such jump
                // before the reading.
                sides_by_count: self.by_count.iter().filter(|&&b| b <= end_s).count(),
                beat_error_as_size: false,
                average_s: Some(average_s),
            },
        );
        let mut caveats = assessment.caveats;
        if masked_s > 0.0 {
            measure::add(&mut caveats, Caveat::Masked { seconds: masked_s });
        }
        let whole = holes.whole(&self.beats, window_from, end_s, GAP_S);
        LiveReading {
            time_s: end_s,
            bph,
            rate_s_per_day: heard(fit.map(|f| f.rate_s_per_day)).map(|v| {
                self.rate_clock.as_ref().map_or(v, |c| {
                    crate::clockstore::correct_rate(v, Self::clock_ppm(c, from, end_s))
                })
            }),
            beat_error_ms: sided(fit.map(|f| f.beat_error_ms)),
            beat_error_unlock_ms: sided(figures.beat_error_unlock_ms),
            amplitude_deg: heard(figures.amplitude_deg),
            amplitude_error_deg: heard(amplitude_error_deg),
            amplitude_even_deg: sided(figures.amplitude_even_deg),
            amplitude_odd_deg: sided(figures.amplitude_odd_deg),
            jitter_us,
            timing_scatter_fraction,
            beats_used: fit.map_or(0, |f| f.beats_used),
            span_s,
            whole,
            snr: self.snr,
            clipped_beats,
            clipped_fraction,
            watch_heard,
            rate_error_s_per_day: assessment.confidence.and_then(|c| c.rate.se),
            beat_error_unlock_error_ms: sided(assessment.confidence.and_then(|c| c.beat_error.se)),
            caveats,
        }
    }
}

/// Whether most of the beats found clipped: a watch far too loud, whose
/// flattened peaks make the signal look weaker than it is. Its beats are
/// read (and the clipping said) rather than taken for silence.
fn clipped_out(x: &[f32], fs: f64, beats: &[Beat]) -> bool {
    beats.len() >= 8 && Clipping::measure(x, fs, beats).beat_fraction > 0.5
}

/// The signal figure of `beats` in `env`: the median beat peak over the
/// median of the envelope, as the readings give it. `None` where the
/// envelope's median is zero (digital silence, a muted input), which has
/// no figure.
pub fn signal_figure(env: &[f32], fs: f64, beats: &[Beat]) -> Option<f32> {
    snr(env, fs, beats)
}

/// Median beat peak over the median of the envelope, or `None` when that
/// median is zero. With no beats it is 0.
fn snr(env: &[f32], fs: f64, beats: &[Beat]) -> Option<f32> {
    let w = (0.001 * fs) as usize;
    let mut peaks: Vec<f32> = beats
        .iter()
        .filter_map(|b| {
            let c = (b.time * fs).round() as usize;
            let a = c.saturating_sub(w);
            let z = (c + w).min(env.len());
            (a < z).then(|| env[a..z].iter().copied().fold(0.0, f32::max))
        })
        .collect();
    if peaks.is_empty() {
        return Some(0.0);
    }
    let mut all: Vec<f32> = env.iter().step_by(7).copied().collect();
    let floor = median_f32(&mut all);
    (floor > 0.0).then(|| median_f32(&mut peaks) / floor)
}

/// Whether `snr` says a watch is heard: at least `min`. Digital silence,
/// with no figure, is not.
fn heard_over(snr: Option<f32>, min: f32) -> bool {
    snr.is_some_and(|s| s >= min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{generate, SynthConfig};

    /// A hole in the sound that costs `lost_s` of the watch's beating (a
    /// USB dropout, a knock that loses a beat) after `silence_s` of
    /// silence, at 20 s in, heard live from a watch whose Tock sounds
    /// unlike its Tick. Returns the analyzer and the reading at each pass,
    /// with the breaks known then.
    fn through_a_hole(
        bph: u32,
        silence_s: f64,
        lost_s: f64,
    ) -> (LiveAnalyzer, Vec<(LiveReading, usize)>) {
        let piece = |duration_s: f64, seed: u64| {
            generate(
                &SynthConfig {
                    bph,
                    duration_s,
                    rate_s_per_day: 10.0,
                    beat_error_ms: 0.6,
                    // A louder unlock on the Tock.
                    odd_gains: Some([0.9, 0.25, 1.0]),
                    seed,
                    ..Default::default()
                },
                |_| 270.0,
                |_| 0.0,
            )
        };
        // A watch beating on from where the first piece stopped (a whole
        // number of beats in), heard again after the silence and `lost_s`
        // more.
        let a = piece(HOLE_AT_S, 1);
        let b = piece(16.0, 2);
        let fs = a.sample_rate as f64;
        let mut samples = a.samples;
        samples.extend(std::iter::repeat_n(0.0, (silence_s * fs) as usize));
        samples.extend_from_slice(&b.samples[(lost_s * fs) as usize..]);
        let mut live = LiveAnalyzer::new(a.sample_rate, LiveConfig::default());
        let mut readings = Vec::new();
        for chunk in samples.chunks(960) {
            if live.push(chunk) {
                readings.push((live.reading(2.0), live.breaks().len()));
            }
        }
        (live, readings)
    }

    const HOLE_AT_S: f64 = 20.0;

    /// The sides keep their names across a hole: the beat error after it,
    /// by the beats' timing, by the unlock and on every amplitude window,
    /// matches the one before. A hole that moved the watch's phase is one
    /// jump, or two where passes straddling it call a second, and no
    /// amplitude window reaches across one. Readings that name a side are
    /// left out from a jump until its sides are checked.
    fn check_hole(bph: u32, silence_s: f64, lost_s: f64) {
        let (live, passes) = through_a_hole(bph, silence_s, lost_s);
        let readings: Vec<&LiveReading> = passes.iter().map(|p| &p.0).collect();
        let case = format!("{bph} bph, {silence_s} s silent, {lost_s} s lost");
        let back = HOLE_AT_S + silence_s;
        let side = |from: f64, to: f64| {
            let run: Vec<_> = live
                .beats()
                .iter()
                .filter(|b| b.time >= from && b.time < to)
                .copied()
                .collect();
            timing::beat_error_ms(&run).expect("beat error")
        };
        let (before, after) = (side(5.0, HOLE_AT_S - 1.0), side(back + 6.0, f64::INFINITY));
        assert!(
            before.abs() > 0.4 && (before - after).abs() < 0.1,
            "{case}: beat error {before} then {after}"
        );
        let unlock = |r: &&LiveReading| r.beat_error_unlock_ms;
        let was = readings
            .iter()
            .filter(|r| (10.0..HOLE_AT_S - 1.0).contains(&r.time_s))
            .find_map(unlock)
            .expect("an unlock beat error before");
        let late: Vec<f64> = readings
            .iter()
            .filter(|r| r.time_s >= back + 6.0)
            .filter_map(unlock)
            .collect();
        assert!(
            !late.is_empty() && late.iter().all(|v| v.signum() == was.signum()),
            "{case}: unlock beat error {was} then {late:?}"
        );
        let breaks = live.breaks();
        for w in live.amplitude_windows() {
            assert!(
                !breaks.iter().any(|&b| w.start_s < b && w.end_s > b),
                "{case}: window {}-{} across {breaks:?}",
                w.start_s,
                w.end_s
            );
            if w.start_s >= back + 6.0 {
                if let Some(v) = w.beat_error_unlock_ms {
                    assert_eq!(v.signum(), was.signum(), "{case}: window at {}", w.start_s);
                }
            }
        }
        // Found when the beats are found again, within a pass window and
        // the few beats a step in their timing needs. Where the phase didn't
        // move, the beats of passes straddling the silence can still step
        // as the silence leaves the window: at most one jump, checked like
        // any other.
        let recovery = back..back + LiveConfig::default().window_s + 1.0;
        let jumps = if lost_s == 0.0 { 0..=1 } else { 1..=2 };
        assert!(
            jumps.contains(&breaks.len()) && breaks.iter().all(|b| recovery.contains(b)),
            "{case}: {breaks:?}"
        );
        if breaks.is_empty() {
            return;
        }
        // Once the jump is found, the sides are not yet told apart: no
        // figure that names a side is read.
        let found = passes
            .iter()
            .find(|p| p.1 > 0)
            .map(|p| p.0.time_s)
            .expect("a pass after the jump");
        let early: Vec<&LiveReading> = passes
            .iter()
            .filter(|p| p.1 > 0 && p.0.time_s < found + 1.0)
            .map(|p| &p.0)
            .collect();
        for r in early {
            assert!(
                r.beat_error_ms.is_none()
                    && r.beat_error_unlock_ms.is_none()
                    && r.amplitude_even_deg.is_none()
                    && r.amplitude_odd_deg.is_none(),
                "{case}: at {} {r:?}",
                r.time_s
            );
        }
        assert!(
            live.sides_known_at(live.duration_s()),
            "{case}: sides never checked"
        );
    }

    #[test]
    fn a_short_silent_hole_keeps_the_sides() {
        for lost_s in [0.0, 0.04, 0.0625, 0.09] {
            check_hole(28_800, 1.0, lost_s);
        }
    }

    #[test]
    fn a_hole_of_about_a_beat_keeps_the_sides() {
        check_hole(28_800, 0.0, 0.11);
        check_hole(28_800, 0.0, 0.14);
    }

    #[test]
    fn a_silent_hole_of_about_a_beat_more_keeps_the_sides() {
        check_hole(28_800, 1.0, 0.11);
        check_hole(18_000, 0.0, 0.17);
    }

    fn feed(a: &mut LiveAnalyzer, x: &[f32]) {
        for block in x.chunks(480) {
            a.push(block);
        }
    }

    #[test]
    fn each_take_starts_positive_live_and_from_the_file() {
        // The same watch with its beat error either way round reads
        // positive from its first steady stretch on, listened to and from
        // the file alike; near no beat error the count names the sides.
        let read = |beat_error_ms: f64| {
            let audio = generate(
                &SynthConfig {
                    duration_s: 40.0,
                    beat_error_ms,
                    ..Default::default()
                },
                |_| 280.0,
                |_| 0.0,
            );
            let mut a = LiveAnalyzer::new(audio.sample_rate, LiveConfig::default());
            feed(&mut a, &audio.samples);
            let log = crate::stream::analyze_samples(
                &audio.samples,
                audio.sample_rate,
                &crate::stream::StreamConfig::default(),
            );
            let file: Vec<Beat> = log
                .beats
                .iter()
                .filter(|b| b.time >= 5.0)
                .copied()
                .collect();
            let live = a.reading(30.0);
            (
                live.beat_error_ms.unwrap(),
                live.beat_error_unlock_ms.unwrap(),
                crate::timing::beat_error_ms(&file).unwrap(),
            )
        };
        for be in [0.6, -0.6, 0.3, -0.3] {
            let (live, unlock, file) = read(be);
            for v in [live, unlock, file] {
                assert!((v - be.abs()).abs() < 0.1, "{be}: {live} {unlock} {file}");
            }
        }
        // Too small for a sign, both read the size with one sign.
        let (live, _, file) = read(-0.03);
        assert!(live.abs() < 0.1 && file.abs() < 0.1, "{live} {file}");
    }

    #[test]
    fn a_muted_input_has_no_signal_figure() {
        // A muted input: digital silence with a lone non-zero sample now
        // and then. The envelope's median is zero, so a beat peak over it
        // has no figure; it once read as an enormous one.
        let fs = 48_000;
        let mut x = vec![0.0f32; 10 * fs as usize];
        for k in (0..x.len()).step_by(6_000) {
            x[k] = 0.5;
        }
        let mut live = LiveAnalyzer::new(fs, LiveConfig::default());
        for chunk in x.chunks(4_800) {
            live.push(chunk);
        }
        let r = live.reading(10.0);
        assert_eq!(r.snr, None);
        assert_eq!(r.rate_s_per_day, None);
        let beats = [Beat {
            index: 0,
            time: 1.0,
            quality: 1.0,
        }];
        let mut env = vec![0.0f32; 2 * fs as usize];
        env[fs as usize] = 0.5;
        assert_eq!(signal_figure(&env, fs as f64, &beats), None);
        env.iter_mut().for_each(|v| *v += 0.01);
        assert_eq!(signal_figure(&env, fs as f64, &beats), Some(0.51 / 0.01));
    }

    #[test]
    fn one_silence_is_one_break() {
        // A 6 s silence while listening: the beats step more than once
        // before they are found again, but the whole take counts one
        // break. Two silences 20 s apart count two.
        let breaks = |silences: &[f64]| {
            let audio = generate(
                &SynthConfig {
                    duration_s: 70.0,
                    ..Default::default()
                },
                |_| 280.0,
                |_| 0.0,
            );
            let fs = audio.sample_rate as usize;
            let mut x = audio.samples;
            for &at in silences {
                let at = (at * fs as f64) as usize;
                x[at..at + 6 * fs].fill(0.0);
            }
            let mut a = LiveAnalyzer::new(audio.sample_rate, LiveConfig::default());
            feed(&mut a, &x);
            let log = BeatLog {
                sample_rate: a.sample_rate(),
                duration_s: a.duration_s(),
                bph: a.bph().unwrap(),
                lift_deg: a.config().analysis.amplitude.lift_deg,
                beats: a.beats().to_vec(),
                amplitude_windows: a.amplitude_windows().to_vec(),
                clipped_beats: Vec::new(),
                clipped_samples: 0,
                jumps: a.breaks().to_vec(),
                sided: Vec::new(),
                by_count: Vec::new(),
                snippets: Vec::new(),
            };
            let cuts: Vec<Break> = log
                .jumps
                .iter()
                .map(|&audio_s| Break {
                    audio_s,
                    gap_s: 0.0,
                })
                .collect();
            let m = crate::session::measure_masked(&log, None, &cuts, &[], 0.0, log.duration_s);
            let count = m.caveats.iter().find_map(|c| match c {
                Caveat::Breaks { count } => Some(*count),
                _ => None,
            });
            (count, log.jumps)
        };
        let (one, jumps) = breaks(&[16.0]);
        assert_eq!(one, Some(1), "{jumps:?}");
        let (two, jumps) = breaks(&[16.0, 42.0]);
        assert_eq!(two, Some(2), "{jumps:?}");
    }

    #[test]
    fn noise_logs_no_beats_but_keeps_its_peaks_to_show() {
        // Steady noise, the same each run.
        let mut seed = 12_345_u32;
        let x: Vec<f32> = (0..48_000 * 20)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1 << 24) as f32 * 0.02 - 0.01
            })
            .collect();
        let mut a = LiveAnalyzer::new(48_000, LiveConfig::default());
        feed(&mut a, &x);
        assert!(a.beats().is_empty(), "{} beats", a.beats().len());
        let c = a.candidates();
        assert!(c.len() > 20, "{} candidates", c.len());
        assert!(c
            .windows(2)
            .all(|w| w[1].time > w[0].time && w[1].index > w[0].index));
        // Kept no longer than the limit, and gone with the log.
        assert!(c.len() <= CANDIDATES_KEPT);
        a.set_bph(Some(28_800));
        assert!(a.candidates().is_empty());
    }

    // Supplied beat times only: no waveform synthesis or detector involved.
    fn viewer_phase_log(before_rate: f64, after_rate: f64) -> BeatLog {
        let before_period = 0.125 / (1.0 + before_rate / 86400.0);
        let after_period = 0.125 / (1.0 + after_rate / 86400.0);
        BeatLog {
            sample_rate: 48000,
            duration_s: 120.0,
            bph: 28800,
            lift_deg: 52.0,
            beats: (0..960)
                .map(|index| Beat {
                    index,
                    time: if index < 480 {
                        index as f64 * before_period
                    } else {
                        60.03 + (index - 480) as f64 * after_period
                    },
                    quality: if index == 500 { 0.1 } else { 1.0 },
                })
                .collect(),
            amplitude_windows: vec![],
            clipped_beats: vec![],
            clipped_samples: 0,
            jumps: vec![60.03],
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        }
    }

    /// A take's clock that drifts corrects each reading by its error over
    /// that reading, as `session::measure` reads the take on the clock's
    /// mapping, not by its one figure for the whole take.
    #[test]
    fn a_drifting_clock_corrects_each_reading_by_its_error_then() {
        // The card runs from 0 to 80 ppm slow over five hours, long enough
        // for its log's mapping to follow the drift: true time is
        // t + 40e-6 t² / 18000 of audio time t.
        let dur = 18_000.0;
        let true_of = |t: f64| t + 40e-6 * t * t / dur;
        let pairs: Vec<(f64, f64)> = (0..=300)
            .map(|k| {
                let t = 60.0 * k as f64;
                (t, 1_700_000_000.0 + true_of(t))
            })
            .collect();
        let fit = crate::clock::ClockFit::for_rates(&pairs).unwrap();
        assert!(fit.corrects() && fit.tracks_drift);
        // A watch keeping true time exactly: each beat at a true 0.125 s,
        // heard at the audio time that maps to it.
        let audio_of = |tt: f64| {
            let mut t = tt;
            for _ in 0..4 {
                t -= true_of(t) - tt;
            }
            t
        };
        let n = (dur / 0.125) as i64;
        let mut log = viewer_phase_log(0.0, 0.0);
        log.duration_s = dur;
        log.jumps.clear();
        log.beats = (0..n)
            .map(|index| Beat {
                index,
                time: audio_of(index as f64 * 0.125),
                quality: 1.0,
            })
            .collect();
        // What `session::measure` reads over audio `from..to`: the beats
        // put on the clock's mapping, then fitted.
        let on_clock = |from: f64, to: f64| {
            let beats: Vec<Beat> = log
                .beats
                .iter()
                .filter(|b| b.time >= from && b.time <= to)
                .map(|b| Beat {
                    time: fit.map(b.time),
                    ..*b
                })
                .collect();
            crate::timing::fit(&beats, 28800).unwrap().rate_s_per_day
        };
        let mut a = LiveAnalyzer::from_log(log.clone(), LiveConfig::default());
        a.set_rate_clock(Some(&fit));
        for end in [1_000.0, 9_000.0, 17_000.0] {
            let card = a.reading_at(end, 60.0).rate_s_per_day.expect("a rate");
            let session = on_clock(end - 60.0, end);
            assert!((card - session).abs() < 0.05, "{end}: {card} {session}");
        }
        // Late in the take the card is about 76 ppm slow, against 40 over
        // the whole take: the take's one figure would be 3 s/d out there.
        let heard = LiveAnalyzer::from_log(log.clone(), LiveConfig::default())
            .reading_at(17_000.0, 60.0)
            .rate_s_per_day
            .unwrap();
        let one = crate::clockstore::correct_rate(heard, fit.ppm);
        assert!((one - on_clock(16_940.0, 17_000.0)).abs() > 1.0, "{one}");
    }

    /// A recording read whole keeps its jumps as breaks: no reading after
    /// one uses an amplitude window that started before it.
    #[test]
    fn a_window_across_a_jump_in_a_read_recording_is_left_out() {
        let mut log = viewer_phase_log(10.0, 10.0);
        let win = |a: f64, deg: f64| AmplitudeWindow {
            start_s: a,
            end_s: a + 2.0,
            even_deg: Some(deg),
            odd_deg: Some(deg),
            beat_error_ms: None,
            beat_error_unlock_ms: None,
            even_unlock_ms: None,
            odd_unlock_ms: None,
            even_drop_ms: None,
            odd_drop_ms: None,
        };
        // The jump is at 60.03 s: the window from 60 s reaches across it.
        log.amplitude_windows = vec![win(58.0, 280.0), win(60.0, 350.0), win(62.0, 280.0)];
        let a = LiveAnalyzer::from_log(log, LiveConfig::default());
        assert_eq!(a.reading_at(62.5, 2.0).amplitude_deg, None);
        assert_eq!(a.reading_at(64.0, 4.0).amplitude_deg, Some(280.0));
        assert_eq!(a.reading_at(60.0, 2.0).amplitude_deg, Some(280.0));
    }

    /// Steady beats for `seconds`: a watch whose Tock sounds unlike its
    /// Tick, and a live analyzer fed the first `fed` seconds of them.
    fn steady_live(seconds: f64, fed: f64) -> (LiveAnalyzer, Vec<f32>) {
        let audio = generate(
            &SynthConfig {
                duration_s: seconds,
                beat_error_ms: 0.6,
                odd_gains: Some([0.9, 0.25, 1.0]),
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        );
        let mut live = LiveAnalyzer::new(audio.sample_rate, LiveConfig::default());
        let k = (fed * audio.sample_rate as f64) as usize;
        feed(&mut live, &audio.samples[..k]);
        (live, audio.samples[k..].to_vec())
    }

    /// A jump found by the beats' timing after a new position, from before
    /// it, starts no amplitude window that reaches back across the new
    /// position's break.
    #[test]
    fn a_late_jump_after_a_new_position_keeps_windows_off_its_break() {
        let (mut live, rest) = steady_live(40.0, 30.0);
        live.new_stretch();
        let stretch = live.duration_s();
        // Off the 2-second grid: a fresh grid from the jump would put a
        // window across the break.
        live.jump_at(stretch - 1.5, true);
        assert_eq!(live.next_amp_s, Some(live.settled_to));
        feed(&mut live, &rest);
        let breaks = live.breaks().to_vec();
        assert!(breaks.contains(&stretch), "{breaks:?}");
        assert!(live
            .amplitude_windows()
            .iter()
            .any(|w| w.start_s >= stretch));
        for w in live.amplitude_windows() {
            assert!(
                !breaks.iter().any(|&b| w.start_s < b && w.end_s > b),
                "window {}-{} across {breaks:?}",
                w.start_s,
                w.end_s
            );
        }
    }

    /// The beats of a stretch between two jumps too close to check join
    /// neither side's Tick Tock profile, whichever of the two is found
    /// first.
    #[test]
    fn an_unchecked_stretch_joins_no_profile() {
        for later_first in [false, true] {
            let (mut live, rest) = steady_live(45.0, 30.0);
            live.set_profile_span(MAX_PROFILE_S);
            let now = live.duration_s();
            let (t1, t2) = (now - 2.0, now - 1.0);
            let between = |live: &LiveAnalyzer| {
                live.snippets
                    .iter()
                    .filter(|s| t1 <= s.0 && s.0 < t2)
                    .count()
            };
            assert!(between(&live) > 0);
            let (a, b) = if later_first { (t2, t1) } else { (t1, t2) };
            live.jump_at(a, false);
            assert!(
                live.sides.pending().is_some(),
                "a check from the first jump"
            );
            live.jump_at(b, false);
            assert!(!live.sides_known_at(t1 + 0.5));
            assert_eq!(between(&live), 0, "kept for the profiles");
            // Once the sides are told apart again, the profiles are made of
            // the kept sounds alone.
            feed(&mut live, &rest);
            assert!(live.sides.pending().is_none(), "sides settled");
            assert_eq!(between(&live), 0);
            let used: usize = live.tick_profiles().iter().flatten().map(|p| p.beats).sum();
            assert!(used > 0);
            assert_eq!(used, live.snippets.len());
        }
    }

    #[test]
    fn a_masked_stretch_is_cut_like_a_break() {
        // 10 s/d before 60 s, 200 s/d after: a mask over the start of the
        // second run leaves readings after it on that run alone, and none
        // inside it.
        let mut log = viewer_phase_log(10.0, 200.0);
        log.jumps.clear();
        let mut a = LiveAnalyzer::from_log(log, LiveConfig::default());
        let before = a.reading_at(90.0, 60.0);
        a.set_masked(&[(70.0, 80.0), (55.0, 62.0)]);
        assert_eq!(a.masked(), [(55.0, 62.0), (70.0, 80.0)]);
        let inside = a.reading_at(75.0, 30.0);
        assert_eq!(inside.rate_s_per_day, None);
        assert_eq!(inside.beats_used, 0);
        let after = a.reading_at(90.0, 60.0);
        assert!((after.span_s - 10.0).abs() < 0.2, "{}", after.span_s);
        assert!(after
            .rate_s_per_day
            .is_some_and(|r| (r - 200.0).abs() < 5.0));
        assert!(after.caveats.contains(&Caveat::Masked { seconds: 17.0 }));
        assert!(!before
            .caveats
            .iter()
            .any(|c| matches!(c, Caveat::Masked { .. })));
        a.set_masked(&[]);
        assert_eq!(a.reading_at(90.0, 60.0).span_s, before.span_s);
    }

    /// A reading is whole only when its averaging time reaches into no
    /// cut and each 2 s of it holds a beat.
    #[test]
    fn a_reading_is_whole_only_clear_of_every_cut() {
        let mut log = viewer_phase_log(10.0, 10.0);
        log.jumps.clear();
        // No beats from 40 to 43 s.
        log.beats.retain(|b| !(40.0..43.0).contains(&b.time));
        let mut a = LiveAnalyzer::from_log(log, LiveConfig::default());
        assert!(a.reading_at(30.0, 20.0).whole);
        assert!(!a.reading_at(50.0, 20.0).whole);
        assert!(a.reading_at(53.0, 10.0).whole);
        a.set_masked(&[(70.0, 80.0)]);
        assert!(!a.reading_at(90.0, 20.0).whole);
        assert!(!a.reading_at(89.0, 10.0).whole);
        assert!(a.reading_at(90.0, 10.0).whole);
        a.set_breaks(vec![95.0]);
        assert!(!a.reading_at(100.0, 10.0).whole);
        assert!(a.reading_at(105.0, 10.0).whole);
    }

    #[test]
    fn viewer_known_phase_jump_does_not_become_rate_or_irregularity() {
        let log = viewer_phase_log(0.0, 0.0);
        let original: Vec<_> = log
            .beats
            .iter()
            .map(|b| (b.index, b.time, b.quality))
            .collect();
        let a = LiveAnalyzer::from_log(log, LiveConfig::default());
        let crossing = a.reading_at(90.0, 60.0);
        println!(
            "known phase: cuts={:?}, span={}, heard={}, rate={:?}, jitter={:?}",
            a.breaks(),
            crossing.span_s,
            crossing.watch_heard,
            crossing.rate_s_per_day,
            crossing.jitter_us
        );
        assert_eq!(a.breaks(), &[60.03]);
        assert_eq!(
            a.beats()
                .iter()
                .map(|b| (b.index, b.time, b.quality))
                .collect::<Vec<_>>(),
            original
        );
        let crossing = a.reading_at(90.0, 60.0);
        assert!(crossing.span_s <= 90.0 - 60.03 + 1e-9, "{crossing:?}");
        assert!(
            crossing.watch_heard,
            "a supplied phase cut is not an irregular watch"
        );
        assert!(
            crossing.rate_s_per_day.unwrap().abs() < 0.001,
            "{crossing:?}"
        );
        assert!(crossing.jitter_us.unwrap() < 0.001);
        assert!(a.reading_at(45.0, 20.0).rate_s_per_day.unwrap().abs() < 0.001);
        assert!(a.reading_at(105.0, 10.0).rate_s_per_day.unwrap().abs() < 0.001);
        // We preserve supplied jumps; we do not infer one from a phase change.
        let mut unmarked = viewer_phase_log(0.0, 0.0);
        unmarked.jumps.clear();
        let unmarked = LiveAnalyzer::from_log(unmarked, LiveConfig::default());
        assert!(unmarked.breaks().is_empty());
        assert!(unmarked.reading_at(90.0, 60.0).span_s > 50.0);
    }

    #[test]
    fn an_opened_log_says_where_its_sides_were_named_by_count() {
        let mut log = viewer_phase_log(0.0, 0.0);
        log.by_count = log.jumps.clone();
        let a = LiveAnalyzer::from_log(log, LiveConfig::default());
        let by_count = |end_s: f64| {
            a.reading_at(end_s, 20.0)
                .caveats
                .iter()
                .find_map(|c| match c {
                    Caveat::SidesByCount { count } => Some(*count),
                    _ => None,
                })
        };
        // Before the jump the sides are as heard; after it, by the count.
        assert_eq!(by_count(45.0), None);
        assert_eq!(by_count(90.0), Some(1));
        // Without the log's by-count jumps, nothing is said.
        let plain = LiveAnalyzer::from_log(viewer_phase_log(0.0, 0.0), LiveConfig::default());
        assert!(!plain
            .reading_at(90.0, 20.0)
            .caveats
            .iter()
            .any(|c| matches!(c, Caveat::SidesByCount { .. })));
    }

    #[test]
    fn viewer_latest_piece_is_distinct_from_the_retained_whole_recording() {
        let log = viewer_phase_log(10.0, -20.0);
        let whole = timing::fit_pieces(&log.beats, log.bph, &log.jumps).unwrap();
        let a = LiveAnalyzer::from_log(log, LiveConfig::default());
        let early = a.reading_at(45.0, 30.0);
        let latest = a.reading_at(90.0, 60.0);
        println!(
            "unequal pieces: early={:?}, latest={:?}, whole={}",
            early.rate_s_per_day, latest.rate_s_per_day, whole.rate_s_per_day
        );
        assert!((early.rate_s_per_day.unwrap() - 10.0).abs() < 0.001);
        assert!(
            (latest.rate_s_per_day.unwrap() + 20.0).abs() < 0.001,
            "{latest:?}"
        );
        assert!(latest.span_s < 30.0);
        assert_eq!(
            a.beats().len(),
            960,
            "all pieces remain available for historical views"
        );
        assert!(whole.beats_used > latest.beats_used);
        assert!(
            (whole.rate_s_per_day + 5.0).abs() < 0.05,
            "whole fit remains a different aggregation: {whole:?}"
        );
    }

    #[test]
    fn viewer_external_cuts_cannot_replace_intrinsic_jumps() {
        let mut log = viewer_phase_log(0.0, 0.0);
        log.jumps = vec![100.0, 60.03, 60.03];
        let mut a = LiveAnalyzer::from_log(log, LiveConfig::default());
        assert_eq!(a.breaks(), &[60.03, 100.0]);
        a.set_breaks(vec![90.0, 30.0, 60.0, 40.0, 60.03, 100.0]);
        assert_eq!(a.breaks(), &[30.0, 40.0, 60.0, 60.03, 90.0, 100.0]);
        a.set_breaks(vec![80.0]);
        assert_eq!(
            a.breaks(),
            &[60.03, 80.0, 100.0],
            "only external cuts are replaced"
        );
        a.set_breaks(vec![]);
        assert_eq!(a.breaks(), &[60.03, 100.0]);
        a.restart();
        assert!(a.beats().is_empty());
        assert!(a.breaks().is_empty());
        a.set_breaks(vec![110.0]);
        assert_eq!(
            a.breaks(),
            &[110.0],
            "discarded history cannot reintroduce old jumps"
        );
    }

    /// Room noise with no watch, seeded so the test repeats.
    fn empty_stand(secs: usize) -> Vec<f32> {
        let mut x = 0x2545_f491_4f6c_dd1du64;
        (0..48_000 * secs)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                ((x >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.02
            })
            .collect()
    }

    /// An empty stand, heard live or read back from its beat log over a
    /// 10 s average, gives no readings, and says no regular beat was found.
    #[test]
    fn an_empty_stand_reads_nothing() {
        let x = empty_stand(60);
        let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
        feed(&mut a, &x);
        let r = a.reading(10.0);
        assert!(!r.watch_heard);
        assert!(
            r.rate_s_per_day.is_none() && r.beat_error_ms.is_none(),
            "{r:?}"
        );
        assert!(
            r.amplitude_deg.is_none() && r.beat_error_unlock_ms.is_none(),
            "{r:?}"
        );
        assert!(
            r.rate_error_s_per_day.is_none()
                && r.caveats.first()
                    == Some(&Caveat::NoWatch {
                        reason: NoWatchReason::Scatter
                    })
        );
    }

    /// The readings carry what `analyze` gives a recording: a short
    /// stretch is said, the rate's ± waits for enough of a run and then
    /// follows the averaging time, and unknowns are caveats.
    #[test]
    fn readings_carry_their_errors_and_caveats() {
        let cfg = SynthConfig {
            duration_s: 90.0,
            rate_s_per_day: 5.0,
            snr_db: 30.0,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 280.0, |_| 0.0);
        let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
        feed(&mut a, &audio.samples);
        let early = a.reading_at(8.0, 10.0);
        assert!(
            early
                .caveats
                .iter()
                .any(|c| matches!(c, Caveat::ShortSpan { .. })),
            "{:?}",
            early.caveats
        );
        assert!(early.rate_error_s_per_day.is_none(), "{early:?}");
        // Under 10 s of sound regularity is not assessed: the readings
        // are shown, not withheld.
        assert!(early.watch_heard, "{early:?}");
        assert!(
            early.rate_s_per_day.is_some() && early.amplitude_deg.is_some(),
            "{early:?}"
        );
        let late = a.reading(10.0);
        assert!(
            late.caveats.contains(&Caveat::LiftUnknown),
            "{:?}",
            late.caveats
        );
        assert!(
            late.caveats.contains(&Caveat::ClockUnknown),
            "{:?}",
            late.caveats
        );
        assert!(late.beat_error_unlock_error_ms.is_some(), "{late:?}");
        let ten = late.rate_error_s_per_day.expect("rate ±");
        let sixty = a.reading(60.0).rate_error_s_per_day.unwrap();
        assert!(sixty < ten, "{sixty} against {ten}");
        a.set_known(true, true);
        let known = a.reading(10.0).caveats;
        assert!(known.is_empty(), "{known:?}");
    }

    /// Driven five times past full scale, the beats are still read, and
    /// the clipping is said.
    #[test]
    fn heavy_clipping_still_reads() {
        let cfg = SynthConfig {
            duration_s: 60.0,
            rate_s_per_day: 10.0,
            beat_error_ms: 0.6,
            snr_db: 24.0,
            seed: 13,
            ..Default::default()
        };
        let mut audio = generate(&cfg, |t| 270.0 + 10.0 * (t / 60.0).sin(), |_| 0.0);
        let peak = audio.samples.iter().fold(0f32, |m, v| m.max(v.abs()));
        for v in &mut audio.samples {
            *v = (*v * 5.0 / peak).clamp(-1.0, 1.0);
        }
        let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
        feed(&mut a, &audio.samples);
        let r = a.reading(10.0);
        assert!(
            r.beats_used > 0 && r.watch_heard && r.clipped_fraction.is_some_and(|f| f > 0.5),
            "{r:?}"
        );
    }

    #[test]
    fn a_steady_watch_keeps_both_sides_amplitude() {
        let cfg = SynthConfig {
            duration_s: 40.0,
            ..Default::default()
        };
        let audio = generate(&cfg, |t| 270.0 + 10.0 * (t / 6.0).sin(), |_| 0.0);
        let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
        feed(&mut a, &audio.samples);
        let w = a.amplitude_windows();
        assert!(w.len() > 15);
        assert!(w
            .iter()
            .all(|w| w.even_deg.is_some() && w.odd_deg.is_some()));
    }

    #[test]
    fn a_new_stretch_keeps_the_beats_and_breaks_the_readings() {
        let cfg = SynthConfig {
            duration_s: 40.0,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 280.0, |t| if t < 20.0 { 0.0 } else { 30.0 });
        let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
        let half = audio.samples.len() / 2;
        feed(&mut a, &audio.samples[..half]);
        let first = a.reading(10.0).rate_s_per_day.unwrap();
        let before = a.beats().len();
        a.new_stretch();
        assert_eq!(a.beats().len(), before, "the beats are kept");
        assert!(a.tick_profiles().iter().all(Option::is_none));
        assert_eq!(a.breaks(), &[20.0]);
        feed(&mut a, &audio.samples[half..]);
        assert!(a.beats().len() > before);
        let r = a.reading(30.0);
        assert!(r.span_s <= 20.0 + 1e-6, "{}", r.span_s);
        let rate = r.rate_s_per_day.unwrap();
        assert!(
            (rate - first - 30.0).abs() < 3.0,
            "only the new stretch: {first} then {rate}"
        );
        assert!(a.tick_profiles().iter().any(Option::is_some));
    }

    /// Setting the rate Auto already found (a calibre picked while
    /// listening) keeps the beats heard so far, as does going back to
    /// Auto; another rate starts the beat log again.
    #[test]
    fn the_rate_already_found_keeps_the_beats() {
        let cfg = SynthConfig {
            duration_s: 20.0,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 280.0, |_| 0.0);
        let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
        feed(&mut a, &audio.samples);
        assert_eq!(a.bph(), Some(28_800));
        let heard = a.beats().len();
        assert!(heard > 100, "{heard}");
        assert!(a.set_bph(Some(28_800)));
        assert_eq!(a.beats().len(), heard);
        assert!(a.reading(10.0).rate_s_per_day.is_some());
        assert!(a.set_bph(None));
        assert_eq!(a.beats().len(), heard);
        assert_eq!(a.bph(), Some(28_800));
        assert!(!a.set_bph(Some(21_600)));
        assert!(a.beats().is_empty());
        // Auto after a rate set by hand listens afresh and finds 28800.
        feed(&mut a, &audio.samples);
        assert!(!a.set_bph(None));
        assert!(a.beats().is_empty());
        feed(&mut a, &audio.samples);
        assert_eq!(a.bph(), Some(28_800));
        assert!(a.beats().len() > 100, "{}", a.beats().len());
    }

    #[test]
    fn clipped_beats_are_counted_as_they_settle() {
        let cfg = SynthConfig {
            duration_s: 20.0,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 280.0, |_| 0.0);
        let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
        feed(&mut a, &audio.samples);
        let clean = a.reading(10.0);
        assert_eq!(clean.clipped_beats, 0);
        assert_eq!(clean.clipped_fraction, Some(0.0));

        // The next 20 s 12 dB too hot, clipped at full scale.
        let peak = audio.samples.iter().fold(0f32, |m, v| m.max(v.abs()));
        let hot: Vec<f32> = audio
            .samples
            .iter()
            .map(|v| (v * 4.0 / peak).clamp(-1.0, 1.0))
            .collect();
        feed(&mut a, &hot);
        let r = a.reading(10.0);
        assert!(r.clipped_fraction.unwrap() > 0.5, "{r:?}");
        // A reading back in the clean stretch has no clipped beats.
        assert_eq!(a.reading_at(15.0, 10.0).clipped_fraction, Some(0.0));

        // Made again from its beat log (a recording analysed at once), it
        // counts the same.
        let log = BeatLog {
            sample_rate: 48000,
            duration_s: a.duration_s(),
            bph: a.bph().unwrap(),
            lift_deg: a.config().analysis.amplitude.lift_deg,
            beats: a.beats().to_vec(),
            amplitude_windows: a.amp.clone(),
            clipped_beats: a.clipped_beats().to_vec(),
            clipped_samples: 0,
            jumps: Vec::new(),
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        };
        let b = LiveAnalyzer::from_log(log, LiveConfig::default());
        let end = b.duration_s();
        let again = b.reading_at(end, 10.0);
        assert!(again.clipped_fraction.unwrap() > 0.5, "{again:?}");
        assert_eq!(again.clipped_beats, a.reading_at(end, 10.0).clipped_beats);
        assert_eq!(b.reading_at(15.0, 10.0).clipped_fraction, Some(0.0));
    }

    #[test]
    fn readings_cover_only_the_run_since_a_pause() {
        let cfg = SynthConfig {
            duration_s: 20.0,
            rate_s_per_day: 10.0,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 270.0, |_| 0.0);
        let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
        feed(&mut a, &audio.samples);
        let before = a.reading(60.0);
        assert!((before.span_s - 20.0).abs() < 3.0, "span {}", before.span_s);
        a.skip(30.0);
        feed(&mut a, &audio.samples);
        let r = a.reading(60.0);
        // Only the 20 s since the pause, not the beats before it.
        assert!(r.span_s < 20.0 && r.span_s > 10.0, "span {}", r.span_s);
        let rate = r.rate_s_per_day.expect("rate");
        assert!((rate - 10.0).abs() < 3.0, "rate {rate}");
        // The beats before the pause are kept, a gap behind the new ones.
        assert!(a.beats().iter().any(|b| b.time < 20.0));
        assert!(a.beats().iter().any(|b| b.time > 50.0));
        assert!(a.duration_s() > 69.0);
    }

    #[test]
    fn profiles_cover_more_than_one_pass() {
        let cfg = SynthConfig {
            duration_s: 20.0,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 270.0, |_| 0.0);
        let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
        a.set_profile_span(10.0);
        feed(&mut a, &audio.samples);
        for p in a.tick_profiles() {
            let p = p.as_ref().expect("profile");
            // 10 s is 40 beats a side; the pass guard trims a couple.
            assert!((36..=41).contains(&p.beats), "{} beats", p.beats);
            let amp = p.amplitude_deg.expect("amplitude");
            assert!((amp - 270.0).abs() < 10.0, "amplitude {amp}");
        }
        a.set_profile_span(2.0);
        feed(&mut a, &audio.samples[..48000]);
        for p in a.tick_profiles() {
            let n = p.as_ref().expect("profile").beats;
            assert!(n <= 9, "{n} beats after shortening");
        }
    }

    #[test]
    fn follows_a_synthetic_watch() {
        let cfg = SynthConfig {
            rate_s_per_day: 12.0,
            beat_error_ms: 0.5,
            duration_s: 30.0,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 270.0, |_| 0.0);
        let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
        feed(&mut a, &audio.samples);
        let r = a.reading(10.0);
        assert_eq!(r.bph, Some(28800));
        let rate = r.rate_s_per_day.expect("rate");
        assert!((rate - 12.0).abs() < 1.0, "rate {rate}");
        let be = r.beat_error_ms.expect("beat error");
        assert!((be.abs() - 0.5).abs() < 0.05, "beat error {be}");
        let amp = r.amplitude_deg.expect("amplitude");
        assert!((amp - 270.0).abs() < 10.0, "amplitude {amp}");
        // A steady synthetic amplitude scatters little between windows.
        let pm = r.amplitude_error_deg.expect("amplitude ±");
        assert!(pm < 1.0, "amplitude ± {pm}");
        // Both sides' sounds, with the marks behind the amplitude, over the
        // default 3 s (12 beats a side at 28,800 bph).
        for p in a.tick_profiles() {
            let p = p.as_ref().expect("profile");
            assert!((10..=13).contains(&p.beats), "{} beats", p.beats);
            let pa = p.amplitude_deg.expect("profile amplitude");
            assert!((pa - 270.0).abs() < 15.0, "profile amplitude {pa}");
        }
        // About 8 beats a second from the first pass on, none counted twice.
        let n = a.beats().len();
        assert!((200..=240).contains(&n), "{n} beats");
        assert!(a.beats().windows(2).all(|w| w[1].index > w[0].index));
    }

    #[test]
    fn agrees_with_the_whole_recording_fit() {
        let cfg = SynthConfig {
            rate_s_per_day: -20.0,
            duration_s: 20.0,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 250.0, |_| 0.0);
        let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
        feed(&mut a, &audio.samples);
        let whole = crate::analyze(&audio, &AnalysisConfig::default());
        // The same beats, numbered the same way. The two templates come
        // from different stretches of audio, so their reference points may
        // differ by a constant; beat to beat the times must agree.
        let live = a.beats();
        let mut shift = None;
        let mut dt = Vec::new();
        for b in live {
            if let Some(w) = whole.beats.iter().find(|w| (w.time - b.time).abs() < 0.01) {
                let s = *shift.get_or_insert(w.index - b.index);
                assert_eq!(w.index - b.index, s, "beat numbering slipped");
                dt.push(w.time - b.time);
            }
        }
        assert!(dt.len() as f64 > 0.95 * live.len() as f64);
        let mean = dt.iter().sum::<f64>() / dt.len() as f64;
        let rms = (dt.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / dt.len() as f64).sqrt();
        assert!(
            mean.abs() < 100e-6 && rms < 10e-6,
            "offset {mean}, rms {rms}"
        );
    }

    #[test]
    fn silence_gives_no_readings_and_lift_rescales() {
        let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
        let mut rng = crate::synth::Rng::new(3);
        let noise: Vec<f32> = (0..48000 * 8).map(|_| 0.01 * rng.normal() as f32).collect();
        feed(&mut a, &noise);
        let r = a.reading(10.0);
        assert!(r.rate_s_per_day.is_none());
        assert!(a.beats().is_empty());

        let audio = generate(&SynthConfig::default(), |_| 260.0, |_| 0.0);
        feed(&mut a, &audio.samples);
        let before = a.reading(10.0).amplitude_deg.expect("amplitude");
        a.set_lift(26.0);
        let after = a.reading(10.0).amplitude_deg.expect("amplitude");
        assert!((after - before / 2.0).abs() < 1e-9);
    }

    /// Over the same stretch of the same beats, a live reading and a saved
    /// one (`session`) read the same figures: the same beats, the same
    /// amplitude windows (none reaching back before the stretch) and the
    /// same medians (#632).
    #[test]
    fn live_and_saved_read_one_stretch_alike() {
        let cfg = SynthConfig {
            duration_s: 30.0,
            beat_error_ms: 0.5,
            rate_s_per_day: 8.0,
            ..Default::default()
        };
        // An amplitude that climbs, so a window more or less moves the
        // median.
        let audio = generate(&cfg, |t| 240.0 + 2.0 * t, |_| 0.0);
        let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
        feed(&mut a, &audio.samples);
        let log = BeatLog {
            sample_rate: 48000,
            duration_s: a.duration_s(),
            bph: a.bph().unwrap(),
            lift_deg: a.config().analysis.amplitude.lift_deg,
            beats: a.beats().to_vec(),
            amplitude_windows: a.amp.clone(),
            clipped_beats: a.clipped_beats().to_vec(),
            clipped_samples: 0,
            jumps: Vec::new(),
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        };
        let end = a.settled_to;
        // A stretch that starts partway through an amplitude window.
        for average_s in [7.3, 9.1] {
            let live = a.reading_at(end, average_s);
            let saved = crate::session::measure_masked(&log, None, &[], &[], end - average_s, end);
            assert!(live.amplitude_deg.is_some(), "{live:?}");
            assert_eq!(live.amplitude_deg, saved.amplitude_deg, "{average_s}");
            assert_eq!(live.amplitude_even_deg, saved.amplitude_even_deg);
            assert_eq!(live.amplitude_odd_deg, saved.amplitude_odd_deg);
            assert_eq!(live.beat_error_unlock_ms, saved.beat_error_unlock_ms);
            assert_eq!(live.beat_error_ms, saved.beat_error_ms);
        }
    }
}
