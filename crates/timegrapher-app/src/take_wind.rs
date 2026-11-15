//! Set Wind on a take's row in Watches (#1665): its hours from full wind
//! to its start given or corrected after the fact, written through core
//! ([`timegrapher_core::full_wind::set_take_wind_h`]).

use crate::watches::TakeAction;
use eframe::egui;
use std::path::Path;
use timegrapher_core::library::Take;

/// Why Set Wind is refused on the take the session on the Bench writes.
pub const HELD: &str = "This take is open on the Bench, and its next save there would \
     overwrite the change. Set its wind once it's no longer on the Bench.";

/// The Set Wind menu of the take `t` in `dir`, which `changes` says can
/// be written, unless it is the take the session on the Bench writes
/// (`held`), which its next write would undo: Full wind [ ] h before it
/// began, or Not Given. What was picked, the menu then closed.
pub fn set_wind_menu(
    ui: &mut egui::Ui,
    t: &Take,
    dir: &Path,
    (changes, held): (bool, Option<&Path>),
) -> Option<TakeAction> {
    let mut act = None;
    let id = egui::Id::new(("set wind", dir));
    let held = held == Some(dir);
    ui.add_enabled_ui(changes && !held, |ui| {
        // Kept open on a click in it, so its field can be typed in.
        let open = egui::containers::menu::MenuConfig::new()
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
        egui::containers::menu::SubMenuButton::new("Set Wind")
            .config(open)
            .ui(ui, |ui| {
                let mut typed: String = ui
                    .data_mut(|d| d.get_temp(id))
                    .unwrap_or_else(|| t.wind_h.map_or_else(String::new, crate::fields::plain));
                ui.horizontal(|ui| {
                    ui.label("Full wind");
                    ui.add(egui::TextEdit::singleline(&mut typed).desired_width(36.0));
                    ui.label("h before it began");
                });
                // Read after the field, so Set takes what was just typed.
                let hours = crate::fields::parse_number(&typed).filter(|h| *h >= 0.0);
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
            .0
            .on_hover_text(
                "Give or correct how long before this take began the watch was fully wound. \
             Its readings are grouped by it, and with none given it's never counted as full \
             wind.",
            )
            .on_disabled_hover_text(if held {
                HELD
            } else {
                "This copy of the app can only read your watches"
            });
    });
    act
}
