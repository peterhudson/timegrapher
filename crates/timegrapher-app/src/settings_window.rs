//! The Settings window: what isn't about the watch on show, in
//! categories down a sidebar as the Mac's System Settings has them.
//! General holds the app's own preferences. Microphone holds the input,
//! its level and its Quartz Correction: each input's kept clock error with
//! Forget beside it, from the same file `timegrapher clock list` and
//! `clock forget` use. Watches Folder holds where the watches and their
//! takes are kept. Calibres holds the calibre table and the user's custom
//! calibres.

use crate::theme::{self, Icon};
use eframe::egui::{self, RichText, Vec2};
use std::path::Path;
use timegrapher_core::clockstore::{ClockStore, DeviceClock};
use timegrapher_core::library::Library;
use timegrapher_core::library::{length_text, size_text};

/// The window's categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    General,
    Microphone,
    WatchesFolder,
    Calibres,
}

impl Tab {
    /// The category's name as the app's settings keep it.
    pub fn key(self) -> &'static str {
        match self {
            Tab::General => "general",
            Tab::Microphone => "microphone",
            Tab::WatchesFolder => "watches_folder",
            Tab::Calibres => "calibres",
        }
    }

    /// The category kept as `key`; General for one unknown.
    pub fn from_key(key: &str) -> Tab {
        TABS.iter()
            .map(|t| t.0)
            .find(|t| t.key() == key)
            .unwrap_or(Tab::General)
    }
}

/// The sidebar's categories, in order, each with its icon, name and hover.
pub const TABS: [(Tab, Icon, &str, &str); 4] = [
    (
        Tab::General,
        Icon::Settings,
        "General",
        "How the app looks, whether it asks about updates, listening when the app opens and \
         Keep Awake",
    ),
    (
        Tab::Microphone,
        Icon::Microphone,
        "Microphone",
        "The microphone to listen to, its input level, its peak and each input's Quartz \
         Correction",
    ),
    (
        Tab::WatchesFolder,
        Icon::WatchBox,
        "Watches Folder",
        "The folder your watches and their takes are kept in",
    ),
    (
        Tab::Calibres,
        Icon::Gears,
        "Calibres",
        "Every calibre in a table to sort and filter, your short list and your custom calibres",
    ),
];

/// The sidebar: each category with its icon and name, the one picked in
/// the accent colour. True when another was picked.
pub fn sidebar(ui: &mut egui::Ui, tab: &mut Tab) -> bool {
    let pal = theme::pal(ui);
    let mut changed = false;
    for &(t, icon, name, hint) in TABS.iter() {
        let (rect, r) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), egui::Sense::click());
        let picked = *tab == t;
        r.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, picked, name)
        });
        let r = r.on_hover_text(hint);
        if picked {
            ui.painter().rect_filled(rect, 6.0, pal.accent);
        } else if r.hovered() {
            ui.painter().rect_filled(rect, 6.0, pal.control_hover);
        }
        let (ink, text) = if picked {
            (pal.on_accent, pal.on_accent)
        } else {
            (pal.text_secondary, pal.text)
        };
        let square = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 18.0, rect.center().y),
            Vec2::splat(18.0),
        );
        crate::icons::draw(ui.painter(), square, icon, ink);
        let galley =
            ui.painter()
                .layout_no_wrap(name.to_string(), egui::FontId::proportional(13.5), text);
        let at = egui::pos2(rect.left() + 36.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().with_clip_rect(rect).galley(at, galley, text);
        if r.clicked() && !picked {
            *tab = t;
            changed = true;
        }
        ui.add_space(2.0);
    }
    changed
}

