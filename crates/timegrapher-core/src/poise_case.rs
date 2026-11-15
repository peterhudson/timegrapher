//! The watch case drawn faintly behind the Dynamic Poise dial (#1762,
//! Peter's case 1, version A): a round bezel just outside the plot's outer
//! ring, lugs at 12H and 6H, crown guards and a knurled crown at 3H, so
//! the dial reads as the watch it is. One geometry for the app, the MCP
//! view and any other surface that draws the dial.
//!
//! Every shape is given in the dial's own units: centred on the dial's
//! middle, y down, 3H along +x, scaled to the outer ring's radius.

/// The bezel's radius in the case's own units, the drawing's scale.
pub const BEZEL: f64 = 140.0;
/// The bezel's radius over the plot's outer ring's: just outside it.
pub const RING_TO_BEZEL: f64 = 1.012;
/// The case body's radius in case units, hidden under the bezel.
const BODY: f64 = 136.0;
/// How far the crown reaches, case units: the case's widest point.
const CROWN_END: f64 = 172.0;

/// Where the hour names that would sit on the case go instead, over the
/// outer ring's radius: the 45° stops beside the lugs and guards, and 3H
/// past the crown. The others keep their own place.
pub const HALF_HOUR_LABEL: f64 = 1.29;
pub const CROWN_LABEL: f64 = 1.34;

/// The lines' widths, px: the case's outline as seen (drawn twice that
/// wide under the face, so only its outer half shows), the bezel's and
/// the crown's knurl.
pub const CASE_LINE_PX: f64 = 1.0;
pub const BEZEL_LINE_PX: f64 = 0.8;
pub const KNURL_LINE_PX: f64 = 0.8;

/// The case at a dial whose outer ring has radius `ring_r`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Case {
    /// The shapes whose union is the case: its body, the four lugs, the
    /// two guards and the crown's tube and head, each a closed outline.
    /// Draw them all outlined, then all filled with the face colour on
    /// top, so only the union's outline shows.
    pub shapes: Vec<Vec<[f64; 2]>>,
    /// The knurl's lines across the crown's head, drawn over the face.
    pub knurl: Vec<[[f64; 2]; 2]>,
    /// The bezel, drawn last over the case: filled with the pane's colour
    /// and outlined.
    pub bezel_r: f64,
}

/// Case units to the dial's at `ring_r`.
fn scale(ring_r: f64) -> f64 {
    RING_TO_BEZEL * ring_r / BEZEL
}

/// A quadratic Bézier from `a` through control `c` to `b`, as `n` points
/// after `a`.
fn quad(a: [f64; 2], c: [f64; 2], b: [f64; 2], n: usize) -> Vec<[f64; 2]> {
    (1..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            [
                u * u * a[0] + 2.0 * u * t * c[0] + t * t * b[0],
                u * u * a[1] + 2.0 * u * t * c[1] + t * t * b[1],
            ]
        })
        .collect()
}

/// One lug, top right, case units (Hugh's `wm1.py`).
fn lug() -> Vec<[f64; 2]> {
    let mut p = vec![[0.0, 0.0], [68.0, -100.0], [68.0, -157.0]];
    p.extend(quad([68.0, -157.0], [68.0, -161.0], [72.0, -161.5], 4));
    p.push([93.0, -162.5]);
    p.extend(quad([93.0, -162.5], [96.0, -162.5], [97.0, -159.0], 4));
    p.push([127.0, -48.0]);
    p
}

/// The crown guard above the crown, case units.
fn guard() -> Vec<[f64; 2]> {
    let mut p = vec![
        [0.0, 0.0],
        [116.0, -82.0],
        [144.0, -70.0],
        [151.0, -60.0],
        [151.0, -31.0],
    ];
    p.extend(quad([151.0, -31.0], [151.0, -27.0], [147.0, -27.0], 4));
    p.push([0.0, -27.0]);
    p
}

/// The crown's head: x 146 to 172, ±22, its outer corners chamfered.
const HEAD: (f64, f64, f64) = (146.0, 26.0, 22.0);

fn crown() -> [Vec<[f64; 2]>; 2] {
    let tube = vec![[136.0, -12.0], [150.0, -12.0], [150.0, 12.0], [136.0, 12.0]];
    let (x0, len, half) = HEAD;
    let head = vec![
        [x0, -half],
        [x0 + len - 5.0, -half],
        [x0 + len, -half + 6.0],
        [x0 + len, half - 6.0],
        [x0 + len - 5.0, half],
        [x0, half],
    ];
    [tube, head]
}

