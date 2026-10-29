//! The `series` command: the structure each reading series of a take
//! shows in time order (a cycle, two levels, level changes, a trend,
//! correlation between readings), with the statistics behind it.

use crate::clock_cmd::AppliedClock;
use std::path::{Path, PathBuf};
use timegrapher_core::clock::ClockFit;
use timegrapher_core::longrun;
use timegrapher_core::longterm::LongConfig;
use timegrapher_core::periodicity::Wheel;
use timegrapher_core::steadiness::{self, p_text, Config, CycleSource, Report, SeriesCheck};
use timegrapher_core::steady_views;
use timegrapher_core::stream::{BeatLog, StreamConfig};
use timegrapher_core::timefreq::size_number;

pub struct Options<'a> {
    pub files: &'a [PathBuf],
    pub clock_log: Option<&'a Path>,
    pub stream: StreamConfig,
    /// `--lift`; without it the calibre's lift angle, else the default.
    pub lift: Option<f64>,
    pub train: crate::long::Train<'a>,
    pub wheels: &'a [String],
    pub check: Config,
    pub json: bool,
    /// Add the Steadiness pane's views to each series in the JSON.
    pub views: bool,
    /// Add the time-frequency maps of each series to the JSON.
    pub maps: bool,
}

pub fn run(o: Options) -> Result<(), String> {
    let extra: Vec<Wheel> = o
        .wheels
        .iter()
        .map(|w| crate::long::parse_wheel(w))
        .collect::<Result<_, _>>()?;
    let crate::long::Read {
        files,
        take: take_file,
        cfg: stream_cfg,
        train: resolved_train,
        calibre,
        escape_teeth,
        clock_log,
        wheels: take_wheels,
        log,
        clock: applied_clock,
        breaks,
        unlisted,
        masked,
        stretches,
        ..
    } = crate::long::read(
        o.files,
        crate::long::ClockArgs {
            log: o.clock_log,
            device: None,
            card_ppm: None,
        },
        &o.stream,
        o.lift,
        o.train,
    )?;
    let mut lc = LongConfig {
        wheels: resolved_train.wheels(log.bph),
        ..Default::default()
    };
    lc.wheels.extend(take_wheels);
    lc.wheels.extend(extra);
    let clock = applied_clock.as_ref().map(|c| &c.fit);
    let mut long = match stretches {
        Some(_) => longrun::analyse_on_clock(&log, clock, &breaks, &lc),
        None => longrun::analyse_with_breaks(&log, clock, &breaks, &lc),
    };
    if let Some(why) = unlisted {
        timegrapher_core::measure::add(
            &mut long.caveats,
            timegrapher_core::measure::Caveat::BreaksUnlisted { why },
        );
    }
    // A take read in stretches is on its clock already.
    let clock = clock.filter(|_| stretches.is_none());
    let rep = steadiness::check(&log, clock, &long, &lc, &o.check);

    if o.json {
        let settings = serde_json::json!({
            "take": take_file.map(|p| p.display().to_string()),
            "clock_log": clock_log.map(|p| p.display().to_string()),
            "bph": stream_cfg.analysis.bph,
            "lift_deg": stream_cfg.analysis.amplitude.lift_deg,
            "masked": masked,
            "stretches": stretches,
            "notch_hz": stream_cfg.analysis.envelope.notch_hz,
            "highpass_hz": stream_cfg.analysis.envelope.highpass_hz,
            "calibre": calibre,
            "escape_teeth": escape_teeth,
            "named_wheels": lc.wheels,
            "reference_lines": crate::long::reference_lines(&lc.wheels),
            "wheels": o.wheels,
            "reading_s": o.check.rate_reading_s,
            "wind_h": o.check.wind_h,
            "stretch_s": o.check.stretch_s,
        });
        let mut doc = report_document(&rep, applied_clock.as_ref())?;
        // What only a chart draws, a few hundred kB on a long take, comes
        // with --views: the probability plots of the whole take's
        // distributions and the Steadiness pane's views.
        strip_points(&mut doc, o.views);
        // Each named wheel's line sizes against amplitude; every window
        // with --maps.
        let lines = timegrapher_core::wheel_lines::lines_for_take(&log, clock, &long, &lc, o.maps);
        doc["wheel_lines"] = serde_json::to_value(lines).map_err(|e| e.to_string())?;
        if o.views {
            let views = steady_views::views(&log, &rep, &long, &lc, o.check.rate_reading_s);
            if let Some(series) = doc["series"].as_array_mut() {
                for (s, v) in series.iter_mut().zip(&views.series) {
                    s["views"] = serde_json::to_value(v).map_err(|e| e.to_string())?;
                }
            }
            doc["period_span"] =
                serde_json::to_value(views.period_span).map_err(|e| e.to_string())?;
        }
        if o.maps {
            add_maps(&mut doc, &log, clock, &long, &lc, o.check.rate_reading_s)?;
        }
        crate::output::print("series", crate::output::input(&files, settings), &doc)
    } else {
        print_text(&rep, clock.is_some_and(|c| c.corrects()));
        Ok(())
    }
}

