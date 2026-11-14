//! The Complication pane (#1576, Hugh's spec, Peter's calls of 13 Nov
//! 2026): the guided test with a complication off, on, then off again in
//! one position, its table, and three plots on one time axis, Amplitude,
//! Rate, then Beat Error, each with the two Offs joined and its finding in
//! words. Every figure and word is core's
//! ([`timegrapher_core::complication`]); this lays them out.

use super::*;
use timegrapher_core::chart_scale;
use timegrapher_core::complication::{self, Change};
use timegrapher_core::figure_text::true_minus;
use timegrapher_core::library::length_text;
use timegrapher_core::session::Measurement;
use timegrapher_core::steadiness::SeriesKind;

/// The table's width: the stretch's name and its three figures.
const TABLE_W: f32 = 400.0;
/// The least width of the plots.
const PLOTS_MIN_W: f32 = 360.0;
/// The pane width Info needs to sit beside the rest.
const INFO_BESIDE_W: f32 = 1180.0;
/// Each plot's least height, the track's over them, and the time axis's
/// under them, points.
const PLOT_MIN_H: f32 = 110.0;
/// Each plot's height over the table on a narrow pane: room to read, and
/// the table still in view under them (Walter).
const PLOT_OVER_H: f32 = 150.0;
const TRACK_H: f32 = 16.0;
const AXIS_H: f32 = 34.0;
/// A plot's margins: left for its numbers, top for its title and the
/// room its top number takes above its line.
const LEFT: f32 = 70.0;
const RIGHT: f32 = 14.0;
const TITLE_H: f32 = 30.0;
/// The room under a plot's lowest line: half its number's height, then
/// the 12 points the Charts lanes leave before the next plot's title
/// (Hugh).
const FOOT_H: f32 = 20.0;
/// A point's radius, its ± bar's width, and the ring's radius.
const DOT_R: f32 = 4.2;
const BAR_W: f32 = 1.5;
const RING_R: f32 = 4.0;
/// The bracket's reach right of the On point, points.
const BRACKET: f32 = 8.0;
/// The Offs' dashed line: its dashes and gaps, points.
const DASH: f32 = 6.0;
const DASH_GAP: f32 = 4.0;
/// The least span of a plot's scale, in places of its figure: five
/// steps of 1° or 0.1 s/d, never ticks of 0.02° (Walter).
const MIN_SPAN_PLACES: f32 = 5.0;
/// How many marks the time axis wants at most.
const TIME_MARKS: f32 = 6.0;
/// How near the pointer must come to a point for its figure, points.
const HOVER_R: f32 = 8.0;
/// The gap between the key's parts, and a mark's room before its words,
/// points.
const KEY_GAP: f32 = 14.0;
const KEY_MARK_W: f32 = 2.0 * RING_R + 8.0;

/// One stretch on the plots: its middle, seconds from the test's start,
/// and its measurement.
struct Point<'a> {
    t: f64,
    m: &'a Measurement,
}

impl TimegrapherApp {
    /// The Complication Test, if that is the guided test on the Bench.
    pub(super) fn complication_test(&self) -> Option<&guided::Test> {
        self.guided
            .as_ref()
            .filter(|t| t.kind == guided::Kind::Complication)
    }

