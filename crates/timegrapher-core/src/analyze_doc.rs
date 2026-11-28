//! The body of `analyze --json` (`timegrapher.analyze/3`), worded once
//! for every surface that writes one: the command line, and the app's
//! bug report, whose excerpt `report --replay` analyses again and
//! compares. Each surface wraps it in its own envelope (schema, software,
//! input files) and names its version, [`VERSION`].

use crate::analysis::{AnalysisConfig, Summary};
use serde_json::{json, Value};

/// The version of the `analyze` document: `analyze/2` dropped the
/// grades, verdicts, named faults and advice; `analyze/3` gives the
/// headline rate, the one the text leads with, as `rate_s_per_day`
/// (#2105, [`body`]), and counts no beats of a watch where none is heard
/// (#2378, [`crate::measure::watch_beats`]).
pub const VERSION: u32 = 3;

/// The settings `analyze` read at, as its document's `input.settings`
/// gives them; `calibre` is the calibre named, if one was. The escapement
/// is given only for a co-axial (#2313).
pub fn settings(cfg: &AnalysisConfig, calibre: Option<&str>) -> Value {
    let mut s = json!({
        "calibre": calibre,
        "bph": cfg.bph,
        "lift_deg": cfg.amplitude.lift_deg,
        "notch_hz": cfg.envelope.notch_hz,
        "highpass_hz": cfg.envelope.highpass_hz,
        "escape_teeth": cfg.escape_teeth,
    });
    if !cfg.amplitude.escapement.is_swiss_lever() {
        s["escapement"] = json!(cfg.amplitude.escapement);
    }
    s
}

/// The document's body: the summary, with its headline rate, the one the
/// text leads with (#2105): corrected onto true time at the document's
/// correction when it has one (`ppm` fast, and where it came from in
/// words, `clock`), else on the sound card's clock. `rate_s_per_day` and
/// `overall.rate_s_per_day` are on that clock; `rate_card_s_per_day`
/// keeps the rate on the card's clock, as tg reads it and the regression
/// check compares it, and the spread `rate_p05`/`rate_p95` stays on the
/// card's clock, as the views label it. The beat rate
/// heard ([`measured`]) is on the headline rate's clock too. Where no
/// watch is heard, no beats of a watch are counted
/// ([`crate::measure::watch_beats`], Norm on #2378). Off every
/// standard rate on Auto (the verdict `analyze` made on the card's
/// clock, never made again here; its figures re-read at that correction
/// by [`Summary::read_off_standard`]), no rate in s/d is given: the fit's
/// in `overall` and the corrected one are null too (Norm on #2223, point
/// 5, and on #2316).
///
/// On a co-axial the overall fit's beat error, the beat times' and not a
/// drop's, is left out (#2313,
/// [`crate::escapement::Escapement::drop_beat_error`]).
pub fn body(summary: &Summary, clock: Option<(f64, &str)>) -> Result<Value, String> {
    let ppm = clock.map(|(ppm, _)| ppm);
    let read;
    let summary = match crate::measure::off_standard(&summary.caveats) {
        Some(_) if ppm.is_some() => {
            let mut s = summary.clone();
            s.read_off_standard(ppm);
            read = s;
            &read
        }
        _ => summary,
    };
    let off = crate::measure::off_standard(&summary.caveats).is_some();
    let counted;
    let summary = if summary.heard_as_watch() {
        summary
    } else {
        let mut s = summary.clone();
        s.count_no_watch();
        counted = s;
        &counted
    };
    let mut body = serde_json::to_value(summary).map_err(|e| e.to_string())?;
    body["overall"] = summary.escapement.overall_json(summary.overall.as_ref());
    let (_, bph) = crate::measure::watch_beats(
        summary.heard_as_watch(),
        summary.beats_found,
        summary.bph,
        summary.bph_set,
    );
    body["bph"] = json!(bph);
    if let Some(m) = measured(summary, ppm) {
        m.write_into(&mut body);
    }
    // The headline rate, on the document's clock (#2105).
    let headline =
        |r: Option<f64>| json!(r.map(|r| ppm.map_or(r, |p| crate::clockstore::correct_rate(r, p))));
    body["rate_card_s_per_day"] = json!(summary.rate_s_per_day);
    body["rate_s_per_day"] = headline(summary.rate_s_per_day);
    if let Some(o) = body["overall"].as_object_mut() {
        let rate = summary
            .overall
            .as_ref()
            .filter(|_| !off)
            .map(|f| f.rate_s_per_day);
        o.insert("rate_s_per_day".into(), headline(rate));
    }
    if let Some((ppm, from)) = clock {
        body["clock"] = crate::clockstore::correction_json(
            ppm,
            from,
            summary
                .overall
                .as_ref()
                .filter(|_| !off)
                .map(|f| f.rate_s_per_day),
        );
    }
    Ok(body)
}

