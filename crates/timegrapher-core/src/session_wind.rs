//! The states of wind: hours after full wind to a tenth, and wind not
//! given as a state of its own (#1665).

use super::Reading;

/// Hours after full wind to a tenth: the states of wind's one key. A
/// reading whose wind was not given is a state of its own (`None`),
/// never pooled with full wind (#1665); so is one whose wind is not a
/// number (`wind_h = nan`), which would otherwise key to 0.
pub(crate) fn wind_key(w: Option<f64>) -> Option<i64> {
    w.filter(|w| w.is_finite())
        .map(|w| (w * 10.0).round() as i64)
}

/// Whether a reading whose wind is `reading` falls in the state of wind
/// `state` (a [`StateIndices::wind_h`](super::StateIndices::wind_h)): hours to a tenth, and wind not
/// given only with wind not given.
pub fn in_wind_state(reading: Option<f64>, state: Option<f64>) -> bool {
    wind_key(reading) == wind_key(state)
}

/// The readings a session's text report is of, and the full-wind state
/// it is set against: the least wound state whose wind was given, against
/// full wind (`wind_h` 0) when that was measured too, with the later
/// state's hours since full wind. A fullest given state that is not full
/// wind is not set against anything: the report names it full wind.
/// With no wind given at all, the readings whose wind was not given, on
/// their own. Readings whose wind was not given are never set against a
/// given state, nor pooled with one (#1665).
pub fn report_states(readings: &[Reading]) -> (Vec<&Reading>, Option<(Vec<&Reading>, f64)>) {
    let at = |k: Option<i64>| -> Vec<&Reading> {
        readings
            .iter()
            .filter(|r| wind_key(r.wind_h) == k)
            .collect()
    };
    let given: Vec<i64> = wind_keys(readings).into_iter().flatten().collect();
    match (given.first(), given.last()) {
        (Some(&first), Some(&last)) => {
            let earlier =
                (first == 0 && last > first).then(|| (at(Some(first)), last as f64 / 10.0));
            (at(Some(last)), earlier)
        }
        _ => (at(None), None),
    }
}

/// The states of wind among `readings`, fullest first, then the one
/// whose wind was not given: the order every surface lists them in.
pub(crate) fn wind_keys(readings: &[Reading]) -> Vec<Option<i64>> {
    let mut keys: Vec<Option<i64>> = readings.iter().map(|r| wind_key(r.wind_h)).collect();
    keys.sort_unstable_by_key(|k| (k.is_none(), *k));
    keys.dedup();
    keys
}
