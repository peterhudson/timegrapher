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
}

struct State {
    fs: f64,
    cfg: StreamConfig,
    template: Option<Vec<f32>>,
    /// The sound of each side so far, to tell the sides apart after a jump.
    sides: Option<beats::SideSounds>,
    jumps: Vec<f64>,
    /// The last jump's stretch was too short to tell its sides: check
    /// the start of the next chunk instead.
    recheck: bool,
    bph: u32,
    beats: Vec<Beat>,
    amp: Vec<AmplitudeWindow>,
    /// Next amplitude window start, seconds.
    next_amp_s: f64,
    clipped_beats: Vec<f64>,
    clipped_samples: usize,
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
        let mut numbered: Vec<Beat> = local
            .iter()
            .map(|b| Beat {
                index: b.index + shift,
                ..*b
            })
            .collect();
        match_sides(
            &mut self.sides,
            &mut self.jumps,
            &mut self.recheck,
            &self.beats[self.beats.len().saturating_sub(beats::JUMP_CONTEXT)..],
            fs,
            &env,
            off,
            &mut numbered,
            anchor,
            core_b,
        );
        let kept_from = self.beats.len();
        for b in &numbered[anchor..] {
            let t = b.time + off;
            if t >= core_b {
                break;
            }
            if let Some(last) = self.beats.last() {
                if t - last.time < 0.5 * beat || b.index <= last.index {
                    continue;
                }
            }
            self.beats.push(Beat { time: t, ..*b });
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

/// Keep Tick and Tock on their own sides across a jump in the beats.
///
/// Counting beats names the sides only while the beats run on without
/// a jump. Where listening stopped and started again (a pause, the
/// computer suspended, a join between two takes) the count across the
/// hole is a guess, and half the time it swaps the sides. So at each
/// jump, and at the start of a chunk that follows one, the side sounds
/// after it are compared with those before ([`beats::side_margin`]);
/// when they match the other way round, the beats from there on are
/// numbered one further, which swaps their sides back. A stretch too
/// short or too alike on both sides to tell keeps its count.
#[allow(clippy::too_many_arguments)]
fn match_sides(
    sides: &mut Option<beats::SideSounds>,
    jumps: &mut Vec<f64>,
    recheck: &mut bool,
    last: &[Beat],
    fs: f64,
    env: &[f32],
    off: f64,
    numbered: &mut [Beat],
    anchor: usize,
    core_b: f64,
) {
    let end = numbered[anchor..]
        .iter()
        .position(|b| b.time + off >= core_b)
        .map_or(numbered.len(), |p| anchor + p);
    if end <= anchor {
        return;
    }
    let before: Vec<Beat> = last
        .iter()
        .map(|b| Beat {
            time: b.time - off,
            ..*b
        })
        .collect();
    let cuts: Vec<usize> = beats::jumps(&numbered[anchor..end], &before)
        .into_iter()
        .map(|p| p + anchor)
        .collect();
    let join = cuts.first() == Some(&anchor) || std::mem::take(recheck);
    for &c in &cuts {
        // Seen again from the next chunk: the same jump.
        let t = numbered[c].time + off;
        if !jumps.last().is_some_and(|&j| t - j <= 1.0) {
            jumps.push(t);
        }
    }
    let mut starts = vec![anchor];
    starts.extend(cuts.iter().copied().filter(|&c| c > anchor));
    let mut reference = sides.clone();
    let mut sure = true;
    for (i, &s) in starts.iter().enumerate() {
        let e = starts.get(i + 1).copied().unwrap_or(end);
        let now = beats::side_sounds(env, fs, &numbered[s..e]);
        let Some(r) = reference.as_ref().filter(|_| i > 0 || join) else {
            // Before any jump: the count names the sides.
            if reference.is_none() {
                reference = now;
            }
            continue;
        };
        match now.map(|now| beats::side_margin(r, &now)) {
            Some(m) if m <= -beats::SIDE_MARGIN_MIN => {
                for b in &mut numbered[s..] {
                    b.index += 1;
                }
            }
            Some(m) if m >= beats::SIDE_MARGIN_MIN => {}
            Some(_) => sure = false,
            None => {
                sure = false;
                // Too few beats here: the next stretch, whatever chunk it
                // is in, still owes the check.
                *recheck = i + 1 == starts.len();
            }
        }
    }
    // Keep the side sounds up to date, from beats whose sides are known.
    if sure {
        if let Some(now) = beats::side_sounds(env, fs, &numbered[anchor..end]) {
            match sides {
                Some(s) => s.blend(&now),
                None => *sides = Some(now),
            }
        }
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
                sides: None,
                jumps: Vec::new(),
                recheck: false,
                bph: cfg.analysis.bph.unwrap_or(0),
                beats: Vec::new(),
                amp: Vec::new(),
                next_amp_s: 0.0,
                clipped_beats: Vec::new(),
                clipped_samples: 0,
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
            jumps: st.jumps,
        }
    }
}
