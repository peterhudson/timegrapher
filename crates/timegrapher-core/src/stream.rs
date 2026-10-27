//! Chunked analysis of recordings too long to hold in memory.
//!
//! The recording is decoded block by block and analysed in chunks with a
//! little overlap on each side. The beat template is built once, from the
//! first chunk, and used for every later chunk, so the beat's reference
//! point stays put across chunk boundaries; otherwise the boundaries would
//! show up as a fake periodic component at the chunk length. Beat numbers
//! run on across chunks, so tick and toc keep their sides for the whole run.

use crate::amplitude::{self, AmplitudeWindow};
use crate::analysis::AnalysisConfig;
use crate::audio::{self, AudioError};
use crate::beats::{self, Beat};
use crate::dsp::envelope;
use crate::profile;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct StreamConfig {
    pub analysis: AnalysisConfig,
    /// Length of each chunk's own stretch of the recording, seconds. Not a
    /// divisor of 60 s, so any boundary effect cannot pose as the fourth wheel.
    pub chunk_s: f64,
    /// Extra audio on each side of a chunk, seconds.
    pub margin_s: f64,
}

impl Default for StreamConfig {
    fn default() -> Self {
        StreamConfig {
            analysis: AnalysisConfig::default(),
            chunk_s: 47.0,
            margin_s: 1.0,
        }
    }
}

/// Every beat of a recording, and the amplitude in short windows.
/// Times are seconds from the start of the recording on the sound card's clock.
#[derive(Debug, Clone, Serialize)]
pub struct BeatLog {
    pub sample_rate: u32,
    pub duration_s: f64,
    pub bph: u32,
    pub lift_deg: f64,
    pub beats: Vec<Beat>,
    pub amplitude_windows: Vec<AmplitudeWindow>,
    /// Times of the beats with a clipped sample within their template
    /// window, as [`crate::analysis::Clipping`] counts them.
    #[serde(skip_serializing)]
    pub clipped_beats: Vec<f64>,
    /// Clipped samples in the whole recording.
    pub clipped_samples: usize,
    /// Audio seconds where the beats jumped ([`beats::jumps`]): listening
    /// stopped and started again there, whether or not a break is listed.
    /// [`crate::breaks::with_jumps`] makes them breaks for the readings.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub jumps: Vec<f64>,
    /// Of `jumps`, those where the side sounds told Tick from Tock after
    /// the jump, so the beat numbers there already keep the sides; at the
    /// others the count across the hole is a guess.
    #[serde(skip)]
    pub sided: Vec<f64>,
    /// The breaks, audio seconds, whose sides are named by the beat count
    /// alone ([`crate::breaks::by_count`]): the jumps not in `sided`, or
    /// after [`crate::breaks::renumber`] the breaks it returns.
    #[serde(skip)]
    pub by_count: Vec<f64>,
    /// Each clean beat's envelope window ([`crate::profile::snippet`]) with
    /// the beat's time, for the beats in the first [`SNIPPETS_KEPT_S`] of
    /// audio, so that a stretch there too short for its timing to be
    /// assessed can be looked at by its sound ([`BeatLog::beat_sound`]).
    #[serde(skip)]
    pub snippets: Vec<(f64, Vec<f32>)>,
}

/// Seconds of audio, from the start, whose beats keep their envelope
/// windows in [`BeatLog::snippets`]: enough for a reading after a settling
/// time, few enough to cost little memory (about 2.6 MB at 28,800 bph and
/// 48 kHz).
pub const SNIPPETS_KEPT_S: f64 = 60.0;

/// Beats kept so far that a chunk looks back through for the good beats
/// before a jump at its start: about four minutes at 28,800 bph, longer
/// than the tracker follows noise after a dropout.
const JUMP_LOOK_BACK: usize = 2000;