/// The time-frequency maps, coherences, Lomb-Scargle periodograms,
/// kurtograms and wheel turns of each series, as --maps adds them.
fn add_maps(
    doc: &mut serde_json::Value,
    log: &BeatLog,
    clock: Option<&ClockFit>,
    long: &longrun::LongReport,
    lc: &LongConfig,
    reading_s: f64,
) -> Result<(), String> {
    let tf = timegrapher_core::timefreq::for_take(log, clock, long, lc);
    doc["time_frequency"] = serde_json::to_value(&tf).map_err(|e| e.to_string())?;
    add_scales(&mut doc["time_frequency"], &tf);
    let coh = timegrapher_core::timefreq::coherences_for_take(log, clock, long, reading_s);
    doc["coherence"] = serde_json::to_value(coh).map_err(|e| e.to_string())?;
    let ls = timegrapher_core::gappy::for_take(log, clock, long, lc, reading_s);
    doc["lomb_scargle"] = serde_json::to_value(ls).map_err(|e| e.to_string())?;
    let kurt = timegrapher_core::timefreq::kurtograms_for_take(log, clock, long, lc);
    doc["kurtograms"] = serde_json::to_value(kurt).map_err(|e| e.to_string())?;
    let turns = timegrapher_core::wheel_lines::turns_for_take(long, lc);
    doc["wheel_turns"] = serde_json::to_value(turns).map_err(|e| e.to_string())?;
    Ok(())
}

fn report_document(
    rep: &Report,
    clock: Option<&AppliedClock>,
) -> Result<serde_json::Value, String> {
    let mut doc = serde_json::to_value(rep).map_err(|e| e.to_string())?;
    doc["clock"] = clock.map_or(serde_json::Value::Null, |c| {
        let mut j = serde_json::json!({
            "ppm": c.fit.ppm,
            "rate_error_s_per_day": c.fit.rate_error_s_per_day,
            "from": c.from,
            "tracks_drift": c.fit.tracks_drift,
        });
        // A log too short or noisy to correct by: what it measured.
        if let Some(w) = c.fit.withheld {
            j["withheld"] = serde_json::json!(w);
        }
        j
    });
    Ok(doc)
}

/// Drops the distributions' probability-plot points: every one, or with
/// `keep_overall` all but the whole take's.
fn strip_points(doc: &mut serde_json::Value, keep_overall: bool) {
    let d = &mut doc["distributions"];
    let mut lists: Vec<&mut serde_json::Value> = Vec::new();
    if let Some(stretches) = d["stretches"].as_array_mut() {
        lists.extend(stretches.iter_mut().map(|s| &mut s["distributions"]));
    }
    for list in lists {
        for e in list.as_array_mut().into_iter().flatten() {
            e.as_object_mut().map(|o| o.remove("points"));
        }
    }
    if !keep_overall {
        for e in d["overall"].as_array_mut().into_iter().flatten() {
            e.as_object_mut().map(|o| o.remove("points"));
        }
    }
}

