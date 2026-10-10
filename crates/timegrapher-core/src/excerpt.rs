//! Cutting a stretch out of a recording without changing a sample: read
//! the integer samples of WAV or FLAC files, and write them as FLAC, which
//! is lossless, so the cut holds exactly what the original held.
//!
//! The FLAC writer is small on purpose: fixed blocks of 4096 samples,
//! independent channels, the fixed predictors of orders 0 to 4 and
//! partitioned Rice coding. On watch recordings that is within a few per
//! cent of `flac -5`. The stream's MD5 is left as zero (unknown), as the
//! format allows.

use crate::audio::{self, AudioError};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Integer samples, interleaved by channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub samples: Vec<i32>,
}

impl Pcm {
    /// Samples per channel.
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    pub fn duration_s(&self) -> f64 {
        self.frames() as f64 / self.sample_rate as f64
    }
}

/// `len_s` seconds from `from_s` of the files read one after another (the
/// segments of one recording), with their samples untouched. The files must
/// share a sample rate, channel count and sample size.
pub fn cut(files: &[PathBuf], from_s: f64, len_s: f64) -> Result<Pcm, AudioError> {
    let mut out: Option<Pcm> = None;
    let mut pos = 0u64; // frames before the current file
    for f in files {
        let info = audio::info(f)?;
        let rate = info.sample_rate as f64;
        let a = (from_s.max(0.0) * rate).round() as u64;
        let b = ((from_s.max(0.0) + len_s.max(0.0)) * rate).round() as u64;
        if pos >= b {
            break;
        }
        if let Some(n) = info.frames {
            if pos + n <= a {
                pos += n;
                continue;
            }
        }
        let p = read(f)?;
        let n = p.frames() as u64;
        if let Some(o) = &out {
            if (o.sample_rate, o.channels, o.bits_per_sample)
                != (p.sample_rate, p.channels, p.bits_per_sample)
            {
                return Err(AudioError::Unsupported(format!(
                    "{} does not match the segments before it",
                    f.display()
                )));
            }
        }
        let lo = a.saturating_sub(pos).min(n) as usize;
        let hi = b.saturating_sub(pos).min(n) as usize;
        let ch = p.channels as usize;
        let part = &p.samples[lo * ch..hi * ch];
        match &mut out {
            Some(o) => o.samples.extend_from_slice(part),
            None => {
                out = Some(Pcm {
                    samples: part.to_vec(),
                    ..p
                })
            }
        }
        pos += n;
    }
    out.ok_or_else(|| AudioError::Unsupported("no audio in that stretch".into()))
}

/// Every sample of a WAV or FLAC file.
pub fn read(path: &Path) -> Result<Pcm, AudioError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "wav" => {
            let mut r = hound::WavReader::open(path).map_err(AudioError::Wav)?;
            let s = r.spec();
            if s.sample_format != hound::SampleFormat::Int {
                return Err(AudioError::Unsupported(
                    "floating-point WAV can't be cut losslessly to FLAC".into(),
                ));
            }
            let samples = r
                .samples::<i32>()
                .collect::<Result<Vec<_>, _>>()
                .map_err(AudioError::Wav)?;
            Ok(Pcm {
                sample_rate: s.sample_rate,
                channels: s.channels,
                bits_per_sample: s.bits_per_sample,
                samples,
            })
        }
        "flac" => {
            let mut r = claxon::FlacReader::open(path).map_err(AudioError::Flac)?;
            let s = r.streaminfo();
            let samples = r
                .samples()
                .collect::<Result<Vec<_>, _>>()
                .map_err(AudioError::Flac)?;
            Ok(Pcm {
                sample_rate: s.sample_rate,
                channels: s.channels as u16,
                bits_per_sample: s.bits_per_sample as u16,
                samples,
            })
        }
        other => Err(AudioError::Unsupported(format!("file extension '{other}'"))),
    }
}

const BLOCK: usize = 4096;

/// Write `pcm` as a FLAC file.
pub fn write_flac(path: &Path, pcm: &Pcm) -> Result<(), AudioError> {
    let bytes = encode_flac(pcm)?;
    let mut f = std::fs::File::create(path).map_err(AudioError::Io)?;
    f.write_all(&bytes).map_err(AudioError::Io)
}

