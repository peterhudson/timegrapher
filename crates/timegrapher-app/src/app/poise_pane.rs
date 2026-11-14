//! The Dynamic Poise pane (#1474, Hugh's spec of 12 Nov): the guided test
//! through the eight upright stops, its table, and the rate by the hour
//! at the top drawn as a plot or round a dial, with core's words on the
//! wave and the heavy spot over it. Every figure and word is core's
//! ([`timegrapher_core::poise`]); this lays them out.

use super::*;
use crate::fact_box::{FactBox, FACT_INSET};
use timegrapher_core::chart_scale;
use timegrapher_core::poise::{self, PoiseFit};

/// How the pane draws the rate by the hour at the top: along a line, or
/// round a watch face. Kept between runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum PoiseView {
    #[default]
    Plot,
    Dial,
}

/// The picture's height, points, and the width below which it goes over
/// the table rather than beside it (Hugh, #1499).
const PICTURE_H: f32 = 300.0;
const PICTURE_MIN_W: f32 = 320.0;
/// The plot's height to its width at most, as in Hugh's spec.
const PLOT_ASPECT: f32 = 0.5;
/// The pane width the Info panel needs to sit beside the rest.
const INFO_BESIDE_W: f32 = 1100.0;
/// A point's radius, and its ± bar's width.
const DOT_R: f32 = 4.2;
const BAR_W: f32 = 1.5;
/// The picture's margins: left for the rate's numbers, bottom for the
/// hours and their name, top for the fastest hour's label.
const LEFT: f32 = 46.0;
const RIGHT: f32 = 14.0;
const TOP: f32 = 30.0;
const BOTTOM: f32 = 36.0;
/// The drawn wave's step, points along the plot's width.
const CURVE_PX: f32 = 4.0;
/// The steps the drawn wave is cut into round the dial.
const DIAL_STEPS: f32 = 120.0;
/// How far a line leans, as a share of its length across, before a
/// label at its end is set off that way rather than centred.
const LEAN: f32 = 0.3;
/// The key's line length, points, and the room round a backed label.
const KEY_LINE: f32 = 14.0;
const BACKING: f32 = 2.0;
/// The plot's dotted Mean line: its dashes and gaps, points.
const DASH: f32 = 3.0;
/// A small label's height, points: a ring's rate this near the mean's
/// name is left out.
const LABEL_H: f32 = 14.0;
/// How far out past the outer ring the hours' names sit, points.
const HOUR_OUT: f32 = 14.0;

/// The upright stops round the face, 45° apart, and the step between.
fn hours() -> (usize, f64) {
    let n = core_session::Position::UPRIGHT.len();
    (n, 360.0 / n as f64)
}

/// The bearing halfway from the stop at `deg` to the next, degrees from
/// 12H: where the dial's ring numbers (by 12H) and the mean ring's name
/// (by 6H) sit, so no point sits on them (Hugh, #1499).
fn between(deg: f64) -> f64 {
    deg + hours().1 / 2.0
}

/// What the picture draws: each position measured (angle, rate, ±) and
/// core's fit once there is one.
struct Picture<'a> {
    points: Vec<(f64, f64, Option<f64>)>,
    fit: Option<&'a PoiseFit>,
    /// The table's Mean and its name, drawn with the wave.
    mean: Option<(f64, String)>,
    /// The facts box on the picture once there is a fit: the heavy spot
    /// (Hugh on #1537).
    facts: Option<FactBox>,
}

impl Picture<'_> {
    /// The rates the picture must hold: each point with its ±, and the
    /// wave.
    fn range(&self) -> Option<(f64, f64)> {
        let mut v: Vec<f64> = Vec::new();
        for &(_, r, se) in &self.points {
            let e = se.unwrap_or(0.0);
            v.extend([r - e, r + e]);
        }
        if let Some(f) = self.fit {
            v.extend(f.curve(DIAL_STEPS as usize).into_iter().map(|(_, r)| r));
        }
        chart_scale::data_range(&v)
    }
}

impl TimegrapherApp {
    /// The Dynamic Poise test, if that is the guided test on the Bench.
    pub(super) fn poise_test(&self) -> Option<&guided::Test> {
        self.guided
            .as_ref()
            .filter(|t| t.kind == guided::Kind::Poise)
    }

    /// The pane's toolbar: View first (Hugh's spec), New Test or Cancel
    /// Test while none runs, the Settle and Each Position the Positions
    /// Test shares, and Copy.
    pub(super) fn poise_settings(&mut self, ui: &mut egui::Ui) {
        theme::keep_next(ui);
        theme::tool_field(
            ui,
            "View",
            "The rate by the hour at the top, along a line or round a watch face: the same \
             points, wave and words",
            |ui| {
                theme::segmented(
                    ui,
                    &mut self.poise_view,
                    &[
                        (
                            PoiseView::Plot,
                            "Plot",
                            "Rate along a line, the hour at the top across it",
                        ),
                        (
                            PoiseView::Dial,
                            "Dial",
                            "Rate round a watch face, 12H at the top, further out when faster",
                        ),
                    ],
                );
            },
        );
        theme::bar_gap(ui);
        let running = self.guided_running();
        if !running {
            if self.ready_test() == Some(NewTake::Poise) {
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
                "Set a Dynamic Poise Test up on a clear Bench, then press Start Test on the \
                 Listen card. It begins at 9H at the top and turns the watch 45° at a time \
                 through the eight upright positions.",
                true,
                None,
            ) {
                self.new_take(NewTake::Poise, false);
            }
            theme::bar_gap(ui);
        }
        self.positions_lengths(ui, running, "Each Position");
        theme::bar_gap(ui);
        let nothing = self
            .poise_test()
            .is_none_or(|t| t.readings_so_far().is_empty())
            .then_some("Nothing measured yet");
        if theme::tool_word_button(
            ui,
            Icon::Copy,
            "Copy",
            "Copy the watch, its calibre, the table with tabs between the columns, and the \
             fit in words, to paste into a spreadsheet or a message",
            false,
            nothing,
        ) {
            let about = self.guided_about();
            if let Some(t) = self.poise_test() {
                ui.ctx().copy_text(guided::export_text(t, &about, '\t'));
                self.info_msg("Copied the Dynamic Poise table".to_string());
            }
        }
    }

