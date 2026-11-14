//! Source-aware position and session aggregates, sharing the legacy estimator.
use super::*;

/// Position, rate, amplitude, unsigned beat error, and its known source.
pub type SourcedFigures = (
    Position,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<BeatErrorSource>,
);

/// Position averages and state indices, choosing one known beat-error source.
/// Values with unknown provenance remain available to callers as individual rows,
/// but never enter a pooled beat-error figure.
pub fn sourced_indices(wind_h: f64, figures: &[SourcedFigures]) -> StateIndices {
    sourced_indices_over(wind_h, figures, &Position::ALL)
}

/// [`sourced_indices`] over `over`, the positions a test takes in: the
/// six for the Positions Test and `session`, the eight upright stops
/// ([`Position::UPRIGHT`]) for a Dynamic Poise test. Figures of other
/// positions are left out of the means.
pub fn sourced_indices_over(
    wind_h: f64,
    figures: &[SourcedFigures],
    over: &[Position],
) -> StateIndices {
    let positions: Vec<PositionValue> = over
        .iter()
        .filter_map(|&p| {
            let rs: Vec<&SourcedFigures> = figures.iter().filter(|f| f.0 == p).collect();
            if rs.is_empty() {
                return None;
            }
            let selected = select_beat_errors(rs.iter().map(|r| r.3.zip(r.4)));
            let values = selected.values.iter().map(|(_, value)| *value);
            Some(PositionValue {
                position: p,
                rate_s_per_day: mean_of(rs.iter().filter_map(|f| f.1)),
                amplitude_deg: mean_of(rs.iter().filter_map(|f| f.2)),
                beat_error_ms: mean_of(values),
                beat_error_source: selected.beat_error_source,
                beat_error_sources: selected.beat_error_sources,
                caveats: source_caveats(&selected, rs.iter().copied()),
                readings: rs.len(),
            })
        })
        .collect();
    let rate = |pred: &dyn Fn(Position) -> bool| -> Vec<f64> {
        positions
            .iter()
            .filter(|v| pred(v.position))
            .filter_map(|v| v.rate_s_per_day)
            .collect()
    };
    let amp = |pred: &dyn Fn(Position) -> bool| -> Vec<f64> {
        positions
            .iter()
            .filter(|v| pred(v.position))
            .filter_map(|v| v.amplitude_deg)
            .collect()
    };
    let all = |_: Position| true;
    let vert = |p: Position| p.is_vertical();
    let hor = |p: Position| !p.is_vertical();
    let xh = mean_of(rate(&hor).into_iter());
    let xv = mean_of(rate(&vert).into_iter());
    let ah = mean_of(amp(&hor).into_iter());
    let av = mean_of(amp(&vert).into_iter());
    let get = |p: Position| {
        positions
            .iter()
            .find(|v| v.position == p)
            .and_then(|v| v.rate_s_per_day)
    };
    let selected = select_beat_errors(
        positions
            .iter()
            .map(|p| p.beat_error_ms.zip(p.beat_error_source)),
    );
    let beat_errors: Vec<_> = selected.values.iter().map(|(_, value)| *value).collect();
    StateIndices {
        wind_h,
        x: mean_of(rate(&all).into_iter()),
        xh,
        xv,
        d_rate: spread(rate(&all).into_iter()),
        d_amplitude: spread(amp(&all).into_iter()),
        dv_rate: spread(rate(&vert).into_iter()),
        dh_rate: spread(rate(&hor).into_iter()),
        dvh_rate: xv.zip(xh).map(|(v, h)| v - h),
        dvh_amplitude: av.zip(ah).map(|(v, h)| v - h),
        di: get(Position::H6).zip(get(Position::CH)).map(|(a, b)| a - b),
        amplitude_mean: mean_of(amp(&all).into_iter()),
        beat_error_mean: mean_of(beat_errors.iter().copied()),
        d_beat_error: spread(beat_errors.iter().copied()),
        beat_error_source: selected.beat_error_source,
        beat_error_sources: selected.beat_error_sources,
        caveats: source_caveats(&selected, figures.iter()),
        positions,
    }
}