fn mirrored(p: &[[f64; 2]], sx: f64, sy: f64) -> Vec<[f64; 2]> {
    p.iter().map(|q| [q[0] * sx, q[1] * sy]).collect()
}

/// The case behind a dial whose outer ring has radius `ring_r`.
pub fn case(ring_r: f64) -> Case {
    let k = scale(ring_r);
    let body: Vec<[f64; 2]> = (0..72)
        .map(|i| {
            let a = f64::from(i) * std::f64::consts::TAU / 72.0;
            [BODY * a.cos(), BODY * a.sin()]
        })
        .collect();
    let mut shapes = vec![body];
    let lug = lug();
    for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
        shapes.push(mirrored(&lug, sx, sy));
    }
    let guard = guard();
    for sy in [1.0, -1.0] {
        shapes.push(mirrored(&guard, 1.0, sy));
    }
    shapes.extend(crown());
    let (x0, len, half) = HEAD;
    let mut knurl = Vec::new();
    let mut y = -half + 5.0;
    while y < half - 3.0 {
        knurl.push([[(x0 + 4.0) * k, y * k], [(x0 + len - 2.0) * k, y * k]]);
        y += 5.0;
    }
    Case {
        shapes: shapes.iter().map(|s| mirrored(s, k, k)).collect(),
        knurl,
        bezel_r: BEZEL * k,
    }
}

/// How far from the middle the case reaches at `ring_r`: the crown's end.
pub fn reach(ring_r: f64) -> f64 {
    CROWN_END * scale(ring_r)
}

/// Where the hour name at `deg` (clockwise from 12H) goes on a dial with
/// the case, as a radius: 3H past the crown, the 45° stops clear of the
/// lugs and guards, or `None` for the names that keep their own place
/// (12H, 6H, 9H).
pub fn label_r(deg: f64, ring_r: f64) -> Option<f64> {
    let d = deg.rem_euclid(360.0);
    if (d - 90.0).abs() < 1e-6 {
        Some(CROWN_LABEL * ring_r)
    } else if ((d - 45.0) / 90.0).fract().abs() < 1e-6 {
        Some(HALF_HOUR_LABEL * ring_r)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bezel sits just outside the ring, the crown at 3H past it, and
    /// each lug's tip above and below it.
    #[test]
    fn the_case_sits_round_the_ring_with_its_crown_at_3() {
        let c = case(100.0);
        assert!((c.bezel_r - 101.2).abs() < 1e-9);
        let far = |f: &dyn Fn(&[f64; 2]) -> bool| {
            c.shapes
                .iter()
                .flatten()
                .filter(|p| f(p))
                .map(|p| p[0].hypot(p[1]))
                .fold(0.0, f64::max)
        };
        // The crown, along +x (3H), reaches past everything else.
        let right = c.shapes.iter().flatten().map(|p| p[0]).fold(0.0, f64::max);
        assert!((right - reach(100.0)).abs() < 1e-6, "{right}");
        let left = c.shapes.iter().flatten().map(|p| p[0]).fold(0.0, f64::min);
        assert!(-left < right, "the crown at 3H only");
        assert!(
            far(&|p| p[0] < 0.0 && p[1].abs() < 1.0) < c.bezel_r,
            "nothing at 9H past the bezel"
        );
        // Lugs top and bottom, past the bezel.
        let top = c.shapes.iter().flatten().map(|p| p[1]).fold(0.0, f64::min);
        let bottom = c.shapes.iter().flatten().map(|p| p[1]).fold(0.0, f64::max);
        assert!(
            (top + bottom).abs() < 1e-9 && -top > c.bezel_r,
            "{top} {bottom}"
        );
        assert_eq!(c.shapes.len(), 1 + 4 + 2 + 2);
        assert_eq!(c.knurl.len(), 8);
        assert!(c.shapes.iter().all(|s| s.len() >= 4));
    }

    /// 3H goes past the crown and the 45° stops past the lugs; 12H, 6H and
    /// 9H keep their own place.
    #[test]
    fn the_hour_names_on_the_case_move_clear_of_it() {
        assert_eq!(label_r(90.0, 100.0), Some(134.0));
        for d in [45.0, 135.0, 225.0, 315.0] {
            assert_eq!(label_r(d, 100.0), Some(129.0), "{d}");
        }
        for d in [0.0, 180.0, 270.0, 360.0] {
            assert_eq!(label_r(d, 100.0), None, "{d}");
        }
        assert!(CROWN_LABEL * 100.0 > reach(100.0), "3H past the crown");
    }
}