fn print_text(r: &Report, calibrated: bool) {
    println!(
        "Recording    {} at {} bph{}",
        crate::long::duration(r.duration_s),
        r.bph,
        if calibrated {
            ", clock corrected (tg, and a session's reference rates, read on the sound card's clock)"
        } else {
            ""
        }
    );
    for (label, line) in crate::long::caveat_lines(&r.caveats) {
        println!("{label:<13}{line}");
    }
    for s in &r.series {
        print_series(s, r.config.outlier_sigma);
    }
    if let Some(c) = &r.isochronism {
        println!();
        println!("{}", c.headline);
        for b in &c.bins {
            println!(
                "  {:>5.0}°  {:+7.1} s/d  (quartiles {:+.1} to {:+.1}, {} readings, {:.1}–{:.1} h)",
                b.amplitude_deg,
                b.rate_s_per_day,
                b.rate_q25,
                b.rate_q75,
                b.readings,
                b.from_wind_h,
                b.to_wind_h
            );
        }
    }
    if let Some(w) = &r.wind_down {
        println!();
        println!("{}", w.headline);
    }
    if let Some(x) = &r.rate_amplitude {
        println!();
        println!("{}", x.headline);
        println!(
            "  {:<14} r {:.2} over {} pairs (worth about {:.0} independent); {:+.3} s/d per degree; p {}",
            "rate vs amp.",
            x.r,
            x.readings,
            x.effective_readings,
            x.s_per_day_per_deg,
            p_text(x.p_value)
        );
    }
    let d = &r.distributions;
    if !d.overall.is_empty() {
        println!();
        println!("Distributions");
        for x in d.overall.iter().filter(|x| x.values == "readings") {
            println!("  {:<12} {}", x.series.replace('_', " "), x.summary);
        }
        if !d.stretches.is_empty() {
            println!(
                "  and over each {} (in the JSON)",
                crate::long::duration(d.stretch_s)
            );
        }
    }
    if let Some(line) = closing_line(r) {
        println!("\n{line}");
    }
}

/// The last line when nothing was found; it leaves "no cycle" out when a
/// period-search line is too fast for the readings.
fn closing_line(r: &Report) -> Option<&'static str> {
    if !r.findings.is_empty() {
        None
    } else if r.series.iter().any(|s| s.faster_than_readings().is_some()) {
        Some("No two levels, level change, trend or correlation between readings detected in any series, and no cycle the readings can show.")
    } else {
        Some("No cycle, two levels, level change, trend or correlation between readings detected in any series.")
    }
}

fn opt(v: Option<f64>, d: usize) -> String {
    v.map_or("-".into(), |x| format!("{x:.d$}"))
}

fn print_series(s: &SeriesCheck, outlier_sigma: f64) {
    let name = s.series.name();
    println!();
    println!("{}", s.headline);
    println!(
        "  {:<14} {} readings of {:.0} s ({} left out as more than {:.0} σ from their 5-minute level); median {}, sd {}, reading to reading {} {}",
        name,
        s.readings,
        s.step_s,
        s.outliers,
        outlier_sigma,
        opt(s.median, 2),
        opt(s.sd, 2),
        opt(s.short_term_sd, 2),
        s.unit
    );
    if let Some(a) = &s.autocorrelation {
        println!(
            "  {:<14} lag-1 r {:.2}, largest of {} lags {:.2} (band ±{:.2}); Ljung–Box p {}{}",
            "independence",
            a.r.get(1).copied().unwrap_or(f64::NAN),
            a.ljung_box_lags,
            a.max_short_lag_r,
            a.band,
            p_text(a.p_value),
            a.memory_s
                .map_or(String::new(), |m| format!("; memory {m:.0} s"))
        );
    }
    if let Some(a) = &s.allan {
        println!(
            "  {:<14} slope {} (−0.5 independent); up to {:.1}× the independent line; steadiest averaging {} s ({} {})",
            "Allan",
            opt(a.slope_short, 2),
            a.max_excess,
            opt(a.best_tau_s, 0),
            opt(a.best_deviation, 2),
            s.unit
        );
    }
    if let Some(c) = &s.cusum {
        println!(
            "  {:<14} max {:.2} at {:.0} s (p {} if independent)",
            "CUSUM",
            c.max,
            c.at_s,
            p_text(c.p_value)
        );
    }
    if let Some(c) = s.changes.as_ref().filter(|c| c.segments.len() > 1) {
        println!(
            "  {:<14} {} stretches of one level, {:.0}% explained:",
            "levels",
            c.segments.len(),
            c.explained * 100.0
        );
        for g in &c.segments {
            println!(
                "  {:<14}   {:>9} to {:>9}  {:>8.2} {}",
                "",
                crate::long::duration(g.start_s),
                crate::long::duration(g.end_s),
                g.mean,
                s.unit
            );
        }
    }
    if let Some(t) = &s.trend {
        println!(
            "  {:<14} {:+.3} {}/h ({:.0}% explained)",
            "trend",
            t.per_hour,
            s.unit,
            t.explained * 100.0
        );
    }
    if let Some(c) = &s.cycle {
        println!(
            "  {:<14} {:.1} s{}{}, {} {} p-p, explains {:.0}% (found by {})",
            "cycle",
            c.period_s,
            c.wheel
                .as_ref()
                .map_or(String::new(), |w| format!(" ({w})")),
            match (c.inner_period_s, c.inner_explained, c.turns) {
                (Some(p), Some(e), Some(n)) =>
                    format!(" = {n} × {p:.2} s (which alone explains {:.0}%)", e * 100.0),
                _ => String::new(),
            },
            size_number(c.peak_to_peak),
            s.unit,
            c.explained * 100.0,
            match c.source {
                CycleSource::TwoState => "the two-state finder",
                CycleSource::Autocorrelation => "the autocorrelation",
                CycleSource::PeriodSearch => "the period search",
            }
        );
    }
    if let Some(p) = &s.period {
        println!(
            "  {:<14} {:.1} s{}, {} {} p-p, explains {:.0}%, false alarm {}",
            "period search",
            p.period_s,
            p.wheel
                .as_ref()
                .map_or(String::new(), |w| format!(" ({w})")),
            size_number(p.size),
            p.size_unit,
            p.explained * 100.0,
            p_text(10f64.powf(-p.significance))
        );
    }
}