    /// The pane's toolbar, with the Info toggle pinned at its right.
    pub(super) fn poise_tools(&mut self, ui: &mut egui::Ui, title: &str) {
        let mut info = self.poise_info;
        let hint = "Info: the fit in full, the twice-round check, and why near 220° the heavy \
                    spot is not stated";
        theme::pane_toolbar_info(ui, title, &help::DYNAMIC_POISE, (&mut info, hint), |ui| {
            self.poise_settings(ui);
            self.export_button(ui, Pane::DynamicPoise);
        });
        self.poise_info = info;
    }

    /// The pane: the banner while the test runs, then the table and the
    /// picture with core's words over it, and the Info panel.
    pub(super) fn poise_pane(&mut self, ui: &mut egui::Ui) {
        // Where Keep This Test? keeps clear of, as the Positions Test's.
        if self.poise_test().is_some() {
            self.results_rects[0] = Some(ui.max_rect());
        }
        let now = self.live.as_ref().map(|l| l.duration_s());
        let phase = match (self.guided.as_mut(), now) {
            (Some(t), Some(now)) if t.kind == guided::Kind::Poise => Some(t.tick(now)),
            _ => None,
        };
        let live = match phase {
            Some(guided::Phase::Measuring(_)) => self.guided_live(ui.ctx()),
            _ => None,
        };
        // Before the first stop is in, the amplitude so far, so a watch
        // near 220° can be let down before the run (Hugh's spec).
        let amp_now = self
            .poise_test()
            .filter(|t| t.step == 0 && t.readings_so_far().is_empty())
            .and_then(|_| self.card_reading())
            .filter(|r| readouts::heard(Some(r)))
            .and_then(|r| {
                Some(poise::amplitude_now_text(
                    r.amplitude_deg?,
                    r.amplitude_error_deg,
                ))
            });
        // No test on the Bench: the take's own upright stops, read again
        // as the Positions pane reads its six, so a saved test's results
        // come back when its take is opened (#1619).
        let kept = if self.guided.is_none() {
            self.free_table_shown(ui.ctx()).map(|t| t.upright.0)
        } else {
            None
        };
        let info = self.poise_info;
        let view = self.poise_view;
        let settle = (self.guided_settle_s, self.guided_measure_s);
        let action = poise_body(
            ui,
            (self.poise_test(), phase, live.as_ref()),
            (
                kept.as_deref().filter(|k| !k.is_empty()),
                self.guided.is_none() && !self.running(),
            ),
            amp_now.as_deref(),
            (view, info),
            settle,
        );
        if let (Some(action), Some(now)) = (action, now) {
            self.apply_guided_action(action, now);
        }
    }
}

/// The pane's body under its toolbar; what a banner button asked for.
fn poise_body(
    ui: &mut egui::Ui,
    (test, phase, live): (
        Option<&guided::Test>,
        Option<guided::Phase>,
        Option<&guided::FreeCell>,
    ),
    (kept, done): (Option<&[timegrapher_core::session::Reading]>, bool),
    amp_now: Option<&str>,
    (view, info): (PoiseView, bool),
    (settle_s, measure_s): (f64, f64),
) -> Option<guided::Action> {
    let readings = match test {
        Some(t) => t.readings_so_far(),
        None => kept.unwrap_or_default(),
    };
    let fit = guided::poise_fit(readings);
    let mean = guided::poise_mean(readings);
    let bottom = ui.available_rect_before_wrap().bottom();
    let layout = test_body::Layout {
        table_ws: &[guided::TABLE_W, guided::TIGHT_TABLE_W],
        result_min_w: PICTURE_MIN_W,
        info_beside_w: INFO_BESIDE_W,
        rule_h: PICTURE_H,
    };
    // A take that is done (no test on the Bench and no sound coming in,
    // Trevor on #1622): no more stops will come.
    let measured = readings
        .iter()
        .filter(|r| poise::point(r.position, &r.measurement).is_some())
        .count();
    let unfitted = poise::unfitted_text(measured, done);
    let picture = |ui: &mut egui::Ui| {
        picture_block(
            ui,
            (readings, fit.as_ref(), &mean),
            (view, &unfitted),
            bottom,
        );
    };
    let notes = guided::poise_table_notes(readings);
    let info_part = |ui: &mut egui::Ui| info_panel(ui, (fit.as_ref(), &unfitted), &notes);
    let kept_table = |ui: &mut egui::Ui| guided::poise_table(ui, readings);
    test_body::test_body(
        ui,
        ("poise pane", &layout),
        (test, phase, live, amp_now),
        info,
        &test_body::Parts {
            about: about_text(settle_s, measure_s),
            kept: kept
                .is_some()
                .then_some(&kept_table as &dyn Fn(&mut egui::Ui)),
            result: &picture,
            info: &info_part,
        },
    )
}

/// The picture with core's words over it, in the room left above `bottom`.
fn picture_block(
    ui: &mut egui::Ui,
    (readings, fit, mean): (
        &[timegrapher_core::session::Reading],
        Option<&PoiseFit>,
        &Option<(f64, String)>,
    ),
    (view, unfitted): (PoiseView, &str),
    bottom: f32,
) {
    words(ui, fit, unfitted);
    let points = readings
        .iter()
        .filter_map(|r| poise::point(r.position, &r.measurement))
        .map(|p| (p.angle_deg, p.rate_s_per_day, p.rate_se))
        .collect();
    let facts = fit.map(|f| {
        let name = match view {
            PoiseView::Plot => "plot",
            PoiseView::Dial => "dial",
        };
        FactBox::plain(
            ui,
            "poise facts",
            name,
            vec![f.heavy_spot_row()],
            Vec::new(),
        )
    });
    let pic = Picture {
        points,
        fit,
        mean: mean.clone(),
        facts,
    };
    let w = ui.available_width().max(PICTURE_MIN_W);
    // The room left in the pane, never under PICTURE_H; the dial never
    // taller than wide, the plot half as tall.
    let tallest = match view {
        PoiseView::Plot => w * PLOT_ASPECT,
        PoiseView::Dial => w,
    };
    let h = (bottom - ui.cursor().top()).min(tallest).max(PICTURE_H);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), egui::Sense::hover());
    match view {
        PoiseView::Plot => draw_plot(ui, rect, &pic),
        PoiseView::Dial => draw_dial(ui, rect, &pic),
    }
}

