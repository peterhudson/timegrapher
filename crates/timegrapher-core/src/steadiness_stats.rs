//! The statistics behind a series' headline on one line, for the app's
//! Steadiness Info and the MCP view alike (one text, #1470).

use super::{p_text, unit_after, SeriesCheck};
use crate::figure_text::fixed_text;
use crate::steady_views::readings_kind;

impl SeriesCheck {
    /// The statistics behind the headline, on one line: readings, median,
    /// spread, scatter from one reading to the next, lag-1 correlation and
    /// Ljung–Box p, the trend and the stretches of one level (each with why
    /// the headline leaves it out, if it does), and the period when found.
    /// ASCII minus; `screen_stats` has the true minus a screen shows.
    pub fn stats_line(&self) -> String {
        let (sp, u) = unit_after(self.unit);
        // At the series' own place, as its readings are written (#1470).
        let at = |v: f64| fixed_text(v, self.series.digits());
        let mut parts = vec![format!(
            "Tested on {} {}",
            self.readings,
            readings_kind(self.step_s)
        )];
        if self.outliers > 0 {
            parts.push(format!(
                "{} left out as more than 6 σ from their 5-minute level",
                self.outliers
            ));
        }
        if let Some(m) = self.median {
            parts.push(format!("median {}{sp}{u}", at(m)));
        }
        if let Some(sd) = self.sd {
            parts.push(format!("SD {}{sp}{u}", at(sd)));
        }
        if let Some(st) = self.short_term_sd {
            // A spread after a ± reads as one: rounded up, as the
            // headline writes it (Norm, #1470).
            parts.push(format!("±{} reading to reading", self.series.fmt_pm(st)));
        }
        if let Some(a) = &self.autocorrelation {
            let lag1 = a.lag_s.iter().zip(&a.r).find(|(l, _)| **l > 0.0);
            if let Some((_, r1)) = lag1 {
                parts.push(format!("lag-1 r {}", fixed_text(*r1, 2)));
            }
            parts.push(format!("Ljung–Box p {}", p_text(a.p_value)));
        }
        // A trend or a split of levels the headline leaves out says why, in
        // the headline's words (Trend::note, Changes::note).
        let why = |n: &Option<String>| n.as_ref().map_or(String::new(), |n| format!(", {n}"));
        if let Some(t) = &self.trend {
            parts.push(format!(
                "trend {}/h{}",
                self.series.fmt_per_hour(t.per_hour),
                why(&t.note)
            ));
        }
        if let Some(ch) = self.changes.as_ref().filter(|ch| ch.segments.len() > 1) {
            parts.push(format!(
                "{} stretches of one level{}",
                ch.segments.len(),
                why(&ch.note)
            ));
        }
        if let Some(p) = &self.period {
            parts.push(format!("period {:.0} s", p.period_s));
        }
        parts.join(" · ")
    }
}
