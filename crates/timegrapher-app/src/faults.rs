//! Possible escapement faults, read from the shape of the Tick and Tock
//! (`timegrapher_core::faults`, the `faults[]` that `analyze` reports).
//! The shape needs a minute of sound, more than the live engine keeps, so
//! the last minute up to the moment shown is read back from the recording
//! (the session's own scratch recording when listening) and analysed as
//! `analyze` would, on a background thread.

use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use timegrapher_core::analysis::{analyze, AnalysisConfig};
use timegrapher_core::audio::Audio;
use timegrapher_core::faults::{Finding, Strength};

/// Seconds of sound each check reads, and the most it waits between checks
/// while listening.
pub const SPAN_S: f64 = 60.0;
pub const EVERY_S: f64 = 30.0;

#[derive(Default)]
pub struct FaultCheck {
    job: Option<Receiver<(f64, Vec<Finding>)>>,
    /// The moment last checked and what was found.
    pub found: Option<(f64, Vec<Finding>)>,
}

impl FaultCheck {
    pub fn clear(&mut self) {
        *self = FaultCheck::default();
    }

    /// Whether a check of the moment `end` is due: every [`EVERY_S`] while
    /// listening, and when the moment shown moves otherwise.
    pub fn due(&self, end: f64, running: bool) -> bool {
        if self.job.is_some() || end < SPAN_S {
            return false;
        }
        let gap = if running { EVERY_S } else { 1.0 };
        self.found
            .as_ref()
            .is_none_or(|(t, _)| (end - t).abs() >= gap)
    }

    pub fn start(
        &mut self,
        ctx: &egui::Context,
        files: Vec<PathBuf>,
        end: f64,
        bph: Option<u32>,
        lift_deg: f64,
    ) {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let from = (end - SPAN_S).max(0.0);
            let found = timegrapher_core::excerpt::cut(&files, from, end - from)
                .map(|pcm| {
                    let full = (1u64 << (pcm.bits_per_sample.max(1) - 1)) as f32;
                    let ch = pcm.channels.max(1) as usize;
                    let audio = Audio {
                        samples: pcm
                            .samples
                            .chunks(ch)
                            .map(|f| f.iter().map(|&s| s as f32).sum::<f32>() / (ch as f32 * full))
                            .collect(),
                        sample_rate: pcm.sample_rate,
                    };
                    analyze(&audio, &config(bph, lift_deg)).summary.faults
                })
                .unwrap_or_default();
            let _ = tx.send((end, found));
            ctx.request_repaint();
        });
        self.job = Some(rx);
    }

    pub fn poll(&mut self) {
        if let Some(r) = self.job.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.found = Some(r);
            self.job = None;
        }
    }

    /// The findings to show for the moment `end`: the last check's, while
    /// it is near enough to describe that moment.
    pub fn at(&self, end: f64) -> &[Finding] {
        match &self.found {
            Some((t, f)) if (end - t).abs() <= EVERY_S + 1.0 => f,
            _ => &[],
        }
    }
}

/// The command line's `analyze` defaults, so the app finds what `analyze`
/// finds on the same minute.
fn config(bph: Option<u32>, lift_deg: f64) -> AnalysisConfig {
    let mut cfg = AnalysisConfig {
        bph,
        escape_teeth: 15,
        ..Default::default()
    };
    cfg.amplitude.lift_deg = lift_deg;
    cfg.envelope.notch_hz = Vec::new();
    cfg.envelope.highpass_hz = 1500.0;
    cfg
}

/// One finding in a line: the fault and the beat it is on.
pub fn line(f: &Finding) -> String {
    format!("{} on the {}", f.label, f.side.word())
}

/// What a finding rests on, for its hint.
pub fn hint(f: &Finding) -> String {
    let strength = match f.strength {
        Strength::Possible => "Possible: the measure is well outside healthy watches",
        Strength::Likely => "Likely: the rule has been confirmed on a real fault",
    };
    let evidence: Vec<String> = f
        .evidence
        .iter()
        .map(|(k, v)| format!("{} {v:.2}", k.replace('_', " ")))
        .collect();
    format!(
        "{strength}. From the shape of the last minute of the {}'s sound: {}. Worth \
         checking on the bench, not a diagnosis. The ? beside Possible Faults says how \
         each is read.",
        f.side.word(),
        evidence.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_is_due_once_a_minute_is_there_and_then_as_the_moment_moves() {
        let mut c = FaultCheck::default();
        assert!(!c.due(30.0, true));
        assert!(c.due(61.0, true));
        c.found = Some((61.0, Vec::new()));
        assert!(!c.due(80.0, true));
        assert!(c.due(92.0, true));
        assert!(c.due(63.0, false));
        assert!(c.at(200.0).is_empty());
    }
}