/// What the pane says before a test: how the test runs.
fn about_text(settle_s: f64, measure_s: f64) -> String {
    format!(
        "Start Test, on the Listen card, leads you through the eight upright positions, \
         45° apart, from 9H at the top, turning the watch the same way round each time:\n\
         1. It says which hour to turn to the top.\n\
         2. It lets the watch settle {settle_s:.0} s.\n\
         3. It measures it for {measure_s:.0} s and fills in its row.\n\
         The wave is fitted once {} positions are in. After the eighth the test ends by \
         itself and keeps the take.",
        poise::MIN_SHOWN
    )
}

/// The title line over the picture (core's short sentence, measured
/// facts) and, where the amplitude is near 220°, a grey chip under it;
/// or core's words for a fit still to come, or one a take lacks. The heavy spot is in the
/// picture's facts box, and why it isn't stated and any caveat in Info
/// (Hugh on #1537). The room is kept from the start, so nothing moves
/// when they come.
fn words(ui: &mut egui::Ui, fit: Option<&PoiseFit>, unfitted: &str) {
    let pal = theme::pal(ui);
    let line_h = ui.text_style_height(&egui::TextStyle::Body);
    ui.allocate_ui(Vec2::new(ui.available_width(), 2.0 * line_h + 8.0), |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        let Some(f) = fit else {
            ui.label(RichText::new(unfitted).color(pal.text_secondary));
            return;
        };
        ui.add(egui::Label::new(RichText::new(&f.short_text).color(pal.text)).wrap());
        let band = matches!(f.withheld, Some(poise::Withheld::NullBand { .. }));
        if band {
            // The chip holds the band's fact.
            egui::Frame::new()
                .fill(pal.control)
                .corner_radius(egui::CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(10, 2))
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(f.heavy_spot_line()).small().color(pal.text),
                        )
                        .wrap(),
                    )
                    .on_hover_text(poise::null_band_note());
                });
        }
    });
}

/// The Info panel: core's full sentence (the wave, its twice-round check,
/// the amplitude, the heavy spot or why it isn't stated, and its
/// caveats), the note on the band near 220°, which alone says why 219.5°
/// matters, then the table's `notes` (Hugh on #1537).
fn info_panel(ui: &mut egui::Ui, (fit, unfitted): (Option<&PoiseFit>, &str), notes: &[String]) {
    let pal = theme::pal(ui);
    ui.spacing_mut().item_spacing.y = 4.0;
    let text = |t: &str, c: egui::Color32| {
        egui::Label::new(RichText::new(t).small().color(c))
            .wrap()
            .selectable(true)
    };
    ui.label(theme::caption("Dynamic Poise").color(pal.text_secondary));
    match fit {
        Some(f) => ui.add(text(&f.sentence_beside_note(), pal.text)),
        None => ui.add(text(unfitted, pal.text_secondary)),
    };
    ui.add_space(6.0);
    ui.add(text(&poise::null_band_note(), pal.text_secondary));
    for n in notes {
        ui.add_space(6.0);
        ui.add(text(n, pal.text_secondary));
    }
}

/// The rate scale for the picture: round ends round every rate it holds.
fn rate_scale(pic: &Picture<'_>) -> (f64, f64, f64) {
    let (lo, hi) = pic.range().unwrap_or((-1.0, 1.0));
    chart_scale::round_ends(lo, hi)
}

