//! The value scale a chart is drawn on, so the app and the MCP view scale
//! the same readings the same way (#513).

/// From the lowest value to the highest, padded a little, so every point
/// drawn is inside the chart: the app's charts over time. `None` with no
/// finite value.
pub fn data_range(values: &[f64]) -> Option<(f64, f64)> {
    let (lo, hi) = values
        .iter()
        .copied()
        .filter(|y| y.is_finite())
        .fold(None, |r: Option<(f64, f64)>, y| {
            Some(r.map_or((y, y), |(lo, hi)| (lo.min(y), hi.max(y))))
        })?;
    let pad = ((hi - lo) * 0.08).max(0.05);
    Some((lo - pad, hi + pad))
}

/// The 1st to 99th percentile of the values, padded a little, leaving out
/// lone wild values (a knock, a dropped beat) so that two of them don't
/// flatten the rest: those beyond three interquartile ranges of the middle
/// half, unless they are more than 3% of the values (then they are part of
/// what the watch does, as in the steadiness test). The steadiness views'
/// scale. `None` with fewer than two finite values.
pub fn spread(values: &[f64]) -> Option<(f64, f64)> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.len() < 2 {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let q = |v: &[f64], f: f64| v[((v.len() - 1) as f64 * f).round() as usize];
    let (q1, q3) = (q(&v, 0.25), q(&v, 0.75));
    let fence = 3.0 * (q3 - q1);
    let kept: Vec<f64> = v
        .iter()
        .copied()
        .filter(|&x| x >= q1 - fence && x <= q3 + fence)
        .collect();
    if kept.len() >= 2 && (v.len() - kept.len()) as f64 <= 0.03 * v.len() as f64 {
        v = kept;
    }
    let (lo, hi) = (q(&v, 0.01), q(&v, 0.99));
    let pad = ((hi - lo) * 0.12).max(1e-3);
    Some((lo - pad, hi + pad))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_charts_range_holds_every_point_padded() {
        let (lo, hi) = data_range(&[1.0, f64::NAN, 3.0, 2.0]).unwrap();
        assert!(
            (lo - 0.84).abs() < 1e-12 && (hi - 3.16).abs() < 1e-12,
            "{lo} {hi}"
        );
        assert_eq!(data_range(&[]), None);
        let (lo, hi) = data_range(&[3.0]).unwrap();
        assert!((lo - 2.95).abs() < 1e-12 && (hi - 3.05).abs() < 1e-12);
    }

    #[test]
    fn two_wild_values_dont_flatten_the_rest() {
        let mut v: Vec<f64> = (0..100).map(|i| (i % 10) as f64 * 0.1).collect();
        v[10] = 50.0;
        v[60] = -40.0;
        let (lo, hi) = spread(&v).unwrap();
        assert!(lo > -1.0 && hi < 2.0, "{lo} {hi}");
        // More than 3% of them is what the watch does: kept.
        for k in 0..5 {
            v[20 + k] = 50.0;
        }
        assert!(spread(&v).unwrap().1 > 40.0);
        assert_eq!(spread(&[1.0]), None);
    }
}
