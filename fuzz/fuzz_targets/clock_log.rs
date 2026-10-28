//! A clock log, parsed at the sound formats a take is recorded in and
//! fitted as `timegrapher clock` and an opened take fit one.
#![no_main]
use libfuzzer_sys::fuzz_target;
use timegrapher_core::clock::{parse_log, ClockFit};

fuzz_target!(|data: &[u8]| {
    let Some((&format, text)) = data.split_first() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(text) else {
        return;
    };
    let rate = [8_000, 44_100, 48_000, 96_000][(format & 3) as usize];
    let bytes_per_frame = [2, 4, 6, 8][(format >> 2 & 3) as usize];
    let Ok(pairs) = parse_log(text, rate, bytes_per_frame) else {
        return;
    };
    for fit in [ClockFit::new(&pairs), ClockFit::for_rates(&pairs)] {
        if let Ok(fit) = fit {
            let _ = fit.unsure();
            let _ = fit.map(0.0);
            let _ = fit.map(pairs.last().map_or(0.0, |p| p.0));
        }
    }
});
