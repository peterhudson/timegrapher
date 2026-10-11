//! Test sessions end to end: synthetic recordings in six positions, read
//! as a session, with Witschi's values and the findings checked. The
//! findings state conditions found in the sound; none judges the watch.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use timegrapher_core::audio::write_wav;
use timegrapher_core::session::{self, evaluate, Position, Reading};
use timegrapher_core::stream::{analyze_file, StreamConfig};
use timegrapher_core::synth::{generate, SynthConfig};

/// A temporary WAV path of its own for every call: the tests run in
/// parallel, and two readings with the same values must not share a file.
fn temp_wav(tag: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "tg-session-{tag}-{}-{}.wav",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn reading(position: Position, wind_h: f64, rate: f64, amp: f64, beat_error: f64) -> Reading {
    let cfg = SynthConfig {
        duration_s: 60.0,
        rate_s_per_day: rate,
        beat_error_ms: beat_error,
        snr_db: 26.0,
        ..Default::default()
    };
    let path = temp_wav(position.code());
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

/// Every code a session finding can carry: conditions found in the sound.
const FACT_CODES: [&str; 9] = [
    "no_regular_beat",
    "unlock_unreliable",
    "clipping",
    "measurement_short",
    "cycle_wheel",
    "cycle_wheel_fraction",
    "cycle_other",
    "shape_unlock_loud",
    "shape_extra_sounds",
];

fn only_facts(rep: &session::SessionReport) {
    for f in &rep.findings {
        assert!(FACT_CODES.contains(&f.code), "{f:?}");
    }
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
    let rep = evaluate(&readings);
    assert_eq!(rep.states.len(), 1);
    let st = &rep.states[0];
    assert!(close(st.x, 8.0 / 6.0, 0.4), "X {:?}", st.x);
    assert!(close(st.xh, 7.0, 0.4), "XH {:?}", st.xh);
    assert!(close(st.xv, -1.5, 0.4), "XV {:?}", st.xv);
    assert!(close(st.d_rate, 16.0, 0.6), "D {:?}", st.d_rate);
    assert!(close(st.dv_rate, 11.0, 0.6), "DV {:?}", st.dv_rate);
    assert!(close(st.dvh_rate, -8.5, 0.6), "DVH {:?}", st.dvh_rate);
    assert!(close(st.di, -14.0, 0.6), "Di {:?}", st.di);

    // A 210° amplitude, a 16 s/d spread and a DVH of −8.5 s/d are figures
    // in the table, not findings.
    only_facts(&rep);
}

#[test]
fn isochronism_from_two_states_of_wind() {
    let readings = vec![
        reading(Position::CH, 0.0, 4.0, 290.0, 0.1),
        reading(Position::H6, 0.0, 1.0, 260.0, 0.1),
        reading(Position::CH, 24.0, 7.0, 250.0, 0.4),
        reading(Position::H6, 24.0, -3.0, 215.0, 0.1),
    ];
    let rep = evaluate(&readings);
    assert_eq!(rep.states.len(), 2);
    assert_eq!(rep.isochronism.len(), 2);
    let ch = rep
        .isochronism
        .iter()
        .find(|i| i.position == Position::CH)
        .unwrap();
    assert!((ch.rate_change - 3.0).abs() < 0.6, "{}", ch.rate_change);
    assert!(
        close(ch.amplitude_change, -40.0, 8.0),
        "{:?}",
        ch.amplitude_change
    );
    assert!(
        close(ch.beat_error_change, 0.3, 0.1),
        "{:?}",
        ch.beat_error_change
    );
    assert!(close(rep.im, -4.0, 0.6), "Im {:?}", rep.im);
    assert!(close(rep.ie, 0.5, 0.6), "Ie {:?}", rep.ie);
}

#[test]
fn large_beat_error_is_measured_not_judged() {
    // Also checks the beat rate is still guessed right with 2.5 ms of beat error.
    let readings = vec![reading(Position::CH, 0.0, 2.0, 280.0, 2.5)];
    assert!(close(readings[0].measurement.beat_error(), 2.5, 0.1));
    let rep = evaluate(&readings);
    only_facts(&rep);
}

#[test]
fn unlock_missing_in_most_windows_is_stated_with_its_share() {
    let mut readings = vec![reading(Position::CH, 0.0, 2.0, 200.0, 0.1)];
    assert!(readings[0].measurement.unlock_coverage.unwrap() > 0.9);
    // As on a smeared tick, where most windows lose one side's unlock.
    readings[0].measurement.unlock_coverage = Some(0.2);
    let rep = evaluate(&readings);
    only_facts(&rep);
    let f = rep
        .findings
        .iter()
        .find(|f| f.code == "unlock_unreliable")
        .expect("coverage stated");
    assert_eq!(f.title, "Unlock found in 20% of windows");
    assert!(f.evidence.starts_with("20% of the "), "{}", f.evidence);
    // The amplitude is still shown.
    assert!(readings[0].measurement.amplitude_deg.is_some());
}

#[test]
fn clipped_recording_is_flagged() {
    let clean = reading(Position::CH, 0.0, 2.0, 280.0, 0.1);
    assert_eq!(clean.measurement.clipped_beats, 0);
    let rep = evaluate(std::slice::from_ref(&clean));
    assert!(!rep.findings.iter().any(|f| f.code == "clipping"));

    // The same watch with the input level far too high: every tick's peak
    // is flattened at full scale.
    let cfg = SynthConfig {
        duration_s: 60.0,
        rate_s_per_day: 2.0,
        beat_error_ms: 0.1,
        snr_db: 26.0,
        ..Default::default()
    };
    let mut audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let peak = audio.samples.iter().fold(0f32, |m, v| m.max(v.abs()));
    for v in &mut audio.samples {
        *v = (*v * 3.0 / peak).clamp(-1.0, 1.0);
    }
    let path = temp_wav("clipped");
    write_wav(&path, &audio).unwrap();
    let log = analyze_file(&path, &StreamConfig::default(), |_| {}).unwrap();
    std::fs::remove_file(&path).ok();
    let m = session::measure(&log, None, 20.0, f64::INFINITY);
    assert!(m.clipped_fraction > 0.5, "{}", m.clipped_fraction);
    // Counted as `analyze` counts it on the whole recording.
    let whole = timegrapher_core::analysis::Clipping::measure(
        &audio.samples,
        audio.sample_rate as f64,
        &log.beats,
    );
    assert!((whole.beats as i64 - log.clipped_beats.len() as i64).abs() <= 2);
    let r = Reading {
        measurement: m,
        ..clean
    };
    let rep = evaluate(&[r]);
    let f = rep.findings.iter().find(|f| f.code == "clipping").unwrap();
    assert!(f.title.starts_with("Clipped on "), "{}", f.title);
    assert!(
        f.evidence.contains(" beats in Dial Up (CH)"),
        "{}",
        f.evidence
    );
}

#[test]
fn short_rate_cycle_is_sized_in_ms() {
    // A 10 s rate swing, too short for 10 s rate readings to follow: the
    // cycle list states it as a timing swing in ms, as `long` does.
    let cfg = SynthConfig {
        duration_s: 240.0,
        snr_db: 30.0,
        ..Default::default()
    };
    let audio = generate(
        &cfg,
        |_| 280.0,
        |t| 60.0 * (2.0 * std::f64::consts::PI * t / 10.0).sin(),
    );
    let path = temp_wav("cycle");
    write_wav(&path, &audio).unwrap();
    let log = analyze_file(&path, &StreamConfig::default(), |_| {}).unwrap();
    std::fs::remove_file(&path).ok();
    let lc = timegrapher_core::longterm::LongConfig::default();
    let report = timegrapher_core::longrun::analyse(&log, None, &lc);
    let cycles = session::cycles(&report, &lc.wheels);
    let c = cycles
        .iter()
        .find(|c| c.series == "rate" && (c.period_s - 10.0).abs() < 0.2)
        .expect("10 s rate cycle");
    assert_eq!(c.unit(), "ms");
    assert!(c.size_text().ends_with(" ms"), "{}", c.size_text());
    assert!(c.size > 0.0 && c.size < 5.0, "{}", c.size);
}

#[test]
fn an_empty_stand_gives_no_readings() {
    // Hiss with no ticks in it: the session shows no rate, amplitude or
    // beat error, and states the timing scatter it found instead.
    let fs = 48000u32;
    let mut state = 12345u64;
    let mut uniform = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 11) as f64 / (1u64 << 53) as f64) as f32 - 0.5
    };
    let samples: Vec<f32> = (0..fs as usize * 60).map(|_| 0.01 * uniform()).collect();
    let audio = timegrapher_core::audio::Audio {
        samples,
        sample_rate: fs,
    };
    let path = temp_wav("empty");
    write_wav(&path, &audio).unwrap();
    let cfg = StreamConfig {
        analysis: timegrapher_core::analysis::AnalysisConfig {
            bph: Some(21600),
            ..Default::default()
        },
        ..Default::default()
    };
    let log = analyze_file(&path, &cfg, |_| {}).unwrap();
    std::fs::remove_file(&path).ok();
    let m = session::measure(&log, None, 20.0, f64::INFINITY);
    assert!(
        !m.watch_heard,
        "noise taken for a watch: jitter {:?} us",
        m.jitter_us
    );
    assert!(m.rate_s_per_day.is_none() && m.amplitude_deg.is_none() && m.beat_error().is_none());
    let r = Reading {
        measurement: m,
        ..reading(Position::CH, 0.0, 5.0, 280.0, 0.2)
    };
    let rep = evaluate(&[r]);
    assert_eq!(rep.findings.len(), 1, "{:?}", rep.findings);
    let f = &rep.findings[0];
    assert_eq!(f.code, "no_regular_beat");
    assert!(
        f.title
            .starts_with("No regular beat found (timing scatter ")
            && f.title.ends_with("% of a beat)"),
        "{}",
        f.title
    );
    assert!(f.evidence.starts_with("in Dial Up (CH)"), "{}", f.evidence);

    // A watch is still heard.
    assert!(
        reading(Position::CH, 0.0, 5.0, 280.0, 0.2)
            .measurement
            .watch_heard
    );
}

#[test]
fn amplitude_over_330_is_shown_not_judged() {
    let mut r = reading(Position::CH, 0.0, 5.0, 280.0, 0.2);
    r.measurement.amplitude_deg = Some(335.0);
    let rep = evaluate(&[r]);
    only_facts(&rep);
    assert!(!rep.findings.iter().any(|f| f.evidence.contains("335")));
}
