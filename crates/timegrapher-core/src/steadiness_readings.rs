//! The readings the steadiness tests read, and the isochronism read from
//! them: shared by [`super::check`] and [`isochronism_alone`], so a pane
//! that shows only the isochronism reads it as the whole check does,
//! without the long-run analysis and the tests (#1830).

use super::*;

/// The tests' settings as the panes run them: they name only cycles that
/// meet Holm's (#659); the report states the setting it ran with.
pub(super) fn pane_config(cfg: &Config) -> Config {
    Config {
        acf_alone: false,
        ..cfg.clone()
    }
}

/// The series the tests read, on the take's one tiling (#636).
pub(super) struct SeriesReadings {
    pub(super) wins: Vec<timing::WindowFit>,
    pub(super) rate_grid: Grid,
    pub(super) amp_grid: Grid,
    pub(super) be_grid: Grid,
    pub(super) selected_be: crate::session::BeatErrorSelection,
    pub(super) amp_readings: Grid,
    pub(super) be_readings: Grid,
    pub(super) amp_paired: Grid,
    pub(super) amp_noise: Vec<f64>,
}

/// The rate's readings of `r` seconds and the amplitude and beat error
/// windows, cut at `breaks`, on `clock` when there is one.
pub(super) fn series_readings(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[crate::breaks::Break],
    r: f64,
) -> SeriesReadings {
    let wins = rate_windows(log, clock, breaks, r);
    let rate_grid = rate_grid(&wins, r);
    let amp_grid = windows_grid(log, clock, breaks, &|w| w.mean());
    let (be_grid, selected_be) = beat_error::grid(log, clock, breaks, None);
    // The tests read every series at the rate's reading length: the
    // amplitude and beat error windows are averaged into the rate's
    // readings, so one reading means the same time for all three. The
    // short windows are kept for finding cycles.
    let [amp_readings, be_readings] = into_readings(&rate_grid, &amp_grid, &be_grid, r);
    let (amp_paired, amp_noise) = pairing_readings(&amp_grid, &amp_readings);
    SeriesReadings {
        wins,
        rate_grid,
        amp_grid,
        be_grid,
        selected_be,
        amp_readings,
        be_readings,
        amp_paired,
        amp_noise,
    }
}

/// The isochronism curve and its walk over time, from the readings `s`.
pub(super) fn isochronism_of(
    s: &SeriesReadings,
    cfg: &Config,
) -> (
    Option<IsochronismCurve>,
    Option<crate::walk::IsochronismWalk>,
) {
    (
        isochronism_curve(&s.rate_grid, &s.amp_paired, &s.amp_noise, cfg),
        crate::walk::walk(&s.rate_grid, &s.amp_grid, cfg.wind_h),
    )
}

/// The isochronism [`super::check`] gives (its `isochronism` and `walk`),
/// read without the long-run analysis or the tests: `breaks` are the cuts
/// the long-run analysis would be given.
pub fn isochronism_alone(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[crate::breaks::Break],
    cfg: &Config,
) -> (
    Option<IsochronismCurve>,
    Option<crate::walk::IsochronismWalk>,
) {
    let cfg = &pane_config(cfg);
    isochronism_of(
        &series_readings(log, clock, breaks, cfg.rate_reading_s),
        cfg,
    )
}