impl StateIndices {
    /// Words for a position-table mean, including the actual source count and
    /// why legacy unknown-source readings do not contribute.
    pub fn beat_error_summary(&self) -> String {
        let mut text = match self.beat_error_source {
            Some(source) => {
                let used = match source {
                    BeatErrorSource::Unlock => self.beat_error_sources.unlock,
                    BeatErrorSource::Drop => self.beat_error_sources.drop,
                };
                format!(
                    "{}: mean of {used} of {} positions.",
                    beat_error_source_label(Some(source)),
                    self.positions.len()
                )
            }
            None => format!("{}: no mean.", beat_error_source_label(None)),
        };
        for caveat in &self.caveats {
            if matches!(
                caveat,
                crate::measure::Caveat::BeatErrorUnknownSource { .. }
            ) {
                if let Some(note) = caveat.long(None) {
                    text.push(' ');
                    text.push_str(&note);
                }
            }
        }
        text
    }
}

fn source_caveats<'a>(
    selected: &BeatErrorSelection,
    figures: impl Iterator<Item = &'a SourcedFigures>,
) -> Vec<crate::measure::Caveat> {
    let mut caveats = selected.caveats();
    let readings = figures
        .filter(|f| f.4.is_none() && f.3.is_some_and(f64::is_finite))
        .count();
    if readings > 0 {
        caveats.push(crate::measure::Caveat::BeatErrorUnknownSource { readings });
    }
    caveats
}

/// The least time in a position, seconds, before a take's Positions Test
/// table reads it: one whole 10 s reading.
pub const POSITION_MIN_S: f64 = 10.0;

/// One position's row of a take's Positions Test table.
#[derive(Debug, Clone)]
pub struct PositionRow {
    pub position: Position,
    /// Seconds marked in the position, after each stretch's settling and
    /// less masked time.
    pub duration_s: f64,
    /// Its reading, once `duration_s` reaches the least asked for.
    pub measurement: Option<Measurement>,
}

/// A take's Positions Test table: a row for each of the six positions,
/// in [`Position::ALL`]'s order, and the values over the positions read.
#[derive(Debug, Clone)]
pub struct PositionsTable {
    pub rows: Vec<PositionRow>,
    pub indices: StateIndices,
}

/// The Positions Test table of any take, live or opened, guided or not:
/// each position's settled `stretches` (position, start, end, seconds on
/// the sound card's clock, settling already trimmed from each start) read
/// together as one reading, as [`measure_masked`] reads a take: the time
/// between them and the `masked` stretches (cleaned with [`masked_spans`])
/// left out. A position is read once its duration reaches `min_s`; the
/// indices are over the positions read.
pub fn positions_table(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[Break],
    masked: &[(f64, f64)],
    stretches: &[(Position, f64, f64)],
    min_s: f64,
) -> PositionsTable {
    let rows = position_rows(
        &Position::ALL,
        (log, clock, breaks, masked),
        stretches,
        min_s,
    );
    let figures: Vec<SourcedFigures> = rows
        .iter()
        .filter_map(|r| {
            let m = r.measurement.as_ref()?;
            Some((
                r.position,
                m.rate_s_per_day,
                m.amplitude_deg,
                m.beat_error(),
                m.beat_error_source,
            ))
        })
        .collect();
    PositionsTable {
        indices: sourced_indices(0.0, &figures),
        rows,
    }
}

/// A take and how it is read: its beats, its clock, its breaks and its
/// masked stretches.
pub type TakeRead<'a> = (
    &'a BeatLog,
    Option<&'a ClockFit>,
    &'a [Break],
    &'a [(f64, f64)],
);

