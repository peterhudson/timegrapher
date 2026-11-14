//! A guided test pane's body, as the Dynamic Poise and Complication panes
//! lay it out: the banner while the test runs, the table beside the
//! result (or the result over the table where the pane is narrow), and
//! the Info panel beside them or under them (#1499, #1576).

use super::*;

/// The gap between the table and the result, and round Info's rule.
pub(super) const GAP: f32 = 28.0;
/// The Info panel's width beside the rest.
pub(super) const INFO_W: f32 = 300.0;

/// How a pane's body is laid out: the table's widths to try beside the
/// result, widest first; the result's least width; the pane width Info
/// needs to sit beside the rest; and the height of the rule before it.
pub(super) struct Layout {
    pub table_ws: &'static [f32],
    pub result_min_w: f32,
    pub info_beside_w: f32,
    pub rule_h: f32,
}

/// What a pane's body draws besides the test's table: what the pane says
/// in its place before a test, its result, and its Info panel.
pub(super) struct Parts<'a> {
    pub about: String,
    /// With no test on the Bench, the take's own table read again, drawn
    /// in `about`'s place (#1619).
    pub kept: Option<&'a dyn Fn(&mut egui::Ui)>,
    pub result: &'a dyn Fn(&mut egui::Ui),
    pub info: &'a dyn Fn(&mut egui::Ui),
}

/// The live test's state for the body: the test, its phase, the figures
/// so far of the step being measured, and the banner's extra line.
pub(super) type Live<'a> = (
    Option<&'a guided::Test>,
    Option<guided::Phase>,
    Option<&'a guided::FreeCell>,
    Option<&'a str>,
);

/// The body under the toolbar, in a scroll area named `id`; what a banner
/// button asked for. `note` is the banner's extra line.
pub(super) fn test_body(
    ui: &mut egui::Ui,
    (id, layout): (&str, &Layout),
    (test, phase, live, note): Live<'_>,
    info: bool,
    parts: &Parts<'_>,
) -> Option<guided::Action> {
    let pal = theme::pal(ui);
    let mut action = None;
    let table = |ui: &mut egui::Ui| match test {
        Some(t) => guided::test_table(ui, t, phase, live),
        None => match parts.kept {
            Some(kept) => kept(ui),
            None => {
                ui.label(RichText::new(&parts.about).color(pal.text_secondary));
            }
        },
    };
    let full = ui.available_rect_before_wrap();
    let beside = info && full.width() >= layout.info_beside_w;
    let main_w = if beside {
        // Less the spacing round the two gaps, so Info ends inside the pane.
        full.width() - INFO_W - GAP - 3.0 * ui.spacing().item_spacing.x
    } else {
        full.width()
    };
    egui::ScrollArea::both()
        .id_salt(id)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(main_w);
                    if let (Some(t), Some(ph)) = (test, phase) {
                        if ph != guided::Phase::Finished {
                            action = guided::banner(ui, t, ph, note);
                            ui.add_space(6.0);
                        }
                    }
                    // Beside the result at the first of the table's widths
                    // that leaves the result its least width; else under it.
                    let side = layout
                        .table_ws
                        .iter()
                        .copied()
                        .find(|w| main_w >= w + GAP + layout.result_min_w);
                    if let Some(table_w) = side {
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(table_w);
                                table(ui);
                            });
                            ui.add_space(GAP);
                            ui.vertical(|ui| (parts.result)(ui));
                        });
                    } else {
                        // The result first (Hugh, #1499).
                        (parts.result)(ui);
                        ui.add_space(8.0);
                        table(ui);
                    }
                    if info && !beside {
                        ui.add_space(8.0);
                        (parts.info)(ui);
                    }
                });
                if beside {
                    ui.add_space(GAP / 2.0);
                    let top = ui.cursor().top();
                    ui.painter().vline(
                        ui.cursor().left(),
                        egui::Rangef::new(top, top + layout.rule_h),
                        egui::Stroke::new(1.0_f32, pal.separator),
                    );
                    ui.add_space(GAP / 2.0);
                    ui.vertical(|ui| {
                        ui.set_width(INFO_W);
                        (parts.info)(ui);
                    });
                }
            });
        });
    action
}