/// Each map's `scale`: the colour scale the app draws it in, linear and
/// log, so every drawing of it agrees.
fn add_scales(maps: &mut serde_json::Value, tf: &[timegrapher_core::timefreq::TimeFrequency]) {
    for (j, m) in maps.as_array_mut().into_iter().flatten().zip(tf) {
        let scale = |log| timegrapher_core::timefreq::Scale::of(&m.values, log);
        j["scale"] = serde_json::json!({ "linear": scale(false), "log": scale(true) });
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "tests build the clock they expect by hand"
)]
mod tests {
    use super::*;
    use crate::clock_cmd::resolve as resolve_clock;
    use std::fs;
    use timegrapher_core::audio;
    use timegrapher_core::clock::{self, ClockFit};

    #[test]
    fn each_map_carries_the_scale_the_app_draws_it_in() {
        use timegrapher_core::timefreq::{Scale, TimeFrequency};
        let tf = vec![TimeFrequency {
            series: "amplitude",
            method: "sst",
            unit: "deg",
            step_s: 2.0,
            window_s: None,
            hop_s: None,
            period_s: vec![100.0, 10.0],
            t_s: vec![5.0, 15.0],
            values: vec![vec![1.0, f64::NAN], vec![4.0, 0.002]],
        }];
        let mut maps = serde_json::to_value(&tf).unwrap();
        add_scales(&mut maps, &tf);
        let at = |log| serde_json::to_value(Scale::of(&tf[0].values, log)).unwrap();
        assert_eq!(maps[0]["scale"]["linear"], at(false));
        assert_eq!(maps[0]["scale"]["log"], at(true));
        assert_eq!(
            maps[0]["scale"]["log"],
            serde_json::json!({ "lo": 4.0 / 300.0, "hi": 4.0 })
        );
    }
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn info() -> audio::AudioInfo {
        audio::AudioInfo {
            sample_rate: 48000,
            frames: Some(48000 * 600),
            bytes_per_frame: 2,
        }
    }

    fn report() -> Report {
        Report {
            duration_s: 600.0,
            bph: 28800,
            breaks: vec![],
            caveats: vec![],
            config: Config::default(),
            series: vec![],
            rate_amplitude: None,
            isochronism: None,
            wind_down: None,
            distributions: timegrapher_core::distribution::Distributions {
                overall: vec![],
                stretch_s: 0.0,
                stretches: vec![],
            },
            spectra: vec![],
            findings: vec![],
        }
    }

