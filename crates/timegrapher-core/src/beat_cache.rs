//! The beats found in a take's sound, kept between opens so a long take
//! opens again in seconds (#726). The sound stays the source: what is
//! kept is only a copy of what [`crate::stream::analyze_files_peak`]
//! gave, exact to the bit, and is read back only when everything that
//! made it is the same:
//!
//! - the sound files: each one's full path, length, modified time and
//!   [`SPANS`] spans of 64 KiB spread from its start to its end, in order
//!   (#1326: a copy of another take, with the same name, length, time and
//!   start, must not read as this one);
//! - the engine: this build's version and a hash of core's own source
//!   (`build.rs`), so any change to core makes every copy stale, with no
//!   version to bump by hand;
//! - every analysis setting ([`StreamConfig`]);
//! - this file's own layout ([`FORMAT`]).
//!
//! Anything else, a copy that can't be read or doesn't match, is left
//! out and the sound is read again: the worst a copy can do is cost the
//! time it was meant to save. Nothing is kept unless the surface turns it
//! on ([`set_dir`]): the app does, at its start; the command line, the MCP
//! server and the tests read the sound every time. The copies live in the
//! system's cache folder ([`default_dir`]), never beside the take, and
//! deleting that folder only makes the next open slow.

use crate::amplitude::AmplitudeWindow;
use crate::beats::Beat;
use crate::stream::{BeatLog, StreamConfig};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, SystemTime};

/// The layout's name and version, first in every copy.
/// Version 2 adds each window's culled sides' edges (#1333); 3, whether
/// the lock-on template was kept (`lock_template_kept`, #1503); 4, each
/// window's sides withheld as too faint to read (#1738).
pub const FORMAT: &[u8; 16] = b"timegrapher.bc/4";
/// Bytes in each span of a sound file read into the key.
const HEAD_BYTES: usize = 1 << 16;
/// Spans of each sound file read into the key: the first at its start,
/// the last at its end, the rest evenly between. 1 MiB a file, so a 40 GB
/// take still opens from its copy in seconds.
pub const SPANS: u64 = 16;
/// A take whose newest file changed this recently may still be recording:
/// no copy is kept of it.
const STILL_WRITING: Duration = Duration::from_secs(120);
/// The copies' total size the folder is held to; the least recently used
/// go first.
pub const KEEP_BYTES: u64 = 2 << 30;
/// The environment variable that turns the copies off for the app
/// (any value but empty), for checking that a take reads the same from
/// its sound.
pub const OFF_VAR: &str = "TIMEGRAPHER_NO_BEAT_CACHE";

static DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Keep copies in `dir`, or none.
pub fn set_dir(dir: Option<PathBuf>) {
    *DIR.write().unwrap_or_else(|e| e.into_inner()) = dir;
}

