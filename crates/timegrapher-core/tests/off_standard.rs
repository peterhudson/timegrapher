//! Auto far from every standard rate (Norm on #2223, point 5, and on
//! #2316): each lock on Auto is judged once, on its first
//! `beats::GUESS_SPAN_S` of beats. Further than
//! `beats::STANDARD_REACH_S_PER_DAY` from every standard rate, its
//! readings give no rate in s/d, the beat rate heard, and the caveat
//! `off_standard_rate` with the standard rate on each side. Within reach
//! of a standard rate that is not the one guessed, the lock takes that
//! one. `analyze`, live and a saved take's reading make the one check.

use serde_json::Value;
use timegrapher_core::analysis::{analyze, AnalysisConfig, Summary};
use timegrapher_core::audio::Audio;
use timegrapher_core::live::{LiveAnalyzer, LiveConfig, LiveReading};
use timegrapher_core::measure::{off_standard, Caveat};
use timegrapher_core::synth::{generate, SynthConfig};
use timegrapher_core::timing::OffStandard;

/// A synthetic balance beating `bph` at 280° for `secs`, gaining `rate`
/// s/d on it.
fn watch(bph: u32, rate: f64, secs: f64) -> Audio {
    let cfg = SynthConfig {
        bph,
        rate_s_per_day: rate,
        duration_s: secs,
        ..Default::default()
    };
    generate(&cfg, |_| 280.0, |_| 0.0)
}

/// The synth's balance heard: `bph` gaining `rate` s/d, as bph.
fn heard(bph: u32, rate: f64) -> f64 {
    f64::from(bph) * (1.0 + rate / 86_400.0)
}

fn set(bph: Option<u32>) -> AnalysisConfig {
    AnalysisConfig {
        bph,
        ..Default::default()
    }
}

fn live(audio: &Audio, bph: Option<u32>) -> LiveAnalyzer {
    let mut l = LiveAnalyzer::new(audio.sample_rate, live_cfg(bph));
    for c in audio.samples.chunks(4800) {
        l.push(c);
    }
    l
}

/// `audio` with `gap_s` from 15 s replaced by silence, or by noise at
/// 1.5 times the sound's RMS there, as `tests/fitted_period.rs` builds it
/// (#2229, #2253).
fn with_gap(mut audio: Audio, gap_s: f64, noise: bool) -> Audio {
    let fs = audio.sample_rate as f64;
    let span = (15.0 * fs) as usize..((15.0 + gap_s) * fs) as usize;
    let n = span.len() as f32;
    let rms = (audio.samples[span.clone()]
        .iter()
        .map(|v| v * v)
        .sum::<f32>()
        / n)
        .sqrt();
    let mut x: u64 = 12_345;
    for s in &mut audio.samples[span] {
        x = x
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let u = (x >> 33) as f32 / (1u64 << 31) as f32 - 0.5;
        *s = if noise { 1.5 * rms * u } else { 0.0 };
    }
    audio
}

/// The rate a reading off every standard rate reads against `bph`, s/d,
/// from its caveat's side.
fn side_rate(o: &OffStandard, bph: u32) -> f64 {
    [o.below, o.above]
        .into_iter()
        .flatten()
        .find(|s| s.bph == bph)
        .unwrap_or_else(|| panic!("no {bph} side in {o:?}"))
        .rate_s_per_day
}

/// No rate in s/d anywhere in a summary off every standard rate.
fn no_rate(s: &Summary) {
    assert_eq!(s.rate_s_per_day, None);
    assert_eq!(s.rate_corrected_s_per_day, None);
    assert_eq!((s.rate_p05, s.rate_p95), (None, None));
    assert_eq!(s.confidence.as_ref().and_then(|c| c.rate.se), None);
}

/// No rate in s/d in a live reading off every standard rate.
fn no_live_rate(r: &LiveReading) {
    assert_eq!(r.rate_s_per_day, None);
    assert_eq!(r.rate_error_s_per_day, None);
}

/// A balance at 17,570 on Auto reads no rate, the beat rate heard, and
/// the standard rate on each side with the rate it reads against each:
/// against 18,000 the rate a set 18,000 reads. Its amplitude and beat
/// error stay.
#[test]
fn a_watch_at_17570_on_auto_has_no_rate_and_the_rate_heard() {
    let audio = watch(17_570, 5.0, 60.0);
    let at_18000 = analyze(&audio, &set(Some(18_000))).summary;
    assert!(off_standard(&at_18000.caveats).is_none(), "a set rate");
    let set_rate = at_18000.rate_s_per_day.expect("a set rate's rate");
    let s = analyze(&audio, &set(None)).summary;
    let o = off_standard(&s.caveats).expect("off_standard_rate");
    no_rate(&s);
    let want = heard(17_570, 5.0);
    assert!((o.bph_measured - want).abs() < 0.05, "{o:?} for {want}");
    let (below, above) = (o.below.expect("below"), o.above.expect("above"));
    assert_eq!((below.bph, above.bph), (17_280, 18_000));
    assert!(
        (side_rate(&o, 18_000) - set_rate).abs() < 0.05,
        "{o:?}, {set_rate}"
    );
    let against_17280 = (want / 17_280.0 - 1.0) * 86_400.0;
    assert!(
        (below.rate_s_per_day - against_17280).abs() < 0.2,
        "{below:?}"
    );
    // A minute holds too few 10 s windows for a rate ±, so none here.
    assert_eq!(o.bph_measured_se, None);
    assert!(s.amplitude_deg.is_some_and(|a| (a - 280.0).abs() < 1.0));
    assert!(s.beat_error_unlock_ms.is_some());
}