    /// An opened take's Complication Test from its session file at
    /// `dir`: the three stretches it lists as Off, On and Off, in that
    /// order, at one position (#1620).
    pub(super) fn load_saved_complication(&mut self, dir: &std::path::Path) {
        let m: Option<Manifest> = std::fs::read_to_string(dir.join(MANIFEST))
            .ok()
            .and_then(|t| toml::from_str(&t).ok());
        let entries: Vec<_> = m
            .map(|m| m.recordings)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.complication.is_some())
            .collect();
        let steps: Vec<String> = complication::ORDER
            .iter()
            .map(|s| s.name().to_lowercase())
            .collect();
        let named: Vec<String> = entries
            .iter()
            .filter_map(|e| e.complication.clone())
            .collect();
        let position = entries
            .first()
            .and_then(|e| Position::from_name(e.position.as_deref()?));
        let one_place = entries
            .iter()
            .all(|e| e.position == entries.first().and_then(|f| f.position.clone()));
        let spans: Option<Vec<(f64, f64)>> = entries
            .iter()
            .map(|e| Some((e.start_s?, e.end_s?)))
            .collect();
        self.complication_saved = match (position, spans) {
            (Some(p), Some(spans)) if named == steps && one_place => {
                guided::Test::saved_complication(p, &spans)
            }
            _ => None,
        };
        if self.complication_saved.is_some() {
            self.reveal_pane(Pane::Complication);
        }
    }

    /// The opened take's Complication Test, read once its beats are in
    /// and no test is on the Bench.
    fn saved_complication_read(&mut self) -> Option<&guided::Test> {
        if self.guided.is_some() || self.running() {
            return None;
        }
        let unread = self
            .complication_saved
            .as_ref()
            .is_some_and(|t| t.readings.is_none());
        if unread && self.batch.is_none() {
            let log = self.live.as_ref().and_then(steady::log_of)?;
            let clock = self.reading_clock(log.duration_s);
            let bph = self.live.as_ref().and_then(|l| l.config().analysis.bph);
            let masked = self.masked.clone();
            if let Some(t) = self.complication_saved.as_mut() {
                t.bph_set = bph;
                t.finish(&log, clock.as_ref(), &masked);
            }
        }
        self.complication_saved
            .as_ref()
            .filter(|t| t.readings.is_some())
    }

    /// The pane's toolbar: New Test or Cancel Test while none runs, the
    /// Settle and Each Stretch every guided test shares, and Copy.
    fn complication_settings(&mut self, ui: &mut egui::Ui) {
        let running = self.guided_running();
        if !running {
            if self.ready_test() == Some(NewTake::Complication) {
                if theme::tool_word_button(
                    ui,
                    Icon::Back,
                    "Cancel Test",
                    "Leave the test set up, with nothing started or kept",
                    false,
                    None,
                ) {
                    self.leave_ready_test();
                }
            } else if theme::tool_word_button(
                ui,
                Icon::Listen,
                "New Test",
                "Set a Complication Test up on a clear Bench, in the position the card shows, \
                 then press Start Test on the Listen card. It measures the watch with the \
                 complication off, on, then off again.",
                true,
                None,
            ) {
                self.new_take(NewTake::Complication, false);
            }
            theme::bar_gap(ui);
        }
        self.positions_lengths(ui, running, "Each Stretch");
        theme::bar_gap(ui);
        let nothing = self
            .complication_test()
            .is_none_or(|t| t.readings_so_far().is_empty())
            .then_some("Nothing measured yet");
        if theme::tool_word_button(
            ui,
            Icon::Copy,
            "Copy",
            "Copy the watch, its calibre, the table with tabs between the columns, and the \
             findings in words, to paste into a spreadsheet or a message",
            false,
            nothing,
        ) {
            let about = self.guided_about();
            if let Some(t) = self.complication_test() {
                ui.ctx().copy_text(guided::export_text(t, &about, '\t'));
                self.info_msg("Copied the Complication table".to_string());
            }
        }
    }

    /// The pane's toolbar, with the Info toggle pinned at its right.
    pub(super) fn complication_tools(&mut self, ui: &mut egui::Ui, title: &str) {
        let mut info = self.complication_info;
        let hint = "Info: how On less Off and its ± are worked out";
        theme::pane_toolbar_info(ui, title, &help::COMPLICATION, (&mut info, hint), |ui| {
            self.complication_settings(ui);
            self.export_button(ui, Pane::Complication);
        });
        self.complication_info = info;
    }

    /// The pane: the banner while the test runs, then the table and the
    /// three plots, and the Info panel.
    pub(super) fn complication_pane(&mut self, ui: &mut egui::Ui) {
        // Where Keep This Test? keeps clear of, as the other tests'.
        if self.complication_test().is_some() {
            self.results_rects[0] = Some(ui.max_rect());
        }
        let now = self.live.as_ref().map(|l| l.duration_s());
        let phase = match (self.guided.as_mut(), now) {
            (Some(t), Some(now)) if t.kind == guided::Kind::Complication => Some(t.tick(now)),
            _ => None,
        };
        let live = match phase {
            Some(guided::Phase::Measuring(_)) => self.guided_live(ui.ctx()),
            _ => None,
        };
        let lengths = (self.guided_settle_s, self.guided_measure_s);
        // No test on the Bench: an opened take's own, read again (#1620).
        if self.complication_test().is_none() {
            let _ = self.saved_complication_read();
        }
        let test = self.complication_test().or_else(|| {
            self.complication_saved
                .as_ref()
                .filter(|t| self.guided.is_none() && t.readings.is_some())
        });
        let action = complication_body(
            ui,
            (test, phase, live.as_ref()),
            (now, lengths),
            self.complication_info,
        );
        if let (Some(action), Some(now)) = (action, now) {
            self.apply_guided_action(action, now);
        }
    }
}

