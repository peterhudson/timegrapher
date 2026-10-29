//! What the readings rest on, headless: a take's clock log and USB
//! lines reach its figures, and never the take opened before.

use super::tests::{reopened, synthetic};
use super::*;
use timegrapher_core::synth::{generate, SynthConfig};

/// A take whose session file names a clock log is read on that log,
/// as `timegrapher session` reads it: the Positions table's rates and
/// the Rate card's figure are corrected for the sound card's clock, so
/// the two agree, and the Rate's hover says so. Without the log the
/// rates are as heard.
#[test]
fn a_takes_clock_log_corrects_its_rates() {
    let base = std::env::temp_dir().join(format!("tg-app-clock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 120.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&base.join("audio-001.wav"), &audio).unwrap();
    // The card's clock 50 ppm off the computer's.
    let mut log = String::from("unix_s,frames\n");
    for k in 0..13 {
        let audio_s = 10.0 * k as f64;
        log += &format!(
            "{:.6},{}\n",
            1_700_000_000.0 + audio_s * (1.0 + 50e-6),
            (audio_s * audio.sample_rate as f64) as u64
        );
    }
    std::fs::write(base.join("clock.csv"), log).unwrap();
    let manifest = |clock: bool| {
        let c = if clock { "clock = 'clock.csv'\n" } else { "" };
        format!(
            "bph = 28800\nlift = 52.0\n\n\
             [[recording]]\nfile = '.'\nposition = 'DU'\n{c}start_s = 0.0\nend_s = 60.0\nsettle_s = 0.0\n\n\
             [[recording]]\nfile = '.'\nposition = 'DD'\n{c}start_s = 60.0\nsettle_s = 0.0\n"
        )
    };
    let rate_of = |clock: bool| {
        std::fs::write(base.join(MANIFEST), manifest(clock)).unwrap();
        let app = reopened(&base);
        let ctx = egui::Context::default();
        let t = app.free_table(&ctx).expect("a table");
        let rate = match t.rows[0].2 {
            guided::FreeCell::Read(Some(r), _, _) => r,
            ref c => panic!("{c:?}"),
        };
        let ppm = app.take_clock.as_ref().map(|f| f.ppm);
        let card = app
            .live
            .as_ref()
            .unwrap()
            .reading_at(110.0, 10.0)
            .rate_s_per_day;
        (
            rate,
            ppm,
            app.true_clock_words(10.0),
            card.expect("a card figure"),
        )
    };
    let (heard, none, words, card_heard) = rate_of(false);
    assert_eq!(none, None);
    assert_eq!(words, None, "no clock kept, no log: no True Clock");
    let (corrected, ppm, words, card) = rate_of(true);
    let ppm = ppm.expect("the log read");
    assert!((ppm.abs() - 50.0).abs() < 0.5, "{ppm}");
    let want = timegrapher_core::clockstore::correct_rate(heard, ppm);
    assert!(
        (corrected - want).abs() < 0.02,
        "{corrected} {want} {heard}"
    );
    assert!((corrected - heard).abs() > 4.0, "{corrected} {heard}");
    assert!(words.is_some_and(|w| w.contains("clock log")));
    // The card's figure is on the same clock as the table.
    let want = timegrapher_core::clockstore::correct_rate(card_heard, ppm);
    assert!((card - want).abs() < 0.02, "{card} {want} {card_heard}");
    let _ = std::fs::remove_dir_all(&base);
}

/// A take's clock log of too few entries corrects nothing: the card's
/// rate is the rate heard, and the Rate hover says what the log
/// measured and why it isn't applied.
#[test]
fn a_loose_clock_log_corrects_nothing_and_says_why() {
    let base = std::env::temp_dir().join(format!("tg-app-clock-loose-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 120.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&base.join("audio-001.wav"), &audio).unwrap();
    // Five entries, the card 50 ppm off: under the ten a correction
    // needs.
    let mut log = String::from("unix_s,frames\n");
    for k in 0..5 {
        let audio_s = 25.0 * k as f64;
        log += &format!(
            "{:.6},{}\n",
            1_700_000_000.0 + audio_s * (1.0 + 50e-6),
            (audio_s * audio.sample_rate as f64) as u64
        );
    }
    std::fs::write(base.join("clock.csv"), log).unwrap();
    let manifest = |clock: bool| {
        let c = if clock { "clock = 'clock.csv'\n" } else { "" };
        format!(
            "bph = 28800\nlift = 52.0\n\n\
             [[recording]]\nfile = '.'\nposition = 'DU'\n{c}settle_s = 0.0\n"
        )
    };
    let card = |clock: bool| {
        std::fs::write(base.join(MANIFEST), manifest(clock)).unwrap();
        let app = reopened(&base);
        let card = app
            .live
            .as_ref()
            .unwrap()
            .reading_at(110.0, 10.0)
            .rate_s_per_day
            .expect("a card figure");
        (app, card)
    };
    let (_, heard) = card(false);
    let (app, rate) = card(true);
    let fit = app.take_clock.as_ref().expect("the log read");
    assert!(!fit.corrects());
    assert_eq!(app.take_rate_ppm(), None);
    assert!((rate - heard).abs() < 1e-9, "{rate} {heard}");
    let words = app.true_clock_words(rate).expect("said");
    assert!(words.starts_with("The clock log's 5 entries"), "{words}");
    assert!(words.contains("not applied"), "{words}");
    let _ = std::fs::remove_dir_all(&base);
}

/// A take kept as its analysis alone is read on its own clock log, and
/// never on the clock of the take opened before it.
#[test]
fn an_analysis_only_take_reads_its_own_clock() {
    let base = std::env::temp_dir().join(format!("tg-app-clock-kept-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (a, b) = (base.join("a"), base.join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let audio = generate(
        &SynthConfig {
            duration_s: 120.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    let wav = a.join("audio-001.wav");
    timegrapher_core::audio::write_wav(&wav, &audio).unwrap();
    // A log of 13 entries 10 s apart on a card `ppm` slow.
    let log = |ppm: f64| {
        let mut log = String::from("unix_s,frames\n");
        for k in 0..13 {
            let audio_s = 10.0 * k as f64;
            log += &format!(
                "{:.6},{}\n",
                1_700_000_000.0 + audio_s * (1.0 + ppm * 1e-6),
                (audio_s * audio.sample_rate as f64) as u64
            );
        }
        log
    };
    let manifest = |audio: bool| {
        format!(
            "{}bph = 28800\nlift = 52.0\n\n[[recording]]\nfile = '.'\nposition = 'DU'\n\
             clock = 'clock.csv'\nsettle_s = 0.0\n",
            if audio { "" } else { "audio = false\n" }
        )
    };
    std::fs::write(a.join("clock.csv"), log(50.0)).unwrap();
    std::fs::write(a.join(MANIFEST), manifest(true)).unwrap();
    // The same sound kept as its analysis, with a log of its own.
    let stored = analysis_take::measure(
        &[wav.as_path()],
        &timegrapher_core::stream::StreamConfig::default(),
        &[],
        serde_json::json!({"name": "timegrapher", "version": "test"}),
        |_| {},
    )
    .unwrap();
    analysis_take::write(&b, &stored).unwrap();
    std::fs::write(b.join("clock.csv"), log(10.0)).unwrap();
    std::fs::write(b.join(MANIFEST), manifest(false)).unwrap();
    let ppm = |app: &TimegrapherApp| app.take_clock.as_ref().map(|f| f.ppm);
    let mut app = reopened(&a);
    assert!((ppm(&app).expect("a's log") - 50.0).abs() < 1.0);
    app.start_batch(&b);
    let t = Instant::now();
    while app.batch.is_some() && t.elapsed().as_secs() < 120 {
        app.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(app.live.is_some(), "{:?}", app.message);
    assert!(
        (ppm(&app).expect("b's own log") - 10.0).abs() < 1.0,
        "{:?}",
        ppm(&app)
    );
    // Opened fresh, the same.
    let app = reopened(&b);
    assert!((ppm(&app).expect("b's own log") - 10.0).abs() < 1.0);
    let _ = std::fs::remove_dir_all(&base);
}

/// A take whose recording carries USB lines has its rate set by them,
/// over a clock log loose or not, as `timegrapher series` reads it: the
/// card figure and charts corrected, and the hover saying so with
/// what the log measured.
#[test]
fn a_takes_usb_lines_set_its_rate() {
    let base = std::env::temp_dir().join(format!("tg-app-clock-usb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (plain, lined) = (base.join("plain"), base.join("lined"));
    let audio = generate(
        &SynthConfig {
            duration_s: 120.0,
            ..Default::default()
        },
        |_| 280.0,
        |_| 0.0,
    );
    // The same sound with the faint 5, 6 and 7 kHz lines a USB card
    // 19.5 ppm slow hears from its bus.
    let mut with_lines = audio.clone();
    let fs = with_lines.sample_rate as f64;
    for (i, v) in with_lines.samples.iter_mut().enumerate() {
        for (f, a) in [(5000.0, 0.002), (6000.0, 0.003), (7000.0, 0.002)] {
            let f = f * (1.0 + 19.5e-6);
            *v += (a * (2.0 * std::f64::consts::PI * f * i as f64 / fs).sin()) as f32;
        }
    }
    let mut log = String::from("unix_s,frames\n");
    for k in 0..5 {
        let audio_s = 25.0 * k as f64;
        log += &format!(
            "{:.6},{}\n",
            1_700_000_000.0 + audio_s * (1.0 + 50e-6),
            (audio_s * audio.sample_rate as f64) as u64
        );
    }
    for (dir, a) in [(&plain, &audio), (&lined, &with_lines)] {
        std::fs::create_dir_all(dir).unwrap();
        timegrapher_core::audio::write_wav(&dir.join("audio-001.wav"), a).unwrap();
        std::fs::write(dir.join("clock.csv"), &log).unwrap();
        std::fs::write(
            dir.join(MANIFEST),
            "bph = 28800\nlift = 52.0\n\n[[recording]]\nfile = '.'\nposition = 'DU'\n\
             clock = 'clock.csv'\nsettle_s = 0.0\n",
        )
        .unwrap();
    }
    let card = |app: &TimegrapherApp| {
        app.live
            .as_ref()
            .unwrap()
            .reading_at(110.0, 10.0)
            .rate_s_per_day
            .expect("a card figure")
    };
    // No lines: the log stands, too loose to correct by.
    let app = reopened(&plain);
    let heard = card(&app);
    assert_eq!(app.take_rate_ppm(), None, "the log alone is too loose");
    let app = reopened(&lined);
    let ppm = app.take_rate_ppm().expect("set by the lines");
    assert!((ppm - 19.5).abs() < 0.2, "{ppm}");
    let rate = card(&app);
    let want = timegrapher_core::clockstore::correct_rate(heard, ppm);
    assert!((rate - want).abs() < 0.05, "{rate} {want}");
    let last = app.trend.last().and_then(|p| p.rate).expect("charts");
    assert!(
        (last - heard).abs() > 1.0,
        "the charts are on the lines' clock: {last} {heard}"
    );
    let words = app.true_clock_words(rate).expect("said");
    assert!(words.starts_with("On this take's USB lines"), "{words}");
    // The figure the lines measured, as the hover prints it.
    let said = format!("clock, {ppm:.1} ppm slow.");
    assert!(words.contains(&said), "{words} {said}");
    assert!(words.contains("The clock log's 5 entries"), "{words}");
    let _ = std::fs::remove_dir_all(&base);
}
/// The steadiness tests' answer on what the readings rest on now, as
/// JSON, once it comes.
fn steadiness_now(app: &mut TimegrapherApp, ctx: &egui::Context) -> String {
    let start = Instant::now();
    loop {
        app.update_steady(ctx);
        if let Some((rev, s)) = &app.steady {
            if *rev == app.rev {
                return serde_json::to_string(&s.report).unwrap();
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(300),
            "the steadiness tests never came"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Every pane's figures: the whole take, the charts' run of readings, the
/// Positions table and the steadiness tests.
fn panes(app: &mut TimegrapherApp, ctx: &egui::Context) -> [String; 4] {
    app.refresh_trend();
    let w = app.whole_take().expect("the whole take");
    [
        format!(
            "{:?} {:?} {}",
            w.m.rate_s_per_day, w.m.amplitude_deg, w.span
        ),
        format!("{:?}", app.trend),
        format!("{:?}", app.free_table(ctx).expect("a table")),
        steadiness_now(app, ctx),
    ]
}

/// A change to what the readings rest on, by name.
type Change = (&'static str, Box<dyn Fn(&mut TimegrapherApp)>);

/// Every figure kept for later follows what its readings rest on: a new
/// lift angle, a clock for the take and a mask each move the whole take,
/// the charts, the Positions table and the steadiness tests, with
/// nothing cleared by hand.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "a test's clock fixture, not a take being opened"
)]
fn every_pane_follows_the_lift_the_clock_and_the_masks() {
    let ctx = egui::Context::default();
    let mut app = synthetic(320.0, 12.0);
    // Read as a take opened, on its own clock.
    app.put_take_breaks(Some(Vec::new()), None);
    let names = ["whole take", "charts", "Positions table", "steadiness"];
    let mut before = panes(&mut app, &ctx);
    let lift = app.lift_deg + 10.0;
    let duration = app.live.as_ref().unwrap().duration_s();
    let changes: [Change; 3] = [
        ("lift", Box::new(move |app| app.set_lift(lift))),
        (
            "clock",
            Box::new(move |app| {
                app.put_take_clock(
                    Some(timegrapher_core::clock::ClockFit::fixed(50.0, duration)),
                    None,
                )
            }),
        ),
        ("mask", Box::new(|app| app.set_masked(vec![(100.0, 160.0)]))),
    ];
    for (change, apply) in &changes {
        apply(&mut app);
        let after = panes(&mut app, &ctx);
        for (i, name) in names.iter().enumerate() {
            assert_ne!(before[i], after[i], "{name} after the {change}");
        }
        before = after;
    }
}