/// A watch within the reach (+900 s/d off 18,000, and −900) reads its
/// rate against 18,000 with no caveat; one past it (+1,100) has none.
#[test]
fn the_reach_is_1000_s_per_day_after_the_fit() {
    for rate in [900.0, -900.0] {
        let s = analyze(&watch(18_000, rate, 40.0), &set(None)).summary;
        assert_eq!(s.bph, 18_000, "{rate}");
        assert!(
            off_standard(&s.caveats).is_none(),
            "{rate}: {:?}",
            s.caveats
        );
        let read = s.rate_s_per_day.expect("a rate");
        assert!((read - rate).abs() < 0.5, "{read} for {rate}");
    }
    let s = analyze(&watch(18_000, 1_100.0, 40.0), &set(None)).summary;
    let o = off_standard(&s.caveats).expect("past the reach");
    no_rate(&s);
    assert!((side_rate(&o, 18_000) - 1_100.0).abs() < 0.5, "{o:?}");
}

/// Live on Auto across a 6 s silence or a 20 s gap of noise (#2253):
/// before, live re-guessed 17,280 after the silence and read +1455.1 s/d
/// where `analyze` read −2059.1 against 18,000. Now both read no rate and
/// the beat rate heard, and their sides' rates agree within 0.05 s/d,
/// over the whole take and over the last 10 s; with no gap too.
#[test]
fn live_on_auto_across_a_gap_agrees_with_analyze() {
    for (secs, gap) in [
        (40.0, None),
        (40.0, Some((6.0, false))),
        (70.0, Some((20.0, true))),
    ] {
        let audio = watch(17_570, 5.0, secs);
        let audio = gap.map_or(audio.clone(), |(g, noise)| with_gap(audio, g, noise));
        let s = analyze(&audio, &set(None)).summary;
        let saved = off_standard(&s.caveats).expect("saved off_standard_rate");
        let l = live(&audio, None);
        for average_s in [secs, 10.0] {
            let at = format!("gap {gap:?}, over {average_s} s");
            let r = l.reading(average_s);
            let o = off_standard(&r.caveats).unwrap_or_else(|| panic!("{at}: {:?}", r.caveats));
            no_live_rate(&r);
            let m = r.measured().expect("the beat rate heard");
            assert_eq!(m.bph, o.bph_measured, "{at}");
            for side in [17_280, 18_000] {
                let (heard, read) = (side_rate(&o, side), side_rate(&saved, side));
                assert!(
                    (heard - read).abs() < 0.05,
                    "{at}: {heard} for {read} at {side}"
                );
            }
        }
    }
}

/// A set rate is the target however far the watch is from it: live and
/// saved, 17,570 set to 18,000 reads its rate and no caveat.
#[test]
fn a_set_rate_is_never_off_standard() {
    let audio = watch(17_570, 5.0, 30.0);
    let r = live(&audio, Some(18_000)).reading(20.0);
    assert!(off_standard(&r.caveats).is_none(), "{:?}", r.caveats);
    let rate = r.rate_s_per_day.expect("a rate");
    assert!((rate + 2_059.1).abs() < 0.1, "{rate}");
}

/// The caveat in words: the beat rate heard, the reach, and each side.
#[test]
fn the_caveat_in_words() {
    let o = OffStandard::reading(18_000, Some(-2_059.11), None).unwrap();
    let c = Caveat::OffStandardRate(o);
    assert_eq!(c.short(52.0, 0).as_deref(), Some("not a standard rate"));
    assert_eq!(
        c.long(None).as_deref(),
        Some(
            "No rate in s/d: over the first 10 s after Auto locked on to the beat, the \
             watch was more than 1000 s/d from every standard rate. This reading: 17571 bph \
             heard, +1455.1 s/d against 17280 bph, -2059.1 s/d against 18000 bph."
        )
    );
    assert_eq!(
        c.reading_scope(),
        Some(timegrapher_core::measure::Scope::Rate)
    );
    // Read back from its JSON, as the MCP server reads caveats.
    let json = serde_json::to_value(c).unwrap();
    assert_eq!(json["code"], "off_standard_rate");
    assert_eq!(serde_json::from_value::<Caveat>(json).unwrap(), c);
}

/// A reading in a lock judged off whose own rate is back within reach of
/// a side (Hugh on #2316): the words stay true, since the verdict is the
/// lock's and the figures are the reading's.
#[test]
fn the_words_hold_when_a_reading_is_back_in_reach() {
    let o = OffStandard::reading(18_000, Some(-990.0), None).unwrap();
    assert_eq!(
        Caveat::OffStandardRate(o).long(None).as_deref(),
        Some(
            "No rate in s/d: over the first 10 s after Auto locked on to the beat, the \
             watch was more than 1000 s/d from every standard rate. This reading: 17794 bph \
             heard, +2568.7 s/d against 17280 bph, -990.0 s/d against 18000 bph."
        )
    );
}

