//! Masks and marks: hiding stretches of a take, the brush, and painted positions.

use super::*;

impl TimegrapherApp {
    /// How many stretches are masked and how long they add up to, after a
    /// space, for the Mask tool's hover; nothing when none is.
    pub(super) fn masked_words(&self) -> String {
        if self.masked.is_empty() {
            return String::new();
        }
        let total: f64 = self.masked.iter().map(|m| m.1 - m.0).sum();
        let n = self.masked.len();
        format!(
            " {n} {} masked, {} in all.",
            if n == 1 { "stretch" } else { "stretches" },
            length_text(total)
        )
    }

    /// Edit ▸ Remove All Masks' question: every masked stretch back into
    /// the readings, once the user says so.
    pub(super) fn remove_masks_dialog(&mut self, ctx: &egui::Context) {
        if !self.remove_masks_asked {
            return;
        }
        let mut choice = None;
        egui::Modal::new(egui::Id::new("remove-masks")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(RichText::new("Remove all masks?").font(theme::semibold(15.0)));
            ui.add_space(4.0);
            ui.label(format!(
                "Every masked stretch goes back into the readings.{}",
                self.masked_words()
            ));
            ui.add_space(8.0);
            choice = theme::dialog_buttons(
                ui,
                &[
                    DialogButton::new(Role::Default, "Remove All Masks", ""),
                    DialogButton::new(Role::Cancel, "Cancel", ""),
                ],
            )
            .map(|i| i == 0);
        });
        if let Some(yes) = choice {
            self.remove_masks_asked = false;
            if yes {
                self.set_masked(Vec::new());
            }
        }
    }

    /// Mask out the stretches `spans` (start and end in the session's
    /// seconds) in place of those masked before: the readings, the charts
    /// and the steadiness tests are worked out again without them.
    pub(super) fn set_masked(&mut self, spans: Vec<(f64, f64)>) {
        let pairs: Vec<[f64; 2]> = spans.iter().map(|&(a, b)| [a, b]).collect();
        self.put_masked(timegrapher_core::session::masked_spans(&pairs));
        if let Some(live) = self.live.as_mut() {
            live.set_masked(&self.masked);
        }
        // A Positions Test's positions are read again without them.
        if let Some(log) = self.live.as_ref().and_then(steady::log_of) {
            let clock = self.reading_clock(log.duration_s);
            if let Some(test) = self.guided.as_mut() {
                test.reread(&log, clock.as_ref(), &self.masked);
            }
        }
        self.refresh_trend();
    }

    /// Add the stretch from `a` to `b` to those masked; one under half a
    /// second, a slip of the mouse, is left alone.
    pub(super) fn mask(&mut self, a: f64, b: f64) {
        let (a, b) = (a.min(b).max(0.0), a.max(b).min(self.latest_s().max(0.0)));
        if b - a < 0.5 {
            return;
        }
        let mut v = self.masked.clone();
        v.push((a, b));
        self.set_masked(v);
    }

    /// Move the edges of the masked stretch `was` to those of `now`; one
    /// left under half a second is brought back. Stretches it now
    /// overlaps join it.
    pub(super) fn reshape_mask(&mut self, was: (f64, f64), now: (f64, f64)) {
        let Some(i) = self.masked.iter().position(|&m| m == was) else {
            return;
        };
        let mut v = self.masked.clone();
        v.remove(i);
        let (a, b) = (now.0.max(0.0), now.1.min(self.latest_s().max(0.0)));
        if b - a >= 0.5 {
            v.push((a, b));
        }
        self.set_masked(v);
    }

    /// The Position Brush's menu button in the Paper Strip toolbar: its
    /// position's code on a teal swatch, and a menu of the six positions, then Stop
    /// Painting while it is on. In the toolbar's "…" menu, a menu of the
    /// same.
    pub(super) fn brush_control(&mut self, ui: &mut egui::Ui, live: bool) {
        let pal = theme::pal(ui);
        let test = self.in_test();
        let why_not = if !live {
            Some("Nothing to paint yet. Listen, or open a recording.")
        } else if test && self.guided_running() {
            Some("Not during a Positions Test: the test sets each position.")
        } else if self.reserve_running() {
            Some("Not during a Power Reserve Test: it runs in one position.")
        } else if test || self.positions_take {
            Some(POSITIONS_TAKE_PAINT)
        } else {
            None
        };
        if why_not.is_some() {
            self.brush = None;
        }
        let shown = self.brush.unwrap_or(self.position);
        ui.add_enabled_ui(why_not.is_none(), |ui| {
            if !theme::in_toolbar(ui) {
                ui.menu_button("Position Brush", |ui| self.brush_items(ui, pal))
                    .response
                    .on_hover_text(BRUSH_HINT)
                    .on_disabled_hover_text(why_not.unwrap_or_default());
                return;
            }
            let font = egui::FontId::proportional(12.0);
            let code = ui
                .painter()
                .layout_no_wrap(shown.code().to_string(), font, theme::ON_TEAL);
            let w = 4.0 + 18.0 + 4.0 + 3.0 + code.size().x + 3.0 + 4.0 + 10.0 + 4.0;
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 26.0), egui::Sense::click());
            let on = self.brush.is_some();
            let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
            let radius = egui::CornerRadius::same(theme::CONTROL_RADIUS);
            if on {
                ui.painter().rect(
                    rect.shrink(0.5),
                    radius,
                    pal.positions.gamma_multiply(0.16),
                    egui::Stroke::new(1.0_f32, pal.positions.gamma_multiply(0.45)),
                    egui::StrokeKind::Inside,
                );
            } else if resp.hovered() || open {
                ui.painter().rect_filled(rect, radius, pal.control_hover);
            }
            let enabled = ui.is_enabled();
            let ink = if on {
                pal.positions
            } else {
                pal.text_tertiary.gamma_multiply(0.75)
            };
            let icon = egui::Rect::from_min_size(rect.min + Vec2::new(4.0, 4.0), Vec2::splat(18.0));
            crate::icons::draw(ui.painter(), icon, Icon::Highlighter, ink);
            // The code in dark text on a teal swatch.
            let code_at = egui::Pos2::new(
                icon.right() + 4.0 + 3.0,
                rect.center().y - code.size().y / 2.0,
            );
            let swatch =
                egui::Rect::from_min_size(code_at, code.size()).expand2(Vec2::new(3.0, 1.0));
            let fade = if enabled { 1.0 } else { 0.4 };
            ui.painter()
                .rect_filled(swatch, 3.0, pal.positions.gamma_multiply(fade));
            ui.painter()
                .galley(code_at, code, theme::ON_TEAL.gamma_multiply(fade));
            // The ▾, drawn as the pop-ups' is.
            let c = egui::Pos2::new(rect.right() - 9.0, rect.center().y + 1.0);
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    c + Vec2::new(-3.5, -1.75),
                    c + Vec2::new(3.5, -1.75),
                    c + Vec2::new(0.0, 1.75),
                ],
                if enabled { pal.text } else { pal.text_tertiary },
                egui::Stroke::NONE,
            ));
            resp.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    enabled,
                    on,
                    "Position Brush",
                )
            });
            let resp = resp
                .on_hover_text(format!("Position Brush: {BRUSH_HINT}"))
                .on_disabled_hover_text(format!("Position Brush: {}", why_not.unwrap_or_default()));
            egui::Popup::menu(&resp).show(|ui| self.brush_items(ui, pal));
        });
    }

    /// The Position Brush menu: each position as its code on a teal swatch
    /// and its name, and Stop Painting while one is in use.
    fn brush_items(&mut self, ui: &mut egui::Ui, pal: &theme::Palette) {
        let font = egui::FontId::proportional(13.0);
        let widths = Position::ALL.map(|p| {
            ui.painter()
                .layout_no_wrap(p.code().to_string(), font.clone(), pal.text)
                .size()
                .x
        });
        // The names in a column of their own after the codes.
        let column = widths.iter().copied().fold(0.0, f32::max);
        for (p, w) in Position::ALL.into_iter().zip(widths) {
            let mut job = egui::text::LayoutJob::default();
            // The code in dark text on a teal swatch, readable in both
            // themes.
            job.append(
                p.code(),
                0.0,
                egui::TextFormat {
                    background: pal.positions,
                    ..egui::TextFormat::simple(font.clone(), theme::ON_TEAL)
                },
            );
            job.append(
                p.name(),
                8.0 + column - w,
                egui::TextFormat::simple(font.clone(), pal.text),
            );
            // The one in use framed, its code still readable.
            let picked = self.brush == Some(p);
            if ui
                .add(egui::Button::new(job).frame_when_inactive(picked))
                .on_hover_text(format!(
                    "Drag along the strip to mark a stretch {}",
                    p.name()
                ))
                .clicked()
            {
                self.brush = Some(p);
                self.masking = false;
                ui.close();
            }
        }
        if self.brush.is_some() {
            ui.separator();
            if ui
                .add(
                    egui::Button::new("Stop Painting")
                        .shortcut_text("Esc")
                        .wrap_mode(egui::TextWrapMode::Extend),
                )
                .clicked()
            {
                self.brush = None;
                ui.close();
            }
        }
    }

    /// Set the stretch from `a` to `b` to the Position Brush's position;
    /// one under half a second, a slip of the mouse, is left alone. A
    /// stretch reaching the newest beat (within a second) puts the watch
    /// in that position from then on.
    pub(super) fn paint_position(&mut self, a: f64, b: f64) {
        let Some(pos) = self.brush else { return };
        let latest = self.latest_s().max(0.0);
        let (a, mut b) = (a.min(b).max(0.0), a.max(b).min(latest));
        // To a tenth of a second: finer than a pointer can place it.
        let (a, b0) = (tenth(a), tenth(b));
        b = b0.min(latest);
        if b - a < 0.5 {
            return;
        }
        if b >= latest - 1.0 {
            b = latest;
        }
        let now = self.session.paint(a, b, pos, self.position, latest);
        self.positions_moved(now);
    }

    /// Take the painting off the painted stretch over `t`: the positions
    /// recorded there come back, with their changes and settling.
    pub(super) fn remove_painting(&mut self, t: f64) {
        let latest = self.latest_s().max(0.0);
        if let Some(now) = self.session.remove_painting(t, self.position, latest) {
            self.positions_moved(now);
        }
    }

    /// Move the change of position at `was` to `now`.
    pub(super) fn move_position_change(&mut self, was: f64, now: f64) {
        let latest = self.latest_s().max(0.0);
        let at = self
            .session
            .move_change(was, tenth(now), self.position, latest);
        self.positions_moved(at);
    }

    /// The positions changed by hand, the watch now in `now`: the
    /// readings are cut at each change and settling again, and the
    /// charts, the steadiness tests and the Positions Test table worked
    /// out afresh.
    pub(super) fn positions_moved(&mut self, now: Position) {
        self.position = now;
        if let Some(live) = self.live.as_mut() {
            let heard = live.duration_s();
            let stretches = self.session.stretches();
            let mut breaks: Vec<f64> = stretches
                .iter()
                .skip(1)
                .map(|s| s.0)
                .chain(self.session.settles().iter().map(|s| s.1))
                .filter(|&t| t <= heard)
                .collect();
            breaks.sort_by(f64::total_cmp);
            breaks.dedup();
            live.set_breaks(breaks);
        }
        self.rev.breaks += 1;
        self.refresh_trend();
    }

    /// Bring back the masked stretch over `t`, if there is one.
    pub(super) fn unmask_at(&mut self, t: f64) {
        let v: Vec<(f64, f64)> = self
            .masked
            .iter()
            .copied()
            .filter(|&(a, b)| !(t >= a && t <= b))
            .collect();
        if v.len() != self.masked.len() {
            self.set_masked(v);
        }
    }
}
