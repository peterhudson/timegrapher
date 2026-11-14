//! A test's stops borrow the noise for their ± only from its stops, never
//! from the handling between them (#1624, Norm's ruling).

use timegrapher_core::audio::write_wav;
use timegrapher_core::session::{self, Position};
use timegrapher_core::stream::{analyze_file, BeatLog, StreamConfig};
use timegrapher_core::synth::{generate, SynthConfig};

/// The eight upright stops of 50 s, 30 s of turning between them, as a
/// Dynamic Poise take: too few rate or amplitude values in each for its
/// own ±.
fn stops() -> Vec<(Position, f64, f64)> {
    Position::UPRIGHT
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let a = 5.0 + 80.0 * i as f64;
            (p, a, a + 50.0)
        })
        .collect()
}

/// Whether `t` is in a turn between stops.
fn turning(t: f64) -> bool {
    !stops().iter().any(|&(_, a, b)| t >= a - 2.0 && t < b + 2.0)
}

/// A take of the three stops whose turns are `loud`: the rate thrown about
/// by the handling and the amplitude dropping, as a hand on the case does.
fn take(loud: bool, tag: &str) -> BeatLog {
    let cfg = SynthConfig {
        duration_s: 645.0,
        snr_db: 26.0,
        seed: 1624,
        ..Default::default()
    };
    let path = std::env::temp_dir().join(format!("tg-borrow-{tag}-{}.wav", std::process::id()));
    let handled = |t: f64| loud && turning(t);
    let amp = |t: f64| {
        if handled(t) {
            200.0 + 60.0 * (t * 1.7).sin()
        } else {
            280.0
        }
    };
    let rate = |t: f64| {
        if handled(t) {
            150.0 * (t * 0.9).sin()
        } else {
            0.0
        }
    };
    write_wav(&path, &generate(&cfg, amp, rate)).unwrap();
    let log = analyze_file(&path, &StreamConfig::default(), |_| {}).unwrap();
    std::fs::remove_file(&path).ok();
    log
}

/// A middle stop's rate and amplitude SEs, as a test's table reads it
/// (`read`: its stops) or as a reading of that stop alone in the take.
fn ses(log: &BeatLog, as_test: bool) -> (f64, f64) {
    let stops = stops();
    let (p, a, b) = stops[3];
    let m = if as_test {
        let rows = session::position_rows(&[p], (log, None, &[], &[]), &stops, 30.0);
        rows[0].measurement.clone().expect("read")
    } else {
        session::measure_masked(log, None, &[], &[], a, b)
    };
    let c = m.confidence.expect("a watch heard");
    (
        c.rate.se.expect("rate ±"),
        c.amplitude.se.expect("amplitude ±"),
    )
}

#[test]
fn loud_handling_between_stops_does_not_grow_a_stops_pm() {
    let quiet = take(false, "quiet");
    let loud = take(true, "loud");
    let (rate_q, amp_q) = ses(&quiet, true);
    let (rate_l, amp_l) = ses(&loud, true);
    // The stops are as steady either way: the handling does not grow
    // their ± (the margin is for the stops' own noise, which falls
    // differently once the turns have moved the beats).
    assert!(rate_l <= 1.25 * rate_q, "{rate_l} vs {rate_q}");
    assert!(amp_l <= 1.25 * amp_q, "{amp_l} vs {amp_q}");
    // Read alone, borrowing from the take around it, the stop's ± takes
    // the handling in: what the rule keeps out.
    let (rate_alone, amp_alone) = ses(&loud, false);
    assert!(rate_alone > 1.5 * rate_q, "{rate_alone} vs {rate_q}");
    // The amplitude's against the same loud stop read as a test (Peter's
    // card on #1672): against the quiet take it was 4.4x only while a
    // window with no value split its stretch and inflated the ± (#1637).
    assert!(amp_alone > 1.5 * amp_l, "{amp_alone} vs {amp_l}");
}

/// One stretch is a plain take, not a test: a caller passing its own span
/// as the stops reads exactly what the plain reading gives (Trevor on
/// #1700), with the midpoint rule and the take around it to borrow from.
#[test]
fn one_span_reads_as_a_plain_take() {
    let log = take(true, "one");
    let (_, a, b) = stops()[3];
    let plain = session::measure_masked(&log, None, &[], &[], a, b);
    // The stop's start falls inside a window (its edges are not on the
    // window grid), so whole-inside and the midpoint rule would differ.
    let one = session::measure_reading(&log, None, &[], &[], (a, b), &[(a, b)]);
    assert_eq!(format!("{one:?}"), format!("{plain:?}"));
    let span = [(a, b)];
    let whole = session::measure_together(&log, None, &[], &[], (&span, &span));
    assert_eq!(format!("{:?}", whole.expect("read")), format!("{plain:?}"));
    let c = one.confidence.expect("a watch heard");
    assert!(c.amplitude.se.is_some(), "amplitude ± kept");
}
