//! Test sessions end to end: synthetic recordings in six positions, read
//! as a session, with Witschi's values and the findings checked.

use std::path::PathBuf;
use timegrapher_core::audio::write_wav;
use timegrapher_core::session::{self, evaluate, Limits, Position, Reading, Severity, Tolerance};
use timegrapher_core::stream::{analyze_file, StreamConfig};
use timegrapher_core::synth::{generate, SynthConfig};

fn reading(position: Position, wind_h: f64, rate: f64, amp: f64, beat_error: f64) -> Reading {
    let cfg = SynthConfig {
        duration_s: 60.0,
        rate_s_per_day: rate,
        beat_error_ms: beat_error,
        snr_db: 26.0,
        ..Default::default()
    };
    let path: PathBuf = std::env::temp_dir().join(format!(
        "tg-session-{}-{wind_h}-{rate}-{amp}-{beat_error}-{}.wav",
        position.code(),
        std::process::id()
    ));
    write_wav(&path, &generate(&cfg, |_| amp, |_| 0.0)).unwrap();
    let log = analyze_file(&path, &StreamConfig::default(), |_| {}).unwrap();
    std::fs::remove_file(&path).ok();
    Reading {
        label: format!("{}.wav", position.code()),
        position,
        wind_h: Some(wind_h),
        date: None,
        notes: None,
        duration_s: log.duration_s,
        bph: log.bph,
        measurement: session::measure(&log, None, 20.0, f64::INFINITY),
        cycles: Vec::new(),
        shape: None,
        reference: None,
    }
}

fn close(a: Option<f64>, b: f64, tol: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < tol)
}

#[test]
fn positions_parse_from_any_common_name() {
    for (s, p) in [
        ("DU", Position::CH),
        ("dial up", Position::CH),
        ("dd", Position::CB),
        ("CL", Position::H6),
        ("crown-left", Position::H6),
        ("PU", Position::H3),
        ("12H", Position::H12),
        ("cd", Position::H9),
    ] {
        assert_eq!(Position::parse(s), Some(p), "{s}");
    }
    assert_eq!(
        Position::from_file_name("ym42_DU_48000-01.flac"),
        Some(Position::CH)
    );
    assert_eq!(
        Position::from_file_name("dandong_CL_20min.flac"),
        Some(Position::H6)
    );
    assert_eq!(Position::from_file_name("rolex_6H.wav"), Some(Position::H6));
    // A duration is not a position.
    assert_eq!(Position::from_file_name("ym42_2h.flac"), None);
    assert_eq!(Position::from_file_name("run_12h.flac"), None);
}