/// Where copies are kept now, if anywhere.
pub fn dir() -> Option<PathBuf> {
    DIR.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The system's cache folder for Timegrapher's beats: never a library or
/// a take's folder, which may be read-only or on another computer.
/// `None` when [`OFF_VAR`] is set or the folder can't be named.
pub fn default_dir() -> Option<PathBuf> {
    if std::env::var_os(OFF_VAR).is_some_and(|v| !v.is_empty()) {
        return None;
    }
    cache_base().map(|b| b.join("beats"))
}

/// The system's cache folder for Timegrapher: the one place every kind of
/// cached file (beats, photo thumbnails) is put under.
pub fn cache_base() -> Option<PathBuf> {
    crate::user_dirs::app_dir(crate::user_dirs::Kind::Cache)
}

/// The key of the beats of `files` read with `cfg`, as a file name; none
/// when a file can't be looked at.
pub fn key(files: &[&Path], cfg: &StreamConfig) -> Option<String> {
    let mut h = Sha256::new();
    h.update(FORMAT);
    h.update(crate::library::ENGINE.as_bytes());
    h.update(format!("{cfg:?}").as_bytes());
    for f in files {
        let meta = fs::metadata(f).ok()?;
        let changed = meta
            .modified()
            .ok()?
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()?;
        h.update(std::path::absolute(f).ok()?.as_os_str().as_encoded_bytes());
        h.update([0]);
        h.update(meta.len().to_le_bytes());
        h.update(changed.as_nanos().to_le_bytes());
        spans(&mut h, f, meta.len())?;
    }
    let d = h.finalize();
    Some(d.iter().take(16).map(|b| format!("{b:02x}")).collect())
}

/// Feed [`SPANS`] spans of `f`, `len` bytes long, into `h`: the whole
/// file when it is no longer than they are.
fn spans(h: &mut Sha256, f: &Path, len: u64) -> Option<()> {
    use std::io::{Seek, SeekFrom};
    let mut file = fs::File::open(f).ok()?;
    let span = HEAD_BYTES as u64;
    let mut buf = Vec::with_capacity(HEAD_BYTES);
    if len <= span * SPANS {
        file.read_to_end(&mut buf).ok()?;
        h.update(&buf);
        return Some(());
    }
    for i in 0..SPANS {
        let at = i * (len - span) / (SPANS - 1);
        file.seek(SeekFrom::Start(at)).ok()?;
        buf.clear();
        (&mut file).take(span).read_to_end(&mut buf).ok()?;
        h.update(&buf);
    }
    Some(())
}

/// The beats of `files` read with `cfg`, and the loudest sample's size,
/// as kept in `dir`; none when there is no copy or it can't be read.
pub fn read(dir: &Path, files: &[&Path], cfg: &StreamConfig) -> Option<(BeatLog, f32)> {
    let path = dir.join(key(files, cfg)?);
    let raw = fs::read(&path).ok()?;
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(&raw[..])
        .read_to_end(&mut bytes)
        .ok()?;
    let got = decode(&bytes)?;
    // Used now: last to go when the folder is held to its size.
    let _ = fs::File::options()
        .write(true)
        .open(&path)
        .and_then(|f| f.set_modified(SystemTime::now()));
    Some(got)
}

/// Keep the beats `log` and loudest sample `peak` of `files`, read with
/// `cfg`, in `dir`; then hold the folder to [`KEEP_BYTES`]. Nothing is
/// kept for files still being written. Written to a file of its own and
/// renamed into place, so a reader never sees half of one.
pub fn write(
    dir: &Path,
    files: &[&Path],
    cfg: &StreamConfig,
    log: &BeatLog,
    peak: f32,
) -> io::Result<()> {
    if files.iter().any(|f| recently_changed(f)) {
        return Ok(());
    }
    let Some(key) = key(files, cfg) else {
        return Ok(());
    };
    fs::create_dir_all(dir)?;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&encode(log, peak))?;
    let bytes = gz.finish()?;
    // Its own name, even for two opens of the same take at once.
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let part = dir.join(format!("{key}.{}-{n}.part", std::process::id()));
    fs::write(&part, &bytes)?;
    crate::recorder::rename_settled(&part, &dir.join(&key)).inspect_err(|_| {
        let _ = fs::remove_file(&part);
    })?;
    hold_to(dir, KEEP_BYTES);
    Ok(())
}

fn recently_changed(f: &Path) -> bool {
    fs::metadata(f)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_none_or(|age| age < STILL_WRITING)
}

/// Remove the least recently used copies in `dir` until they come to no
/// more than `keep` bytes.
fn hold_to(dir: &Path, keep: u64) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut kept: Vec<(SystemTime, u64, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            (m.is_file() && e.path().extension().is_none())
                .then(|| Some((m.modified().ok()?, m.len(), e.path())))?
        })
        .collect();
    kept.sort();
    let mut total: u64 = kept.iter().map(|k| k.1).sum();
    for (_, len, path) in kept {
        if total <= keep {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

/// `log` and `peak` as bytes, every field exact.
pub fn encode(log: &BeatLog, peak: f32) -> Vec<u8> {
    let mut o = Vec::with_capacity(32 + log.beats.len() * 20);
    o.extend_from_slice(FORMAT);
    o.extend_from_slice(&log.sample_rate.to_le_bytes());
    o.extend_from_slice(&log.bph.to_le_bytes());
    f64s(&mut o, &[log.duration_s, log.lift_deg]);
    o.extend_from_slice(&peak.to_le_bytes());
    o.extend_from_slice(&(log.clipped_samples as u64).to_le_bytes());
    o.push(u8::from(log.lock_template_kept));
    len(&mut o, log.beats.len());
    for b in &log.beats {
        o.extend_from_slice(&b.index.to_le_bytes());
        o.extend_from_slice(&b.time.to_le_bytes());
        o.extend_from_slice(&b.quality.to_le_bytes());
    }
    len(&mut o, log.amplitude_windows.len());
    for w in &log.amplitude_windows {
        f64s(&mut o, &[w.start_s, w.end_s]);
        for v in window_options(w) {
            match v {
                Some(v) => {
                    o.push(1);
                    o.extend_from_slice(&v.to_le_bytes());
                }
                None => o.push(0),
            }
        }
        let [e, d] = w.faint_withheld;
        o.push(u8::from(e) | u8::from(d) << 1);
    }
    for times in [&log.clipped_beats, &log.jumps, &log.sided, &log.by_count] {
        len(&mut o, times.len());
        f64s(&mut o, times);
    }
    len(&mut o, log.snippets.len());
    for (t, s) in &log.snippets {
        o.extend_from_slice(&t.to_le_bytes());
        len(&mut o, s.len());
        for v in s {
            o.extend_from_slice(&v.to_le_bytes());
        }
    }
    o
}

fn window_options(w: &AmplitudeWindow) -> [Option<f64>; 12] {
    let [even, odd] = w.culled();
    let [eu, ed] = even.map_or([None; 2], |e| e.map(Some));
    let [ou, od] = odd.map_or([None; 2], |e| e.map(Some));
    [
        w.even_deg,
        w.odd_deg,
        w.beat_error_ms,
        w.beat_error_unlock_ms,
        w.even_unlock_ms,
        w.odd_unlock_ms,
        w.even_drop_ms,
        w.odd_drop_ms,
        eu,
        ed,
        ou,
        od,
    ]
}

fn len(o: &mut Vec<u8>, n: usize) {
    o.extend_from_slice(&(n as u64).to_le_bytes());
}

fn f64s(o: &mut Vec<u8>, v: &[f64]) {
    for x in v {
        o.extend_from_slice(&x.to_le_bytes());
    }
}

/// Reads bytes in order, never past the end.
struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.0.split_first_chunk::<N>()?;
        self.0 = rest;
        Some(*head)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take::<1>().map(|b| b[0])
    }
    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }
    fn f32(&mut self) -> Option<f32> {
        self.take().map(f32::from_le_bytes)
    }
    fn f64(&mut self) -> Option<f64> {
        self.take().map(f64::from_le_bytes)
    }
    fn i64(&mut self) -> Option<i64> {
        self.take().map(i64::from_le_bytes)
    }
    /// A count of items each at least `each` bytes long, no more than
    /// the bytes left could hold.
    fn len(&mut self, each: usize) -> Option<usize> {
        let n = usize::try_from(u64::from_le_bytes(self.take()?)).ok()?;
        (n <= self.0.len() / each.max(1)).then_some(n)
    }
    fn f64s(&mut self) -> Option<Vec<f64>> {
        let n = self.len(8)?;
        (0..n).map(|_| self.f64()).collect()
    }
}

