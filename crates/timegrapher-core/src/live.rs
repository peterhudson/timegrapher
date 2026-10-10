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
use crate::profile::{self, Side, TickProfile};
use std::collections::VecDeque;

/// A pause between beats longer than this, seconds, breaks the beat log:
/// the readings never fit across it, since the watch may have been moved
/// or handled, and beats may have been counted wrongly across it.
pub const GAP_S: f64 = 2.0;

/// Longest stretch the tick and tock profiles can cover, seconds.
pub const MAX_PROFILE_S: f64 = 60.0;
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
#[derive(Debug, Clone, Copy, Serialize)]
pub struct LiveReading {
    /// Audio time of the reading, seconds from the start.
    pub time_s: f64,
    pub bph: Option<u32>,
    pub rate_s_per_day: Option<f64>,
    /// Signed beat error from the fit to the beat times (near the drop), ms.
    pub beat_error_ms: Option<f64>,
    /// Signed beat error measured from the unlock, as tg and commercial
    /// timegraphers measure it: median over the amplitude windows, ms.
    pub beat_error_unlock_ms: Option<f64>,
    pub amplitude_deg: Option<f64>,
    pub amplitude_even_deg: Option<f64>,
    pub amplitude_odd_deg: Option<f64>,
    pub jitter_us: Option<f64>,
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
}

impl LiveReading {
    /// Enough clipped beats to doubt the amplitude and beat error, by the
    /// same threshold as `analysis::Clipping::warns`.
    pub fn clipping_warns(&self) -> bool {
        self.clipped_fraction
            .is_some_and(|f| f > Clipping::WARN_FRACTION)
    }
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
        a.beats = log.beats;
        a.amp = log.amplitude_windows;
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
        self.beats.clear();
        self.clipped.clear();
        self.amp.clear();
        self.next_amp_s = None;
        self.profiles = [None, None];
        self.snippets.clear();
        self.settled_to = self.duration_s();
        self.quiet_passes = 0;
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
                if snr(&env, fs, &b) >= self.cfg.min_snr && b.len() >= 8 {
                    self.template = Some(t);
                    self.bph = Some(bph);
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
        if s < self.cfg.min_snr || self.template.is_none() {
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
        let span_s = if lo < hi {
            end_s - self.beats[lo].time.max(from)
        } else {
            0.0
        };
        let clipped_beats = self.clipped.partition_point(|&t| t <= end_s)
            - self
                .clipped
                .partition_point(|&t| t < self.beats.get(lo).map_or(end_s, |b| b.time));
        let fit = self
            .bph
            .and_then(|bph| timing::fit(&self.beats[lo..hi], bph));
        let a0 = self.amp.partition_point(|w| w.end_s <= from);
        let a1 = self.amp.partition_point(|w| w.end_s <= end_s + 1e-9);
        let wins: Vec<&AmplitudeWindow> = self.amp[a0..a1.max(a0)].iter().collect();
        let med = |f: &dyn Fn(&AmplitudeWindow) -> Option<f64>| -> Option<f64> {
            let mut v: Vec<f64> = wins.iter().filter_map(|w| f(w)).collect();
            (!v.is_empty()).then(|| median(&mut v))
        };
        LiveReading {
            time_s: end_s,
            bph: self.bph,
            rate_s_per_day: fit.map(|f| f.rate_s_per_day),
            beat_error_ms: fit.map(|f| f.beat_error_ms),
            beat_error_unlock_ms: med(&|w| w.beat_error_unlock_ms),
            amplitude_deg: med(&|w| w.mean()),
            amplitude_even_deg: med(&|w| w.even_deg),
            amplitude_odd_deg: med(&|w| w.odd_deg),
            jitter_us: fit.map(|f| f.jitter_us),
            beats_used: fit.map_or(0, |f| f.beats_used),
            span_s,
            snr: self.snr,
            clipped_beats,
            clipped_fraction: (hi > lo).then(|| clipped_beats as f64 / (hi - lo) as f64),
        }
    }
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
        assert!(!clean.clipping_warns());

        // The next 20 s 12 dB too hot, clipped at full scale.
        let peak = audio.samples.iter().fold(0f32, |m, v| m.max(v.abs()));
        let hot: Vec<f32> = audio
            .samples
            .iter()
            .map(|v| (v * 4.0 / peak).clamp(-1.0, 1.0))
            .collect();
        feed(&mut a, &hot);
        let r = a.reading(10.0);
        assert!(r.clipping_warns(), "{r:?}");
        assert!(r.clipped_fraction.unwrap() > 0.5, "{r:?}");
        // A reading back in the clean stretch carries no warning.
        assert!(!a.reading_at(15.0, 10.0).clipping_warns());
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
