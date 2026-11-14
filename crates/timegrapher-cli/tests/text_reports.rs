//! `timegrapher session` writes the plain-text Positions Test and Dynamic
//! Poise reports (core `text_report`, #1508, #1509) beside report.html,
//! and gives them in its JSON as `text_reports`.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use timegrapher_core::synth::{generate, SynthConfig};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tg-text-reports-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 40 s of a steady watch at `rate` s/d and `amplitude` degrees.
fn watch_wav(path: &Path, rate: f64, amplitude: f64, seed: u64) {
    let audio = generate(
        &SynthConfig {
            duration_s: 40.0,
            rate_s_per_day: rate,
            beat_error_ms: 0.4,
            seed,
            ..Default::default()
        },
        move |_| amplitude,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(path, &audio).unwrap();
}

fn session(dir: &Path, toml: &str) -> Value {
    std::fs::write(dir.join("session.toml"), toml).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .args(["session", dir.to_str().unwrap(), "--json"])
        .env("TIMEGRAPHER_CONFIG_DIR", dir.join("config"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn report<'a>(doc: &'a Value, file: &str) -> Option<&'a str> {
    doc["text_reports"]
        .as_array()?
        .iter()
        .find(|r| r["file"] == file)
        .and_then(|r| r["text"].as_str())
}

fn entry(file: &str, position: &str, wind_h: f64) -> String {
    format!(
        "[[recording]]\nfile = \"{file}\"\nposition = \"{position}\"\nsettle_s = 5\nwind_h = {wind_h}\n\n"
    )
}

/// A six-position session gives the Positions Test report alone: its
/// figures are the session's own, in the pane's order, and the file
/// beside report.html holds the same text as the JSON.
#[test]
fn a_positions_session_writes_its_positions_report() {
    let dir = scratch("positions").join("2026-11-13 19.17");
    std::fs::create_dir_all(&dir).unwrap();
    let mut toml =
        String::from("watch = \"Test Watch\"\nbph = 28800\nlift = 52\nlift_known = true\n\n");
    // Listed out of the pane's order, to show the report puts them in it.
    for (i, (pos, rate)) in [
        ("CR", -8.0),
        ("DU", 6.0),
        ("CU", -4.0),
        ("DD", 4.0),
        ("CL", 0.0),
        ("CD", 2.0),
    ]
    .iter()
    .enumerate()
    {
        let f = format!("{pos}.wav");
        watch_wav(&dir.join(&f), *rate, 270.0, i as u64 + 1);
        toml += &entry(&f, pos, 0.0);
    }
    let doc = session(&dir, &toml);
    let text = report(&doc, "positions-report.txt").expect("a positions report");
    assert!(report(&doc, "poise-report.txt").is_none());
    let on_disk = std::fs::read_to_string(dir.join("session_report/positions-report.txt")).unwrap();
    assert_eq!(on_disk, text);

    assert!(text.starts_with("Positions Test\n\n"), "{text}");
    assert!(text.contains("Watch       Test Watch\n"), "{text}");
    assert!(text.contains("Date        2026-11-13 19:17\n"), "{text}");
    assert!(text.contains("Lift Angle  52°\n"), "{text}");
    // The rows in the pane's order, each figure the session's own.
    let order = [
        "Dial Up (DU)",
        "Dial Down (DD)",
        "Crown Down (CD, 9H)",
        "Crown Left (CL, 6H)",
        "Crown Up (CU, 3H)",
        "Crown Right (CR, 12H)",
    ];
    let at: Vec<usize> = order.iter().map(|p| text.find(p).expect(p)).collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{text}");
    let x = doc["report"]["states"][0]["x"].as_f64().unwrap();
    let average = timegrapher_core::figure_text::signed_text(x, 1);
    let line = text.lines().find(|l| l.starts_with("Average")).unwrap();
    assert!(line.contains(&average), "{line} vs {average}");
    for r in doc["readings"].as_array().unwrap() {
        let rate = r["measurement"]["rate_s_per_day"].as_f64().unwrap();
        let code = match r["position"].as_str().unwrap() {
            "CH" => "DU",
            "CB" => "DD",
            c => c,
        };
        let row = text
            .lines()
            .find(|l| l.contains(&format!("{code})")))
            .unwrap_or_else(|| panic!("{code} in {text}"));
        let shown = timegrapher_core::figure_text::signed_text(rate, 1);
        assert!(row.contains(&shown), "{shown} in {row}");
    }
    assert!(text.contains("Vertical less horizontal rate"), "{text}");
    assert!(!text.contains("Against the Full-Wind Test"), "{text}");
    assert!(text
        .lines()
        .all(|l| l.chars().count() <= 72 || l.starts_with('-')));
}

/// A session with the four 45° stops gives the Dynamic Poise report: the
/// eight stops in the pane's order, the Mean row and core's sentence,
/// the same words `poise[0].fit.text` gives, wrapped.
#[test]
fn a_poise_session_writes_its_poise_report() {
    let dir = scratch("poise");
    let mut toml = String::from("bph = 28800\nlift = 52\n\n");
    let stops = ["9H", "7H30", "6H", "4H30", "3H", "1H30", "12H", "10H30"];
    for (i, pos) in stops.iter().enumerate() {
        let angle = 270.0 - 45.0 * i as f64;
        let rate = 5.0 * f64::to_radians(angle).cos();
        let f = format!("{}.wav", pos.replace('H', "h"));
        watch_wav(&dir.join(&f), rate, 200.0, i as u64 + 11);
        toml += &entry(&f, pos, 0.0);
    }
    let doc = session(&dir, &toml);
    let text = report(&doc, "poise-report.txt").expect("a poise report");
    assert!(report(&doc, "positions-report.txt").is_none());
    assert!(text.starts_with("Dynamic Poise Test\n\n"), "{text}");
    assert!(
        !text.contains("Date"),
        "no date for a folder not named by its start: {text}"
    );
    let order = [
        "Crown Down (CD, 9H)",
        "7:30 Up (7H30)",
        "Crown Left (CL, 6H)",
        "4:30 Up (4H30)",
        "Crown Up (CU, 3H)",
        "1:30 Up (1H30)",
        "Crown Right (CR, 12H)",
        "10:30 Up (10H30)",
    ];
    let at: Vec<usize> = order.iter().map(|p| text.find(p).expect(p)).collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{text}");
    assert!(text.lines().any(|l| l.starts_with("Mean")), "{text}");
    let words = doc["poise"][0]["fit"]["text"].as_str().unwrap();
    let flat = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat(text).contains(&flat(words)), "{words}\n{text}");
    assert!(text
        .lines()
        .all(|l| l.chars().count() <= 72 || l.starts_with('-')));
}

/// A session with the six positions at full wind and again 24 h later
/// reports the later readings, set against full wind with Im and Ie.
#[test]
fn a_session_with_two_states_sets_the_later_against_full_wind() {
    let dir = scratch("states");
    let mut toml = String::from("bph = 28800\nlift = 52\n\n");
    for (i, pos) in ["DU", "DD", "CD", "CL", "CU", "CR"].iter().enumerate() {
        let (a, b) = (format!("{pos}-0.wav"), format!("{pos}-24.wav"));
        watch_wav(&dir.join(&a), 2.0 * i as f64, 280.0, i as u64 + 21);
        watch_wav(&dir.join(&b), 2.0 * i as f64 - 6.0, 240.0, i as u64 + 31);
        toml += &entry(&a, pos, 0.0);
        toml += &entry(&b, pos, 24.0);
    }
    let doc = session(&dir, &toml);
    let text = report(&doc, "positions-report.txt").expect("a positions report");
    assert!(
        text.contains("Against the Full-Wind Test, 24.0 h after winding\n"),
        "{text}"
    );
    let at = text.find("Against").unwrap();
    assert!(
        text[at..].lines().all(|l| l.chars().count() <= 72),
        "{text}"
    );
    let x = doc["report"]["states"][1]["x"].as_f64().unwrap();
    let line = text.lines().find(|l| l.starts_with("Average")).unwrap();
    assert!(
        line.contains(&timegrapher_core::figure_text::signed_text(x, 1)),
        "the later state's average: {line}"
    );
    let im = doc["report"]["im"].as_f64().unwrap();
    let ie = doc["report"]["ie"].as_f64().unwrap();
    let im = timegrapher_core::figure_text::signed_text(im, 1);
    let ends = |name: &str, figure: &str| {
        text.lines()
            .any(|l| l.contains(name) && l.ends_with(&format!("  {figure} s/d")))
    };
    assert!(ends("(Im)", &im), "{text}");
    let ie = timegrapher_core::figure_text::fixed_text(ie, 1);
    assert!(ends("(Ie)", &ie), "{text}");
}
