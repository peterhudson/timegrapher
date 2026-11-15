//! Select one source before a beat-error statistic is computed.

use super::{chosen_beat_error, BeatErrorSource};
use crate::measure::{Caveat, Counted};
use serde::Serialize;

/// Counts of the sourced readings offered to an aggregate, before selection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct BeatErrorSources {
    pub unlock: usize,
    pub drop: usize,
}

/// A source and its observations, with original indices to preserve gap membership.
#[derive(Debug, Clone)]
pub struct BeatErrorSelection {
    pub beat_error_source: Option<BeatErrorSource>,
    pub beat_error_sources: BeatErrorSources,
    pub values: Vec<(usize, f64)>,
    /// What the counts and values are of.
    pub counted: Counted,
}

impl BeatErrorSelection {
    /// Factual caveats for the source choice, including observations left out.
    pub fn caveats(&self) -> Vec<Caveat> {
        let mut caveats = Vec::new();
        if let Some(source) = self.beat_error_source {
            if let Some(caveat) = source.caveat() {
                caveats.push(caveat);
            }
            let readings = self.beat_error_sources.unlock + self.beat_error_sources.drop;
            if self.values.len() < readings {
                caveats.push(Caveat::BeatErrorSourceReadings {
                    source,
                    used: self.values.len(),
                    readings,
                    counted: self.counted,
                });
            }
        }
        caveats
    }
}

/// Apply the direct reading's preference once to a set of sourced readings.
/// Missing or non-finite observations do not count. Values keep their input order;
/// an excluded observation's index is not reassigned to its neighbour.
pub fn select_beat_errors(
    readings: impl IntoIterator<Item = Option<(f64, BeatErrorSource)>>,
) -> BeatErrorSelection {
    let readings: Vec<_> = readings
        .into_iter()
        .enumerate()
        .filter_map(|(i, reading)| {
            let (value, source) = reading?;
            value.is_finite().then_some((i, value, source))
        })
        .collect();
    let beat_error_sources = BeatErrorSources {
        unlock: readings
            .iter()
            .filter(|r| r.2 == BeatErrorSource::Unlock)
            .count(),
        drop: readings
            .iter()
            .filter(|r| r.2 == BeatErrorSource::Drop)
            .count(),
    };
    let first = |source| readings.iter().find(|r| r.2 == source).map(|r| r.1);
    let beat_error_source =
        chosen_beat_error(first(BeatErrorSource::Unlock), first(BeatErrorSource::Drop))
            .map(|(_, source)| source);
    let values = readings
        .into_iter()
        .filter(|r| Some(r.2) == beat_error_source)
        .map(|(i, value, _)| (i, value))
        .collect();
    BeatErrorSelection {
        beat_error_source,
        beat_error_sources,
        values,
        counted: Counted::Readings,
    }
}

/// [`select_beat_errors`] over the amplitude's windows, each offering its
/// chosen beat error; the caveats count windows, not readings.
pub fn select_window_beat_errors<'a>(
    windows: impl IntoIterator<Item = &'a crate::amplitude::AmplitudeWindow>,
) -> BeatErrorSelection {
    BeatErrorSelection {
        counted: Counted::Windows,
        ..select_beat_errors(
            windows
                .into_iter()
                .map(|w| chosen_beat_error(w.beat_error_unlock_ms, w.beat_error_ms)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_sources_keep_only_unlocks_and_their_original_indices() {
        let selected = select_beat_errors([
            Some((8.0, BeatErrorSource::Drop)),
            Some((0.3, BeatErrorSource::Unlock)),
            None,
            Some((0.5, BeatErrorSource::Unlock)),
        ]);
        assert_eq!(selected.beat_error_source, Some(BeatErrorSource::Unlock));
        assert_eq!(
            selected.beat_error_sources,
            BeatErrorSources { unlock: 2, drop: 1 }
        );
        assert_eq!(selected.values, vec![(1, 0.3), (3, 0.5)]);
        assert!(matches!(
            selected.caveats().as_slice(),
            [Caveat::BeatErrorSourceReadings {
                source: BeatErrorSource::Unlock,
                used: 2,
                readings: 3,
                counted: Counted::Readings,
            }]
        ));
    }

    #[test]
    fn all_drop_readings_keep_their_source_and_caveat() {
        let selected = select_beat_errors([
            Some((0.6, BeatErrorSource::Drop)),
            Some((0.8, BeatErrorSource::Drop)),
        ]);
        assert_eq!(selected.beat_error_source, Some(BeatErrorSource::Drop));
        assert_eq!(
            selected.beat_error_sources,
            BeatErrorSources { unlock: 0, drop: 2 }
        );
        assert_eq!(selected.values, vec![(0, 0.6), (1, 0.8)]);
        assert!(matches!(selected.caveats().as_slice(), [Caveat::FROM_DROP]));
    }

    #[test]
    fn missing_readings_have_no_source_and_no_counts() {
        let selected = select_beat_errors([None, Some((f64::NAN, BeatErrorSource::Unlock))]);
        assert_eq!(selected.beat_error_source, None);
        assert_eq!(selected.beat_error_sources, BeatErrorSources::default());
        assert!(selected.values.is_empty());
        assert!(selected.caveats().is_empty());
    }

    #[test]
    fn a_window_selection_counts_windows() {
        let window = |unlock: Option<f64>, drop: Option<f64>| crate::amplitude::AmplitudeWindow {
            start_s: 0.0,
            end_s: 2.0,
            even_deg: None,
            odd_deg: None,
            beat_error_ms: drop,
            beat_error_unlock_ms: unlock,
            even_unlock_ms: None,
            odd_unlock_ms: None,
            even_drop_ms: None,
            odd_drop_ms: None,
            extra: None,
            sound_1: [f32::NAN; 2],
        };
        let windows = [
            window(Some(0.3), Some(8.0)),
            window(None, Some(8.0)),
            window(Some(0.5), None),
        ];
        let selected = select_window_beat_errors(&windows);
        let by_reading = select_beat_errors(
            windows
                .iter()
                .map(|w| chosen_beat_error(w.beat_error_unlock_ms, w.beat_error_ms)),
        );
        assert_eq!(selected.counted, Counted::Windows);
        assert_eq!(by_reading.counted, Counted::Readings);
        assert_eq!(selected.values, by_reading.values);
        assert_eq!(selected.values, vec![(0, 0.3), (2, 0.5)]);
        assert!(matches!(
            selected.caveats().as_slice(),
            [Caveat::BeatErrorSourceReadings {
                source: BeatErrorSource::Unlock,
                used: 2,
                readings: 3,
                counted: Counted::Windows,
            }]
        ));
    }
}
