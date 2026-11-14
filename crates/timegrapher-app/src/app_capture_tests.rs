//! Listening, headless: sound taken in slices without frames, and the
//! gaps in a live input that core's gap rule makes breaks.

use super::tests::{queued_capture, save_failure_fixture};
use super::*;
use std::sync::mpsc::Sender;
use timegrapher_core::synth::{generate, SynthConfig};

/// Pause and Stop keep the sound the input had already sent: the blocks
/// still queued when the input closes are saved and read too (#873).
#[test]
fn pause_and_stop_keep_sound_already_queued() {
    for stop in [false, true] {
        let (mut app, base) =
            save_failure_fixture(if stop { "stop-queued" } else { "pause-queued" });
        let tx = queued_capture(&mut app, &base);
        let saved = |a: &TimegrapherApp| a.session.recorder.as_ref().unwrap().duration_s();
        let before = saved(&app);
        // 10 s in 20 ms blocks, all queued before the input closes.
        for k in 0..500u64 {
            tx.send(block(2000 + 20 * k)).unwrap();
        }
        if stop {
            app.stop_listening();
        } else {
            app.pause();
        }
        let after = saved(&app);
        drop(app);
        drop(tx);
        let _ = std::fs::remove_dir_all(base);
        assert!(
            (after - before - 10.0).abs() < 1e-6,
            "stop {stop}: 10 s were queued, {} s more were saved",
            after - before
        );
    }
}

/// An input that failed just before Pause still says why, as it would
/// while listening, and the sound before the failure is kept (#873).
#[test]
fn pause_reports_an_input_failure_already_queued() {
    let (mut app, base) = save_failure_fixture("pause-queued-error");
    let tx = queued_capture(&mut app, &base);
    let before = app.session.recorder.as_ref().unwrap().duration_s();
    for k in 0..50u64 {
        tx.send(block(2000 + 20 * k)).unwrap();
    }
    tx.send(Event::Error("device unplugged".into())).unwrap();
    app.pause();
    let after = app.session.recorder.as_ref().unwrap().duration_s();
    let fault = app.input_fault.clone();
    drop(app);
    drop(tx);
    let _ = std::fs::remove_dir_all(base);
    assert!(
        (after - before - 1.0).abs() < 1e-6,
        "{} s more were saved",
        after - before
    );
    assert!(fault.is_some_and(|f| f.starts_with("Input stopped.")));
}

