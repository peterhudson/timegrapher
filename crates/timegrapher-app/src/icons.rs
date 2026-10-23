//! The pictures on the pane buttons. Each is a small drawing on a 24 by 24
//! grid, kept as the SVG it was designed in and drawn with egui's painter:
//! the few SVG elements and path commands the drawings use, read once and
//! flattened into lines.

use crate::theme::Icon;
use eframe::egui::{self, epaint, Color32, Pos2, Rect, Shape, Stroke};
use std::sync::OnceLock;

/// Every icon, in the order of [`drawing`]'s arms.
const ALL: [Icon; 25] = [
    Icon::Readings,
    Icon::Watch,
    Icon::Strip,
    Icon::Profile,
    Icon::Scope,
    Icon::Isochronism,
    Icon::Positions,
    Icon::RateChart,
    Icon::AmplitudeChart,
    Icon::BeatErrorChart,
    Icon::RateDistribution,
    Icon::AmplitudeDistribution,
    Icon::BeatErrorDistribution,
    Icon::Steadiness,
    Icon::Periods,
    Icon::Cycle,
    Icon::Autocorrelation,
    Icon::Allan,
    Icon::Cusum,
    Icon::Fft,
    Icon::Stft,
    Icon::Wavelet,
    Icon::Reserve,
    Icon::Microphone,
    Icon::Settings,
];

