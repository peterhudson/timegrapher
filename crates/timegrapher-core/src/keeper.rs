//! A take kept as its analysis while it is recorded, its sound never
//! written (#1514). The sound passes through once, block by block: the
//! beats are found as a saved take's file reading finds them
//! ([`stream`]'s chunks), and each stretch's Tick Tock profiles, the
//! USB-line clock and the beat shape are read from a short buffer behind
//! the beats, as [`analysis_take::measure`]'s second reading reads them
//! from the file. One [`SecondReading`] does that work for both, so a
//! take kept while it is recorded is kept exactly as one kept after.

use crate::analysis_take::{
    self, measure_shape, Stored, StoredShape, StoredUsbClock, StretchProfiles, PROFILE_EVERY_S,
};
use crate::audio::AudioError;
use crate::beats::Beat;
use crate::checkpoint::{self, Checkpointed};
use crate::dsp::envelope;
use crate::profile;
use crate::stream::{self, BeatLog, StreamConfig};
use crate::usbclock;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};

/// Audio either side of a profile's stretch, for its beats' windows, as
/// the app's profile pane reads it.
const PROFILE_MARGIN_S: f64 = 0.05;

/// The most sound, seconds, held back unprofiled before the watch is
/// first heard, when the beat rate may still change
/// ([`stream::Seen::heard`]). Past it the profiles are read on, so a watch
/// never heard can't hold back the whole take; but stretches read before
/// a watch heard only after 10 min of sound are read at the rate guessed
/// then, which can differ from the rate the whole reading settles on.
const MAX_HELD_S: f64 = 600.0;

/// Blocks of sound waiting for the Keeper's reading: enough to ride out a
/// slow moment, few enough to hold little.
const QUEUED_BLOCKS: usize = 256;

/// [`analysis_take::measure`]'s second reading, fed the sound in order:
/// the profiles of each [`PROFILE_EVERY_S`] stretch once its beats are
/// known, the first [`usbclock::MAX_DURATION_S`] for the USB-line clock,
/// and each recording's shape stretch.
pub(crate) struct SecondReading {
    fs: f64,
    margin: usize,
    step: usize,
    usb_keep: usize,
    usb: Vec<f32>,
    /// Each shape stretch, first and last sample, and its sound so far.
    cuts: Vec<(usize, usize, Vec<f32>)>,
    /// The sound from sample `buf0` on, back to the next stretch's margin.
    buf: Vec<f32>,
    buf0: usize,
    /// Samples read.
    pos: usize,
    /// The next stretch to profile, and the first beat it may hold.
    k: usize,
    next_beat: usize,
    profiles: Vec<StretchProfiles>,
    /// Each profile's stretch, counted from 0.
    stretches: Vec<usize>,
    /// The stretch profiled past a jump whose sides were still to be told
    /// apart: the jump's time, the profile's place and its stretch, and the
    /// stretch's sound from sample `lo`, to read again if they swap.
    held: Option<Held>,
}

/// [`SecondReading::held`].
struct Held {
    jump_s: f64,
    at: usize,
    k: usize,
    lo: usize,
    x: Vec<f32>,
}

impl SecondReading {
    pub(crate) fn new(sample_rate: u32, shapes: &[Option<(f64, f64)>]) -> Self {
        let fs = f64::from(sample_rate);
        SecondReading {
            fs,
            margin: (PROFILE_MARGIN_S * fs).round() as usize,
            step: (PROFILE_EVERY_S * fs).round() as usize,
            usb_keep: (usbclock::MAX_DURATION_S * fs) as usize,
            usb: Vec::new(),
            cuts: shapes
                .iter()
                .flatten()
                .map(|&(a, b)| ((a * fs) as usize, (b * fs) as usize, Vec::new()))
                .collect(),
            buf: Vec::new(),
            buf0: 0,
            pos: 0,
            k: 0,
            next_beat: 0,
            profiles: Vec::new(),
            stretches: Vec::new(),
            held: None,
        }
    }

    /// The samples after those read so far.
    pub(crate) fn audio(&mut self, block: &[f32]) {
        let (s, e) = (self.pos, self.pos + block.len());
        if self.usb.len() < self.usb_keep {
            let take = (self.usb_keep - self.usb.len()).min(block.len());
            self.usb.extend_from_slice(&block[..take]);
        }
        for (a, b, v) in &mut self.cuts {
            if e > *a && s < *b {
                v.extend_from_slice(&block[(*a).max(s) - s..(*b).min(e) - s]);
            }
        }
        self.buf.extend_from_slice(block);
        self.pos = e;
    }

    /// Samples read so far.
    pub(crate) fn read(&self) -> usize {
        self.pos
    }

    /// The stretches profiled so far, and the time, seconds, every
    /// stretch ending by it is profiled to (one without beats has none).
    fn profiled(&self) -> (&[StretchProfiles], f64) {
        (&self.profiles, self.start_s(self.k))
    }

