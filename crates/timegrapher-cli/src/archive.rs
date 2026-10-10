//! The little a report bundle needs from ZIP: entries stored as they are
//! (the audio is FLAC already and the rest is small), read back by the
//! same rules, and the SHA-256 the manifest lists each file by.

use std::path::Path;

/// Write `entries` (name, bytes) as a ZIP file with stored entries.
pub fn write_zip(path: &Path, entries: &[(String, Vec<u8>)]) -> Result<(), String> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        if data.len() >= u32::MAX as usize || out.len() >= u32::MAX as usize {
            return Err(format!("{name}: too big for a report bundle"));
        }
        let crc = crc32(data);
        let offset = out.len() as u32;
        let n = name.as_bytes();
        // Local file header.
        put32(&mut out, 0x0403_4b50);
        put16(&mut out, 20); // version needed
        put16(&mut out, 0x0800); // UTF-8 names
        put16(&mut out, 0); // stored
        put16(&mut out, 0); // time
        put16(&mut out, 0x21); // 1980-01-01
        put32(&mut out, crc);
        put32(&mut out, data.len() as u32);
        put32(&mut out, data.len() as u32);
        put16(&mut out, n.len() as u16);
        put16(&mut out, 0);
        out.extend_from_slice(n);
        out.extend_from_slice(data);
        // Central directory entry.
        put32(&mut central, 0x0201_4b50);
        put16(&mut central, 20);
        put16(&mut central, 20);
        put16(&mut central, 0x0800);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put16(&mut central, 0x21);
        put32(&mut central, crc);
        put32(&mut central, data.len() as u32);
        put32(&mut central, data.len() as u32);
        put16(&mut central, n.len() as u16);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put32(&mut central, 0);
        put32(&mut central, offset);
        central.extend_from_slice(n);
    }
    let at = out.len() as u32;
    let size = central.len() as u32;
    out.extend_from_slice(&central);
    put32(&mut out, 0x0605_4b50);
    put16(&mut out, 0);
    put16(&mut out, 0);
    put16(&mut out, entries.len() as u16);
    put16(&mut out, entries.len() as u16);
    put32(&mut out, size);
    put32(&mut out, at);
    put16(&mut out, 0);
    std::fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
}

/// The entries of a ZIP file whose entries are stored, as `write_zip`
/// writes them.
pub fn read_zip(path: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let bad = |what: &str| format!("{}: {what}", path.display());
    let eocd = (0..b.len().saturating_sub(21))
        .rev()
        .find(|&i| get32(&b, i) == Some(0x0605_4b50))
        .ok_or_else(|| bad("not a ZIP file"))?;
    let count = get16(&b, eocd + 10).ok_or_else(|| bad("truncated"))? as usize;
    let mut at = get32(&b, eocd + 16).ok_or_else(|| bad("truncated"))? as usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        if get32(&b, at) != Some(0x0201_4b50) {
            return Err(bad("damaged central directory"));
        }
        let field = |o: usize| get16(&b, at + o).map(|v| v as usize);
        let method = field(10).ok_or_else(|| bad("truncated"))?;
        let size = get32(&b, at + 20).ok_or_else(|| bad("truncated"))? as usize;
        let crc = get32(&b, at + 16).ok_or_else(|| bad("truncated"))?;
        let (n, extra, comment) = (field(28), field(30), field(32));
        let (Some(n), Some(extra), Some(comment)) = (n, extra, comment) else {
            return Err(bad("truncated"));
        };
        let local = get32(&b, at + 42).ok_or_else(|| bad("truncated"))? as usize;
        let name = b
            .get(at + 46..at + 46 + n)
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .ok_or_else(|| bad("truncated"))?;
        at += 46 + n + extra + comment;
        if name.ends_with('/') {
            continue;
        }
        if method != 0 {
            return Err(bad(&format!(
                "{name} is compressed; unpack the bundle and pass its folder instead"
            )));
        }
        let ln = get16(&b, local + 26).ok_or_else(|| bad("truncated"))? as usize;
        let le = get16(&b, local + 28).ok_or_else(|| bad("truncated"))? as usize;
        let start = local + 30 + ln + le;
        let data = b
            .get(start..start + size)
            .ok_or_else(|| bad("truncated"))?
            .to_vec();
        if crc32(&data) != crc {
            return Err(bad(&format!("{name} is damaged (CRC mismatch)")));
        }
        entries.push((name, data));
    }
    Ok(entries)
}

fn put16(o: &mut Vec<u8>, v: u16) {
    o.extend_from_slice(&v.to_le_bytes());
}

fn put32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_le_bytes());
}

fn get16(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn get32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// SHA-256 of `data`, as lower-case hex.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, c) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|v| format!("{v:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_values() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let long = vec![b'a'; 1000];
        assert_eq!(
            sha256_hex(&long),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
    }

    #[test]
    fn crc32_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn zip_round_trip() {
        let p = std::env::temp_dir().join(format!("tg-zip-{}.zip", std::process::id()));
        let entries = vec![
            ("manifest.json".to_string(), b"{}".to_vec()),
            ("audio/excerpt.flac".to_string(), (0..=255u8).collect()),
            ("empty.txt".to_string(), Vec::new()),
        ];
        write_zip(&p, &entries).unwrap();
        assert_eq!(read_zip(&p).unwrap(), entries);
        std::fs::remove_file(&p).unwrap();
    }
}
