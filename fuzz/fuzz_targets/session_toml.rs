//! A take's `session.toml`, parsed and checked as every reader does
//! (`Manifest::validate_analysis_inputs`), then each recording's times,
//! masks and settings worked out from it as an opened take's are.
#![no_main]
use libfuzzer_sys::fuzz_target;
use timegrapher_core::session::{masked_spans, Manifest};
use timegrapher_core::{analysis_take, take};

fuzz_target!(|text: &str| {
    let Ok(m) = toml::from_str::<Manifest>(text) else {
        return;
    };
    if m.validate_analysis_inputs().is_err() {
        return;
    }
    let _ = take::stream_settings(&m, None, take::Given::default());
    for duration_s in [0.0, 60.0, 86_400.0] {
        let _ = analysis_take::shape_stretches(&m, duration_s);
        for e in &m.recordings {
            let _ = take::reads_nothing(e);
            let _ = take::entry_times(e, &m);
            let _ = take::span_of(e, duration_s);
            let _ = masked_spans(&e.masked);
        }
    }
});