impl BeatLog {
    /// [`crate::measure::beat_sound_found`] for `beats`, a stretch's
    /// beats as this log holds them (audio time): the profiles of their
    /// kept envelope windows, Tick from the even beats. `None` when the
    /// stretch reaches past [`SNIPPETS_KEPT_S`] or holds too few clean
    /// beats for a profile.
    pub fn beat_sound(
        &self,
        beats: &[Beat],
        cfg: &crate::amplitude::AmplitudeConfig,
    ) -> Option<bool> {
        if beats.last().is_some_and(|b| b.time >= SNIPPETS_KEPT_S) {
            return None;
        }
        let find = |t: f64| {
            let k = self.snippets.partition_point(|s| s.0 < t - 1e-9);
            self.snippets
                .get(k)
                .filter(|s| (s.0 - t).abs() < 1e-9)
                .map(|s| s.1.as_slice())
        };
        let snips = beats
            .iter()
            .filter(|b| b.quality > 0.4)
            .filter_map(|b| Some((b.index.rem_euclid(2) == 0, find(b.time)?)));
        let beat = 3600.0 / self.bph.max(1) as f64;
        crate::measure::beat_sound_found(snips, self.sample_rate as f64, 2.0 * beat, cfg)
    }
}

struct State {
    fs: f64,
    cfg: StreamConfig,
    template: Option<Vec<f32>>,
    /// Tick and Tock told apart across each jump by their sound, as live
    /// listening tells them.
    sides: beats::SideCheck,
    jumps: Vec<f64>,
    bph: u32,
    beats: Vec<Beat>,
    amp: Vec<AmplitudeWindow>,
    /// Next amplitude window start, seconds.
    next_amp_s: f64,
    clipped_beats: Vec<f64>,
    clipped_samples: usize,
    snippets: Vec<(f64, Vec<f32>)>,
}