/// The plot: x the hour at the top, 12H to 12H, a mark every 45°; y the
/// rate. The points with their ±, then the wave and its fastest hour in
/// grey once core has fitted it.
fn draw_plot(ui: &egui::Ui, rect: egui::Rect, pic: &Picture<'_>) {
    let pal = theme::pal(ui);
    let painter = ui.painter_at(rect);
    let area = egui::Rect::from_min_max(
        rect.min + Vec2::new(LEFT, TOP),
        rect.max - Vec2::new(RIGHT, BOTTOM),
    );
    let (lo, hi, step) = rate_scale(pic);
    let x = |deg: f64| area.left() + (deg / 360.0) as f32 * area.width();
    let y = |r: f64| area.bottom() - ((r - lo) / (hi - lo)) as f32 * area.height();
    let small = egui::FontId::proportional(11.0);
    let grid = egui::Stroke::new(1.0_f32, pal.separator);
    let n = ((hi - lo) / step).round() as i64;
    for k in 0..=n {
        let v = lo + k as f64 * step;
        painter.hline(area.x_range(), y(v), grid);
        // No rate's numbers until there is a rate.
        if pic.points.is_empty() {
            continue;
        }
        painter.text(
            egui::pos2(area.left() - 6.0, y(v)),
            egui::Align2::RIGHT_CENTER,
            chart_scale::axis_number(v, step),
            small.clone(),
            pal.text_secondary,
        );
    }
    painter.text(
        egui::pos2(area.left() - 6.0, area.top() - 10.0),
        egui::Align2::RIGHT_BOTTOM,
        "s/d",
        small.clone(),
        pal.text_secondary,
    );
    let (n_hours, hour_step) = hours();
    for i in 0..=n_hours {
        let deg = i as f64 * hour_step;
        let half = i % 2 == 1;
        let ink = if half {
            pal.text_tertiary
        } else {
            pal.text_secondary
        };
        let top = egui::pos2(x(deg), area.top());
        let bottom = egui::pos2(x(deg), area.bottom());
        if half {
            painter.extend(egui::Shape::dashed_line(&[top, bottom], grid, 2.0, 3.0));
        } else {
            painter.line_segment([top, bottom], grid);
        }
        painter.text(
            egui::pos2(x(deg), area.bottom() + 4.0),
            egui::Align2::CENTER_TOP,
            poise::hour_code(deg),
            small.clone(),
            ink,
        );
    }
    painter.text(
        egui::pos2(area.center().x, rect.bottom() - 2.0),
        egui::Align2::CENTER_BOTTOM,
        "Hour at the top",
        small.clone(),
        pal.text_secondary,
    );
    let mut wave = Vec::new();
    if let Some(f) = pic.fit {
        // The table's Mean, dotted across the plot under the wave, on a
        // whole pixel so its dashes show (Hugh, Walter, #1499).
        let mean = pic
            .mean
            .as_ref()
            .map(|(m, name)| (y(*m).round() + 0.5, name));
        if let Some((my, _)) = mean {
            let line = [egui::pos2(area.left(), my), egui::pos2(area.right(), my)];
            let dotted = egui::Stroke::new(1.0_f32, pal.text_secondary);
            painter.extend(egui::Shape::dashed_line(&line, dotted, DASH, DASH));
        }
        wave = plot_wave(&painter, area, (&x, &y), f, pal);
        if let Some((my, name)) = mean {
            // Above the line at the slowest hour, where the wave is below
            // it (Hugh, #1499).
            backed(
                &painter,
                egui::pos2(x(f.slowest().0), my - 3.0),
                egui::Align2::CENTER_BOTTOM,
                name,
                pal,
            );
        }
    }
    let key = plot_key(&painter, area, pic, pal);
    if let Some(facts) = &pic.facts {
        plot_facts(ui, &painter, (area, key), facts, (wave, (&x, &y), pic));
    }
    plot_points(&painter, (&x, &y), pic, pal);
}

/// The plot's x of an angle and y of a rate.
type Axes<'a> = (&'a dyn Fn(f64) -> f32, &'a dyn Fn(f64) -> f32);

/// The facts box on the plot (Hugh on #1537): in its top left, or in its
/// top right under the key where the wave or a point runs under the left
/// (guide section 11), clear of both; none where both would cover them.
fn plot_facts(
    ui: &egui::Ui,
    painter: &egui::Painter,
    (area, key): (egui::Rect, Option<egui::Rect>),
    facts: &FactBox,
    (mut trace, (x, y), pic): (Vec<egui::Pos2>, Axes<'_>, &Picture<'_>),
) {
    for &(deg, r, se) in &pic.points {
        let e = se.unwrap_or(0.0);
        for d in [deg, deg + 360.0 * f64::from(deg.rem_euclid(360.0) == 0.0)] {
            trace.extend([r - e, r, r + e].map(|v| egui::pos2(x(d), y(v))));
        }
    }
    let inset = Vec2::splat(FACT_INSET);
    let right_top = egui::pos2(
        area.right() - FACT_INSET - facts.size().x,
        key.map_or(area.top(), |k| k.bottom()) + FACT_INSET,
    );
    facts.paint(ui, painter, &[area.left_top() + inset, right_top], &trace);
}

/// The measured points on the plot; 12H is at both ends, so its point is
/// drawn at each.
fn plot_points(
    painter: &egui::Painter,
    (x, y): (&dyn Fn(f64) -> f32, &dyn Fn(f64) -> f32),
    pic: &Picture<'_>,
    pal: &theme::Palette,
) {
    for &(deg, r, se) in &pic.points {
        let ends: &[f64] = if deg.rem_euclid(360.0) == 0.0 {
            &[0.0, 360.0]
        } else {
            &[deg]
        };
        for &d in ends {
            point(
                painter,
                egui::pos2(x(d), y(r)),
                se.map(|e| y(r - e) - y(r)),
                pal,
            );
        }
    }
}

/// The plot's key, inside its top right: the points, and the wave once
/// it is fitted; where it is.
fn plot_key(
    painter: &egui::Painter,
    area: egui::Rect,
    pic: &Picture<'_>,
    pal: &theme::Palette,
) -> Option<egui::Rect> {
    if pic.points.is_empty() {
        return None;
    }
    let small = egui::FontId::proportional(11.0);
    let row = 16.0;
    let mut rows = vec![format!("Upright positions ({})", pic.points.len())];
    if pic.fit.is_some() {
        rows.push("Fitted wave".to_string());
    }
    let widest = rows
        .iter()
        .map(|t| {
            painter
                .layout_no_wrap(t.clone(), small.clone(), pal.text)
                .size()
                .x
        })
        .fold(0.0_f32, f32::max);
    let w = KEY_LINE + 6.0 + widest;
    let top_left = egui::pos2(area.right() - w - 8.0, area.top() + 6.0);
    let back =
        egui::Rect::from_min_size(top_left, Vec2::new(w, row * rows.len() as f32)).expand(4.0);
    painter.rect_filled(back, 4.0, pal.card);
    for (i, t) in rows.iter().enumerate() {
        let mid = top_left.y + row * (i as f32 + 0.5);
        let at = |dx: f32| egui::pos2(top_left.x + dx, mid);
        if i == 0 {
            painter.circle_filled(at(KEY_LINE / 2.0), DOT_R, pal.trace_rate);
        } else {
            painter.line_segment(
                [at(0.0), at(KEY_LINE)],
                egui::Stroke::new(1.5_f32, pal.text_secondary),
            );
        }
        painter.text(
            at(KEY_LINE + 6.0),
            egui::Align2::LEFT_CENTER,
            t,
            small.clone(),
            pal.text,
        );
    }
    Some(back)
}