/// The drawing behind each icon, with the pun it makes.
fn drawing(icon: Icon) -> &'static str {
    match icon {
        // A speedometer: The three big numbers you glance at, like a car's dials.
        Icon::Readings => {
            r#"<path d="M3.5 17a8.5 8.5 0 0 1 17 0"/><path d="M12 17l4.2-5.6"/><circle cx="12" cy="17" r="1.3" fill="currentColor"/><path d="M6.2 11.3l1 .9M12 8.5v1.3M17.8 11.3l-1 .9"/>"#
        }
        // A brass pocket watch: Your watches, and the take of one running or on show. Drawn in brass.
        Icon::Watch => {
            r#"<circle cx="12" cy="14" r="7"/><path d="M12 7V5.2"/><circle cx="12" cy="3.6" r="1.6"/><path d="M12 14v-3.6M12 14l2.6 1.4"/>"#
        }
        // A pole dancer: The strip, as in stripper. Family friendly: long hair, a dress, and a pole. In pink.
        Icon::Strip => {
            r#"<path d="M5 2v20"/><circle cx="12.6" cy="4.4" r="1.9"/><path d="M14.3 3.3c2.4-.2 4 1.4 4.8 3.6-.8-.3-1.6-.2-2.2.2 1.4.8 2.2 2.4 2.4 4.2-1.2-.8-2.4-1-3.4-.8.6 1 .8 2 .6 3"/><path d="M11.6 6.4L5 3.6"/><path d="M11.6 8.6L5 10.6"/><path d="M12.2 6.4l-.2 3.2"/><path d="M11.4 9.4L7.6 16.6c3 1.2 6.4 1.1 9.4-.4L13 9.4z"/><path d="M9.4 17.3L5 20"/><path d="M14.4 17.1l2 4.9"/>"#
        }
        // A stethoscope: Listening closely to one beat, the way a doctor listens to a heart.
        Icon::Profile => {
            r#"<path d="M5.5 3v5a4.5 4.5 0 0 0 9 0V3"/><path d="M10 12.5v2.5a4.5 4.5 0 0 0 9 0v-1"/><circle cx="19" cy="12" r="2"/>"#
        }
        // A magnifying glass on a beat: One beat's sound, looked at up close.
        Icon::Scope => {
            r#"<circle cx="10" cy="10" r="6.5"/><path d="M14.8 14.8L20.5 20.5"/><path stroke-width="1.3" d="M4 10h1.6l1-4.4 1 8.6 1-6.6 1 5 1-3.2.9 2 .8-1h2.6"/>"#
        }
        // A metronome: Keeping the same time however wide the swing.
        Icon::Isochronism => {
            r#"<path d="M7.5 21h9L14 4h-4z"/><path d="M12 17.5L16.5 6"/><circle cx="15.2" cy="9.4" r="1.4" fill="currentColor"/>"#
        }
        // A die showing six: Six faces, six positions: DU, DD, CU, CD, CL and CR. In teal.
        Icon::Positions => {
            r#"<rect x="3.5" y="3.5" width="17" height="17" rx="3.5"/><g fill="currentColor" stroke="none"><circle cx="8.5" cy="8" r="1.4"/><circle cx="8.5" cy="12" r="1.4"/><circle cx="8.5" cy="16" r="1.4"/><circle cx="15.5" cy="8" r="1.4"/><circle cx="15.5" cy="12" r="1.4"/><circle cx="15.5" cy="16" r="1.4"/></g>"#
        }
        // A leaping hare: Gaining or losing: how fast the watch runs.
        Icon::RateChart => {
            r#"<path d="M4.2 14.2c.4-3.4 3.8-5.6 7.8-5.2 2.4.2 4.2 1.3 5 2.8"/><path d="M16.8 13.4c-.9 1.6-2.6 2.3-4.6 2.3"/><path d="M4.2 14.2c.2 1.8 1.8 3 3.8 3 1.6 0 3-.6 3.8-1.6"/><path d="M7.6 13.6c1.4.4 2.4 1.6 2.4 3.2l-.6 2.2H4.4"/><circle cx="18.4" cy="11.4" r="2.4"/><circle cx="19.2" cy="11" r=".6" fill="currentColor" stroke="none"/><path d="M17.2 9.4c-1.8-2.2-2.6-4.8-2.2-7.2 1.6 1.4 2.8 4 3 6.8"/><path d="M19.2 9.2c.4-2.6 1.4-4.8 3-6 .2 2.2-.4 4.6-1.8 6.6"/><path d="M15.6 15l2.6 2.6M14 15.6l1.8 3"/><circle cx="3.6" cy="12.4" r="1.3" fill="currentColor" stroke="none"/>"#
        }
        // A swing: How far the balance swings, like a child on a swing.
        Icon::AmplitudeChart => {
            r#"<path d="M3 3.5h18"/><path d="M10 3.5L6 15M14 3.5L10 16.2"/><path d="M4.6 15.4l6.6 1.4"/><path d="M4.5 19.5a11 11 0 0 0 15 0" stroke-dasharray="1.5 2.4"/>"#
        }
        // Two children on a seesaw: Tick and tock out of balance: one up, one down.
        Icon::BeatErrorChart => {
            r#"<path d="M3 13.4l18 5.2"/><path d="M12 16l-2.4 4.6h4.8z"/><circle cx="5" cy="8.8" r="1.6"/><path d="M5 10.4v3.4M5 11.6l1.8.6M5 13.8l1.6 2.6"/><circle cx="19" cy="12.4" r="1.6"/><path d="M19 14v3.8M19 15.2l-1.8.4M19 17.8l-1.4 2.8"/>"#
        }
        // A dromedary: One hump, like a normal distribution.
        Icon::RateDistribution => {
            r#"<path d="M4 10.5C4 8 5.6 7.4 7 6.6 8.2 3 12 3 13 6.6c.6 1.8 1.8 2.2 3 2.6L18.2 6.8c.4-1 1.4-1.6 2.6-1.4l1.2.6-.4 1-1.2.2-1.8 4.2c-.6 1.6-1.8 2.6-3.6 2.6H6.6C5 14 4 12.6 4 10.5z"/><path d="M6.6 14v7.5M8.6 14v7.5M14.2 14v7.5M16.2 13.8v7.7"/><path d="M4 10.4l-1 3.6"/><circle cx="20.4" cy="6.6" r=".5" fill="currentColor" stroke="none"/>"#
        }
        // An hourglass: The sand piles up in a bell-shaped heap: a distribution, grain by grain.
        Icon::AmplitudeDistribution => {
            r#"<path d="M5.5 2.5h13M5.5 21.5h13"/><path d="M7.5 2.5c0 5 4 6.4 4 9.5s-4 4.5-4 9.5M16.5 2.5c0 5-4 6.4-4 9.5s4 4.5 4 9.5"/><path d="M8.4 21.4c1.8 0 2.2-3.6 3.6-3.6s1.8 3.6 3.6 3.6z" fill="currentColor"/><path d="M12 13v3" stroke-dasharray=".6 1.6"/>"#
        }
        // A jellyfish: A jellyfish's body is called its bell.
        Icon::BeatErrorDistribution => {
            r#"<path d="M4.5 12.5a7.5 7.5 0 0 1 15 0c-1.25.9-2.5.9-3.75 0-1.25.9-2.5.9-3.75 0-1.25.9-2.5.9-3.75 0-1.25.9-2.5.9-3.75 0z"/><path d="M7.4 14c-.8 2 .8 3.4 0 5.4M10.8 14.2c-.8 2.1.8 3.9 0 6.6M13.2 14.2c.8 2.1-.8 3.9 0 6.6M16.6 14c.8 2-.8 3.4 0 5.4"/>"#
        }
        // A spirit level: Is it holding level, or drifting?
        Icon::Steadiness => {
            r#"<rect x="2.5" y="8.5" width="19" height="7" rx="2"/><rect x="8.5" y="10.3" width="7" height="3.4" rx="1.7"/><circle cx="12.6" cy="12" r=".9" fill="currentColor"/><path d="M10.2 9v6M13.8 9v6"/>"#
        }
        // A steam train: The periods of the train wheels, which turn up in the rate.
        Icon::Periods => {
            r#"<path d="M3 17V7.5h5V11h10.5a2.5 2.5 0 0 1 2.5 2.5V17z"/><path d="M2 7.5h7"/><path d="M15.5 11V7.6h2.6V11"/><rect x="4.4" y="9" width="2.2" height="2.2" rx=".4"/><circle cx="6" cy="19" r="2.2"/><circle cx="11.8" cy="19" r="2.2"/><circle cx="17.4" cy="19" r="2.2"/><path d="M6 19h11.4"/>"#
        }
        // A bicycle: One cycle's shape. A cycle.
        Icon::Cycle => {
            r#"<circle cx="6" cy="16" r="4"/><circle cx="18" cy="16" r="4"/><path d="M6 16l4-7h6.5L18 16M10 9l3 7H6M13 16l3.5-7"/><path d="M8.6 7h2.8M16.5 9l.6-2.5h2.2"/>"#
        }
        // A parrot: Something that repeats itself.
        Icon::Autocorrelation => {
            r#"<path d="M3 18.2h14"/><circle cx="11" cy="6.2" r="3.2"/><path d="M14 4.6c2.4-.6 4.2.8 4 3.2-.1 1.2-.8 2.2-1.8 2.4.3-.9-.1-1.8-1.6-1.8"/><circle cx="11.8" cy="5.6" r=".7" fill="currentColor" stroke="none"/><path d="M9.4 3.2c-.6-1.2-.4-2.2.4-2.8M11 3c.1-1.2.7-2 1.7-2.2"/><path d="M8.4 8.4c-2 2.4-2.4 6-.8 9.8h3.4c1.8-2.6 2.4-6 1.4-9.2"/><path d="M8.8 11c1.4 1.6 1.8 3.8 1.2 6.4"/><path d="M8.2 18.2l-2.6 4.4M10 18.2l-.6 4.6"/>"#
        }
        // An Allen key and its bolt: Allan, Allen. Sorry.
        Icon::Allan => {
            r#"<path d="M4 6.2v9.6a5.2 5.2 0 0 0 5.2 5.2h7.2v-3.2H9.2a2 2 0 0 1-2-2V6.2"/><path d="M4 6.2l.8-1.4h1.6l.8 1.4"/><path d="M17.5 2.6l3.4 2v3.9l-3.4 2-3.4-2V4.6z"/><path d="M17.5 4.9l1.4.8v1.6l-1.4.8-1.4-.8V5.7z"/>"#
        }
        // An abacus: A running sum, kept on beads.
        Icon::Cusum => {
            r#"<rect x="3" y="4.5" width="18" height="15" rx="2"/><path d="M3 9.5h18M3 14.5h18"/><g fill="currentColor" stroke="none"><ellipse cx="6.4" cy="9.5" rx="1.6" ry="2.3"/><ellipse cx="10.8" cy="9.5" rx="1.6" ry="2.3"/><ellipse cx="17.4" cy="9.5" rx="1.6" ry="2.3"/><ellipse cx="6.4" cy="14.5" rx="1.6" ry="2.3"/><ellipse cx="13.4" cy="14.5" rx="1.6" ry="2.3"/><ellipse cx="17.4" cy="14.5" rx="1.6" ry="2.3"/></g>"#
        }
        // A prism: Splitting the rate into its frequencies, as a prism splits light.
        Icon::Fft => {
            r#"<path d="M11 4l7.5 14h-15z"/><path d="M1.5 13.4l6.4-1.6"/><path d="M14.6 11.6l7.9-3M14.9 12.2l7.6 0M15.2 12.8l7.3 3.4"/>"#
        }
        // A window: The STFT reads the readings a window at a time.
        Icon::Stft => {
            r#"<rect x="4.5" y="3.5" width="15" height="15" rx="1.5"/><path d="M12 3.5v15M4.5 11h15"/><path d="M2.5 21h19"/>"#
        }
        // A little breaking wave: A wavelet.
        Icon::Wavelet => {
            r#"<path d="M2 18.5c2.6 0 4-1.6 5-4.6 1.2-3.8 3.8-6.4 7.4-6.4 3 0 5.1 2 5.1 4.4 0 1.8-1.3 3-2.9 3-1.4 0-2.4-1-2.4-2.2 0-1 .7-1.8 1.6-1.8"/><path d="M2 21.5h20"/>"#
        }
        // A fuel gauge near empty: The power reserve, run down to the last.
        Icon::Reserve => {
            r#"<path d="M3.5 17a8.5 8.5 0 0 1 17 0"/><path d="M12 17l-5.2-4.4"/><circle cx="12" cy="17" r="1.3" fill="currentColor"/><path d="M5.2 9.6l1.2 1M18.8 9.6l-1.2 1M12 6.8v1.6"/><path d="M3 20.5h18"/>"#
        }
        // An old studio microphone on its stand: Where the watch is heard. Drawn in brass, like the watch.
        Icon::Microphone => {
            r#"<rect x="8.5" y="2.5" width="7" height="11" rx="3.5"/><path d="M8.5 6.5h2.2M8.5 9.5h2.2M13.3 6.5h2.2M13.3 9.5h2.2"/><path d="M5.5 10.5a6.5 6.5 0 0 0 13 0"/><path d="M12 17v4M8 21.5h8"/>"#
        }
        // A gear: The works behind the dial, set once and left to run.
        Icon::Settings => {
            r#"<circle cx="12" cy="12" r="6.5"/><circle cx="12" cy="12" r="2.4"/><path stroke-width="2.6" d="M18.5 12h2.6M16.6 16.6l1.8 1.8M12 18.5v2.6M7.4 16.6l-1.8 1.8M5.5 12H2.9M7.4 7.4L5.6 5.6M12 5.5V2.9M16.6 7.4l1.8-1.8"/>"#
        }
    }
}

