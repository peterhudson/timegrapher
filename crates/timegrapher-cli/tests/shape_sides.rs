//! `timegrapher shape` reads the recording as `analyze` does: the same
//! beats with the same sides, so its Tick and Tock are analyze's Tick and
//! Tock, and each side's amplitude is analyze's (#2104).
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use timegrapher_core::synth::{generate, SynthConfig};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tg-shape-sides-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The command's stdout, run with a settings folder of its own.
fn run(config: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .args(args)
        .env("TIMEGRAPHER_CONFIG_DIR", config)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn json(config: &Path, args: &[&str]) -> Value {
    let text = run(config, args);
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{args:?}: {e}\n{text}"))
}

/// A watch whose two sides sound apart: the even beats' unlock comes
/// 0.3 ms earlier (a larger unlock-to-drop time, so a smaller amplitude)
/// and the odd beats' impulse sits later, with a beat error of
/// `beat_error_ms` (its sign sets which side the engine names Tick).
fn two_sided_wav(path: &Path, beat_error_ms: f64) {
    let audio = generate(
        &SynthConfig {
            duration_s: 20.0,
            beat_error_ms,
            even_unlock_lead_s: 0.0003,
            odd_impulse_at: Some(0.6),
            ..Default::default()
        },
        |_| 275.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(path, &audio).unwrap();
}

/// The figure after `word ` in `line`, as a whole number.
fn after(line: &str, word: &str) -> i64 {
    let at = line
        .find(word)
        .unwrap_or_else(|| panic!("{word} in {line}"))
        + word.len();
    let digits: String = line[at..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits
        .parse()
        .unwrap_or_else(|_| panic!("{word} in {line}"))
}

#[test]
fn shape_names_the_sides_and_reads_the_amplitudes_as_analyze_does() {
    let dir = scratch("sides");
    let config = dir.join("config");
    // Both signs of beat error, so one of the two takes has its Tick on
    // the odd beats of a plain beat count.
    for (k, be) in [0.4, -0.4].into_iter().enumerate() {
        let wav = dir.join(format!("watch{k}.wav"));
        two_sided_wav(&wav, be);
        let w = wav.to_str().unwrap();
        let a = json(&config, &["analyze", w, "--json"]);
        let s = json(&config, &["shape", w, "--json"]);
        for side in ["even", "odd"] {
            let of_analyze = a["shape"][side]["i12_ms"].as_f64();
            let of_shape = s[side]["windows"]["i12_ms"].as_f64();
            assert!(of_analyze.is_some(), "{a}");
            assert_eq!(of_shape, of_analyze, "{side} at {be} ms");
        }
        // The sides' amplitudes, as analyze words them and as shape's
        // table gives them.
        let text = run(&config, &["analyze", w]);
        let line = text.lines().find(|l| l.starts_with("Amplitude")).unwrap();
        let (tick, tock) = (after(line, "Tick"), after(line, "Tock"));
        assert_ne!(tick, tock, "the sides should differ: {line}");
        let table = run(&config, &["shape", w]);
        let row = table
            .lines()
            .find(|l| l.starts_with("amplitude, deg"))
            .unwrap_or_else(|| panic!("{table}"));
        let cells: Vec<&str> = row["amplitude, deg".len()..].split_whitespace().collect();
        assert_eq!(
            cells,
            [tick.to_string(), tock.to_string(), "-".into(), "-".into()],
            "{line}\n{row}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// On a co-axial, the amplitude row carries `analyze`'s caveat for it,
/// in core's words, as `analyze` says it beside the same figure.
#[test]
fn shape_says_a_co_axial_amplitude_is_an_estimate() {
    let dir = scratch("coaxial");
    let config = dir.join("config");
    let wav = dir.join("watch.wav");
    let w = wav.to_str().unwrap();
    run(&config, &["synth", w, "--duration", "20", "--bph", "21600"]);
    let words = timegrapher_core::measure::COAXIAL_ESTIMATE_SHORT;
    let analyze = run(&config, &["analyze", w, "--calibre", "3861"]);
    assert!(analyze.contains(words), "{analyze}");
    let table = run(&config, &["shape", w, "--calibre", "3861"]);
    let rows: Vec<&str> = table.lines().collect();
    let at = rows
        .iter()
        .position(|l| l.starts_with("amplitude, deg"))
        .unwrap_or_else(|| panic!("{table}"));
    let under: Vec<&str> = rows[at + 1..]
        .iter()
        .take_while(|l| l.starts_with("  "))
        .map(|l| l.trim())
        .collect();
    assert!(under.contains(&words), "{table}");
    // A caveat on the rate alone, which analyze gives this take too, is
    // not said under the amplitude.
    let clock = timegrapher_core::measure::Caveat::ClockUnknown
        .short(52.0, 0)
        .unwrap();
    assert!(analyze.contains(&clock), "{analyze}");
    assert!(!under.contains(&clock.as_str()), "{table}");
    // A lever's amplitude carries no such words.
    let lever = run(&config, &["shape", w]);
    assert!(lever.contains("amplitude, deg"), "{lever}");
    assert!(!lever.contains(words), "{lever}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// On noise, where `analyze` finds no regular beat, `shape` says so in
/// `analyze`'s words, that line alone, and prints no table; its JSON keeps
/// its fields, with no window and no figure (#2371). Both over 10 s (the
/// timing scatter decides) and under it (no verdict either way, Norm on
/// #2333: `analyze` says regularity is not assessed, and so does `shape`).
#[test]
fn shape_measures_nothing_on_noise() {
    let dir = scratch("noise");
    let config = dir.join("config");
    for seconds in [20, 5] {
        let wav = dir.join(format!("noise{seconds}.wav"));
        let w = wav.to_str().unwrap();
        let mut seed = 1u64;
        let samples = (0..seconds * 48_000)
            .map(|_| {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.2
            })
            .collect();
        let audio = timegrapher_core::Audio {
            samples,
            sample_rate: 48_000,
        };
        timegrapher_core::audio::write_wav(&wav, &audio).unwrap();
        let analyze = run(&config, &["analyze", w]);
        let said: Vec<&str> = analyze.lines().skip(1).collect();
        if seconds >= 10 {
            assert!(
                said.len() == 1 && said[0].starts_with("No regular beat found"),
                "{seconds} s: {analyze}"
            );
        } else {
            // Under 10 s there is no verdict either way (Norm on #2333).
            let not_assessed = timegrapher_core::measure::not_assessed_text(seconds as f64);
            assert_eq!(
                said.first(),
                Some(&not_assessed.as_str()),
                "{seconds} s: {analyze}"
            );
        }
        let text = run(&config, &["shape", w]);
        assert_eq!(text.trim_end(), said[0], "{seconds} s: {text}");
        let s = json(&config, &["shape", w, "--json"]);
        assert_eq!(s["windows"], Value::Array(vec![]), "{seconds} s: {s}");
        for side in ["even", "odd"] {
            assert_eq!(s[side]["windows_measured"], 0, "{seconds} s {side}: {s}");
            assert!(
                s[side]["windows"]["i12_ms"].is_null(),
                "{seconds} s {side}: {s}"
            );
            assert!(s[side]["whole"].is_null(), "{seconds} s {side}: {s}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Half a second of a watch: too little for any verdict, so `analyze`
/// reads no shape and says "Beat regularity not assessed". `shape` says
/// exactly that line, on the same seconds, and prints no table (#2371).
#[test]
fn shape_on_a_moment_of_sound_says_what_analyze_says() {
    let dir = scratch("moment");
    let config = dir.join("config");
    let wav = dir.join("moment.wav");
    let w = wav.to_str().unwrap();
    run(&config, &["synth", w, "--duration", "0.5"]);
    let a = json(&config, &["analyze", w, "--json"]);
    assert!(a["shape"].is_null(), "{a}");
    let analyze = run(&config, &["analyze", w]);
    let said = analyze
        .lines()
        .find(|l| l.starts_with("Beat regularity not assessed"))
        .unwrap_or_else(|| panic!("{analyze}"));
    let text = run(&config, &["shape", w]);
    assert_eq!(text.trim_end(), said, "{text}");
    let s = json(&config, &["shape", w, "--json"]);
    assert_eq!(s["windows"], Value::Array(vec![]), "{s}");
    let _ = std::fs::remove_dir_all(&dir);
}
