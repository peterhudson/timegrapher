//! Where a take's readings hear no watch: the stretches whose beats are
//! the stand's noise, not the watch's, so the strip draws them as possible
//! beats and fits its width to the watch's alone (#2177).

use super::TrendReading;

/// The stretches of a take from 0 to `end_s` that no reading of `trend`
/// with a watch heard ([`TrendReading::heard`]) covers, in time order. A
/// reading at `t` covers the `average_s` before it; two readings in a row
/// with a watch heard cover all between them too, however far apart a
/// long take's trend spaces them. What is after the last reading counts
/// as that reading does. With no reading at all, nothing is known, so none.
pub fn unheard_spans(trend: &[TrendReading], average_s: f64, end_s: f64) -> Vec<(f64, f64)> {
    let Some(last) = trend.last() else {
        return Vec::new();
    };
    let mut heard: Vec<(f64, f64)> = Vec::new();
    let mut run: Option<(f64, f64)> = None;
    for p in trend {
        run = match (run, p.heard) {
            (Some((a, _)), true) => Some((a, p.t)),
            (None, true) => Some(((p.t - average_s).max(0.0), p.t)),
            (Some(r), false) => {
                heard.push(r);
                None
            }
            (None, false) => None,
        };
    }
    if let Some((a, _)) = run {
        heard.push((a, end_s.max(last.t)));
    }
    let mut out = Vec::new();
    let mut from = 0.0;
    for (a, b) in heard {
        if a > from {
            out.push((from, a));
        }
        from = from.max(b);
    }
    if from < end_s {
        out.push((from, end_s));
    }
    out
}

/// Whether `t` lies in one of `spans` ([`unheard_spans`]): a beat there
/// is a possible beat, not the watch's.
pub fn in_spans(spans: &[(f64, f64)], t: f64) -> bool {
    spans.iter().any(|&(a, b)| t >= a && t <= b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t: f64, heard: bool) -> TrendReading {
        TrendReading {
            t,
            rate_s_per_day: None,
            amplitude_deg: None,
            amplitude_even_deg: None,
            amplitude_odd_deg: None,
            beat_error_unlock_ms: None,
            beat_error_ms: None,
            rate_error_s_per_day: None,
            amplitude_error_deg: None,
            beat_error_unlock_error_ms: None,
            heard,
        }
    }

    /// The stand, the watch, the stand again (#2177): the stand's noise at
    /// both ends is unheard, the watch's stretch from a reading's look back
    /// before its first heard reading to its last is not.
    #[test]
    fn the_stand_at_both_ends_is_unheard() {
        let trend: Vec<_> = (1..=230)
            .map(|s| at(s as f64, (81..=171).contains(&s)))
            .collect();
        let spans = unheard_spans(&trend, 10.0, 230.0);
        assert_eq!(spans, vec![(0.0, 71.0), (171.0, 230.0)]);
        assert!(in_spans(&spans, 40.0) && in_spans(&spans, 200.0));
        assert!(!in_spans(&spans, 75.0) && !in_spans(&spans, 170.0));
    }

    /// A long take's trend, readings an hour apart: two heard in a row
    /// cover all between them.
    #[test]
    fn heard_readings_far_apart_cover_between() {
        let trend = [at(3600.0, true), at(7200.0, true), at(10800.0, false)];
        let spans = unheard_spans(&trend, 10.0, 10800.0);
        assert_eq!(spans, vec![(0.0, 3590.0), (7200.0, 10800.0)]);
    }

    /// Heard to the end: nothing after the last reading is unheard; with no
    /// readings nothing is known; none heard, all of it is unheard.
    #[test]
    fn the_ends() {
        let heard = [at(10.0, true), at(20.0, true)];
        assert!(unheard_spans(&heard, 10.0, 25.0).is_empty());
        assert!(unheard_spans(&[], 10.0, 25.0).is_empty());
        let none = [at(10.0, false), at(20.0, false)];
        assert_eq!(unheard_spans(&none, 10.0, 25.0), vec![(0.0, 25.0)]);
    }
}