/// The line width the drawings were designed with, in grid units.
const WIDTH: f32 = 1.7;

/// One run of line in a drawing, in grid units.
#[derive(Debug, Clone, PartialEq)]
struct Part {
    points: Vec<Pos2>,
    closed: bool,
    fill: bool,
    stroke: Option<f32>,
    dash: Option<(f32, f32)>,
}

/// Draw `icon` in the square `r`, in colour `c`.
pub fn draw(painter: &egui::Painter, r: Rect, icon: Icon, c: Color32) {
    let scale = r.width().min(r.height()) / 24.0;
    let at = |p: Pos2| r.left_top() + p.to_vec2() * scale;
    for part in parts(icon) {
        let pts: Vec<Pos2> = part.points.iter().map(|&p| at(p)).collect();
        if part.fill && pts.len() >= 3 {
            painter.add(fill(&pts, c));
        }
        let Some(w) = part.stroke else { continue };
        let stroke = Stroke::new(w * scale, c);
        let runs = match part.dash {
            Some((on, off)) => dashes(&pts, part.closed, on * scale, off * scale),
            None => vec![pts.clone()],
        };
        for run in runs {
            if run.len() < 2 {
                continue;
            }
            if part.closed && part.dash.is_none() {
                painter.add(Shape::closed_line(run, stroke));
            } else {
                // Round ends, as the drawings have.
                for &end in [run[0], run[run.len() - 1]].iter() {
                    painter.circle_filled(end, stroke.width / 2.0, c);
                }
                painter.add(Shape::line(run, stroke));
            }
        }
    }
}

