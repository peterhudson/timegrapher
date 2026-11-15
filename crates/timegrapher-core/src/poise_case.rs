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

/// The crown guard above the crown, case units. Its inner end, under the
/// body, is cut where its two inner edges meet, so its outline never
/// crosses itself (#1772).
fn guard() -> Vec<[f64; 2]> {
    let mut p = vec![
        [116.0 * 27.0 / 82.0, -27.0],
        [116.0, -82.0],
        [144.0, -70.0],
        [151.0, -60.0],
        [151.0, -31.0],
    ];
    p.extend(quad([151.0, -31.0], [151.0, -27.0], [147.0, -27.0], 4));
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

impl Case {
    /// Each shape cut into convex pieces, for a surface that fills only
    /// convex polygons (the app's egui): the lugs are concave (#1772). The pieces of a shape cover it exactly, so filling them
    /// all fills the shape; the outlines are still drawn from `shapes`.
    pub fn convex_parts(&self) -> Vec<Vec<[f64; 2]>> {
        self.shapes.iter().flat_map(|s| convex_parts(s)).collect()
    }
}

/// The turn at `b` from `a` to `c`: positive one way round, negative the
/// other, 0 straight on.
fn turn(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0])
}

/// Twice the outline's signed area: its sign is its way round.
fn area2(p: &[[f64; 2]]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

/// The vertex where the outline turns against its way round, if any.
fn reflex(p: &[[f64; 2]]) -> Option<usize> {
    let n = p.len();
    let way = area2(p).signum();
    let tol = 1e-9 * area2(p).abs();
    (0..n).find(|&i| way * turn(p[(i + n - 1) % n], p[i], p[(i + 1) % n]) < -tol)
}

/// Whether segments `a`-`b` and `c`-`d` cross at a point inside both.
fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let (d1, d2) = (turn(a, b, c), turn(a, b, d));
    let (d3, d4) = (turn(c, d, a), turn(c, d, b));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// Whether `q` is inside the outline (even-odd).
fn inside(p: &[[f64; 2]], q: [f64; 2]) -> bool {
    let n = p.len();
    let mut on = false;
    for i in 0..n {
        let (a, b) = (p[i], p[(i + 1) % n]);
        if (a[1] > q[1]) != (b[1] > q[1])
            && q[0] < a[0] + (q[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
        {
            on = !on;
        }
    }
    on
}

/// The outline from vertex `i` round to vertex `j`, both kept.
fn arc(p: &[[f64; 2]], i: usize, j: usize) -> Vec<[f64; 2]> {
    let n = p.len();
    let mut out = vec![p[i]];
    let mut k = i;
    while k != j {
        k = (k + 1) % n;
        out.push(p[k]);
    }
    out
}

/// A cut from reflex vertex `i` to `j` that lies inside the outline,
/// preferring one that leaves both sides convex, so a lug is two pieces,
/// not a fan of slivers: a sliver's needle tips feather out as hairlines
/// past the case (#1780); then one that leaves `i` convex on both sides.
fn cut(p: &[[f64; 2]], i: usize) -> Option<usize> {
    let n = p.len();
    let clear = |j: usize| {
        let (a, b) = (p[i], p[j]);
        let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        inside(p, mid) && (0..n).all(|e| !cross(a, b, p[e], p[(e + 1) % n]))
    };
    let mends = |j: usize| {
        [arc(p, i, j), arc(p, j, i)]
            .iter()
            .all(|s| reflex(s) != Some(0))
    };
    let near = |j: &usize| *j != i && *j != (i + 1) % n && *j != (i + n - 1) % n;
    let open: Vec<usize> = (0..n).filter(near).filter(|&j| clear(j)).collect();
    let whole = |j: usize| {
        [arc(p, i, j), arc(p, j, i)]
            .iter()
            .all(|s| reflex(s).is_none())
    };
    open.iter()
        .copied()
        .find(|&j| whole(j))
        .or_else(|| open.iter().copied().find(|&j| mends(j)))
        .or(open.first().copied())
}

/// A closed outline cut into convex pieces that cover it exactly: a
/// convex one whole, a concave one cut from a reflex vertex and each side
/// cut again.
pub fn convex_parts(p: &[[f64; 2]]) -> Vec<Vec<[f64; 2]>> {
    let Some(i) = reflex(p) else {
        return vec![p.to_vec()];
    };
    let Some(j) = cut(p, i) else {
        return vec![p.to_vec()];
    };
    let mut out = convex_parts(&arc(p, i, j));
    out.extend(convex_parts(&arc(p, j, i)));
    out
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

    /// The convex pieces: each convex, together the shape's area, and the
    /// body, guards and crown whole; the lugs are the cut ones.
    #[test]
    fn the_case_cuts_into_convex_pieces_that_cover_it() {
        let c = case(100.0);
        for s in &c.shapes {
            // A plain outline: no two of its edges cross.
            let n = s.len();
            let edge = |i: usize| (s[i], s[(i + 1) % n]);
            for i in 0..n {
                for j in i + 2..n {
                    let ((a, b), (p, q)) = (edge(i), edge(j));
                    assert!(!cross(a, b, p, q), "{i} {j} cross");
                }
            }
            let parts = convex_parts(s);
            assert!(parts.iter().all(|q| reflex(q).is_none()), "{parts:?}");
            let whole = area2(s).abs();
            let sum: f64 = parts.iter().map(|q| area2(q).abs()).sum();
            assert!((sum - whole).abs() < 1e-9 * whole, "{sum} {whole}");
        }
        let cut = |i: usize| convex_parts(&c.shapes[i]).len();
        assert_eq!([0, 5, 6, 7, 8].map(cut), [1; 5]);
        // Each lug in two pieces, not a fan of slivers whose needle tips
        // feather out past the case as hairlines (#1780).
        assert_eq!([1, 2, 3, 4].map(cut), [2; 4]);
        let all = c.convex_parts();
        assert_eq!(all.len(), (0..c.shapes.len()).map(cut).sum::<usize>());
        // An L cuts into two, an already convex square stays whole.
        let l = [
            [0.0, 0.0],
            [2.0, 0.0],
            [2.0, 1.0],
            [1.0, 1.0],
            [1.0, 2.0],
            [0.0, 2.0],
        ];
        assert_eq!(convex_parts(&l).len(), 2);
        assert_eq!(
            convex_parts(
                &l[..4]
                    .iter()
                    .map(|p| [p[0], p[1].min(1.0)])
                    .collect::<Vec<_>>()
            )
            .len(),
            1
        );
    }
}