/// Small text on the pane's background, so no line crosses it.
fn backed(
    painter: &egui::Painter,
    pos: egui::Pos2,
    align: egui::Align2,
    text: &str,
    pal: &theme::Palette,
) {
    let galley = painter.layout_no_wrap(
        text.to_string(),
        egui::FontId::proportional(11.0),
        pal.text_secondary,
    );
    let rect = align.anchor_size(pos, galley.size());
    painter.rect_filled(rect.expand(BACKING), BACKING, pal.card);
    painter.galley(rect.min, galley, pal.text_secondary);
}

/// Core's fit on the plot: the wave in grey, and its fastest hour as an
/// upright line from the top, named, once the wave is clear of the noise;
/// the wave's points as drawn.
fn plot_wave(
    painter: &egui::Painter,
    area: egui::Rect,
    (x, y): (&dyn Fn(f64) -> f32, &dyn Fn(f64) -> f32),
    f: &PoiseFit,
    pal: &theme::Palette,
) -> Vec<egui::Pos2> {
    let small = egui::FontId::proportional(11.0);
    let mut wave: Vec<egui::Pos2> = f
        .curve((area.width() / CURVE_PX) as usize)
        .into_iter()
        .map(|(d, r)| egui::pos2(x(d), y(r)))
        .collect();
    let grey = egui::Stroke::new(1.5_f32, pal.text_secondary);
    painter.add(egui::Shape::line(wave.clone(), grey));
    if f.wave_clear {
        let (peak, top) = f.fastest();
        painter.line_segment(
            [egui::pos2(x(peak), area.top()), egui::pos2(x(peak), y(top))],
            egui::Stroke::new(1.2_f32, pal.text_secondary),
        );
        painter.text(
            egui::pos2(x(peak), area.top() - 2.0),
            egui::Align2::CENTER_BOTTOM,
            format!("Fastest {}", poise::hour_code(peak)),
            small.clone(),
            pal.text,
        );
        // The fastest hour's line is the wave's too, for what keeps
        // clear of it.
        let (from, to) = (area.top(), y(top));
        let steps = ((to - from) / CURVE_PX).max(1.0) as usize;
        wave.extend(
            (0..=steps).map(|i| egui::pos2(x(peak), from + (to - from) * i as f32 / steps as f32)),
        );
    }
    wave
}

/// Where the dial puts a rate at an hour: the rate scale's ends at the
/// `inner` and `outer` rings round `centre`.
struct Dial {
    centre: egui::Pos2,
    inner: f32,
    outer: f32,
    scale: (f64, f64, f64),
}

impl Dial {
    fn radius(&self, r: f64) -> f32 {
        let (lo, hi, _) = self.scale;
        self.inner + ((r - lo) / (hi - lo)) as f32 * (self.outer - self.inner)
    }

    /// The point `rad` out from the centre at `deg` round from 12H.
    fn at(&self, deg: f64, rad: f32) -> egui::Pos2 {
        let a = deg.to_radians() as f32;
        self.centre + Vec2::new(a.sin(), -a.cos()) * rad
    }
}

/// The dial: 12H at the top, each point at its hour, further out when
/// faster; the mean as a dotted ring. The zero ring is not drawn, so the
/// sign reads from the rings' numbers.
fn draw_dial(ui: &egui::Ui, rect: egui::Rect, pic: &Picture<'_>) {
    let pal = theme::pal(ui);
    let painter = ui.painter_at(rect);
    let small = egui::FontId::proportional(11.0);
    let (whole, (rect, label_at)) = (rect, dial_room(&painter, rect, pic));
    let label_room = 30.0;
    let outer = (rect.height() / 2.0 - label_room).min(rect.width() / 2.0 - label_room - 20.0);
    let dial = Dial {
        centre: rect.center(),
        inner: outer * 0.15,
        outer,
        scale: rate_scale(pic),
    };
    // The slowest point's bearing once there is a wave; till then, between
    // two stops.
    let numbers = (!pic.points.is_empty()).then(|| {
        let bearing = pic.fit.map_or(between(0.0), |f| f.slowest().0);
        let mean_r = pic.fit.and(pic.mean.as_ref()).map(|m| dial.radius(m.0));
        (bearing, mean_r)
    });
    let mean_label = pic
        .fit
        .zip(pic.mean.as_ref())
        .map(|(f, (m, name))| mean_label_at(&painter, &dial, (f.slowest().0, *m, name), pic, pal));
    let keep_off = ring_numbers_keep_off(&dial, &pic.points, mean_label.as_ref());
    dial_grid(&painter, &dial, pal, numbers, &keep_off);
    painter.text(
        label_at,
        egui::Align2::RIGHT_TOP,
        DIAL_KEY,
        small,
        pal.text_secondary,
    );
    if let Some(facts) = &pic.facts {
        facts.paint(
            ui,
            &painter,
            &[whole.left_top() + Vec2::splat(FACT_INSET)],
            &[],
        );
    }
    if let Some(f) = pic.fit {
        dial_wave(&painter, &dial, (f, pic.mean.as_ref().map(|m| m.0)), pal);
    }
    for &(deg, r, se) in &pic.points {
        let p = dial.at(deg, dial.radius(r));
        // The ± along the spoke, out and in.
        if let Some(e) = se {
            let (a, b) = (
                dial.at(deg, dial.radius(r + e)),
                dial.at(deg, dial.radius(r - e)),
            );
            painter.line_segment([a, b], egui::Stroke::new(BAR_W, pal.trace_rate));
        }
        painter.circle(
            p,
            DOT_R,
            pal.trace_rate,
            egui::Stroke::new(1.5_f32, pal.card),
        );
    }
    if let Some((at, name)) = mean_label {
        backed(
            &painter,
            at.center(),
            egui::Align2::CENTER_CENTER,
            name,
            pal,
        );
    }
}

/// What the dial's top right says.
const DIAL_KEY: &str = "Further out = faster, s/d";