/// The bytes of a FLAC file holding `pcm`.
pub fn encode_flac(pcm: &Pcm) -> Result<Vec<u8>, AudioError> {
    let ch = pcm.channels as usize;
    let bps = pcm.bits_per_sample as u32;
    let Some(size_code) = size_code(bps) else {
        return Err(AudioError::Unsupported(format!("{bps}-bit samples")));
    };
    if !(1..=8).contains(&ch) || pcm.samples.len() % ch != 0 {
        return Err(AudioError::Unsupported(format!(
            "{ch} channels of {bps}-bit samples"
        )));
    }
    if pcm.sample_rate == 0 || rate_code(pcm.sample_rate).0 == 0 {
        return Err(AudioError::Unsupported(format!(
            "sample rate {}",
            pcm.sample_rate
        )));
    }
    let frames = pcm.frames();
    let mut out = Vec::with_capacity(pcm.samples.len() * bps as usize / 12 + 64);
    out.extend_from_slice(b"fLaC");
    // STREAMINFO, the last metadata block.
    let mut w = Bits::default();
    w.put(1, 1);
    w.put(0, 7);
    w.put(34, 24);
    w.put(BLOCK as u64, 16);
    w.put(BLOCK as u64, 16);
    w.put(0, 24);
    w.put(0, 24);
    w.put(pcm.sample_rate as u64, 20);
    w.put(ch as u64 - 1, 3);
    w.put(bps as u64 - 1, 5);
    w.put(frames as u64, 36);
    for _ in 0..16 {
        w.put(0, 8);
    }
    out.extend_from_slice(&w.finish());

    let mut chan = vec![0i64; BLOCK];
    for (number, start) in (0..frames).step_by(BLOCK).enumerate() {
        let n = BLOCK.min(frames - start);
        let mut w = Bits::default();
        w.put(0b11_1111_1111_1110, 14);
        w.put(0, 1); // reserved
        w.put(0, 1); // fixed block size
        w.put(0b0111, 4); // block size - 1 follows in 16 bits
        let (rate_code, rate_tail) = rate_code(pcm.sample_rate);
        w.put(rate_code, 4);
        w.put(ch as u64 - 1, 4); // independent channels
        w.put(size_code, 3);
        w.put(0, 1);
        for b in utf8_number(number as u64) {
            w.put(b as u64, 8);
        }
        w.put(n as u64 - 1, 16);
        if let Some((v, bits)) = rate_tail {
            w.put(v, bits);
        }
        let crc = crc8(w.bytes_so_far());
        w.put(crc as u64, 8);
        for c in 0..ch {
            for (i, v) in chan[..n].iter_mut().enumerate() {
                *v = pcm.samples[(start + i) * ch + c] as i64;
            }
            subframe(&mut w, &chan[..n], bps);
        }
        w.align();
        let crc = crc16(w.bytes_so_far());
        w.put(crc as u64, 16);
        out.extend_from_slice(&w.finish());
    }
    Ok(out)
}

/// One channel of one block: constant, verbatim or the best fixed
/// predictor, whichever is smallest.
fn subframe(w: &mut Bits, x: &[i64], bps: u32) {
    let n = x.len();
    if x.iter().all(|&v| v == x[0]) {
        w.put(0, 1);
        w.put(0b000000, 6);
        w.put(0, 1);
        w.put_signed(x[0], bps);
        return;
    }
    let verbatim = n as u64 * bps as u64;
    let mut best: Option<(u64, usize, Vec<i64>, RicePlan)> = None;
    for order in 0..=4usize.min(n.saturating_sub(1)) {
        let r = residual(x, order);
        let (bits, parts) = rice_plan(&r, n, order);
        let total = order as u64 * bps as u64 + bits;
        if best.as_ref().map_or(true, |b| total < b.0) {
            best = Some((total, order, r, parts));
        }
    }
    match best {
        Some((total, order, r, parts)) if total < verbatim => {
            w.put(0, 1);
            w.put(0b001000 | order as u64, 6);
            w.put(0, 1);
            for &v in &x[..order] {
                w.put_signed(v, bps);
            }
            let porder = parts.len().trailing_zeros();
            w.put(0b00, 2); // 4-bit Rice parameters
            w.put(porder as u64, 4);
            let mut i = 0;
            for &(k, len) in &parts {
                w.put(k as u64, 4);
                for &v in &r[i..i + len] {
                    w.put_rice(v, k);
                }
                i += len;
            }
        }
        _ => {
            w.put(0, 1);
            w.put(0b000001, 6);
            w.put(0, 1);
            for &v in x {
                w.put_signed(v, bps);
            }
        }
    }
}

/// The residual of the fixed predictor of `order`, after its warm-up.
fn residual(x: &[i64], order: usize) -> Vec<i64> {
    (order..x.len())
        .map(|i| match order {
            0 => x[i],
            1 => x[i] - x[i - 1],
            2 => x[i] - 2 * x[i - 1] + x[i - 2],
            3 => x[i] - 3 * x[i - 1] + 3 * x[i - 2] - x[i - 3],
            _ => x[i] - 4 * x[i - 1] + 6 * x[i - 2] - 4 * x[i - 3] + x[i - 4],
        })
        .collect()
}

fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

/// Each Rice partition's parameter and length.
type RicePlan = Vec<(u32, usize)>;

/// The cheapest Rice coding of `r`: bits it takes (with the 6-bit method
/// and order fields) and each partition's parameter and length.
fn rice_plan(r: &[i64], n: usize, order: usize) -> (u64, RicePlan) {
    let u: Vec<u64> = r.iter().map(|&v| zigzag(v)).collect();
    let mut best: Option<(u64, RicePlan)> = None;
    for p in 0..=8u32 {
        let parts = 1usize << p;
        if n % parts != 0 || n / parts <= order {
            break;
        }
        let size = n / parts;
        let mut bits = 6u64;
        let mut plan = Vec::with_capacity(parts);
        let mut i = 0;
        for j in 0..parts {
            let len = if j == 0 { size - order } else { size };
            let (k, b) = best_k(&u[i..i + len]);
            bits += 4 + b;
            plan.push((k, len));
            i += len;
        }
        if best.as_ref().map_or(true, |b| bits < b.0) {
            best = Some((bits, plan));
        }
    }
    best.unwrap_or_else(|| {
        let (k, b) = best_k(&u);
        (10 + b, vec![(k, u.len())])
    })
}

/// The Rice parameter (at most 14) that codes `u` in the fewest bits.
fn best_k(u: &[u64]) -> (u32, u64) {
    let cost = |k: u32| u.iter().map(|&v| (v >> k) + 1 + k as u64).sum::<u64>();
    let mean = u.iter().sum::<u64>() / u.len().max(1) as u64;
    let guess = (64 - mean.leading_zeros()).min(14);
    let mut best = (guess, cost(guess));
    for k in [guess.saturating_sub(1), (guess + 1).min(14)] {
        let c = cost(k);
        if c < best.1 {
            best = (k, c);
        }
    }
    best
}

/// The frame header's code for a sample size. Sizes the header can't name
/// would need STREAMINFO, which not every decoder reads.
fn size_code(bps: u32) -> Option<u64> {
    match bps {
        8 => Some(0b001),
        12 => Some(0b010),
        16 => Some(0b100),
        20 => Some(0b101),
        24 => Some(0b110),
        _ => None,
    }
}

/// The frame header's code for a sample rate, and the value that follows
/// the block size when the code doesn't name the rate itself.
fn rate_code(rate: u32) -> (u64, Option<(u64, u32)>) {
    let named = match rate {
        88200 => 0b0001,
        176400 => 0b0010,
        192000 => 0b0011,
        8000 => 0b0100,
        16000 => 0b0101,
        22050 => 0b0110,
        24000 => 0b0111,
        32000 => 0b1000,
        44100 => 0b1001,
        48000 => 0b1010,
        96000 => 0b1011,
        _ => 0,
    };
    if named != 0 {
        (named, None)
    } else if rate < 1 << 16 {
        (0b1101, Some((rate as u64, 16)))
    } else if rate % 10 == 0 && rate / 10 < 1 << 16 {
        (0b1110, Some((rate as u64 / 10, 16)))
    } else {
        (0, None)
    }
}

/// FLAC's UTF-8-like coding of a frame number.
fn utf8_number(v: u64) -> Vec<u8> {
    if v < 0x80 {
        return vec![v as u8];
    }
    let mut bytes = 2;
    while v >= 1u64 << (5 * bytes + 1) {
        bytes += 1;
    }
    let mut out = vec![0u8; bytes];
    let mut x = v;
    for b in out[1..].iter_mut().rev() {
        *b = 0x80 | (x & 0x3F) as u8;
        x >>= 6;
    }
    out[0] = (0xFF00u16 >> bytes) as u8 | x as u8;
    out
}

fn crc8(data: &[u8]) -> u8 {
    let mut c = 0u8;
    for &b in data {
        c ^= b;
        for _ in 0..8 {
            c = if c & 0x80 != 0 {
                (c << 1) ^ 0x07
            } else {
                c << 1
            };
        }
    }
    c
}

fn crc16(data: &[u8]) -> u16 {
    let mut c = 0u16;
    for &b in data {
        c ^= (b as u16) << 8;
        for _ in 0..8 {
            c = if c & 0x8000 != 0 {
                (c << 1) ^ 0x8005
            } else {
                c << 1
            };
        }
    }
    c
}