/// The pane's body under its toolbar; what a banner button asked for.
fn complication_body(
    ui: &mut egui::Ui,
    (test, phase, live): (
        Option<&guided::Test>,
        Option<guided::Phase>,
        Option<&guided::FreeCell>,
    ),
    (now, (settle_s, measure_s)): (Option<f64>, (f64, f64)),
    info: bool,
) -> Option<guided::Action> {
    let bottom = ui.available_rect_before_wrap().bottom();
    let layout = test_body::Layout {
        table_ws: &[TABLE_W],
        result_min_w: PLOTS_MIN_W,
        info_beside_w: INFO_BESIDE_W,
        rule_h: 3.0 * PLOT_MIN_H,
    };
    // Over the table, on a narrow pane, the plots take their least height
    // so the table shows under them (Walter).
    let side_w = ui.available_width() - TABLE_W - test_body::GAP;
    let plots = |ui: &mut egui::Ui| {
        let over = ui.available_width() > side_w + 1.0;
        plots_block(ui, test, now, (!over).then_some(bottom));
    };
    test_body::test_body(
        ui,
        ("complication pane", &layout),
        (test, phase, live, None),
        info,
        &test_body::Parts {
            about: about_text(settle_s, measure_s),
            kept: None,
            result: &plots,
            info: &info_panel,
        },
    )
}

/// What the pane says before a test: how the test runs.
fn about_text(settle_s: f64, measure_s: f64) -> String {
    format!(
        "Start Test, on the Listen card, measures the watch in the position the card shows, \
         with its complication off, on, then off again:\n\
         1. It says when to turn the complication on or off.\n\
         2. It lets the watch settle {settle_s:.0} s.\n\
         3. It measures it for {measure_s:.0} s and fills in its row.\n\
         After the third, the test ends by itself, gives On less Off and keeps the take."
    )
}

/// The Info panel: how On less Off and its ± are worked out, in core's
/// words.
fn info_panel(ui: &mut egui::Ui) {
    let pal = theme::pal(ui);
    ui.spacing_mut().item_spacing.y = 4.0;
    ui.label(theme::caption("Complication").color(pal.text_secondary));
    // Core's short paragraphs, each its own label (Hugh).
    for para in complication::method_note().split("\n\n") {
        ui.add(
            egui::Label::new(RichText::new(para).small().color(pal.text))
                .wrap()
                .selectable(true),
        );
        ui.add_space(4.0);
    }
}