impl State {
    /// Analyse `x`, which starts at sample `x0`, keeping the beats whose
    /// time falls in `[core_a, core_b)` (seconds).
    fn chunk(&mut self, x: &[f32], x0: usize, core_a: f64, core_b: f64) {
        let fs = self.fs;
        let a = &self.cfg.analysis;
        let env = envelope(x, fs, &a.envelope);
        let local = match &self.template {
            Some(t) => beats::detect_with_template(&env, fs, self.bph, t),
            None => {
                self.bph = a.bph.unwrap_or_else(|| beats::guess_bph(&env, fs));
                let (b, t) = beats::detect(&env, fs, self.bph);
                self.template = Some(t);
                b
            }
        };
        let off = x0 as f64 / fs;
        let beat = 3600.0 / self.bph as f64;

        // Number the chunk's beats on from the last beat kept so far.
        let Some(anchor) = local
            .iter()
            .position(|b| b.time + off >= core_a && b.time + off < core_b)
        else {
            return;
        };
        let (mut numbered, _) = beats::number_on(
            &local,
            anchor,
            local[anchor].time + off,
            self.beats.last(),
            beat,
        );
        // Jumps in the timing, with the beats just before the chunk so one
        // at its start is seen too.
        let end = numbered[anchor..]
            .iter()
            .position(|b| b.time + off >= core_b)
            .map_or(numbered.len(), |p| anchor + p);
        // The good beats before the chunk, reaching back past a stretch of
        // noise beats after a dropout ([`beats::jumps_among_good`]).
        let back = &self.beats[self.beats.len().saturating_sub(JUMP_LOOK_BACK)..];
        let before: Vec<Beat> = back[beats::good_tail_start(back, beats::JUMP_CONTEXT)..]
            .iter()
            .map(|b| Beat {
                time: b.time - off,
                ..*b
            })
            .collect();
        let cuts: Vec<usize> = beats::jumps_among_good(&numbered[anchor..end], &before)
            .into_iter()
            .map(|p| p + anchor)
            .collect();
        let kept_from = self.beats.len();
        for k in anchor..end {
            let b = numbered[k];
            let t = b.time + off;
            // Seen again from the next chunk: the same jump.
            if cuts.contains(&k) && !self.jumps.last().is_some_and(|&j| t - j <= 1.0) {
                self.jumps.push(t);
                self.sides.jump(t);
            }
            if let Some(last) = self.beats.last() {
                if t - last.time < 0.5 * beat || b.index <= last.index {
                    continue;
                }
            }
            self.beats.push(Beat { time: t, ..b });
            if let Some(w) = profile::snippet(&env, fs, b.time) {
                if t < SNIPPETS_KEPT_S && b.quality > 0.4 {
                    self.snippets.push((t, w.to_vec()));
                }
                self.sides.beat(t, b.index, b.quality, w);
            }
            if let Some((j, beats::SideCall::Swapped)) = self.sides.settle() {
                swap_sides_from(j, &mut self.beats, &mut self.amp);
                swap_sides_from(j - off, &mut numbered, &mut []);
            }
        }

        // Clipping, counted on this chunk's own stretch.
        let idx = crate::diagnose::clipped_indices(x, crate::analysis::CLIP_LEVEL);
        if !idx.is_empty() {
            let at = |i: usize| off + i as f64 / fs;
            self.clipped_samples += idx
                .iter()
                .filter(|&&i| at(i) >= core_a && at(i) < core_b)
                .count();
            let (pre, post) = (beats::PRE_S, beats::POST_S);
            for b in &self.beats[kept_from..] {
                let lo = ((b.time - pre - off) * fs).floor().max(0.0) as usize;
                let hi = ((b.time + post - off) * fs).ceil().max(0.0) as usize;
                let k = idx.partition_point(|&i| i < lo);
                if idx.get(k).is_some_and(|&i| i <= hi) {
                    self.clipped_beats.push(b.time);
                }
            }
        }

        // Amplitude on a fixed grid of windows starting inside this chunk.
        let w = a.amplitude_window_s;
        let x_end = off + x.len() as f64 / fs;
        let mut to = self.next_amp_s;
        while to < core_b && to + w <= x_end {
            to += w;
        }
        if to > self.next_amp_s {
            let wins = amplitude::windows_between(
                &env,
                fs,
                &numbered,
                2.0 * beat,
                w,
                self.next_amp_s - off,
                to - off,
                &a.amplitude,
            );
            self.amp.extend(wins.into_iter().map(|mut win| {
                win.start_s += off;
                win.end_s += off;
                win
            }));
            self.next_amp_s = to;
        }
    }
}

/// Number every beat from `t` seconds on one further, which swaps Tick
/// and Tock back after a jump ([`beats::SideCheck`]), and swap the sides of
/// the amplitude windows from there.
fn swap_sides_from(t: f64, beats: &mut [Beat], amp: &mut [AmplitudeWindow]) {
    for b in beats.iter_mut().filter(|b| b.time >= t) {
        b.index += 1;
    }
    for w in amp.iter_mut().filter(|w| w.start_s >= t) {
        w.swap_sides();
    }
}

/// Analyse a WAV or FLAC file of any length into a beat log. `progress`
/// is called with the seconds of audio processed so far.
pub fn analyze_file(
    path: &Path,
    cfg: &StreamConfig,
    progress: impl FnMut(f64),
) -> Result<BeatLog, AudioError> {
    analyze_files(&[path], cfg, progress)
}

/// Analyse several files as one continuous recording, in the order given
/// (a long capture split into segments with no gaps between them).
pub fn analyze_files(
    paths: &[&Path],
    cfg: &StreamConfig,
    mut progress: impl FnMut(f64),
) -> Result<BeatLog, AudioError> {
    let Some(first) = paths.first() else {
        return Err(AudioError::Unsupported("no files given".into()));
    };
    let info = audio::info(first)?;
    for p in &paths[1..] {
        let i = audio::info(p)?;
        if i.sample_rate != info.sample_rate {
            return Err(AudioError::Unsupported(format!(
                "{} is at {} Hz but {} is at {} Hz",
                p.display(),
                i.sample_rate,
                first.display(),
                info.sample_rate
            )));
        }
    }
    let mut c = Chunker::new(info.sample_rate, cfg);
    for path in paths {
        audio::stream(path, 1 << 16, |block| {
            c.push(block);
            progress(c.next as f64 / c.st.fs);
        })?;
    }
    progress(c.total as f64 / c.st.fs);
    Ok(c.finish())
}