#[test]
fn six_positions_give_witschi_values_and_findings() {
    // Horizontal positions fast, vertical slower, 6H slowest and low.
    let set = [
        (Position::CH, 6.0, 285.0),
        (Position::CB, 8.0, 280.0),
        (Position::H9, -2.0, 250.0),
        (Position::H6, -8.0, 210.0),
        (Position::H3, 1.0, 255.0),
        (Position::H12, 3.0, 252.0),
    ];
    let readings: Vec<Reading> = set
        .iter()
        .map(|&(p, r, a)| reading(p, 0.0, r, a, 0.2))
        .collect();
    for (r, &(p, rate, amp)) in readings.iter().zip(&set) {
        let m = &r.measurement;
        assert!(
            close(m.rate_s_per_day, rate, 0.5),
            "{p}: rate {:?}",
            m.rate_s_per_day
        );
        assert!(
            close(m.amplitude_deg, amp, 8.0),
            "{p}: amplitude {:?}",
            m.amplitude_deg
        );
        assert!(
            close(m.beat_error_ms.map(f64::abs), 0.2, 0.05),
            "{p}: {:?}",
            m.beat_error_ms
        );
        // Unlock and drop beat errors agree when the sides' impulses match.
        assert!(
            close(m.beat_error(), 0.2, 0.05),
            "{p}: {:?}",
            m.beat_error_unlock_ms
        );
        assert!(
            (m.end_s - m.start_s - 40.0).abs() < 0.5,
            "settling time not skipped"
        );
    }
    let rep = evaluate(&readings, &Tolerance::default(), &Limits::default());
    assert_eq!(rep.states.len(), 1);
    let st = &rep.states[0];
    assert!(close(st.x, 8.0 / 6.0, 0.4), "X {:?}", st.x);
    assert!(close(st.xh, 7.0, 0.4), "XH {:?}", st.xh);
    assert!(close(st.xv, -1.5, 0.4), "XV {:?}", st.xv);
    assert!(close(st.d_rate, 16.0, 0.6), "D {:?}", st.d_rate);
    assert!(close(st.dv_rate, 11.0, 0.6), "DV {:?}", st.dv_rate);
    assert!(close(st.dvh_rate, -8.5, 0.6), "DVH {:?}", st.dvh_rate);
    assert!(close(st.di, -14.0, 0.6), "Di {:?}", st.di);

    let titles: Vec<&str> = rep.findings.iter().map(|f| f.title.as_str()).collect();
    assert!(
        titles.contains(&"Large differences between positions"),
        "{titles:?}"
    );
    assert!(
        titles.contains(&"Amplitude outside tolerance"),
        "{titles:?}"
    );
    assert!(
        titles.contains(&"Vertical and horizontal rates differ"),
        "{titles:?}"
    );
    assert!(
        !titles.contains(&"Mean rate outside tolerance"),
        "{titles:?}"
    );
    assert!(
        !titles.iter().any(|t| t.contains("Beat error")),
        "{titles:?}"
    );
    let codes: Vec<&str> = rep.findings.iter().map(|f| f.code).collect();
    assert!(codes.contains(&"positional_delta"), "{codes:?}");
    let low = rep
        .findings
        .iter()
        .find(|f| f.code == "amplitude_tolerance")
        .unwrap();
    assert_eq!(readings[low.recording.unwrap()].position, Position::H6);
    // Faults first.
    assert!(rep
        .findings
        .windows(2)
        .all(|w| w[0].severity <= w[1].severity));
    // Only the low 6H amplitude (210°, under the vertical 240°) is outside.
    let out: Vec<Position> = readings
        .iter()
        .zip(&rep.verdicts)
        .filter(|(_, v)| v.amplitude == session::Mark::Outside)
        .map(|(r, _)| r.position)
        .collect();
    assert_eq!(out, vec![Position::H6]);
}

#[test]
fn isochronism_from_two_states_of_wind() {
    let readings = vec![
        reading(Position::CH, 0.0, 4.0, 290.0, 0.1),
        reading(Position::H6, 0.0, 1.0, 260.0, 0.1),
        reading(Position::CH, 24.0, 7.0, 250.0, 0.1),
        reading(Position::H6, 24.0, -3.0, 215.0, 0.1),
    ];
    let rep = evaluate(&readings, &Tolerance::default(), &Limits::default());
    assert_eq!(rep.states.len(), 2);
    assert_eq!(rep.isochronism.len(), 2);
    let ch = rep
        .isochronism
        .iter()
        .find(|i| i.position == Position::CH)
        .unwrap();
    assert!((ch.rate_change - 3.0).abs() < 0.6, "{}", ch.rate_change);
    assert!(close(rep.im, -4.0, 0.6), "Im {:?}", rep.im);
    assert!(close(rep.ie, 0.5, 0.6), "Ie {:?}", rep.ie);
    // The tolerance applies fully wound only: the 215° at 24 h is not judged.
    assert_eq!(rep.verdicts[3].amplitude, session::Mark::NotJudged);
}

#[test]
fn large_beat_error_is_a_fault() {
    // Also checks the beat rate is still guessed right with 2.5 ms of beat error.
    let readings = vec![reading(Position::CH, 0.0, 2.0, 280.0, 2.5)];
    let rep = evaluate(&readings, &Tolerance::default(), &Limits::default());
    let f = &rep.findings[0];
    assert_eq!(f.severity, Severity::Fault);
    assert_eq!(f.title, "Large beat error");
    assert!(f.evidence.contains("2.5"), "{}", f.evidence);
}