/// The JSON of `analyze` off every standard rate: no rate anywhere, the
/// caveat with the beat rate heard and both sides, and `bph_measured`
/// equal to the caveat's, with and without a clock correction.
#[test]
fn the_analyze_document_off_standard() {
    let s = analyze(&watch(17_570, 5.0, 40.0), &set(None)).summary;
    for clock in [None, Some((20.0, "a stored correction"))] {
        let doc = timegrapher_core::analyze_doc::body(&s, clock).unwrap();
        assert!(doc["rate_s_per_day"].is_null(), "{clock:?}");
        assert!(doc["overall"]["rate_s_per_day"].is_null(), "{clock:?}");
        assert!(doc["clock"]["rate_s_per_day"].is_null(), "{clock:?}");
        let c = doc["caveats"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["code"] == "off_standard_rate")
            .unwrap_or_else(|| panic!("{clock:?}: {}", doc["caveats"]));
        assert_eq!(c["bph_measured"], doc["bph_measured"], "{clock:?}");
        assert_eq!(c["bph_measured_se"], doc["bph_measured_se"], "{clock:?}");
        assert_eq!(c["below"]["bph"], 17_280);
        assert_eq!(c["above"]["bph"], 18_000);
        // The beat rate heard on the clock the document corrects to.
        let card = side_rate(&off_standard(&s.caveats).unwrap(), 18_000);
        let rate = clock.map_or(card, |(ppm, _)| {
            timegrapher_core::clockstore::correct_rate(card, ppm)
        });
        let want = 18_000.0 * (1.0 + rate / 86_400.0);
        let got = doc["bph_measured"].as_f64().unwrap();
        assert!((got - want).abs() < 0.01, "{clock:?}: {got} for {want}");
    }
}

/// Each side is the nearest standard rate, none past the slowest or the
/// fastest; within the reach of any standard rate, or with a rate set,
/// there is no caveat.
#[test]
fn the_sides_and_the_reach() {
    let o = OffStandard::of(None, 12_000, Some(-8_640.0)).unwrap();
    assert_eq!(o.below, None);
    assert_eq!(o.above.map(|s| s.bph), Some(12_000));
    let o = OffStandard::of(None, 72_000, Some(2_000.0)).unwrap();
    assert_eq!((o.below.map(|s| s.bph), o.above), (Some(72_000), None));
    assert!(OffStandard::of(None, 18_000, Some(1_000.0)).is_none());
    assert!(OffStandard::of(None, 18_000, Some(-1_000.0)).is_none());
    assert!(OffStandard::of(None, 18_000, Some(1_000.01)).is_some());
    assert!(OffStandard::of(Some(18_000), 18_000, Some(-2_059.0)).is_none());
    assert!(OffStandard::of(None, 18_000, None).is_none());
    // In reach of 17,280, though guessed as 18,000: no caveat, and the
    // lock takes 17,280.
    let r = (heard(17_280, 990.0) / 18_000.0 - 1.0) * 86_400.0;
    assert!(OffStandard::of(None, 18_000, Some(r)).is_none());
    assert_eq!(OffStandard::nearest_in_reach(18_000, r), Some(17_280));
    assert_eq!(OffStandard::nearest_in_reach(18_000, 900.0), Some(18_000));
    assert_eq!(OffStandard::nearest_in_reach(18_000, -2_059.0), None);
    // The rate's ± goes into the beat rate heard's, `bph × σ / 86,400`,
    // never shown as s/d.
    let o = OffStandard::reading(18_000, Some(-2_059.0), Some(4.8)).unwrap();
    assert!((o.bph_measured_se.unwrap() - 1.0).abs() < 1e-12, "{o:?}");
    assert_eq!(timegrapher_core::beats::STANDARD_REACH_S_PER_DAY, 1_000.0);
}

/// Norm's sweep (on #2316, from Hugh's question): over every gap
/// between standard rates and past 12,000 and 72,000, read against
/// every standard rate as the guess, whenever the caveat is made every
/// side it names is further than 1000 s/d off, and whenever it is not,
/// one side is within 1000 s/d. The same holds as `debug_assert!`s in
/// `OffStandard::of`.
#[test]
fn every_gap_off_standard_only_with_no_side_in_reach() {
    use timegrapher_core::beats::STANDARD_BPH;
    let reach = |r: f64| r.abs() <= 1_000.0;
    let (mut off, mut on) = (0, 0);
    // From 10,000 to 75,000 bph heard, in steps of 0.25 bph, and each
    // reach's edges exactly.
    let edges = STANDARD_BPH.iter().flat_map(|&b| {
        let b = f64::from(b);
        [-1_000.0, 1_000.0].map(|r| b * (1.0 + r / 86_400.0))
    });
    let heards = (0..260_000)
        .map(|i| 10_000.0 + 0.25 * f64::from(i))
        .chain(edges);
    for h in heards {
        for &guess in &STANDARD_BPH {
            let rate = (h / f64::from(guess) - 1.0) * 86_400.0;
            let sides = OffStandard::reading(guess, Some(rate), None).unwrap();
            let sides: Vec<f64> = [sides.below, sides.above]
                .into_iter()
                .flatten()
                .map(|s| s.rate_s_per_day)
                .collect();
            match OffStandard::of(None, guess, Some(rate)) {
                Some(o) => {
                    off += 1;
                    for s in [o.below, o.above].into_iter().flatten() {
                        assert!(!reach(s.rate_s_per_day), "{h} bph on {guess}: {o:?}");
                    }
                }
                None => {
                    on += 1;
                    assert!(
                        sides.iter().any(|&r| reach(r)),
                        "{h} bph on {guess}: {sides:?}"
                    );
                }
            }
        }
    }
    assert!(off > 0 && on > 0, "{off} off, {on} in reach");
}