/// The parts of `icon`, read from its drawing the first time it is asked for.
fn parts(icon: Icon) -> &'static [Part] {
    static PARTS: OnceLock<Vec<Vec<Part>>> = OnceLock::new();
    let all = PARTS.get_or_init(|| ALL.iter().map(|&i| parse(drawing(i))).collect());
    ALL.iter()
        .position(|&i| i == icon)
        .map(|n| all[n].as_slice())
        .unwrap_or(&[])
}

/// A filled outline, cut into triangles so a dent in it stays a dent.
fn fill(pts: &[Pos2], c: Color32) -> Shape {
    let mut mesh = epaint::Mesh::default();
    for &p in pts {
        mesh.colored_vertex(p, c);
    }
    for [a, b, t] in triangles(pts) {
        mesh.add_triangle(a, b, t);
    }
    Shape::mesh(mesh)
}

/// Ear clipping: the outline's corners in threes, each triangle inside it.
fn triangles(pts: &[Pos2]) -> Vec<[u32; 3]> {
    let cross = |o: Pos2, a: Pos2, b: Pos2| (a - o).x * (b - o).y - (a - o).y * (b - o).x;
    let area: f32 = (0..pts.len())
        .map(|i| cross(Pos2::ZERO, pts[i], pts[(i + 1) % pts.len()]))
        .sum();
    let turn = area.signum();
    let mut left: Vec<usize> = (0..pts.len()).collect();
    let mut out = Vec::new();
    while left.len() > 3 {
        let n = left.len();
        let ear = (0..n).find(|&i| {
            let (a, b, c) = (left[(i + n - 1) % n], left[i], left[(i + 1) % n]);
            let (pa, pb, pc) = (pts[a], pts[b], pts[c]);
            if cross(pa, pb, pc) * turn <= 0.0 {
                return false;
            }
            !left.iter().any(|&k| {
                k != a && k != b && k != c && {
                    let p = pts[k];
                    let (d1, d2, d3) = (cross(pa, pb, p), cross(pb, pc, p), cross(pc, pa, p));
                    d1 * turn >= 0.0 && d2 * turn >= 0.0 && d3 * turn >= 0.0
                }
            })
        });
        // A tangle with no ear left: take a corner anyway rather than stop.
        let i = ear.unwrap_or(0);
        out.push([
            left[(i + n - 1) % n] as u32,
            left[i] as u32,
            left[(i + 1) % n] as u32,
        ]);
        left.remove(i);
    }
    if left.len() == 3 {
        out.push([left[0] as u32, left[1] as u32, left[2] as u32]);
    }
    out
}

