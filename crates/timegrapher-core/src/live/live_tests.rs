use super::*;
use crate::synth::{generate, SynthConfig};
use crate::timing;

/// A hole in the sound that costs `lost_s` of the watch's beating (a
/// USB dropout, a knock that loses a beat) after `silence_s` of
/// silence, at 20 s in, heard live from a watch whose Tock sounds
/// unlike its Tick. Returns the analyzer and the reading at each pass,
/// with the breaks known then.
fn through_a_hole(
    bph: u32,
    silence_s: f64,
    lost_s: f64,
) -> (LiveAnalyzer, Vec<(LiveReading, usize)>) {
    let piece = |duration_s: f64, seed: u64| {
        generate(
            &SynthConfig {
                bph,
                duration_s,
                rate_s_per_day: 10.0,
                beat_error_ms: 0.6,
                // A louder unlock on the Tock.
                odd_gains: Some([0.9, 0.25, 1.0]),
                seed,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        )
    };
    // A watch beating on from where the first piece stopped (a whole
    // number of beats in), heard again after the silence and `lost_s`
    // more.
    let a = piece(HOLE_AT_S, 1);
    let b = piece(16.0, 2);
    let fs = a.sample_rate as f64;
    let mut samples = a.samples;
    samples.extend(std::iter::repeat_n(0.0, (silence_s * fs) as usize));
    samples.extend_from_slice(&b.samples[(lost_s * fs) as usize..]);
    let mut live = LiveAnalyzer::new(a.sample_rate, LiveConfig::default());
    let mut readings = Vec::new();
    for chunk in samples.chunks(960) {
        if live.push(chunk) {
            readings.push((live.reading(2.0), live.breaks().len()));
        }
    }
    (live, readings)
}

const HOLE_AT_S: f64 = 20.0;

/// The sides keep their names across a hole: the beat error after it,
/// by the beats' timing, by the unlock and on every amplitude window,
/// matches the one before. A hole that moved the watch's phase is one
/// jump, or two where passes straddling it call a second, and no
/// amplitude window reaches across one. Readings that name a side are
/// left out from a jump until its sides are checked.
fn check_hole(bph: u32, silence_s: f64, lost_s: f64) {
    let (live, passes) = through_a_hole(bph, silence_s, lost_s);
    let readings: Vec<&LiveReading> = passes.iter().map(|p| &p.0).collect();
    let case = format!("{bph} bph, {silence_s} s silent, {lost_s} s lost");
    let back = HOLE_AT_S + silence_s;
    let side = |from: f64, to: f64| {
        let run: Vec<_> = live
            .beats()
            .iter()
            .filter(|b| b.time >= from && b.time < to)
            .copied()
            .collect();
        timing::beat_error_ms(&run).expect("beat error")
    };
    let (before, after) = (side(5.0, HOLE_AT_S - 1.0), side(back + 6.0, f64::INFINITY));
    assert!(
        before.abs() > 0.4 && (before - after).abs() < 0.1,
        "{case}: beat error {before} then {after}"
    );
    let unlock = |r: &&LiveReading| r.beat_error_unlock_ms;
    let was = readings
        .iter()
        .filter(|r| (10.0..HOLE_AT_S - 1.0).contains(&r.time_s))
        .find_map(unlock)
        .expect("an unlock beat error before");
    let late: Vec<f64> = readings
        .iter()
        .filter(|r| r.time_s >= back + 6.0)
        .filter_map(unlock)
        .collect();
    assert!(
        !late.is_empty() && late.iter().all(|v| v.signum() == was.signum()),
        "{case}: unlock beat error {was} then {late:?}"
    );
    let breaks = live.breaks();
    for w in live.amplitude_windows() {
        assert!(
            !breaks.iter().any(|&b| w.start_s < b && w.end_s > b),
            "{case}: window {}-{} across {breaks:?}",
            w.start_s,
            w.end_s
        );
        if w.start_s >= back + 6.0 {
            if let Some(v) = w.beat_error_unlock_ms {
                assert_eq!(v.signum(), was.signum(), "{case}: window at {}", w.start_s);
            }
        }
    }
    // Found when the beats are found again, within a pass window and
    // the few beats a step in their timing needs. Where the phase didn't
    // move, the beats of passes straddling the silence can still step
    // as the silence leaves the window: at most one jump, checked like
    // any other.
    let recovery = back..back + LiveConfig::default().window_s + 1.0;
    let jumps = if lost_s == 0.0 { 0..=1 } else { 1..=2 };
    assert!(
        jumps.contains(&breaks.len()) && breaks.iter().all(|b| recovery.contains(b)),
        "{case}: {breaks:?}"
    );
    if breaks.is_empty() {
        return;
    }
    // Once the jump is found, the sides are not yet told apart: no
    // figure that names a side is read.
    let found = passes
        .iter()
        .find(|p| p.1 > 0)
        .map(|p| p.0.time_s)
        .expect("a pass after the jump");
    let early: Vec<&LiveReading> = passes
        .iter()
        .filter(|p| p.1 > 0 && p.0.time_s < found + 1.0)
        .map(|p| &p.0)
        .collect();
    for r in early {
        assert!(
            r.beat_error_ms.is_none()
                && r.beat_error_unlock_ms.is_none()
                && r.amplitude_even_deg.is_none()
                && r.amplitude_odd_deg.is_none(),
            "{case}: at {} {r:?}",
            r.time_s
        );
    }
    assert!(
        live.sides_known_at(live.duration_s()),
        "{case}: sides never checked"
    );
    super::sound_tests::sample_sides_agree(&live, &case);
}

#[test]
fn a_live_reading_keeps_the_supplied_rate_separate_from_detection() {
    for bph_set in [None, Some(28800)] {
        let mut cfg = LiveConfig::default();
        cfg.analysis.bph = bph_set;
        let mut live = LiveAnalyzer::new(48000, cfg);
        // A detector's guess does not become a user-supplied rate.
        live.bph = Some(72000);
        let reading = live.reading(30.0);
        assert_eq!(reading.bph_set, bph_set);
        let text = crate::measure::no_watch_text(
            crate::measure::NoWatchReason::Scatter,
            Some(8600.0),
            reading.bph_set,
        );
        assert_eq!(text.contains("% of a beat"), bph_set.is_some());
        assert!(text.contains("8.6 ms"));
    }
}

#[test]
fn a_short_silent_hole_keeps_the_sides() {
    for lost_s in [0.0, 0.04, 0.0625, 0.09] {
        check_hole(28_800, 1.0, lost_s);
    }
}

#[test]
fn a_hole_of_about_a_beat_keeps_the_sides() {
    check_hole(28_800, 0.0, 0.11);
    check_hole(28_800, 0.0, 0.14);
}

#[test]
fn a_silent_hole_of_about_a_beat_more_keeps_the_sides() {
    check_hole(28_800, 1.0, 0.11);
    check_hole(18_000, 0.0, 0.17);
}

pub(super) fn feed(a: &mut LiveAnalyzer, x: &[f32]) {
    for block in x.chunks(480) {
        a.push(block);
    }
}

#[test]
fn each_take_starts_positive_live_and_from_the_file() {
    // The same watch with its beat error either way round reads
    // positive from its first steady stretch on, listened to and from
    // the file alike; near no beat error the count names the sides.
    let read = |beat_error_ms: f64| {
        let audio = generate(
            &SynthConfig {
                duration_s: 40.0,
                beat_error_ms,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let mut a = LiveAnalyzer::new(audio.sample_rate, LiveConfig::default());
        feed(&mut a, &audio.samples);
        let log = crate::stream::analyze_samples(
            &audio.samples,
            audio.sample_rate,
            &crate::stream::StreamConfig::default(),
        );
        let file: Vec<Beat> = log
            .beats
            .iter()
            .filter(|b| b.time >= 5.0)
            .copied()
            .collect();
        let live = a.reading(30.0);
        (
            live.beat_error_ms.unwrap(),
            live.beat_error_unlock_ms.unwrap(),
            crate::timing::beat_error_ms(&file).unwrap(),
        )
    };
    for be in [0.6, -0.6, 0.3, -0.3] {
        let (live, unlock, file) = read(be);
        for v in [live, unlock, file] {
            assert!((v - be.abs()).abs() < 0.1, "{be}: {live} {unlock} {file}");
        }
    }
    // Too small for a sign, both read the size with one sign.
    let (live, _, file) = read(-0.03);
    assert!(live.abs() < 0.1 && file.abs() < 0.1, "{live} {file}");
}

#[test]
fn a_muted_input_has_no_signal_figure() {
    // A muted input: digital silence with a lone non-zero sample now
    // and then. The envelope's median is zero, so a beat peak over it
    // has no figure; it once read as an enormous one.
    let fs = 48_000;
    let mut x = vec![0.0f32; 10 * fs as usize];
    for k in (0..x.len()).step_by(6_000) {
        x[k] = 0.5;
    }
    let mut live = LiveAnalyzer::new(fs, LiveConfig::default());
    for chunk in x.chunks(4_800) {
        live.push(chunk);
    }
    let r = live.reading(10.0);
    assert_eq!(r.snr, None);
    assert_eq!(r.rate_s_per_day, None);
    let beats = [Beat {
        index: 0,
        time: 1.0,
        quality: 1.0,
    }];
    let mut env = vec![0.0f32; 2 * fs as usize];
    env[fs as usize] = 0.5;
    assert_eq!(signal_figure(&env, fs as f64, &beats), None);
    env.iter_mut().for_each(|v| *v += 0.01);
    assert_eq!(signal_figure(&env, fs as f64, &beats), Some(0.51 / 0.01));
}

#[test]
fn one_silence_is_one_break() {
    // A 6 s silence while listening: the beats step more than once
    // before they are found again, but the whole take counts one
    // break. Two silences 20 s apart count two.
    let breaks = |silences: &[f64]| {
        let audio = generate(
            &SynthConfig {
                duration_s: 70.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let fs = audio.sample_rate as usize;
        let mut x = audio.samples;
        for &at in silences {
            let at = (at * fs as f64) as usize;
            x[at..at + 6 * fs].fill(0.0);
        }
        let mut a = LiveAnalyzer::new(audio.sample_rate, LiveConfig::default());
        feed(&mut a, &x);
        let log = BeatLog {
            sample_rate: a.sample_rate(),
            duration_s: a.duration_s(),
            bph: a.bph().unwrap(),
            lift_deg: a.config().analysis.amplitude.lift_deg,
            beats: a.beats().to_vec(),
            amplitude_windows: a.amplitude_windows().to_vec(),
            clipped_beats: Vec::new(),
            clipped_samples: 0,
            jumps: a.breaks().to_vec(),
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        };
        let cuts: Vec<Break> = log
            .jumps
            .iter()
            .map(|&audio_s| Break {
                audio_s,
                gap_s: 0.0,
            })
            .collect();
        let m = crate::session::measure_masked(&log, None, &cuts, &[], 0.0, log.duration_s);
        let count = m.caveats.iter().find_map(|c| match c {
            Caveat::Breaks { count } => Some(*count),
            _ => None,
        });
        (count, log.jumps)
    };
    let (one, jumps) = breaks(&[16.0]);
    assert_eq!(one, Some(1), "{jumps:?}");
    let (two, jumps) = breaks(&[16.0, 42.0]);
    assert_eq!(two, Some(2), "{jumps:?}");
}

#[test]
fn noise_logs_no_beats_but_keeps_its_peaks_to_show() {
    // Steady noise, the same each run.
    let mut seed = 12_345_u32;
    let x: Vec<f32> = (0..48_000 * 20)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 24) as f32 * 0.02 - 0.01
        })
        .collect();
    let mut a = LiveAnalyzer::new(48_000, LiveConfig::default());
    feed(&mut a, &x);
    assert!(a.beats().is_empty(), "{} beats", a.beats().len());
    let c = a.candidates();
    assert!(c.len() > 20, "{} candidates", c.len());
    assert!(c
        .windows(2)
        .all(|w| w[1].time > w[0].time && w[1].index > w[0].index));
    // Kept no longer than the limit, and gone with the log.
    assert!(c.len() <= CANDIDATES_KEPT);
    a.set_bph(Some(28_800));
    assert!(a.candidates().is_empty());
}

// Supplied beat times only: no waveform synthesis or detector involved.
pub(super) fn viewer_phase_log(before_rate: f64, after_rate: f64) -> BeatLog {
    let before_period = crate::timing::period_s(0.125, before_rate);
    let after_period = crate::timing::period_s(0.125, after_rate);
    BeatLog {
        sample_rate: 48000,
        duration_s: 120.0,
        bph: 28800,
        lift_deg: 52.0,
        beats: (0..960)
            .map(|index| Beat {
                index,
                time: if index < 480 {
                    index as f64 * before_period
                } else {
                    60.03 + (index - 480) as f64 * after_period
                },
                quality: if index == 500 { 0.1 } else { 1.0 },
            })
            .collect(),
        amplitude_windows: vec![],
        clipped_beats: vec![],
        clipped_samples: 0,
        jumps: vec![60.03],
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    }
}

#[test]
fn trend_source_lines_never_take_the_other_sources_readings() {
    use crate::session::BeatErrorSource::{Drop, Unlock};
    let r = LiveAnalyzer::from_log(viewer_phase_log(10.0, 10.0), LiveConfig::default())
        .reading_at(50.0, 20.0);
    let mut point = TrendReading::of(50.0, &r);
    point.beat_error_unlock_ms = Some(0.2);
    point.beat_error_ms = Some(0.8);
    assert_eq!(point.beat_error_from(Unlock), Some(0.2));
    assert_eq!(point.beat_error_from(Drop), None);
    point.beat_error_unlock_ms = None;
    assert_eq!(point.beat_error_from(Unlock), None);
    assert_eq!(point.beat_error_from(Drop), Some(0.8));
}

#[test]
fn live_beat_error_names_its_source_and_only_drop_has_the_caveat() {
    use crate::session::BeatErrorSource;
    let mut log = viewer_phase_log(10.0, 10.0);
    log.amplitude_windows = (0..30)
        .map(|i| AmplitudeWindow {
            start_s: i as f64 * 2.0,
            end_s: (i + 1) as f64 * 2.0,
            even_deg: Some(270.0),
            odd_deg: Some(270.0),
            beat_error_ms: Some(0.8),
            beat_error_unlock_ms: Some(0.2),
            even_unlock_ms: None,
            odd_unlock_ms: None,
            even_drop_ms: None,
            odd_drop_ms: None,
            culled_ms: None,
            faint_withheld: [false; 2],
        })
        .collect();
    let mut live = LiveAnalyzer::from_log(log, LiveConfig::default());
    let unlock = live.reading_at(50.0, 20.0);
    assert_eq!(unlock.beat_error_source, Some(BeatErrorSource::Unlock));
    assert_eq!(unlock.beat_error_unlock_ms, Some(0.2));
    assert!(!unlock.caveats.contains(&Caveat::FROM_DROP));
    for w in &mut live.amp {
        w.beat_error_unlock_ms = None;
    }
    let drop = live.reading_at(50.0, 20.0);
    assert_eq!(drop.beat_error_source, Some(BeatErrorSource::Drop));
    assert_eq!(drop.beat_error_ms, unlock.beat_error_ms);
    assert!(drop.caveats.contains(&Caveat::FROM_DROP));
    assert_eq!(drop.beat_error_unlock_error_ms, None);
    let empty = LiveAnalyzer::new(48000, LiveConfig::default()).reading(20.0);
    assert_eq!(empty.beat_error_source, None);
    assert!(!empty.caveats.contains(&Caveat::FROM_DROP));
}

/// A take's clock that drifts corrects each reading by its error over
/// that reading, as `session::measure` reads the take on the clock's
/// mapping, not by its one figure for the whole take.
#[test]
fn a_drifting_clock_corrects_each_reading_by_its_error_then() {
    // The card runs from 0 to 80 ppm slow over five hours, long enough
    // for its log's mapping to follow the drift: true time is
    // t + 40e-6 t² / 18000 of audio time t.
    let dur = 18_000.0;
    let true_of = |t: f64| t + 40e-6 * t * t / dur;
    let pairs: Vec<(f64, f64)> = (0..=300)
        .map(|k| {
            let t = 60.0 * k as f64;
            (t, 1_700_000_000.0 + true_of(t))
        })
        .collect();
    let fit = crate::clock::ClockFit::for_rates(&pairs).unwrap();
    assert!(fit.corrects() && fit.tracks_drift);
    // A watch keeping true time exactly: each beat at a true 0.125 s,
    // heard at the audio time that maps to it.
    let audio_of = |tt: f64| {
        let mut t = tt;
        for _ in 0..4 {
            t -= true_of(t) - tt;
        }
        t
    };
    let n = (dur / 0.125) as i64;
    let mut log = viewer_phase_log(0.0, 0.0);
    log.duration_s = dur;
    log.jumps.clear();
    log.beats = (0..n)
        .map(|index| Beat {
            index,
            time: audio_of(index as f64 * 0.125),
            quality: 1.0,
        })
        .collect();
    // What `session::measure` reads over audio `from..to`: the beats
    // put on the clock's mapping, then fitted.
    let on_clock = |from: f64, to: f64| {
        let beats: Vec<Beat> = log
            .beats
            .iter()
            .filter(|b| b.time >= from && b.time <= to)
            .map(|b| Beat {
                time: fit.map(b.time),
                ..*b
            })
            .collect();
        crate::timing::fit(&beats, 28800).unwrap().rate_s_per_day
    };
    let mut a = LiveAnalyzer::from_log(log.clone(), LiveConfig::default());
    a.set_rate_clock(Some(&fit));
    for end in [1_000.0, 9_000.0, 17_000.0] {
        let card = a.reading_at(end, 60.0).rate_s_per_day.expect("a rate");
        let session = on_clock(end - 60.0, end);
        assert!((card - session).abs() < 0.05, "{end}: {card} {session}");
    }
    // Late in the take the card is about 76 ppm slow, against 40 over
    // the whole take: the take's one figure would be 3 s/d out there.
    let heard = LiveAnalyzer::from_log(log.clone(), LiveConfig::default())
        .reading_at(17_000.0, 60.0)
        .rate_s_per_day
        .unwrap();
    let one = crate::clockstore::correct_rate(heard, fit.ppm);
    assert!((one - on_clock(16_940.0, 17_000.0)).abs() > 1.0, "{one}");
}

/// A recording read whole keeps its jumps as breaks: no reading after
/// one uses an amplitude window that started before it.
#[test]
fn a_window_across_a_jump_in_a_read_recording_is_left_out() {
    let mut log = viewer_phase_log(10.0, 10.0);
    let win = |a: f64, deg: f64| AmplitudeWindow {
        start_s: a,
        end_s: a + 2.0,
        even_deg: Some(deg),
        odd_deg: Some(deg),
        beat_error_ms: None,
        beat_error_unlock_ms: None,
        even_unlock_ms: None,
        odd_unlock_ms: None,
        even_drop_ms: None,
        odd_drop_ms: None,
        culled_ms: None,
        faint_withheld: [false; 2],
    };
    // The jump is at 60.03 s: the window from 60 s reaches across it.
    log.amplitude_windows = vec![win(58.0, 280.0), win(60.0, 350.0), win(62.0, 280.0)];
    let a = LiveAnalyzer::from_log(log, LiveConfig::default());
    assert_eq!(a.reading_at(62.5, 2.0).amplitude_deg, None);
    assert_eq!(a.reading_at(64.0, 4.0).amplitude_deg, Some(280.0));
    assert_eq!(a.reading_at(60.0, 2.0).amplitude_deg, Some(280.0));
}

/// Steady beats for `seconds`: a watch whose Tock sounds unlike its
/// Tick, and a live analyzer fed the first `fed` seconds of them.
pub(super) fn steady_live(seconds: f64, fed: f64) -> (LiveAnalyzer, Vec<f32>) {
    let audio = generate(
        &SynthConfig {
            duration_s: seconds,
            beat_error_ms: 0.6,
            odd_gains: Some([0.9, 0.25, 1.0]),
            ..Default::default()
        },
        |_| 270.0,
        |_| 0.0,
    );
    let mut live = LiveAnalyzer::new(audio.sample_rate, LiveConfig::default());
    let k = (fed * audio.sample_rate as f64) as usize;
    feed(&mut live, &audio.samples[..k]);
    (live, audio.samples[k..].to_vec())
}

/// A jump found by the beats' timing after a new position, from before
/// it, starts no amplitude window that reaches back across the new
/// position's break.
#[test]
fn a_late_jump_after_a_new_position_keeps_windows_off_its_break() {
    let (mut live, rest) = steady_live(40.0, 30.0);
    live.new_stretch();
    let stretch = live.duration_s();
    // Off the 2-second grid: a fresh grid from the jump would put a
    // window across the break.
    live.jump_at(stretch - 1.5, true);
    assert_eq!(live.next_amp_s, Some(live.settled_to));
    feed(&mut live, &rest);
    let breaks = live.breaks().to_vec();
    assert!(breaks.contains(&stretch), "{breaks:?}");
    assert!(live
        .amplitude_windows()
        .iter()
        .any(|w| w.start_s >= stretch));
    for w in live.amplitude_windows() {
        assert!(
            !breaks.iter().any(|&b| w.start_s < b && w.end_s > b),
            "window {}-{} across {breaks:?}",
            w.start_s,
            w.end_s
        );
    }
}

#[test]
fn a_masked_stretch_is_cut_like_a_break() {
    // 10 s/d before 60 s, 200 s/d after: a mask over the start of the
    // second run leaves readings after it on that run alone, and none
    // inside it.
    let mut log = viewer_phase_log(10.0, 200.0);
    log.jumps.clear();
    let mut a = LiveAnalyzer::from_log(log, LiveConfig::default());
    let before = a.reading_at(90.0, 60.0);
    a.set_masked(&[(70.0, 80.0), (55.0, 62.0)]);
    assert_eq!(a.masked(), [(55.0, 62.0), (70.0, 80.0)]);
    let inside = a.reading_at(75.0, 30.0);
    assert_eq!(inside.rate_s_per_day, None);
    assert_eq!(inside.beats_used, 0);
    let after = a.reading_at(90.0, 60.0);
    assert!((after.span_s - 10.0).abs() < 0.2, "{}", after.span_s);
    assert!(after
        .rate_s_per_day
        .is_some_and(|r| (r - 200.0).abs() < 5.0));
    assert!(after.caveats.contains(&Caveat::Masked { seconds: 17.0 }));
    assert!(!before
        .caveats
        .iter()
        .any(|c| matches!(c, Caveat::Masked { .. })));
    a.set_masked(&[]);
    assert_eq!(a.reading_at(90.0, 60.0).span_s, before.span_s);
}

/// A reading is whole only when its averaging time reaches into no
/// cut and each 2 s of it holds a beat.
#[test]
fn a_reading_is_whole_only_clear_of_every_cut() {
    let mut log = viewer_phase_log(10.0, 10.0);
    log.jumps.clear();
    // No beats from 40 to 43 s.
    log.beats.retain(|b| !(40.0..43.0).contains(&b.time));
    let mut a = LiveAnalyzer::from_log(log, LiveConfig::default());
    assert!(a.reading_at(30.0, 20.0).whole);
    assert!(!a.reading_at(50.0, 20.0).whole);
    assert!(a.reading_at(53.0, 10.0).whole);
    a.set_masked(&[(70.0, 80.0)]);
    assert!(!a.reading_at(90.0, 20.0).whole);
    assert!(!a.reading_at(89.0, 10.0).whole);
    assert!(a.reading_at(90.0, 10.0).whole);
    a.set_breaks(vec![95.0]);
    assert!(!a.reading_at(100.0, 10.0).whole);
    assert!(a.reading_at(105.0, 10.0).whole);
}

#[test]
fn viewer_known_phase_jump_does_not_become_rate_or_irregularity() {
    let log = viewer_phase_log(0.0, 0.0);
    let original: Vec<_> = log
        .beats
        .iter()
        .map(|b| (b.index, b.time, b.quality))
        .collect();
    let a = LiveAnalyzer::from_log(log, LiveConfig::default());
    let crossing = a.reading_at(90.0, 60.0);
    println!(
        "known phase: cuts={:?}, span={}, heard={}, rate={:?}, jitter={:?}",
        a.breaks(),
        crossing.span_s,
        crossing.watch_heard,
        crossing.rate_s_per_day,
        crossing.jitter_us
    );
    assert_eq!(a.breaks(), &[60.03]);
    assert_eq!(
        a.beats()
            .iter()
            .map(|b| (b.index, b.time, b.quality))
            .collect::<Vec<_>>(),
        original
    );
    let crossing = a.reading_at(90.0, 60.0);
    assert!(crossing.span_s <= 90.0 - 60.03 + 1e-9, "{crossing:?}");
    assert!(
        crossing.watch_heard,
        "a supplied phase cut is not an irregular watch"
    );
    assert!(
        crossing.rate_s_per_day.unwrap().abs() < 0.001,
        "{crossing:?}"
    );
    assert!(crossing.jitter_us.unwrap() < 0.001);
    assert!(a.reading_at(45.0, 20.0).rate_s_per_day.unwrap().abs() < 0.001);
    assert!(a.reading_at(105.0, 10.0).rate_s_per_day.unwrap().abs() < 0.001);
    // We preserve supplied jumps; we do not infer one from a phase change.
    let mut unmarked = viewer_phase_log(0.0, 0.0);
    unmarked.jumps.clear();
    let unmarked = LiveAnalyzer::from_log(unmarked, LiveConfig::default());
    assert!(unmarked.breaks().is_empty());
    assert!(unmarked.reading_at(90.0, 60.0).span_s > 50.0);
}

#[test]
fn an_opened_log_says_where_its_sides_were_named_by_count() {
    let mut log = viewer_phase_log(0.0, 0.0);
    log.by_count = log.jumps.clone();
    let jumps = log.jumps.clone();
    let a = LiveAnalyzer::from_log(log, LiveConfig::default());
    // The app's hover lists them as the log gave them.
    assert!(!jumps.is_empty());
    assert_eq!(a.by_count(), jumps.as_slice());
    let by_count = |end_s: f64| {
        a.reading_at(end_s, 20.0)
            .caveats
            .iter()
            .find_map(|c| match c {
                Caveat::SidesByCount { count } => Some(*count),
                _ => None,
            })
    };
    // Before the jump the sides are as heard; after it, by the count.
    assert_eq!(by_count(45.0), None);
    assert_eq!(by_count(90.0), Some(1));
    // Without the log's by-count jumps, nothing is said.
    let plain = LiveAnalyzer::from_log(viewer_phase_log(0.0, 0.0), LiveConfig::default());
    assert!(plain.by_count().is_empty());
    assert!(!plain
        .reading_at(90.0, 20.0)
        .caveats
        .iter()
        .any(|c| matches!(c, Caveat::SidesByCount { .. })));
}

#[test]
fn viewer_latest_piece_is_distinct_from_the_retained_whole_recording() {
    let log = viewer_phase_log(10.0, -20.0);
    let whole = timing::fit_pieces(&log.beats, log.bph, &log.jumps).unwrap();
    let a = LiveAnalyzer::from_log(log, LiveConfig::default());
    let early = a.reading_at(45.0, 30.0);
    let latest = a.reading_at(90.0, 60.0);
    println!(
        "unequal pieces: early={:?}, latest={:?}, whole={}",
        early.rate_s_per_day, latest.rate_s_per_day, whole.rate_s_per_day
    );
    assert!((early.rate_s_per_day.unwrap() - 10.0).abs() < 0.001);
    assert!(
        (latest.rate_s_per_day.unwrap() + 20.0).abs() < 0.001,
        "{latest:?}"
    );
    assert!(latest.span_s < 30.0);
    assert_eq!(
        a.beats().len(),
        960,
        "all pieces remain available for historical views"
    );
    assert!(whole.beats_used > latest.beats_used);
    assert!(
        (whole.rate_s_per_day + 5.0).abs() < 0.05,
        "whole fit remains a different aggregation: {whole:?}"
    );
}

#[test]
fn viewer_external_cuts_cannot_replace_intrinsic_jumps() {
    let mut log = viewer_phase_log(0.0, 0.0);
    log.jumps = vec![100.0, 60.03, 60.03];
    let mut a = LiveAnalyzer::from_log(log, LiveConfig::default());
    assert_eq!(a.breaks(), &[60.03, 100.0]);
    a.set_breaks(vec![90.0, 30.0, 60.0, 40.0, 60.03, 100.0]);
    assert_eq!(a.breaks(), &[30.0, 40.0, 60.0, 60.03, 90.0, 100.0]);
    a.set_breaks(vec![80.0]);
    assert_eq!(
        a.breaks(),
        &[60.03, 80.0, 100.0],
        "only external cuts are replaced"
    );
    a.set_breaks(vec![]);
    assert_eq!(a.breaks(), &[60.03, 100.0]);
    a.restart();
    assert!(a.beats().is_empty());
    assert!(a.breaks().is_empty());
    a.set_breaks(vec![110.0]);
    assert_eq!(
        a.breaks(),
        &[110.0],
        "discarded history cannot reintroduce old jumps"
    );
}

/// Room noise with no watch, seeded so the test repeats.
fn empty_stand(secs: usize) -> Vec<f32> {
    let mut x = 0x2545_f491_4f6c_dd1du64;
    (0..48_000 * secs)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            ((x >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.02
        })
        .collect()
}

/// The stand's noise, then a few seconds of a watch, heard live: the
/// noise before a regular beat was found counts by its peaks, as the
/// saved reading of the same sound logs them, so the short run at the
/// end is no watch either way (#679, #680). After 8 s of the watch no
/// whole window holds it; after 20 s two do, and only the noise's
/// peaks before them say no watch over a 60 s average, as saved does.
#[test]
fn a_watch_heard_briefly_after_the_stand_noise_is_no_watch_live_or_saved() {
    let rms = crate::synth::noise_rms(SynthConfig::default().snr_db) * crate::synth::SCALE;
    for (watch_s, averages) in [(8.0, &[10.0, 30.0, 60.0][..]), (20.0, &[60.0][..])] {
        let mut rng = crate::synth::Rng::new(5);
        let mut x: Vec<f32> = (0..110 * 48_000)
            .map(|_| (rms * rng.normal()) as f32)
            .collect();
        let watch = generate(
            &SynthConfig {
                duration_s: watch_s,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        x.extend_from_slice(&watch.samples);
        let mut a = LiveAnalyzer::new(48_000, LiveConfig::default());
        feed(&mut a, &x);
        let log =
            crate::stream::analyze_samples(&x, 48_000, &crate::stream::StreamConfig::default());
        let end = x.len() as f64 / 48_000.0;
        for &average_s in averages {
            let live = a.reading(average_s);
            let saved = crate::session::measure_masked(&log, None, &[], &[], end - average_s, end);
            let at = format!("{watch_s} s of watch, {average_s} s average");
            assert!(!saved.watch_heard, "{at}, saved");
            assert!(!live.watch_heard, "{at}, live: {live:?}");
        }
    }
}

/// A break (a new position) starts the verdict again: the stand's
/// noise before it does not count against the watch heard after it.
/// Before a whole 10 s window is heard after the break, regularity is
/// not judged on the scatter, and the beats' sound says a watch.
#[test]
fn a_break_starts_the_watch_verdict_again() {
    let rms = crate::synth::noise_rms(SynthConfig::default().snr_db) * crate::synth::SCALE;
    let mut rng = crate::synth::Rng::new(5);
    let mut x: Vec<f32> = (0..60 * 48_000)
        .map(|_| (rms * rng.normal()) as f32)
        .collect();
    let watch = generate(
        &SynthConfig {
            duration_s: 30.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let mut a = LiveAnalyzer::new(48_000, LiveConfig::default());
    feed(&mut a, &x);
    a.new_stretch();
    let (early, late) = watch.samples.split_at(9 * 48_000);
    feed(&mut a, early);
    let r = a.reading(60.0);
    assert!(r.timing_scatter_fraction.is_none(), "{r:?}");
    assert!(r.watch_heard, "{r:?}");
    feed(&mut a, late);
    let r = a.reading(60.0);
    x.extend_from_slice(&watch.samples);
    let log = crate::stream::analyze_samples(&x, 48_000, &crate::stream::StreamConfig::default());
    let at = [crate::breaks::Break {
        audio_s: 60.0,
        gap_s: 0.0,
    }];
    let saved = crate::session::measure_masked(&log, None, &at, &[], 60.0, 90.0);
    assert!(saved.watch_heard, "{saved:?}");
    assert!(r.watch_heard, "{r:?}");
}

/// One engine reads a stretch (#512): with a break, or a mask's end,
/// inside the averaging time, live reads its latest run, and that
/// reading is the saved reading of the same stretch, fitted as it was
/// fitted before (whole, its beat error's sign not turned).
#[test]
fn live_reads_its_latest_run_as_a_saved_reading_of_it() {
    let watch = generate(
        &SynthConfig {
            duration_s: 120.0,
            beat_error_ms: 0.6,
            ..Default::default()
        },
        |_| 280.0,
        |t| 4.0 + t / 60.0,
    );
    let log = crate::stream::analyze_samples(
        &watch.samples,
        watch.sample_rate,
        &crate::stream::StreamConfig::default(),
    );
    let (from, end) = (60.0, 110.0);
    let brk = [crate::breaks::Break {
        audio_s: from,
        gap_s: 0.0,
    }];
    let mask = [(50.0, from)];
    for (breaks, masked) in [(&brk[..], &[][..]), (&[][..], &mask[..])] {
        let mut live = LiveAnalyzer::from_log(log.clone(), LiveConfig::default());
        live.set_breaks(breaks.iter().map(|b| b.audio_s).collect());
        live.set_masked(masked);
        let r = live.reading_at(end, 90.0);
        let saved = crate::session::measure_masked(&log, None, breaks, masked, from, end);
        let close = |a: Option<f64>, b: Option<f64>, what: &str| {
            let (a, b) = (a.expect(what), b.expect(what));
            assert!(
                (a - b).abs() <= 1e-9 * b.abs().max(1.0),
                "{what}: live {a}, saved {b}"
            );
        };
        close(r.rate_s_per_day, saved.rate_s_per_day, "rate");
        close(r.beat_error_ms, saved.beat_error_ms, "beat error");
        close(
            r.beat_error_unlock_ms,
            saved.beat_error_unlock_ms,
            "unlock beat error",
        );
        close(r.amplitude_deg, saved.amplitude_deg, "amplitude");
        // As live fitted it before: the run whole, one sign.
        let run: Vec<crate::beats::Beat> = log
            .beats
            .iter()
            .filter(|b| b.time >= from && b.time <= end)
            .copied()
            .collect();
        let whole = timing::fit(&run, log.bph).unwrap();
        assert_eq!(r.rate_s_per_day, Some(whole.rate_s_per_day));
        assert_eq!(
            r.beat_error_ms.map(f64::abs),
            Some(whole.beat_error_ms.abs())
        );
    }
}

/// Noise-scattered beats, a pause, then a few seconds of a tidy run:
/// the live card judges the sound since the latest break, as a saved
/// reading does, so the short run at the end is no watch, and live and
/// saved give one scatter (#679, #680).
#[test]
fn a_short_tidy_run_after_noise_is_no_watch_live_or_saved() {
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let mut rand = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x >> 40) as f64 / (1u64 << 24) as f64
    };
    let mut times: Vec<f64> = Vec::new();
    let mut t = 0.0;
    while t < 114.0 {
        times.push(t);
        t += 0.05 + 0.15 * rand();
    }
    times.extend((0..30).map(|i| 116.2 + i as f64 * 0.125));
    let log = BeatLog {
        sample_rate: 48000,
        duration_s: 120.0,
        bph: 28800,
        lift_deg: 52.0,
        beats: times
            .iter()
            .enumerate()
            .map(|(index, &time)| Beat {
                index: index as i64,
                time,
                quality: 1.0,
            })
            .collect(),
        amplitude_windows: vec![],
        clipped_beats: vec![],
        clipped_samples: 0,
        jumps: vec![],
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    let a = LiveAnalyzer::from_log(log.clone(), LiveConfig::default());
    let end = 120.0;
    for average_s in [10.0, 30.0] {
        let live = a.reading_at(end, average_s);
        assert!(!live.watch_heard, "{average_s}: {live:?}");
    }
    let live = a.reading_at(end, 30.0);
    let saved = crate::session::measure_masked(&log, None, &[], &[], end - 30.0, end);
    assert!(!saved.watch_heard);
    assert_eq!(live.jitter_us, saved.jitter_us);
}

/// An empty stand, heard live or read back from its beat log over a
/// 10 s average, gives no readings, and says no regular beat was found.
#[test]
fn an_empty_stand_reads_nothing() {
    let x = empty_stand(60);
    let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
    feed(&mut a, &x);
    let r = a.reading(10.0);
    assert!(!r.watch_heard);
    assert!(
        r.rate_s_per_day.is_none() && r.beat_error_ms.is_none(),
        "{r:?}"
    );
    assert!(
        r.amplitude_deg.is_none() && r.beat_error_unlock_ms.is_none(),
        "{r:?}"
    );
    assert!(
        r.rate_error_s_per_day.is_none()
            && r.caveats.first()
                == Some(&Caveat::NoWatch {
                    reason: NoWatchReason::Scatter
                })
    );
}

/// The readings carry what `analyze` gives a recording: a short
/// stretch is said, the rate's ± waits for enough of a run and then
/// follows the averaging time, and unknowns are caveats.
#[test]
fn readings_carry_their_errors_and_caveats() {
    let cfg = SynthConfig {
        // Past the 300 s of beats the rate's ± takes its scatter from
        // (#1470).
        duration_s: 330.0,
        rate_s_per_day: 5.0,
        snr_db: 30.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
    feed(&mut a, &audio.samples);
    let early = a.reading_at(8.0, 10.0);
    assert!(
        early
            .caveats
            .iter()
            .any(|c| matches!(c, Caveat::ShortSpan { .. })),
        "{:?}",
        early.caveats
    );
    assert!(early.rate_error_s_per_day.is_none(), "{early:?}");
    // Under 10 s of sound regularity is not assessed: the readings
    // are shown, not withheld.
    assert!(early.watch_heard, "{early:?}");
    assert!(
        early.rate_s_per_day.is_some() && early.amplitude_deg.is_some(),
        "{early:?}"
    );
    let late = a.reading(10.0);
    assert!(
        late.caveats.contains(&Caveat::LiftUnknown),
        "{:?}",
        late.caveats
    );
    assert!(
        late.caveats.contains(&Caveat::ClockUnknown),
        "{:?}",
        late.caveats
    );
    assert!(late.beat_error_unlock_error_ms.is_some(), "{late:?}");
    let ten = late.rate_error_s_per_day.expect("rate ±");
    let sixty = a.reading(60.0).rate_error_s_per_day.unwrap();
    assert!(sixty < ten, "{sixty} against {ten}");
    a.set_known(true, true);
    let known = a.reading(10.0).caveats;
    assert!(known.is_empty(), "{known:?}");
}

/// Driven five times past full scale, the beats are still read, and
/// the clipping is said.
#[test]
fn heavy_clipping_still_reads() {
    let cfg = SynthConfig {
        duration_s: 60.0,
        rate_s_per_day: 10.0,
        beat_error_ms: 0.6,
        snr_db: 24.0,
        seed: 13,
        ..Default::default()
    };
    let mut audio = generate(&cfg, |t| 270.0 + 10.0 * (t / 60.0).sin(), |_| 0.0);
    let peak = audio.samples.iter().fold(0f32, |m, v| m.max(v.abs()));
    for v in &mut audio.samples {
        *v = (*v * 5.0 / peak).clamp(-1.0, 1.0);
    }
    let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
    feed(&mut a, &audio.samples);
    let r = a.reading(10.0);
    assert!(
        r.beats_used > 0 && r.watch_heard && r.clipped_fraction.is_some_and(|f| f > 0.5),
        "{r:?}"
    );
}

#[test]
fn a_steady_watch_keeps_both_sides_amplitude() {
    let cfg = SynthConfig {
        duration_s: 40.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |t| 270.0 + 10.0 * (t / 6.0).sin(), |_| 0.0);
    let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
    feed(&mut a, &audio.samples);
    let w = a.amplitude_windows();
    assert!(w.len() > 15);
    assert!(w
        .iter()
        .all(|w| w.even_deg.is_some() && w.odd_deg.is_some()));
}

#[test]
fn a_new_stretch_keeps_the_beats_and_breaks_the_readings() {
    let cfg = SynthConfig {
        duration_s: 40.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |t| if t < 20.0 { 0.0 } else { 30.0 });
    let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
    let half = audio.samples.len() / 2;
    feed(&mut a, &audio.samples[..half]);
    let first = a.reading(10.0).rate_s_per_day.unwrap();
    let before = a.beats().len();
    a.new_stretch();
    assert_eq!(a.beats().len(), before, "the beats are kept");
    assert!(a.tick_profiles().iter().all(Option::is_none));
    assert_eq!(a.breaks(), &[20.0]);
    feed(&mut a, &audio.samples[half..]);
    assert!(a.beats().len() > before);
    let r = a.reading(30.0);
    assert!(r.span_s <= 20.0 + 1e-6, "{}", r.span_s);
    let rate = r.rate_s_per_day.unwrap();
    assert!(
        (rate - first - 30.0).abs() < 3.0,
        "only the new stretch: {first} then {rate}"
    );
    assert!(a.tick_profiles().iter().any(Option::is_some));
}

/// Setting the rate Auto already found (a calibre picked while
/// listening) keeps the beats heard so far, as does going back to
/// Auto; another rate starts the beat log again.
#[test]
fn the_rate_already_found_keeps_the_beats() {
    let cfg = SynthConfig {
        duration_s: 20.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
    feed(&mut a, &audio.samples);
    assert_eq!(a.bph(), Some(28_800));
    let heard = a.beats().len();
    assert!(heard > 100, "{heard}");
    assert!(a.set_bph(Some(28_800)));
    assert_eq!(a.beats().len(), heard);
    assert!(a.reading(10.0).rate_s_per_day.is_some());
    assert!(a.set_bph(None));
    assert_eq!(a.beats().len(), heard);
    assert_eq!(a.bph(), Some(28_800));
    assert!(!a.set_bph(Some(21_600)));
    assert!(a.beats().is_empty());
    // Auto after a rate set by hand listens afresh and finds 28800.
    feed(&mut a, &audio.samples);
    assert!(!a.set_bph(None));
    assert!(a.beats().is_empty());
    feed(&mut a, &audio.samples);
    assert_eq!(a.bph(), Some(28_800));
    assert!(a.beats().len() > 100, "{}", a.beats().len());
}

#[test]
fn clipped_beats_are_counted_as_they_settle() {
    let cfg = SynthConfig {
        duration_s: 20.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
    feed(&mut a, &audio.samples);
    let clean = a.reading(10.0);
    assert_eq!(clean.clipped_beats, 0);
    assert_eq!(clean.clipped_fraction, Some(0.0));

    // The next 20 s 12 dB too hot, clipped at full scale.
    let peak = audio.samples.iter().fold(0f32, |m, v| m.max(v.abs()));
    let hot: Vec<f32> = audio
        .samples
        .iter()
        .map(|v| (v * 4.0 / peak).clamp(-1.0, 1.0))
        .collect();
    feed(&mut a, &hot);
    let r = a.reading(10.0);
    assert!(r.clipped_fraction.unwrap() > 0.5, "{r:?}");
    // A reading back in the clean stretch has no clipped beats.
    assert_eq!(a.reading_at(15.0, 10.0).clipped_fraction, Some(0.0));

    // Made again from its beat log (a recording analysed at once), it
    // counts the same.
    let log = BeatLog {
        sample_rate: 48000,
        duration_s: a.duration_s(),
        bph: a.bph().unwrap(),
        lift_deg: a.config().analysis.amplitude.lift_deg,
        beats: a.beats().to_vec(),
        amplitude_windows: a.amp.clone(),
        clipped_beats: a.clipped_beats().to_vec(),
        clipped_samples: 0,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    let b = LiveAnalyzer::from_log(log, LiveConfig::default());
    let end = b.duration_s();
    let again = b.reading_at(end, 10.0);
    assert!(again.clipped_fraction.unwrap() > 0.5, "{again:?}");
    assert_eq!(again.clipped_beats, a.reading_at(end, 10.0).clipped_beats);
    assert_eq!(b.reading_at(15.0, 10.0).clipped_fraction, Some(0.0));
}

#[test]
fn readings_cover_only_the_run_since_a_pause() {
    let cfg = SynthConfig {
        duration_s: 20.0,
        rate_s_per_day: 10.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 270.0, |_| 0.0);
    let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
    feed(&mut a, &audio.samples);
    let before = a.reading(60.0);
    assert!((before.span_s - 20.0).abs() < 3.0, "span {}", before.span_s);
    a.skip(30.0);
    feed(&mut a, &audio.samples);
    let r = a.reading(60.0);
    // Only the 20 s since the pause, not the beats before it.
    assert!(r.span_s < 20.0 && r.span_s > 10.0, "span {}", r.span_s);
    let rate = r.rate_s_per_day.expect("rate");
    assert!((rate - 10.0).abs() < 3.0, "rate {rate}");
    // The beats before the pause are kept, a gap behind the new ones.
    assert!(a.beats().iter().any(|b| b.time < 20.0));
    assert!(a.beats().iter().any(|b| b.time > 50.0));
    assert!(a.duration_s() > 69.0);
}

#[test]
fn profiles_cover_more_than_one_pass() {
    let cfg = SynthConfig {
        duration_s: 20.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 270.0, |_| 0.0);
    let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
    a.set_profile_span(10.0);
    feed(&mut a, &audio.samples);
    for p in a.tick_profiles() {
        let p = p.as_ref().expect("profile");
        // 10 s is 40 beats a side; the pass guard trims a couple.
        assert!((36..=41).contains(&p.beats), "{} beats", p.beats);
        let amp = p.amplitude_deg.expect("amplitude");
        assert!((amp - 270.0).abs() < 10.0, "amplitude {amp}");
    }
    a.set_profile_span(2.0);
    feed(&mut a, &audio.samples[..48000]);
    for p in a.tick_profiles() {
        let n = p.as_ref().expect("profile").beats;
        assert!(n <= 9, "{n} beats after shortening");
    }
}

#[test]
fn follows_a_synthetic_watch() {
    let cfg = SynthConfig {
        rate_s_per_day: 12.0,
        beat_error_ms: 0.5,
        // 30 amplitude windows and more for the amplitude's ± (#1470).
        duration_s: 70.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 270.0, |_| 0.0);
    let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
    feed(&mut a, &audio.samples);
    let r = a.reading(10.0);
    assert_eq!(r.bph, Some(28800));
    let rate = r.rate_s_per_day.expect("rate");
    assert!((rate - 12.0).abs() < 1.0, "rate {rate}");
    let be = r.beat_error_ms.expect("beat error");
    assert!((be.abs() - 0.5).abs() < 0.05, "beat error {be}");
    let amp = r.amplitude_deg.expect("amplitude");
    assert!((amp - 270.0).abs() < 10.0, "amplitude {amp}");
    // A steady synthetic amplitude scatters little between windows.
    let pm = r.amplitude_error_deg.expect("amplitude ±");
    assert!(pm < 1.0, "amplitude ± {pm}");
    // Both sides' sounds, with the marks behind the amplitude, over the
    // default 3 s (12 beats a side at 28,800 bph).
    for p in a.tick_profiles() {
        let p = p.as_ref().expect("profile");
        assert!((10..=13).contains(&p.beats), "{} beats", p.beats);
        let pa = p.amplitude_deg.expect("profile amplitude");
        assert!((pa - 270.0).abs() < 15.0, "profile amplitude {pa}");
    }
    // About 8 beats a second from the first pass on, none counted twice.
    let n = a.beats().len();
    assert!((520..=570).contains(&n), "{n} beats");
    assert!(a.beats().windows(2).all(|w| w[1].index > w[0].index));
}

#[test]
fn agrees_with_the_whole_recording_fit() {
    let cfg = SynthConfig {
        rate_s_per_day: -20.0,
        duration_s: 20.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 250.0, |_| 0.0);
    let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
    feed(&mut a, &audio.samples);
    let whole = crate::analyze(&audio, &AnalysisConfig::default());
    // The same beats, numbered the same way. The two templates come
    // from different stretches of audio, so their reference points may
    // differ by a constant; beat to beat the times must agree.
    let live = a.beats();
    let mut shift = None;
    let mut dt = Vec::new();
    for b in live {
        if let Some(w) = whole.beats.iter().find(|w| (w.time - b.time).abs() < 0.01) {
            let s = *shift.get_or_insert(w.index - b.index);
            assert_eq!(w.index - b.index, s, "beat numbering slipped");
            dt.push(w.time - b.time);
        }
    }
    assert!(dt.len() as f64 > 0.95 * live.len() as f64);
    let mean = dt.iter().sum::<f64>() / dt.len() as f64;
    let rms = (dt.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / dt.len() as f64).sqrt();
    assert!(
        mean.abs() < 100e-6 && rms < 10e-6,
        "offset {mean}, rms {rms}"
    );
}

#[test]
fn a_trend_is_read_at_core_times_and_only_whole_readings_have_figures() {
    let t = trend_times(60.0);
    assert_eq!((t.len(), t[0], *t.last().unwrap()), (120, 0.5, 60.0));
    let long = trend_times(8.0 * 3600.0);
    assert!((TREND_POINTS as usize - 1..=TREND_POINTS as usize).contains(&long.len()));
    assert!((long[1] - long[0] - 7.2).abs() < 1e-9);

    let cfg = SynthConfig {
        duration_s: 40.0,
        rate_s_per_day: 6.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 270.0, |_| 0.0);
    let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
    feed(&mut a, &audio.samples);
    let trend = a.trend(10.0);
    assert_eq!(trend.len(), trend_times(a.duration_s()).len());
    for p in &trend {
        let r = a.reading_at(p.t, 10.0);
        assert_eq!(*p, TrendReading::of(p.t, &r));
        assert_eq!(
            p.rate_s_per_day.is_some(),
            r.whole && r.rate_s_per_day.is_some()
        );
    }
    // The last reads the watch's rate.
    let last = trend.last().unwrap();
    assert!(
        last.heard && (last.rate_s_per_day.unwrap() - 6.0).abs() < 0.5,
        "{last:?}"
    );
}

#[test]
fn silence_gives_no_readings_and_lift_rescales() {
    let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
    let mut rng = crate::synth::Rng::new(3);
    let noise: Vec<f32> = (0..48000 * 8).map(|_| 0.01 * rng.normal() as f32).collect();
    feed(&mut a, &noise);
    let r = a.reading(10.0);
    assert!(r.rate_s_per_day.is_none());
    assert!(a.beats().is_empty());

    let audio = generate(&SynthConfig::default(), |_| 260.0, |_| 0.0);
    feed(&mut a, &audio.samples);
    let before = a.reading(10.0).amplitude_deg.expect("amplitude");
    a.set_lift(26.0);
    let after = a.reading(10.0).amplitude_deg.expect("amplitude");
    assert!((after - before / 2.0).abs() < 1e-9);
}

/// Over the same stretch of the same beats, a live reading and a saved
/// one (`session`) read the same figures: the same beats, the same
/// amplitude windows (none reaching back before the stretch) and the
/// same medians (#632).
#[test]
fn live_and_saved_read_one_stretch_alike() {
    let cfg = SynthConfig {
        duration_s: 30.0,
        beat_error_ms: 0.5,
        rate_s_per_day: 8.0,
        ..Default::default()
    };
    // An amplitude that climbs, so a window more or less moves the
    // median.
    let audio = generate(&cfg, |t| 240.0 + 2.0 * t, |_| 0.0);
    let mut a = LiveAnalyzer::new(48000, LiveConfig::default());
    feed(&mut a, &audio.samples);
    let log = BeatLog {
        sample_rate: 48000,
        duration_s: a.duration_s(),
        bph: a.bph().unwrap(),
        lift_deg: a.config().analysis.amplitude.lift_deg,
        beats: a.beats().to_vec(),
        amplitude_windows: a.amp.clone(),
        clipped_beats: a.clipped_beats().to_vec(),
        clipped_samples: 0,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    let end = a.settled_to;
    // A stretch that starts partway through an amplitude window.
    for average_s in [7.3, 9.1] {
        let live = a.reading_at(end, average_s);
        let saved = crate::session::measure_masked(&log, None, &[], &[], end - average_s, end);
        assert!(live.amplitude_deg.is_some(), "{live:?}");
        assert_eq!(live.amplitude_deg, saved.amplitude_deg, "{average_s}");
        assert_eq!(live.amplitude_even_deg, saved.amplitude_even_deg);
        assert_eq!(live.amplitude_odd_deg, saved.amplitude_odd_deg);
        assert_eq!(live.beat_error_unlock_ms, saved.beat_error_unlock_ms);
        assert_eq!(live.beat_error_ms, saved.beat_error_ms);
    }
}

/// A reading takes its amplitude windows by halving (#978), and takes
/// the very ones it took by looking at every window: across a mask, at
/// every averaging time, and from a log whose windows come out of
/// order.
#[test]
fn a_reading_finds_its_windows_by_halving() {
    let cfg = SynthConfig {
        duration_s: 60.0,
        rate_s_per_day: 8.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |t| 240.0 + 2.0 * t, |_| 0.0);
    let mut heard = LiveAnalyzer::new(48000, LiveConfig::default());
    feed(&mut heard, &audio.samples);
    let mut log = BeatLog {
        sample_rate: 48000,
        duration_s: heard.duration_s(),
        bph: heard.bph().unwrap(),
        lift_deg: heard.config().analysis.amplitude.lift_deg,
        beats: heard.beats().to_vec(),
        amplitude_windows: heard.amp.clone(),
        clipped_beats: heard.clipped_beats().to_vec(),
        clipped_samples: 0,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    assert!(log.amplitude_windows.len() > 20);
    let in_order = LiveAnalyzer::from_log(log.clone(), LiveConfig::default());
    log.amplitude_windows.reverse();
    let mut a = LiveAnalyzer::from_log(log, LiveConfig::default());
    a.set_masked(&[(31.3, 36.0)]);
    let mids: Vec<f64> = a.amp.iter().map(amp_mid).collect();
    assert!(mids.windows(2).all(|m| m[0] <= m[1]), "in order: {mids:?}");
    assert_eq!(
        format!("{:?}", a.amp),
        format!("{:?}", in_order.amp),
        "as the log in order"
    );
    let mut taken = 0;
    for end in [20.0, 31.0, 40.0, 45.5, 50.0, a.settled_to] {
        for average_s in [2.0, 7.3, 10.0, 30.0] {
            let st = a.stretch_at(end - average_s, end);
            let every = crate::session::stretch_windows(&st.holes, &a.amp, st.from, end);
            let halved = crate::session::stretch_windows(
                &st.holes,
                a.amp_within(st.from, end),
                st.from,
                end,
            );
            let at = |w: &[&AmplitudeWindow]| w.iter().map(|w| w.start_s).collect::<Vec<_>>();
            assert_eq!(at(&halved), at(&every), "{end} {average_s}");
            taken += every.len();
        }
    }
    assert!(taken > 50, "{taken} windows taken");
}

/// A profile's guide for each side is the unlock of the latest window
/// to end by then that read that side, or of the first window after
/// when none has ended yet.
#[test]
fn profile_guides_come_from_the_latest_window_that_read_them() {
    let w = |start_s: f64, even: Option<f64>, odd: Option<f64>| AmplitudeWindow {
        start_s,
        end_s: start_s + 10.0,
        even_deg: None,
        odd_deg: None,
        beat_error_ms: None,
        beat_error_unlock_ms: None,
        even_unlock_ms: even,
        odd_unlock_ms: odd,
        even_drop_ms: None,
        odd_drop_ms: None,
        culled_ms: None,
        faint_withheld: [false; 2],
    };
    let mut a = LiveAnalyzer::new(8000, LiveConfig::default());
    a.amp = vec![
        w(0.0, Some(1.0), Some(2.0)),
        w(10.0, Some(3.0), None),
        w(20.0, Some(5.0), Some(6.0)),
    ];
    // Before any window has ended: the first after.
    assert_eq!(a.profile_guides_at(5.0), [Some(1.0), Some(2.0)]);
    // A window ending at the time counts; a side the latest did not
    // read takes the one before.
    assert_eq!(a.profile_guides_at(20.0), [Some(3.0), Some(2.0)]);
    assert_eq!(a.profile_guides_at(25.0), [Some(3.0), Some(2.0)]);
    assert_eq!(a.profile_guides_at(100.0), [Some(5.0), Some(6.0)]);
}
