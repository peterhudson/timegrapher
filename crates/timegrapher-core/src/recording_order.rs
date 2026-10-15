//! Physical recording files in the desktop's established joining order.
//!
//! Callers parse their session manifest and pass its file entries. This
//! module chooses only file order; it does not apply measurement settings,
//! position intervals, clock corrections or break mappings.

use std::path::{Path, PathBuf};

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// One explicit path unchanged, or a folder's direct WAV/FLAC files.
/// Existing files listed by the manifest come first, then unlisted files
/// in natural name order. Missing, nested and whole-folder entries do not
/// rank files; repeated entries rank a physical file only once.
pub fn recordings_in(path: &Path, listed: &[String]) -> Result<Vec<PathBuf>, String> {
    if !path.is_dir() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(path)
        .map_err(|e| format!("Can't read the folder {}: {e}", path.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|p| {
            p.is_file()
                && p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                    e.eq_ignore_ascii_case("wav") || e.eq_ignore_ascii_case("flac")
                })
        })
        .collect();
    if files.is_empty() {
        return Err(format!(
            "There are no WAV or FLAC files in {}.",
            path.display()
        ));
    }
    files.sort_by(|a, b| natural_order(&file_name(a), &file_name(b)));
    let mut names: Vec<String> = Vec::new();
    for entry in listed {
        let file = path.join(entry);
        if file.is_file() && file.parent().is_some_and(|parent| parent == path) {
            let name = file_name(&file);
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    // Stable sorting retains natural order among unlisted files.
    files.sort_by_key(|file| {
        names
            .iter()
            .position(|name| name == &file_name(file))
            .unwrap_or(usize::MAX)
    });
    Ok(files)
}

/// Numbers in names count as numbers, without integer overflow. Otherwise
/// compare ignoring case, with the original spelling as the final tie break.
pub fn natural_order(a: &str, b: &str) -> std::cmp::Ordering {
    fn chunks(s: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for c in s.chars() {
            let digit = c.is_ascii_digit();
            match out.last_mut() {
                Some((d, run)) if *d == digit => run.push(c),
                _ => out.push((digit, c.to_string())),
            }
        }
        out
    }
    let (ca, cb) = (chunks(a), chunks(b));
    for ((da, ra), (db, rb)) in ca.iter().zip(&cb) {
        let o = match (da, db) {
            (true, true) => {
                let (ta, tb) = (ra.trim_start_matches('0'), rb.trim_start_matches('0'));
                ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb))
            }
            _ => ra.to_lowercase().cmp(&rb.to_lowercase()),
        };
        if o.is_ne() {
            return o;
        }
    }
    ca.len().cmp(&cb.len()).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Folder(PathBuf);
    impl Folder {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("tg-recording-order-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn file(&self, name: &str) {
            std::fs::write(self.0.join(name), []).unwrap();
        }
        fn names(&self, listed: &[&str]) -> Vec<String> {
            recordings_in(
                &self.0,
                &listed.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            )
            .unwrap()
            .iter()
            .map(|p| file_name(p))
            .collect()
        }
    }
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn natural_comparison_retains_desktop_numeric_case_and_tie_rules() {
        use std::cmp::Ordering::{Equal, Less};
        for (a, b) in [
            ("part2.wav", "part10.wav"),
            ("take_part01-099", "take_part02-001"),
            ("A.wav", "b.wav"),
            ("part02.wav", "part2.wav"),
            ("A.wav", "a.wav"),
            (
                "part999999999999999999999999999999",
                "part1000000000000000000000000000000",
            ),
        ] {
            assert_eq!(natural_order(a, b), Less, "{a} versus {b}");
            assert_eq!(natural_order(b, a), Less.reverse());
        }
        assert_eq!(natural_order("same00.wav", "same00.wav"), Equal);
    }

    #[test]
    fn folder_selection_is_direct_audio_files_in_natural_order() {
        let folder = Folder::new();
        for name in ["part10.WAV", "part2.flac", "part01.wav", "notes.txt"] {
            folder.file(name);
        }
        std::fs::create_dir(folder.0.join("directory.wav")).unwrap();
        assert_eq!(
            folder.names(&[]),
            ["part01.wav", "part2.flac", "part10.WAV"]
        );
    }

    #[test]
    fn manifest_first_occurrences_rank_only_existing_direct_files() {
        let folder = Folder::new();
        for name in ["part10.wav", "part2.wav", "part1.wav", "part3.wav"] {
            folder.file(name);
        }
        std::fs::create_dir(folder.0.join("nested")).unwrap();
        folder.file("nested/part1.wav");
        assert_eq!(
            folder.names(&[
                "missing.wav",
                ".",
                "nested/part1.wav",
                "part10.wav",
                "part2.wav",
                "part10.wav"
            ]),
            ["part10.wav", "part2.wav", "part1.wav", "part3.wav"]
        );
        assert_eq!(
            folder.names(&[".", "missing.wav", "nested/part1.wav"]),
            ["part1.wav", "part2.wav", "part3.wav", "part10.wav"]
        );
    }

    #[test]
    fn empty_folders_fail_and_explicit_paths_are_unchanged() {
        let folder = Folder::new();
        std::fs::create_dir(folder.0.join("sub.wav")).unwrap();
        assert!(recordings_in(&folder.0, &[]).is_err());
        let explicit = folder.0.join("not-present.anything");
        assert_eq!(recordings_in(&explicit, &[]).unwrap(), [explicit]);
    }
}
