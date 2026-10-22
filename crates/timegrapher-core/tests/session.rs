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

#[test]
fn positions_are_named_as_the_app_names_them() {
    assert_eq!(Position::H3.display_label(), "Crown Up (CU, 3H)");
    assert_eq!(Position::H12.display_label(), "Crown Right (CR, 12H)");
    assert_eq!(Position::H9.display_label(), "Crown Down (CD, 9H)");
    assert_eq!(Position::H6.display_label(), "Crown Left (CL, 6H)");
    assert_eq!(Position::CH.display_label(), "Dial Up (DU)");
    assert_eq!(Position::CB.display_label(), "Dial Down (DD)");
    // The JSON keeps Witschi's code alone.
    assert_eq!(Position::H3.label(), "Crown Up (3H)");
    assert_eq!(Position::CB.label(), "Dial Down (CB)");
}

#[test]
fn a_masked_stretch_is_left_out_of_the_readings() {
    // A bump of the stand: the beats run 400 s/d fast for ten seconds.
    let cfg = SynthConfig {
        duration_s: 60.0,
        rate_s_per_day: 5.0,
        snr_db: 26.0,
        ..Default::default()
    };
    let path = temp_wav("masked");
    let bump = |t: f64| {
        if (30.0..40.0).contains(&t) {
            400.0
        } else {
            0.0
        }
    };
    write_wav(&path, &generate(&cfg, |_| 280.0, bump)).unwrap();
    let mut log = analyze_file(&path, &StreamConfig::default(), |_| {}).unwrap();
    std::fs::remove_file(&path).ok();
    let whole = session::measure(&log, None, 20.0, f64::INFINITY);
    let masked = session::masked_spans(&[[29.0, 41.0], [40.0, 41.5], [9.0, 8.0]]);
    assert_eq!(masked, [(29.0, 41.5)], "cleaned and joined");
    session::mask_log(&mut log, &masked);
    let m = session::measure_masked(&log, None, &[], &masked, 20.0, f64::INFINITY);
    assert!(
        close(m.rate_s_per_day, 5.0, 3.0),
        "masked {:?}, whole {:?}",
        m.rate_s_per_day,
        whole.rate_s_per_day
    );
    assert!(
        !close(whole.rate_s_per_day, 5.0, 3.0),
        "{:?}",
        whole.rate_s_per_day
    );
    assert!((m.masked_s - 12.5).abs() < 1e-9);
    assert!(m
        .caveats
        .contains(&timegrapher_core::measure::Caveat::Masked { seconds: 12.5 }));
    assert!(!m
        .caveats
        .iter()
        .any(|c| matches!(c, timegrapher_core::measure::Caveat::BeatsMissing { .. })));
    let json = serde_json::to_value(&m).unwrap();
    assert_eq!(json["masked_s"], 12.5);
    assert_eq!(
        serde_json::to_value(&whole).unwrap().get("masked_s"),
        None,
        "left out when nothing is masked"
    );
}

#[test]
fn no_rate_window_takes_in_the_ragged_edge_after_a_mask() {
    // A watch at 5 s/d, 28,800 bph. For two seconds after the mask the
    // beats run 300 s/d fast: only a window that starts inside the mask
    // and reaches past its end, fitted on those two seconds alone, sees
    // that rate without the steady beats around it.
    let bph = 28_800;
    let period = 3600.0 / bph as f64 / (1.0 + 5.0 / 86400.0);
    let mask = (100.0, 203.0);
    let beats: Vec<timegrapher_core::beats::Beat> = (0..(240.0 / period) as i64)
        .map(|k| {
            let t = k as f64 * period;
            let fast = (t - 203.0).clamp(0.0, 2.0) * 300.0 / 86400.0;
            timegrapher_core::beats::Beat {
                index: k,
                time: t - if t < 205.0 { fast } else { 0.0 },
                quality: 1.0,
            }
        })
        .filter(|b| !(mask.0..mask.1).contains(&b.time))
        .collect();
    let log = timegrapher_core::stream::BeatLog {
        sample_rate: 44_100,
        duration_s: 240.0,
        bph,
        lift_deg: 52.0,
        beats,
        amplitude_windows: Vec::new(),
        clipped_beats: Vec::new(),
        clipped_samples: 0,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
    };
    let m = session::measure_masked(&log, None, &[], &[mask], 80.0, 232.0);
    // The 10 s windows that remain lie wholly on one side of the mask.
    let (p05, p95) = (
        m.rate_p05.expect("rate windows"),
        m.rate_p95.expect("rate windows"),
    );
    assert!((p05 - 5.0).abs() < 1.0, "10 s rates reach {p05} s/d");
    assert!((p95 - 5.0).abs() < 1.0, "10 s rates reach {p95} s/d");
}