/// The room the dial takes in `rect`, under the facts box once there is
/// one, which takes the top left corner (Hugh on #1537), and where the
/// key's top right goes: on the box's row where both fit beside each
/// other, else under it.
fn dial_room(
    painter: &egui::Painter,
    rect: egui::Rect,
    pic: &Picture<'_>,
) -> (egui::Rect, egui::Pos2) {
    let corner = rect.right_top() + Vec2::new(-4.0, 4.0);
    let Some(size) = pic.facts.as_ref().map(FactBox::size) else {
        return (rect, corner);
    };
    let key = painter
        .layout_no_wrap(
            DIAL_KEY.into(),
            egui::FontId::proportional(11.0),
            egui::Color32::WHITE,
        )
        .size();
    let head = size.y + 2.0 * FACT_INSET;
    let beside = size.x + 2.0 * FACT_INSET + 16.0 + key.x + 4.0 <= rect.width();
    let (key_at, top) = if beside {
        (corner, head)
    } else {
        (corner + Vec2::new(0.0, head), head + key.y + 4.0)
    };
    (
        egui::Rect::from_min_max(rect.min + Vec2::new(0.0, top), rect.max),
        key_at,
    )
}

/// What a ring number must keep off besides the hours' names: each
/// point's dot and the mean's name with its backing (Hugh, after Walter's
/// walk of #1546).
fn ring_numbers_keep_off(
    dial: &Dial,
    points: &[(f64, f64, Option<f64>)],
    mean_label: Option<&(egui::Rect, &str)>,
) -> Vec<egui::Rect> {
    let mut boxes: Vec<egui::Rect> = points
        .iter()
        .map(|&(d, r, _)| {
            egui::Rect::from_center_size(dial.at(d, dial.radius(r)), Vec2::splat(2.0 * DOT_R))
        })
        .collect();
    boxes.extend(mean_label.map(|(at, _)| at.expand(BACKING)));
    boxes
}

/// Where the mean ring's name goes, and the name: on the slowest point's
/// bearing (Hugh, #1499); half a stop round, one way or the other, when a
/// point would sit under any part of it.
fn mean_label_at<'n>(
    painter: &egui::Painter,
    dial: &Dial,
    (bearing, m, name): (f64, f64, &'n str),
    pic: &Picture<'_>,
    pal: &theme::Palette,
) -> (egui::Rect, &'n str) {
    let size = painter
        .layout_no_wrap(name.to_string(), egui::FontId::proportional(11.0), pal.text)
        .size();
    let deg = mean_name_bearing(dial, (bearing, m, size), &pic.points);
    let at = egui::Align2::CENTER_CENTER.anchor_size(dial.at(deg, dial.radius(m)), size);
    (at, name)
}

/// Where the mean ring's name of `size` goes: the first of the slowest
/// `bearing` and half a stop either side of it whose whole box, backing
/// and a dot's radius round it, holds no point (Walter, #1499).
fn mean_name_bearing(
    dial: &Dial,
    (bearing, m, size): (f64, f64, Vec2),
    points: &[(f64, f64, Option<f64>)],
) -> f64 {
    let half = hours().1 / 2.0;
    let clear = |deg: f64| {
        let label = egui::Align2::CENTER_CENTER
            .anchor_size(dial.at(deg, dial.radius(m)), size)
            .expand(BACKING + DOT_R);
        !points
            .iter()
            .any(|&(d, r, _)| label.contains(dial.at(d, dial.radius(r))))
    };
    [bearing, bearing + half, bearing - half]
        .into_iter()
        .find(|&d| clear(d))
        .unwrap_or(bearing + half)
}

/// Core's fit round the dial: the mean as a dotted ring, the wave in
/// grey, and its fastest hour as a line from the centre, named, once the
/// wave is clear of the noise.
fn dial_wave(
    painter: &egui::Painter,
    dial: &Dial,
    (f, mean): (&PoiseFit, Option<f64>),
    pal: &theme::Palette,
) {
    let steps = DIAL_STEPS as usize;
    let ring_r = dial.radius(mean.unwrap_or(f.wave.mean.value));
    let ring: Vec<egui::Pos2> = (0..=steps)
        .map(|i| dial.at(360.0 * i as f64 / f64::from(DIAL_STEPS), ring_r))
        .collect();
    let dotted = egui::Stroke::new(1.0_f32, pal.text_secondary);
    painter.extend(egui::Shape::dashed_line(&ring, dotted, 1.0, 3.0));
    let wave: Vec<egui::Pos2> = f
        .curve(steps)
        .into_iter()
        .map(|(d, r)| dial.at(d, dial.radius(r)))
        .collect();
    painter.add(egui::Shape::line(
        wave,
        egui::Stroke::new(1.5_f32, pal.text_secondary),
    ));
    if !f.wave_clear {
        return;
    }
    let (peak, top) = f.fastest();
    let tip = dial.at(peak, dial.radius(top));
    painter.line_segment(
        [dial.centre, tip],
        egui::Stroke::new(1.2_f32, pal.text_secondary),
    );
    // Named at the line's outer end, just beyond the wave (Hugh, #1499),
    // set off the way the line points.
    let out = (tip - dial.centre).normalized();
    let align = egui::Align2([
        lean(out.x, egui::Align::Min, egui::Align::Max),
        lean(out.y, egui::Align::Min, egui::Align::Max),
    ]);
    let galley = painter.layout_no_wrap(
        format!("Fastest {}", poise::hour_code(peak)),
        egui::FontId::proportional(11.0),
        pal.text,
    );
    let rect = align.anchor_size(tip + out * 8.0, galley.size());
    painter.rect_filled(rect.expand(BACKING), BACKING, pal.card);
    painter.galley(rect.min, galley, pal.text);
}

/// Which way a label leans off a line pointing `d` along one axis: past
/// its end when the line leans that way, centred when it barely does.
fn lean(d: f32, plus: egui::Align, minus: egui::Align) -> egui::Align {
    if d > LEAN {
        plus
    } else if d < -LEAN {
        minus
    } else {
        egui::Align::Center
    }
}