/// A big-endian bit writer.
#[derive(Default)]
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn put(&mut self, v: u64, bits: u32) {
        let mut bits = bits;
        while bits > 0 {
            let take = bits.min(32);
            bits -= take;
            let part = (v >> bits) & ((1u64 << take) - 1);
            self.acc = (self.acc << take) | part;
            self.n += take;
            while self.n >= 8 {
                self.n -= 8;
                self.out.push((self.acc >> self.n) as u8);
            }
            self.acc &= (1u64 << self.n) - 1;
        }
    }

    fn put_signed(&mut self, v: i64, bits: u32) {
        self.put((v as u64) & ((1u64 << bits) - 1), bits);
    }

    fn put_rice(&mut self, v: i64, k: u32) {
        let u = zigzag(v);
        let mut q = u >> k;
        while q >= 32 {
            self.put(0, 32);
            q -= 32;
        }
        self.put(1, q as u32 + 1);
        if k > 0 {
            self.put(u & ((1u64 << k) - 1), k);
        }
    }

    fn align(&mut self) {
        if self.n > 0 {
            self.put(0, 8 - self.n);
        }
    }

    /// The whole bytes written so far (call after `align` for a frame).
    fn bytes_so_far(&self) -> &[u8] {
        &self.out
    }

    fn finish(mut self) -> Vec<u8> {
        self.align();
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(p: &Pcm) {
        let bytes = encode_flac(p).unwrap();
        let mut r = claxon::FlacReader::new(std::io::Cursor::new(bytes)).unwrap();
        let s = r.streaminfo();
        assert_eq!(s.sample_rate, p.sample_rate);
        assert_eq!(s.channels as u16, p.channels);
        assert_eq!(s.bits_per_sample as u16, p.bits_per_sample);
        assert_eq!(s.samples, Some(p.frames() as u64));
        let back: Vec<i32> = r.samples().map(|v| v.unwrap()).collect();
        assert_eq!(back, p.samples);
    }

    fn noise(n: usize, seed: u64, amp: i32) -> Vec<i32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((s >> 33) as i64 % (2 * amp as i64 + 1) - amp as i64) as i32
            })
            .collect()
    }

    #[test]
    fn mono_16_bit_ticks_and_noise() {
        let mut x = noise(48000 * 3 + 123, 1, 40);
        for t in (0..x.len()).step_by(6000) {
            for i in 0..300.min(x.len() - t) {
                x[t + i] += (20000.0 * (-(i as f64) / 60.0).exp() * (i as f64 * 0.9).sin()) as i32;
            }
        }
        round_trip(&Pcm {
            sample_rate: 48000,
            channels: 1,
            bits_per_sample: 16,
            samples: x,
        });
    }

    #[test]
    fn extremes_silence_and_short_blocks() {
        let mut x = vec![0i32; 5000];
        x.extend([i16::MAX as i32, i16::MIN as i32].repeat(3000));
        x.extend(noise(3, 2, 30000));
        round_trip(&Pcm {
            sample_rate: 44100,
            channels: 1,
            bits_per_sample: 16,
            samples: x,
        });
        round_trip(&Pcm {
            sample_rate: 8000,
            channels: 1,
            bits_per_sample: 16,
            samples: vec![7],
        });
    }

    #[test]
    fn stereo_24_bit() {
        let l = noise(10_000, 3, 8_000_000);
        let r = noise(10_000, 4, 1000);
        let x = l.iter().zip(&r).flat_map(|(&a, &b)| [a, b]).collect();
        round_trip(&Pcm {
            sample_rate: 96000,
            channels: 2,
            bits_per_sample: 24,
            samples: x,
        });
    }

    #[test]
    fn frame_numbers_past_one_byte() {
        assert_eq!(utf8_number(0x7F), [0x7F]);
        assert_eq!(utf8_number(0x80), [0xC2, 0x80]);
        assert_eq!(utf8_number(0x800), [0xE0, 0xA0, 0x80]);
        // 200 blocks: frame numbers past 127 take two bytes.
        round_trip(&Pcm {
            sample_rate: 48000,
            channels: 1,
            bits_per_sample: 16,
            samples: noise(BLOCK * 200 + 5, 5, 100),
        });
    }

    #[test]
    fn cut_spans_segments() {
        let dir = std::env::temp_dir().join(format!("tg-excerpt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 1000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut files = Vec::new();
        for k in 0..3 {
            let f = dir.join(format!("audio-{k}.wav"));
            let mut w = hound::WavWriter::create(&f, spec).unwrap();
            for i in 0..2000 {
                w.write_sample((k * 2000 + i) as i16).unwrap();
            }
            w.finalize().unwrap();
            files.push(f);
        }
        let p = cut(&files, 1.5, 3.0).unwrap();
        assert_eq!(p.samples, (1500..4500).collect::<Vec<i32>>());
        let flac = dir.join("cut.flac");
        write_flac(&flac, &p).unwrap();
        assert_eq!(read(&flac).unwrap(), p);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