pub const QUARTZ_HELP: &[&str] = &[
    "A sound card's quartz clock runs a little fast or slow, so every rate read on it is off \
     by the same amount. While you listen, the input's clock is timed here against the \
     computer's; after 20 minutes Keep stores that error for the input, and its rates are \
     shown corrected as True Clock from then on. Each input keeps its own. It is only right \
     if the computer's clock is kept by internet time.",
    "Each input kept is listed here with its error, when it was measured and how. Forget \
     asks first, then removes it: rates from that input are then read on its own clock again, uncorrected, \
     until it is timed and kept afresh. Do this after changing the microphone or sound card \
     behind an input's name.",
    "The command line and the MCP server read the same list, so \
     Forget here forgets it for them too.",
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
    theme::group(ui, "Watches Folder", None, |g| {
        let Some(root) = root else {
            g.row(
                "No Home Folder",
                theme::Caption::Text("Choose a folder to keep your watches in."),
                "There is no home folder to keep your watches in",
                |ui| {
                    if ui
                        .add(theme::secondary(ui, "Choose Another…"))
                        .on_hover_text(
                            "Use a Watches folder somewhere else, or an empty folder for a new one",
                        )
                        .clicked()
                    {
                        act = Some(LibraryAction::Choose);
                    }
                },
            );
            return;
        };
        let path = root.display().to_string();
        g.row(
            "Location",
            theme::Caption::Text(&path),
            if chosen {
                "The folder you chose"
            } else {
                "The default: Timegrapher Data in your home folder"
            },
            |ui| {
                if ui
                    .add_enabled(root.is_dir(), theme::secondary(ui, show_in_words()))
                    .on_hover_text("Open the Watches folder")
                    .on_disabled_hover_text("The folder isn't there yet")
                    .clicked()
                {
                    act = Some(LibraryAction::Show);
                }
            },
        );
        match lib {
            Ok(Some(l)) => {
                let takes =
                    l.watches.iter().map(|w| w.takes.len()).sum::<usize>() + l.unsorted.takes.len();
                let held = format!(
                    "{}, {}, {}",
                    count(l.watches.len(), "watch", "watches"),
                    count(takes, "take", "takes"),
                    size_text(l.bytes())
                );
                let why = match (&l.read_only, moving) {
                    (Some(why), None) => format!("{why}."),
                    _ => "Every watch, its photos and its takes.".to_string(),
                };
                g.row(
                    &held,
                    theme::Caption::Text(&why),
                    "What the Watches folder holds",
                    |_| {},
                );
                if !l.unreadable.is_empty() {
                    g.whole(|ui| {
                        theme::alert_label(
                            ui,
                            theme::Alert::Warning,
                            &format!(
                                "{} can't be read and {} left as {}:",
                                count(l.unreadable.len(), "folder", "folders"),
                                if l.unreadable.len() == 1 { "is" } else { "are" },
                                if l.unreadable.len() == 1 {
                                    "it is"
                                } else {
                                    "they are"
                                },
                            ),
                            false,
                        );
                        for e in &l.unreadable {
                            ui.label(RichText::new(e).small().color(pal.text_secondary));
                        }
                    });
                }
            }
            Ok(None) => {
                g.whole(|ui| {
                    ui.label(
                        RichText::new("No watches are kept here yet.").color(pal.text_secondary),
                    );
                });
            }
            Err(e) => {
                g.whole(|ui| {
                    theme::alert_label(
                        ui,
                        theme::Alert::Bad,
                        &format!("Your watches can't be read: {e}"),
                        false,
                    );
                });
            }
        }
        if let Some(to) = moving {
            g.whole(|ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Moving your watches to {}", to.display()));
                });
            });
            return;
        }
        g.whole(|ui| {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        matches!(lib, Ok(Some(_))),
                        theme::secondary(ui, "Move Watches…"),
                    )
                    .on_hover_text(
                        "Move the whole Watches folder into another folder and keep using it \
                         there. It asks first.",
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
                        .on_hover_text(
                            "Use Timegrapher Data in your home folder again. Nothing is moved.",
                        )
                        .clicked()
                {
                    act = Some(LibraryAction::UseDefault);
                }
            });
        });
    });
    act
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

/// The input listening, as its clock is being timed against the
/// computer's.
pub struct Timing {
    /// The input's name as the microphone menu shows it.
    pub shown: String,
    /// Seconds of sound heard since timing began, and how many it needs.
    pub heard_s: f64,
    pub needed_s: f64,
    /// Once timed, the error in words, with the whole of it for a hover,
    /// and whether it is offered to keep (none kept, or the kept one is
    /// off from it).
    pub timed: Option<(String, String)>,
    pub offered: bool,
}

/// What a press in the Quartz Correction section asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuartzAct {
    /// Keep the clock as timed for the input listening.
    Keep,
    /// Forget the input's kept clock.
    Forget(String),
}