/// A line cut into dashes `on` long with gaps `off` long.
fn dashes(pts: &[Pos2], closed: bool, on: f32, off: f32) -> Vec<Vec<Pos2>> {
    let mut path = pts.to_vec();
    if closed && !pts.is_empty() {
        path.push(pts[0]);
    }
    let mut out = Vec::new();
    let mut run = vec![path[0]];
    let (mut drawing, mut left) = (true, on);
    for w in path.windows(2) {
        let (mut a, b) = (w[0], w[1]);
        let mut len = a.distance(b);
        while len > left {
            let p = a + (b - a) * (left / len);
            if drawing {
                run.push(p);
                out.push(std::mem::take(&mut run));
            } else {
                run = vec![p];
            }
            len -= left;
            a = p;
            drawing = !drawing;
            left = if drawing { on } else { off };
        }
        left -= len;
        if drawing {
            run.push(b);
        }
    }
    if drawing && run.len() > 1 {
        out.push(run);
    }
    out
}

/// How a part is painted, as set on it or on the group around it.
#[derive(Clone, Copy)]
struct Paint {
    fill: bool,
    stroke: bool,
    width: f32,
    dash: Option<(f32, f32)>,
}

/// Read a drawing's elements into parts.
fn parse(svg: &str) -> Vec<Part> {
    let mut out = Vec::new();
    let mut groups = vec![Paint {
        fill: false,
        stroke: true,
        width: WIDTH,
        dash: None,
    }];
    let mut rest = svg;
    while let Some(start) = rest.find('<') {
        let end = rest[start..].find('>').map_or(rest.len(), |e| start + e);
        let tag = rest[start + 1..end].trim_end_matches('/').trim();
        rest = &rest[(end + 1).min(rest.len())..];
        if tag.starts_with('/') {
            if groups.len() > 1 {
                groups.pop();
            }
            continue;
        }
        let (name, attrs) = tag.split_once(' ').unwrap_or((tag, ""));
        let attr = |key: &str| attribute(attrs, key);
        let num = |key: &str| attr(key).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let outer = *groups.last().unwrap_or(&groups[0]);
        let paint = Paint {
            fill: attr("fill").map_or(outer.fill, |v| v != "none"),
            stroke: attr("stroke").map_or(outer.stroke, |v| v != "none"),
            width: attr("stroke-width")
                .and_then(|v| v.parse().ok())
                .unwrap_or(outer.width),
            dash: attr("stroke-dasharray")
                .map(|v| {
                    let n = numbers(v);
                    (
                        n.first().copied().unwrap_or(1.0),
                        n.get(1).copied().unwrap_or(1.0),
                    )
                })
                .or(outer.dash),
        };
        let shapes: Vec<(Vec<Pos2>, bool)> = match name {
            "g" => {
                if !tag.ends_with('/') {
                    groups.push(paint);
                }
                continue;
            }
            "path" => path(attr("d").unwrap_or("")),
            "circle" => vec![(ellipse(num("cx"), num("cy"), num("r"), num("r")), true)],
            "ellipse" => vec![(ellipse(num("cx"), num("cy"), num("rx"), num("ry")), true)],
            "rect" => vec![(
                rounded(num("x"), num("y"), num("width"), num("height"), num("rx")),
                true,
            )],
            _ => continue,
        };
        for (points, closed) in shapes {
            out.push(Part {
                points,
                closed,
                fill: paint.fill,
                stroke: paint.stroke.then_some(paint.width),
                dash: paint.dash,
            });
        }
    }
    out
}

