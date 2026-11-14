use super::*;

/// A sentence wraps at [`WIDTH`] columns at a space, and a figure with
/// its ± is never split across lines (Hugh, 2026-11-14).
#[test]
fn a_sentence_wraps_at_72_columns_and_keeps_each_figure_whole() {
    let text = "Once-round wave over 8 upright positions: 6.4 ± 0.6 s/d each way from \
                the mean, fastest at 12 ± 4° (nearest 12H30 at the top). Twice-round \
                terms: cos 2α -1.2 ± 0.6 s/d, sin 2α -1.9 ± 0.6 s/d.";
    let out = wrap(text, "");
    for line in out.lines() {
        assert!(line.chars().count() <= WIDTH, "{line:?}");
        assert!(!line.ends_with('±') && !line.starts_with('±'), "{line:?}");
    }
    // Nothing is lost or reordered.
    let joined = out.split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(
        joined,
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    );
    assert!(out.lines().count() >= 3);
}

/// Where a "±" would fall at the end of a line, the figure goes whole to
/// the next one.
#[test]
fn a_figure_at_the_break_moves_whole_to_the_next_line() {
    // 66 columns of words, then a figure that would straddle column 72.
    let text = format!("{} 6.4 ± 0.6 s/d", "word ".repeat(13).trim_end());
    let out = wrap(&text, "");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert!(lines[1].starts_with("6.4 ± 0.6"), "{out}");
}

/// A bullet's later lines are indented under its first word.
#[test]
fn a_bullet_wraps_under_its_first_word() {
    let out = wrap(&"long ".repeat(30), "- ");
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with("- long"));
    assert!(lines[1..].iter().all(|l| l.starts_with("  long")), "{out}");
}

/// The Date line comes from the take folder's name, the start in its
/// recording computer's time; a folder not so named gives none.
#[test]
fn the_date_is_the_take_folders_start() {
    assert_eq!(
        take_date("2026-11-13 19.17 Dynamic Poise").as_deref(),
        Some("2026-11-13 19:17")
    );
    assert_eq!(take_date("ym42 copied"), None);
}

/// A missing fact's line is left out of the heading, never filled in.
#[test]
fn a_heading_leaves_out_what_is_not_known() {
    let h = Header {
        watch: Some("Test Watch".into()),
        bph: Some(28800),
        ..Default::default()
    };
    let text = heading("Positions Test", &h);
    assert!(text.starts_with("Positions Test\n\n"));
    assert!(text.contains("Watch      Test Watch\n"), "{text}");
    assert!(text.contains("Beat Rate  28,800 bph\n"), "{text}");
    assert!(
        text.contains(&format!("Software   {}\n", software())),
        "{text}"
    );
    for absent in ["Date", "Calibre", "Lift Angle", "Settling", "Measured"] {
        assert!(!text.contains(absent), "{absent} in {text}");
    }
}

/// The software line names the build and core's source, as the Watches
/// list's hover does (#1588).
#[test]
fn the_software_line_names_the_engine() {
    let (version, source) = crate::library::engine();
    assert_eq!(
        software(),
        format!("Timegrapher {version}, engine {source}")
    );
}

/// A table puts the names left and the figures right, with a rule under
/// its two header rows and above its summary rows.
#[test]
fn a_table_lines_up_its_columns() {
    let rows: Vec<Vec<String>> = [
        ["Position", "Rate"],
        ["", "s/d"],
        ["Dial Up (DU)", "+5.0"],
        ["Crown Up (CU, 3H)", "-12.5"],
        ["Average", "-3.8"],
    ]
    .iter()
    .map(|r| r.iter().map(|s| s.to_string()).collect())
    .collect();
    assert_eq!(
        table(&rows, 2, 1),
        "Position            Rate\n\
         \x20                    s/d\n\
         ------------------------\n\
         Dial Up (DU)        +5.0\n\
         Crown Up (CU, 3H)  -12.5\n\
         ------------------------\n\
         Average             -3.8\n"
    );
}

#[test]
fn a_beat_rate_is_grouped_by_thousands() {
    assert_eq!(grouped(28800), "28,800");
    assert_eq!(grouped(18000), "18,000");
    assert_eq!(grouped(360000), "360,000");
    assert_eq!(grouped(900), "900");
}

/// Moved from the app's guided tests with `caveat_condition`.
#[test]
fn a_position_says_how_its_beat_error_was_given() {
    assert_eq!(
        caveat_condition("Dial Up", &Caveat::BeatErrorAsSize).as_deref(),
        Some("Dial Up: beat error given as a size, its sign differs either side of a cut")
    );
    assert_eq!(
        caveat_condition("Dial Up", &Caveat::SidesByCount { count: 1 }).as_deref(),
        Some(
            "Dial Up: Tick and Tock named by the beat count across 1 break, not told \
             apart by their sound"
        )
    );
    assert_eq!(caveat_condition("Dial Up", &Caveat::LiftUnknown), None);
}
