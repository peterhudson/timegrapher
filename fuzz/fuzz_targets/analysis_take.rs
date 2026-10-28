//! A take kept as its analysis alone (`analysis/`), read as the app, the
//! CLI and the Library read one, then measured. The input is the header,
//! the beats, the windows and the profiles, split by `common::SPLIT`; each
//! file is gzipped before it is written, unless the input starts with `!`,
//! when they are written as they are (for the gzip reading itself).
#![no_main]
use flate2::{write::GzEncoder, Compression};
use libfuzzer_sys::fuzz_target;
use std::io::Write;
use timegrapher_core::analysis_take::{self, BEATS, DIR, HEADER, PROFILES, WINDOWS};

#[path = "common.rs"]
mod common;

fn gzip(b: &[u8]) -> Vec<u8> {
    let mut e = GzEncoder::new(Vec::new(), Compression::fast());
    e.write_all(b).unwrap();
    e.finish().unwrap()
}

fuzz_target!(|data: &[u8]| {
    let (raw, data) = match data.split_first() {
        Some((b'!', rest)) => (true, rest),
        _ => (false, data),
    };
    let p = common::parts(data);
    let take = common::scratch("analysis_take");
    let dir = take.join(DIR);
    std::fs::create_dir_all(&dir).unwrap();
    let part = |i: usize| p.get(i).copied().unwrap_or_default();
    std::fs::write(dir.join(HEADER), part(0)).unwrap();
    for (i, name) in [(1, BEATS), (2, WINDOWS), (3, PROFILES)] {
        let b = if raw { part(i).to_vec() } else { gzip(part(i)) };
        std::fs::write(dir.join(name), b).unwrap();
    }
    if let Ok(s) = analysis_take::read(&take) {
        let log = &s.log;
        let _ = timegrapher_core::session::measure(log, None, 0.0, log.duration_s);
    }
});