    #[test]
    fn the_closing_line_leaves_out_no_cycle_only_beside_a_fast_line() {
        use timegrapher_core::longterm::Grid;
        use timegrapher_core::steadiness::{check_series, Period, SeriesKind};
        // White noise: no cycle, level change or trend.
        let steady = |period: Option<Period>| {
            let mut seed = 7u64;
            let mut z = || {
                (0..12)
                    .map(|_| {
                        seed = seed
                            .wrapping_mul(6364136223846793005)
                            .wrapping_add(1442695040888963407);
                        (seed >> 11) as f64 / (1u64 << 53) as f64
                    })
                    .sum::<f64>()
                    - 6.0
            };
            let y = (0..360).map(|_| 5.0 + 2.0 * z()).collect();
            check_series(
                SeriesKind::Rate,
                Grid {
                    t0: 0.0,
                    step: 10.0,
                    y,
                },
                None,
                period,
                None,
                &[],
                &Config::default(),
            )
        };
        let line = Period {
            period_s: 5.0,
            significance: 10.0,
            explained: 0.5,
            peak_to_peak: 0.04,
            timing_peak_to_peak_ms: Some(0.04),
            size: 0.04,
            size_unit: "ms",
            wheel: Some("escape wheel".into()),
            other_periods_s: vec![],
            harmonic_of: None,
            other_harmonic_of: vec![],
        };
        let mut r = report();
        r.series = vec![steady(None)];
        assert_eq!(
            closing_line(&r),
            Some("No cycle, two levels, level change, trend or correlation between readings detected in any series.")
        );
        r.series = vec![steady(Some(line))];
        assert!(
            r.series[0].faster_than_readings().is_some(),
            "{}",
            r.series[0].headline
        );
        assert_eq!(
            closing_line(&r),
            Some("No two levels, level change, trend or correlation between readings detected in any series, and no cycle the readings can show.")
        );
    }

    struct Log(PathBuf);
    impl Log {
        fn new(text: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "timegrapher-series-clock-{}-{}.csv",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::write(&path, text).unwrap();
            Self(path)
        }
    }
    impl Drop for Log {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn series_clock_context_retains_selected_fallback_and_all_report_fields() {
        let rep = report();
        // This is the pre-change production JSON conversion: no clock context.
        let original = serde_json::to_value(&rep).unwrap();
        assert!(original.get("clock").is_none());
        for (ppm, from) in [
            (12.5, "stored for 'input', 2026-10-15"),
            (-12.5, "from 3 USB lines in the recording, 0.2 ppm apart"),
            (0.0, "stored for 'zero input', 2026-10-15"),
        ] {
            let mut calls = 0;
            let applied = resolve_clock(
                None,
                &info(),
                600.0,
                || Ok(None),
                || {
                    calls += 1;
                    Ok(Some((ppm, from.into())))
                },
            )
            .unwrap()
            .unwrap();
            assert_eq!(calls, 1);
            let expected_fit = timegrapher_core::session::fixed_clock(ppm, 600.0).unwrap();
            assert_eq!(
                serde_json::to_value(&applied.fit).unwrap(),
                serde_json::to_value(&expected_fit).unwrap()
            );
            assert_eq!(applied.fit.map(300.0), expected_fit.map(300.0));
            let mut doc = report_document(&rep, Some(&applied)).unwrap();
            assert_eq!(
                doc.as_object_mut().unwrap().remove("clock").unwrap(),
                json!({
                    "ppm": ppm,
                    "rate_error_s_per_day": ppm * 1e-6 * 86400.0,
                    "from": from,
                    "tracks_drift": false,
                })
            );
            assert_eq!(doc, original, "every previous report field is unchanged");
        }
        let none = resolve_clock(None, &info(), 600.0, || Ok(None), || Ok(None)).unwrap();
        let mut doc = report_document(&rep, none.as_ref()).unwrap();
        assert_eq!(
            doc.as_object_mut().unwrap().remove("clock"),
            Some(serde_json::Value::Null)
        );
        assert_eq!(doc, original);
        assert!(resolve_clock(
            None,
            &info(),
            600.0,
            || Ok(None),
            || Err("store unreadable".into())
        )
        .err()
        .unwrap()
        .contains("store unreadable"));
    }

