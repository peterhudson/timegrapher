//! Live analysis: audio arrives a block at a time and the readings follow.
//!
//! Every half second the last few seconds of audio are run through the
//! same envelope, template and beat tracker as a recording, and the beats
//! that have settled since the previous pass are added to a running beat
//! log. The template is built from the first seconds of signal, then again
//! from the first 47 s as a file's reading builds it, which are read again
//! (#1327); from then on the beat's reference point stays put. The
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
use crate::timing::WindowFit;
use std::collections::VecDeque;

pub use crate::clean::GAP_S;

/// Longest stretch the tick and tock profiles can cover, seconds.
pub const MAX_PROFILE_S: f64 = 60.0;

/// Seconds of settled beats the tick and tock profiles cover when asked
/// for `seconds`: 0.5 s at the least, [`MAX_PROFILE_S`] at the most.
pub fn profile_span_s(seconds: f64) -> f64 {
    seconds.clamp(0.5, MAX_PROFILE_S)
}

/// How far back the rate's ± looks for its 10 s windows, seconds: the
/// latest unbroken run of beats, up to this long. The averaging time
/// holds too few windows to judge the rate on its own.
pub const RATE_RUN_S: f64 = 300.0;

/// The least time back from a reading's end, within the sound heard since
/// the latest break, whose 10 s windows judge whether a watch is heard
/// (`session::scatter_jitter_us`): enough for one whole window under a
/// 10 s average.
pub const SCATTER_LOOKBACK_S: f64 = 15.0;

/// The rate windows' length and step, as every reading lays them
/// ([`measure::RATE_WINDOW_S`], [`measure::RATE_WINDOW_STEP_S`]).
const RATE_W: f64 = measure::RATE_WINDOW_S;
const RATE_STEP: f64 = measure::RATE_WINDOW_STEP_S;

/// How far back the take's rate windows are first looked for when a run
/// holds too few for the rate's ±, s: 120 s, shorter than a saved
/// reading's [`crate::session::RATE_FIRST_REACH_S`] because the run's own
/// windows are counted with the borrowed ones and most often hold nearly
/// all of them; the reach doubles from there (`session::nearest_windows`).
const RATE_FIRST_REACH_S: f64 = measure::RATE_SE_MIN_S * RATE_FIRST_REACH_SHARE;
/// [`RATE_FIRST_REACH_S`] as a share of the beats the rate's ± needs.
const RATE_FIRST_REACH_SHARE: f64 = 0.4;

use crate::stream::BeatLog;
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

/// [`LiveConfig::min_snr`]'s default: beats louder than this many times
/// the envelope's median count as signal.
pub const MIN_SNR: f32 = 4.0;

