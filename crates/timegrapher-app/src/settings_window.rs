//! The Settings window: what isn't about the watch on show. General holds
//! the app's own preferences, the Watches folder where the watches and
//! their takes are kept, and Sound Card Clock Calibration, each input's
//! kept clock error with Forget beside it, from the same file
//! `timegrapher clock list` and `clock forget` use. Calibres holds the
//! calibre list to browse and the user's own calibres.

use crate::theme;
use eframe::egui::{self, RichText};
use std::path::Path;
use timegrapher_core::clockstore::{ClockStore, DeviceClock};
use timegrapher_core::library::Library;

/// The window's sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    General,
    Microphone,
    Calibres,
}

pub const TABS: [(Tab, &str, &str); 3] = [
    (
        Tab::General,
        "General",
        "How the app looks, whether it asks about updates, listening when the app opens, \
         Keep Awake, the folder your watches are kept in, and each input's kept clock error",
    ),
    (
        Tab::Microphone,
        "Microphone",
        "The microphone to listen to, its input level and its peak",
    ),
    (
        Tab::Calibres,
        "Calibres",
        "Every calibre to search, your short list and your own calibres",
    ),
];

pub const CLOCK_HELP: &[&str] = &[
    "A sound card's clock runs a little fast or slow, so every rate read on it is off by \
     the same amount. Listening for long enough times the input's clock against the \
     computer's; Keep then stores that error for the input, and its rates are shown \
     corrected as True Clock from then on.",
    "Each input kept is listed here with its error, when it was measured and how. Forget \
     asks first, then removes it: rates from that input are then read on its own clock again, uncorrected, \
     until it is timed and kept afresh. Do this after changing the microphone or sound card \
     behind an input's name.",
    "The command line and the MCP server read the same list (`timegrapher clock list`, \
     `timegrapher clock forget`), so Forget here forgets it for them too.",
];

pub const LIBRARY_HELP: &[&str] = &[
    "Your watches are kept in one folder, holding a folder for each watch: its details, its \
     photos and every take made of it. Each take inside is a standard take folder, so the \
     command line and the MCP server read it as they read one anywhere else.",
    "Nothing in it records where it is, so the whole folder can be moved, synced or copied to \
     another computer. Move Watches… moves it and keeps using it there; Choose Another… uses \
     a Watches folder somewhere else, such as one copied from another computer, or an empty \
     folder for a new one. Use the Default goes back to Timegrapher Data in your home folder.",
    "Only one copy of the app at a time can change your watches. A second copy opens them to \
     read only, and says so here; the folder can't be moved while another copy has it open.",
];

/// What the Watches Folder section asks the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryAction {
    Show,
    Move,
    Choose,
    UseDefault,
}

/// "Show in Finder", "Show in Explorer" or "Show in File Manager".
pub fn show_in_words() -> &'static str {
    if cfg!(target_os = "macos") {
        "Show in Finder"
    } else if cfg!(target_os = "windows") {
        "Show in Explorer"
    } else {
        "Show in File Manager"
    }
}

/// "1 watch", "3 watches".
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The Watches Folder section: where the watches are kept, what it holds, and the
/// ways to move it or use another. `lib` is the library as last read:
/// `Ok(None)` when none is at `root` yet. `moving` names where it is being
/// moved to, while it is. `chosen` is true when the user picked the
/// location (not the default).
pub fn library_section(
    ui: &mut egui::Ui,
    root: Option<&Path>,
    lib: &Result<Option<Library>, String>,
    moving: Option<&Path>,
    chosen: bool,
) -> Option<LibraryAction> {
    let pal = theme::pal(ui);
    let mut act = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Watches Folder").strong());
        theme::help(ui, "Watches Folder", LIBRARY_HELP);
    });
    ui.add_space(4.0);
    let Some(root) = root else {
        ui.label(
            RichText::new(
                "There is no home folder to keep your watches in: choose a folder for them.",
            )
            .color(pal.text_secondary),
        );
        if ui
            .add(theme::secondary(ui, "Choose Another…"))
            .on_hover_text("Use a Watches folder somewhere else, or an empty folder for a new one")
            .clicked()
        {
            act = Some(LibraryAction::Choose);
        }
        return act;
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new("Location").color(pal.text_tertiary));
        ui.label(root.display().to_string())
            .on_hover_text(if chosen {
                "The folder you chose"
            } else {
                "The default: Timegrapher Data in your home folder"
            });
    });
    match lib {
        Ok(Some(l)) => {
            let takes =
                l.watches.iter().map(|w| w.takes.len()).sum::<usize>() + l.unsorted.takes.len();
            ui.label(format!(
                "{}, {}, {}",
                count(l.watches.len(), "watch", "watches"),
                count(takes, "take", "takes"),
                crate::app::fmt_bytes(l.bytes())
            ));
            if let (Some(why), None) = (&l.read_only, moving) {
                ui.label(RichText::new(format!("{why}.")).color(pal.text_secondary));
            }
            if !l.unreadable.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "{} can't be read and {} left as {}:",
                        count(l.unreadable.len(), "folder", "folders"),
                        if l.unreadable.len() == 1 { "is" } else { "are" },
                        if l.unreadable.len() == 1 {
                            "it is"
                        } else {
                            "they are"
                        },
                    ))
                    .color(pal.bad),
                );
                for e in &l.unreadable {
                    ui.label(RichText::new(e).small().color(pal.text_secondary));
                }
            }
        }
        Ok(None) => {
            ui.label(RichText::new("No watches are kept here yet.").color(pal.text_secondary));
        }
        Err(e) => {
            ui.label(RichText::new(format!("Your watches can't be read: {e}")).color(pal.bad));
        }
    }
    ui.add_space(8.0);
    if let Some(to) = moving {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!("Moving your watches to {}", to.display()));
        });
        return None;
    }
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(root.is_dir(), theme::secondary(ui, show_in_words()))
            .on_hover_text("Open the Watches folder")
            .on_disabled_hover_text("The folder isn't there yet")
            .clicked()
        {
            act = Some(LibraryAction::Show);
        }
        if ui
            .add_enabled(
                matches!(lib, Ok(Some(_))),
                theme::secondary(ui, "Move Watches…"),
            )
            .on_hover_text(
                "Move the whole Watches folder into another folder and keep using it there. It \
                 asks first.",
            )
            .on_disabled_hover_text("No watches are kept here to move")
            .clicked()
        {
            act = Some(LibraryAction::Move);
        }
        if ui
            .add(theme::secondary(ui, "Choose Another…"))
            .on_hover_text(
                "Use a Watches folder somewhere else, such as one copied from another \
                 computer, or an empty folder for a new one. Nothing is moved.",
            )
            .clicked()
        {
            act = Some(LibraryAction::Choose);
        }
        if chosen
            && ui
                .add(theme::secondary(ui, "Use the Default"))
                .on_hover_text("Use Timegrapher Data in your home folder again. Nothing is moved.")
                .clicked()
        {
            act = Some(LibraryAction::UseDefault);
        }
    });
    act
}

