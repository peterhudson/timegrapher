//! Why a live reading's beat error comes from the drop (#1439).

use super::tests::viewer_phase_log;
use super::*;

/// A live reading whose unlock windows disagree in sign beyond their
/// spread, two in five at +0.6 ms and the rest at −0.9 ms (#1439),
/// gives the drop's beat error and says the unlock was inconsistent
/// (Trevor's mutant on #1458).
#[test]
fn a_live_reading_says_an_inconsistent_unlock_falls_back_to_the_drop() {
    use crate::measure::DropWhy;
    use crate::session::BeatErrorSource;
    let mut log = viewer_phase_log(10.0, 10.0);
    log.amplitude_windows = (0..60)
        .map(|i| AmplitudeWindow {
            start_s: i as f64 * 2.0,
            end_s: (i + 1) as f64 * 2.0,
            even_deg: Some(270.0),
            odd_deg: Some(270.0),
            beat_error_ms: Some(0.6),
            beat_error_unlock_ms: Some(match i % 5 < 3 {
                true => -0.9 - i as f64 * 1e-4,
                false => 0.6 + i as f64 * 1e-4,
            }),
            even_unlock_ms: None,
            odd_unlock_ms: None,
            even_drop_ms: None,
            odd_drop_ms: None,
            extra: None,
            sound_1: [f32::NAN; 2],
        })
        .collect();
    let live = LiveAnalyzer::from_log(log, LiveConfig::default());
    let r = live.reading_at(120.0, 100.0);
    assert_eq!(r.beat_error_unlock_ms, None, "{r:?}");
    assert_eq!(r.beat_error_source, Some(BeatErrorSource::Drop));
    assert!(r.caveats.contains(&Caveat::BeatErrorFromDrop {
        why: DropWhy::UnlockInconsistent
    }));
}
