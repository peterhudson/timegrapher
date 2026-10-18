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
use crate::dsp::{envelope, median, median_f32};
use crate::measure::{self, Caveat};
use crate::profile::{self, Side, TickProfile};
use std::collections::VecDeque;

/// A pause between beats longer than this, seconds, breaks the beat log:
/// the readings never fit across it, since the watch may have been moved
/// or handled, and beats may have been counted wrongly across it.
pub const GAP_S: f64 = 2.0;

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
    /// windows, ms. Positive means Tick (the run's first beat) comes late.
    pub beat_error_unlock_ms: Option<f64>,
    pub amplitude_deg: Option<f64>,
    /// Roughly how far `amplitude_deg` could be off from the scatter of
    /// the 2 s windows alone, as `amplitude::standard_error` gives it for a
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
    /// The beat rate the logged beats were numbered at, kept when a long
    /// silence lets a guessed rate go, so readings back in the log still
    /// fit.
    logged_bph: Option<u32>,
    /// Known jumps in an imported beat log. Replacing position/settling
    /// boundaries must not erase these intrinsic discontinuities.
    intrinsic_breaks: Vec<f64>,
    /// Intrinsic jumps and caller-supplied stretch boundaries, oldest first:
    /// readings never fit across one.
    breaks: Vec<f64>,
    /// Stretches masked out of the readings by hand, start and end in
    /// seconds, sorted and apart: cut like breaks, and nothing read in
    /// them.
    masked: Vec<(f64, f64)>,
    /// Whether the sound card's clock is corrected, for the caveats.
    clock_known: bool,
}