/// The Quartz Correction section: the input listening timed against the
/// computer's clock, with Keep once it is, then each input's kept clock
/// error, with Forget beside it; `current` is the input listening,
/// marked. `store` is the list as last read, or why it couldn't be.
pub fn quartz_section(
    ui: &mut egui::Ui,
    store: &Result<ClockStore, String>,
    current: Option<&str>,
    timing: Option<&Timing>,
) -> Option<QuartzAct> {
    let pal = theme::pal(ui);
    let mut act = None;
    theme::group(ui, "Quartz Correction", Some(QUARTZ_HELP), |g| {
        if let Some(t) = timing {
            let label = format!("Timing \"{}\"", t.shown);
            let hint = "The input listening, timed against the computer's clock";
            match &t.timed {
                Some((words, hint_words)) => {
                    let caption = if t.offered {
                        words.clone()
                    } else {
                        format!("{words}. The correction kept for it agrees.")
                    };
                    g.row(&label, theme::Caption::Text(&caption), hint, |ui| {
                        if t.offered
                            && ui
                                .button("Keep")
                                .on_hover_text(format!(
                                    "Keep this input's clock error and correct its rates. \
                                     {hint_words}"
                                ))
                                .clicked()
                        {
                            act = Some(QuartzAct::Keep);
                        }
                    });
                }
                None => {
                    let heard = format!(
                        "{} of {} heard",
                        length_text(t.heard_s),
                        length_text(t.needed_s)
                    );
                    g.row(&label, theme::Caption::Text(&heard), hint, |ui| {
                        let frac = (t.heard_s / t.needed_s).clamp(0.0, 1.0) as f32;
                        ui.add(
                            egui::ProgressBar::new(frac)
                                .desired_width(120.0)
                                .desired_height(6.0),
                        )
                        .on_hover_text(
                            "Timed once this much sound has been heard in one go; a break in \
                             listening starts it again",
                        );
                    });
                }
            }
        }
        let store = match store {
            Ok(s) => s,
            Err(e) => {
                g.whole(|ui| {
                    theme::alert_label(
                        ui,
                        theme::Alert::Bad,
                        &format!("The kept corrections can't be read: {e}"),
                        false,
                    );
                });
                return;
            }
        };
        let none_now = current.filter(|d| store.get(d).is_none());
        let none_words = match (none_now, store.devices.is_empty()) {
            (Some(now), _) => Some(format!(
                "None kept for \"{now}\", the input listening. Once it is timed above, Keep \
                 stores it here."
            )),
            (None, true) => Some(
                "None kept for any input. Listen for 20 minutes or more and the input is \
                 timed here, with Keep to store it."
                    .to_string(),
            ),
            (None, false) => None,
        };
        if let Some(w) = none_words {
            g.whole(|ui| {
                ui.add(egui::Label::new(RichText::new(w).color(pal.text_secondary)).wrap());
            });
        }
        for c in &store.devices {
            let name = if Some(c.device.as_str()) == current {
                format!("{}  (listening)", c.device)
            } else {
                c.device.clone()
            };
            let over = if c.span_s > 0.0 {
                format!(" over {}", length_text(c.span_s))
            } else {
                String::new()
            };
            let caption = format!(
                "{}. Measured {}, {}{over}.",
                error_words(c),
                c.measured_utc.get(..10).unwrap_or(&c.measured_utc),
                c.how_text()
            );
            let hint = format!(
                "This input's clock runs {}{}: rates read on it are {} s/d off, and True Clock \
                 takes that off. Measured {} UTC.",
                ppm_words(c.ppm),
                c.ppm_sd
                    .map_or(String::new(), |sd| format!(", give or take {sd:.1} ppm")),
                sd_words(c.rate_error_s_per_day()),
                c.measured_utc,
            );
            g.row(&name, theme::Caption::Text(&caption), &hint, |ui| {
                if ui
                    .button("Forget")
                    .on_hover_text(format!(
                        "Stop correcting rates from \"{}\": they are read on its own clock \
                         again until it is timed and kept afresh. The command line and the \
                         MCP server forget it too.",
                        c.device
                    ))
                    .clicked()
                {
                    act = Some(QuartzAct::Forget(c.device.clone()));
                }
            });
        }
    });
    act
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Quartz Correction times the input listening: how much sound it has
    /// heard until it is timed, then the error with Keep, which it returns
    /// when pressed; nothing offered when the kept one agrees.
    #[test]
    fn quartz_correction_times_the_input_listening_with_keep() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let draw = |timing: &Timing| {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    quartz_section(ui, &Ok(ClockStore::default()), Some("mic"), Some(timing));
                });
            });
            out.shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let mut t = Timing {
            shown: "USB Mic".into(),
            heard_s: 300.0,
            needed_s: 1200.0,
            timed: None,
            offered: false,
        };
        let text = draw(&t);
        assert!(text.iter().any(|l| l == "Timing \"USB Mic\""), "{text:?}");
        assert!(
            text.iter().any(|l| l.starts_with("5:00 of 20:00 heard")),
            "{text:?}"
        );
        assert!(!text.iter().any(|l| l == "Keep"), "{text:?}");
        t.timed = Some(("Runs 20.0 ppm slow".into(), String::new()));
        t.offered = true;
        let text = draw(&t);
        assert!(text.iter().any(|l| l == "Runs 20.0 ppm slow"), "{text:?}");
        assert!(text.iter().any(|l| l == "Keep"), "{text:?}");
        t.offered = false;
        let text = draw(&t);
        assert!(!text.iter().any(|l| l == "Keep"), "{text:?}");
        assert!(text.iter().any(|l| l.contains("agrees")), "{text:?}");
    }

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