/// A lock on Auto takes the standard rate nearest the beat rate heard
/// when that is in reach and is not the rate guessed (Norm on #2316): a
/// balance at 17,280 gaining 990 s/d, which Auto guesses as 18,000, reads
/// 17,280 and its rate, as the same sound set to 17,280 reads it within
/// 0.05 s/d: in `analyze`, a take's reading, and live at every
/// averaging time from its first reading. There is no caveat.
#[test]
fn a_lock_takes_the_nearest_standard_in_reach() {
    let audio = watch(17_280, 990.0, 60.0);
    let set_17280 = analyze(&audio, &set(Some(17_280))).summary;
    let want = set_17280.rate_s_per_day.expect("a set rate's rate");
    let s = analyze(&audio, &set(None)).summary;
    assert_eq!(s.bph, 17_280);
    assert!(off_standard(&s.caveats).is_none(), "{:?}", s.caveats);
    let got = s.rate_s_per_day.expect("a rate");
    assert!((got - want).abs() < 0.05, "analyze {got} for {want}");
    // A take's reading (`take::measure_opened`), on Auto and set.
    let (log, read) = opened(&audio, None);
    assert_eq!(log.bph, 17_280);
    assert!(off_standard(&read.caveats).is_none(), "{:?}", read.caveats);
    let (_, set_read) = opened(&audio, Some(17_280));
    let (got, want) = (
        read.rate_s_per_day.unwrap(),
        set_read.rate_s_per_day.unwrap(),
    );
    assert!((got - want).abs() < 0.05, "take {got} for {want}");
    // Live, each second, at every averaging time.
    let auto = LiveAnalyzer::new(audio.sample_rate, live_cfg(None));
    let set_live = LiveAnalyzer::new(audio.sample_rate, live_cfg(Some(17_280)));
    let mut first = None;
    each_second(&audio, [auto, set_live], |t, [a, b]| {
        for average_s in timegrapher_core::live::AVERAGES {
            let (r, w) = (a.reading(average_s), b.reading(average_s));
            let Some(want) = w.rate_s_per_day else {
                continue;
            };
            first.get_or_insert(t);
            let at = format!("at {t} s over {average_s} s");
            assert_eq!(r.bph, Some(17_280), "{at}");
            assert!(off_standard(&r.caveats).is_none(), "{at}: {:?}", r.caveats);
            let got = r.rate_s_per_day.unwrap_or_else(|| panic!("{at}: no rate"));
            assert!((got - want).abs() < 0.05, "{at}: {got} for {want}");
        }
    });
    // The first figures once 10 s is heard (Norm on #2333; from 5 s
    // before, on the beats' sound). Sampled each whole second of sound,
    // a live reading ends a little before the sound it has (its read
    // lag), so at 10 s of sound it has not quite 10 s heard: the first
    // whole second with figures is the next.
    let ten = timegrapher_core::measure::SHORT_SPAN_S as u32;
    assert!(
        first.is_some_and(|t| t <= ten + 1),
        "first reading at {first:?} s"
    );
}

/// A 28,800 watch running down from -200 to -2200 s/d over a minute is
/// within reach on its first 10 s, so every reading keeps its rate in
/// s/d, the last ones far past 1000 s/d off (#1968's dying watch), in
/// `analyze` and live at every averaging time.
#[test]
fn a_watch_running_down_keeps_its_rate() {
    let cfg = SynthConfig {
        bph: 28_800,
        rate_s_per_day: -200.0,
        duration_s: 60.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |t| -2_000.0 * t / 60.0);
    let s = analyze(&audio, &set(None)).summary;
    assert_eq!(s.bph, 28_800);
    assert!(off_standard(&s.caveats).is_none(), "{:?}", s.caveats);
    assert!(s.rate_s_per_day.is_some());
    let l = live(&audio, None);
    for average_s in timegrapher_core::live::AVERAGES {
        let r = l.reading(average_s);
        assert!(
            off_standard(&r.caveats).is_none(),
            "{average_s}: {:?}",
            r.caveats
        );
        assert!(r.rate_s_per_day.is_some(), "{average_s}");
    }
    let last = l.reading(2.0).rate_s_per_day.unwrap();
    assert!(last < -1_500.0, "{last}");
}

/// A balance at 17,570 on Auto is off every standard rate from its first
/// reading, live at every averaging time: no reading gives a rate in s/d.
#[test]
fn off_standard_from_the_first_reading() {
    let audio = watch(17_570, 5.0, 30.0);
    let l = LiveAnalyzer::new(audio.sample_rate, live_cfg(None));
    let mut read = 0;
    each_second(&audio, [l], |t, [l]| {
        for average_s in timegrapher_core::live::AVERAGES {
            let r = l.reading(average_s);
            // A reading still waiting, under 10 s heard, has no figures
            // and no verdict (Norm on #2333).
            if r.bph.is_none() || !r.watch_heard || r.finding_beat || r.listening.is_some() {
                continue;
            }
            read += 1;
            let at = format!("at {t} s over {average_s} s");
            assert!(off_standard(&r.caveats).is_some(), "{at}: {:?}", r.caveats);
            assert_eq!(r.rate_s_per_day, None, "{at}");
        }
    });
    assert!(read > 0);
}

