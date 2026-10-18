//! Saving a pane as a picture: a PNG cut from the screen, or an SVG
//! drawing written from the shapes egui painted for the pane.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, Shape};
use eframe::epaint::{ClippedShape, ColorMode, PathStroke, StrokeKind, TextShape};

/// Which kind of picture a path asks for: SVG by its extension, PNG
/// otherwise.
pub fn is_svg(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
}

/// The save dialog for a pane's picture, starting in `dir` with the pane's
/// name. A name typed without an extension is saved as a PNG.
pub fn ask_path(title: &str, dir: Option<&Path>) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new()
        .set_title(format!("Export {title}"))
        .add_filter("PNG Picture", &["png", "PNG"])
        .add_filter("SVG Drawing", &["svg", "SVG"])
        .set_file_name(format!("{title}.png"));
    if let Some(dir) = dir.filter(|d| d.is_dir()) {
        d = d.set_directory(dir);
    }
    let p = d.save_file()?;
    Some(if p.extension().is_none() {
        p.with_extension("png")
    } else {
        p
    })
}

/// The part of a screenshot under `rect`, in points at `pixels_per_point`.
pub fn crop(image: &ColorImage, rect: Rect, pixels_per_point: f32) -> ColorImage {
    let [w, h] = image.size;
    let px = |v: f32, max: usize| ((v * pixels_per_point).round().max(0.0) as usize).min(max);
    let (x0, x1) = (px(rect.left(), w), px(rect.right(), w));
    let (y0, y1) = (px(rect.top(), h), px(rect.bottom(), h));
    let (cw, ch) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    let mut pixels = Vec::with_capacity(cw * ch);
    for y in y0..y0 + ch {
        pixels.extend_from_slice(&image.pixels[y * w + x0..y * w + x0 + cw]);
    }
    ColorImage::new([cw, ch], pixels)
}

/// Write an image as an 8-bit RGBA PNG.
pub fn write_png(path: &Path, image: &ColorImage) -> std::io::Result<()> {
    let [w, h] = image.size;
    if w == 0 || h == 0 {
        return Err(std::io::Error::other("the pane has no area to save"));
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(std::io::Error::other)?;
    let data: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|c| c.to_srgba_unmultiplied())
        .collect();
    writer
        .write_image_data(&data)
        .map_err(std::io::Error::other)?;
    writer.finish().map_err(std::io::Error::other)
}

/// An SVG drawing of the shapes inside `rect`, on a `background` fill,
/// one SVG unit to a point. Shapes keep their clipping; images (textured
/// meshes) and backend callbacks are left out.
pub fn svg(shapes: &[ClippedShape], rect: Rect, background: Color32) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="{x} {y} {w} {h}">"#,
        x = n(rect.left()),
        y = n(rect.top()),
        w = n(rect.width()),
        h = n(rect.height()),
    );
    let _ = writeln!(
        out,
        r#"<rect x="{}" y="{}" width="{}" height="{}"{}/>"#,
        n(rect.left()),
        n(rect.top()),
        n(rect.width()),
        n(rect.height()),
        paint("fill", background),
    );
    let mut clips: Vec<Rect> = Vec::new();
    let mut body = String::new();
    let mut open: Option<usize> = None;
    for c in shapes {
        let clip = c.clip_rect.intersect(rect);
        if !clip.is_positive() || !c.shape.visual_bounding_rect().intersects(clip) {
            continue;
        }
        let i = match clips.iter().position(|r| *r == clip) {
            Some(i) => i,
            None => {
                clips.push(clip);
                clips.len() - 1
            }
        };
        if open != Some(i) {
            if open.is_some() {
                body.push_str("</g>\n");
            }
            let _ = writeln!(body, r#"<g clip-path="url(#c{i})">"#);
            open = Some(i);
        }
        shape(&mut body, &c.shape);
    }
    if open.is_some() {
        body.push_str("</g>\n");
    }
    if !clips.is_empty() {
        out.push_str("<defs>\n");
        for (i, r) in clips.iter().enumerate() {
            let _ = writeln!(
                out,
                r#"<clipPath id="c{i}"><rect x="{}" y="{}" width="{}" height="{}"/></clipPath>"#,
                n(r.left()),
                n(r.top()),
                n(r.width()),
                n(r.height()),
            );
        }
        out.push_str("</defs>\n");
    }
    out.push_str(&body);
    out.push_str("</svg>\n");
    out
}