/// With the window drawing no frames at all (a locked screen, a
/// minimised window), the sound is still saved and read as it comes in.
#[test]
fn listening_carries_on_without_frames() {
    let (mut app, base) = save_failure_fixture("no-frames");
    let tx = queued_capture(&mut app, &base);
    let before = app.heard_s();
    let saved_before = app.session.recorder.as_ref().unwrap().duration_s();
    let shell = crate::pump::Shell::new(app);
    // 10 s of sound in the 20 ms blocks a microphone sends: several
    // passes of the listening thread, and quick enough in a debug build
    // run alongside the other tests.
    for k in 0..500u64 {
        tx.send(Event::Audio(timegrapher_core::capture::Block {
            samples: vec![0.0; 960],
            at: std::time::UNIX_EPOCH + Duration::from_millis(2000 + 20 * k),
        }))
        .unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let (heard, saved) = {
            let a = shell.app();
            (
                a.heard_s(),
                a.session.recorder.as_ref().unwrap().duration_s(),
            )
        };
        if heard - before >= 10.0 - 1e-6 && saved - saved_before >= 10.0 - 1e-6 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "heard {heard} s and saved {saved} s without frames"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(shell);
    drop(tx);
    let _ = std::fs::remove_dir_all(base);
}

/// A backlog is taken a frame's worth at a time, so the window never
/// freezes while it catches up.
#[test]
fn a_backlog_is_taken_in_slices() {
    let (mut app, base) = save_failure_fixture("backlog");
    let tx = queued_capture(&mut app, &base);
    for k in 0..3000u64 {
        tx.send(Event::Audio(timegrapher_core::capture::Block {
            samples: vec![0.0; 960],
            at: std::time::UNIX_EPOCH + Duration::from_millis(2000 + 20 * k),
        }))
        .unwrap();
    }
    let first = app.poll_for(Duration::from_millis(1));
    assert!((1..3000).contains(&first), "took {first} blocks at once");
    let mut taken = first;
    while taken < 3000 {
        let n = app.poll_for(Duration::from_millis(200));
        assert!(n > 0);
        taken += n;
    }
    assert!(app.running());
    drop(tx);
    let _ = std::fs::remove_dir_all(base);
}

/// New Session while listening, when closing the kept sound fails:
/// the error stays shown and listening doesn't start again over it.
#[test]
fn new_session_keeps_a_close_error() {
    let (mut app, base) = save_failure_fixture("new-session-close");
    let scratch = app.session.recorder.as_ref().unwrap().dir().to_path_buf();
    std::fs::remove_file(scratch.join("session.json")).unwrap();
    std::fs::create_dir(scratch.join("session.json")).unwrap();
    let tx = queued_capture(&mut app, &base);
    assert!(app.running() && app.session.mic());
    app.new_session();
    assert!(!app.running());
    let (m, error) = app.message.clone().expect("the close error is shown");
    assert!(error, "{m}");
    assert!(m.contains("pause"), "{m}");
    drop(tx);
    let _ = std::fs::remove_dir_all(base);
}

/// A computer that slept mid-take carries on, with the gap a break.
/// Core's gap rule holds the blocks after a jump until it has seen
/// enough of them, so the sound runs on 10 s after waking (#689).
#[test]
fn a_sleep_becomes_a_break() {
    let (app, heard, base) = stalled("slept", 3_600_000);
    let breaks = app.session.recorder.as_ref().unwrap().breaks();
    assert_eq!(breaks.len(), 1);
    assert!(
        (breaks[0].gap_s - 3600.0).abs() < 0.05,
        "{}",
        breaks[0].gap_s
    );
    assert!((app.heard_s() - heard - 3610.0).abs() < 0.05);
    let (m, error) = app.message.clone().unwrap();
    assert!(!error);
    assert!(m.contains("timestamps jumped by"), "{m}");
    let _ = std::fs::remove_dir_all(base);
}

/// A stall of a second, under the 5 s the app once took for a sleep,
/// is a break by core's gap rule, as `record` lists it (#689).
#[test]
fn a_short_stall_is_a_break_by_cores_rule() {
    let (app, heard, base) = stalled("stall", 1000);
    let breaks = app.session.recorder.as_ref().unwrap().breaks();
    assert_eq!(breaks.len(), 1);
    assert!((breaks[0].gap_s - 1.0).abs() < 0.05, "{}", breaks[0].gap_s);
    assert!((app.heard_s() - heard - 11.0).abs() < 0.05);
    let _ = std::fs::remove_dir_all(base);
}

/// Sound the input dropped because its queue was full (#483) is a break
/// of just that stretch, however short, and is never spliced: the take
/// lists it, the time runs on across it, and the message says why.
#[test]
fn sound_the_input_dropped_is_a_break() {
    let (mut app, base, tx, heard) = listened("dropped");
    let saved = app.session.recorder.as_ref().unwrap().duration_s();
    // 30 ms dropped, so the next block arrives 30 ms after the last
    // one's end.
    tx.send(Event::Lost(1_440)).unwrap();
    send_after(&tx, 30, 500);
    while app.poll_for(Duration::from_secs(60)) > 0 {}
    assert!(app.running());
    let breaks = app.session.recorder.as_ref().unwrap().breaks();
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert!((breaks[0].gap_s - 0.03).abs() < 1e-9, "{}", breaks[0].gap_s);
    assert!(
        (breaks[0].audio_s - saved).abs() < 1e-9,
        "{}",
        breaks[0].audio_s
    );
    assert!((app.heard_s() - heard - 10.03).abs() < 1e-6);
    let (m, error) = app.message.clone().unwrap();
    assert!(!error);
    assert!(m.starts_with("30 ms of sound was lost"), "{m}");
    drop(tx);
    let _ = std::fs::remove_dir_all(base);
}

/// A glitch the input carries on after (a buffer underrun or overrun,
/// #1046) doesn't stop the take: it is a break of no time lost where it
/// fell, the sound either side never read across, and the message says
/// so. Two with no sound between are one.
#[test]
fn a_glitch_is_a_break_not_a_stop() {
    let (mut app, base, tx, heard) = listened("glitch");
    let saved = app.session.recorder.as_ref().unwrap().duration_s();
    tx.send(Event::Glitch).unwrap();
    tx.send(Event::Glitch).unwrap();
    send_after(&tx, 0, 500);
    while app.poll_for(Duration::from_secs(60)) > 0 {}
    assert!(app.running());
    assert!(app.input_fault.is_none());
    let breaks = app.session.recorder.as_ref().unwrap().breaks();
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].gap_s, 0.0);
    assert!((breaks[0].audio_s - saved).abs() < 1e-9, "{breaks:?}");
    assert!((app.heard_s() - heard - 10.0).abs() < 1e-6);
    let (m, error) = app.message.clone().unwrap();
    assert!(!error);
    assert_eq!(m, timegrapher_core::breaks::GLITCH_TEXT);
    drop(tx);
    let _ = std::fs::remove_dir_all(base);
}