/// One verdict per lock, near the line: an 18,000 watch whose rate swings
/// ±30 s/d about -985 (in reach on its first 10 s) gives a rate in every
/// reading, and one about -1015 (out of reach) gives none, at 2 s and at
/// 60 s alike, though their 2 s readings cross the line. Each swing
/// starts away from the line, so the lock's first seconds, judged on the
/// beats they have, side with its first 10 s.
#[test]
fn one_verdict_per_lock_near_the_line() {
    for (centre, off, away) in [(-985.0, false, 1.0), (-1_015.0, true, -1.0)] {
        let cfg = SynthConfig {
            bph: 18_000,
            rate_s_per_day: centre,
            duration_s: 60.0,
            ..Default::default()
        };
        // A whole swing in 10 s, so the first 10 s average its centre.
        let swing = |t: f64| away * 30.0 * (std::f64::consts::TAU * t / 10.0).sin();
        let audio = generate(&cfg, |_| 280.0, swing);
        let s = analyze(&audio, &set(None)).summary;
        assert_eq!(off_standard(&s.caveats).is_some(), off, "{centre}");
        let l = LiveAnalyzer::new(audio.sample_rate, live_cfg(None));
        each_second(&audio, [l], |t, [l]| {
            for average_s in [2.0, 60.0] {
                let r = l.reading(average_s);
                // Still waiting, under 10 s heard: no figures and no
                // verdict (Norm on #2333).
                if r.bph.is_none() || !r.watch_heard || r.finding_beat || r.listening.is_some() {
                    continue;
                }
                let at = format!("{centre} at {t} s over {average_s} s");
                assert_eq!(off_standard(&r.caveats).is_some(), off, "{at}");
                assert_eq!(r.rate_s_per_day.is_none(), off, "{at}");
            }
        });
    }
}

/// The edges of the gap between 17,280 and 18,000: 18,000 at -990 s/d
/// is in reach of 18,000 and 17,280 at +1010 is in reach of neither, in
/// `analyze` and live.
#[test]
fn the_gap_edges() {
    for (bph, rate, off) in [(18_000, -990.0, false), (17_280, 1_010.0, true)] {
        let audio = watch(bph, rate, 40.0);
        let s = analyze(&audio, &set(None)).summary;
        let r = live(&audio, None).reading(20.0);
        for (caveats, read, how) in [
            (&s.caveats, s.rate_s_per_day, "analyze"),
            (&r.caveats, r.rate_s_per_day, "live"),
        ] {
            assert_eq!(off_standard(caveats).is_some(), off, "{bph} {rate} {how}");
            assert_eq!(read.is_none(), off, "{bph} {rate} {how}");
        }
        if !off {
            assert_eq!((s.bph, r.bph), (bph, Some(bph)));
            assert!((s.rate_s_per_day.unwrap() - rate).abs() < 0.5);
        }
    }
}