    #[test]
    fn series_clock_log_wins_and_serializes_the_fit_actually_applied() {
        // Supplied clock entries only; no acoustic analysis or local clock store.
        // Eleven entries a minute apart, enough to correct by.
        let text: String = std::iter::once("audio_s,unix_s\n".to_string())
            .chain((0..=10).map(|i| {
                let a = i as f64 * 60.0;
                format!("{a},{}\n", 1000.0 + a * 1.0000125)
            }))
            .collect();
        let text = text.as_str();
        let log = Log::new(text);
        let applied = resolve_clock(
            Some(&log.0),
            &info(),
            600.0,
            || Ok(None),
            || panic!("explicit log must bypass stored/USB fallback"),
        )
        .unwrap()
        .unwrap();
        let pairs = clock::parse_log(text, 48000, 2).unwrap();
        let expected = ClockFit::for_rates(&pairs).unwrap();
        assert!(expected.corrects());
        assert_eq!(
            serde_json::to_value(&applied.fit).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        assert_eq!(applied.fit.map(150.0), expected.map(150.0));
        assert!((applied.fit.ppm - 12.5).abs() < 1e-6);
        let doc = report_document(&report(), Some(&applied)).unwrap();
        assert_eq!(
            doc["clock"],
            json!({
                "ppm": expected.ppm,
                "rate_error_s_per_day": expected.rate_error_s_per_day,
                "from": format!("from clock log {}", log.0.file_name().unwrap().to_string_lossy()),
                "tracks_drift": expected.tracks_drift,
            })
        );
    }

    #[test]
    fn series_clock_from_a_short_log_says_why_it_corrects_nothing() {
        let text = "audio_s,unix_s\n0,1000\n300,1300.00375\n600,1600.0075\n";
        let log = Log::new(text);
        let applied = resolve_clock(
            Some(&log.0),
            &info(),
            600.0,
            || Ok(None),
            || panic!("fallback"),
        )
        .unwrap()
        .unwrap();
        assert!(!applied.fit.corrects());
        let clock = &report_document(&report(), Some(&applied)).unwrap()["clock"];
        assert_eq!(clock["ppm"], 0.0);
        assert_eq!(clock["withheld"]["reason"], "few_entries");
        assert_eq!(clock["withheld"]["points"], 3);
    }

    #[test]
    fn series_clock_takes_the_usb_lines_over_a_log() {
        let text = "audio_s,unix_s\n0,1000\n300,1300.00375\n600,1600.0075\n";
        let log = Log::new(text);
        let applied = resolve_clock(
            Some(&log.0),
            &info(),
            600.0,
            || {
                Ok(Some((
                    19.5,
                    "from 12 USB lines in the recording, 0.1 ppm apart".into(),
                )))
            },
            || panic!("fallback"),
        )
        .unwrap()
        .unwrap();
        assert!(applied.fit.corrects());
        assert_eq!(
            applied.from,
            format!(
                "from 12 USB lines in the recording, 0.1 ppm apart; clock log {} for its breaks",
                log.0.file_name().unwrap().to_string_lossy()
            )
        );
        let clock = &report_document(&report(), Some(&applied)).unwrap()["clock"];
        assert_eq!(clock["ppm"], 19.5);
        assert_eq!(clock["withheld"]["reason"], "usb_lines");
        assert_eq!(clock["withheld"]["points"], 3);
    }

    #[test]
    fn series_clock_drifting_log_and_invalid_log_do_not_use_fallback() {
        let mut text = String::from("audio_s,unix_s\n");
        for i in 0..=60 {
            let audio_s = i as f64 * 600.0;
            text.push_str(&format!("{audio_s},{}\n", 1000.0 + audio_s * 1.0000125));
        }
        let log = Log::new(&text);
        let applied = resolve_clock(
            Some(&log.0),
            &info(),
            36000.0,
            || Ok(None),
            || panic!("fallback"),
        )
        .unwrap()
        .unwrap();
        assert!(applied.fit.tracks_drift);
        assert_eq!(
            report_document(&report(), Some(&applied)).unwrap()["clock"]["tracks_drift"],
            true
        );
        let bad = Log::new("audio_s,unix_s\n0,1000\n");
        let error = resolve_clock(
            Some(&bad.0),
            &info(),
            600.0,
            || Ok(None),
            || panic!("invalid log must fail rather than change clock source"),
        )
        .err()
        .unwrap();
        assert!(error.contains(&bad.0.display().to_string()));
    }
}
