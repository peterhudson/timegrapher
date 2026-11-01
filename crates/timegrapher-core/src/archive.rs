//! The little a report bundle needs from ZIP and SHA-256, on the `zip` and
//! `sha2` crates. Bundles are written with entries stored as they are (the
//! audio is FLAC already and the rest is small); they read back stored or
//! deflated, so a bundle someone unpacked and zipped again still replays.
//! What a bundle may hold and run is checked by its reader, not here.

use std::io::{self, Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// The most a bundle may unpack to, bytes. Real bundles are a few MB; the
/// 1 GiB limit leaves ample room while keeping a crafted one from filling memory.
const MAX_UNPACKED: u64 = 1 << 30;

/// At most 10,000 files and directories in an unpacked bundle. Reports have
/// only tens of entries; this also bounds work and storage for empty entries.
const MAX_ENTRIES: usize = 10_000;
/// At most 16 path components below the bundle root, including the file name.
/// Reports use only a few levels; this bounds traversal of deeply nested input.
const MAX_DEPTH: usize = 16;

/// A decompressed byte limit in the report reader's MB/GB wording.
/// These units retain the ZIP reader's binary sizes and existing labels.
pub(crate) fn unpacked_size(bytes: u64) -> String {
    const MB: u64 = 1 << 20;
    const GB: u64 = 1 << 30;
    if bytes >= GB {
        format!("{} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{} MB", bytes as f64 / MB as f64)
    } else {
        format!("{bytes} bytes")
    }
}

/// Read no more than one byte past the remaining unpacked budget.
fn read_limited(mut reader: impl Read, limit: u64) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    reader.by_ref().take(limit + 1).read_to_end(&mut data)?;
    Ok(data)
}

/// The files of an unpacked report bundle, with names relative to `path`.
/// Links and special files are errors: the bundle writer never makes them.
pub fn read_folder(path: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    read_folder_with_limits(path, MAX_UNPACKED, MAX_ENTRIES, MAX_DEPTH)
}

fn read_folder_with_limits(
    path: &Path,
    max_unpacked: u64,
    max_entries: usize,
    max_depth: usize,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let bad = |p: &Path, what: &dyn std::fmt::Display| format!("{}: {what}", p.display());
    let metadata = std::fs::symlink_metadata(path).map_err(|e| bad(path, &e))?;
    if metadata.file_type().is_symlink() {
        return Err(bad(path, &"symlink; not a report bundle"));
    }
    let mut stack = vec![(std::fs::read_dir(path).map_err(|e| bad(path, &e))?, 0)];
    let mut entries = Vec::new();
    let mut count = 0;
    let mut left = max_unpacked;
    while let Some((dir, depth)) = stack.last_mut() {
        let Some(entry) = dir.next() else {
            stack.pop();
            continue;
        };
        let entry = entry.map_err(|e| bad(path, &e))?;
        let p = entry.path();
        let depth = *depth + 1;
        count += 1;
        if count > max_entries {
            return Err(bad(
                &p,
                &format!("more than {max_entries} entries; not a report bundle"),
            ));
        }
        if depth > max_depth {
            return Err(bad(
                &p,
                &format!("depth exceeds {max_depth}; not a report bundle"),
            ));
        }
        let kind = entry.file_type().map_err(|e| bad(&p, &e))?;
        if kind.is_symlink() {
            return Err(bad(&p, &"symlink; not a report bundle"));
        }
        if kind.is_dir() {
            stack.push((std::fs::read_dir(&p).map_err(|e| bad(&p, &e))?, depth));
        } else if kind.is_file() {
            let file = std::fs::File::open(&p).map_err(|e| bad(&p, &e))?;
            let data = read_limited(file, left).map_err(|e| bad(&p, &e))?;
            left = left.checked_sub(data.len() as u64).ok_or_else(|| {
                bad(
                    &p,
                    &format!(
                        "unpacks to more than {}; not a report bundle",
                        unpacked_size(max_unpacked)
                    ),
                )
            })?;
            let name = p
                .strip_prefix(path)
                .map_err(|e| bad(&p, &e))?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            entries.push((name, data));
        } else {
            return Err(bad(
                &p,
                &"not a regular file or directory; not a report bundle",
            ));
        }
    }
    Ok(entries)
}

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
    read_zip_with_limit(path, MAX_UNPACKED)
}