/// The three plots on one time axis, with the track naming each stretch
/// over them and the key under them, in the room left above `bottom`.
fn plots_block(
    ui: &mut egui::Ui,
    test: Option<&guided::Test>,
    now: Option<f64>,
    bottom: Option<f32>,
) {
    let w = ui.available_width().max(PLOTS_MIN_W);
    let key_h = key_height(ui, w) + 6.0;
    let plot_h = bottom.map_or(PLOT_OVER_H, |bottom| {
        let room = bottom - ui.cursor().top() - TRACK_H - AXIS_H - key_h;
        (room / 3.0).max(PLOT_MIN_H)
    });
    let h = TRACK_H + 3.0 * plot_h + AXIS_H;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), egui::Sense::hover());
    let tips = draw_plots(ui, rect, plot_h, test, now);
    // Each point's figure, on hover, as the key says.
    if let Some(p) = resp.hover_pos() {
        let near = tips
            .iter()
            .filter(|(at, _)| at.distance(p) <= HOVER_R)
            .min_by(|a, b| a.0.distance(p).total_cmp(&b.0.distance(p)));
        if let Some((_, text)) = near {
            resp.on_hover_text_at_pointer(text.as_str());
        }
    }
    plot_key(ui);
}

/// The time the plots span, seconds of the session: from the test's start
/// to now, or to the last stretch's end, and never under the three
/// stretches' own length.
fn time_span(test: &guided::Test, now: Option<f64>) -> (f64, f64) {
    let t0 = test.began_s().unwrap_or(0.0);
    let end = test
        .stretches()
        .iter()
        .map(|&(_, _, b)| b)
        .fold(now.unwrap_or(t0), f64::max);
    let least = complication::ORDER.len() as f64 * (test.settle_s + test.measure_s);
    (t0, end.max(t0 + least))
}

/// The time axis's step, seconds: the first of the app's time steps
/// ([`theme::TIME_STEPS`]), in whole seconds, that gives no more than
/// [`TIME_MARKS`] marks over `span`.
fn time_step(span: f64) -> f64 {
    let most = f64::from(TIME_MARKS);
    let whole = theme::TIME_STEPS
        .iter()
        .copied()
        .filter(|s| s.fract() == 0.0);
    whole
        .clone()
        .find(|s| span / s <= most)
        .or_else(|| whole.clone().next_back())
        .unwrap_or(span)
}

/// The three plots, the track over them and the time axis under them;
/// each point drawn, with its figure for the hover.
fn draw_plots(
    ui: &egui::Ui,
    rect: egui::Rect,
    plot_h: f32,
    test: Option<&guided::Test>,
    now: Option<f64>,
) -> Vec<(egui::Pos2, String)> {
    let pal = theme::pal(ui);
    let painter = ui.painter_at(rect);
    let left = rect.left() + LEFT;
    let right = rect.right() - RIGHT;
    let (t0, t1) = test.map_or((0.0, 1.0), |t| time_span(t, now));
    let x = |t: f64| left + ((t - t0) / (t1 - t0)) as f32 * (right - left);
    if let Some(t) = test {
        draw_track(&painter, t, now, (rect.top(), &x), pal);
    }
    let readings = test.map_or(&[][..], |t| t.readings_so_far());
    let points: Vec<Point<'_>> = readings
        .iter()
        .map(|r| Point {
            t: complication::middle(&r.measurement),
            m: &r.measurement,
        })
        .collect();
    let measurements: Vec<Measurement> = readings.iter().map(|r| r.measurement.clone()).collect();
    let grid = egui::Stroke::new(1.0_f32, pal.separator);
    let step = time_step(t1 - t0);
    let plots_top = rect.top() + TRACK_H;
    let plots_bottom = plots_top + 3.0 * plot_h;
    let mut k = 0.0;
    let mut marks = Vec::new();
    while t0 + k * step <= t1 {
        let at = x(t0 + k * step);
        marks.push(at);
        painter.text(
            egui::pos2(at, plots_bottom + 4.0),
            egui::Align2::CENTER_TOP,
            length_text(k * step),
            egui::FontId::proportional(11.0),
            pal.text_secondary,
        );
        k += 1.0;
    }
    painter.text(
        egui::pos2(right, plots_bottom + AXIS_H),
        egui::Align2::RIGHT_BOTTOM,
        "Time, min:s",
        egui::FontId::proportional(11.0),
        pal.text_secondary,
    );
    let mut tips = Vec::new();
    for (i, q) in complication::SHOWN.into_iter().enumerate() {
        let top = plots_top + i as f32 * plot_h;
        let area = egui::Rect::from_min_max(
            egui::pos2(left, top + TITLE_H),
            egui::pos2(right, top + plot_h - FOOT_H),
        );
        for &at in &marks {
            painter.vline(at, area.y_range(), grid);
        }
        let change = complication::change(q, &measurements);
        plot_title(&painter, egui::pos2(rect.left(), top), q, change, pal);
        tips.extend(draw_one(&painter, area, (q, &points, change), &x, pal));
    }
    tips
}

