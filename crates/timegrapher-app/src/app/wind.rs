//! The watch's wind, on the Listen card (#1665, #1915): when it was last
//! fully wound, kept with the watch picked in Watches or for a Quick
//! Listen's session, set in the card's Wind field or by a test's Start,
//! and each take saved with its hours since then. A take begun or opened
//! shows its own.

use super::*;
use timegrapher_core::full_wind;
use timegrapher_core::reading_text::{
    wind_before_take_words, wind_field_began_words, wind_field_since_words, wind_since_words,
};

/// Whether Start on a test that begins at full wind (the Positions Test's
/// full-wind step, the Power Reserve Test) sets Full Wind Now by itself.
/// Peter, 14 Nov: "Yes, pressing Start on the full-wind step should set
/// Full Wind Now by itself." Recorded on #1665:
/// <https://github.com/peterhudson/timegrapher/issues/1665#issuecomment-6880962654>
pub(super) const START_SETS_FULL_WIND: bool = true;

/// The wind the card keeps.
#[derive(Default)]
pub(super) struct Wind {
    /// A Quick Listen's full wind, Unix seconds: a watch picked keeps its
    /// own in Watches.
    wound_at: Option<f64>,
    /// The take on show, and its hours since full wind as its session file
    /// gives them, read once for it.
    shown: Option<(PathBuf, Option<f64>)>,
    /// The hours typed in Full Wind [ ] h ago.
    ago: String,
}

/// A choice in the Wind part of a Set menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum WindPick {
    Now,
    HoursAgo(f64),
    NotGiven,
}

impl TimegrapherApp {
    /// When the watch on the stand was last fully wound, Unix seconds: the
    /// picked watch's, else the Quick Listen's; none where not given.
    pub(super) fn wound_unix(&self) -> Option<f64> {
        match self.open_watch() {
            Some(e) => e
                .watch
                .full_wind
                .as_deref()
                .and_then(full_wind::unix_of_utc),
            None => self.wind.wound_at,
        }
    }

    /// Full wind at `at` (Unix seconds; none for Not Given): kept with the
    /// watch picked, else for the Quick Listen.
    pub(super) fn set_wound(&mut self, at: Option<f64>) {
        let Some(mut w) = self.open_watch().map(|e| e.watch.clone()) else {
            self.wind.wound_at = at;
            return;
        };
        w.full_wind = at.map(|t| {
            timegrapher_core::recorder::utc(
                std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(t.max(0.0)),
            )
        });
        let saved = match &mut self.library {
            Ok(Some(lib)) => lib.update(&w),
            _ => Err("there is no Watches folder".into()),
        };
        if let Err(e) = saved {
            self.error(format!("Couldn't save the wind to {}: {e}", w.name));
        }
    }

    /// Set the next test against the `i`th of the watch's full-wind tests
    /// on offer.
    pub(super) fn pick_full_wind(&mut self, i: usize) {
        if let Some(f) = self.guided_choices.get(i) {
            self.guided_earlier = Some(f.clone());
            self.guided_earlier_file = false;
        }
    }

    /// A watch picked: a Quick Listen's wind is let go.
    pub(super) fn forget_quick_wind(&mut self) {
        self.wind.wound_at = None;
    }

    /// A test that begins at full wind has started: Full Wind Now, where
    /// [`START_SETS_FULL_WIND`].
    pub(super) fn started_from_full_wind(&mut self) {
        if START_SETS_FULL_WIND {
            self.set_wound(Some(guided::unix_now()));
        }
    }

    /// Hours since full wind at `at_unix`, to a tenth.
    fn wind_h_at(&self, at_unix: f64) -> Option<f64> {
        Some(full_wind::hours_since(self.wound_unix()?, at_unix))
    }

    /// The take's hours since full wind at its start, as it is saved.
    pub(super) fn take_wind_h(&self) -> Option<f64> {
        let started = self
            .session
            .recorder
            .as_ref()
            .and_then(|r| full_wind::unix_of_utc(r.started_utc()));
        self.wind_h_at(started.unwrap_or_else(guided::unix_now))
    }

    /// Each entry's hours since full wind, as a take saves them: a
    /// Positions Test's 24 hour half its own, from its full-wind test;
    /// any other take the card's.
    pub(super) fn entry_wind_h(&self) -> Option<f64> {
        self.guided
            .as_ref()
            .and_then(|t| t.wind_h())
            .or_else(|| self.take_wind_h())
    }

    /// The take on show's hours since full wind (inner none where not
    /// given), or none with no take on show.
    pub(super) fn shown_wind(&mut self) -> Option<Option<f64>> {
        let Some(take) = self.take_on_show() else {
            self.wind.shown = None;
            return None;
        };
        if self.wind.shown.as_ref().map(|s| &s.0) != Some(&take) {
            let h = read_manifest(&take)
                .ok()
                .flatten()
                .and_then(|(m, _)| full_wind::take_wind_h(&m));
            self.wind.shown = Some((take, h));
        }
        self.wind.shown.as_ref().map(|s| s.1)
    }