/// A measured point in the rate's colour, with its ± (`half`, points
/// from the dot to the bar's end) as a bar through it.
fn point(painter: &egui::Painter, p: egui::Pos2, half: Option<f32>, pal: &theme::Palette) {
    if let Some(h) = half {
        painter.line_segment(
            [p - Vec2::new(0.0, h), p + Vec2::new(0.0, h)],
            egui::Stroke::new(BAR_W, pal.trace_rate),
        );
    }
    painter.circle(
        p,
        DOT_R,
        pal.trace_rate,
        egui::Stroke::new(1.5_f32, pal.card),
    );
}

/// The dial's rings at the scale's round rates, with their numbers, and
/// a spoke and name for each upright hour.
/// `numbers`: the bearing the rings' rates go on, and the mean ring's
/// radius, whose name goes there too, so a rate that would sit on it is
/// left out.
fn dial_grid(
    painter: &egui::Painter,
    dial: &Dial,
    pal: &theme::Palette,
    numbers: Option<(f64, Option<f32>)>,
    keep_off: &[egui::Rect],
) {
    let small = egui::FontId::proportional(11.0);
    let grid = egui::Stroke::new(1.0_f32, pal.separator);
    let (n_hours, hour_step) = hours();
    let names: Vec<(f64, std::sync::Arc<egui::Galley>)> = (0..n_hours)
        .map(|i| {
            let deg = i as f64 * hour_step;
            let galley =
                painter.layout_no_wrap(poise::hour_code(deg).to_string(), small.clone(), pal.text);
            (deg, galley)
        })
        .collect();
    let mut name_boxes: Vec<egui::Rect> = names
        .iter()
        .map(|(deg, g)| hour_name_box(dial, *deg, g.size()))
        .collect();
    name_boxes.extend_from_slice(keep_off);
    let (lo, hi, step) = dial.scale;
    let n = ((hi - lo) / step).round() as i64;
    for k in 0..=n {
        let v = lo + k as f64 * step;
        let r = dial.radius(v);
        painter.circle_stroke(dial.centre, r, grid);
        let Some((bearing, mean_r)) = numbers else {
            continue;
        };
        if mean_r.is_some_and(|m| (m - r).abs() < LABEL_H) {
            continue;
        }
        // Each ring's rate on the slowest point's bearing, inside the
        // wave, on the pane's background (Hugh, #1499); left out where it
        // would touch an hour's name (#1530), a point or the mean's name.
        let text = chart_scale::axis_number(v, step);
        let size = painter
            .layout_no_wrap(text.clone(), small.clone(), pal.text)
            .size();
        if !clear_of(dial.at(bearing, r), size, &name_boxes) {
            continue;
        }
        backed(
            painter,
            dial.at(bearing, r),
            egui::Align2::CENTER_CENTER,
            &text,
            pal,
        );
    }
    for (i, (deg, galley)) in names.into_iter().enumerate() {
        let half = i % 2 == 1;
        let ink = if half {
            pal.text_tertiary
        } else {
            pal.text_secondary
        };
        let spoke = [dial.centre, dial.at(deg, dial.outer)];
        let name_at = hour_name_box(dial, deg, galley.size()).min;
        if half {
            painter.extend(egui::Shape::dashed_line(&spoke, grid, 2.0, 3.0));
        } else {
            painter.line_segment(spoke, grid);
        }
        painter.galley(name_at, galley, ink);
    }
}

/// Where an hour's name of `size` sits round the dial at `deg`.
fn hour_name_box(dial: &Dial, deg: f64, size: Vec2) -> egui::Rect {
    egui::Align2::CENTER_CENTER.anchor_size(dial.at(deg, dial.outer + HOUR_OUT), size)
}

