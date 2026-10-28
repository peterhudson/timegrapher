//! What the targets share: a scratch folder of this process's own, since
#![allow(dead_code)]
//! the readers read files.
use std::path::PathBuf;

pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tg-fuzz-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The parts of a fuzz input that holds several files, split where
/// [`SPLIT`] appears.
pub const SPLIT: &[u8] = b"\n--8<--\n";

pub fn parts(data: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut rest = data;
    while let Some(i) = rest.windows(SPLIT.len()).position(|w| w == SPLIT) {
        out.push(&rest[..i]);
        rest = &rest[i + SPLIT.len()..];
    }
    out.push(rest);
    out
}
