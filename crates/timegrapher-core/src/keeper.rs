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
use crate::dsp::envelope;
use crate::profile::{self, Side};
use crate::stream::{self, BeatLog, StreamConfig};
use crate::usbclock;
use std::sync::mpsc;

/// Audio either side of a profile's stretch, for its beats' windows, as
/// the app's profile pane reads it.
const PROFILE_MARGIN_S: f64 = 0.05;

/// The most sound, seconds, held back while a jump's sides are still to
/// be told apart ([`stream::Seen::pending`]): past it the profiles are
/// read on, so a jump whose sides are never told apart can't hold back
/// the whole take.
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

    /// Profile every stretch whose sound and margin are in and that ends
    /// by sample `until`, with `beats` (audio time) at `bph`.
    pub(crate) fn profile_until(
        &mut self,
        beats: &[Beat],
        bph: u32,
        cfg: &StreamConfig,
        until: usize,
    ) {
        while self.buf0 + self.buf.len() >= (self.k + 1) * self.step + self.margin
            && (self.k + 1) * self.step <= until
        {
            self.profile_next(beats, bph, cfg, f64::INFINITY);
            let keep = (self.k * self.step).saturating_sub(self.margin);
            if keep > self.buf0 {
                self.buf.drain(..keep - self.buf0);
                self.buf0 = keep;
            }
        }
    }

    /// Stretch `k`'s profiles, its end no later than `duration_s`; then
    /// the next stretch.
    fn profile_next(&mut self, beats: &[Beat], bph: u32, cfg: &StreamConfig, duration_s: f64) {
        let (fs, k) = (self.fs, self.k);
        self.k += 1;
        let (a, b) = (k * self.step, (k + 1) * self.step);
        let lo = a.saturating_sub(self.margin).max(self.buf0);
        let hi = (b + self.margin).min(self.buf0 + self.buf.len());
        let x = &self.buf[lo - self.buf0..hi - self.buf0];
        let (from, to) = (a as f64 / fs, b as f64 / fs);
        while beats.get(self.next_beat).is_some_and(|bt| bt.time < from) {
            self.next_beat += 1;
        }
        let x0 = lo as f64 / fs;
        let local: Vec<Beat> = beats[self.next_beat.min(beats.len())..]
            .iter()
            .take_while(|bt| bt.time < to)
            .map(|bt| Beat {
                time: bt.time - x0,
                ..*bt
            })
            .collect();
        if local.is_empty() {
            return;
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
        self.profiles.push(StretchProfiles {
            end_s: to.min(duration_s),
            signal: crate::live::signal_figure(&env, fs, &local),
            sides,
        });
    }

    /// Every profile so far with Tick and Tock the other way round: the
    /// take's sides were named the other way round after they were read
    /// ([`stream::Seen::swaps`]), so each beat's number is one further.
    fn swap_sides(&mut self) {
        for p in &mut self.profiles {
            p.sides.swap(0, 1);
            for (s, side) in p.sides.iter_mut().zip([Side::A, Side::B]) {
                if let Some(s) = s {
                    s.side = side;
                }
            }
        }
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
        self.profile_until(&log.beats, log.bph, cfg, usize::MAX);
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
}

impl Keeper {
    /// Start keeping a take recorded as `how` says.
    pub fn start(how: KeepAs) -> Keeper {
        let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(QUEUED_BLOCKS);
        let worker = std::thread::spawn(move || keep(&how, rx));
        Keeper {
            tx: Some(tx),
            worker: Some(worker),
        }
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

/// The Keeper's reading: the blocks from `rx`, until it closes, read as
/// one recording.
fn keep(how: &KeepAs, rx: mpsc::Receiver<Vec<f32>>) -> Result<Stored, AudioError> {
    let cfg = &how.cfg;
    let fs = f64::from(how.sample_rate);
    let mut second = SecondReading::new(how.sample_rate, &how.shapes);
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
                if t == f64::NEG_INFINITY {
                    second.swap_sides();
                }
            }
            swaps = seen.swaps.len();
            // A chunk behind, so the beats there are kept for good, and
            // not past a jump whose sides may yet swap, unless that would
            // hold back too much sound.
            let behind = ((seen.core_a - cfg.margin_s) * fs).max(0.0) as usize;
            let held_to = second.read().saturating_sub((MAX_HELD_S * fs) as usize);
            let jump = seen
                .pending
                .map_or(usize::MAX, |t| (t * fs).max(0.0) as usize)
                .max(held_to);
            second.profile_until(seen.beats, seen.bph, cfg, behind.min(jump));
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
    use crate::analysis_take::{files, measure};
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
        let audio = generate(
            &SynthConfig {
                duration_s: seconds,
                beat_error_ms: 0.6,
                odd_gains: Some([0.9, 0.25, 1.0]),
                ..Default::default()
            },
            |t| 270.0 + 10.0 * (t / 10.0).sin(),
            |_| 0.0,
        );
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
        let info = audio::info(&paths[0]).unwrap();
        let keeper = Keeper::start(KeepAs {
            sample_rate: info.sample_rate,
            bytes_per_frame: info.bytes_per_frame,
            cfg: cfg.clone(),
            shapes: shapes.to_vec(),
            software: software(),
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
        keeper.finish().unwrap()
    }

    /// The files `analysis/` would hold, by name.
    fn kept_files(s: &Stored) -> Vec<(&'static str, Vec<u8>)> {
        files(s).unwrap()
    }

    /// Kept while it is recorded, a take's analysis is the same, byte for
    /// byte, as when it is kept from its sound after (#1514): fed in odd
    /// blocks, across many chunks (short ones here), a recording split
    /// into files, and the shape stretch.
    #[test]
    fn kept_while_recorded_is_kept_as_after() {
        let dir = scratch("parity");
        let paths = take(&dir, 75.0, 3);
        let cfg = StreamConfig {
            chunk_s: 7.0,
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
                assert!(x == y, "{name} differs, blocks {sizes:?}");
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Profiles read before the take's sides were named the other way
    /// round, swapped, are those read after, with every beat numbered one
    /// further ([`SecondReading::swap_sides`]).
    #[test]
    fn swapped_profiles_are_those_read_with_the_beats_renumbered() {
        let dir = scratch("swap");
        let paths = take(&dir, 12.0, 1);
        let cfg = StreamConfig::default();
        let log = stream::analyze_files(&[paths[0].as_path()], &cfg, |_| {}).unwrap();
        let mut sound = Vec::new();
        audio::stream(&paths[0], 1 << 16, |b| sound.extend_from_slice(b)).unwrap();
        let read = |beats: &[Beat]| {
            let mut r = SecondReading::new(log.sample_rate, &[]);
            r.audio(&sound);
            r.profile_until(beats, log.bph, &cfg, usize::MAX);
            r
        };
        let mut first = read(&log.beats);
        let renumbered: Vec<Beat> = log
            .beats
            .iter()
            .map(|b| Beat {
                index: b.index + 1,
                ..*b
            })
            .collect();
        let after = read(&renumbered);
        assert!(first.profiles.len() >= 4);
        assert!(first
            .profiles
            .iter()
            .all(|p| p.sides.iter().all(Option::is_some)));
        first.swap_sides();
        assert_eq!(
            format!("{:?}", first.profiles),
            format!("{:?}", after.profiles)
        );
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
        };
        let s = Keeper::start(how()).finish().unwrap();
        assert!(s.log.beats.is_empty() && s.profiles.is_empty());
        let k = Keeper::start(how());
        assert!(k.push(&[0.0; 4800]));
        drop(k);
    }
}