impl Default for LiveConfig {
    fn default() -> Self {
        LiveConfig {
            analysis: AnalysisConfig::default(),
            window_s: 4.0,
            update_s: 0.5,
            min_snr: MIN_SNR,
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
    /// The supplied beat rate (option, calibre or session file), never a guess.
    /// With no supplied rate, no-watch text gives scatter only in ms (#857).
    pub bph_set: Option<u32>,
    pub rate_s_per_day: Option<f64>,
    /// Signed beat error from the fit to the beat times (near the drop), ms.
    pub beat_error_ms: Option<f64>,
    /// Signed beat error measured from the unlock, the figure checked
    /// against tg's beat error in regress: median over the amplitude
    /// windows, ms. Positive means Tick comes late; each take starts
    /// positive ([`crate::beats::SideCheck`]).
    pub beat_error_unlock_ms: Option<f64>,
    /// Source of the displayed unlock-else-drop beat error; absent when withheld.
    pub beat_error_source: Option<crate::session::BeatErrorSource>,
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
    /// scaled to the averaging time, its noise from at least
    /// [`crate::confidence::MIN_NOISE_VALUES`] of them, borrowed from
    /// the take before the reading where the run holds fewer (#1470).
    /// `None` until the take holds that many, about
    /// [`measure::RATE_SE_MIN_S`] of beats, and when no regular beat is
    /// found.
    pub rate_error_s_per_day: Option<f64>,
    /// The rate values the take holds up to the reading to take the rate's
    /// noise from, every other 10 s window, up to the
    /// [`crate::confidence::MIN_NOISE_VALUES`] its ± needs: for a hover
    /// that says how near the ± is. Not saved.
    #[serde(skip)]
    pub rate_noise_values: usize,
    /// The 2 s windows in the averaging time that read an amplitude, and
    /// those the take holds up to the reading, up to the
    /// [`crate::confidence::MIN_NOISE_VALUES`] its ± takes its scatter
    /// from: for a hover that says why there is no ±. Not saved.
    #[serde(skip)]
    pub amplitude_values: usize,
    #[serde(skip)]
    pub amplitude_noise_values: usize,
    /// `beat_error_unlock_ms`'s ±: the standard error of its 2 s windows
    /// in the averaging time, its noise borrowed as the amplitude's.
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
    /// The rate a fresh guess names instead of Auto's lock, and the
    /// passes of doubt it holds ([`LiveAnalyzer::lock_doubted`]).
    doubt: Option<(u32, u32)>,
    template: Option<Vec<f32>>,
    /// Building the template again from the first chunk (#1327).
    rebuild: rebuild::Rebuild,
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
    /// The take's one sample of beat sound, spread evenly over everything
    /// heard ([`crate::sound_sample`]): each kept beat's time, even side
    /// and window. The watch verdict reads it, as a saved reading does.
    sample: crate::sound_sample::SoundSample<Vec<f32>>,
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

/// The share of a window's beats at Auto's locked rate that must be good
/// for the lock to stand unquestioned ([`LiveAnalyzer::lock_doubted`]). A
/// watch locked on at twice its rate has every other beat empty, about
/// half good; one at its own rate, nearly all.
const LOCK_GOOD_SHARE: f64 = 0.75;

/// The doubt that lets Auto's lock go, seconds: passes of doubt times
/// the time between passes ([`LiveAnalyzer::lock_doubted`]). Six passes
/// at the default half second; short beside the 10 s a regular beat is
/// judged on.
const RELOCK_S: f64 = 3.0;

impl LiveAnalyzer {
    pub fn new(sample_rate: u32, cfg: LiveConfig) -> Self {
        LiveAnalyzer {
            fs: sample_rate as f64,
            bph: cfg.analysis.bph,
            found_bph: false,
            doubt: None,
            cfg,
            buf: Vec::new(),
            buf0: 0,
            total: 0,
            next_pass: 0,
            settled_to: 0.0,
            template: None,
            rebuild: Default::default(),
            beats: Vec::new(),
            clipped: Vec::new(),
            amp: Vec::new(),
            next_amp_s: None,
            profiles: [None, None],
            snippets: VecDeque::new(),
            sample: Default::default(),
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
        // The clean beats' sound the log kept, each with its side: the
        // verdict's sample, and the profiles' recent span.
        let mut kept = log.snippets.into_iter().peekable();
        for b in &log.beats {
            while kept.next_if(|s| s.0 < b.time - 1e-9).is_some() {}
            if let Some(s) = kept.next_if(|s| (s.0 - b.time).abs() < 1e-9) {
                let even = b.index.rem_euclid(2) == 0;
                a.sample.push(s.0, even, s.1.clone());
                a.snippets.push_back((s.0, even, s.1));
            }
        }
        a.beats = log.beats;
        a.amp = log.amplitude_windows;
        // In order of midpoint, as listening keeps them, so a reading finds
        // its stretch's windows by halving ([`Self::amp_within`]). The
        // engine writes them so; the sort only moves a log that isn't.
        a.amp.sort_by(|x, y| amp_mid(x).total_cmp(&amp_mid(y)));
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
        self.in_shadow(|l| l.set_profile_span(seconds));
    }

    fn profile_span(&self) -> f64 {
        profile_span_s(self.cfg.profile_s)
    }

    /// Times of the settled beats whose sound clipped, oldest first.
    pub fn clipped_beats(&self) -> &[f64] {
        &self.clipped
    }

    pub fn amplitude_windows(&self) -> &[AmplitudeWindow] {
        &self.amp
    }

    /// The take's sample of beat sound ([`crate::sound_sample`]), oldest
    /// first, as a log keeps it ([`BeatLog::snippets`]), so a reading of
    /// the log judges a stretch's beat sound as this card does.
    pub fn snippets(&self) -> Vec<(f64, Vec<f32>)> {
        self.sample
            .items()
            .iter()
            .map(|s| (s.0, s.2.clone()))
            .collect()
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
        self.forget_rebuild();
        self.bph = self.cfg.analysis.bph;
        self.found_bph = false;
        self.doubt = None;
        self.logged_bph = self.bph;
        self.beats.clear();
        self.clipped.clear();
        self.amp.clear();
        self.next_amp_s = None;
        self.profiles = [None, None];
        self.snippets.clear();
        self.sample.clear();
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
        self.give_up_rebuild("a new stretch began first");
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
            self.sample.retain(|s| !(a <= s.0 && s.0 < b));
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
        self.sample.swap_sides_from(t);
        for w in self.amp.iter_mut().filter(|w| w.start_s >= t) {
            w.swap_sides();
        }
    }

    /// Replace caller-supplied position/settling boundaries, retaining known
    /// jumps from the imported beat log. Distinct timestamps remain distinct;
    /// a position boundary is not evidence that a nearby jump is the same cut.
    pub fn set_breaks(&mut self, mut breaks: Vec<f64>) {
        self.give_up_rebuild("breaks were set first");
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
        self.in_shadow(|l| l.set_masked(spans));
    }

    /// Move the audio clock on by `seconds` with no audio, for a pause in
    /// listening: the beat log, template and readings are kept, the audio
    /// buffer starts afresh, and the beats after the pause carry on in time
    /// after a gap (which the readings don't fit across).
    pub fn skip(&mut self, seconds: f64) {
        self.give_up_rebuild("listening paused first");
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
            self.in_shadow(|l| {
                l.set_bph(bph);
            });
        }
        heard
    }

    /// Say whether the lift angle is the calibre's and the sound card's
    /// clock is corrected: each unknown is a caveat.
    pub fn set_known(&mut self, lift_known: bool, clock_known: bool) {
        self.cfg.analysis.lift_known = lift_known;
        self.clock_known = clock_known;
        self.in_shadow(|l| l.set_known(lift_known, clock_known));
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
        self.in_shadow(|l| l.set_rate_clock(clock));
    }

    /// A clock's error over audio `from..to`, ppm fast: its mapping's
    /// slope there.
    fn clock_ppm(c: &crate::clock::ClockFit, from: f64, to: f64) -> f64 {
        if to - from < 1e-6 {
            return c.ppm;
        }
        crate::clock::ppm_of_scale((c.map(to) - c.map(from)) / (to - from))
    }

    /// Change the lift angle: windows read again at it as a kept take's ([`amplitude::relift`]).
    pub fn set_lift(&mut self, lift_deg: f64) {
        let old = self.cfg.analysis.amplitude.lift_deg;
        if lift_deg > 0.0 && lift_deg != old {
            if let Some(bph) = self.bph {
                // A swing is two beats.
                let osc_period_s = 2.0 * crate::timing::beat_period_s(None, bph);
                amplitude::relift(&mut self.amp, osc_period_s, old, lift_deg);
                // Each profile read again from its own edges at the new
                // lift, through the same gate, not scaled (#1539).
                for p in self.profiles.iter_mut().flatten() {
                    p.amplitude_deg = p.amplitude_at(osc_period_s, lift_deg);
                }
            }
            self.cfg.analysis.amplitude.lift_deg = lift_deg;
        }
        self.in_shadow(|l| l.set_lift(lift_deg));
    }

    /// Keep up to `n` beats from now on, the oldest dropped beyond them.
    pub fn set_max_beats(&mut self, n: usize) {
        self.cfg.max_beats = n;
        self.in_shadow(|l| l.set_max_beats(n));
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
        ran |= self.rebuild_after_push(samples, ran);
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
                Some(last) => last.index + beats::beats_on(t - last.time, beat),
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

        let (bph, local) = self.beats_in(&env, x, a.bph);
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
            self.quiet_pass(&local, off, from, to, beat);
            return;
        }
        // Auto locked on to a rate the beats don't bear out: let it go,
        // and the next pass guesses again (#1617).
        if self.lock_doubted(&env, &local) {
            self.let_go_of_guess();
            self.jump_at(to, false);
            self.settled_to = to;
            return;
        }
        // Number this pass's beats on from the last settled beat.
        let Some(anchor) = beats::anchor_of(&local, off, from, to, self.beats.last(), beat) else {
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
        self.take_beats(x, &env, &numbered[anchor..], off, to, beat);
        self.find_jumps_and_trim();
        self.amplitude_pass(&env, &numbered, off, from, to, beat);
        self.update_profiles(to, beat);
        self.settled_to = to;
    }

    /// The beats in this pass's envelope `env` of samples `x`, at the rate
    /// locked on to, or else at `bph_set` or a guessed rate, locking on to
    /// it when a watch is heard in enough of them.
    fn beats_in(&mut self, env: &[f32], x: &[f32], bph_set: Option<u32>) -> (u32, Vec<Beat>) {
        let fs = self.fs;
        match (&self.template, self.bph) {
            (Some(t), Some(bph)) => (bph, beats::detect_with_template(env, fs, bph, t)),
            _ => {
                let bph = bph_set.unwrap_or_else(|| beats::guess_bph(env, fs));
                let (b, t) = beats::detect(env, fs, bph);
                let heard =
                    heard_over(snr(env, fs, &b), self.cfg.min_snr) || clipped_out(x, fs, &b);
                if heard && b.len() >= 8 {
                    self.template = Some(t);
                    self.bph = Some(bph);
                    self.found_bph = bph_set.is_none();
                    self.logged_bph = Some(bph);
                }
                (bph, b)
            }
        }
    }

    /// A pass with no watch heard over `from` to `to`: its peaks kept as
    /// candidates, and the lock let go after a long silence.
    fn quiet_pass(&mut self, local: &[Beat], off: f64, from: f64, to: f64, beat: f64) {
        self.keep_candidates(local, off, from, to, beat);
        self.quiet_passes += 1;
        // A long silence with a guessed rate: start over on the next
        // watch, which may beat at another rate.
        if self.quiet_passes as f64 * self.cfg.update_s >= 5.0
            && self.cfg.analysis.bph.is_none()
            && self.template.is_some()
        {
            self.let_go_of_guess();
        }
        self.settled_to = to;
        self.next_amp_s = None;
    }

    /// Let a guessed rate go: the next pass heard guesses it afresh.
    fn let_go_of_guess(&mut self) {
        self.template = None;
        self.forget_rebuild();
        self.bph = None;
        self.found_bph = false;
        self.doubt = None;
    }

    /// Whether Auto's lock has stopped fitting the sound (#1617). A pass
    /// where fewer than [`LOCK_GOOD_SHARE`] of the window's beats at the
    /// locked rate are good asks a fresh guess on the window: one naming
    /// the locked rate clears the doubt, one naming another rate adds a
    /// pass to it (or starts it afresh on a third rate), and a pass with
    /// enough good beats takes one off. The lock is let go once the
    /// doubt holds [`RELOCK_S`] of passes. So a watch put back after
    /// handling noise locked Auto on to another rate (an 18,000 bph watch
    /// read at 36,000, every other beat empty) is read again at its own
    /// rate. A rate set by hand is never doubted.
    fn lock_doubted(&mut self, env: &[f32], local: &[Beat]) -> bool {
        let locked = match (self.bph, self.found_bph, self.cfg.analysis.bph) {
            (Some(b), true, None) => b,
            _ => {
                self.doubt = None;
                return false;
            }
        };
        let good = local.iter().filter(|b| b.is_good()).count();
        if local.is_empty() || good as f64 >= LOCK_GOOD_SHARE * local.len() as f64 {
            self.doubt = self.doubt.and_then(|(r, n)| (n > 1).then_some((r, n - 1)));
            return false;
        }
        let guess = beats::guess_bph(env, self.fs);
        self.doubt = match self.doubt {
            _ if guess == locked => None,
            Some((r, n)) if r == guess => Some((r, n + 1)),
            _ => Some((guess, 1)),
        };
        self.doubt
            .is_some_and(|(_, n)| n as f64 * self.cfg.update_s >= RELOCK_S)
    }

    /// Settles this pass's numbered beats before `to`, noting those that
    /// clipped and keeping the good ones' sound for the profiles.
    fn take_beats(
        &mut self,
        x: &[f32],
        env: &[f32],
        numbered: &[Beat],
        off: f64,
        to: f64,
        beat: f64,
    ) {
        let fs = self.fs;
        // One scan of the window finds whether anything clipped; only then
        // is each new beat looked at on its own.
        let any_clipped = Clipping::measure(x, fs, &[]).samples > 0;
        for b in numbered {
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
                if let Some(w) = profile::snippet(env, fs, b.time) {
                    let even = b.index.rem_euclid(2) == 0;
                    self.snippets.push_back((t, even, w.to_vec()));
                    self.sample.push(t, even, w.to_vec());
                    self.sides.beat(t, b.index, b.quality, w);
                }
            }
        }
    }

    /// Marks the jumps among the latest good beats, and drops the oldest
    /// beats past those kept.
    fn find_jumps_and_trim(&mut self) {
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
    }

    /// Amplitude on a grid of windows that have settled by `to`.
    fn amplitude_pass(
        &mut self,
        env: &[f32],
        numbered: &[Beat],
        off: f64,
        from: f64,
        to: f64,
        beat: f64,
    ) {
        let fs = self.fs;
        let a = &self.cfg.analysis;
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
                env,
                fs,
                numbered,
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
    }

    /// The Tick and Tock profiles from the sound kept up to `to`.
    fn update_profiles(&mut self, to: f64, beat: f64) {
        let fs = self.fs;
        // After the windows, so those of this pass follow a swap too.
        self.settle_sides();
        let span = self.profile_span();
        while self.snippets.front().is_some_and(|f| f.0 < to - span) {
            self.snippets.pop_front();
        }
        // Until the sides are told apart there is no Tick or Tock to show.
        let sided = self.sides.pending().is_none();
        let guides = self.profile_guides_at(to);
        let a = &self.cfg.analysis;
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

    /// The cuts in the sound itself: every break that is not a jump in
    /// the beats, and every masked stretch. The watch verdict looks back
    /// to the latest of them.
    fn heard_cuts(&self) -> Cuts<'static> {
        let breaks: Vec<Break> = self
            .breaks
            .iter()
            .filter(|b| !self.intrinsic_breaks.contains(b))
            .map(|&audio_s| Break {
                audio_s,
                gap_s: 0.0,
            })
            .collect();
        Cuts::new(None, &breaks, &self.masked)
    }

    /// The peaks heard from `from_s` to `to_s`: the settled beats, and
    /// the candidates where no regular beat was being logged, in time
    /// order and counted on as one run, as a saved reading's beat log
    /// holds them. With no candidates there, the settled beats as they
    /// are.
    fn heard_peaks(&self, from_s: f64, to_s: f64) -> Vec<Beat> {
        let beats = &self.beats[crate::session::stretch_beats(&self.beats, from_s, to_s)];
        let mut peaks: Vec<Beat> = self
            .candidates
            .iter()
            .filter(|c| c.time >= from_s && c.time < to_s)
            .copied()
            .collect();
        if peaks.is_empty() {
            return beats.to_vec();
        }
        peaks.extend_from_slice(beats);
        peaks.sort_by(|a, b| a.time.total_cmp(&b.time));
        let beat = 3600.0 / self.bph.or(self.logged_bph).unwrap_or(1) as f64;
        for i in 1..peaks.len() {
            peaks[i].index =
                peaks[i - 1].index + beats::beats_on(peaks[i].time - peaks[i - 1].time, beat);
        }
        peaks
    }

    /// Readings over the `average_s` seconds before `end_s`.
    pub fn reading_at(&self, end_s: f64, average_s: f64) -> LiveReading {
        let window_from = end_s - average_s;
        let masked_s = crate::session::masked_within(&self.masked, window_from, end_s);
        let st = self.stretch_at(window_from, end_s);
        let (from, lo, hi, span_s) = (st.from, st.lo, st.hi, st.span_s);
        let holes = &st.holes;
        let bph = self.bph.or(self.logged_bph);
        // No window reaches back across the latest cut: the amplitude
        // windows' grid starts again at each cut, and `stretch_windows`
        // keeps only windows clear of every hole, so a jump in a recording
        // read whole falls between two.
        let wins =
            crate::session::stretch_windows(holes, self.amp_within(from, end_s), from, end_s);
        // Fitted as every reading of a stretch is (#512). The latest run
        // holds no cut (`stretch_at` starts it after the latest one), so
        // it is fitted whole and its beat error's sign does not turn.
        let crate::session::StretchFits { fit, figures, .. } =
            crate::session::stretch_fits(&self.beats[lo..hi], bph, &[], &wins);
        let (run_from, rate_windows) = self.run_rate_windows(&st, bph, end_s);
        // The take's windows the ±s borrow their noise from (#1470): the
        // run's own rate windows when they hold enough.
        let take_rate = (!crate::session::rate_noise_enough(&rate_windows))
            .then(|| self.take_rate_windows(&st, bph, run_from, &rate_windows));
        let take_rate = take_rate.as_deref().unwrap_or(&rate_windows);
        let take_amp = self.take_amplitude_windows(&st, end_s);
        let amplitude_error_deg =
            crate::confidence::amplitude_windows_se(wins.iter().copied(), &take_amp, |w| w.mean());
        let Verdict {
            jitter_us,
            timing_scatter_fraction,
            no_watch,
        } = self.verdict_at(holes, bph, window_from, end_s, st.listened_s);
        let watch_heard = no_watch.is_none();
        let heard = |v: Option<f64>| v.filter(|_| watch_heard);
        // Figures that name a side wait until the sides are told apart.
        let sided = self.sides_known_at(end_s);
        let sided = |v: Option<f64>| heard(v).filter(|_| sided);
        let clipped_fraction = st.clipped_fraction();
        let take = measure::TakeWindows {
            rate: take_rate,
            amplitude: &take_amp,
        };
        let assessment = self.assess_at(
            &st,
            (&rate_windows, &wins, take),
            bph,
            no_watch,
            (end_s, average_s),
        );
        let mut caveats = assessment.caveats;
        let beat_error_ms = sided(fit.map(|f| f.beat_error_ms));
        let beat_error_unlock_ms = sided(figures.beat_error_unlock_ms);
        let beat_error_source =
            crate::session::chosen_beat_error(beat_error_unlock_ms, beat_error_ms)
                .map(|(_, source)| source);
        if let Some(caveat) =
            beat_error_source.and_then(|s| s.caveat_why(figures.unlock_inconsistent))
        {
            measure::add(&mut caveats, caveat);
        }
        if masked_s > 0.0 {
            measure::add(&mut caveats, Caveat::Masked { seconds: masked_s });
        }
        self.add_rebuild_caveat(&mut caveats);
        let whole = holes.whole(&self.beats, window_from, end_s, GAP_S);
        LiveReading {
            bph_set: self.cfg.analysis.bph,
            time_s: end_s,
            bph,
            rate_s_per_day: heard(fit.map(|f| f.rate_s_per_day)).map(|v| {
                self.rate_clock.as_ref().map_or(v, |c| {
                    crate::clockstore::correct_rate(v, Self::clock_ppm(c, from, end_s))
                })
            }),
            beat_error_ms,
            beat_error_unlock_ms,
            beat_error_source,
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
            clipped_beats: st.clipped_beats,
            clipped_fraction,
            watch_heard,
            rate_error_s_per_day: assessment.confidence.and_then(|c| c.rate.se),
            rate_noise_values: crate::confidence::rate_value_count(take_rate.len())
                .min(crate::confidence::MIN_NOISE_VALUES),
            amplitude_values: wins.iter().filter(|w| w.mean().is_some()).count(),
            amplitude_noise_values: take_amp
                .iter()
                .filter(|w| w.mean().is_some())
                .count()
                .min(crate::confidence::MIN_NOISE_VALUES),
            beat_error_unlock_error_ms: sided(assessment.confidence.and_then(|c| c.beat_error.se)),
            caveats,
        }
    }

    /// The amplitude windows whose midpoint lies from `from_s` up to but
    /// not at `to_s`, found by halving: the only ones
    /// [`crate::session::window_in`] can take for that stretch. `amp` is
    /// kept in order of midpoint: listening adds each pass's windows after
    /// the last, and [`Self::from_log`] sorts a log's. Filtering every
    /// window of a 65 h take for each of a trend's 4,000 readings held
    /// the window for seconds after a mask (#978).
    fn amp_within(&self, from_s: f64, to_s: f64) -> &[AmplitudeWindow] {
        let lo = self.amp.partition_point(|w| amp_mid(w) < from_s);
        let hi = self.amp.partition_point(|w| amp_mid(w) < to_s);
        &self.amp[lo..hi.max(lo)]
    }

    /// The stretch read at `end_s` over the averaging time from
    /// `window_from`.
    fn stretch_at(&self, window_from: f64, end_s: f64) -> Stretch {
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
        Stretch {
            holes,
            listened_s,
            from,
            lo,
            hi,
            span_s,
            clipped_beats,
        }
    }

    /// Whether a watch is heard at `end_s`, and the beats' scatter it is
    /// judged from.
    fn verdict_at(
        &self,
        holes: &Cuts,
        bph: Option<u32>,
        window_from: f64,
        end_s: f64,
        listened_s: f64,
    ) -> Verdict {
        // Beats scattered like noise are no regular beat: no readings, as
        // `analyze` gives none. Under 10 s of sound it is not assessed.
        // The scatter is a saved reading's, from the 10 s windows over all
        // the sound heard since the latest break or mask, not the latest
        // run's alone: a short regular run at the end of noise is no watch
        // (#679, #680). A jump in the beats is no break in the sound, so
        // the windows skip it but still look back past it. The sound heard
        // before a regular beat was found counts by its peaks (the
        // candidates), as a saved reading logs them. It looks back at
        // least SCATTER_LOOKBACK_S, so a 10 s average holds a whole window,
        // from the tiling point at or before that (#636), as session's
        // windows lie on the same tiling: so the sound before a jump in
        // the beats gets its own whole window.
        // Within that of a break, before a window can be whole, regularity
        // is not assessed; past it, no whole window is noise.
        let heard_since = self.heard_cuts().stretch_from(end_s).max(0.0);
        let step = crate::measure::RATE_WINDOW_STEP_S;
        let look_back = ((end_s - SCATTER_LOOKBACK_S) / step).floor() * step;
        let scatter_from = window_from.min(look_back).max(heard_since).max(0.0);
        let scatter = bph.map_or_else(Vec::new, |b| {
            crate::session::scatter_windows(
                holes,
                &self.heard_peaks(scatter_from, end_s),
                b,
                scatter_from,
                end_s,
                crate::session::window_part_s(&self.amp),
            )
        });
        let jitter_us = crate::session::scatter_jitter_us(&scatter);
        let judged_s = judged_s(!scatter.is_empty(), end_s, window_from, heard_since, holes);
        let timing_scatter_fraction =
            bph.and_then(|b| crate::beats::timing_scatter_fraction(jitter_us, b));
        // Where the timing can't say, the beats' averaged sound, from the
        // take's sample of beat sound, says whether a watch is heard: over
        // the beats the scatter is read from, as a saved reading judges
        // its stretch's ([`BeatLog::beat_sound`]), not the latest run's.
        let sound_beats = crate::session::stretch_beats(&self.beats, scatter_from, end_s);
        let beat_sound = match (bph, &self.beats[sound_beats]) {
            (Some(b), [first, .., last]) if measure::beat_sound_needed(jitter_us, b, judged_s) => {
                let snips: Vec<(f64, bool, &[f32])> = self
                    .sample
                    .items()
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
            Some(b) => measure::no_watch_reason(jitter_us, b, judged_s, beat_sound),
            None => measure::regularity_assessed(listened_s).then_some(NoWatchReason::Scatter),
        };
        Verdict {
            jitter_us,
            timing_scatter_fraction,
            no_watch,
        }
    }

    /// The beats from `from_s` up to the stretch's end: no window
    /// [`Cuts::windows`] lays from `from_s` reads an earlier one.
    fn beats_from(&self, st: &Stretch, from_s: f64) -> &[Beat] {
        let lo = self.beats[..st.hi].partition_point(|b| b.time < from_s);
        &self.beats[lo..st.hi]
    }

    /// The rate's 10 s windows over the run so far: the latest unbroken
    /// run of beats, back to a break, up to RATE_RUN_S; and where the run
    /// starts.
    fn run_rate_windows(
        &self,
        st: &Stretch,
        bph: Option<u32>,
        end_s: f64,
    ) -> (f64, Vec<WindowFit>) {
        let floor = st.from.min(end_s - RATE_RUN_S);
        let from = floor.max(st.holes.run_cut(&self.beats, floor, end_s));
        let windows = bph.map_or_else(Vec::new, |b| {
            st.holes.windows(
                self.beats_from(st, from),
                b,
                from,
                end_s,
                RATE_W,
                RATE_STEP,
                GAP_S,
            )
        });
        (from, windows)
    }

    /// The take's rate windows nearest a reading whose run from `run_from`
    /// holds the windows `run`, outside the masks and across its cuts,
    /// that the rate's ± borrows its noise from where the run holds too
    /// few (#1470): those before the run, ending by its start, then the
    /// run's own. All lie on the take's one tiling (#636).
    fn take_rate_windows(
        &self,
        st: &Stretch,
        bph: Option<u32>,
        run_from: f64,
        run: &[WindowFit],
    ) -> Vec<WindowFit> {
        bph.map_or_else(Vec::new, |b| {
            crate::session::nearest_windows(
                (run_from, run_from, run_from),
                RATE_FIRST_REACH_S,
                |a, _| {
                    let beats = self.beats_from(st, a);
                    let mut w = st
                        .holes
                        .windows(beats, b, a, run_from, RATE_W, RATE_STEP, GAP_S);
                    w.extend_from_slice(run);
                    w
                },
                crate::session::rate_noise_enough,
            )
        })
    }

    /// The take's amplitude windows up to `end_s` nearest it, as
    /// [`Self::take_rate_windows`] for the amplitude and beat error.
    fn take_amplitude_windows(&self, st: &Stretch, end_s: f64) -> Vec<&AmplitudeWindow> {
        crate::session::nearest_windows(
            (end_s, end_s, end_s),
            crate::session::AMPLITUDE_FIRST_REACH_S,
            |a, _| crate::session::stretch_windows(&st.holes, self.amp_within(a, end_s), a, end_s),
            |w: &[&AmplitudeWindow]| crate::session::noise_enough(w),
        )
    }

    /// The confidence and caveats of the reading over `st`, from the run's
    /// rate windows and the stretch's amplitude windows, its ± borrowing
    /// noise from `take`.
    fn assess_at(
        &self,
        st: &Stretch,
        (rate_windows, wins, take): (&[WindowFit], &[&AmplitudeWindow], measure::TakeWindows),
        bph: Option<u32>,
        no_watch: Option<NoWatchReason>,
        (end_s, average_s): (f64, f64),
    ) -> measure::Assessment {
        // The rate's ± is scaled to the averaging time by `measure`.
        measure::assess_in(
            rate_windows,
            wins,
            take,
            &measure::Context {
                watch_heard: no_watch.is_none(),
                no_watch_reason: no_watch.unwrap_or_default(),
                bph_heard: None,
                lift_known: self.cfg.analysis.lift_known,
                clock_known: self.clock_known || self.rate_clock.is_some(),
                beats: st.hi - st.lo,
                expected: bph.map_or(0.0, |b| b as f64 / 3600.0 * st.span_s),
                clipped_fraction: st.clipped_fraction().unwrap_or(0.0),
                // The time the beats stand for, each one beat long: a full
                // 10 s average is 10 s of beats, though its first and last
                // beats are one beat less apart.
                span_s: st.span_s + bph.map_or(0.0, |b| 3600.0 / b as f64),
                listened_s: st.listened_s,
                breaks: 0,
                // Tick and Tock here are named across every such jump
                // before the reading.
                sides_by_count: self.by_count.iter().filter(|&&b| b <= end_s).count(),
                beat_error_as_size: false,
                average_s: Some(average_s),
            },
        )
    }
}

/// The stretch a live reading reads: the latest unbroken run of settled
/// beats within the averaging time.
struct Stretch {
    /// The breaks and masked stretches in what has been heard.
    holes: Cuts<'static>,
    /// The sound heard over the averaging time, since the start or the
    /// latest break.
    listened_s: f64,
    /// Where the run's reading starts.
    from: f64,
    /// The run's first settled beat.
    lo: usize,
    /// One past the run's last settled beat.
    hi: usize,
    /// The time the run's beats span.
    span_s: f64,
    /// The run's beats that clipped.
    clipped_beats: usize,
}

impl Stretch {
    /// The fraction of the run's beats that clipped; none with no beats.
    fn clipped_fraction(&self) -> Option<f64> {
        (self.hi > self.lo).then(|| self.clipped_beats as f64 / (self.hi - self.lo) as f64)
    }
}

/// Whether a watch is heard, and the beats' scatter it is judged from.
struct Verdict {
    /// The median timing jitter of the 10 s windows, in µs.
    jitter_us: Option<f64>,
    /// That jitter as a fraction of a beat.
    timing_scatter_fraction: Option<f64>,
    /// Why no watch is heard; none when one is.
    no_watch: Option<NoWatchReason>,
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

/// Averaging times offered for the readings, seconds: the app's Average
/// Over, and what `analyze --trend-average` takes.
pub const AVERAGES: [f64; 6] = [2.0, 4.0, 10.0, 20.0, 30.0, 60.0];
/// The averaging time the app starts at, seconds.
pub const DEFAULT_AVERAGE_S: f64 = 10.0;
/// The seconds of beats the live strip shows at first: the app's strip,
/// and the strip `record --live` keeps.
pub const DEFAULT_STRIP_S: f64 = 30.0;

/// Most readings a trend over a whole take is read at: the app's charts
/// over time, and the MCP view's.
pub const TREND_POINTS: f64 = 4000.0;

/// An amplitude window's midpoint, the time [`crate::session::window_in`]
/// places it by.
fn amp_mid(w: &AmplitudeWindow) -> f64 {
    (w.start_s + w.end_s) / 2.0
}

/// The times a trend over `duration_s` seconds is read at: evenly spaced,
/// at most [`TREND_POINTS`] of them and no closer than 0.5 s, the last at
/// or before the end.
pub fn trend_times(duration_s: f64) -> Vec<f64> {
    let step = (duration_s / TREND_POINTS).max(0.5);
    let mut out = Vec::new();
    let mut t = step;
    while t <= duration_s + 1e-9 {
        out.push(t);
        t += step;
    }
    out
}

/// One reading of a trend over a take: the figures a chart over time
/// draws at `t`. A reading whose averaging time is not a whole bin (just
/// after the start, a break or a masked stretch, or over a dropout; see
/// [`LiveReading::whole`]) has no figures, since it isn't the average the
/// chart is of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrendReading {
    pub t: f64,
    pub rate_s_per_day: Option<f64>,
    pub amplitude_deg: Option<f64>,
    /// The Tick's (even beats') and the Tock's amplitude.
    pub amplitude_even_deg: Option<f64>,
    pub amplitude_odd_deg: Option<f64>,
    pub beat_error_unlock_ms: Option<f64>,
    pub beat_error_ms: Option<f64>,
    /// Each figure's standard error, as its card shows it after "±", so
    /// a chart's values box and the card agree (#1063, #1470): the rate's, the amplitude's (Tick's and Tock's take the
    /// same, as on the card) and the beat error from the unlock's.
    pub rate_error_s_per_day: Option<f64>,
    pub amplitude_error_deg: Option<f64>,
    pub beat_error_unlock_error_ms: Option<f64>,
    /// The beats were spaced like a watch's, not scattered like noise.
    pub heard: bool,
}

/// Keep non-overlapping readings in time order, at least one averaging time apart.
/// The microsecond tolerance preserves the grid boundary despite floating rounding.
pub fn spaced_readings<T>(readings: impl IntoIterator<Item = (f64, T)>, every_s: f64) -> Vec<T> {
    const TIME_EPS_S: f64 = 1e-6;
    let mut last = f64::NEG_INFINITY;
    readings
        .into_iter()
        .filter_map(|(t, value)| {
            if t >= last + every_s - TIME_EPS_S {
                last = t;
                Some(value)
            } else {
                None
            }
        })
        .collect()
}

impl TrendReading {
    /// The displayed beat error and its source, using the direct-reading rule.
    pub fn beat_error(&self) -> Option<(f64, crate::session::BeatErrorSource)> {
        crate::session::chosen_beat_error(self.beat_error_unlock_ms, self.beat_error_ms)
    }

    /// One source's displayed readings, leaving the other source as holes.
    pub fn beat_error_from(&self, source: crate::session::BeatErrorSource) -> Option<f64> {
        self.beat_error()
            .filter(|(_, s)| *s == source)
            .map(|(v, _)| v)
    }

    /// The trend's reading at `t` from the live reading there.
    pub fn of(t: f64, r: &LiveReading) -> Self {
        if !r.whole {
            return TrendReading {
                t,
                rate_s_per_day: None,
                amplitude_deg: None,
                amplitude_even_deg: None,
                amplitude_odd_deg: None,
                beat_error_unlock_ms: None,
                beat_error_ms: None,
                rate_error_s_per_day: None,
                amplitude_error_deg: None,
                beat_error_unlock_error_ms: None,
                heard: false,
            };
        }
        TrendReading {
            t,
            rate_s_per_day: r.rate_s_per_day,
            amplitude_deg: r.amplitude_deg,
            amplitude_even_deg: r.amplitude_even_deg,
            amplitude_odd_deg: r.amplitude_odd_deg,
            beat_error_unlock_ms: r.beat_error_unlock_ms,
            beat_error_ms: r.beat_error_ms,
            rate_error_s_per_day: r.rate_error_s_per_day,
            amplitude_error_deg: r.amplitude_error_deg,
            beat_error_unlock_error_ms: r.beat_error_unlock_error_ms,
            heard: r.beats_used > 0 && r.watch_heard,
        }
    }
}

impl LiveAnalyzer {
    /// The readings over the whole of what has been heard, each averaged
    /// over `average_s` ([`Self::reading_at`]), at [`trend_times`].
    pub fn trend(&self, average_s: f64) -> Vec<TrendReading> {
        trend_times(self.duration_s())
            .into_iter()
            .map(|t| TrendReading::of(t, &self.reading_at(t, average_s)))
            .collect()
    }
}

/// How long the watch verdict judges a reading ending at `end_s` over,
/// seconds: none while no scatter window fits (`fitted` false) within
/// [`SCATTER_LOOKBACK_S`] of the latest heard break, `heard_since`, or the
/// latest of the reading's `holes`, a jump in the beats too: the windows
/// skip a jump, so until one fits after it there may be none, and none is
/// no verdict of noise (#1170). Past that, having none is.
fn judged_s(fitted: bool, end_s: f64, window_from: f64, heard_since: f64, holes: &Cuts) -> f64 {
    let cut_since = holes.stretch_from(end_s).max(heard_since);
    if !fitted && end_s - cut_since < SCATTER_LOOKBACK_S {
        0.0
    } else {
        (end_s - window_from.max(heard_since)).max(0.0)
    }
}

mod rebuild;

#[cfg(test)]
mod jump_tests;

#[cfg(test)]
mod count_tests;
#[cfg(test)]
mod drop_why_tests;
#[cfg(test)]
mod sound_tests;

#[cfg(test)]
#[path = "live/live_tests.rs"]
mod tests;

#[cfg(test)]
mod relock_tests;
