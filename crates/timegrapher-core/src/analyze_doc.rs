//! The body of `analyze --json` (`timegrapher.analyze/2`), worded once
//! for every surface that writes one: the command line, and the app's
//! bug report, whose excerpt `report --replay` analyses again and
//! compares. Each surface wraps it in its own envelope (schema, software,
//! input files).

use crate::analysis::{AnalysisConfig, Summary};
use serde_json::{json, Value};

/// The settings `analyze` read at, as its document's `input.settings`
/// gives them; `calibre` is the calibre named, if one was.
pub fn settings(cfg: &AnalysisConfig, calibre: Option<&str>) -> Value {
    json!({
        "calibre": calibre,
        "bph": cfg.bph,
        "lift_deg": cfg.amplitude.lift_deg,
        "notch_hz": cfg.envelope.notch_hz,
        "highpass_hz": cfg.envelope.highpass_hz,
        "escape_teeth": cfg.escape_teeth,
    })
}

/// The document's body: the summary, its rates on the sound card's clock
/// (what tg reads and the regression check compares), with the clock's
/// correction beside them when one is known (`ppm` slow, and where it came
/// from in words).
pub fn body(summary: &Summary, clock: Option<(f64, &str)>) -> Result<Value, String> {
    let mut body = serde_json::to_value(summary).map_err(|e| e.to_string())?;
    if let Some((ppm, from)) = clock {
        body["clock"] = crate::clockstore::correction_json(
            ppm,
            from,
            summary.overall.as_ref().map(|f| f.rate_s_per_day),
        );
    }
    Ok(body)
}

/// The correction `analyze` puts beside a summary with no other given:
/// the recording's own USB lines, when it has them.
pub fn usb_clock(summary: &Summary) -> Option<(f64, String)> {
    summary.usb_clock.map(|c| c.correction())
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
}
