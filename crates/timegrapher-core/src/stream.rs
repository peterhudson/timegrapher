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
}

struct State {
    fs: f64,
    cfg: StreamConfig,
    template: Option<Vec<f32>>,
    bph: u32,
    beats: Vec<Beat>,
    amp: Vec<AmplitudeWindow>,
    /// Next amplitude window start, seconds.
    next_amp_s: f64,
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
        let numbered: Vec<Beat> = local
            .iter()
            .map(|b| Beat {
                index: b.index + shift,
                ..*b
            })
            .collect();
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
    let fs = info.sample_rate as f64;
    let core = (cfg.chunk_s * fs).round() as usize;
    let margin = (cfg.margin_s * fs).round() as usize;
    let mut st = State {
        fs,
        cfg: cfg.clone(),
        template: None,
        bph: cfg.analysis.bph.unwrap_or(0),
        beats: Vec::new(),
        amp: Vec::new(),
        next_amp_s: 0.0,
    };
    // `buf` holds samples from absolute index `buf0`; the next chunk's own
    // stretch starts at `next`.
    let mut buf: Vec<f32> = Vec::new();
    let mut buf0 = 0usize;
    let mut next = 0usize;
    let mut total = 0usize;
    for path in paths {
        audio::stream(path, 1 << 16, |block| {
            buf.extend_from_slice(block);
            total += block.len();
            while buf0 + buf.len() >= next + core + margin {
                let from = next.saturating_sub(margin);
                let x = &buf[from - buf0..next + core + margin - buf0];
                st.chunk(x, from, next as f64 / fs, (next + core) as f64 / fs);
                next += core;
                let keep_from = next.saturating_sub(margin);
                buf.drain(..keep_from - buf0);
                buf0 = keep_from;
                progress(next as f64 / fs);
            }
        })?;
    }
    if total > next {
        let from = next.saturating_sub(margin);
        st.chunk(&buf[from - buf0..], from, next as f64 / fs, f64::INFINITY);
    }
    progress(total as f64 / fs);
    Ok(BeatLog {
        sample_rate: info.sample_rate,
        duration_s: total as f64 / fs,
        bph: st.bph,
        lift_deg: cfg.analysis.amplitude.lift_deg,
        beats: st.beats,
        amplitude_windows: st.amp,
    })
}