/// The verdict is made on the card's clock, and a document's correction
/// never makes it again (Trevor and Norm on #2316): 18,000 at +1001.5 s/d
/// with the card 20 ppm slow keeps the caveat, its figures corrected,
/// and +999.0 with the card 20 ppm fast keeps its rate.
#[test]
fn a_clock_correction_reads_the_figures_but_never_the_verdict() {
    let s = analyze(&watch(18_000, 1_001.5, 40.0), &set(None)).summary;
    let card = side_rate(&off_standard(&s.caveats).expect("off"), 18_000);
    let doc = timegrapher_core::analyze_doc::body(&s, Some((-20.0, "given"))).unwrap();
    let c = (doc["caveats"].as_array().unwrap().iter())
        .find(|c| c["code"] == "off_standard_rate")
        .expect("the caveat stays");
    let corrected = timegrapher_core::clockstore::correct_rate(card, -20.0);
    assert!(
        corrected.abs() < 1_000.0,
        "{corrected}: a correction that crosses the line"
    );
    assert_eq!(c["below"]["bph"], 18_000, "{c}");
    let got = c["below"]["rate_s_per_day"].as_f64().unwrap();
    assert!((got - corrected).abs() < 1e-6, "{c}");
    assert!(doc["rate_s_per_day"].is_null());

    let s = analyze(&watch(18_000, 999.0, 40.0), &set(None)).summary;
    assert!(off_standard(&s.caveats).is_none());
    let doc = timegrapher_core::analyze_doc::body(&s, Some((20.0, "given"))).unwrap();
    let corrected = doc["rate_corrected_s_per_day"]
        .as_f64()
        .or(doc["clock"]["rate_s_per_day"].as_f64());
    assert!(doc["rate_s_per_day"].is_number(), "{doc}");
    assert!(
        corrected.is_some_and(|r| r.abs() > 1_000.0),
        "{corrected:?}"
    );
    let codes: Vec<&Value> = doc["caveats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| &c["code"])
        .collect();
    assert!(
        !codes.iter().any(|c| *c == "off_standard_rate"),
        "{codes:?}"
    );
}

/// A new lock is judged afresh: 17,570 for 15 s, a 6 s silence, then
/// 28,800 for 30 s reads 28,800 and its rate over the last 10 s; the
/// reverse reads the caveat and no rate.
#[test]
fn a_lock_found_again_is_judged_again() {
    let gap = |a: &Audio, b: &Audio| {
        let mut samples = a.samples.clone();
        samples.extend(std::iter::repeat_n(0.0, 6 * a.sample_rate as usize));
        samples.extend_from_slice(&b.samples);
        Audio {
            samples,
            sample_rate: a.sample_rate,
        }
    };
    let (slow, fast) = (watch(17_570, 5.0, 15.0), watch(28_800, 5.0, 30.0));
    let r = live(&gap(&slow, &fast), None).reading(10.0);
    assert_eq!(r.bph, Some(28_800));
    assert!(off_standard(&r.caveats).is_none(), "{:?}", r.caveats);
    assert!(r.rate_s_per_day.is_some_and(|x| (x - 5.0).abs() < 1.0));
    let (fast, slow) = (watch(28_800, 5.0, 15.0), watch(17_570, 5.0, 30.0));
    let r = live(&gap(&fast, &slow), None).reading(10.0);
    assert!(off_standard(&r.caveats).is_some(), "{:?}", r.caveats);
    assert_eq!(r.rate_s_per_day, None);
}

/// A saved take's reading on Auto makes the same check: its beat log's
/// lock (`analysis::lock_rate_of`) is judged as `analyze` judges it, in
/// `take::measure_opened`, which every `session` reading and `record`
/// check reads through. `session::Measurement::read_off_standard` leaves
/// out the rate, its spread and its states and gives the beat rate heard
/// as `analyze` reads it. Set to 18,000, there is no caveat.
#[test]
fn a_saved_reading_on_auto_is_off_standard_as_analyze_is() {
    let audio = watch(17_570, 5.0, 40.0);
    let saved = off_standard(&analyze(&audio, &set(None)).summary.caveats).unwrap();
    let log = log_of(&audio, None);
    let lock = timegrapher_core::analysis::lock_rate_of(&log);
    assert!(OffStandard::of(None, log.bph, lock).is_some());
    assert!(OffStandard::of(Some(18_000), log.bph, lock).is_none());
    let (_, set_18000) = opened(&audio, Some(18_000));
    assert!(off_standard(&set_18000.caveats).is_none());
    assert!(set_18000.rate_s_per_day.is_some());
    let (log, m) = opened(&audio, None);
    let o = off_standard(&m.caveats).expect("off_standard_rate");
    assert_eq!(m.rate_s_per_day, None);
    assert_eq!(m.rate_window_median_s_per_day, None);
    assert_eq!((m.rate_p05, m.rate_p95), (None, None));
    let none = timegrapher_core::twostate::find(&[], &Default::default());
    assert_eq!(
        serde_json::to_value(&m.rate_states).unwrap(),
        serde_json::to_value(&none).unwrap()
    );
    assert_eq!(m.measured(log.bph).map(|h| h.bph), Some(o.bph_measured));
    for side in [17_280, 18_000] {
        let (read, want) = (side_rate(&o, side), side_rate(&saved, side));
        assert!((read - want).abs() < 0.05, "{read} for {want} at {side}");
    }
}

/// `audio` written to a file and read as a take's one entry is read
/// (`take::open_files`, `take::measure_opened`), on Auto or set: its beat
/// log and its reading.
fn opened(
    audio: &Audio,
    bph: Option<u32>,
) -> (
    timegrapher_core::stream::BeatLog,
    timegrapher_core::session::Measurement,
) {
    use timegrapher_core::take;
    let dir = std::env::temp_dir().join(format!(
        "tg-off-standard-{}-{:?}-{}",
        std::process::id(),
        bph,
        audio.samples.len()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("take.wav");
    timegrapher_core::audio::write_wav(&wav, audio).unwrap();
    let s = take::EntrySettings {
        stream: timegrapher_core::stream::StreamConfig {
            analysis: set(bph),
            ..Default::default()
        },
        card_clock: true,
        settle_s: Some(0.0),
    };
    let (usb, card) = (|_: &[_]| Ok(None), |_: &std::path::Path| Ok(None));
    let o = take::open_files(std::slice::from_ref(&wav), None, &s, usb, card, |_, _| {}).unwrap();
    let e = timegrapher_core::session::RecordingEntry {
        file: wav.display().to_string(),
        ..Default::default()
    };
    let read = take::measure_opened(o, &Default::default(), &e, &s).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    (read.log, read.measurement)
}

/// The beat log of `audio` read as a file, on Auto or at a set rate.
fn log_of(audio: &Audio, bph: Option<u32>) -> timegrapher_core::stream::BeatLog {
    let cfg = timegrapher_core::stream::StreamConfig {
        analysis: set(bph),
        ..Default::default()
    };
    timegrapher_core::stream::analyze_samples(&audio.samples, audio.sample_rate, &cfg)
}

fn live_cfg(bph: Option<u32>) -> LiveConfig {
    LiveConfig {
        analysis: set(bph),
        ..Default::default()
    }
}

/// `audio` pushed in 0.1 s blocks to each analyzer in `ls`, with `at`
/// called after each whole second, with the seconds pushed.
fn each_second<const N: usize>(
    audio: &Audio,
    mut ls: [LiveAnalyzer; N],
    mut at: impl FnMut(u32, &[LiveAnalyzer; N]),
) {
    let block = audio.sample_rate as usize / 10;
    for (i, c) in audio.samples.chunks(block).enumerate() {
        for l in &mut ls {
            l.push(c);
        }
        if (i + 1) % 10 == 0 {
            at(((i + 1) / 10) as u32, &ls);
        }
    }
}

/// Live with its figures read as a file (Trevor on #2316): once what was
/// heard is read again as a file, 17,570 on Auto still has no rate, with
/// the caveat's sides equal to `analyze`'s within 0.05 s/d, and 17,280 at
/// +990 reads 17,280 and its rate, as `analyze` does, not "finding the
/// beat".
#[test]
fn read_as_a_file_on_auto_judges_as_analyze_does() {
    for (bph, rate) in [(17_570, 5.0), (17_280, 990.0)] {
        let audio = watch(bph, rate, 40.0);
        let s = analyze(&audio, &set(None)).summary;
        let mut l = LiveAnalyzer::new(audio.sample_rate, live_cfg(None)).with_figures_as_file();
        for c in audio.samples.chunks(4800) {
            l.push(c);
        }
        l.read_as_file_now();
        assert!(l.reads_as_file(), "{bph}");
        for average_s in [10.0, 30.0] {
            let at = format!("{bph} over {average_s} s");
            let r = l.reading(average_s);
            assert!(!r.finding_beat && r.watch_heard, "{at}: {r:?}");
            match off_standard(&s.caveats) {
                Some(saved) => {
                    let o =
                        off_standard(&r.caveats).unwrap_or_else(|| panic!("{at}: {:?}", r.caveats));
                    no_live_rate(&r);
                    for side in [17_280, 18_000] {
                        let (got, want) = (side_rate(&o, side), side_rate(&saved, side));
                        assert!(
                            (got - want).abs() < 0.05,
                            "{at}: {got} for {want} at {side}"
                        );
                    }
                }
                None => {
                    assert_eq!((s.bph, r.bph), (17_280, Some(17_280)), "{at}");
                    assert!(off_standard(&r.caveats).is_none(), "{at}: {:?}", r.caveats);
                    let got = r.rate_s_per_day.unwrap_or_else(|| panic!("{at}: no rate"));
                    assert!((got - rate).abs() < 0.5, "{at}: {got}");
                }
            }
        }
    }
}

/// A live reading at a take's end, read as the take's whole reading
/// (`LiveReading::at_whole_end`, Trevor on #2316): in a lock off every
/// standard rate it has no rate in s/d and keeps the caveat, though the
/// whole reading it takes its figures from has its rate.
#[test]
fn a_reading_at_the_whole_end_keeps_the_locks_verdict() {
    let audio = watch(17_570, 5.0, 40.0);
    let l = live(&audio, None);
    let r = l.reading(40.0);
    assert!(off_standard(&r.caveats).is_some());
    let log = log_of(&audio, None);
    let whole = timegrapher_core::session::measure(&log, None, 0.0, r.time_s);
    assert!(
        whole.rate_s_per_day.is_some(),
        "whole is read without the verdict"
    );
    let end = r.clone().at_whole_end(&whole, 40.0);
    assert!(end.whole_take, "the whole reading holds: {whole:?}");
    no_live_rate(&end);
    let o = off_standard(&end.caveats).unwrap_or_else(|| panic!("{:?}", end.caveats));
    let want = OffStandard::reading(log.bph, whole.rate_s_per_day, None).unwrap();
    assert!(
        (side_rate(&o, 18_000) - side_rate(&want, 18_000)).abs() < 1e-9,
        "{o:?}"
    );
}

/// A take with a lead of stand noise before the watch (Trevor on #2316):
/// its lock is judged on its first 10 s after the lead
/// (`analysis::lock_rate_of`), as `analyze` judges it. 17,570 read at
/// 18,000 after 8 s of noise: the lock's rate is the watch's, -2059.1
/// s/d, and the lock is off every standard rate, as in `analyze`.
#[test]
fn a_take_with_a_noise_lead_is_judged_after_the_lead() {
    let audio = noise_lead(watch(17_570, 5.0, 50.0), 8.0);
    let s = analyze(&audio, &set(None)).summary;
    let lead = |c: &[Caveat]| c.iter().any(|c| matches!(c, Caveat::LeadLeftOut { .. }));
    assert!(lead(&s.caveats), "{:?}", s.caveats);
    let saved = off_standard(&s.caveats).expect("analyze off");
    let log = log_of(&audio, Some(18_000));
    let lock = timegrapher_core::analysis::lock_rate_of(&log).expect("a lock rate");
    let want = side_rate(&saved, 18_000);
    assert!((lock - want).abs() < 0.5, "{lock} for {want}");
    assert!(OffStandard::of(None, log.bph, Some(lock)).is_some());
}

/// `audio` with its first `secs` replaced by noise, 60 times the RMS of
/// the watch's sound there: loud enough to be read as a lead of noise.
fn noise_lead(mut audio: Audio, secs: f64) -> Audio {
    let n = (secs * audio.sample_rate as f64) as usize;
    let rms = (audio.samples[..n].iter().map(|v| v * v).sum::<f32>() / n as f32).sqrt();
    let mut x: u64 = 987_654_321;
    for s in &mut audio.samples[..n] {
        x = x
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *s = 60.0 * rms * ((x >> 33) as f32 / (1u64 << 31) as f32 - 0.5);
    }
    audio
}

/// A whole reading judged off every standard rate already, as the app
/// reads its whole take (#2253, #2327): a live reading at its end keeps
/// that verdict, no rate in s/d and the caveat with the whole's figures,
/// though whole has no rate left to read again (Trevor's trap on #2316),
/// and though the live reading's own lock was not judged off: here it
/// is read at a set rate, so only the whole's verdict can give it.
#[test]
fn a_whole_read_off_already_keeps_its_verdict_at_the_end() {
    let audio = watch(17_570, 5.0, 40.0);
    let log = log_of(&audio, None);
    let l = live(&audio, Some(log.bph));
    let r = l.reading(40.0);
    assert!(off_standard(&r.caveats).is_none() && r.rate_s_per_day.is_some());
    let mut whole = timegrapher_core::session::measure(&log, None, 0.0, r.time_s);
    whole.judge_lock(None, &log);
    assert!(whole.rate_s_per_day.is_none(), "{:?}", whole.caveats);
    let want = off_standard(&whole.caveats).expect("whole read off");
    let end = r.at_whole_end(&whole, 40.0);
    assert!(end.whole_take, "the whole reading holds: {whole:?}");
    no_live_rate(&end);
    let o = off_standard(&end.caveats).unwrap_or_else(|| panic!("{:?}", end.caveats));
    assert_eq!(o, want);
}

/// One verdict for a take's beats (`analysis::lock_off_standard`): on
/// Auto, a log locked off every standard rate is off; at a set rate, or
/// a watch within reach of its standard, it is not.
#[test]
fn a_beat_logs_lock_is_judged_once() {
    use timegrapher_core::analysis::lock_off_standard;
    let off = log_of(&watch(17_570, 0.0, 30.0), None);
    assert!(lock_off_standard(None, &off));
    assert!(!lock_off_standard(Some(off.bph), &off));
    let near = log_of(&watch(18_000, 900.0, 30.0), None);
    assert!(!lock_off_standard(None, &near), "{} bph", near.bph);
}

/// An opened take's engine, built from its beat log
/// (`LiveAnalyzer::from_log`), holds no lock of its own: on Auto its
/// readings take the log's verdict, as the saved take's reading does, so
/// an opened 17,570 has no rate in s/d (#2327). At a set rate it reads
/// against that rate.
#[test]
fn an_engine_from_a_log_takes_the_logs_verdict() {
    let audio = watch(17_570, 0.0, 40.0);
    let l = LiveAnalyzer::from_log(log_of(&audio, None), live_cfg(None));
    for average_s in [10.0, 40.0] {
        let r = l.reading_at(l.duration_s(), average_s);
        assert!(r.watch_heard, "{r:?}");
        no_live_rate(&r);
        assert!(off_standard(&r.caveats).is_some(), "{:?}", r.caveats);
    }
    let set = LiveAnalyzer::from_log(log_of(&audio, Some(18_000)), live_cfg(Some(18_000)));
    let r = set.reading_at(set.duration_s(), 10.0);
    assert!(r.rate_s_per_day.is_some(), "{r:?}");
    assert!(off_standard(&r.caveats).is_none(), "{:?}", r.caveats);
    // Its rate then set to the one its beats are read at: they stand, and
    // the log's Auto verdict no longer holds.
    let mut l = l;
    let bph = l.bph().expect("a rate");
    l.set_bph(Some(bph));
    let r = l.reading_at(l.duration_s(), 10.0);
    assert!(r.watch_heard && r.rate_s_per_day.is_some(), "{r:?}");
    assert!(off_standard(&r.caveats).is_none(), "{:?}", r.caveats);
}

/// A take's Positions Test table in a lock off every standard rate
/// (`PositionsTable::judge_lock`): no position has a rate in s/d,
/// each names the caveat, and no value over the positions is in s/d;
/// amplitude stays, as `session` reads each position (#2253).
#[test]
fn a_positions_table_off_standard_has_no_rate() {
    use timegrapher_core::session::{positions_table, Position};
    let log = log_of(&watch(17_570, 0.0, 60.0), None);
    let stretches = [(Position::CH, 0.0, 30.0), (Position::H6, 30.0, 60.0)];
    let mut t = positions_table(&log, None, &[], &[], &stretches, 10.0);
    let read = |t: &timegrapher_core::session::PositionsTable| {
        (t.rows.iter())
            .filter_map(|r| r.measurement.as_ref())
            .count()
    };
    assert_eq!(read(&t), 2);
    assert!(
        t.indices.d_rate.is_some(),
        "read at the guess: {:?}",
        t.indices
    );
    t.judge_lock(Some(log.bph), &log);
    assert!(t.indices.d_rate.is_some(), "a set rate is never read off");
    t.judge_lock(None, &log);
    assert_eq!(read(&t), 2);
    for m in t.rows.iter().filter_map(|r| r.measurement.as_ref()) {
        assert_eq!(m.rate_s_per_day, None);
        assert!(m.amplitude_deg.is_some());
        assert!(off_standard(&m.caveats).is_some(), "{:?}", m.caveats);
    }
    let i = &t.indices;
    assert_eq!((i.x, i.d_rate, i.dvh_rate), (None, None, None), "{i:?}");
}