/// Sound held by core's gap rule after a jump counts as heard, so the
/// input isn't taken for silent ("No sound is arriving") while the rule
/// waits on it (#689).
#[test]
fn sound_held_after_a_jump_is_heard() {
    let (mut app, base, tx, _) = listened("held-heard");
    // The last block heard long enough ago that silence would stop
    // listening, then half a second of sound after a 1 s jump: held.
    app.audio_at = Some(Instant::now() - Duration::from_secs(10));
    send_after(&tx, 1000, 25);
    while app.poll_for(Duration::from_secs(60)) > 0 {}
    assert!(app.running());
    assert!(app.audio_at.unwrap().elapsed() < Duration::from_secs(3));
    drop(tx);
    let _ = std::fs::remove_dir_all(base);
}

/// Stopped while core's gap rule holds sound after a jump, the take
/// keeps that sound and its break (#689).
#[test]
fn a_pause_keeps_the_sound_held_after_a_jump() {
    let (mut app, base, tx, heard) = listened("held-pause");
    send_after(&tx, 1000, 25);
    while app.poll_for(Duration::from_secs(60)) > 0 {}
    app.pause();
    let breaks = app.session.recorder.as_ref().unwrap().breaks();
    assert_eq!(breaks.len(), 1);
    assert!((breaks[0].gap_s - 1.0).abs() < 0.05, "{}", breaks[0].gap_s);
    assert!(
        (app.heard_s() - heard - 1.5).abs() < 0.05,
        "{}",
        app.heard_s()
    );
    drop(tx);
    let _ = std::fs::remove_dir_all(base);
}

/// A take listened to for 1 s, then nothing for `gap_ms` by the
/// blocks' timestamps, then 10 s more: the app after all of it, what
/// it had heard before the gap, and the take's folder.
fn stalled(name: &str, gap_ms: u64) -> (TimegrapherApp, f64, PathBuf) {
    let (mut app, base, tx, heard) = listened(name);
    send_after(&tx, gap_ms, 500);
    while app.poll_for(Duration::from_secs(60)) > 0 {}
    assert!(app.running());
    drop(tx);
    (app, heard, base)
}

/// A take listened to for 1 s of 20 ms blocks: the app, its folder, the
/// input's queue and what it has heard.
fn listened(name: &str) -> (TimegrapherApp, PathBuf, Sender<Event>, f64) {
    let (mut app, base) = save_failure_fixture(name);
    let tx = queued_capture(&mut app, &base);
    for k in 0..50 {
        tx.send(block(2000 + 20 * k)).unwrap();
    }
    // Everything queued, however long it takes.
    while app.poll_for(Duration::from_secs(60)) > 0 {}
    let heard = app.heard_s();
    assert!(app.session.recorder.as_ref().unwrap().breaks().is_empty());
    (app, base, tx, heard)
}

/// `n` blocks after [`listened`]'s, the first `gap_ms` after its last.
fn send_after(tx: &Sender<Event>, gap_ms: u64, n: u64) {
    let woke = 2000 + 20 * 49 + gap_ms;
    for k in 0..n {
        tx.send(block(woke + 20 * k)).unwrap();
    }
}

/// A 20 ms block of silence at 48 kHz stamped `ms` after the epoch.
fn block(ms: u64) -> Event {
    Event::Audio(timegrapher_core::capture::Block {
        samples: vec![0.0; 960],
        at: std::time::UNIX_EPOCH + Duration::from_millis(ms),
    })
}