impl LiveAnalyzer {
    pub fn new(sample_rate: u32, cfg: LiveConfig) -> Self {
        LiveAnalyzer {
            fs: sample_rate as f64,
            bph: cfg.analysis.bph,
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
            snr: None,
            quiet_passes: 0,
            logged_bph: None,
            intrinsic_breaks: Vec::new(),
            breaks: Vec::new(),
            masked: Vec::new(),
            clock_known: false,
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
        a.beats = log.beats;
        a.amp = log.amplitude_windows;
        a.clipped = log.clipped_beats;
        a.intrinsic_breaks = log.jumps;
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

    /// Forget the beats and the template, keeping the audio clock running
    /// (for a new watch or a new position).
    pub fn restart(&mut self) {
        self.template = None;
        self.bph = self.cfg.analysis.bph;
        self.logged_bph = self.bph;
        self.beats.clear();
        self.clipped.clear();
        self.amp.clear();
        self.next_amp_s = None;
        self.profiles = [None, None];
        self.snippets.clear();
        self.settled_to = self.duration_s();
        self.quiet_passes = 0;
        self.intrinsic_breaks.clear();
        self.breaks.clear();
    }

    /// Start a new stretch now, for a new position: the beats so far are
    /// kept, the sound still being worked on (the watch being handled) is
    /// let go, the profiles start again, and no reading fits across the
    /// break.
    pub fn new_stretch(&mut self) {
        let now = self.duration_s();
        self.breaks.push(now);
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

    /// Fix the beat rate, or `None` to guess it. Starts the beat log again.
    pub fn set_bph(&mut self, bph: Option<u32>) {
        if bph != self.cfg.analysis.bph {
            self.cfg.analysis.bph = bph;
            self.restart();
        }
    }

    /// Say whether the lift angle is the calibre's and the sound card's
    /// clock is corrected: each unknown is a caveat.
    pub fn set_known(&mut self, lift_known: bool, clock_known: bool) {
        self.cfg.analysis.lift_known = lift_known;
        self.clock_known = clock_known;
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
            self.cfg.analysis.amplitude.lift_deg = lift_deg;
        }
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
        let fs = self.fs;
        let win = ((self.cfg.window_s * fs).ceil() as usize).min(self.buf.len());
        // Wait for most of a window before the first pass.
        if (win as f64) < 0.75 * self.cfg.window_s * fs {
            return;
        }
        let x = &self.buf[self.buf.len() - win..];
        let off = (self.buf0 + (self.buf.len() - win) as u64) as f64 / fs;
        let end = self.total as f64 / fs;
        let a = &self.cfg.analysis;
        let env = envelope(x, fs, &a.envelope);

        let (bph, local) = match (&self.template, self.bph) {
            (Some(t), Some(bph)) => (bph, beats::detect_with_template(&env, fs, bph, t)),
            _ => {
                let bph = a.bph.unwrap_or_else(|| beats::guess_bph(&env, fs));
                let (b, t) = beats::detect(&env, fs, bph);
                let heard = snr(&env, fs, &b) >= self.cfg.min_snr || clipped_out(x, fs, &b);
                if heard && b.len() >= 8 {
                    self.template = Some(t);
                    self.bph = Some(bph);
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
        self.snr = Some(s);
        let heard = s >= self.cfg.min_snr || clipped_out(x, fs, &local);
        if !heard || self.template.is_none() {
            self.quiet_passes += 1;
            // A long silence with a guessed rate: start over on the next
            // watch, which may beat at another rate.
            if self.quiet_passes as f64 * self.cfg.update_s >= 5.0
                && a.bph.is_none()
                && self.template.is_some()
            {
                self.template = None;
                self.bph = None;
            }
            self.settled_to = to;
            self.next_amp_s = None;
            return;
        }
        self.quiet_passes = 0;

        // Number this pass's beats on from the last settled beat.
        let Some(anchor) = local.iter().position(|b| {
            let t = b.time + off;
            t >= from && t < to
        }) else {
            self.settled_to = to;
            return;
        };
        let anchor_index = match self.beats.last() {
            Some(last) => {
                last.index
                    + ((local[anchor].time + off - last.time) / beat)
                        .round()
                        .max(1.0) as i64
            }
            None => 0,
        };
        let shift = anchor_index - local[anchor].index;
        let numbered: Vec<Beat> = local
            .iter()
            .map(|b| Beat {
                index: b.index + shift,
                ..*b
            })
            .collect();
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
            if b.quality > 0.4 {
                if let Some(w) = profile::snippet(&env, fs, b.time) {
                    self.snippets
                        .push_back((t, b.index.rem_euclid(2) == 0, w.to_vec()));
                }
            }
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
        let span = self.profile_span();
        while self.snippets.front().is_some_and(|f| f.0 < to - span) {
            self.snippets.pop_front();
        }
        self.profiles = [Side::A, Side::B].map(|side| {
            let even = side == Side::A;
            let w: Vec<&[f32]> = self
                .snippets
                .iter()
                .filter(|s| s.1 == even)
                .map(|s| s.2.as_slice())
                .collect();
            profile::from_snippets(&w, fs, side, 2.0 * beat, &a.amplitude)
        });
        self.settled_to = to;
    }

    /// Readings over the last `average_s` seconds of settled beats.
    pub fn reading(&self, average_s: f64) -> LiveReading {
        self.reading_at(self.settled_to, average_s)
    }

    /// Readings over the `average_s` seconds before `end_s`.
    pub fn reading_at(&self, end_s: f64, average_s: f64) -> LiveReading {
        let mut from = end_s - average_s;
        let masked_s = crate::session::masked_within(&self.masked, from, end_s);
        // A masked stretch is cut like a break: readings start again at
        // its end, and inside one there is nothing to read.
        let mask_cut = self
            .masked
            .iter()
            .filter(|m| m.0 <= end_s)
            .map(|m| m.1.min(end_s))
            .fold(f64::NEG_INFINITY, f64::max);
        let latest_break = self
            .breaks
            .iter()
            .rev()
            .find(|&&b| b <= end_s)
            .copied()
            .unwrap_or(f64::NEG_INFINITY)
            .max(mask_cut);
        // The sound heard over the averaging time, since the start or the
        // latest break.
        let since = latest_break.max(0.0);
        let listened_s = (end_s - from.max(since).max(0.0)).max(0.0);
        let mut lo = self.beats.partition_point(|b| b.time < from);
        let hi = self.beats.partition_point(|b| b.time <= end_s);
        // Only the latest unbroken run of beats.
        if let Some(gap) = (lo + 1..hi)
            .rev()
            .find(|&i| self.beats[i].time - self.beats[i - 1].time > GAP_S)
        {
            lo = gap;
            from = self.beats[gap].time - 1e-6;
        }
        // Nor across the start of a new stretch or a masked one.
        if latest_break > from {
            from = latest_break;
            lo = lo.max(self.beats.partition_point(|x| x.time < latest_break));
        }
        let lo = lo.min(hi);
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
        let a0 = self.amp.partition_point(|w| w.end_s <= from);
        let a1 = self.amp.partition_point(|w| w.end_s <= end_s + 1e-9);
        let wins: Vec<&AmplitudeWindow> = self.amp[a0..a1.max(a0)]
            .iter()
            .filter(|w| {
                !self
                    .masked
                    .iter()
                    .any(|&(a, b)| w.start_s < b && w.end_s > a)
            })
            .collect();
        let med = |f: &dyn Fn(&AmplitudeWindow) -> Option<f64>| -> Option<f64> {
            let mut v: Vec<f64> = wins.iter().filter_map(|w| f(w)).collect();
            (!v.is_empty()).then(|| median(&mut v))
        };
        let amplitude_error_deg = crate::amplitude::standard_error(
            &wins.iter().filter_map(|w| w.mean()).collect::<Vec<_>>(),
        );
        // Beats scattered like noise are no regular beat: no readings, as
        // `analyze` gives none. Under 10 s of sound it is not assessed.
        let jitter_us = fit.map(|f| f.jitter_us);
        let timing_scatter_fraction =
            bph.and_then(|b| crate::beats::timing_scatter_fraction(jitter_us, b));
        let watch_heard = !measure::regularity_assessed(listened_s)
            || bph.is_some_and(|b| crate::beats::watch_heard(jitter_us, b));
        let heard = |v: Option<f64>| v.filter(|_| watch_heard);
        let clipped_fraction = (hi > lo).then(|| clipped_beats as f64 / (hi - lo) as f64);
        // The rate's ± from 10 s windows over the run so far (the latest
        // unbroken run of beats, back to a break, up to RATE_RUN_S), scaled
        // to the averaging time by `measure`.
        let rate_windows = bph.map_or_else(Vec::new, |b| {
            let floor = from.min(end_s - RATE_RUN_S).max(latest_break);
            let mut r0 = self.beats.partition_point(|x| x.time < floor);
            if let Some(gap) = (r0 + 1..hi)
                .rev()
                .find(|&i| self.beats[i].time - self.beats[i - 1].time > GAP_S)
            {
                r0 = gap;
            }
            timing::windows(&self.beats[r0.min(lo)..hi], b, 10.0, 5.0)
        });
        let assessment = measure::assess(
            &rate_windows,
            &wins,
            &measure::Context {
                watch_heard,
                bph_heard: None,
                lift_known: self.cfg.analysis.lift_known,
                clock_known: self.clock_known,
                beats: hi - lo,
                expected: bph.map_or(0.0, |b| b as f64 / 3600.0 * span_s),
                clipped_fraction: clipped_fraction.unwrap_or(0.0),
                // The time the beats stand for, each one beat long: a full
                // 10 s average is 10 s of beats, though its first and last
                // beats are one beat less apart.
                span_s: span_s + bph.map_or(0.0, |b| 3600.0 / b as f64),
                listened_s,
                breaks: 0,
                average_s: Some(average_s),
            },
        );
        let mut caveats = assessment.caveats;
        if masked_s > 0.0 {
            measure::add(&mut caveats, Caveat::Masked { seconds: masked_s });
        }
        LiveReading {
            time_s: end_s,
            bph,
            rate_s_per_day: heard(fit.map(|f| f.rate_s_per_day)),
            beat_error_ms: heard(fit.map(|f| f.beat_error_ms)),
            beat_error_unlock_ms: heard(med(&|w| w.beat_error_unlock_ms)),
            amplitude_deg: heard(med(&|w| w.mean())),
            amplitude_error_deg: heard(amplitude_error_deg),
            amplitude_even_deg: heard(med(&|w| w.even_deg)),
            amplitude_odd_deg: heard(med(&|w| w.odd_deg)),
            jitter_us,
            timing_scatter_fraction,
            beats_used: fit.map_or(0, |f| f.beats_used),
            span_s,
            snr: self.snr,
            clipped_beats,
            clipped_fraction,
            watch_heard,
            rate_error_s_per_day: assessment.confidence.and_then(|c| c.rate.se),
            beat_error_unlock_error_ms: assessment.confidence.and_then(|c| c.beat_error.se),
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

/// Median beat peak over the median of the envelope.
fn snr(env: &[f32], fs: f64, beats: &[Beat]) -> f32 {
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
        return 0.0;
    }
    let mut all: Vec<f32> = env.iter().step_by(7).copied().collect();
    let floor = median_f32(&mut all).max(f32::MIN_POSITIVE);
    median_f32(&mut peaks) / floor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{generate, SynthConfig};

    fn feed(a: &mut LiveAnalyzer, x: &[f32]) {
        for block in x.chunks(480) {
            a.push(block);
        }
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
        assert!(r.rate_error_s_per_day.is_none() && r.caveats.first() == Some(&Caveat::NoWatch));
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
}
