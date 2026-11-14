//! Which of a reading's points over the strip are joined by its line.

use super::gap_before;

/// The pairs of neighbouring `points` from `from_s` to `to_s` joined by a
/// line: those with no gap ([`gap_before`]) between them.
pub(super) fn joined(points: &[[f64; 2]], from_s: f64, to_s: f64) -> Vec<([f64; 2], [f64; 2])> {
    let lo = points.partition_point(|p| p[0] < from_s);
    let hi = points.partition_point(|p| p[0] <= to_s);
    (lo.max(1)..hi)
        .filter(|&i| !gap_before(points, i))
        .map(|i| (points[i - 1], points[i]))
        .filter(|(a, b)| a[0] >= from_s && a[1].is_finite() && b[1].is_finite())
        .collect()
}

/// The pieces of line through `points` from `from_s` to `to_s`: each
/// unbroken run of [`joined`] pairs as one line, so it is drawn whole.
/// Drawn a pair at a time, a level pair under a point long, as amplitude's
/// close readings make on a long strip, was rounded to the pixel at both
/// ends into nothing.
pub(super) fn runs(points: &[[f64; 2]], from_s: f64, to_s: f64) -> Vec<Vec<[f64; 2]>> {
    let mut out: Vec<Vec<[f64; 2]>> = Vec::new();
    for (a, b) in joined(points, from_s, to_s) {
        match out.last_mut() {
            Some(run) if run.last() == Some(&a) => run.push(b),
            _ => out.push(vec![a, b]),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pairs that share a point are one run; a gap or a missing reading
    /// starts another.
    #[test]
    fn joined_pairs_make_one_run_until_a_gap() {
        let pts = [
            [0.0, 1.0],
            [0.5, 1.0],
            [1.0, 1.0],
            [1.5, f64::NAN],
            [2.0, 2.0],
            [2.5, 2.0],
            [3.0, 2.0],
            [9.0, 3.0],
            [9.5, 3.0],
            [10.0, 3.0],
        ];
        assert_eq!(
            runs(&pts, 0.0, 10.0),
            vec![
                vec![pts[0], pts[1], pts[2]],
                vec![pts[4], pts[5], pts[6]],
                vec![pts[7], pts[8], pts[9]],
            ]
        );
    }
}
