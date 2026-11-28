//! What the app's tests share: a synthetic watch heard, a themed context
//! and a screen, frames, painted text, and takes saved, queued and opened.

use super::*;
/// The settling the tests of a move set the pane's Settle to.
pub(super) const SETTLE_S: f64 = 10.0;

use timegrapher_core::synth::{generate, SynthConfig};

/// `seconds` of digital silence heard and stopped, its charts' readings
/// read: no watch, as the cards say "No Beats Heard".
pub(super) fn silent(seconds: usize) -> TimegrapherApp {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "silence".into());
    let live = app.bench.recording.live.as_mut().expect("live");
    for _ in 0..seconds * 50 {
        live.push(&[0.0_f32; 960]);
    }
    app.refresh_trend();
    app
}

pub(super) fn synthetic(seconds: f64, rate: f64) -> TimegrapherApp {
    synthetic_varying(seconds, rate, |_| 0.0)
}

/// [`synthetic`] with `extra(t)` s/d more rate at each time `t`.
pub(super) fn synthetic_varying(
    seconds: f64,
    rate: f64,
    extra: impl Fn(f64) -> f64,
) -> TimegrapherApp {
    synthetic_swinging(seconds, rate, |_| 280.0, extra)
}

/// [`synthetic_varying`] with the amplitude `amp(t)` degrees at each time
/// `t`.
pub(super) fn synthetic_swinging(
    seconds: f64,
    rate: f64,
    amp: impl Fn(f64) -> f64,
    extra: impl Fn(f64) -> f64,
) -> TimegrapherApp {
    synthetic_set(seconds, rate, None, amp, extra)
}

/// [`synthetic_swinging`] heard with the beat rate set to `bph`, or on
/// Auto with none: a set rate is read against however far the watch is
/// from it, where Auto past 1000 s/d gives no rate (#2223).
pub(super) fn synthetic_set(
    seconds: f64,
    rate: f64,
    bph: Option<u32>,
    amp: impl Fn(f64) -> f64,
    extra: impl Fn(f64) -> f64,
) -> TimegrapherApp {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.bench.take.bph = bph;
    app.reset_session(48000, "syn".into());
    let cfg = SynthConfig {
        duration_s: seconds,
        rate_s_per_day: rate,
        ..Default::default()
    };
    let audio = generate(&cfg, amp, extra);
    let live = app.bench.recording.live.as_mut().unwrap();
    for b in audio.samples.chunks(960) {
        live.push(b);
    }
    app.poll();
    // The charts read on what the take rests on now, as the frame reads
    // them: the engine's whole run of readings, read once. A reading asked
    // for at each pass would read everything heard again as a file each
    // time until the template is built again (#1987), only to be replaced
    // here (#2442).
    app.refresh_trend();
    assert_eq!(app.bench.results.trend_key, app.trend_now(), "read once");
    app
}

pub(super) fn screen() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(1280.0, 820.0),
        )),
        ..Default::default()
    }
}

/// Every piece of text a frame painted.
pub(super) fn painted_text(out: &egui::FullOutput) -> String {
    out.shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A context with the app's fonts and styles, as the window has.
pub(super) fn themed() -> egui::Context {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    ctx
}

/// Draw `n` frames of the whole window.
pub(super) fn frames(app: &mut TimegrapherApp, ctx: &egui::Context, n: usize) {
    for _ in 0..n {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1280.0, 820.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.panels(ctx));
    }
}

pub(super) fn save_failure_fixture(name: &str) -> (TimegrapherApp, PathBuf) {
    let base =
        std::env::temp_dir().join(format!("tg-app-save-failure-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.reset_session(48000, "test".into());
    app.session.listen();
    app.session.recorder =
        Some(Recorder::start(&base.join("live"), app.info("test", 48000, 16)).unwrap());
    append_save_test_second(&mut app, 1);
    (app, base)
}

pub(super) fn append_save_test_second(app: &mut TimegrapherApp, at: u64) {
    let block = timegrapher_core::capture::Block {
        samples: vec![0.0; 48000],
        at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(at),
    };
    app.session
        .recorder
        .as_mut()
        .unwrap()
        .write(&block)
        .unwrap();
    for samples in block.samples.chunks(960) {
        app.bench.recording.live.as_mut().unwrap().push(samples);
    }
}

// Replay supplies a real Capture without opening hardware. Its tiny file
// has no samples; replace only its public receiver with a controlled queue.
pub(super) fn queued_capture(
    app: &mut TimegrapherApp,
    base: &Path,
) -> std::sync::mpsc::Sender<Event> {
    let path = base.join("empty-input.wav");
    hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap()
    .finalize()
    .unwrap();
    let mut capture = capture::replay_file(&path).unwrap();
    // End is sent after audio::stream closes the input. Wait before
    // replacing rx so Windows cleanup cannot race that reader thread.
    assert!(matches!(
        capture
            .rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap(),
        Event::End
    ));
    let (tx, rx) = std::sync::mpsc::channel();
    capture.rx = rx;
    capture.is_file = false;
    app.bench.recording.capture = Some(capture);
    app.bench.recording.opened_at = Instant::now();
    tx
}

/// A recording of `seconds` written to `dir/name`, opened and analysed
/// whole in a new app.
pub(super) fn opened_recording(dir: &Path, name: &str, seconds: f64) -> TimegrapherApp {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    if !path.exists() {
        let cfg = SynthConfig {
            duration_s: seconds,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 280.0, |_| 0.0);
        timegrapher_core::audio::write_wav(&path, &audio).unwrap();
    }
    reopened(&path)
}

pub(super) fn reopened(path: &Path) -> TimegrapherApp {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.start_batch(path);
    let t = Instant::now();
    while app.bench.recording.batch.is_some() && t.elapsed().as_secs() < 120 {
        app.poll();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(app.bench.recording.live.is_some(), "{:?}", app.message);
    app
}