fn attribute<'a>(attrs: &'a str, key: &str) -> Option<&'a str> {
    let mut rest = attrs;
    while let Some(i) = rest.find('=') {
        let name = rest[..i].trim();
        let after = rest[i + 1..].trim_start().strip_prefix('"')?;
        let close = after.find('"')?;
        if name == key {
            return Some(&after[..close]);
        }
        rest = &after[close + 1..];
    }
    None
}

/// The numbers in a list like `1.6-2.2.4 .9`, where a sign or a second
/// point starts the next one.
fn numbers(s: &str) -> Vec<f32> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<f32>| {
        if let Ok(v) = cur.parse() {
            out.push(v);
        }
        cur.clear();
    };
    for ch in s.chars() {
        match ch {
            '-' if !cur.is_empty() && !cur.ends_with('e') => {
                flush(&mut cur, &mut out);
                cur.push(ch);
            }
            '.' if cur.contains('.') => {
                flush(&mut cur, &mut out);
                cur.push(ch);
            }
            '0'..='9' | '.' | '-' | 'e' => cur.push(ch),
            _ => flush(&mut cur, &mut out),
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// Points around an ellipse.
fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<Pos2> {
    let n = 40;
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            Pos2::new(cx + rx * a.cos(), cy + ry * a.sin())
        })
        .collect()
}