/// The track over the plots: each stretch named, faint while it settles.
fn draw_track(
    painter: &egui::Painter,
    test: &guided::Test,
    now: Option<f64>,
    (top, x): (f32, &dyn Fn(f64) -> f32),
    pal: &theme::Palette,
) {
    let font = egui::FontId::proportional(11.0);
    let spans = test.measured(now.unwrap_or(0.0));
    for (i, &(from, to)) in spans.iter().enumerate() {
        let settle_from = from - test.settle_s;
        let band = |a: f64, b: f64, c: egui::Color32| {
            let r =
                egui::Rect::from_min_max(egui::pos2(x(a), top), egui::pos2(x(b), top + TRACK_H));
            painter.rect_filled(r, 2.0, c);
        };
        // One block for the stretch, its settle a lighter fill of the same
        // bar, in the strip's proportion (Hugh's spec).
        band(settle_from, to, pal.control);
        let veil = egui::Rect::from_min_max(
            egui::pos2(x(settle_from), top),
            egui::pos2(x(from), top + TRACK_H),
        );
        let left_round = egui::CornerRadius {
            nw: 2,
            sw: 2,
            ne: 0,
            se: 0,
        };
        let shade = pal.card.gamma_multiply(1.0 - strip::SETTLE_OF_MEASURED);
        painter.rect_filled(veil, left_round, shade);
        painter.text(
            egui::pos2(x(settle_from) + 4.0, top + TRACK_H / 2.0),
            egui::Align2::LEFT_CENTER,
            test.step_code(i),
            font.clone(),
            pal.text,
        );
    }
}

/// A plot's title: its name in bold, then core's finding once all three
/// stretches are in.
fn plot_title(
    painter: &egui::Painter,
    at: egui::Pos2,
    q: SeriesKind,
    change: Option<Change>,
    pal: &theme::Palette,
) {
    let name = painter.layout_no_wrap(q.title().to_string(), theme::semibold(13.0), pal.text);
    let w = name.size().x;
    painter.galley(at, name, pal.text);
    if let Some(c) = change {
        painter.text(
            at + Vec2::new(w, 0.0),
            egui::Align2::LEFT_TOP,
            format!(
                " · {}",
                true_minus(&complication::finding_text(q, c.on_less_off))
            ),
            egui::FontId::proportional(13.0),
            pal.text,
        );
    }
}

