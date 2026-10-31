//! Saving a pane as a picture: an SVG from a frame's shapes, a PNG from a screenshot.

use super::*;

impl TimegrapherApp {
    /// Export: save the pane as a PNG picture or an SVG drawing. An
    /// action, so it never folds into the toolbar's "…".
    pub(super) fn export_button(&mut self, ui: &mut egui::Ui, pane: Pane) {
        theme::keep_next(ui);
        if theme::tool_button(ui, Icon::Export, "Export…", EXPORT_HINT, None) {
            let dir = self.export_dir.clone().or_else(|| {
                Path::new(self.file_path.trim())
                    .parent()
                    .map(Path::to_path_buf)
            });
            if let Some(path) = export::ask_path(pane.title(), dir.as_deref()) {
                self.export_dir = path.parent().map(Path::to_path_buf);
                self.start_export(pane, path);
                ui.ctx().request_repaint();
            }
        }
    }

    /// Draw `pane` for a picture on the next frames and save it to `path`.
    pub(super) fn start_export(&mut self, pane: Pane, path: PathBuf) {
        self.export = Some(PaneExport {
            pane,
            path,
            rect: None,
            asked: false,
            frames: 0,
        });
    }

    /// Where a picture of a pane has its toolbar: the pane's name, and a
    /// chart's key, since the tab bar isn't in the picture.
    pub(super) fn export_heading(&self, ui: &mut egui::Ui, pane: Pane) {
        let pal = theme::pal(ui);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(pane.title())
                    .font(theme::semibold(13.0))
                    .color(pal.text),
            );
            let key = if pane == Pane::Charts {
                chart_key(self.charts_drawn, pal)
            } else {
                Vec::new()
            };
            for (name, color, gaps) in key {
                ui.add_space(10.0);
                match gaps {
                    Some(g) => theme::dash(ui, color, g),
                    None => theme::dot(ui, color),
                };
                ui.label(RichText::new(name).small().color(pal.text_secondary));
            }
        });
    }

    /// The pane's shapes are in: an SVG is written now, and a PNG waits for
    /// the screenshot of the frame, asked for here.
    pub(super) fn pane_drawn_for_export(
        &mut self,
        ctx: &egui::Context,
        rect: egui::Rect,
        shapes: Vec<egui::epaint::ClippedShape>,
        background: Color32,
    ) {
        let Some(e) = &mut self.export else {
            return;
        };
        if export::is_svg(&e.path) {
            let (pane, path) = (e.pane, e.path.clone());
            self.export = None;
            let doc = export::svg(&shapes, rect, background);
            self.export_done(pane, &path, std::fs::write(&path, doc));
        } else {
            e.rect = Some(rect);
            if !e.asked {
                e.asked = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            ctx.request_repaint();
        }
    }

    /// A screenshot came back: cut the pane out of it and save it.
    fn screenshot_for_export(&mut self, image: &egui::ColorImage, pixels_per_point: f32) {
        let Some(PaneExport {
            pane,
            path,
            rect: Some(rect),
            asked: true,
            ..
        }) = self.export.take()
        else {
            return;
        };
        let part = export::crop(image, rect, pixels_per_point);
        self.export_done(pane, &path, export::write_png(&path, &part));
    }

    /// A pane being saved as a PNG: the screenshot of its frame, or giving
    /// up when none comes.
    pub(super) fn export_tick(&mut self, ctx: &egui::Context) {
        if self.export.is_some() {
            let shot = ctx.input(|i| {
                i.raw.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = shot {
                self.screenshot_for_export(&image, ctx.pixels_per_point());
            } else if let Some(e) = &mut self.export {
                // A pane that isn't drawn, or a screen that can't be read.
                e.frames += 1;
                if e.frames > 60 {
                    let pane = e.pane;
                    self.export = None;
                    self.error(format!(
                        "Couldn't save {}: the pane wasn't drawn for the picture",
                        pane.title()
                    ));
                }
            }
        }
    }

    fn export_done(&mut self, pane: Pane, path: &Path, result: std::io::Result<()>) {
        match result {
            Ok(()) => self.info_msg(format!("Saved {} to {}", pane.title(), path.display())),
            Err(e) => self.error(format!(
                "Couldn't save {} to {}: {e}",
                pane.title(),
                path.display()
            )),
        }
    }
}

/// A pane being saved as a picture: it's drawn with its name in place of
/// its toolbar until saved, an SVG from that frame's shapes, a PNG from a
/// screenshot of it.
pub(super) struct PaneExport {
    pub(super) pane: Pane,
    path: PathBuf,
    /// Where the pane was drawn, once it has been.
    pub(super) rect: Option<egui::Rect>,
    /// Whether the screenshot has been asked for.
    asked: bool,
    /// Frames waited, so a picture that can't be taken gives up.
    frames: u32,
}