/// How a kept clock was measured, in words.
pub fn source_words(source: &str) -> &'static str {
    match source {
        "log" => "from a recording's clock log",
        "measure" => "while listening, against the computer's clock",
        _ => "typed in",
    }
}

/// "12.3 ppm slow", "4.0 ppm fast", or "0.0 ppm" for a clock that keeps
/// time as it reads.
pub fn ppm_words(ppm: f64) -> String {
    let size = format!("{:.1} ppm", ppm.abs());
    if size == "0.0 ppm" {
        size
    } else {
        format!("{size} {}", if ppm > 0.0 { "slow" } else { "fast" })
    }
}

/// A rate error in s/d with its sign, "+1.06" or "−0.42", and "0.00" for
/// one too small to show.
pub fn sd_words(s_per_day: f64) -> String {
    match format!("{s_per_day:+.2}").as_str() {
        "+0.00" | "-0.00" => "0.00".to_string(),
        s => s.replace('-', "−"),
    }
}

/// One kept clock's error in words: "12.3 ppm slow, +1.06 s/d on every rate".
pub fn error_words(c: &DeviceClock) -> String {
    format!(
        "{}, {} s/d on every rate",
        ppm_words(c.ppm),
        sd_words(c.rate_error_s_per_day())
    )
}

/// The Sound Card Clock section: each kept input with Forget beside it.
/// `store` is the list as last read, or why it couldn't be. Returns the
/// input whose Forget was pressed.
pub fn clock_section(ui: &mut egui::Ui, store: &Result<ClockStore, String>) -> Option<String> {
    let pal = theme::pal(ui);
    let mut forget = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Sound Card Clock Calibration").strong());
        theme::help(ui, "Sound Card Clock Calibration", CLOCK_HELP);
    });
    ui.add_space(4.0);
    let store = match store {
        Ok(s) => s,
        Err(e) => {
            ui.label(RichText::new(format!("The kept clocks can't be read: {e}")).color(pal.bad));
            return None;
        }
    };
    if store.devices.is_empty() {
        ui.label(
            RichText::new(
                "No input's clock is kept. Listen for 20 minutes or more and Keep the clock \
                 the Rate card offers, and it is listed here.",
            )
            .color(pal.text_secondary),
        );
        return None;
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            egui::Grid::new("kept clocks")
                .num_columns(4)
                .spacing([18.0, 8.0])
                .striped(true)
                .show(ui, |ui| {
                    for h in ["Input", "Clock", "Measured", ""] {
                        ui.label(RichText::new(h).small().color(pal.text_tertiary));
                    }
                    ui.end_row();
                    for c in &store.devices {
                        ui.label(&c.device);
                        ui.label(error_words(c)).on_hover_text(format!(
                        "This input's clock runs {}{}: rates read on it are {} s/d off, and True \
                         Clock takes that off.",
                        ppm_words(c.ppm),
                        c.ppm_sd
                            .map_or(String::new(), |sd| format!(", give or take {sd:.1} ppm")),
                        sd_words(c.rate_error_s_per_day()),
                    ));
                        let over = if c.span_s > 0.0 {
                            format!(" over {}", crate::strip::fmt_time(c.span_s))
                        } else {
                            String::new()
                        };
                        ui.label(format!(
                            "{}, {}{over}",
                            c.measured_utc.get(..10).unwrap_or(&c.measured_utc),
                            source_words(&c.source)
                        ))
                        .on_hover_text(format!("Measured {} UTC", c.measured_utc));
                        if ui
                            .button("Forget")
                            .on_hover_text(format!(
                            "Stop correcting rates from \"{}\": they are read on its own clock \
                             again until it is timed and kept afresh. The command line and the MCP \
                             server forget it too.",
                            c.device
                        ))
                            .clicked()
                        {
                            forget = Some(c.device.clone());
                        }
                        ui.end_row();
                    }
                });
        });
    forget
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tiny_error_reads_as_none() {
        assert_eq!(sd_words(-0.001), "0.00");
        assert_eq!(sd_words(0.004), "0.00");
        assert_eq!(sd_words(-0.42), "−0.42");
        assert_eq!(sd_words(1.06), "+1.06");
        assert_eq!(ppm_words(-0.01), "0.0 ppm");
        assert_eq!(ppm_words(12.3), "12.3 ppm slow");
    }
}