    /// Set Wind on the take `take`: its hours since full wind given
    /// (`wind_h`) or not, written through core, and its row and the card
    /// read again.
    pub(super) fn set_take_wind(&mut self, take: &Path, wind_h: Option<f64>) {
        // The session's own take: its next save would undo it (Felix).
        if self.take_held().as_deref() == Some(take) {
            return self.error(crate::take_wind::HELD.into());
        }
        if let Err(e) = full_wind::set_take_wind_h(take, wind_h) {
            return self.error(format!("Couldn't set the take's wind: {e}"));
        }
        self.wind.shown = None;
        if let Ok(Some(l)) = &mut self.library {
            l.scan();
        }
    }

    /// The Listen card's Wind field (#1915): a take opened shows its own
    /// wind, and a take begun, running or stopped, its hours from full
    /// wind to its start, fixed; neither can be set there. Before a take,
    /// the watch's wind now, counting up, set from the field.
    pub(super) fn wind_field(&mut self) -> WindField {
        if let Some(h) = self.shown_wind() {
            return WindField::began(h, WIND_SHOWN_HINT);
        }
        if self.live.is_some() {
            return WindField::began(self.entry_wind_h(), WIND_BEGUN_HINT);
        }
        let h = self.wind_h_at(guided::unix_now());
        WindField {
            words: wind_field_since_words(h),
            hover: format!("{}. {WIND_HINT}", wind_since_words(h)),
            settable: self.position_editable(),
            given: h.is_some(),
        }
    }

    /// Act on a choice in the Wind menu.
    pub(super) fn pick_wind(&mut self, pick: WindPick) {
        let now = guided::unix_now();
        self.set_wound(match pick {
            WindPick::Now => Some(now),
            WindPick::HoursAgo(h) => Some(now - h.max(0.0) * 3600.0),
            WindPick::NotGiven => None,
        });
    }

    /// The Wind menu in `ui`: its pick, once one is made, and the menu
    /// closed.
    pub(super) fn wind_menu(&mut self, ui: &mut egui::Ui) -> Option<WindPick> {
        if self.wind.ago.is_empty() {
            let h = self.wind_h_at(guided::unix_now()).unwrap_or(0.0);
            self.wind.ago = fields::plain(h);
        }
        let picked = wind_choices(ui, &mut self.wind.ago);
        if picked.is_some() {
            self.wind.ago.clear();
            ui.close();
        }
        picked
    }
}

/// The Listen card's Wind field as it stands.
pub(super) struct WindField {
    /// "Full 2.0 h ago" before a take, "Began 2.0 h after full" from its
    /// start, or "Not given".
    pub words: String,
    /// The wind in full, and what the field does.
    pub hover: String,
    /// Whether it opens the Wind menu.
    pub settable: bool,
    /// Whether a wind is given: muted where not.
    pub given: bool,
}

impl WindField {
    /// A take's wind, hours from full wind to its start, fixed.
    fn began(h: Option<f64>, hint: &str) -> Self {
        WindField {
            words: wind_field_began_words(h),
            hover: format!("{}. {hint}", wind_before_take_words(h)),
            settable: false,
            given: h.is_some(),
        }
    }
}

/// The Wind field's hover before a take.
const WIND_HINT: &str = "When the watch was last fully wound. Each take is saved with the \
     hours from then to its start. Set it here before a take: Full Wind Now, some hours ago, \
     or Not Given.";

/// The Wind field's hover once a take has begun.
const WIND_BEGUN_HINT: &str = "Fixed when the take began, and saved with it.";

/// The Wind field's hover for a take opened.
const WIND_SHOWN_HINT: &str = "Saved with the take: change it with Set Wind on its row in \
     Watches.";

/// The Wind menu: Full Wind Now, Full Wind [ ] h ago with its hours
/// typed in `ago`, and Not Given; the one picked.
fn wind_choices(ui: &mut egui::Ui, ago: &mut String) -> Option<WindPick> {
    let mut picked = None;
    if ui.selectable_label(false, "Full Wind Now").clicked() {
        picked = Some(WindPick::Now);
    }
    ui.horizontal(|ui| {
        let typed = fields::parse_number(ago).filter(|h| *h >= 0.0);
        if ui
            .add_enabled(typed.is_some(), egui::Button::new("Full Wind").frame(false))
            .clicked()
        {
            picked = typed.map(WindPick::HoursAgo);
        }
        let field = ui.add(egui::TextEdit::singleline(ago).desired_width(36.0));
        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            picked = typed.map(WindPick::HoursAgo);
        }
        ui.label("h ago");
    });
    if ui.selectable_label(false, "Not Given").clicked() {
        picked = Some(WindPick::NotGiven);
    }
    picked
}

#[cfg(test)]
#[path = "../app_wind_tests.rs"]
mod tests;
