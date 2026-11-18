//! `long`'s text and report give its rates as signed figures, the slice
//! range included (#734).
use std::process::Command;

#[test]
fn the_slice_range_reads_as_signed_rates() {
    let dir = std::env::temp_dir().join(format!("tg-long-text-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
            .env("TIMEGRAPHER_CONFIG_DIR", &dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    // The synthetic watch runs +5 s/d, so every slice reads fast.
    let wav = dir.join("watch.wav");
    let wav = wav.to_str().unwrap();
    run(&["synth", wav, "--duration", "40"]);
    let out = dir.join("long");
    let text = run(&["long", wav, "--out", out.to_str().unwrap()]);
    let line = text
        .lines()
        .find(|l| l.starts_with("Rate "))
        .unwrap_or_else(|| panic!("no rate line in {text}"));
    assert!(line.contains("s/d over the run; "), "{line}");
    assert!(line.contains(" slices from +"), "{line}");
    assert!(line.contains(" to +"), "{line}");
    let html = std::fs::read_to_string(out.join("report.html")).unwrap();
    let row = html
        .lines()
        .find(|l| l.starts_with("<tr><th>Rate</th>"))
        .expect("the report's rate row");
    assert!(row.contains(" slices from +"), "{row}");
    assert!(row.contains(" to +"), "{row}");
    // Each search says the shortest cycle it tried, in the text and the
    // report, and the dashed line is a rate over searches (#1897). On 40 s
    // the rate is searched and the amplitude is not, which is said, never
    // "none named" (#1880).
    use timegrapher_core::longterm::TOO_SHORT_TO_SEARCH;
    for (out, shortest) in [
        (&text, "  Shortest cycle searched: "),
        (&html, "Shortest cycle searched: "),
    ] {
        assert_eq!(out.matches(shortest).count(), 1, "{out}");
        assert_eq!(out.matches(TOO_SHORT_TO_SEARCH).count(), 1, "{out}");
    }
    assert_eq!(text.matches("none named").count(), 1, "{text}");
    assert_eq!(html.matches("No periodic change").count(), 1);
    assert!(
        html.contains(&timegrapher_core::steady_views::threshold_meaning(
            timegrapher_core::longterm::LongConfig::default().false_alarm
        ))
    );
    assert!(
        !html.contains("too short to search."),
        "one fact, one place"
    );
    assert!(!html.contains("chance of being noise"));
    let _ = std::fs::remove_dir_all(&dir);
}
