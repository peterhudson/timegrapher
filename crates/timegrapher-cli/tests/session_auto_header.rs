//! A session read on Auto says so in its header (#2432): the text and the
//! HTML report give "Auto" and the beat rate heard, never the standard
//! rate the lock was read at alone, and a set rate as before.
use std::{path::Path, path::PathBuf, process::Command};

struct Dir(PathBuf);
impl Dir {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("tg-auto-header-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .env("TIMEGRAPHER_CONFIG_DIR", std::env::temp_dir())
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap()
}

/// A take folder: 40 s of a balance at `bph` (none: quiet noise with no
/// watch in it), a session file that keeps 17,280, and a recorder's
/// `session.json` with `rec` as its `bph`.
fn take(dir: &Path, bph: Option<u32>, rec: &str) -> String {
    let wav = dir.join("take.wav");
    match bph {
        Some(bph) => {
            let bph = format!("--bph={bph}");
            let wav = wav.to_string_lossy();
            run(&["synth", &wav, "--duration=40", &bph, "--rate=0"]);
        }
        None => std::fs::write(&wav, noise_wav(48_000, 40)).unwrap(),
    }
    std::fs::write(
        dir.join("session.toml"),
        "bph = 17280\n\n[[recording]]\nfile = \"take.wav\"\nposition = \"DU\"\n",
    )
    .unwrap();
    let record = format!(r#"{{"watch": "", "position": "DU", "lift_deg": 52.0, "bph": {rec}}}"#);
    std::fs::write(dir.join("session.json"), record).unwrap();
    dir.to_string_lossy().into_owned()
}

/// `secs` of quiet noise at `rate` Hz as a mono 16-bit WAV: a take with
/// no regular beat in it.
fn noise_wav(rate: u32, secs: u32) -> Vec<u8> {
    let n = rate * secs;
    let mut out = Vec::with_capacity(44 + 2 * n as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + 2 * n).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(2 * rate).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(2 * n).to_le_bytes());
    let mut x: u32 = 1;
    for _ in 0..n {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let v = ((x >> 16) as i32 % 601 - 300) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

fn watch_line(text: &str) -> &str {
    text.lines()
        .find(|l| l.starts_with("Watch "))
        .unwrap_or_else(|| panic!("no Watch line in {text}"))
}

/// The HTML report's Movement row, from the report the text names.
fn movement_row(text: &str) -> String {
    let file = text
        .lines()
        .find_map(|l| l.strip_prefix("Report "))
        .unwrap_or_else(|| panic!("no Report line in {text}"))
        .trim();
    let html = std::fs::read_to_string(file).unwrap();
    let row = html
        .split_once("<tr><th>Movement</th><td>")
        .unwrap_or_else(|| panic!("no Movement row in {html}"))
        .1;
    row.split_once("</td>").unwrap().0.to_string()
}

#[test]
fn off_every_standard_on_auto_the_header_gives_the_rate_heard() {
    let dir = Dir::new("off");
    let take = take(&dir.0, Some(17_570), "null");
    let text = run(&["session", &take]);
    let line = watch_line(&text);
    // 17,570 bph synthesised at 0 s/d, heard at 17570 to its place.
    let want = "Auto, 17570 bph heard, lift angle 52°, the default";
    assert_eq!(line, format!("Watch        {want}"));
    assert_eq!(movement_row(&text), want);
}

#[test]
fn on_a_standard_rate_on_auto_the_header_gives_the_rate_found() {
    let dir = Dir::new("std");
    let take = take(&dir.0, Some(18_000), "null");
    let text = run(&["session", &take]);
    let line = watch_line(&text);
    assert!(line.contains("Auto, 18000 bph found, lift angle"), "{line}");
    assert!(movement_row(&text).starts_with("Auto, 18000 bph found, "));
}

#[test]
fn a_set_rate_is_given_as_before() {
    let dir = Dir::new("set");
    let take = take(&dir.0, Some(17_570), "17280");
    let text = run(&["session", &take]);
    let line = watch_line(&text);
    assert!(line.contains("17280 bph, lift angle"), "{line}");
    assert!(!line.contains("Auto"), "{line}");
    assert!(movement_row(&text).starts_with("17280 bph, "));
}

#[test]
fn with_no_watch_heard_on_auto_the_header_says_auto_alone() {
    let dir = Dir::new("none");
    let take = take(&dir.0, None, "null");
    let text = run(&["session", &take]);
    let want = "Auto, lift angle 52°, the default";
    assert_eq!(watch_line(&text), format!("Watch        {want}"));
    assert_eq!(movement_row(&text), want);
}
