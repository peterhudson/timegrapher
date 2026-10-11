//! A few seconds from anywhere in a long recording, read quickly for the
//! Scope. A WAV seeks straight there; a FLAC is searched by its frame
//! headers, so a moment an hour into a segment is found without decoding
//! the hour before it.

use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use timegrapher_core::audio;

/// Mono samples from `t0` seconds at `rate` samples a second.
pub struct Span {
    pub t0: f64,
    pub rate: f64,
    pub mono: Vec<f32>,
}

/// `len_s` seconds from `from_s` of the files read one after another (the
/// segments of one recording), mixed to mono. Shorter at the end of the
/// recording; an error when nothing of it is there.
pub fn read(files: &[PathBuf], from_s: f64, len_s: f64) -> Result<Span, String> {
    let from_s = from_s.max(0.0);
    let mut out: Option<Span> = None;
    let mut pos = 0u64; // frames before the current file
    for f in files {
        let info = audio::info(f).map_err(|e| e.to_string())?;
        let rate = info.sample_rate as f64;
        let n = info
            .frames
            .ok_or_else(|| format!("{} doesn't give its length", f.display()))?;
        let a = (from_s * rate).floor() as u64;
        let b = ((from_s + len_s.max(0.0)) * rate).ceil() as u64;
        if pos >= b {
            break;
        }
        if pos + n > a {
            let lo = a.saturating_sub(pos).min(n);
            let hi = b.saturating_sub(pos).min(n);
            let part = frames(f, lo, hi)?;
            match &mut out {
                Some(o) => o.mono.extend_from_slice(&part),
                None => {
                    out = Some(Span {
                        t0: (pos + lo) as f64 / rate,
                        rate,
                        mono: part,
                    })
                }
            }
        }
        pos += n;
    }
    out.filter(|s| !s.mono.is_empty())
        .ok_or_else(|| "no sound in that stretch".to_string())
}

/// Frames `lo..hi` of one file, mixed to mono, full scale ±1.
fn frames(path: &Path, lo: u64, hi: u64) -> Result<Vec<f32>, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "wav" => wav_frames(path, lo, hi),
        "flac" => flac_frames(path, lo, hi),
        other => Err(format!("file extension '{other}'")),
    }
}

fn wav_frames(path: &Path, lo: u64, hi: u64) -> Result<Vec<f32>, String> {
    let mut r = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = r.spec();
    let ch = spec.channels.max(1) as usize;
    r.seek(lo as u32).map_err(|e| e.to_string())?;
    let want = (hi - lo) as usize * ch;
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r
            .samples::<f32>()
            .take(want)
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            r.samples::<i32>()
                .take(want)
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string())?
        }
    };
    Ok(raw
        .chunks(ch)
        .map(|f| f.iter().sum::<f32>() / ch as f32)
        .collect())
}

fn flac_frames(path: &Path, lo: u64, hi: u64) -> Result<Vec<f32>, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let header = flac_header(&mut file)?;
    let len = file.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
    // Bisect the bytes for the last frame starting at or before `lo`.
    let (mut lo_off, mut hi_off) = (header.len() as u64, len);
    while hi_off - lo_off > 1 << 16 {
        let mid = lo_off + (hi_off - lo_off) / 2;
        match frame_at(path, &header, mid, hi_off)? {
            Some((off, t)) if t <= lo => lo_off = off,
            _ => hi_off = mid,
        }
    }
    let mut src = BufReader::new(File::open(path).map_err(|e| e.to_string())?);
    src.seek(SeekFrom::Start(lo_off))
        .map_err(|e| e.to_string())?;
    let mut reader = claxon::FlacReader::new(Cursor::new(header.clone()).chain(src))
        .map_err(|e| e.to_string())?;
    let scale = 1.0 / (1u64 << (reader.streaminfo().bits_per_sample.max(1) - 1)) as f32;
    let mut blocks = reader.blocks();
    let mut out = Vec::with_capacity((hi - lo) as usize);
    let mut store = Vec::new();
    // The first frame's own number places it; later frames follow on.
    let mut t: Option<u64> = None;
    while let Some(b) = blocks.read_next_or_eof(store).map_err(|e| e.to_string())? {
        let start = *t.get_or_insert(b.time());
        let n = b.duration() as u64;
        let ch = b.channels();
        for i in lo.max(start)..hi.min(start + n) {
            let i = (i - start) as u32;
            let sum: i32 = (0..ch).map(|c| b.sample(c, i)).sum();
            out.push(sum as f32 * scale / ch as f32);
        }
        t = Some(start + n);
        store = b.into_buffer();
        if start + n >= hi {
            break;
        }
    }
    Ok(out)
}