/// Live's first chunk read again with the file's template takes over
/// about a minute in (#1327): the readings carry on with an amplitude, the
/// charts' points are read again with none past the newest, and the strip's
/// anchor moves with the beats by under a millisecond, so the view stays.
#[test]
fn listening_carries_on_when_the_first_chunk_read_again_takes_over() {
    let (mut app, base) = save_failure_fixture("takeover");
    let tx = queued_capture(&mut app, &base);
    let cfg = SynthConfig {
        duration_s: 80.0,
        ..Default::default()
    };
    let audio = generate(&cfg, |_| 280.0, |_| 0.0);
    let mut before = None;
    for (k, c) in audio.samples.chunks(480).enumerate() {
        tx.send(Event::Audio(timegrapher_core::capture::Block {
            samples: c.to_vec(),
            at: std::time::UNIX_EPOCH + Duration::from_millis(2000 + 10 * k as u64),
        }))
        .unwrap();
        if k % 50 == 49 {
            let live = app.live.as_ref().unwrap();
            if live.template_switch_s().is_none() {
                before = app.anchor.map(|a| (a, live.beats().to_vec()));
            }
            app.poll();
        }
    }
    // Each poll takes in what a frame's time allows: all of it, in time.
    for _ in 0..1000 {
        app.poll();
        if app.live.as_ref().unwrap().duration_s() >= 79.9 {
            break;
        }
    }
    let live = app.live.as_ref().unwrap();
    assert!(live.template_switch_s().is_some(), "not taken over");
    assert!(app.reading().and_then(|r| r.amplitude_deg).is_some());
    assert!(app.trend.windows(2).all(|w| w[0].t < w[1].t));
    // A beat 30 s in stands where it stood on the strip, within the
    // scatter of a beat's time between two templates.
    let (a0, beats0) = before.expect("an anchor before the takeover");
    let a1 = app.anchor.expect("an anchor after");
    let period = 3600.0 / live.bph().unwrap() as f64;
    let lead = |a: &timegrapher_core::timing::Anchor, beats: &[timegrapher_core::beats::Beat]| {
        let b = beats
            .iter()
            .min_by(|x, y| (x.time - 30.0).abs().total_cmp(&(y.time - 30.0).abs()));
        let b = b.unwrap();
        b.time - a.time - (b.index - a.index) as f64 * period
    };
    let moved = lead(&a1, live.beats()) - lead(&a0, &beats0);
    assert!(moved.abs() < 2e-4, "{moved}");
    drop(tx);
    let _ = std::fs::remove_dir_all(base);
}

/// While listening, the Scope reads a moment further back than the
/// engine holds from the take being recorded, at the take's own time
/// across a break: the sound kept leaves the break's gap out. It once
/// said "No sound kept" for anything but the last few seconds, though
/// the take held it.
#[test]
fn the_scope_reads_the_take_being_recorded_across_a_break() {
    let (mut app, base, tx, heard) = listened("scope-heard");
    // A second's stall, then 10 s whose blocks each carry their number.
    let woke = 2000 + 20 * 49 + 1000;
    for k in 0..500u64 {
        tx.send(Event::Audio(timegrapher_core::capture::Block {
            samples: vec![k as f32 / 1000.0; 960],
            at: std::time::UNIX_EPOCH + Duration::from_millis(woke + 20 * k),
        }))
        .unwrap();
    }
    while app.poll_for(Duration::from_secs(60)) > 0 {}
    assert_eq!(app.session.recorder.as_ref().unwrap().breaks().len(), 1);
    assert!(app.session.mic());
    // Block 250 by the take's time, which runs on across the gap.
    let resumed = heard + 1.0;
    let lo = resumed + 0.02 * 250.0 + 0.005;
    assert!(app.start_scope_job(lo, lo + 0.01), "a job to read the take");
    let (a, b, got) = app
        .scope_job
        .take()
        .unwrap()
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    assert!(a <= lo && b >= lo + 0.01);
    let sp = got.unwrap();
    let i = ((lo - sp.t0) * sp.rate).round() as usize;
    assert!((sp.mono[i] - 0.25).abs() < 2e-3, "{} at {lo}", sp.mono[i]);
    drop(tx);
    let _ = std::fs::remove_dir_all(base);
}