/// Whether a label of `size` centred `at`, with its backing, keeps off
/// every box in `others`.
fn clear_of(at: egui::Pos2, size: Vec2, others: &[egui::Rect]) -> bool {
    let label = egui::Align2::CENTER_CENTER
        .anchor_size(at, size)
        .expand(BACKING);
    !others.iter().any(|o| o.intersects(label))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ring number that would touch an hour's name is left out (#1530):
    /// the outer ring's on the 12H bearing, just under "12H"; one ring in
    /// is clear.
    #[test]
    fn a_ring_number_keeps_off_the_hours_names() {
        let dial = Dial {
            centre: egui::pos2(300.0, 300.0),
            inner: 30.0,
            outer: 200.0,
            scale: (0.0, 20.0, 10.0),
        };
        let names = [hour_name_box(&dial, 0.0, Vec2::new(22.0, 13.0))];
        let number = Vec2::new(14.0, 13.0);
        assert!(!clear_of(dial.at(0.0, dial.outer), number, &names));
        assert!(clear_of(dial.at(0.0, dial.radius(10.0)), number, &names));
    }

    /// The painted facts box takes the first place clear of the trace,
    /// and none where every place would cover it (#1537).
    #[test]
    fn a_painted_facts_box_keeps_clear_of_the_trace() {
        let ctx = egui::Context::default();
        let mut placed = Vec::new();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let rows = vec![("Heavy spot".to_string(), "6H at the top, 180°".to_string())];
                let facts = FactBox::plain(ui, "test facts", "box", rows, Vec::new());
                let p = ui.painter();
                let places = [egui::pos2(10.0, 10.0), egui::pos2(300.0, 10.0)];
                let size = facts.size();
                let under_left = [egui::pos2(10.0, 10.0) + size / 2.0];
                let under_both = [under_left[0], places[1] + size / 2.0];
                for trace in [&[][..], &under_left[..], &under_both[..]] {
                    placed.push(facts.paint(ui, p, &places, trace).map(|r| r.min));
                }
            });
        });
        assert_eq!(
            placed,
            [
                Some(egui::pos2(10.0, 10.0)),
                Some(egui::pos2(300.0, 10.0)),
                None
            ]
        );
    }

    /// Every text `draw` puts on a 600 pt square.
    fn drawn_texts(draw: impl FnOnce(&egui::Ui)) -> Vec<String> {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::splat(600.0),
            )),
            ..Default::default()
        };
        let mut draw = Some(draw);
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some(d) = draw.take() {
                    d(ui);
                }
            });
        });
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect()
    }

    /// The texts `dial_grid` draws with ring numbers on `bearing`, keeping
    /// off `keep_off` as well as the hours' names.
    fn grid_texts(dial: &Dial, bearing: f64, keep_off: &[egui::Rect]) -> Vec<String> {
        drawn_texts(|ui| {
            let numbers = Some((bearing, None));
            dial_grid(ui.painter(), dial, theme::pal(ui), numbers, keep_off);
        })
    }

    /// The whole dial drawn: a point on the ring numbers' bearing, at a
    /// ring's own rate, takes that ring's number out (Trevor on #1550).
    #[test]
    fn the_drawn_dial_leaves_out_the_number_under_a_point() {
        let mut pic = Picture {
            points: vec![(0.0, -10.0, None), (90.0, 10.0, None), (180.0, 0.0, None)],
            fit: None,
            mean: None,
            facts: None,
        };
        let (lo, hi, step) = rate_scale(&pic);
        let ring = lo + step;
        let number = chart_scale::axis_number(ring, step);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(600.0));
        let before = drawn_texts(|ui| draw_dial(ui, rect, &pic));
        assert!(before.contains(&number), "{number} in {before:?}");
        // Before a wave the numbers sit between the first two stops.
        pic.points.push((between(0.0), ring, None));
        assert_eq!(rate_scale(&pic), (lo, hi, step));
        let after = drawn_texts(|ui| draw_dial(ui, rect, &pic));
        assert!(!after.contains(&number), "{number} in {after:?}");
    }

    /// Drawn, the outer ring's number on the 12H bearing is left out while
    /// the next ring in keeps its number (#1530, Trevor on #1546).
    #[test]
    fn the_drawn_dial_leaves_out_only_the_number_under_an_hours_name() {
        let dial = Dial {
            centre: egui::pos2(300.0, 300.0),
            inner: 30.0,
            outer: 200.0,
            scale: (0.0, 20.0, 10.0),
        };
        let texts = grid_texts(&dial, 0.0, &[]);
        let (outer, next_in) = (
            chart_scale::axis_number(20.0, 10.0),
            chart_scale::axis_number(10.0, 10.0),
        );
        assert!(texts.iter().any(|t| t == "12H"), "{texts:?}");
        assert!(!texts.contains(&outer), "{texts:?}");
        assert!(texts.contains(&next_in), "{texts:?}");
    }

    /// The slowest dot sits on its own ring's number, since the numbers
    /// ride its bearing: that number is left out, and so is one under the
    /// mean's name, while the rings clear of both keep theirs (Hugh, after
    /// Walter's walk of #1546).
    #[test]
    fn a_ring_number_keeps_off_the_points_and_the_means_name() {
        let dial = Dial {
            centre: egui::pos2(300.0, 300.0),
            inner: 30.0,
            outer: 200.0,
            scale: (-20.0, 20.0, 10.0),
        };
        let number = |v: f64| chart_scale::axis_number(v, 10.0);
        // The slowest point at 6H, -10 s/d, and two others off the bearing.
        let points = [
            (180.0, -10.0, Some(0.5)),
            (0.0, 10.0, None),
            (90.0, 0.0, None),
        ];
        let bare = grid_texts(&dial, 180.0, &ring_numbers_keep_off(&dial, &[], None));
        assert!(bare.contains(&number(-10.0)), "{bare:?}");
        assert!(bare.contains(&number(10.0)), "{bare:?}");
        let mean_at = egui::Align2::CENTER_CENTER
            .anchor_size(dial.at(180.0, dial.radius(10.0)), Vec2::new(60.0, 13.0));
        let keep_off = ring_numbers_keep_off(&dial, &points, Some(&(mean_at, "Mean +10.0")));
        let texts = grid_texts(&dial, 180.0, &keep_off);
        assert!(!texts.contains(&number(-10.0)), "{texts:?}");
        assert!(!texts.contains(&number(10.0)), "{texts:?}");
        assert!(texts.contains(&number(0.0)), "{texts:?}");
        assert!(texts.contains(&number(-20.0)), "{texts:?}");
    }

    /// A point under the name's end, not its centre, moves the name off
    /// its bearing: Walter's ETA 2824 run, where "Mean +11.2" on the 6H
    /// spoke covered the 6H point at +10.8.
    #[test]
    fn the_mean_name_keeps_off_a_point_under_any_part_of_it() {
        let dial = Dial {
            centre: egui::pos2(300.0, 300.0),
            inner: 30.0,
            outer: 200.0,
            scale: (0.0, 20.0, 5.0),
        };
        let size = Vec2::new(60.0, 13.0);
        let (bearing, mean) = (180.0, 11.2);
        // Clear of the name: it stays on the bearing.
        let far = [(0.0, 11.2, None)];
        assert_eq!(
            mean_name_bearing(&dial, (bearing, mean, size), &far),
            bearing
        );
        // At 6H, 0.4 s/d in: 3.4 pt from the name's centre along the
        // spoke, under it. And beside the spoke, under the name's end
        // but 20 pt from its centre, past the old test's reach.
        let at = dial.at(bearing, dial.radius(mean)) + Vec2::new(20.0, 0.0);
        let r_end = dial.centre.distance(at);
        let deg_end = f64::from((at - dial.centre).angle().to_degrees() + 90.0);
        let rate_end = (f64::from(r_end - dial.inner) / f64::from(dial.outer - dial.inner)) * 20.0;
        for under in [(180.0, 10.8, None), (deg_end, rate_end, None)] {
            let moved = mean_name_bearing(&dial, (bearing, mean, size), &[under]);
            assert_ne!(moved, bearing, "{under:?}");
            assert_eq!((moved - bearing).abs(), hours().1 / 2.0);
        }
    }
}