    /// Profile every stretch whose sound and margin are in and that ends
    /// by sample `until`, with `beats` (audio time) at `bph`; one that holds
    /// the jump at `pending` seconds, whose sides may yet swap, is kept to
    /// be read again ([`SecondReading::swap_from`]).
    pub(crate) fn profile_until(
        &mut self,
        beats: &[Beat],
        bph: u32,
        cfg: &StreamConfig,
        (until, pending): (usize, Option<f64>),
    ) {
        while self.buf0 + self.buf.len() >= (self.k + 1) * self.step + self.margin
            && (self.k + 1) * self.step <= until
        {
            let (k, lo) = (self.k, (self.k * self.step).saturating_sub(self.margin));
            let read = self.profile_next(beats, bph, cfg, f64::INFINITY);
            let (from, to) = (self.start_s(k), self.start_s(k + 1));
            if let Some(t) = pending.filter(|&t| read && from <= t && t < to) {
                let lo = lo.max(self.buf0);
                let hi = ((k + 1) * self.step + self.margin).min(self.buf0 + self.buf.len());
                self.held = Some(Held {
                    jump_s: t,
                    at: self.profiles.len() - 1,
                    k,
                    lo,
                    x: self.buf[lo - self.buf0..hi - self.buf0].to_vec(),
                });
            }
            let keep = (self.k * self.step).saturating_sub(self.margin);
            if keep > self.buf0 {
                self.buf.drain(..keep - self.buf0);
                self.buf0 = keep;
            }
        }
    }

    /// Stretch `k`'s start, seconds.
    fn start_s(&self, k: usize) -> f64 {
        (k * self.step) as f64 / self.fs
    }

    /// Stretch `k`'s profiles, its end no later than `duration_s`; then
    /// the next stretch. True when it had beats, so a profile was kept.
    fn profile_next(
        &mut self,
        beats: &[Beat],
        bph: u32,
        cfg: &StreamConfig,
        duration_s: f64,
    ) -> bool {
        let k = self.k;
        self.k += 1;
        let (a, b) = (k * self.step, (k + 1) * self.step);
        let lo = a.saturating_sub(self.margin).max(self.buf0);
        let hi = (b + self.margin).min(self.buf0 + self.buf.len());
        let from = self.start_s(k);
        while beats.get(self.next_beat).is_some_and(|bt| bt.time < from) {
            self.next_beat += 1;
        }
        let x = &self.buf[lo - self.buf0..hi - self.buf0];
        let beats = &beats[self.next_beat.min(beats.len())..];
        let Some(p) = self.stretch(x, lo, k, beats, bph, cfg, duration_s) else {
            return false;
        };
        self.profiles.push(p);
        self.stretches.push(k);
        true
    }

    /// The profiles of stretch `k`, its sound `x` from sample `lo`, read
    /// with `beats` from its first on; none when it holds no beat.
    #[allow(clippy::too_many_arguments)]
    fn stretch(
        &self,
        x: &[f32],
        lo: usize,
        k: usize,
        beats: &[Beat],
        bph: u32,
        cfg: &StreamConfig,
        duration_s: f64,
    ) -> Option<StretchProfiles> {
        let fs = self.fs;
        let to = self.start_s(k + 1);
        let x0 = lo as f64 / fs;
        let local: Vec<Beat> = beats
            .iter()
            .take_while(|bt| bt.time < to)
            .map(|bt| Beat {
                time: bt.time - x0,
                ..*bt
            })
            .collect();
        if local.is_empty() {
            return None;
        }
        let beat_s = 3600.0 / bph.max(1) as f64;
        let env = envelope(x, fs, &cfg.analysis.envelope);
        let sides = profile::profiles(
            &env,
            fs,
            &local,
            2.0 * beat_s,
            0.0,
            x.len() as f64 / fs,
            &cfg.analysis.amplitude,
        );
        Some(StretchProfiles {
            end_s: to.min(duration_s),
            signal: crate::live::signal_figure(&env, fs, &local),
            sides,
        })
    }

    /// The beats numbered one further from `t` seconds on (a side swap,
    /// [`stream::Seen::swaps`]; minus infinity for all of them), after
    /// their profiles were read: each stretch from `t` on has Tick and
    /// Tock the other way round, and the one `t` falls in, kept for this
    /// ([`SecondReading::profile_until`]), is read again with `beats`.
    fn swap_from(&mut self, t: f64, beats: &[Beat], bph: u32, cfg: &StreamConfig) {
        for (i, &k) in self.stretches.iter().enumerate() {
            if self.start_s(k) >= t {
                self.profiles[i].swap_sides();
            }
        }
        if let Some(h) = self.held.take_if(|h| h.jump_s == t) {
            let first = beats.partition_point(|b| b.time < self.start_s(h.k));
            let again = self.stretch(&h.x, h.lo, h.k, &beats[first..], bph, cfg, f64::INFINITY);
            if let Some(p) = again {
                self.profiles[h.at] = p;
            }
        }
    }

    /// Let go of the stretch kept for a jump other than `pending`: its
    /// sides were told apart, or it is no longer pending.
    fn release(&mut self, pending: Option<f64>) {
        self.held.take_if(|h| Some(h.jump_s) != pending);
    }