/// A row for each of `over`, in its order, read as [`positions_table`]
/// reads each position: its settled `stretches` together, once their
/// duration reaches `min_s`. The Dynamic Poise pane reads a take's eight
/// upright stops ([`Position::UPRIGHT`]) by it.
pub fn position_rows(
    over: &[Position],
    (log, clock, breaks, masked): TakeRead<'_>,
    stretches: &[(Position, f64, f64)],
    min_s: f64,
) -> Vec<PositionRow> {
    over.iter()
        .map(|&p| {
            let mut spans: Vec<(f64, f64)> = stretches
                .iter()
                .filter(|s| s.0 == p)
                .map(|&(_, a, b)| (a.max(0.0), b.min(log.duration_s)))
                .filter(|(a, b)| a < b)
                .collect();
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            let duration_s = spans
                .iter()
                .map(|&(a, b)| b - a - masked_within(masked, a, b))
                .sum::<f64>()
                .max(0.0);
            let measurement = (duration_s >= min_s)
                .then(|| measure_together(log, clock, breaks, masked, &spans))
                .flatten();
            PositionRow {
                position: p,
                duration_s,
                measurement,
            }
        })
        .collect()
}

/// What Im is called wherever a comparison names it: the app's table,
/// its CSV and the text report.
pub const IM_NAME: &str = "Largest change in rate, crown right left out (Im)";
/// What Ie is called, likewise.
pub const IE_NAME: &str = "Change in the mean rate, a size (Ie)";

/// The full-wind test against a later one, `hours` on: the figures
/// `session` gives for the two states of wind.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    /// Hours between the two tests' starts.
    pub hours: f64,
    /// Each position of the later test measured in both: rate at full
    /// wind, rate after `hours` and the change (s/d), then the same for
    /// amplitude (degrees).
    pub rows: Vec<(Position, [Option<f64>; 6])>,
    /// The largest change in rate over the positions, crown right (12H)
    /// left out ([`IM_NAME`]), s/d.
    pub im: Option<f64>,
    /// The size of the change in the mean rate over the positions
    /// measured in both ([`IE_NAME`]), s/d.
    pub ie: Option<f64>,
}