/// A number short enough to read: two decimals at most.
fn n(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

/// ` fill="#rrggbb"` with its opacity, or ` fill="none"` when clear.
fn paint(attr: &str, c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    match a {
        0 => format!(r#" {attr}="none""#),
        255 => format!(r##" {attr}="#{r:02x}{g:02x}{b:02x}""##),
        a => format!(
            r##" {attr}="#{r:02x}{g:02x}{b:02x}" {attr}-opacity="{}""##,
            n(a as f32 / 255.0)
        ),
    }
}

fn stroke_attrs(width: f32, color: Color32) -> String {
    if width <= 0.0 || color.a() == 0 {
        return r#" stroke="none""#.into();
    }
    format!(r#"{} stroke-width="{}""#, paint("stroke", color), n(width))
}

fn path_stroke_color(s: &PathStroke, bounds: Rect) -> Color32 {
    match &s.color {
        ColorMode::Solid(c) => *c,
        ColorMode::UV(f) => f(bounds, bounds.center()),
    }
}

fn points(pts: &[Pos2]) -> String {
    pts.iter()
        .map(|p| format!("{},{}", n(p.x), n(p.y)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shape(out: &mut String, s: &Shape) {
    match s {
        Shape::Noop | Shape::Callback(_) => {}
        Shape::Vec(v) => v.iter().for_each(|s| shape(out, s)),
        Shape::Circle(c) => {
            let _ = writeln!(
                out,
                r#"<circle cx="{}" cy="{}" r="{}"{}{}/>"#,
                n(c.center.x),
                n(c.center.y),
                n(c.radius),
                paint("fill", c.fill),
                stroke_attrs(c.stroke.width, c.stroke.color),
            );
        }
        Shape::Ellipse(e) => {
            let _ = writeln!(
                out,
                r#"<ellipse cx="{}" cy="{}" rx="{}" ry="{}"{}{}/>"#,
                n(e.center.x),
                n(e.center.y),
                n(e.radius.x),
                n(e.radius.y),
                paint("fill", e.fill),
                stroke_attrs(e.stroke.width, e.stroke.color),
            );
        }
        Shape::LineSegment {
            points: [a, b],
            stroke,
        } => {
            let _ = writeln!(
                out,
                r#"<line x1="{}" y1="{}" x2="{}" y2="{}"{}/>"#,
                n(a.x),
                n(a.y),
                n(b.x),
                n(b.y),
                stroke_attrs(stroke.width, stroke.color),
            );
        }
        Shape::Path(p) => {
            if p.points.is_empty() {
                return;
            }
            let bounds = Rect::from_points(&p.points);
            let color = path_stroke_color(&p.stroke, bounds);
            let tag = if p.closed { "polygon" } else { "polyline" };
            let fill = if p.closed {
                paint("fill", p.fill)
            } else {
                r#" fill="none""#.into()
            };
            let _ = writeln!(
                out,
                r#"<{tag} points="{}"{fill}{} stroke-linejoin="round"/>"#,
                points(&p.points),
                stroke_attrs(p.stroke.width, color),
            );
        }
        Shape::Rect(r) => {
            // egui strokes inside, on, or outside the edge; SVG on it.
            let w = r.stroke.width;
            let rect = match r.stroke_kind {
                StrokeKind::Inside => r.rect.shrink(w / 2.0),
                StrokeKind::Middle => r.rect,
                StrokeKind::Outside => r.rect.expand(w / 2.0),
            };
            let cr = r.corner_radius;
            let radius = (cr.nw as f32 + cr.ne as f32 + cr.sw as f32 + cr.se as f32) / 4.0;
            let rx = if radius > 0.0 {
                format!(r#" rx="{}""#, n(radius))
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                r#"<rect x="{}" y="{}" width="{}" height="{}"{rx}{}{}/>"#,
                n(rect.left()),
                n(rect.top()),
                n(rect.width().max(0.0)),
                n(rect.height().max(0.0)),
                paint("fill", r.fill),
                stroke_attrs(w, r.stroke.color),
            );
        }
        Shape::Text(t) => text(out, t),
        Shape::Mesh(m) => {
            // Flat colour comes from the font texture's white pixel; any
            // other texture is a picture, which isn't carried over.
            if m.texture_id != egui::TextureId::default() {
                return;
            }
            for tri in m.indices.as_chunks::<3>().0 {
                let v: Vec<_> = tri
                    .iter()
                    .filter_map(|&i| m.vertices.get(i as usize))
                    .collect();
                if v.len() < 3 || v[0].color.a() == 0 {
                    continue;
                }
                let pts: Vec<Pos2> = v.iter().map(|v| v.pos).collect();
                let _ = writeln!(
                    out,
                    r#"<polygon points="{}"{}/>"#,
                    points(&pts),
                    paint("fill", v[0].color),
                );
            }
        }
        Shape::QuadraticBezier(q) => {
            let [a, b, c] = q.points;
            let fill = if q.closed {
                paint("fill", q.fill)
            } else {
                r#" fill="none""#.into()
            };
            let bounds = Rect::from_points(&q.points);
            let _ = writeln!(
                out,
                r#"<path d="M{},{} Q{},{} {},{}{}"{fill}{}/>"#,
                n(a.x),
                n(a.y),
                n(b.x),
                n(b.y),
                n(c.x),
                n(c.y),
                if q.closed { " Z" } else { "" },
                stroke_attrs(q.stroke.width, path_stroke_color(&q.stroke, bounds)),
            );
        }
        Shape::CubicBezier(q) => {
            let [a, b, c, d] = q.points;
            let fill = if q.closed {
                paint("fill", q.fill)
            } else {
                r#" fill="none""#.into()
            };
            let bounds = Rect::from_points(&q.points);
            let _ = writeln!(
                out,
                r#"<path d="M{},{} C{},{} {},{} {},{}{}"{fill}{}/>"#,
                n(a.x),
                n(a.y),
                n(b.x),
                n(b.y),
                n(c.x),
                n(c.y),
                n(d.x),
                n(d.y),
                if q.closed { " Z" } else { "" },
                stroke_attrs(q.stroke.width, path_stroke_color(&q.stroke, bounds)),
            );
        }
    }
}

fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c if (c as u32) < 0x20 => {}
            c => o.push(c),
        }
    }
    o
}

/// The font a section asks for, as SVG font attributes.
fn font_attrs(f: &egui::FontId) -> String {
    let family = match &f.family {
        egui::FontFamily::Monospace => r#"font-family="monospace""#.to_string(),
        egui::FontFamily::Name(name) if name.as_ref() == "display" => {
            r#"font-family="Inter Display, Inter, sans-serif" font-weight="600""#.into()
        }
        egui::FontFamily::Name(name) if name.as_ref() == "semibold" => {
            r#"font-family="Inter, sans-serif" font-weight="600""#.into()
        }
        _ => r#"font-family="Inter, sans-serif""#.into(),
    };
    format!(r#"{family} font-size="{}""#, n(f.size))
}

/// Text as one `<text>` a row, a `<tspan>` for each run of one style, each
/// placed where egui laid its first letter.
fn text(out: &mut String, t: &TextShape) {
    let job = &t.galley.job;
    // Which section each letter of the text belongs to, so the laid-out
    // letters can be matched back to their colour and font.
    let mut letters: Vec<(char, usize)> = Vec::with_capacity(job.text.len());
    for (i, s) in job.sections.iter().enumerate() {
        if let Some(part) = job.text.get(s.byte_range.clone()) {
            letters.extend(part.chars().map(|c| (c, i)));
        }
    }
    let mut at = 0;
    let section_of = |c: char, at: &mut usize| -> usize {
        let from = *at;
        while *at < letters.len() {
            let (l, s) = letters[*at];
            *at += 1;
            if l == c {
                return s;
            }
        }
        *at = from;
        letters.get(from).map_or(0, |l| l.1)
    };
    let transform = if t.angle != 0.0 {
        format!(
            r#" transform="rotate({} {} {})""#,
            n(t.angle.to_degrees()),
            n(t.pos.x),
            n(t.pos.y)
        )
    } else {
        String::new()
    };
    for row in &t.galley.rows {
        let mut runs: Vec<(usize, Pos2, String)> = Vec::new();
        for g in &row.row.glyphs {
            let s = section_of(g.chr, &mut at);
            match runs.last_mut() {
                Some((rs, _, txt)) if *rs == s => txt.push(g.chr),
                _ => runs.push((s, t.pos + row.pos.to_vec2() + g.pos.to_vec2(), g.chr.into())),
            }
        }
        if runs.iter().all(|(_, _, txt)| txt.trim().is_empty()) {
            continue;
        }
        let _ = write!(out, r#"<text xml:space="preserve"{transform}>"#);
        for (s, pos, txt) in runs {
            let Some(section) = job.sections.get(s) else {
                continue;
            };
            let f = &section.format;
            let mut color = t
                .override_text_color
                .unwrap_or(if f.color == Color32::PLACEHOLDER {
                    t.fallback_color
                } else {
                    f.color
                });
            if t.opacity_factor < 1.0 {
                color = color.gamma_multiply(t.opacity_factor);
            }
            let _ = write!(
                out,
                r#"<tspan x="{}" y="{}" {}{}>{}</tspan>"#,
                n(pos.x),
                n(pos.y),
                font_attrs(&f.font_id),
                paint("fill", color),
                escape(&txt),
            );
        }
        out.push_str("</text>\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Stroke, Vec2};

    #[test]
    fn a_png_is_written_and_read_back() {
        let mut img = ColorImage::filled([20, 10], Color32::from_rgb(10, 20, 30));
        img.pixels[0] = Color32::RED;
        let part = crop(
            &img,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(5.0, 2.5)),
            2.0,
        );
        assert_eq!(part.size, [10, 5]);
        assert_eq!(part.pixels[0], Color32::RED);
        let dir = std::env::temp_dir().join(format!("tg-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chart.png");
        write_png(&path, &part).unwrap();
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let info = r.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (10, 5));
        assert_eq!(&buf[..4], &[255, 0, 0, 255]);
        assert_eq!(&buf[4..8], &[10, 20, 30, 255]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_crop_past_the_edge_stops_at_it() {
        let img = ColorImage::filled([8, 8], Color32::WHITE);
        let part = crop(
            &img,
            Rect::from_min_max(Pos2::new(4.0, 4.0), Pos2::new(20.0, 20.0)),
            1.0,
        );
        assert_eq!(part.size, [4, 4]);
    }

    #[test]
    fn svg_carries_lines_shapes_and_text_and_leaves_out_what_lies_outside() {
        let ctx = egui::Context::default();
        let mut shapes = Vec::new();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let galley = ctx.fonts_mut(|f| {
                f.layout_no_wrap(
                    "Rate <s/day>".into(),
                    egui::FontId::proportional(12.0),
                    Color32::from_rgb(200, 30, 40),
                )
            });
            let area = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(200.0, 100.0));
            let mut add = |s: Shape| {
                shapes.push(ClippedShape {
                    clip_rect: area,
                    shape: s,
                })
            };
            add(Shape::line(
                vec![
                    Pos2::new(20.0, 20.0),
                    Pos2::new(60.0, 40.0),
                    Pos2::new(90.0, 30.0),
                ],
                Stroke::new(1.5_f32, Color32::from_rgb(0, 128, 255)),
            ));
            add(Shape::circle_filled(
                Pos2::new(50.0, 50.0),
                3.0,
                Color32::from_rgb(255, 160, 0),
            ));
            add(Shape::rect_filled(
                Rect::from_min_size(Pos2::new(30.0, 60.0), Vec2::new(20.0, 10.0)),
                0.0,
                Color32::from_rgba_unmultiplied(100, 100, 100, 128),
            ));
            add(Shape::galley(Pos2::new(15.0, 80.0), galley, Color32::WHITE));
            // Off to the side of the picture: left out.
            add(Shape::circle_filled(
                Pos2::new(500.0, 500.0),
                3.0,
                Color32::GREEN,
            ));
        });
        let area = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(200.0, 100.0));
        let s = svg(&shapes, area, Color32::from_rgb(250, 250, 250));
        assert!(s.starts_with("<svg "), "{s}");
        assert!(s.contains(r#"viewBox="10 10 200 100""#), "{s}");
        assert!(s.contains(r##"fill="#fafafa""##), "background: {s}");
        assert!(
            s.contains(r##"<polyline points="20,20 60,40 90,30" fill="none" stroke="#0080ff" stroke-width="1.5""##),
            "{s}"
        );
        assert!(
            s.contains(r##"<circle cx="50" cy="50" r="3" fill="#ffa000""##),
            "{s}"
        );
        assert!(s.contains(r#"fill-opacity="0.5""#), "{s}");
        assert!(s.contains("Rate &lt;s/day&gt;"), "{s}");
        assert!(s.contains(r##"fill="#c81e28""##), "text colour: {s}");
        assert!(s.contains(r#"font-size="12""#), "{s}");
        assert!(!s.contains("#00ff00"), "outside shape kept: {s}");
        assert!(s.contains(r#"<clipPath id="c0">"#), "{s}");
        assert!(s.trim_end().ends_with("</svg>"));
    }

    #[test]
    fn a_name_without_svg_is_a_png() {
        assert!(is_svg(Path::new("a/Chart.SVG")));
        assert!(!is_svg(Path::new("a/Chart.png")));
        assert!(!is_svg(Path::new("a/Chart")));
    }
}
