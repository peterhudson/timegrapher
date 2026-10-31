//! The dock: the panes' tiles, their tab rows and the tab rows' arrows.

use super::*;

pub(super) struct PaneBehavior<'a> {
    pub(super) app: &'a mut TimegrapherApp,
    /// The tab row being drawn, set before its tabs are.
    pub(super) row: Option<TabRow>,
}

/// A tab row too long for its width: its two arrows stay in their places,
/// greyed at the row's ends, so nothing slides under the pointer.
#[derive(Clone, Copy)]
pub(super) struct TabRow {
    container: TileId,
    first: Option<TileId>,
    last: Option<TileId>,
    /// Room before the first tab and after the last, which keeps the row's
    /// own arrows drawn at its ends.
    lead: f32,
    trail: f32,
    /// The left and right arrows' places.
    slots: [egui::Rect; 2],
    /// Whether each arrow has further to go.
    can: [bool; 2],
    /// How far a click moves the row.
    step: f32,
    /// The row's own arrow buttons' width.
    arrow: f32,
    layer: egui::LayerId,
    drawn: bool,
}

impl PaneBehavior<'_> {
    /// egui_tiles' default tab (its `Behavior::tab_ui`, 0.14), which an
    /// override can't call.
    fn plain_tab(
        &mut self,
        tiles: &mut Tiles<Pane>,
        ui: &mut egui::Ui,
        id: egui::Id,
        tile_id: TileId,
        state: &egui_tiles::TabState,
    ) -> egui::Response {
        use egui_tiles::Behavior as _;
        let text = self.tab_title_for_tile(tiles, tile_id);
        let close_btn_size = Vec2::splat(self.close_button_outer_size());
        let close_btn_left_padding = 4.0;
        let font_id = egui::TextStyle::Button.resolve(ui.style());
        let galley = text.into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, font_id);
        let x_margin = self.tab_title_spacing(ui.visuals());
        let button_width = galley.size().x
            + 2.0 * x_margin
            + f32::from(state.closable) * (close_btn_left_padding + close_btn_size.x);
        let (_, tab_rect) = ui.allocate_space(Vec2::new(button_width, ui.available_height()));
        let tab_response = ui
            .interact(tab_rect, id, egui::Sense::click_and_drag())
            .on_hover_cursor(self.tab_hover_cursor_icon());
        // A gap where a tab is dragged from.
        if ui.is_rect_visible(tab_rect) && !state.is_being_dragged {
            let bg_color = self.tab_bg_color(ui.visuals(), tiles, tile_id, state);
            let stroke = self.tab_outline_stroke(ui.visuals(), tiles, tile_id, state);
            ui.painter().rect(
                tab_rect.shrink(0.5),
                0.0,
                bg_color,
                stroke,
                egui::StrokeKind::Inside,
            );
            if state.active {
                // The tab joins the pane below it.
                ui.painter().hline(
                    tab_rect.x_range(),
                    tab_rect.bottom(),
                    egui::Stroke::new(stroke.width + 1.0, bg_color),
                );
            }
            let text_color = self.tab_text_color(ui.visuals(), tiles, tile_id, state);
            let text_position = egui::Align2::LEFT_CENTER
                .align_size_within_rect(galley.size(), tab_rect.shrink(x_margin))
                .min;
            ui.painter().galley(text_position, galley, text_color);
            if state.closable {
                let close_btn_rect = egui::Align2::RIGHT_CENTER
                    .align_size_within_rect(close_btn_size, tab_rect.shrink(x_margin));
                let close_btn_id = ui.auto_id_with("tab_close_btn");
                let close_btn_response = ui
                    .interact(close_btn_rect, close_btn_id, egui::Sense::click_and_drag())
                    .on_hover_cursor(egui::CursorIcon::Default);
                let visuals = ui.style().interact(&close_btn_response);
                let rect = close_btn_rect
                    .shrink(self.close_button_inner_margin())
                    .expand(visuals.expansion);
                let stroke = visuals.fg_stroke;
                ui.painter()
                    .line_segment([rect.left_top(), rect.right_bottom()], stroke);
                ui.painter()
                    .line_segment([rect.right_top(), rect.left_bottom()], stroke);
                if close_btn_response.clicked() && self.on_tab_close(tiles, tile_id) {
                    tiles.remove(tile_id);
                }
            }
        }
        self.on_tab_button(tiles, tile_id, tab_response)
    }
}