/// The beat rate heard over the whole recording (#2223): from the fit
/// over every beat, on the clock its rate is shown on, corrected at `ppm`
/// when the document carries a correction, with the rate's ±. Off every
/// standard rate, the caveat's ([`crate::timing::OffStandard`]), which
/// [`body`] reads at the same correction. None when no regular beat is
/// found, as every reading.
pub fn measured(summary: &Summary, ppm: Option<f64>) -> Option<crate::timing::MeasuredRate> {
    if let Some(o) = crate::measure::off_standard(&summary.caveats) {
        return Some(o.measured());
    }
    let rate = summary.overall.as_ref().map(|f| {
        ppm.map_or(f.rate_s_per_day, |p| {
            crate::clockstore::correct_rate(f.rate_s_per_day, p)
        })
    });
    let se = summary.confidence.as_ref().and_then(|c| c.rate.se);
    crate::timing::MeasuredRate::of(summary.bph, rate, se)
}

/// The correction `analyze` puts beside a summary with no other given:
/// the recording's own USB lines, when it has them and they apply
/// ([`crate::usbclock::UsbClock::applies`]).
pub fn usb_clock(summary: &Summary) -> Option<(f64, String)> {
    summary
        .usb_clock
        .and_then(crate::usbclock::UsbClock::applied)
        .map(|c| c.correction())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::analyze;
    use crate::synth::{generate, SynthConfig};

    /// The settings name what was read at, and the body carries the
    /// summary with the correction beside its unchanged card rate.
    #[test]
    fn the_body_carries_the_summary_and_the_correction_beside_it() {
        let audio = generate(
            &SynthConfig {
                duration_s: 20.0,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        );
        let cfg = AnalysisConfig::default();
        let a = analyze(&audio, &cfg);
        let s = settings(&cfg, Some("ETA 2824-2"));
        assert_eq!(s["calibre"], "ETA 2824-2");
        assert_eq!(s["highpass_hz"], cfg.envelope.highpass_hz);
        let plain = body(&a.summary, None).unwrap();
        assert_eq!(plain["bph"], a.summary.bph);
        assert!(plain.get("clock").is_none());
        let corrected = body(&a.summary, Some((10.0, "typed in"))).unwrap();
        assert_eq!(corrected["clock"]["ppm"], 10.0);
        assert_eq!(corrected["clock"]["from"], "typed in");
        assert_eq!(corrected["bph"], plain["bph"]);
    }

    /// A balance beating 17,570 read against 18,000 (the hairspring
    /// maker's case, #2223): the document gives the beat rate heard,
    /// 17570 at its place, 430 bph slow of the set rate, from the same
    /// fit as the rate it gives; on the clock the rate is corrected to
    /// when the document carries a correction.
    #[test]
    fn the_document_gives_the_beat_rate_heard() {
        let audio = generate(
            &SynthConfig {
                duration_s: 60.0,
                bph: 17_570,
                rate_s_per_day: 0.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let cfg = AnalysisConfig {
            bph: Some(18_000),
            ..Default::default()
        };
        let a = analyze(&audio, &cfg);
        assert!(a.summary.watch_heard);
        let doc = body(&a.summary, None).unwrap();
        let heard = doc["bph_measured"].as_f64().unwrap();
        let m = measured(&a.summary, None).unwrap();
        assert_eq!(heard, m.bph);
        assert_eq!((m.whole(), m.offset_from(18_000)), (17_570.0, -430.0));
        let rate = a.summary.overall.unwrap().rate_s_per_day;
        assert_eq!(
            m,
            crate::timing::MeasuredRate::of(18_000, Some(rate), None).unwrap()
        );
        // 60 s holds too few rate values for a ±, so neither has one.
        assert_eq!(a.summary.confidence.unwrap().rate.se, None);
        assert!(doc["bph_measured_se"].is_null());
        // With a rate ± of 4.8 s/d, the beat rate heard has 4.8 / 86,400
        // of the rate it is read against, 1 bph (Norm on #2303).
        let mut with_pm = a.summary.clone();
        with_pm.confidence.as_mut().unwrap().rate.se = Some(4.8);
        let se = body(&with_pm, None).unwrap()["bph_measured_se"]
            .as_f64()
            .unwrap();
        assert!((se - 18_000.0 * 4.8 / 86_400.0).abs() < 1e-12, "{se}");
        // Corrected for a card 50 ppm fast, the rate reads 50 ppm more
        // (`clockstore::correct_rate`), and so does the beat rate heard.
        let corrected = body(&a.summary, Some((50.0, "typed in"))).unwrap();
        let on_true = corrected["bph_measured"].as_f64().unwrap();
        assert!(
            (on_true / heard - 1.0 - 50e-6).abs() < 1e-8,
            "{on_true} {heard}"
        );
        let corrected_rate = crate::clockstore::correct_rate(rate, 50.0);
        let m50 = crate::timing::MeasuredRate::of(18_000, Some(corrected_rate), None).unwrap();
        assert_eq!(on_true, m50.bph);
    }

    /// The headline rate is the one the text leads with (#2105): with a
    /// correction, `rate_s_per_day`, `overall.rate_s_per_day` and
    /// `clock.rate_s_per_day` are the rate corrected at it, and
    /// `rate_card_s_per_day` is the card's; with none, all are the card's.
    #[test]
    fn the_headline_rate_is_on_the_documents_clock() {
        let audio = generate(
            &SynthConfig {
                duration_s: 30.0,
                rate_s_per_day: 6.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let a = analyze(&audio, &AnalysisConfig::default());
        let card = a.summary.rate_s_per_day.unwrap();
        let plain = body(&a.summary, None).unwrap();
        assert_eq!(plain["rate_s_per_day"], card);
        assert_eq!(plain["overall"]["rate_s_per_day"], card);
        assert_eq!(plain["rate_card_s_per_day"], card);
        let doc = body(&a.summary, Some((-20.0, "typed in"))).unwrap();
        let corrected = crate::clockstore::correct_rate(card, -20.0);
        assert!(
            (corrected - card + 1.728).abs() < 1e-3,
            "{corrected} {card}"
        );
        assert_eq!(doc["rate_s_per_day"], corrected);
        assert_eq!(doc["overall"]["rate_s_per_day"], corrected);
        assert_eq!(doc["clock"]["rate_s_per_day"], corrected);
        assert_eq!(doc["rate_card_s_per_day"], card);
        // The spread stays on the card's clock, as the views label it.
        assert_eq!(doc["rate_p05"], plain["rate_p05"]);
    }

    /// No regular beat, no reading: no beat rate heard either, and no
    /// beats of a watch found (Norm on #2378): `beats_found` and its share
    /// 0, `beats_missing` with its caveat, no `bph` or share of one with
    /// no rate set; with one set, `bph` is it and the share stays.
    #[test]
    fn noise_gives_no_beat_rate_heard() {
        let audio = generate(
            &SynthConfig {
                duration_s: 20.0,
                snr_db: -30.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let a = analyze(&audio, &AnalysisConfig::default());
        assert!(!a.summary.watch_heard);
        assert!(a.summary.beats_found > 0, "the peaks the noise gives");
        let doc = body(&a.summary, None).unwrap();
        assert!(doc.get("bph_measured").is_none());
        assert!(doc.get("bph_measured_se").is_none());
        assert_eq!(doc["beats_found"], 0);
        assert_eq!(doc["beats_found_fraction"], 0.0);
        assert!(doc["bph"].is_null());
        assert!(doc.get("bph_heard").is_none());
        assert!(doc["timing_scatter_fraction"].is_null());
        assert_eq!(doc["jitter_us"], json!(a.summary.jitter_us));
        assert_eq!(doc["doubts"]["beats_missing"], true);
        let codes: Vec<&Value> = doc["caveats"].as_array().unwrap().iter().collect();
        assert!(codes.iter().any(|c| c["code"] == "no_watch"));
        let missing = codes.iter().find(|c| c["code"] == "beats_missing").unwrap();
        assert_eq!(missing["fraction"], 0.0);
        let set = analyze(
            &audio,
            &AnalysisConfig {
                bph: Some(28_800),
                ..Default::default()
            },
        );
        assert!(!set.summary.watch_heard);
        let doc = body(&set.summary, None).unwrap();
        assert_eq!(doc["bph"], 28_800);
        assert_eq!(doc["beats_found"], 0);
        assert!(doc.get("bph_heard").is_none());
        assert_eq!(
            doc["timing_scatter_fraction"],
            json!(set.summary.timing_scatter_fraction)
        );
        assert!(!doc["caveats"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["code"] == "set_rate_disagrees"));
    }

    /// A reading still waiting, under 10 s, has heard no watch yet, so the
    /// document counts no beats of one (Norm on #2378 and #2333): on 5 s
    /// of noise and on 5 s of a clean watch alike, `beats_found` 0 and no
    /// `bph` with no rate set; with one set, `bph` is it.
    #[test]
    fn a_reading_still_waiting_counts_no_beats_of_a_watch() {
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        let noise = crate::audio::Audio {
            samples: (0..5 * 48_000)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    (((x >> 11) as f64 / (1u64 << 53) as f64) - 0.5) as f32 * 0.02
                })
                .collect(),
            sample_rate: 48_000,
        };
        let watch = generate(
            &SynthConfig {
                duration_s: 5.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        for (what, audio) in [("noise", &noise), ("watch", &watch)] {
            for set in [None, Some(28_800)] {
                let cfg = AnalysisConfig {
                    bph: set,
                    ..Default::default()
                };
                let s = analyze(audio, &cfg).summary;
                assert!(s.awaiting && !s.heard_as_watch(), "{what}: {:?}", s.caveats);
                assert!(s.beats_found > 0, "{what}: the peaks found");
                let doc = body(&s, None).unwrap();
                let at = format!("{what}, set {set:?}");
                assert_eq!(doc["beats_found"], 0, "{at}");
                assert_eq!(doc["beats_found_fraction"], 0.0, "{at}");
                assert_eq!(doc["bph"], json!(set), "{at}");
                assert!(doc.get("bph_heard").is_none(), "{at}");
                assert!(doc["rate_s_per_day"].is_null(), "{at}");
            }
        }
    }
}