#[test]
fn no_rate_window_takes_in_the_ragged_edge_after_a_break() {
    // The same watch, with the audio stopped from 100 s to 203 s of true
    // time: a break of 103 s at audio second 100. For two seconds after it
    // the beats run 300 s/d fast, seen only by a window that starts inside
    // the gap and reaches past its end.
    let bph = 28_800;
    let period = 3600.0 / bph as f64 / (1.0 + 5.0 / 86400.0);
    let brk = timegrapher_core::breaks::Break {
        audio_s: 100.0,
        gap_s: 103.0,
    };
    let beats: Vec<timegrapher_core::beats::Beat> = (0..(240.0 / period) as i64)
        .filter_map(|k| {
            let t = k as f64 * period;
            let fast = (t - 203.0).clamp(0.0, 2.0) * 300.0 / 86400.0;
            let t = t - if t < 205.0 { fast } else { 0.0 };
            let audio = if t < 100.0 {
                t
            } else if t >= 203.0 {
                t - brk.gap_s
            } else {
                return None;
            };
            Some(timegrapher_core::beats::Beat {
                index: k,
                time: audio,
                quality: 1.0,
            })
        })
        .collect();
    let log = timegrapher_core::stream::BeatLog {
        sample_rate: 44_100,
        duration_s: 240.0 - brk.gap_s,
        bph,
        lift_deg: 52.0,
        beats,
        amplitude_windows: Vec::new(),
        clipped_beats: Vec::new(),
        clipped_samples: 0,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
    };
    let m = session::measure_masked(&log, None, &[brk], &[], 80.0, 129.0);
    let (p05, p95) = (
        m.rate_p05.expect("rate windows"),
        m.rate_p95.expect("rate windows"),
    );
    assert!((p05 - 5.0).abs() < 1.0, "10 s rates reach {p05} s/d");
    assert!((p95 - 5.0).abs() < 1.0, "10 s rates reach {p95} s/d");
}