impl egui_tiles::Behavior<Pane> for PaneBehavior<'_> {
    /// Each pane is a card under its tab.
    fn pane_ui(&mut self, ui: &mut egui::Ui, _tile: TileId, pane: &mut Pane) -> UiResponse {
        let pal = theme::pal(ui);
        let rect = ui.max_rect();
        self.app.pane_top = rect.top();
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(theme::CARD_RADIUS), pal.card);
        let inner = rect.shrink2(Vec2::new(8.0, 6.0));
        // Saving the pane as a picture: its name in place of its toolbar,
        // and what it paints kept for an SVG.
        let exporting = self.app.export.as_ref().is_some_and(|e| e.pane == *pane);
        let layer = ui.layer_id();
        let first = ui
            .ctx()
            .graphics(|g| g.get(layer).map_or(0, |l| l.next_idx().0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            if exporting {
                self.app.export_heading(ui, *pane);
                ui.add_space(4.0);
            } else if self.app.pane_tools(ui, *pane) {
                ui.add_space(4.0);
            }
            // A plot with no room to be read: one line in its place, never
            // a sliver.
            if self.app.too_short(ui, *pane) {
                return;
            }
            match pane {
                Pane::Watch => self.app.watch_pane(ui),
                Pane::Strip => self.app.strip_pane(ui),
                Pane::Sound => self.app.sound_pane(ui),
                p if p.steady_view().is_some() => self
                    .app
                    .steady_pane(ui, p.steady_view().unwrap_or_default()),
                Pane::Isochronism => self.app.isochronism_pane(ui),
                Pane::PositionsTest => self.app.positions_pane(ui),
                Pane::PowerReserve => {
                    self.app.results_rects[1] = Some(ui.max_rect());
                    self.app.reserve_pane(ui)
                }
                Pane::Scope => self.app.scope_pane(ui),
                Pane::Distributions => self.app.distributions_pane(ui),
                Pane::Charts => self.app.charts_pane(ui),
                _ => {}
            }
        });
        if exporting {
            let shapes = ui.ctx().graphics(|g| {
                g.get(layer)
                    .map(|l| l.all_entries().skip(first).cloned().collect())
                    .unwrap_or_default()
            });
            self.app
                .pane_drawn_for_export(ui.ctx(), inner, shapes, pal.card);
        }
        UiResponse::None
    }

    /// A pane averaging over a time of its own names it.
    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        RichText::new(pane.title())
            .font(theme::semibold(12.5))
            .into()
    }

    fn is_tab_closable(&self, tiles: &Tiles<Pane>, tile_id: TileId) -> bool {
        matches!(tiles.get(tile_id), Some(egui_tiles::Tile::Pane(_)))
    }

    /// Closing a tab hides the pane instead of removing it, so the Panes
    /// list can bring it back in the same place.
    fn on_tab_close(&mut self, tiles: &mut Tiles<Pane>, tile_id: TileId) -> bool {
        if let Some(egui_tiles::Tile::Pane(p)) = tiles.get(tile_id) {
            let p = *p;
            set_pane_visible(tiles, p, false);
            self.app.info_msg(format!(
                "Hid {}. Its icon at the top, or the View menu, brings it back.",
                p.title()
            ));
        }
        false
    }

    /// No pane squeezed to a sliver on a short window: the Paper Strip,
    /// the main view, keeps room to read under the cards.
    fn min_size(&self) -> f32 {
        PANE_MIN_H
    }

    fn simplification_options(&self) -> SimplificationOptions {
        SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
        }
    }

    // The tabs sit on the window like captions over their cards, with no
    // bar or outline of their own.
    fn tab_bar_color(&self, visuals: &egui::Visuals) -> Color32 {
        theme::palette(visuals.dark_mode).window
    }

    /// The selected tab takes the card's colour, joined to the card below
    /// it like a folder's tab.
    fn tab_bg_color(
        &self,
        visuals: &egui::Visuals,
        _tiles: &Tiles<Pane>,
        _tile_id: TileId,
        state: &egui_tiles::TabState,
    ) -> Color32 {
        let p = theme::palette(visuals.dark_mode);
        if state.active {
            p.card
        } else {
            p.window
        }
    }

    fn on_tab_button(
        &mut self,
        _tiles: &Tiles<Pane>,
        tile_id: TileId,
        button_response: egui::Response,
    ) -> egui::Response {
        // Where each tab was drawn, for the tab row to scroll to one just
        // picked.
        let rect = button_response.rect;
        button_response
            .ctx
            .data_mut(|d| d.insert_temp(egui::Id::new(("tab rect", tile_id)), rect));
        button_response.on_hover_text(
            "Drag a tab to move the pane, or beside another to show both. The × hides it; \
             its icon at the top, or the View menu, brings it back.",
        )
    }

    fn tab_outline_stroke(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<Pane>,
        _tile_id: TileId,
        _state: &egui_tiles::TabState,
    ) -> egui::Stroke {
        egui::Stroke::NONE
    }

    /// Scroll the tab row to a tab just picked, by its tab or by its icon,
    /// that is out of its view, from where the tabs were last drawn. A row
    /// too long for its width keeps both arrows in their places, greyed at
    /// its ends.
    fn top_bar_right_ui(
        &mut self,
        tiles: &Tiles<Pane>,
        ui: &mut egui::Ui,
        tile_id: TileId,
        tabs: &egui_tiles::Tabs,
        scroll_offset: &mut f32,
    ) {
        self.row = None;
        let shown: Vec<TileId> = tabs
            .children
            .iter()
            .copied()
            .filter(|&c| tiles.is_visible(c))
            .collect();
        let rects: Vec<Option<egui::Rect>> = ui.ctx().data(|d| {
            shown
                .iter()
                .map(|&c| d.get_temp(egui::Id::new(("tab rect", c))))
                .collect()
        });
        let tabs_w: f32 = rects.iter().flatten().map(|r| r.width()).sum();
        // The row's own arrows: buttons of at least 20 points and a gap,
        // which it counts as 20. It draws the left one once scrolled past
        // 20 points, the right one while there is more to its right; room
        // before the first tab and after the last keeps both drawn at the
        // ends.
        let gap = ui.spacing().item_spacing.x;
        let glyph = ui.fonts_mut(|f| {
            f.layout_no_wrap(
                "⏴".into(),
                egui::TextStyle::Button.resolve(ui.style()),
                Color32::WHITE,
            )
            .size()
            .x
        });
        let arrow = (glyph + 2.0 * ui.spacing().button_padding.x).max(TAB_ARROW);
        let (counted, slot) = (TAB_ARROW + gap, arrow + gap);
        let (lead, trail) = (slot, 2.0 * slot - counted + 1.0);
        let avail = ui.available_width();
        let over_key = egui::Id::new(("tab row over", tile_id));
        let step_key = egui::Id::new(("tab row step", tile_id));
        let (was, step) = ui.ctx().data_mut(|d| {
            (
                d.get_temp::<bool>(over_key).unwrap_or(false),
                d.remove_temp::<f32>(step_key),
            )
        });
        let over = tabs_w + lead + trail > avail + 0.1;
        ui.ctx().data_mut(|d| d.insert_temp(over_key, over));
        if !over {
            *scroll_offset = 0.0;
            return;
        }
        let view = avail - 2.0 * slot;
        let (least, most) = (lead, (lead + tabs_w - view).max(lead));
        let mut off = scroll_offset.max(least);
        let pick_key = egui::Id::new(("tab picked", tile_id));
        if let Some(step) = step {
            off += step;
            // An arrow's click ends any move toward a tab just picked.
            if let Some(active) = tabs.active {
                ui.ctx()
                    .data_mut(|d| d.insert_temp(pick_key, (active, TAB_REVEAL_TRIES)));
            }
        } else if let Some(active) = tabs.active {
            let shown_before = ui.ctx().data(|d| d.get_temp::<(TileId, u8)>(pick_key));
            let tries = match shown_before {
                Some((t, n)) if t == active => n,
                _ => 0,
            };
            // A tab new to the row has no place yet: the next frame. Moved
            // a frame at a time, from where it was last drawn, until it is
            // whole in view between the arrows.
            let rect = shown
                .iter()
                .position(|&c| c == active)
                .and_then(|i| rects[i]);
            if let (true, Some(rect)) = (tries < TAB_REVEAL_TRIES, rect) {
                let row = ui.max_rect();
                let (left, right) = (row.left() + slot, row.right() - slot);
                let done = if rect.right() > right + 0.5 {
                    off += rect.right() - right;
                    tries + 1
                } else if rect.left() < left - 0.5 {
                    off -= left - rect.left();
                    tries + 1
                } else {
                    TAB_REVEAL_TRIES
                };
                ui.ctx()
                    .data_mut(|d| d.insert_temp(pick_key, (active, done)));
                ui.ctx().request_repaint();
            }
        }
        let off = off.clamp(least, most);
        // The row moves itself a slot when its left arrow first shows.
        *scroll_offset = if was { off } else { (off - counted).max(20.5) };
        let row = ui.max_rect();
        self.row = Some(TabRow {
            container: tile_id,
            first: shown.first().copied(),
            last: shown.last().copied(),
            lead,
            trail,
            slots: [
                egui::Rect::from_min_max(row.min, egui::pos2(row.left() + slot, row.bottom())),
                egui::Rect::from_min_max(egui::pos2(row.right() - slot, row.top()), row.max),
            ],
            can: [off > least + 0.5, off < most - 0.5],
            step: (view / 3.0).max(20.0),
            arrow,
            layer: ui.layer_id(),
            drawn: false,
        });
    }

    /// egui_tiles' own tab, with room before the first and after the last
    /// of a long row, and the row's arrows drawn over its own.
    fn tab_ui(
        &mut self,
        tiles: &mut Tiles<Pane>,
        ui: &mut egui::Ui,
        id: egui::Id,
        tile_id: TileId,
        state: &egui_tiles::TabState,
    ) -> egui::Response {
        if let Some(row) = self.row.as_mut().filter(|r| !r.drawn) {
            row.drawn = true;
            tab_row_arrows(ui.ctx(), *row);
        }
        let row = self.row;
        if let Some(row) = row.filter(|r| r.first == Some(tile_id)) {
            ui.add_space(row.lead);
        }
        let response = self.plain_tab(tiles, ui, id, tile_id, state);
        if let Some(row) = row.filter(|r| r.last == Some(tile_id)) {
            ui.add_space(row.trail);
        }
        response
    }

    fn tab_bar_hline_stroke(&self, _visuals: &egui::Visuals) -> egui::Stroke {
        egui::Stroke::NONE
    }

    fn tab_text_color(
        &self,
        visuals: &egui::Visuals,
        _tiles: &Tiles<Pane>,
        _tile_id: TileId,
        state: &egui_tiles::TabState,
    ) -> Color32 {
        let p = theme::palette(visuals.dark_mode);
        if state.active {
            p.text
        } else {
            p.text_tertiary
        }
    }

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        TAB_BAR_H
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        8.0
    }

    fn resize_stroke(
        &self,
        style: &egui::Style,
        resize_state: egui_tiles::ResizeState,
    ) -> egui::Stroke {
        let accent = theme::palette(style.visuals.dark_mode).accent;
        match resize_state {
            egui_tiles::ResizeState::Idle => egui::Stroke::NONE,
            egui_tiles::ResizeState::Hovering => {
                egui::Stroke::new(2.0_f32, accent.gamma_multiply(0.6))
            }
            egui_tiles::ResizeState::Dragging => egui::Stroke::new(2.0_f32, accent),
        }
    }
}