    /// The take kept: the stretches left profiled with the whole `log`,
    /// the beat shape and the USB-line clock measured, and the header
    /// written ([`analysis_take::from_log`]).
    pub(crate) fn finish(
        mut self,
        log: BeatLog,
        cfg: &StreamConfig,
        shapes: &[Option<(f64, f64)>],
        bytes_per_frame: u32,
        software: serde_json::Value,
    ) -> Stored {
        self.profile_until(&log.beats, log.bph, cfg, (usize::MAX, None));
        let n_total = (log.duration_s * self.fs).round() as usize;
        while self.k * self.step < n_total.min(self.buf0 + self.buf.len()) {
            self.profile_next(&log.beats, log.bph, cfg, log.duration_s);
        }
        let shape = shapes
            .iter()
            .enumerate()
            .filter_map(|(i, s)| Some((i, (*s)?)))
            .zip(self.cuts)
            .filter_map(|((recording, (from_s, to_s)), (_, _, x))| {
                let summary = measure_shape(&x, self.fs, log.bph, from_s)?;
                Some(StoredShape {
                    recording,
                    from_s,
                    to_s,
                    summary,
                })
            })
            .collect();
        let usb_clock = usbclock::measure(&self.usb, log.sample_rate).map(|c| StoredUsbClock {
            ppm: c.ppm,
            lines: c.lines,
            spread_ppm: c.spread_ppm,
        });
        let mut signals: Vec<f32> = self.profiles.iter().filter_map(|p| p.signal).collect();
        let signal_median = (!signals.is_empty()).then(|| crate::dsp::median_f32(&mut signals));
        // The beats' header, with what the second reading found.
        let mut stored = analysis_take::from_log(log, cfg, bytes_per_frame, software);
        stored.header.profiles = self.profiles.len();
        stored.header.usb_clock = usb_clock;
        stored.header.signal_median = signal_median;
        stored.header.shape = shape;
        stored.profiles = self.profiles;
        stored
    }
}

/// A take being kept as its analysis while it is recorded: [`Keeper::push`]
/// each block of sound as it comes, then [`Keeper::finish`] for what
/// [`analysis_take::save`] writes. Nothing of the sound is written. The
/// reading runs on threads of its own, so pushing only queues the block.
pub struct Keeper {
    tx: Option<mpsc::SyncSender<Vec<f32>>>,
    worker: Option<std::thread::JoinHandle<Result<Stored, AudioError>>>,
    checkpointed: Arc<Mutex<Checkpointed>>,
}

/// What a [`Keeper`] keeps, besides the sound it is given.
pub struct KeepAs {
    pub sample_rate: u32,
    /// Bytes a frame of the sound would take in its file, for reading
    /// `clock.csv`'s audio column ([`analysis_take::Header::bytes_per_frame`]).
    pub bytes_per_frame: u32,
    pub cfg: StreamConfig,
    /// Each recording's shape stretch ([`analysis_take::shape_stretches`]).
    pub shapes: Vec<Option<(f64, f64)>>,
    pub software: serde_json::Value,
    /// Where to keep the analysis on the disk as it is recorded, if
    /// anywhere ([`checkpoint`]).
    pub checkpoints: Option<Checkpoints>,
}

/// Parts of the analysis kept on the disk as it is recorded
/// ([`checkpoint`]): in the take folder `take`, one each `every_s` of
/// sound ([`checkpoint::EVERY_S`]).
pub struct Checkpoints {
    pub take: PathBuf,
    pub every_s: f64,
}

impl Keeper {
    /// Start keeping a take recorded as `how` says.
    pub fn start(how: KeepAs) -> Keeper {
        let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(QUEUED_BLOCKS);
        let checkpointed = Arc::new(Mutex::new(Checkpointed::default()));
        let status = checkpointed.clone();
        let worker = std::thread::spawn(move || keep(&how, rx, status));
        Keeper {
            tx: Some(tx),
            worker: Some(worker),
            checkpointed,
        }
    }