/// Full wind (`full`) against `later`, `hours` on, read by
/// [`evaluate_states`] as `session` reads a file with both. None when
/// either is empty.
pub fn comparison(
    full: &[SourcedFigures],
    hours: f64,
    later: &[SourcedFigures],
) -> Option<Comparison> {
    if full.is_empty() || later.is_empty() {
        return None;
    }
    let states = vec![sourced_indices(0.0, full), sourced_indices(hours, later)];
    let report = evaluate_states(states, Vec::new());
    let (first, last) = (report.states.first()?, report.states.last()?);
    let change = |x: Option<f64>, y: Option<f64>| y.zip(x).map(|(y, x)| y - x);
    let rows = later
        .iter()
        .filter_map(|f| {
            let p = f.0;
            let a = first.positions.iter().find(|v| v.position == p)?;
            let b = last.positions.iter().find(|v| v.position == p)?;
            Some((
                p,
                [
                    a.rate_s_per_day,
                    b.rate_s_per_day,
                    change(a.rate_s_per_day, b.rate_s_per_day),
                    a.amplitude_deg,
                    b.amplitude_deg,
                    change(a.amplitude_deg, b.amplitude_deg),
                ],
            ))
        })
        .collect();
    Some(Comparison {
        hours,
        rows,
        im: report.im,
        ie: report.ie,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(p: Position, value: f64, source: BeatErrorSource) -> SourcedFigures {
        (p, Some(1.0), Some(270.0), Some(value), Some(source))
    }
    #[test]
    fn position_and_state_aggregates_never_mix_sources() {
        let s = sourced_indices(
            0.0,
            &[
                row(Position::CH, 0.2, BeatErrorSource::Unlock),
                row(Position::CH, 0.4, BeatErrorSource::Unlock),
                row(Position::CH, 8.0, BeatErrorSource::Drop),
                row(Position::CB, 9.0, BeatErrorSource::Drop),
            ],
        );
        let du = s
            .positions
            .iter()
            .find(|p| p.position == Position::CH)
            .unwrap();
        assert!((du.beat_error_ms.unwrap() - 0.3).abs() < 1e-12);
        assert_eq!(
            du.beat_error_sources,
            BeatErrorSources { unlock: 2, drop: 1 }
        );
        assert!(matches!(
            du.caveats.as_slice(),
            [crate::measure::Caveat::BeatErrorSourceReadings {
                used: 2,
                readings: 3,
                counted: crate::measure::Counted::Readings,
                ..
            }]
        ));
        assert!((s.beat_error_mean.unwrap() - 0.3).abs() < 1e-12);
        assert_eq!(s.beat_error_source, Some(BeatErrorSource::Unlock));
        assert_eq!(
            s.beat_error_sources,
            BeatErrorSources { unlock: 1, drop: 1 }
        );
        assert_eq!(
            s.d_beat_error, None,
            "one chosen-source position cannot give a spread"
        );
    }
    #[test]
    fn drop_only_position_statistics_keep_their_source() {
        let s = sourced_indices(
            0.0,
            &[
                row(Position::CH, 0.4, BeatErrorSource::Drop),
                row(Position::CB, 0.8, BeatErrorSource::Drop),
            ],
        );
        assert_eq!(s.beat_error_source, Some(BeatErrorSource::Drop));
        assert!((s.beat_error_mean.unwrap() - 0.6).abs() < 1e-12);
        assert_eq!(s.d_beat_error, Some(0.4));
        assert!(matches!(
            s.caveats.as_slice(),
            [crate::measure::Caveat::FROM_DROP]
        ));
    }
    #[test]
    fn unknown_source_rows_never_enter_beat_error_aggregates() {
        let figures = [
            (Position::CH, Some(2.0), Some(280.0), Some(0.4), None),
            (Position::CB, Some(4.0), Some(260.0), Some(0.8), None),
        ];
        let s = sourced_indices(0.0, &figures);
        assert_eq!(s.beat_error_mean, None);
        assert_eq!(s.d_beat_error, None);
        assert_eq!(s.beat_error_source, None);
        assert_eq!(s.x, Some(3.0));
        assert_eq!(s.amplitude_mean, Some(270.0));
        assert!(s.positions.iter().all(|p| p.beat_error_ms.is_none()));
        assert!(matches!(
            s.caveats.as_slice(),
            [crate::measure::Caveat::BeatErrorUnknownSource { readings: 2 }]
        ));
        let mixed = [figures[0], row(Position::CH, 0.2, BeatErrorSource::Unlock)];
        let s = sourced_indices(0.0, &mixed);
        assert_eq!(s.beat_error_mean, Some(0.2));
        assert_eq!(s.beat_error_source, Some(BeatErrorSource::Unlock));
        assert!(matches!(
            s.positions[0].caveats.as_slice(),
            [crate::measure::Caveat::BeatErrorUnknownSource { readings: 1 }]
        ));
    }
    #[test]
    fn position_mean_words_count_the_selected_source_and_explain_unknowns() {
        let known = sourced_indices(
            0.0,
            &[
                row(Position::CH, 0.2, BeatErrorSource::Unlock),
                row(Position::CB, 0.8, BeatErrorSource::Drop),
                row(Position::H3, 0.4, BeatErrorSource::Unlock),
            ],
        );
        assert_eq!(
            known.beat_error_summary(),
            "Beat Error from the Unlock: mean of 2 of 3 positions."
        );
        let unknown = sourced_indices(0.0, &[(Position::CH, None, None, Some(0.4), None)]);
        assert!(unknown.beat_error_summary().contains("source not recorded"));
        assert!(unknown.beat_error_summary().contains("no mean"));
        assert!(unknown
            .beat_error_summary()
            .contains("leaves out 1 reading whose"));
    }
}