/// The "fLaC" marker and every metadata block, which a reader needs before
/// any frame.
fn flac_header(file: &mut File) -> Result<Vec<u8>, String> {
    let mut head = vec![0u8; 4];
    file.read_exact(&mut head).map_err(|e| e.to_string())?;
    if &head != b"fLaC" {
        return Err("not a FLAC file".into());
    }
    loop {
        let mut h = [0u8; 4];
        file.read_exact(&mut h).map_err(|e| e.to_string())?;
        let n = u32::from_be_bytes([0, h[1], h[2], h[3]]) as usize;
        let mut body = vec![0u8; n];
        file.read_exact(&mut body).map_err(|e| e.to_string())?;
        head.extend_from_slice(&h);
        head.extend_from_slice(&body);
        if h[0] & 0x80 != 0 {
            return Ok(head);
        }
    }
}

/// The first whole frame starting at or after byte `from` (and before
/// `until`), with its first sample number. A frame is only taken once it
/// decodes with a good checksum, so a sync pattern in the audio can't fool
/// it.
fn frame_at(
    path: &Path,
    header: &[u8],
    from: u64,
    until: u64,
) -> Result<Option<(u64, u64)>, String> {
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    f.seek(SeekFrom::Start(from)).map_err(|e| e.to_string())?;
    let take = (until.saturating_sub(from)).min(1 << 20) as usize;
    let mut buf = vec![0u8; take];
    let got = read_up_to(&mut f, &mut buf)?;
    buf.truncate(got);
    for i in 0..buf.len().saturating_sub(1) {
        if buf[i] != 0xff || buf[i + 1] & 0xfe != 0xf8 {
            continue;
        }
        let off = from + i as u64;
        let mut src = BufReader::new(File::open(path).map_err(|e| e.to_string())?);
        src.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
        let Ok(mut r) = claxon::FlacReader::new(Cursor::new(header.to_vec()).chain(src)) else {
            continue;
        };
        if let Ok(Some(b)) = r.blocks().read_next_or_eof(Vec::new()) {
            return Ok(Some((off, b.time())));
        }
    }
    Ok(None)
}

fn read_up_to(f: &mut File, buf: &mut [u8]) -> Result<usize, String> {
    let mut got = 0;
    while got < buf.len() {
        match f.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(got)
}

#[cfg(test)]
mod tests {
    use super::*;
    use timegrapher_core::excerpt::{self, Pcm};

    fn tone(frames: usize, ch: u16) -> Pcm {
        let samples = (0..frames)
            .flat_map(|i| {
                let v = ((i as f64 * 0.37).sin() * 12_000.0) as i32 + (i % 97) as i32;
                (0..ch).map(move |c| v + c as i32)
            })
            .collect();
        Pcm {
            sample_rate: 8000,
            channels: ch,
            bits_per_sample: 16,
            samples,
        }
    }

    #[test]
    fn flac_stretches_match_a_full_read() {
        let dir = std::env::temp_dir().join(format!("tg-span-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pcm = tone(8000 * 120, 2);
        let a = dir.join("a.flac");
        let b = dir.join("b.flac");
        excerpt::write_flac(&a, &pcm).unwrap();
        excerpt::write_flac(&b, &tone(8000 * 30, 2)).unwrap();
        let files = vec![a, b];
        for &(from, len) in &[
            (0.0, 1.0),
            (61.3, 2.5),
            (119.0, 3.0),
            (100.0, 0.01),
            (148.0, 5.0),
        ] {
            let got = read(&files, from, len).unwrap();
            let want = excerpt::cut(&files, from, len).unwrap();
            let mono: Vec<f32> = want
                .samples
                .chunks(2)
                .map(|f| (f[0] + f[1]) as f32 / 32768.0 / 2.0)
                .collect();
            assert_eq!(got.mono.len(), mono.len(), "at {from}");
            assert!((got.t0 - (from * 8000.0).floor() / 8000.0).abs() < 1e-9);
            for (x, y) in got.mono.iter().zip(&mono) {
                assert!((x - y).abs() < 1e-6, "at {from}");
            }
        }
        assert!(read(&files, 200.0, 1.0).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