    /// How far the analysis is kept on the disk ([`KeepAs::checkpoints`]).
    pub fn checkpointed(&self) -> Checkpointed {
        self.checkpointed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// The next block of sound. False once the reading has stopped (it
    /// failed), when [`Keeper::finish`] says why.
    pub fn push(&self, block: &[f32]) -> bool {
        self.tx
            .as_ref()
            .is_some_and(|tx| tx.send(block.to_vec()).is_ok())
    }

    /// The take kept, once every block pushed is read.
    pub fn finish(mut self) -> Result<Stored, AudioError> {
        drop(self.tx.take());
        self.worker
            .take()
            .expect("a Keeper finishes once")
            .join()
            .map_err(|_| AudioError::Unsupported("the analysis stopped unexpectedly".into()))?
    }
}

impl Drop for Keeper {
    fn drop(&mut self) {
        // Let the reading end, and wait for it, so no thread outlives it.
        drop(self.tx.take());
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

/// The sample the Keeper may profile up to, once it has `read` samples
/// and the chunk just read starts `core_a` seconds in. Up to that chunk's
/// start: its beats from there on may still change with the next chunk's
/// (a beat is only ever kept at or after a chunk's start,
/// [`beats::anchor_of`]); before it every beat is kept for good, but for
/// a side swap, which [`SecondReading::swap_from`] makes on the profiles.
/// Nothing before the watch is `heard`, while its rate may change, unless
/// that would hold back more than [`MAX_HELD_S`] of sound.
fn profile_bound(core_a: f64, heard: bool, read: usize, fs: f64) -> usize {
    let behind = (core_a * fs).max(0.0) as usize;
    let held_to = read.saturating_sub((MAX_HELD_S * fs) as usize);
    let wait = if heard { usize::MAX } else { 0 };
    behind.min(wait.max(held_to))
}

/// The Keeper's reading: the blocks from `rx`, until it closes, read as
/// one recording.
fn keep(
    how: &KeepAs,
    rx: mpsc::Receiver<Vec<f32>>,
    status: Arc<Mutex<Checkpointed>>,
) -> Result<Stored, AudioError> {
    let cfg = &how.cfg;
    let fs = f64::from(how.sample_rate);
    let mut second = SecondReading::new(how.sample_rate, &how.shapes);
    let mut parts = how.checkpoints.as_ref().map(|c| {
        checkpoint::Writer::new(checkpoint::Writing {
            take: &c.take,
            every_s: c.every_s,
            sample_rate: how.sample_rate,
            bytes_per_frame: how.bytes_per_frame,
            cfg,
            software: how.software.clone(),
            status,
        })
    });
    let mut swaps = 0;
    let feed = move |push: &mut dyn FnMut(&[f32]) -> bool| {
        for block in rx {
            if !push(&block) {
                break;
            }
        }
        Ok(())
    };
    let log = stream::analyze_fed_seen(
        how.sample_rate,
        cfg,
        stream::prepare_threads(),
        feed,
        |_| {},
        |seen| {
            // The chunks overlap: only the sound after what was read.
            let from = second.read().saturating_sub(seen.x0).min(seen.x.len());
            second.audio(&seen.x[from..]);
            for &(_, t) in &seen.swaps[swaps..] {
                second.swap_from(t, seen.beats, seen.bph, cfg);
            }
            swaps = seen.swaps.len();
            second.release(seen.pending);
            let until = profile_bound(seen.core_a, seen.heard, second.read(), fs);
            second.profile_until(seen.beats, seen.bph, cfg, (until, seen.pending));
            if let Some(parts) = parts.as_mut() {
                let (profiles, to_s) = second.profiled();
                parts.after_chunk(&seen, profiles, to_s);
            }
        },
    )?;
    Ok(second.finish(
        log,
        cfg,
        &how.shapes,
        how.bytes_per_frame,
        how.software.clone(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis_take::{self, files, measure};
    use crate::audio::{self, Audio};
    use crate::synth::{generate, SynthConfig};
    use std::path::{Path, PathBuf};

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tg-keeper-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A watch's sound, `seconds` long, written as a WAV in `dir` in
    /// `parts` files one after the other, as a long take splits.
    fn take(dir: &Path, seconds: f64, parts: usize) -> Vec<PathBuf> {
        take_after(dir, 0.0, seconds, parts, SynthConfig::default().bph)
    }

    /// [`take`], after `lead_s` seconds of noise and clicks at no steady
    /// rate, which no watch is heard in.
    fn take_after(dir: &Path, lead_s: f64, seconds: f64, parts: usize, bph: u32) -> Vec<PathBuf> {
        let mut audio = generate(
            &SynthConfig {
                bph,
                duration_s: seconds,
                beat_error_ms: 0.6,
                odd_gains: Some([0.9, 0.25, 1.0]),
                ..Default::default()
            },
            |t| 270.0 + 10.0 * (t / 10.0).sin(),
            |_| 0.0,
        );
        let mut seed = 12_345u32;
        let mut lead: Vec<f32> = (0..(lead_s * f64::from(audio.sample_rate)) as usize)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (f64::from(seed >> 8) / f64::from(1u32 << 24) - 0.5) as f32 * 0.004
            })
            .collect();
        // A click every 80 to 240 ms.
        let mut at = 0usize;
        while at + 200 < lead.len() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            for (i, v) in lead[at..at + 200].iter_mut().enumerate() {
                *v += 0.3 * (-(i as f32) / 40.0).exp() * if i % 2 == 0 { 1.0 } else { -1.0 };
            }
            let gap_s = 0.08 + 0.16 * f64::from(seed >> 8) / f64::from(1u32 << 24);
            at += (gap_s * f64::from(audio.sample_rate)) as usize;
        }
        lead.append(&mut audio.samples);
        audio.samples = lead;
        let n = audio.samples.len().div_ceil(parts);
        audio
            .samples
            .chunks(n)
            .enumerate()
            .map(|(i, x)| {
                let path = dir.join(format!("take_part01-{:03}.wav", i + 1));
                let part = Audio {
                    samples: x.to_vec(),
                    sample_rate: audio.sample_rate,
                };
                audio::write_wav(&path, &part).unwrap();
                path
            })
            .collect()
    }

    fn software() -> serde_json::Value {
        serde_json::json!({"name": "timegrapher", "version": "test"})
    }

    /// The take in `paths` kept by a [`Keeper`] fed its sound in blocks
    /// of the `sizes` given, over and over.
    fn kept_live(
        paths: &[PathBuf],
        cfg: &StreamConfig,
        shapes: &[Option<(f64, f64)>],
        sizes: &[usize],
    ) -> Stored {
        kept_with_parts(paths, cfg, shapes, sizes, None).0
    }

    /// [`kept_live`], its parts kept on the disk as `checkpoints` says,
    /// and how far they reached.
    fn kept_with_parts(
        paths: &[PathBuf],
        cfg: &StreamConfig,
        shapes: &[Option<(f64, f64)>],
        sizes: &[usize],
        checkpoints: Option<Checkpoints>,
    ) -> (Stored, Checkpointed) {
        let info = audio::info(&paths[0]).unwrap();
        let keeper = Keeper::start(KeepAs {
            sample_rate: info.sample_rate,
            bytes_per_frame: info.bytes_per_frame,
            cfg: cfg.clone(),
            shapes: shapes.to_vec(),
            software: software(),
            checkpoints,
        });
        let mut sound = Vec::new();
        for p in paths {
            audio::stream(p, 1 << 16, |b| sound.extend_from_slice(b)).unwrap();
        }
        let mut at = 0;
        for &n in sizes.iter().cycle() {
            if at >= sound.len() {
                break;
            }
            let end = (at + n).min(sound.len());
            assert!(keeper.push(&sound[at..end]));
            at = end;
        }
        let status = keeper.checkpointed.clone();
        let stored = keeper.finish().unwrap();
        let status = status.lock().unwrap().clone();
        (stored, status)
    }

    /// The files `analysis/` would hold, by name.
    fn kept_files(s: &Stored) -> Vec<(&'static str, Vec<u8>)> {
        files(s).unwrap()
    }

    /// The Keeper profiles up to the start of the chunk just read once the
    /// watch is heard; before, nothing, until more than 600 s of sound
    /// would be held, then all but the latest 600 s (never past the
    /// chunk's start).
    #[test]
    fn profiles_wait_for_the_chunk_and_for_the_watch() {
        let fs = 48_000.0;
        let at = |s: f64| (s * fs) as usize;
        assert_eq!(profile_bound(94.0, true, at(142.0), fs), at(94.0));
        assert_eq!(profile_bound(0.0, true, at(48.0), fs), 0);
        assert_eq!(profile_bound(94.0, false, at(142.0), fs), 0);
        assert_eq!(profile_bound(542.0, false, at(590.0), fs), 0);
        assert_eq!(profile_bound(601.0, false, at(649.0), fs), at(49.0));
        assert_eq!(profile_bound(658.0, false, at(706.0), fs), at(106.0));
        assert_eq!(profile_bound(658.0, false, at(1400.0), fs), at(658.0));
    }

    /// Kept while it is recorded, a take's analysis is the same, byte for
    /// byte, as when it is kept from its sound after (#1514): fed in odd
    /// blocks, across several chunks (short ones here, but long enough
    /// for the watch to be heard in the first), a recording split into
    /// files, and the shape stretch. Chunks of 11.5 s end half a second
    /// into a stretch (10 to 12 s, 22 to 24 s, ...), so a stretch read
    /// as soon as its sound is in, before the next chunk's beats, misses
    /// its last beats.
    #[test]
    fn kept_while_recorded_is_kept_as_after() {
        for chunk_s in [12.0, 11.5] {
            kept_in_chunks_is_kept_as_after(chunk_s);
        }
    }

    fn kept_in_chunks_is_kept_as_after(chunk_s: f64) {
        let dir = scratch(&format!("parity-{chunk_s}"));
        let paths = take(&dir, 75.0, 3);
        let cfg = StreamConfig {
            chunk_s,
            ..Default::default()
        };
        let shapes = vec![Some((20.0, 80.0))];
        let refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
        let after = measure(&refs, &cfg, &shapes, software(), |_| {}).unwrap();
        assert!(after.profiles.len() >= 30, "{}", after.profiles.len());
        assert_eq!(after.header.shape.len(), 1);
        for sizes in [vec![1, 4097, 333, 150_001, 7], vec![48_000 * 7]] {
            let live = kept_live(&paths, &cfg, &shapes, &sizes);
            let (a, b) = (kept_files(&after), kept_files(&live));
            assert_eq!(a.len(), b.len());
            for ((name, x), (_, y)) in a.iter().zip(&b) {
                assert!(
                    x == y,
                    "{name} differs, {chunk_s} s chunks, blocks {sizes:?}"
                );
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A watch first heard after a lead of noise, its rate guessed from
    /// each chunk until then (28,800 in this noise), is kept while recorded
    /// as after: its first stretches wait for the rate the whole reading
    /// settles on (21,600).
    #[test]
    fn a_watch_heard_after_noise_is_kept_as_after() {
        let dir = scratch("lead");
        let paths = take_after(&dir, 30.0, 30.0, 1, 21_600);
        let cfg = StreamConfig {
            chunk_s: 12.0,
            ..Default::default()
        };
        assert_eq!(cfg.analysis.bph, None);
        let refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
        let after = measure(&refs, &cfg, &[], software(), |_| {}).unwrap();
        assert!(after.log.beats.iter().any(|b| b.time > 35.0));
        assert_eq!(after.log.bph, 21_600);
        assert!(after.profiles.iter().any(|p| p.end_s <= 30.0));
        let live = kept_live(&paths, &cfg, &[], &[4097, 1, 99_999]);
        let (a, b) = (kept_files(&after), kept_files(&live));
        for ((name, x), (_, y)) in a.iter().zip(&b) {
            assert!(x == y, "{name} differs");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Profiles read before a side swap, put right after it, are those
    /// read with the beats renumbered from the swap on: from minus
    /// infinity (the take's sides named the other way round) and from a
    /// jump in the middle of a stretch, read past it while it was pending
    /// ([`SecondReading::swap_from`]).
    #[test]
    fn swapped_profiles_are_those_read_with_the_beats_renumbered() {
        let dir = scratch("swap");
        let paths = take(&dir, 12.0, 1);
        let cfg = StreamConfig::default();
        let log = stream::analyze_files(&[paths[0].as_path()], &cfg, |_| {}).unwrap();
        let mut sound = Vec::new();
        audio::stream(&paths[0], 1 << 16, |b| sound.extend_from_slice(b)).unwrap();
        let read = |beats: &[Beat], pending: Option<f64>| {
            let mut r = SecondReading::new(log.sample_rate, &[]);
            r.audio(&sound);
            r.profile_until(beats, log.bph, &cfg, (usize::MAX, pending));
            r
        };
        let from = |t: f64| -> Vec<Beat> {
            log.beats
                .iter()
                .map(|b| Beat {
                    index: b.index + i64::from(b.time >= t),
                    ..*b
                })
                .collect()
        };
        // Midway through the fourth stretch.
        for t in [f64::NEG_INFINITY, 7.0] {
            let mut first = read(&log.beats, t.is_finite().then_some(t));
            assert!(first.profiles.len() >= 4);
            assert!(first
                .profiles
                .iter()
                .all(|p| p.sides.iter().all(Option::is_some)));
            let renumbered = from(t);
            first.swap_from(t, &renumbered, log.bph, &cfg);
            assert_eq!(
                format!("{:?}", first.profiles),
                format!("{:?}", read(&renumbered, None).profiles),
                "swap from {t}"
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The parts read back, as far as they reach: the take kept, beats,
    /// amplitude windows and profiles, cut there.
    fn assert_parts_are_the_take_so_far(take: &Path, whole: &Stored, to_s: f64) {
        let parts = checkpoint::read(take).unwrap().stored;
        assert_eq!(parts.header.duration_s, to_s);
        assert_eq!(parts.log.bph, whole.log.bph);
        // Kept to the µs, as `analysis/` keeps them.
        let n = whole.log.beats.partition_point(|b| b.time < to_s);
        assert!(n > 0);
        let rows = |s: &Stored, n: usize| {
            analysis_take::beats_rows(&s.log.beats[..n], &s.log.clipped_beats, s.log.bph)
        };
        assert_eq!(rows(&parts, parts.log.beats.len()), rows(whole, n));
        let wins: Vec<_> = whole
            .log
            .amplitude_windows
            .iter()
            .filter(|w| w.start_s < to_s)
            .cloned()
            .collect();
        assert!(!wins.is_empty());
        for csv in [analysis_take::windows_csv, analysis_take::culled_csv] {
            assert_eq!(csv(&parts.log.amplitude_windows), csv(&wins));
        }
        let n = whole.profiles.partition_point(|p| p.end_s <= to_s);
        assert!(n > 0);
        // Kept as `analysis/` keeps them.
        assert!(
            analysis_take::profiles_bin(&parts.profiles)
                == analysis_take::profiles_bin(&whole.profiles[..n])
        );
        let before = |v: &[f64]| v.iter().copied().filter(|&t| t < to_s).collect::<Vec<_>>();
        assert_eq!(parts.log.jumps, before(&whole.log.jumps));
        assert_eq!(parts.log.sided, before(&whole.log.sided));
    }

    /// Kept on the disk as it is recorded, every 20 s here, a take reads
    /// back from its parts as the take kept up to the last part's end, and
    /// the take kept is the same, byte for byte, as with no parts.
    #[test]
    fn parts_read_back_as_the_take_so_far() {
        let dir = scratch("parts");
        let paths = take(&dir, 75.0, 1);
        let cfg = StreamConfig {
            chunk_s: 11.5,
            ..Default::default()
        };
        let plain = kept_live(&paths, &cfg, &[], &[4097]);
        let parts = Some(Checkpoints {
            take: dir.clone(),
            every_s: 20.0,
        });
        let (kept, status) = kept_with_parts(&paths, &cfg, &[], &[4097, 1, 99_999], parts);
        assert_eq!(kept_files(&kept), kept_files(&plain));
        // The last chunk read starts 69 s in, so the parts reach 60 s, or
        // less where its windows were still being found on other threads.
        assert_eq!(status.failed, None);
        assert!([20.0, 40.0, 60.0].contains(&status.to_s), "{}", status.to_s);
        assert_parts_are_the_take_so_far(&dir, &kept, status.to_s);
        checkpoint::remove(&dir).unwrap();
        assert!(!checkpoint::exists(&dir));
        checkpoint::remove(&dir).unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The parts read back are the take kept from its sound cut where they
    /// reach (Norm on #1711): a take that ended there. The outlier check
    /// near the cut sees only the windows before it, as that take's does.
    /// The sound has a wrong-sounding Tick in one window within 30 s of
    /// each possible cut (8 to 10 s, 28 to 30 s and 48 to 50 s): the
    /// unlock 1 ms early there, as the seed-1 sound renders it.
    #[test]
    fn parts_read_back_as_a_take_that_ended_there() {
        let dir = scratch("cut");
        let render = |lead_s: f64| {
            generate(
                &SynthConfig {
                    duration_s: 75.0,
                    seed: 1,
                    even_unlock_lead_s: lead_s,
                    ..Default::default()
                },
                |_| 270.0,
                |_| 0.0,
            )
        };
        let mut audio = render(0.0);
        let wrong = render(0.001);
        let fs = audio.sample_rate as usize;
        for from in [8, 28, 48] {
            let r = from * fs..(from + 2) * fs;
            audio.samples[r.clone()].copy_from_slice(&wrong.samples[r]);
        }
        let path = dir.join("take_part01-001.wav");
        audio::write_wav(&path, &audio).unwrap();
        let cfg = StreamConfig {
            chunk_s: 10.0,
            ..Default::default()
        };
        let parts = Some(Checkpoints {
            take: dir.clone(),
            every_s: 20.0,
        });
        let (_, status) = kept_with_parts(&[path], &cfg, &[], &[4097], parts);
        assert_eq!(status.failed, None);
        let to_s = status.to_s;
        assert!([20.0, 40.0, 60.0].contains(&to_s), "{to_s}");
        let cut_dir = dir.join("cut");
        std::fs::create_dir_all(&cut_dir).unwrap();
        let cut_path = cut_dir.join("take_part01-001.wav");
        let cut = Audio {
            samples: audio.samples[..(to_s as usize) * fs].to_vec(),
            sample_rate: audio.sample_rate,
        };
        audio::write_wav(&cut_path, &cut).unwrap();
        // As it would be opened: kept in `analysis/` and read back, its
        // times to the µs as the parts keep them.
        let ended = kept_live(&[cut_path], &cfg, &[], &[4097]);
        analysis_take::write(&cut_dir, &ended).unwrap();
        let ended = analysis_take::read(&cut_dir).unwrap();
        let parts = checkpoint::read(&dir).unwrap().stored;
        // The wrong Tick just before the cut is left out, in both.
        let at = |s: &Stored| {
            s.log
                .amplitude_windows
                .iter()
                .find(|w| w.start_s == to_s - 12.0)
                .map(|w| (w.even_deg, w.odd_deg))
                .unwrap()
        };
        let (even, odd) = at(&parts);
        assert!(even.is_none() != odd.is_none(), "{even:?} {odd:?}");
        assert_eq!(at(&ended), (even, odd));
        // The take that ended reads its last chunk with no sound after
        // it, the parts with the sound after: there alone the beats'
        // quality (each beat's match, beside its time) differs, a little,
        // and the last stretch's profiles. Every time, number, window and
        // culled side is the same.
        let last_chunk = to_s - cfg.chunk_s;
        let (a, b) = (&parts.log.beats, &ended.log.beats);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b) {
            // To the µs, as kept.
            let us = |t: f64| (t * 1e6).round() as i64;
            assert_eq!((x.index, us(x.time)), (y.index, us(y.time)));
            if x.time < last_chunk {
                assert_eq!(x.quality, y.quality, "{}", x.time);
            } else {
                assert!((x.quality / y.quality - 1.0).abs() < 0.01, "{}", x.time);
            }
        }
        for csv in [analysis_take::windows_csv, analysis_take::culled_csv] {
            assert_eq!(
                csv(&parts.log.amplitude_windows),
                csv(&ended.log.amplitude_windows)
            );
        }
        // No reported figure moves (Norm on #1711): the whole measurement,
        // rate, amplitude, beat error, their ±, the watch heard and the
        // caveats. Only beat quality and the last stretch's profiles, left
        // out above, differ, for the reason given there.
        let measured = |s: &Stored| {
            let m = crate::session::measure(&s.log, None, 0.0, f64::INFINITY);
            serde_json::to_string_pretty(&m).unwrap()
        };
        assert_eq!(measured(&parts), measured(&ended));
        let (p, q) = (&parts.profiles, &ended.profiles);
        assert_eq!(p.len(), q.len());
        let n = p.len() - 1;
        assert_eq!(p[n].end_s, to_s);
        assert!(analysis_take::profiles_bin(&p[..n]) == analysis_take::profiles_bin(&q[..n]));
        assert_eq!(parts.log.jumps, ended.log.jumps);
        assert_eq!(parts.log.sided, ended.log.sided);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A take whose sides are named only well after its first parts (its
    /// beat error grows slowly from 0, past 0.1 ms only after a minute)
    /// reads back from its parts with the first parts' beats numbered one
    /// further and their sides swapped, as the reading renumbered them.
    #[test]
    fn sides_named_after_a_part_read_back_renamed() {
        let dir = scratch("renamed");
        let audio = generate(
            &SynthConfig {
                duration_s: 150.0,
                beat_error_ms: 0.0,
                beat_error_drift_ms_per_s: 0.0015,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let path = dir.join("take_part01-001.wav");
        audio::write_wav(&path, &audio).unwrap();
        let cfg = StreamConfig {
            chunk_s: 10.0,
            ..Default::default()
        };
        let parts = Some(Checkpoints {
            take: dir.clone(),
            every_s: 10.0,
        });
        let (kept, status) = kept_with_parts(&[path], &cfg, &[], &[48_000], parts);
        let partial: checkpoint::Partial = serde_json::from_slice(
            &std::fs::read(dir.join(checkpoint::DIR).join(checkpoint::HEADER)).unwrap(),
        )
        .unwrap();
        let renamed = partial.renamed_before.expect("named after a part");
        // Named past 0.1 ms, after 67 s: whole parts before, and after.
        assert!(
            (3..partial.parts.len()).contains(&renamed),
            "{renamed} of {}",
            partial.parts.len()
        );
        assert_parts_are_the_take_so_far(&dir, &kept, status.to_s);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A part changed on the disk is left out, with every part after
    /// it, and the take reads up to the part before, saying why; a part
    /// file the list doesn't name is passed over; a take with no parts,
    /// or whose first part doesn't read, has none to read.
    #[test]
    fn parts_not_as_listed_are_left_out() {
        let dir = scratch("tampered");
        let paths = take(&dir, 50.0, 1);
        let cfg = StreamConfig {
            chunk_s: 12.0,
            ..Default::default()
        };
        let parts = Some(Checkpoints {
            take: dir.clone(),
            every_s: 8.0,
        });
        let (_, status) = kept_with_parts(&paths, &cfg, &[], &[4097], parts);
        // The last chunk read starts 48 s in: 6 parts, or fewer while its
        // windows were still being found.
        assert!(status.to_s >= 24.0, "{}", status.to_s);
        let n = (status.to_s / 8.0) as usize;
        let folder = dir.join(checkpoint::DIR);
        let unlisted = format!("{:04}-beats.csv.gz", n + 1);
        std::fs::write(folder.join(unlisted), b"not listed").unwrap();
        let all = checkpoint::read(&dir).unwrap();
        assert_eq!(all.stored.header.duration_s, status.to_s);
        assert_eq!(all.left_out, None);
        let part = folder.join("0002-windows.csv.gz");
        let mut bytes = std::fs::read(&part).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        std::fs::write(&part, &bytes).unwrap();
        let first = checkpoint::read(&dir).unwrap();
        let h = &first.stored.header;
        assert_eq!(h.duration_s, 8.0);
        let rows = (h.beats, h.windows, h.profiles);
        let log = &first.stored.log;
        assert_eq!(
            rows,
            (
                log.beats.len(),
                log.amplitude_windows.len(),
                first.stored.profiles.len()
            )
        );
        assert!(log.beats.last().unwrap().time < 8.0);
        assert!(log.amplitude_windows.iter().all(|w| w.start_s < 8.0));
        let why = first.left_out.unwrap();
        assert!(
            why.starts_with(&format!("parts 2 to {n} of {n}, from 8 s: "))
                && why.contains("0002-windows.csv.gz isn't as partial.json lists it"),
            "{why}"
        );
        // Grown past its listed size: refused by its size alone.
        std::fs::write(folder.join("0001-beats.csv.gz"), vec![0u8; 1 << 20]).unwrap();
        let e = checkpoint::read(&dir).unwrap_err().to_string();
        assert!(
            e.contains("0001-beats.csv.gz isn't as partial.json lists it"),
            "{e}"
        );
        checkpoint::remove(&dir).unwrap();
        let e = checkpoint::read(&dir).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Parts that can't be written leave the recording and its reading
    /// going, say why, and the take is kept all the same.
    #[test]
    fn parts_not_written_say_why_and_the_take_is_kept() {
        let dir = scratch("unwritable");
        let paths = take(&dir, 40.0, 1);
        let cfg = StreamConfig {
            chunk_s: 12.0,
            ..Default::default()
        };
        let plain = kept_live(&paths, &cfg, &[], &[4097]);
        // A file where the take's folder should be.
        let blocked = dir.join("a file");
        std::fs::write(&blocked, b"").unwrap();
        let parts = Some(Checkpoints {
            take: blocked,
            every_s: 12.0,
        });
        let (kept, status) = kept_with_parts(&paths, &cfg, &[], &[4097], parts);
        assert_eq!(kept_files(&kept), kept_files(&plain));
        assert_eq!(status.to_s, 0.0);
        assert!(status
            .failed
            .is_some_and(|e| e.contains("analysis.partial")));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Nothing pushed keeps an empty take, and a Keeper dropped unfinished
    /// stops its reading.
    #[test]
    fn an_empty_or_dropped_keeper_ends() {
        let how = || KeepAs {
            sample_rate: 48_000,
            bytes_per_frame: 2,
            cfg: StreamConfig::default(),
            shapes: Vec::new(),
            software: software(),
            checkpoints: None,
        };
        let s = Keeper::start(how()).finish().unwrap();
        assert!(s.log.beats.is_empty() && s.profiles.is_empty());
        let k = Keeper::start(how());
        assert!(k.push(&[0.0; 4800]));
        drop(k);
    }
}
