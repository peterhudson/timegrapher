//! Set Wind… on a take's row in Watches (#1665): its hours from full wind
//! to its start given or corrected after the fact, written through core
//! ([`timegrapher_core::full_wind::set_take_wind_h`]).

use crate::watches::TakeAction;
use eframe::egui;
use std::path::Path;
use timegrapher_core::library::Take;

/// The Set Wind… menu of the take `t` in `dir`, which `changes` says can
/// be written: Full wind [ ] h before it began, or Not Given. What was
/// picked, the menu then closed.
pub fn set_wind_menu(ui: &mut egui::Ui, t: &Take, dir: &Path, changes: bool) -> Option<TakeAction> {
    let mut act = None;
    let id = egui::Id::new(("set wind", dir));
    ui.add_enabled_ui(changes, |ui| {
        ui.menu_button("Set Wind…", |ui| {
            let mut typed: String = ui
                .data_mut(|d| d.get_temp(id))
                .unwrap_or_else(|| t.wind_h.map_or_else(String::new, crate::fields::plain));
            let hours = crate::fields::parse_number(&typed).filter(|h| *h >= 0.0);
            ui.horizontal(|ui| {
                ui.label("Full wind");
                ui.add(egui::TextEdit::singleline(&mut typed).desired_width(36.0));
                ui.label("h before it began");
            });
            if ui
                .add_enabled(hours.is_some(), egui::Button::new("Set"))
                .clicked()
            {
                act = hours.map(|h| TakeAction::SetWind(dir.to_path_buf(), Some(h)));
            }
            if ui.button("Not Given").clicked() {
                act = Some(TakeAction::SetWind(dir.to_path_buf(), None));
            }
            if act.is_some() {
                ui.data_mut(|d| d.remove::<String>(id));
                ui.close();
            } else {
                ui.data_mut(|d| d.insert_temp(id, typed));
            }
        })
        .response
        .on_hover_text(
            "Give or correct how long before this take began the watch was fully wound. \
             Its readings are grouped by it, and with none given it's never counted as full \
             wind.",
        );
    });
    act
}
