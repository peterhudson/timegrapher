//! The little a report bundle needs from ZIP and SHA-256, on the `zip` and
//! `sha2` crates. Bundles are written with entries stored as they are (the
//! audio is FLAC already and the rest is small); they read back stored or
//! deflated, so a bundle someone unpacked and zipped again still replays.
//! What a bundle may hold and run is checked by its reader, not here.

use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// The most a bundle may unpack to, bytes. Real bundles are a few MB; the
/// limit keeps a crafted one from filling memory.
const MAX_UNPACKED: u64 = 1 << 30;

/// Write `entries` (name, bytes) as a ZIP file with stored entries.
pub fn write_zip(path: &Path, entries: &[(String, Vec<u8>)]) -> Result<(), String> {
    let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    // Stored, no Zip64, and the default 1980-01-01 time, so the same
    // entries always make the same bytes.
    let opts = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(false);
    for (name, data) in entries {
        if data.len() >= u32::MAX as usize {
            return Err(format!("{name}: too big for a report bundle"));
        }
        zip.start_file(name.as_str(), opts)
            .and_then(|()| zip.write_all(data).map_err(Into::into))
            .map_err(|e| format!("{name}: {e}"))?;
    }
    let out = zip
        .finish()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::write(path, out.into_inner()).map_err(|e| format!("{}: {e}", path.display()))
}

/// The file entries of a ZIP file, stored or deflated, in order. Folder
/// entries are left out. A damaged entry (a bad CRC, a truncated file) is
/// an error.
pub fn read_zip(path: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let bad = |what: &dyn std::fmt::Display| format!("{}: {what}", path.display());
    let file = std::fs::File::open(path).map_err(|e| bad(&e))?;
    let mut zip = ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| bad(&e))?;
    let mut entries = Vec::with_capacity(zip.len());
    let mut left = MAX_UNPACKED;
    for i in 0..zip.len() {
        let f = zip.by_index(i).map_err(|e| bad(&e))?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().map_err(|e| bad(&e))?.into_owned();
        let mut data = Vec::new();
        // One byte past the limit tells a bundle at the limit from one over.
        f.take(left + 1)
            .read_to_end(&mut data)
            .map_err(|e| bad(&format!("{name} is damaged ({e})")))?;
        left = left
            .checked_sub(data.len() as u64)
            .ok_or_else(|| bad(&"unpacks to more than 1 GB; not a report bundle"))?;
        entries.push((name, data));
    }
    Ok(entries)
}

/// SHA-256 of `data`, as lower-case hex.
pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("tg-zip-{tag}-{}.zip", std::process::id()))
    }

    fn sample() -> Vec<(String, Vec<u8>)> {
        vec![
            ("manifest.json".to_string(), b"{}".to_vec()),
            ("audio/excerpt.flac".to_string(), (0..=255u8).collect()),
            ("empty.txt".to_string(), Vec::new()),
        ]
    }

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
    fn zip_round_trip() {
        let p = temp("round");
        let entries = sample();
        write_zip(&p, &entries).unwrap();
        assert_eq!(read_zip(&p).unwrap(), entries);
        // The same entries make the same bytes.
        let first = std::fs::read(&p).unwrap();
        write_zip(&p, &entries).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), first);
        std::fs::remove_file(&p).unwrap();
    }

    #[test]
    fn a_rezipped_bundle_with_folders_and_deflate_reads() {
        let p = temp("deflate");
        let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.add_directory("audio/", opts).unwrap();
        for (name, data) in sample() {
            zip.start_file(name, opts).unwrap();
            zip.write_all(&data).unwrap();
        }
        std::fs::write(&p, zip.finish().unwrap().into_inner()).unwrap();
        assert_eq!(read_zip(&p).unwrap(), sample());
        std::fs::remove_file(&p).unwrap();
    }

    #[test]
    fn a_damaged_entry_is_an_error() {
        let p = temp("crc");
        let entries = vec![("audio/excerpt.flac".to_string(), vec![7u8; 4096])];
        write_zip(&p, &entries).unwrap();
        let mut b = std::fs::read(&p).unwrap();
        let at = b.windows(4).position(|w| w == [7, 7, 7, 7]).unwrap() + 100;
        b[at] ^= 0xff;
        std::fs::write(&p, &b).unwrap();
        let err = read_zip(&p).unwrap_err();
        assert!(err.contains("damaged"), "{err}");
        std::fs::remove_file(&p).unwrap();
    }

    /// Every truncation, and every byte flipped in turn, of a small bundle
    /// either reads or gives an error; none panics.
    #[test]
    fn malformed_zips_give_errors_not_panics() {
        let p = temp("malformed");
        write_zip(&p, &sample()).unwrap();
        let good = std::fs::read(&p).unwrap();
        let q = temp("malformed-case");
        let try_bytes = |b: &[u8]| {
            std::fs::write(&q, b).unwrap();
            let run = std::panic::catch_unwind(|| read_zip(&q));
            assert!(run.is_ok(), "read_zip panicked on {} bytes", b.len());
        };
        for n in 0..good.len() {
            try_bytes(&good[..n]);
        }
        for i in 0..good.len() {
            for flip in [0x01, 0x80, 0xff] {
                let mut b = good.clone();
                b[i] ^= flip;
                try_bytes(&b);
            }
        }
        // Not a ZIP at all.
        try_bytes(b"PK\x05\x06");
        try_bytes(&[0u8; 64]);
        std::fs::remove_file(&p).unwrap();
        std::fs::remove_file(&q).unwrap();
    }
}