#[test]
fn no_rate_window_takes_in_the_ragged_edge_after_a_pause_in_a_clock_log() {
    // The pause of the break test, as an app take saves it: the break is
    // listed at the frame where the sound resumes, and the clock log, a
    // line every 10 s and one per block around the pause, writes its
    // first line after the pause one 10 ms block later, with the frames
    // counted to the end of that block. The clock fit takes the two lines
    // either side of the step as the recorder's pair at a pause and puts
    // its own break at the line at the pause.
    let bph = 28_800;
    let period = 3600.0 / bph as f64 / (1.0 + 5.0 / 86400.0);
    let (pause_s, gap_s, block_s) = (100.0, 103.0, 0.01);
    let brk = timegrapher_core::breaks::Break {
        audio_s: pause_s,
        gap_s,
    };
    let mut lines: Vec<(f64, f64)> = (1..=10)
        .map(|i| (i as f64 * 10.0, i as f64 * 10.0))
        .collect();
    lines.extend((0..4).map(|i| {
        let a = pause_s + block_s + i as f64 * 10.0;
        (a, a + gap_s)
    }));
    let mut clock = timegrapher_core::clock::ClockFit::new(&lines).unwrap();
    assert_eq!(clock.breaks.len(), 1);
    assert!((clock.breaks[0].audio_s - pause_s).abs() < 1e-9);
    // As `session` reads it: the clock fit's break matched to the listed one.
    clock.place_breaks(&[brk]);
    assert!((clock.breaks[0].audio_s - pause_s).abs() < 1e-9);
    // Then the same with a second pause of 0.3 s at 207 s of true time,
    // 4 s of sound after the first: too short for the clock fit to find,
    // so only its listed break marks it.
    for short in [None, Some(0.3)] {
        let short_at = pause_s + gap_s + 4.0;
        let beats: Vec<timegrapher_core::beats::Beat> = (0..(240.0 / period) as i64)
            .filter_map(|k| {
                let t = k as f64 * period;
                let fast = (t - 203.0).clamp(0.0, 2.0) * 300.0 / 86400.0;
                let t = t - if t < 205.0 { fast } else { 0.0 };
                let audio = if t < pause_s {
                    t
                } else if t < pause_s + gap_s {
                    return None;
                } else {
                    match short {
                        Some(g) if t >= short_at + g => t - gap_s - g,
                        Some(_) if t >= short_at => return None,
                        _ => t - gap_s,
                    }
                };
                Some(timegrapher_core::beats::Beat {
                    index: k,
                    time: audio,
                    quality: 1.0,
                })
            })
            .collect();
        let mut breaks = vec![brk];
        breaks.extend(short.map(|g| timegrapher_core::breaks::Break {
            audio_s: short_at - gap_s,
            gap_s: g,
        }));
        let log = timegrapher_core::stream::BeatLog {
            sample_rate: 44_100,
            duration_s: 240.0 - gap_s - short.unwrap_or(0.0),
            bph,
            lift_deg: 52.0,
            beats,
            amplitude_windows: Vec::new(),
            clipped_beats: Vec::new(),
            clipped_samples: 0,
            jumps: Vec::new(),
            sided: Vec::new(),
            by_count: Vec::new(),
        };
        let m = session::measure_masked(&log, Some(&clock), &breaks, &[], 80.0, 129.0);
        let (p05, p95) = (
            m.rate_p05.expect("rate windows"),
            m.rate_p95.expect("rate windows"),
        );
        let rate = m.rate_s_per_day.expect("rate");
        assert!(
            (p05 - 5.0).abs() < 1.0,
            "{short:?}: 10 s rates reach {p05} s/d"
        );
        assert!(
            (p95 - 5.0).abs() < 1.0,
            "{short:?}: 10 s rates reach {p95} s/d"
        );
        assert!((rate - 5.0).abs() < 1.0, "{short:?}: rate {rate} s/d");
    }
}

#[test]
fn a_break_with_no_gap_keeps_its_single_cut() {
    // A restart with no time lost, as a jump the analysis found: from a
    // beat on, the sound runs 3 ms late and the watch at 10 s/d, not 5.
    // Only that one cut splits the fit, and every beat is in it.
    let bph = 28_800;
    let period = |r: f64| 3600.0 / bph as f64 / (1.0 + r / 86400.0);
    let at = 500;
    let beats: Vec<timegrapher_core::beats::Beat> = (0..960)
        .map(|k| timegrapher_core::beats::Beat {
            index: k,
            time: if k < at {
                k as f64 * period(5.0)
            } else {
                at as f64 * period(5.0) + 0.003 + (k - at) as f64 * period(10.0)
            },
            quality: 1.0,
        })
        .collect();
    let jump = beats[at as usize].time;
    let log = timegrapher_core::stream::BeatLog {
        sample_rate: 44_100,
        duration_s: 120.0,
        bph,
        lift_deg: 52.0,
        beats: beats.clone(),
        amplitude_windows: Vec::new(),
        clipped_beats: Vec::new(),
        clipped_samples: 0,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
    };
    let inside: Vec<_> = beats
        .iter()
        .filter(|b| b.time >= 20.0 && b.time < 110.0)
        .copied()
        .collect();
    let want = timegrapher_core::timing::fit_pieces(&inside, bph, &[jump - 1e-6]).unwrap();
    // A gap under 0 s, as the recorder can write, is taken as none.
    for gap_s in [0.0, -0.2] {
        let brk = timegrapher_core::breaks::Break {
            audio_s: jump,
            gap_s,
        };
        let m = session::measure_masked(&log, None, &[brk], &[], 20.0, 110.0);
        let rate = m.rate_s_per_day.expect("rate");
        assert!(
            (rate - want.rate_s_per_day).abs() < 1e-9,
            "gap {gap_s} s: rate {rate} s/d, one cut gives {} s/d",
            want.rate_s_per_day
        );
    }
}

