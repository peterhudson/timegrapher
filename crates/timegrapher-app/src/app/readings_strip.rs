//! The readings at the top of the Bench ("Top of the Window", #1000,
//! part 3): pinned, the full cards in the layout, as always; unpinned, a
//! strip 44 pt tall saying the position, the watch, ▶ with the take's
//! length, and the three figures, which opens the full cards over the
//! panes on a click, so nothing below them moves. They close when the
//! pointer leaves them, or on Esc. The pin, at the strip's right end,
//! stays where it is either way.

use super::readouts::{
    amplitude_figure, beat_error_figure, card_rects, heard, rate_figure, six_widths,
};
use super::*;
use timegrapher_core::figure_text::true_minus;

/// The strip's height, and the pin's card's, pt.
pub(super) const STRIP_H: f32 = 44.0;

/// The pin's card's width, pt.
const PIN_W: f32 = 40.0;

/// Room between the pieces of text in a strip's card, and inside its
/// edges, pt.
const SPACE: f32 = 8.0;

/// A piece of a strip card's text: its words, size in pt, and whether it
/// is the strong text or the quiet.
pub(super) type Piece = (String, f32, bool);

impl TimegrapherApp {
    /// The readings across the top: the cards pinned in the layout, or
    /// the strip; the pin at the right end.
    pub(super) fn readings(&mut self, ui: &mut egui::Ui) {
        let gap = theme::GAP;
        let total = ui.available_width();
        let h = if self.readings_pinned {
            theme::READINGS_CARD_H
        } else {
            STRIP_H
        };
        let (row, _) = ui.allocate_exact_size(Vec2::new(total, h), egui::Sense::hover());
        let cards =
            egui::Rect::from_min_max(row.min, egui::pos2(row.right() - PIN_W - gap, row.bottom()));
        let pin = egui::Rect::from_min_size(
            egui::pos2(row.right() - PIN_W, row.top()),
            Vec2::new(PIN_W, STRIP_H),
        );
        self.pin_card(ui, pin);
        if self.readings_pinned {
            set_open(ui.ctx(), false);
            let mut inner = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("readings pinned")
                    .max_rect(cards)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            self.full_cards(&mut inner);
        } else {
            self.strip(ui, cards);
        }
        ui.add_space(theme::GAP - ui.spacing().item_spacing.y);
    }

    /// The full cards and their controls, in `ui`'s width.
    fn full_cards(&mut self, ui: &mut egui::Ui) {
        let out = crate::pump::timed("cards", || self.readouts(ui));
        self.card_controls(ui, &out);
    }