/// Frames a tab just picked is moved toward the tab row's view.
const TAB_REVEAL_TRIES: u8 = 8;

/// egui_tiles' tab row arrow, points.
const TAB_ARROW: f32 = 20.0;

/// A long tab row's two arrows, in fixed places over the row's own, which
/// they keep from being clicked: greyed where the row has no further to go.
fn tab_row_arrows(ctx: &egui::Context, row: TabRow) {
    let p = theme::palette(ctx.style().visuals.dark_mode);
    for (side, (glyph, sign)) in [("⏴", -1.0_f32), ("⏵", 1.0)].into_iter().enumerate() {
        let rect = row.slots[side];
        let ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new(("tab arrow ui", row.container, side)),
            egui::UiBuilder::new().layer_id(row.layer).max_rect(rect),
        );
        let can = row.can[side];
        let response = ui.interact(
            rect,
            egui::Id::new(("tab arrow", row.container, side)),
            egui::Sense::click(),
        );
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, p.window);
        let button = egui::Rect::from_center_size(
            egui::pos2(
                if side == 0 {
                    rect.left() + row.arrow / 2.0
                } else {
                    rect.right() - row.arrow / 2.0
                },
                rect.center().y,
            ),
            Vec2::new(row.arrow, TAB_ARROW),
        );
        if can && response.hovered() {
            painter.rect_filled(button, 4.0, p.control_hover);
        }
        let colour = if can {
            p.text_secondary
        } else {
            p.text_tertiary.gamma_multiply(0.45)
        };
        painter.text(
            button.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            egui::TextStyle::Button.resolve(&ctx.style()),
            colour,
        );
        let response = response.on_hover_text(match (side, can) {
            (0, true) => "Scroll the tabs to the left.",
            (_, true) => "Scroll the tabs to the right.",
            (0, false) => "The first tab is in view.",
            _ => "The last tab is in view.",
        });
        if can && response.clicked() {
            ctx.data_mut(|d| {
                d.insert_temp(
                    egui::Id::new(("tab row step", row.container)),
                    sign * row.step,
                )
            });
            ctx.request_repaint();
        }
    }
}