/// What [`encode`] wrote, or none for anything else.
pub fn decode(bytes: &[u8]) -> Option<(BeatLog, f32)> {
    let mut r = Bytes(bytes);
    if &r.take::<16>()? != FORMAT {
        return None;
    }
    let sample_rate = r.u32()?;
    let bph = r.u32()?;
    let duration_s = r.f64()?;
    let lift_deg = r.f64()?;
    let peak = r.f32()?;
    let clipped_samples = usize::try_from(u64::from_le_bytes(r.take()?)).ok()?;
    let lock_template_kept = match r.take::<1>()? {
        [0] => false,
        [1] => true,
        _ => return None,
    };
    let n = r.len(20)?;
    let beats = (0..n)
        .map(|_| {
            Some(Beat {
                index: r.i64()?,
                time: r.f64()?,
                quality: r.f32()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let n = r.len(24)?;
    let amplitude_windows = (0..n)
        .map(|_| read_window(&mut r))
        .collect::<Option<Vec<_>>>()?;
    let clipped_beats = r.f64s()?;
    let jumps = r.f64s()?;
    let sided = r.f64s()?;
    let by_count = r.f64s()?;
    let n = r.len(16)?;
    let snippets = (0..n)
        .map(|_| {
            let t = r.f64()?;
            let m = r.len(4)?;
            Some((t, (0..m).map(|_| r.f32()).collect::<Option<Vec<_>>>()?))
        })
        .collect::<Option<Vec<_>>>()?;
    if !r.0.is_empty() {
        return None;
    }
    let log = BeatLog {
        lock_template_kept,
        sample_rate,
        duration_s,
        bph,
        lift_deg,
        beats,
        amplitude_windows,
        clipped_beats,
        clipped_samples,
        jumps,
        sided,
        by_count,
        snippets,
    };
    Some((log, peak))
}

fn read_window(r: &mut Bytes) -> Option<AmplitudeWindow> {
    let start_s = r.f64()?;
    let end_s = r.f64()?;
    let mut v = [None; 12];
    for x in &mut v {
        *x = match r.u8()? {
            0 => None,
            1 => Some(r.f64()?),
            _ => return None,
        };
    }
    let [even_deg, odd_deg, beat_error_ms, beat_error_unlock_ms, even_unlock_ms, odd_unlock_ms, even_drop_ms, odd_drop_ms, eu, ed, ou, od] =
        v;
    let faint = r.u8()?;
    if faint > 3 {
        return None;
    }
    let mut w = AmplitudeWindow {
        start_s,
        end_s,
        even_deg,
        odd_deg,
        beat_error_ms,
        beat_error_unlock_ms,
        even_unlock_ms,
        odd_unlock_ms,
        even_drop_ms,
        odd_drop_ms,
        culled_ms: None,
        faint_withheld: [faint & 1 != 0, faint & 2 != 0],
    };
    w.set_culled([
        eu.zip(ed).map(|(u, d)| [u, d]),
        ou.zip(od).map(|(u, d)| [u, d]),
    ]);
    Some(w)
}

#[cfg(test)]
#[path = "beat_cache_tests.rs"]
mod tests;