#[test]
fn a_pause_at_either_end_of_a_reading_is_no_missing_beat() {
    // Every beat heard, 60 s either side of a pause of 1,000 s at audio
    // second 60, as when a position change is written at a pause. A
    // reading that ends or starts at the pause, or spans it, expects only
    // the beats of the time it listened.
    let bph = 28_800;
    let period = 3600.0 / bph as f64;
    let gap_s = 1000.0;
    let brk = timegrapher_core::breaks::Break {
        audio_s: 60.0,
        gap_s,
    };
    let beats: Vec<timegrapher_core::beats::Beat> = (0..960)
        .map(|k| timegrapher_core::beats::Beat {
            index: k + if k >= 480 { (gap_s / period) as i64 } else { 0 },
            time: k as f64 * period,
            quality: 1.0,
        })
        .collect();
    let log = timegrapher_core::stream::BeatLog {
        sample_rate: 44_100,
        duration_s: 120.0,
        bph,
        lift_deg: 52.0,
        beats,
        amplitude_windows: Vec::new(),
        clipped_beats: Vec::new(),
        clipped_samples: 0,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
    };
    // No clock, a steady correction, and a clock log that holds the pause
    // with its break moved to the listed one, as `session` reads it.
    let lines: Vec<(f64, f64)> = (0..=12)
        .map(|i| {
            let a = i as f64 * 10.0;
            (a, a + if a >= 60.0 { gap_s } else { 0.0 })
        })
        .collect();
    let mut logged = timegrapher_core::clock::ClockFit::new(&lines).unwrap();
    logged.place_breaks(&[brk]);
    let clocks = [None, session::fixed_clock(0.0, 120.0), Some(logged)];
    for (c, clock) in clocks.iter().enumerate() {
        for (from, to) in [(0.0, 60.0), (60.0, 120.0), (0.0, 120.0)] {
            let m = session::measure_masked(&log, clock.as_ref(), &[brk], &[], from, to);
            assert!(
                m.clean_fraction > 0.99,
                "clock {c}, {from}-{to} s: {} of the beats expected",
                m.clean_fraction
            );
            assert!(
                !m.caveats.iter().any(|k| matches!(
                    k,
                    timegrapher_core::measure::Caveat::BeatsMissing { .. }
                        | timegrapher_core::measure::Caveat::ShortSpan { .. }
                )),
                "clock {c}, {from}-{to} s: {:?}",
                m.caveats
            );
        }
    }
}

#[test]
fn a_reversed_stretch_reads_as_empty_rather_than_panicking() {
    let log = timegrapher_core::stream::BeatLog {
        sample_rate: 44_100,
        duration_s: 60.0,
        bph: 28_800,
        lift_deg: 52.0,
        beats: (0..480)
            .map(|k| timegrapher_core::beats::Beat {
                index: k,
                time: k as f64 * 0.125,
                quality: 1.0,
            })
            .collect(),
        amplitude_windows: Vec::new(),
        clipped_beats: Vec::new(),
        clipped_samples: 0,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
    };
    for (from, to) in [(0.0, -20.0), (-20.0, -19.0), (30.0, 10.0)] {
        let m = session::measure_masked(&log, None, &[], &[], from, to);
        assert!(m.rate_s_per_day.is_none(), "{from}-{to} s");
        assert_eq!(m.beats, 0, "{from}-{to} s");
    }
    // A start that is not a number reads from the start.
    let m = session::measure_masked(&log, None, &[], &[], f64::NAN, 10.0);
    assert!(m.beats > 0);
}

