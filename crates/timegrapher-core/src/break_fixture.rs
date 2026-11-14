//! A take with a break, for the tests of the views that read the beat
//! error and the readings piece by piece between a take's breaks (#1608,
//! #1723).

use crate::longterm::{self, Grid, LongConfig};
use crate::steadiness::{level_by_piece, window_grids};

/// A take with a break for the views' tests (#1723): the rub scenario,
/// paused 60 s half way, read as `long` and `series` read it, with the
/// config they read it on.
pub(crate) fn a_take_with_a_break() -> (
    crate::stream::BeatLog,
    crate::longrun::LongReport,
    LongConfig,
) {
    let s = crate::scenarios::find("rub").unwrap();
    let log = crate::scenarios::beat_log(&s, 1);
    let lc = LongConfig {
        wheels: crate::periodicity::standard_wheels(crate::scenarios::BPH, 20),
        ..Default::default()
    };
    let breaks = [crate::breaks::Break {
        audio_s: 0.5 * log.duration_s,
        gap_s: 60.0,
    }];
    let long = crate::longrun::analyse_with_breaks(&log, None, &breaks, &lc);
    assert!(!long.breaks.is_empty());
    (log, long, lc)
}

/// The beat error's windows of the take from [`a_take_with_a_break`],
/// levelled by piece and, for a test to tell the two apart, across the
/// break.
pub(crate) fn its_beat_error(
    log: &crate::stream::BeatLog,
    long: &crate::longrun::LongReport,
    lc: &LongConfig,
) -> (Grid, Grid) {
    let [_, be] = window_grids(log, long.clock.as_ref(), &long.breaks);
    (
        level_by_piece(&be, long.clock.as_ref(), &long.breaks, lc),
        longterm::detrend(&be, lc.detrend_s),
    )
}