/// One plot: its scale's lines and numbers, each stretch at its middle
/// with its ±, the Offs joined dashed, and once all three are in the ring
/// where On would sit with the complication off and the bracket from it to
/// On. Gives each point and its figure for the hover.
fn draw_one(
    painter: &egui::Painter,
    area: egui::Rect,
    (q, points, change): (SeriesKind, &[Point<'_>], Option<Change>),
    x: &dyn Fn(f64) -> f32,
    pal: &theme::Palette,
) -> Vec<(egui::Pos2, String)> {
    let figs: Vec<(f64, complication::Figure)> = points
        .iter()
        .filter_map(|p| Some((p.t, complication::figure(q, &complication::figures(p.m))?)))
        .collect();
    let mut values: Vec<f64> = figs
        .iter()
        .flat_map(|(_, f)| {
            let e = f.se.unwrap_or(0.0);
            [f.value - e, f.value + e]
        })
        .collect();
    values.extend(change.map(|c| c.off.value));
    let grid = egui::Stroke::new(1.0_f32, pal.separator);
    if let (Some(&a), Some(&b)) = (
        values.iter().min_by(|a, b| a.total_cmp(b)),
        values.iter().max_by(|a, b| a.total_cmp(b)),
    ) {
        // At least five places of the figure, about the middle.
        let least = f64::from(MIN_SPAN_PLACES) * 10f64.powi(-(q.digits() as i32));
        let mid = (a + b) / 2.0;
        values.extend([mid - least / 2.0, mid + least / 2.0]);
    }
    let Some((lo, hi)) = chart_scale::data_range(&values) else {
        painter.hline(area.x_range(), area.bottom(), grid);
        return Vec::new();
    };
    let (lo, hi, step) = chart_scale::round_ends(lo, hi);
    let y = |v: f64| area.bottom() - ((v - lo) / (hi - lo)) as f32 * area.height();
    let n = ((hi - lo) / step).round() as i64;
    for k in 0..=n {
        let v = lo + k as f64 * step;
        painter.hline(area.x_range(), y(v), grid);
        let mut label = true_minus(&chart_scale::axis_number(v, step));
        if q == SeriesKind::Rate && v > 0.0 {
            label = format!("+{label}");
        }
        painter.text(
            egui::pos2(area.left() - 6.0, y(v)),
            egui::Align2::RIGHT_CENTER,
            format!("{label}{}", complication::unit_text(q)),
            egui::FontId::proportional(11.0),
            pal.text_secondary,
        );
    }
    let colour = crate::steady::series_colour(q, pal);
    let quiet = egui::Stroke::new(1.0_f32, pal.text_secondary);
    // The Offs joined, under the points.
    if let [(ta, a), _, (tc, c)] = figs[..] {
        painter.extend(egui::Shape::dashed_line(
            &[egui::pos2(x(ta), y(a.value)), egui::pos2(x(tc), y(c.value))],
            quiet,
            DASH,
            DASH_GAP,
        ));
    }
    if let (Some(c), Some(&(t_on, on))) = (change, figs.get(1)) {
        let ring = egui::pos2(x(t_on), y(c.off.value));
        painter.circle(ring, RING_R, pal.card, quiet);
        let (a, b) = (y(c.off.value), y(on.value));
        let r = x(t_on) + BRACKET;
        painter.line_segment([egui::pos2(r - 3.0, a), egui::pos2(r, a)], quiet);
        painter.line_segment([egui::pos2(r, a), egui::pos2(r, b)], quiet);
        painter.line_segment([egui::pos2(r - 3.0, b), egui::pos2(r, b)], quiet);
    }
    let mut tips = Vec::new();
    for &(t, f) in &figs {
        let p = egui::pos2(x(t), y(f.value));
        tips.push((p, true_minus(&complication::figure_text(q, f))));
        if let Some(e) = f.se {
            let (a, b) = (
                egui::pos2(p.x, y(f.value + e)),
                egui::pos2(p.x, y(f.value - e)),
            );
            painter.line_segment([a, b], egui::Stroke::new(BAR_W, colour));
        }
        painter.circle(p, DOT_R, colour, egui::Stroke::new(1.5_f32, pal.card));
    }
    tips
}

/// One part of the key under the plots: its mark and its words.
#[derive(Clone, Copy)]
enum KeyMark {
    Dot,
    Dash,
    Ring,
    Bracket,
}

/// The key's parts, in the order they drop from the end when the row has
/// no room (Hugh's spec, mock version 8).
const KEY: [(KeyMark, &str); 4] = [
    (
        KeyMark::Dot,
        "Each stretch (bars: its ±, hover for the figure)",
    ),
    (KeyMark::Dash, "Offs joined"),
    (KeyMark::Ring, "Where On would sit with it off"),
    (KeyMark::Bracket, "The difference"),
];

/// Which row each of the key's parts sits on, each `widths` wide with
/// [`KEY_GAP`] between, in `room`: whole parts only, a part that won't
/// fit going to the next row, never cut (Hugh).
fn key_rows(widths: &[f32], room: f32) -> Vec<usize> {
    let (mut row, mut used) = (0, 0.0);
    widths
        .iter()
        .map(|&w| {
            if used > 0.0 && used + KEY_GAP + w > room {
                row += 1;
                used = 0.0;
            }
            used += if used > 0.0 { KEY_GAP } else { 0.0 } + w;
            row
        })
        .collect()
}

/// The key's words laid out, and the row each part sits on in `room`.
fn key_layout(ui: &egui::Ui, room: f32) -> (Vec<std::sync::Arc<egui::Galley>>, Vec<usize>) {
    let pal = theme::pal(ui);
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galleys: Vec<_> = KEY
        .iter()
        .map(|&(_, words)| {
            ui.painter()
                .layout_no_wrap(words.to_string(), font.clone(), pal.text_secondary)
        })
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|g| KEY_MARK_W + g.size().x).collect();
    let rows = key_rows(&widths, room);
    (galleys, rows)
}

/// The key's height in `room`: a row for each its parts take.
fn key_height(ui: &egui::Ui, room: f32) -> f32 {
    let rows = key_layout(ui, room).1;
    ui.text_style_height(&egui::TextStyle::Body) * rows.last().map_or(1, |r| r + 1) as f32
}

/// The key under the plots, for all three, from the plot titles' left
/// edge: every part whole, onto a second row where one row has no room.
fn plot_key(ui: &mut egui::Ui) {
    let pal = theme::pal(ui);
    let mark_w = KEY_MARK_W;
    let (galleys, rows) = key_layout(ui, ui.available_width());
    let h = ui.text_style_height(&egui::TextStyle::Body);
    let n_rows = rows.last().map_or(1, |r| r + 1);
    let (area, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), h * n_rows as f32),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    let quiet = egui::Stroke::new(1.0_f32, pal.text_secondary);
    let mut left = area.left();
    let mut last_row = 0;
    for (((mark, _), galley), row) in KEY.iter().zip(galleys).zip(rows) {
        if row != last_row {
            (left, last_row) = (area.left(), row);
        }
        let cy = area.top() + h * (row as f32 + 0.5);
        let c = egui::pos2(left + RING_R + 1.0, cy);
        match mark {
            KeyMark::Dot => {
                painter.line_segment([c - Vec2::new(0.0, 5.0), c + Vec2::new(0.0, 5.0)], quiet);
                painter.circle_filled(c, DOT_R - 1.0, pal.text_secondary);
            }
            KeyMark::Dash => {
                painter.line_segment([c - Vec2::new(5.0, 0.0), c - Vec2::new(1.5, 0.0)], quiet);
                painter.line_segment([c + Vec2::new(1.5, 0.0), c + Vec2::new(5.0, 0.0)], quiet);
            }
            KeyMark::Ring => {
                painter.circle(c, RING_R, pal.card, quiet);
            }
            KeyMark::Bracket => {
                let (a, b) = (c - Vec2::new(0.0, 5.0), c + Vec2::new(0.0, 5.0));
                painter.line_segment([a - Vec2::new(3.0, 0.0), a], quiet);
                painter.line_segment([a, b], quiet);
                painter.line_segment([b - Vec2::new(3.0, 0.0), b], quiet);
            }
        }
        let w = galley.size().x;
        let at = egui::pos2(left + mark_w, cy - galley.size().y / 2.0);
        painter.galley(at, galley, pal.text_secondary);
        left += mark_w + w + KEY_GAP;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The key keeps every part whole, wrapping one that won't fit.
    #[test]
    fn the_key_wraps_whole_parts() {
        let widths = [100.0, 50.0, 80.0, 40.0];
        assert_eq!(key_rows(&widths, 1000.0), [0, 0, 0, 0]);
        assert_eq!(key_rows(&widths, 100.0 + KEY_GAP + 50.0), [0, 0, 1, 1]);
        assert_eq!(key_rows(&widths, 90.0), [0, 1, 2, 3]);
    }

    /// The axis steps come from the app's one ladder, in whole seconds.
    #[test]
    fn the_time_axis_steps_through_the_apps_ladder() {
        assert_eq!(time_step(180.0), 30.0);
        assert_eq!(time_step(400.0), 120.0);
        assert!(theme::TIME_STEPS.contains(&time_step(5000.0)));
    }
}