#[test]
fn a_clock_step_no_listed_pause_explains_is_cut() {
    // The system clock steps 1 s forward at audio second 98 with no pause
    // in the sound: the log has a line at the step, so the clock fit's
    // break sits there, and no listed break moves it. The beats run on
    // unbroken, so one fit across the step reads the 1 s as the watch
    // falling behind. First with nothing listed, the log running to 160 s;
    // then with a listed pause of 0.3 s at 100 s, too short for the clock
    // fit, the log ending at 100 s.
    let bph = 28_800;
    let period = 3600.0 / bph as f64 / (1.0 + 5.0 / 86400.0);
    for short in [None, Some(0.3)] {
        let mut lines: Vec<(f64, f64)> = (0..10)
            .map(|i| (i as f64 * 10.0, i as f64 * 10.0))
            .collect();
        lines.push((98.0, 99.0));
        let end = if short.is_some() { 100.0 } else { 160.0 };
        lines.extend(
            (0..)
                .map(|i| 100.0 + i as f64 * 10.0)
                .take_while(|&a| a <= end)
                .map(|a| (a, a + 1.0)),
        );
        let mut clock = timegrapher_core::clock::ClockFit::new(&lines).unwrap();
        let listed: Vec<timegrapher_core::breaks::Break> = short
            .iter()
            .map(|&gap_s| timegrapher_core::breaks::Break {
                audio_s: 100.0,
                gap_s,
            })
            .collect();
        clock.place_breaks(&listed);
        // The step's break, and no other: the short listed pause has no log
        // entry after it for the clock fit to take a break at.
        assert_eq!(clock.breaks.len(), 1, "{:?}", clock.breaks);
        assert!((clock.breaks[0].audio_s - 98.0).abs() < 1e-9);
        assert!((clock.breaks[0].gap_s - 1.0).abs() < 1e-6);
        let g = short.unwrap_or(0.0);
        let beats: Vec<timegrapher_core::beats::Beat> = (0..(156.0 / period) as i64)
            .filter_map(|k| {
                let t = k as f64 * period;
                if (100.0..100.0 + g).contains(&t) {
                    return None;
                }
                Some(timegrapher_core::beats::Beat {
                    index: k,
                    time: if t < 100.0 { t } else { t - g },
                    quality: 1.0,
                })
            })
            .collect();
        let log = timegrapher_core::stream::BeatLog {
            sample_rate: 44_100,
            duration_s: 156.0 - g,
            bph,
            lift_deg: 52.0,
            beats,
            amplitude_windows: Vec::new(),
            clipped_beats: Vec::new(),
            clipped_samples: 0,
            jumps: Vec::new(),
            sided: Vec::new(),
            by_count: Vec::new(),
        };
        let m = session::measure_masked(&log, Some(&clock), &listed, &[], 80.0, 150.0);
        let rate = m.rate_s_per_day.expect("rate");
        assert!((rate - 5.0).abs() < 0.01, "{short:?}: rate {rate} s/d");
    }
}

#[test]
fn a_stretch_of_exactly_ten_seconds_is_heard() {
    // Perfect beats at 28,800 bph over exactly 10 s, and just either side
    // of it (issue #228): with no beat at or after the 10 s window's end,
    // the stretch still has one whole window, so its scatter is measured
    // and the watch is heard. Before, 10 s exactly read "no watch heard".
    for duration in [9.999_f64, 10.0, 10.125, 20.0] {
        let log = timegrapher_core::stream::BeatLog {
            sample_rate: 48_000,
            duration_s: duration,
            bph: 28_800,
            lift_deg: 52.0,
            beats: (0..)
                .take_while(|&i| i as f64 * 0.125 < duration)
                .map(|i| timegrapher_core::beats::Beat {
                    index: i,
                    time: i as f64 * 0.125,
                    quality: 1.0,
                })
                .collect(),
            amplitude_windows: Vec::new(),
            clipped_beats: Vec::new(),
            clipped_samples: 0,
            jumps: Vec::new(),
            sided: Vec::new(),
            by_count: Vec::new(),
        };
        let m = session::measure(&log, None, 0.0, duration);
        assert!(m.watch_heard, "{duration} s: {:?}", m.caveats);
        assert!(m.rate_s_per_day.unwrap().abs() < 1e-6, "{duration} s");
        if duration >= 10.0 {
            assert!(m.jitter_us.is_some(), "{duration} s");
        }
    }
}