/// Analyse a recording held in memory into a beat log, chunk by chunk
/// exactly as [`analyze_files`] reads a file, so that a reading of a file
/// and of the same audio in memory rest on the same beats.
pub fn analyze_samples(samples: &[f32], sample_rate: u32, cfg: &StreamConfig) -> BeatLog {
    let mut c = Chunker::new(sample_rate, cfg);
    for block in samples.chunks(1 << 16) {
        c.push(block);
    }
    c.finish()
}

/// The chunked reading of one recording: audio goes in in blocks of any
/// size, beats come out on a fixed grid of chunks.
struct Chunker {
    st: State,
    core: usize,
    margin: usize,
    /// `buf` holds samples from absolute index `buf0`; the next chunk's own
    /// stretch starts at `next`.
    buf: Vec<f32>,
    buf0: usize,
    next: usize,
    total: usize,
    sample_rate: u32,
}

impl Chunker {
    fn new(sample_rate: u32, cfg: &StreamConfig) -> Self {
        let fs = sample_rate as f64;
        Chunker {
            st: State {
                fs,
                cfg: cfg.clone(),
                template: None,
                sides: beats::SideCheck::default(),
                jumps: Vec::new(),
                bph: cfg.analysis.bph.unwrap_or(0),
                beats: Vec::new(),
                amp: Vec::new(),
                next_amp_s: 0.0,
                clipped_beats: Vec::new(),
                clipped_samples: 0,
                snippets: Vec::new(),
            },
            core: (cfg.chunk_s * fs).round() as usize,
            margin: (cfg.margin_s * fs).round() as usize,
            buf: Vec::new(),
            buf0: 0,
            next: 0,
            total: 0,
            sample_rate,
        }
    }

    fn push(&mut self, block: &[f32]) {
        let (core, margin) = (self.core, self.margin);
        self.buf.extend_from_slice(block);
        self.total += block.len();
        while self.buf0 + self.buf.len() >= self.next + core + margin {
            let from = self.next.saturating_sub(margin);
            let x = &self.buf[from - self.buf0..self.next + core + margin - self.buf0];
            let fs = self.st.fs;
            self.st.chunk(
                x,
                from,
                self.next as f64 / fs,
                (self.next + core) as f64 / fs,
            );
            self.next += core;
            let keep_from = self.next.saturating_sub(margin);
            self.buf.drain(..keep_from - self.buf0);
            self.buf0 = keep_from;
        }
    }

    fn finish(mut self) -> BeatLog {
        let fs = self.st.fs;
        if self.total > self.next {
            let from = self.next.saturating_sub(self.margin);
            let x = &self.buf[from - self.buf0..];
            self.st.chunk(x, from, self.next as f64 / fs, f64::INFINITY);
        }
        let st = self.st;
        BeatLog {
            sample_rate: self.sample_rate,
            duration_s: self.total as f64 / fs,
            bph: st.bph,
            lift_deg: st.cfg.analysis.amplitude.lift_deg,
            beats: st.beats,
            amplitude_windows: {
                let mut amp = st.amp;
                amplitude::reject_side_outliers(&mut amp, amplitude::OUTLIER_SPAN_S);
                amp
            },
            clipped_beats: st.clipped_beats,
            clipped_samples: st.clipped_samples,
            by_count: crate::breaks::by_count(st.jumps.iter().copied(), st.sides.sided()),
            jumps: st.jumps,
            sided: st.sides.sided().to_vec(),
            snippets: st.snippets,
        }
    }
}