fn read_zip_with_limit(path: &Path, max_unpacked: u64) -> Result<Vec<(String, Vec<u8>)>, String> {
    let bad = |what: &dyn std::fmt::Display| format!("{}: {what}", path.display());
    let file = std::fs::File::open(path).map_err(|e| bad(&e))?;
    let mut zip = ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| bad(&e))?;
    let mut entries = Vec::with_capacity(zip.len());
    let mut left = max_unpacked;
    for i in 0..zip.len() {
        let f = zip.by_index(i).map_err(|e| bad(&e))?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().map_err(|e| bad(&e))?.into_owned();
        let data = read_limited(f, left).map_err(|e| bad(&format!("{name} is damaged ({e})")))?;
        left = left.checked_sub(data.len() as u64).ok_or_else(|| {
            bad(&format!(
                "unpacks to more than {}; not a report bundle",
                unpacked_size(max_unpacked)
            ))
        })?;
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

#[cfg(test)]
mod size_limit_tests {
    use super::*;

    fn folder(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("tg-folder-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn folder_byte_limit_is_total_and_inclusive() {
        let p = folder("bytes");
        std::fs::write(p.join("a"), [0; 8]).unwrap();
        std::fs::create_dir(p.join("nested")).unwrap();
        std::fs::write(p.join("nested/b"), [0; 8]).unwrap();
        assert_eq!(read_folder_with_limits(&p, 16, 3, 2).unwrap().len(), 2);
        std::fs::write(p.join("nested/b"), [0; 9]).unwrap();
        let err = read_folder_with_limits(&p, 16, 3, 2).unwrap_err();
        assert!(err.contains("more than 16 bytes"), "{err}");
        std::fs::remove_dir_all(p).unwrap();
    }

    #[test]
    fn folder_entry_limit_counts_empty_directories() {
        let p = folder("entries");
        std::fs::create_dir_all(p.join("a/b")).unwrap();
        assert!(read_folder_with_limits(&p, 0, 2, 3).unwrap().is_empty());
        std::fs::create_dir(p.join("c")).unwrap();
        let err = read_folder_with_limits(&p, 0, 2, 3).unwrap_err();
        assert!(err.contains("more than 2 entries"), "{err}");
        std::fs::remove_dir_all(p).unwrap();
    }

    #[test]
    fn folder_depth_limit_counts_path_components() {
        let p = folder("depth");
        std::fs::create_dir_all(p.join("a/b")).unwrap();
        assert!(read_folder_with_limits(&p, 0, 4, 2).unwrap().is_empty());
        std::fs::write(p.join("a/b/c"), []).unwrap();
        let err = read_folder_with_limits(&p, 0, 4, 2).unwrap_err();
        assert!(err.contains("depth exceeds 2"), "{err}");
        std::fs::remove_dir_all(p).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn folder_file_symlink_is_refused_by_path() {
        let p = folder("file-link");
        let target = p.join("target");
        std::fs::write(&target, b"data").unwrap();
        let link = p.join("link");
        std::os::unix::fs::symlink(target, &link).unwrap();
        let err = read_folder(&p).unwrap_err();
        assert!(
            err.contains("symlink") && err.contains(&link.display().to_string()),
            "{err}"
        );
        std::fs::remove_dir_all(p).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn folder_directory_symlink_is_refused_by_path() {
        let p = folder("dir-link");
        let target = p.join("target");
        std::fs::create_dir(&target).unwrap();
        let link = p.join("link");
        std::os::unix::fs::symlink(target, &link).unwrap();
        let err = read_folder(&p).unwrap_err();
        assert!(
            err.contains("symlink") && err.contains(&link.display().to_string()),
            "{err}"
        );
        // The supplied root must not be followed either.
        let err = read_folder(&link).unwrap_err();
        assert!(
            err.contains("symlink") && err.contains(&link.display().to_string()),
            "{err}"
        );
        std::fs::remove_dir_all(p).unwrap();
    }
}

#[cfg(test)]
mod limit_wording_tests {
    use super::*;

    #[test]
    fn folder_and_zip_limits_use_megabytes() {
        let path = std::env::temp_dir().join(format!("tg-limit-words-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        let limit = 1 << 20;
        let data = vec![0; limit + 1];
        std::fs::write(path.join("data"), &data).unwrap();
        let folder_err = read_folder_with_limits(&path, limit as u64, 1, 1).unwrap_err();
        let zip = path.join("report.zip");
        write_zip(&zip, &[("data".into(), data)]).unwrap();
        let zip_err = read_zip_with_limit(&zip, limit as u64).unwrap_err();
        for err in [folder_err, zip_err] {
            assert!(
                err.contains("unpacks to more than 1 MB; not a report bundle"),
                "{err}"
            );
        }
        assert_eq!(unpacked_size(MAX_UNPACKED), "1 GB");
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[cfg(test)]
mod bounded_read_tests {
    use super::*;
    use std::io;

    struct Counted {
        bytes: usize,
        allowed: usize,
    }

    impl Read for Counted {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            assert!(
                buf.len() <= self.allowed - self.bytes,
                "read exceeds limit plus one"
            );
            buf.fill(0);
            self.bytes += buf.len();
            Ok(buf.len())
        }
    }

    #[test]
    fn folder_read_stops_at_limit_plus_one() {
        let limit = 16;
        let mut reader = Counted {
            bytes: 0,
            allowed: limit + 1,
        };
        let data = read_limited(&mut reader, limit as u64).unwrap();
        assert_eq!(data.len(), limit + 1);
        assert_eq!(reader.bytes, limit + 1);
    }
}
