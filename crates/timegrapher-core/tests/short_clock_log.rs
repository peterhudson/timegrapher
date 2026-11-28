//! A take Stopped within a second or two, whose clock log is too short to
//! fit (#2334).

use std::path::Path;
use timegrapher_core::clock::{self, ClockError, ClockFit};
use timegrapher_core::recorder::CLOCK_EARLY_S;
use timegrapher_core::take::{clock, read_clock};

/// Two clock log lines a second apart, the card on time.
fn two_lines() -> String {
    "unix_s,frames\n1790000000.0,0\n1790000001.0,48000\n".to_string()
}

/// A take Stopped within a second or two has two clock lines, too few to
/// fit: it reads on the card's clock, as one with no log, and opens with
/// no fault. Over a longer take, two lines are a fault.
#[test]
fn a_take_too_short_for_its_clock_log_reads_on_the_card_clock() {
    let two = two_lines();
    let pairs = clock::parse_log(&two, 48_000, 2).unwrap();
    assert!(matches!(
        ClockFit::for_rates(&pairs),
        Err(ClockError::TooFewPoints(2))
    ));
    for usb in [None, Some(12.0)] {
        for short in [0.3, 1.0, CLOCK_EARLY_S] {
            let fit = clock(Some(&two), 48_000, 2, usb, None, short);
            assert!(fit.unwrap().is_none(), "{usb:?} at {short} s");
        }
        let long = clock(Some(&two), 48_000, 2, usb, None, CLOCK_EARLY_S + 0.5);
        assert_eq!(
            long.unwrap_err(),
            "clock log has 2 usable entries; need 3",
            "{usb:?}"
        );
    }
    let read = read_clock(
        (Some(&two), Some(Path::new("/somewhere/clock.csv"))),
        (48_000, 2),
        1.0,
        || Ok(None),
        || Ok(None),
    )
    .unwrap();
    assert!(read.fit.is_none() && read.from.is_none() && read.source.is_none());
}