/// Points around a rectangle with round corners of radius `r`.
fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<Pos2> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let corners = [
        (x + w - r, y + r, -90.0_f32),
        (x + w - r, y + h - r, 0.0),
        (x + r, y + h - r, 90.0),
        (x + r, y + r, 180.0),
    ];
    let mut out = Vec::new();
    for (cx, cy, from) in corners {
        for i in 0..=6 {
            let a = (from + 90.0 * i as f32 / 6.0).to_radians();
            out.push(Pos2::new(cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    out.dedup();
    out
}

/// The runs of a path's `d`, each with whether it was closed.
fn path(d: &str) -> Vec<(Vec<Pos2>, bool)> {
    // Split into commands and their numbers.
    let mut cmds: Vec<(char, Vec<f32>)> = Vec::new();
    let mut start = None;
    for (i, ch) in d.char_indices() {
        if ch.is_ascii_alphabetic() && ch != 'e' {
            if let Some((c, s)) = start {
                cmds.push((c, numbers(&d[s..i])));
            }
            start = Some((ch, i + 1));
        }
    }
    if let Some((c, s)) = start {
        cmds.push((c, numbers(&d[s..])));
    }

    let mut out = Vec::new();
    let mut run: Vec<Pos2> = Vec::new();
    let mut cur = Pos2::ZERO;
    let mut begin = Pos2::ZERO;
    // The last control point, for S and smooth curves.
    let mut last_ctrl: Option<Pos2> = None;
    let finish = |run: &mut Vec<Pos2>, out: &mut Vec<(Vec<Pos2>, bool)>, closed: bool| {
        if run.len() > 1 {
            out.push((std::mem::take(run), closed));
        } else {
            run.clear();
        }
    };
    for (cmd, n) in cmds {
        let rel = cmd.is_ascii_lowercase();
        let base = |cur: Pos2| if rel { cur.to_vec2() } else { egui::Vec2::ZERO };
        let mut ctrl = None;
        match cmd.to_ascii_uppercase() {
            'M' => {
                finish(&mut run, &mut out, false);
                for (k, xy) in n.as_chunks::<2>().0.iter().enumerate() {
                    cur = Pos2::new(xy[0], xy[1]) + base(cur);
                    if k == 0 {
                        begin = cur;
                    }
                    run.push(cur);
                }
            }
            'L' => {
                for xy in n.as_chunks::<2>().0 {
                    cur = Pos2::new(xy[0], xy[1]) + base(cur);
                    run.push(cur);
                }
            }
            'H' => {
                for &x in &n {
                    cur.x = x + if rel { cur.x } else { 0.0 };
                    run.push(cur);
                }
            }
            'V' => {
                for &y in &n {
                    cur.y = y + if rel { cur.y } else { 0.0 };
                    run.push(cur);
                }
            }
            'C' | 'S' => {
                let k = if cmd.eq_ignore_ascii_case(&'C') { 6 } else { 4 };
                for v in n.chunks_exact(k) {
                    let b = base(cur);
                    let (c1, c2, to) = if k == 6 {
                        (
                            Pos2::new(v[0], v[1]) + b,
                            Pos2::new(v[2], v[3]) + b,
                            Pos2::new(v[4], v[5]) + b,
                        )
                    } else {
                        let c1 = last_ctrl.map_or(cur, |c| cur + (cur - c));
                        (c1, Pos2::new(v[0], v[1]) + b, Pos2::new(v[2], v[3]) + b)
                    };
                    for i in 1..=12 {
                        let t = i as f32 / 12.0;
                        let u = 1.0 - t;
                        let p = cur.to_vec2() * (u * u * u)
                            + c1.to_vec2() * (3.0 * u * u * t)
                            + c2.to_vec2() * (3.0 * u * t * t)
                            + to.to_vec2() * (t * t * t);
                        run.push(p.to_pos2());
                    }
                    cur = to;
                    last_ctrl = Some(c2);
                }
                ctrl = last_ctrl;
            }
            'Q' => {
                for v in n.as_chunks::<4>().0 {
                    let b = base(cur);
                    let (c, to) = (Pos2::new(v[0], v[1]) + b, Pos2::new(v[2], v[3]) + b);
                    for i in 1..=8 {
                        let t = i as f32 / 8.0;
                        let u = 1.0 - t;
                        let p = cur.to_vec2() * (u * u)
                            + c.to_vec2() * (2.0 * u * t)
                            + to.to_vec2() * (t * t);
                        run.push(p.to_pos2());
                    }
                    cur = to;
                }
            }
            'A' => {
                for v in n.as_chunks::<7>().0 {
                    let to = Pos2::new(v[5], v[6]) + base(cur);
                    run.extend(arc(cur, to, v[0], v[1], v[2], v[3] != 0.0, v[4] != 0.0));
                    cur = to;
                }
            }
            'Z' => {
                cur = begin;
                finish(&mut run, &mut out, true);
                run.push(cur);
            }
            _ => {}
        }
        last_ctrl = ctrl;
    }
    finish(&mut run, &mut out, false);
    out
}

/// Points along an SVG arc from `from` to `to`, `to` included.
fn arc(from: Pos2, to: Pos2, rx: f32, ry: f32, rot: f32, large: bool, sweep: bool) -> Vec<Pos2> {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 || from == to {
        return vec![to];
    }
    let (sin, cos) = rot.to_radians().sin_cos();
    let h = (from - to) / 2.0;
    let x1 = cos * h.x + sin * h.y;
    let y1 = -sin * h.x + cos * h.y;
    let grow = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if grow > 1.0 {
        rx *= grow.sqrt();
        ry *= grow.sqrt();
    }
    let num = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut k = (num / den).sqrt();
    if large == sweep {
        k = -k;
    }
    let (cx1, cy1) = (k * rx * y1 / ry, -k * ry * x1 / rx);
    let mid = (from.to_vec2() + to.to_vec2()) / 2.0;
    let (cx, cy) = (cos * cx1 - sin * cy1 + mid.x, sin * cx1 + cos * cy1 + mid.y);
    let angle = |ux: f32, uy: f32| uy.atan2(ux);
    let a1 = angle((x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut da = angle((-x1 - cx1) / rx, (-y1 - cy1) / ry) - a1;
    let tau = std::f32::consts::TAU;
    if sweep && da < 0.0 {
        da += tau;
    } else if !sweep && da > 0.0 {
        da -= tau;
    }
    let steps = ((da.abs() / tau) * 40.0).ceil().max(2.0) as usize;
    let mut out: Vec<Pos2> = (1..steps)
        .map(|i| {
            let a = a1 + da * i as f32 / steps as f32;
            let (x, y) = (rx * a.cos(), ry * a.sin());
            Pos2::new(cos * x - sin * y + cx, sin * x + cos * y + cy)
        })
        .collect();
    out.push(to);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_drawing_is_read_and_stays_on_its_grid() {
        for icon in ALL {
            let svg = drawing(icon);
            let parts = parts(icon);
            let elements = svg.matches("<path").count()
                + svg.matches("<circle").count()
                + svg.matches("<ellipse").count()
                + svg.matches("<rect").count();
            assert!(parts.len() >= elements, "{icon:?}: {} parts", parts.len());
            for p in parts {
                assert!(p.points.len() >= 2, "{icon:?}: a part with one point");
                for q in &p.points {
                    assert!(
                        (0.0..=24.0).contains(&q.x) && (0.0..=24.0).contains(&q.y),
                        "{icon:?}: {q:?} is off the grid"
                    );
                }
            }
        }
    }

    #[test]
    fn numbers_split_where_svg_runs_them_together() {
        assert_eq!(numbers("1.6-2.2.4 .9"), vec![1.6, -2.2, 0.4, 0.9]);
        assert_eq!(numbers("-.8-.3-1.6"), vec![-0.8, -0.3, -1.6]);
    }

    #[test]
    fn a_path_follows_its_commands() {
        let runs = path("M3 4h2v3H3zM10 10l1 1");
        assert_eq!(runs.len(), 2);
        let (square, closed) = &runs[0];
        assert!(closed);
        assert_eq!(
            square,
            &vec![
                Pos2::new(3.0, 4.0),
                Pos2::new(5.0, 4.0),
                Pos2::new(5.0, 7.0),
                Pos2::new(3.0, 7.0),
            ]
        );
        assert_eq!(
            runs[1],
            (vec![Pos2::new(10.0, 10.0), Pos2::new(11.0, 11.0)], false)
        );
    }

    #[test]
    fn a_half_circle_arc_bulges_the_right_way() {
        // The speedometer's dial: from left to right over the top.
        let pts = arc(
            Pos2::new(3.5, 17.0),
            Pos2::new(20.5, 17.0),
            8.5,
            8.5,
            0.0,
            false,
            true,
        );
        let top = pts.iter().map(|p| p.y).fold(f32::MAX, f32::min);
        assert!((top - 8.5).abs() < 0.1, "top at {top}");
        assert_eq!(*pts.last().unwrap(), Pos2::new(20.5, 17.0));
    }

    #[test]
    fn a_dented_outline_is_filled_without_its_dent() {
        // An arrowhead pointing up, with a notch cut into its base.
        let pts = [
            Pos2::new(0.0, 0.0),
            Pos2::new(2.0, 1.0),
            Pos2::new(4.0, 0.0),
            Pos2::new(2.0, 4.0),
        ];
        let tris = triangles(&pts);
        assert_eq!(tris.len(), 2);
        let area: f32 = tris
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| pts[i as usize]);
                ((b - a).x * (c - a).y - (b - a).y * (c - a).x).abs() / 2.0
            })
            .sum();
        // The outline's own area: 8 for the triangle less 2 for the notch.
        assert!((area - 6.0).abs() < 1e-4, "area {area}");
    }

    #[test]
    fn dashes_keep_their_lengths() {
        let line = [Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0)];
        let runs = dashes(&line, false, 2.0, 2.0);
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[1], vec![Pos2::new(4.0, 0.0), Pos2::new(6.0, 0.0)]);
    }
}
