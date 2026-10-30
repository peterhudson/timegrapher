//! Listening, headless: sound taken in slices without frames, and the
//! gaps in a live input that core's gap rule makes breaks.

use super::tests::{queued_capture, save_failure_fixture};
use super::*;
use std::sync::mpsc::Sender;

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
