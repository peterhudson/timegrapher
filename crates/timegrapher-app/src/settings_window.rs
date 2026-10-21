//! The Settings window: what isn't about the watch on show. General holds
//! the app's own preferences, Calibres the calibre list to browse and the
//! user's own calibres, and Sound Card Clock each input's kept clock error
//! with Forget beside it, from the same file `timegrapher clock list` and
//! `clock forget` use.

use crate::theme;
use eframe::egui::{self, RichText};
use timegrapher_core::clockstore::{ClockStore, DeviceClock};

/// The window's sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    General,
    Calibres,
    Clock,
}

pub const TABS: [(Tab, &str, &str); 3] = [
    (
        Tab::General,
        "General",
        "How the app looks and whether it asks about updates",
    ),
    (
        Tab::Calibres,
        "Calibres",
        "Every calibre to search, your short list and your own calibres",
    ),
    (
        Tab::Clock,
        "Sound Card Clock",
        "Each input's clock error, kept to correct its rates, with Forget beside it",
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
        ui.label(RichText::new("Sound Card Clock").strong());
        theme::help(ui, "Sound Card Clock", CLOCK_HELP);
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