    /// The pin: lit while the cards are pinned open in the layout.
    fn pin_card(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let pal = theme::pal(ui);
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(theme::CARD_RADIUS), pal.card);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("readings pin")
                .max_rect(rect)
                .layout(egui::Layout::centered_and_justified(
                    egui::Direction::LeftToRight,
                )),
        );
        let hint = if self.readings_pinned {
            "Unpin the readings: a strip across the top, its cards opening over the panes on a \
             click (View > Compact Readings)"
        } else {
            "Pin the readings' cards open, in the layout (View > Compact Readings)"
        };
        let r = theme::icon_toggle(&mut child, Icon::Pin, self.readings_pinned, pal.accent);
        if r.on_hover_text(hint).clicked() {
            self.run_menu_command(ui.ctx(), menus::MenuCommand::CompactReadings);
        }
    }

    /// The strip in `rect`, unless its cards are open over it; a click
    /// anywhere but ▶ opens them.
    fn strip(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let ctx = ui.ctx().clone();
        if !is_open(&ctx) {
            let pal = theme::pal(ui);
            let cells = card_rects(rect, six_widths(rect.width(), theme::GAP), theme::GAP);
            for c in cells {
                ui.painter()
                    .rect_filled(c, egui::CornerRadius::same(theme::CARD_RADIUS), pal.card);
            }
            // The strip first, so ▶, drawn after, takes its own clicks.
            let strip = ui.interact(rect, ui.id().with("readings strip"), egui::Sense::click());
            self.strip_cells(ui, cells);
            // Over the watch cell with no watch, the strip's one hover
            // also gives the calibre, lift angle and beat rate in full,
            // which the cell may show cut short (Hugh, #1526).
            let on_watch = self.open_watch().is_none()
                && strip.hover_pos().is_some_and(|p| cells[5].contains(p));
            let hover = if on_watch {
                format!("{}\n\nOpen the readings' cards", self.watch_detail())
            } else {
                "Open the readings' cards".to_string()
            };
            if strip
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(hover)
                .clicked()
            {
                set_open(&ctx, true);
            }
        }
        if is_open(&ctx) {
            self.cards_over(ui, rect);
        }
    }

    /// The full cards over the panes, from the strip's top left, with a
    /// shadow; they close when the pointer leaves them, or on Esc.
    fn cards_over(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let ctx = ui.ctx().clone();
        let area = egui::Area::new(ui.id().with("readings over"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .show(&ctx, |ui| {
                let pal = theme::pal(ui);
                let full = egui::Rect::from_min_size(
                    rect.min,
                    Vec2::new(rect.width(), theme::READINGS_CARD_H),
                );
                let back = full.expand(SPACE / 2.0);
                let radius = egui::CornerRadius::same(theme::CARD_RADIUS);
                ui.painter()
                    .add(ui.visuals().popup_shadow.as_shape(back, radius));
                ui.painter().rect_filled(back, radius, pal.window);
                ui.set_min_width(rect.width());
                ui.set_max_width(rect.width());
                self.full_cards(ui);
            });
        let over = area.response.rect.expand(SPACE / 2.0);
        let esc = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        // A menu or list opened from a card stays open over them.
        let away = ctx.pointer_hover_pos().is_some_and(|p| !over.contains(p))
            && !egui::Popup::is_any_open(&ctx);
        if esc || away {
            set_open(&ctx, false);
        }
    }

    /// The strip's cards, each in the place of the card it stands for:
    /// the position, the watch, ▶ with the take's length, and the three
    /// figures. Each says what fits, from all of it down to its figure.
    fn strip_cells(&mut self, ui: &mut egui::Ui, cells: [egui::Rect; 6]) {
        let inner = |c: egui::Rect| c.shrink2(Vec2::new(SPACE + 4.0, 0.0));
        let here = self.shown_position();
        fit(
            ui,
            inner(cells[0]),
            &[
                vec![strong(here.name(), 15.0), quiet(here.code(), 13.0)],
                vec![strong(here.code(), 15.0)],
            ],
        );
        fit(ui, inner(cells[5]), &self.watch_pieces());
        // Listen: ▶, then the take's length and state.
        let listen = inner(cells[1]);
        let mut go = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("strip listen")
                .max_rect(listen)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let length = length_text(self.live.as_ref().map_or(0.0, |l| l.duration_s()));
        let state = self.state_words();
        self.listen_go(&mut go, Some(&format!("\n\n{length} · {state}")));
        let after = go.min_rect().right() + SPACE;
        let words = egui::Rect::from_x_y_ranges(after..=listen.right(), listen.y_range());
        fit(
            ui,
            words,
            &[
                vec![strong(&length, 18.0), quiet(state, 13.0)],
                vec![strong(&length, 18.0)],
            ],
        );
        let r = self.card_reading();
        let r = r.as_ref().filter(|r| heard(Some(r)));
        let figures = [
            ("Rate", r.and_then(rate_figure), "s/d"),
            ("Amplitude", r.and_then(amplitude_figure), ""),
            ("Beat Error", r.and_then(beat_error_figure), "ms"),
        ];
        for (i, (label, f, unit)) in figures.into_iter().enumerate() {
            let f = true_minus(&f.unwrap_or_else(|| "—".into()));
            let mut whole = vec![quiet(label, 13.0), strong(&f, 22.0)];
            let mut bare = vec![strong(&f, 22.0)];
            // At the narrowest window, the figure smaller, its unit kept.
            let mut small = vec![strong(&f, 16.0)];
            if !unit.is_empty() {
                for pieces in [&mut whole, &mut bare, &mut small] {
                    pieces.push(quiet(unit, 13.0));
                }
            }
            fit(ui, inner(cells[2 + i]), &[whole, bare, small]);
        }
    }

    /// The watch cell's facts: the lift angle, "52°", the beat rate as
    /// the Calibre card writes it, "Auto 28800", and the rate alone,
    /// "28800" (`bph_parts`).
    pub(super) fn watch_facts(&self) -> (String, String, String) {
        let heard = self.live.as_ref().and_then(|l| l.bph());
        let set = self.bph_set_by_calibre();
        let angle = format!("{}°", fields::plain(self.lift_deg));
        let (how, rate) = self.bph_parts(set, heard);
        let bare = if rate.is_empty() {
            how.unwrap_or_default().to_string()
        } else {
            rate
        };
        (angle, self.bph_shown(set, heard), bare)
    }

    /// The watch cell's calibre, lift angle and beat rate in full:
    /// "Not Set · 52° · Auto 28800".
    pub(super) fn watch_detail(&self) -> String {
        let (angle, bph, _) = self.watch_facts();
        let calibre = if self.calibre.is_empty() {
            "Not Set"
        } else {
            &self.calibre
        };
        format!("{calibre} · {angle} · {bph}")
    }

    /// The watch's card in the strip: its name, then its calibre, lift
    /// angle and beat rate; where that doesn't fit, a watch's name alone
    /// (its record holds the rest), or in a Quick Listen the lift angle and
    /// beat rate, which the watchmaker sets there.
    pub(super) fn watch_pieces(&self) -> Vec<Vec<Piece>> {
        let (angle, bph, bare) = self.watch_facts();
        let lift = format!("{angle} · {bph}");
        let detail = self.watch_detail();
        match self.open_watch().map(|e| e.watch.name.clone()) {
            Some(name) => vec![
                vec![strong(&name, 15.0), quiet(&detail, 13.0)],
                vec![strong(&name, 15.0)],
            ],
            // With no watch the lift angle and the beat rate are what a
            // watchmaker checks at a glance, so the narrowest keeps both,
            // the angle first and the beat rate without "Auto": where even
            // that is too wide the beat rate is the one cut short (Peter,
            // #1526; Hugh). The cell's hover gives it in full.
            None => vec![
                vec![strong("No Watch", 15.0), quiet(&detail, 13.0)],
                vec![quiet(&lift, 13.0)],
                vec![quiet(&angle, 13.0), quiet(&bare, 13.0)],
            ],
        }
    }
}

fn strong(words: &str, size: f32) -> Piece {
    (words.to_string(), size, true)
}

fn quiet(words: &str, size: f32) -> Piece {
    (words.to_string(), size, false)
}

/// Draw the first of `choices` that fits in `rect`'s width, its pieces
/// left to right and centred in its height. If none does, the last, each
/// piece cut short with "…" where it runs out of room, so a figure is
/// never shown clipped as if it were whole.
fn fit(ui: &egui::Ui, rect: egui::Rect, choices: &[Vec<Piece>]) {
    let pal = theme::pal(ui);
    let painter = ui.painter().with_clip_rect(rect);
    let lay_in = |pieces: &Vec<Piece>, room: f32| -> Vec<std::sync::Arc<egui::Galley>> {
        let mut left = room;
        pieces
            .iter()
            .map(|(words, size, is_strong)| {
                let colour = if *is_strong {
                    pal.text
                } else {
                    pal.text_secondary
                };
                let mut job = egui::text::LayoutJob::single_section(
                    words.clone(),
                    egui::TextFormat::simple(egui::FontId::proportional(*size), colour),
                );
                job.wrap = egui::text::TextWrapping::truncate_at_width(left.max(0.0));
                let g = painter.layout_job(job);
                left -= g.size().x + SPACE;
                g
            })
            .collect()
    };
    let lay = |pieces: &Vec<Piece>| lay_in(pieces, f32::INFINITY);
    let width = |gs: &[std::sync::Arc<egui::Galley>]| {
        gs.iter().map(|g| g.size().x).sum::<f32>() + SPACE * gs.len().saturating_sub(1) as f32
    };
    let mut laid = choices.iter().map(lay);
    let first = laid.next().unwrap_or_default();
    let galleys = if width(&first) <= rect.width() {
        first
    } else {
        laid.find(|gs| width(gs) <= rect.width())
            .or_else(|| choices.last().map(|c| lay_in(c, rect.width())))
            .unwrap_or_default()
    };
    let mut x = rect.left();
    for g in galleys {
        let (w, h) = (g.size().x, g.size().y);
        let at = egui::pos2(x, rect.center().y - h / 2.0);
        painter.galley(at, g, egui::Color32::PLACEHOLDER);
        x += w + SPACE;
    }
}

/// Whether the cards are open over the panes, kept for the frame after.
pub(super) fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp(open_id())).unwrap_or(false)
}

fn set_open(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}

fn open_id() -> egui::Id {
    egui::Id::new("readings open over the panes")
}
