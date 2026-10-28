//! Calibrating the sound card's clock against a trusted one.
//!
//! A sound card's crystal is typically off by 10–50 ppm (1–4 s/d) and
//! drifts with temperature, so a rate read against it is only as good as
//! the crystal. During a recording, a log of the audio position against
//! NTP-disciplined system time lets each beat time be mapped onto true
//! time. Each log entry is a pair: seconds of audio captured so far (from
//! the frame count) and the system clock at that moment.

use crate::breaks::{self, Break};
use crate::dsp::{median, robust_sd};
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

/// What a clock `ppm` slow (negative: fast) does to a rate, s/d: the
/// rate read on it less the true rate, to first order.
pub fn rate_error_s_per_day(ppm: f64) -> f64 {
    ppm * 1e-6 * 86400.0
}

/// The fitted mapping from audio time to true time.
#[derive(Debug, Clone, Serialize)]
pub struct ClockFit {
    /// Entries used and entries rejected as outliers (late timestamps).
    pub points: usize,
    pub rejected: usize,
    /// Span of the log, seconds.
    pub span_s: f64,
    /// How much faster true time runs than the sound card, ppm. Positive
    /// means the sound card is slow, so uncorrected rates read too fast.
    pub ppm: f64,
    /// The same as a rate error, s/d: subtract it from an uncorrected rate.
    pub rate_error_s_per_day: f64,
    /// Standard error of `ppm` from the scatter of the entries about one
    /// straight line.
    pub ppm_sd: f64,
    /// RMS of the entries about the mapping, milliseconds.
    pub residual_ms: f64,
    /// Whether the mapping follows slow drift (local fits) rather than
    /// one straight line.
    pub tracks_drift: bool,
    /// Pauses in listening seen as steps in the log: the mapping is fitted
    /// with the gaps taken out, and [`ClockFit::map`] puts them back. Each
    /// is at the log's first entry after its step, or at the last entry
    /// before it when the two are under [`PAUSE_PAIR_S`] apart (the
    /// recorder's pair at a pause), until [`ClockFit::place_breaks`] moves
    /// it to the listed break.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub breaks: Vec<Break>,
    /// Set when the log measures the clock too loosely to correct rates
    /// by ([`ClockFit::for_rates`]): what it measured, and why that was
    /// not applied. `ppm` and `rate_error_s_per_day` are then 0, so rates
    /// stay as heard on the sound card's clock, and the mapping keeps only
    /// the log's breaks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withheld: Option<Withheld>,
    /// For each of `breaks`, the audio seconds of the log's last entry
    /// before its step: a listed break must fall at or after it to be that
    /// break.
    #[serde(skip)]
    entry_before: Vec<f64>,
    /// The log's entries as given, kept to fit again with a listed break
    /// the log's own steps miss ([`ClockFit::place_breaks`]).
    #[serde(skip)]
    entries: Vec<(f64, f64)>,
    /// Knots of the mapping: audio seconds and true seconds, both from the
    /// first entry's values.
    #[serde(skip)]
    knots: Vec<(f64, f64)>,
    #[serde(skip)]
    origin: (f64, f64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClockError {
    TooFewPoints(usize),
    NotIncreasing,
    /// The entries' audio times span more than [`MAX_SPAN_S`], seconds.
    TooLong(f64),
}

impl std::fmt::Display for ClockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClockError::TooFewPoints(n) => write!(f, "clock log has {n} usable entries; need 3"),
            ClockError::NotIncreasing => write!(f, "clock log times do not increase"),
            ClockError::TooLong(s) => write!(
                f,
                "clock log audio times span {:.3e} s; a recording is at most {} days",
                s,
                MAX_SPAN_S / 86400.0
            ),
        }
    }
}

impl std::error::Error for ClockError {}

/// A clock log's measurement of the sound card left out of the rates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Withheld {
    /// What the log measured: ppm, its standard error, and the entries
    /// and span it was measured from.
    pub ppm: f64,
    pub ppm_sd: f64,
    pub points: usize,
    pub span_s: f64,
    /// `few_entries`: under [`MIN_POINTS`] entries, too few for the
    /// scatter to say how sure the line is. `unsure`: the standard error
    /// is over [`MAX_SD_PPM`], as uncertain as the error a correction
    /// would take out. `usb_lines`: the recording's USB lines measured the
    /// card, and correct the rates instead ([`ClockFit::rate_from_usb`]).
    pub reason: &'static str,
}

impl Withheld {
    /// One line for a reader: what was measured and why it isn't applied.
    pub fn note(&self) -> String {
        let why = match self.reason {
            "few_entries" => format!("too few entries to trust (under {MIN_POINTS})"),
            "usb_lines" => "the recording's USB lines measure it instead".into(),
            _ => format!("its uncertainty is over {MAX_SD_PPM:.0} ppm"),
        };
        format!(
            "the clock log's {} entries over {:.0} s measure the sound card at {:+.0} ± {:.0} ppm; not applied, as {why}",
            self.points, self.span_s, self.ppm, self.ppm_sd
        )
    }
}

/// The USB lines and a clock log that would correct on its own both
/// measuring the sound card, more than [`DISAGREE_PPM`] apart
/// ([`ClockFit::sources_disagree`]). The lines time the card against the
/// computer's USB clock, the log against true time: on the stored takes
/// those agree within about 1 ppm, but another computer's crystal can be
/// tens of ppm off (30 ppm is 2.6 s/d).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SourcesDisagree {
    /// ppm the card runs slow by the USB lines: the rates are corrected by
    /// this.
    pub usb_ppm: f64,
    /// ppm by the clock log, and its standard error.
    pub log_ppm: f64,
    pub log_ppm_sd: f64,
}

impl SourcesDisagree {
    /// One line for a reader: both figures, and which one corrects.
    pub fn note(&self) -> String {
        format!(
            "the USB lines measure the sound card at {:+.1} ppm and the clock log at {:+.0} ± {:.0} ppm; the rates are corrected by the USB lines",
            self.usb_ppm, self.log_ppm, self.log_ppm_sd
        )
    }
}

/// How far apart, ppm (0.43 s/d), the USB lines and a clock log that would
/// correct on its own may measure the card before
/// [`ClockFit::sources_disagree`] says so.
pub const DISAGREE_PPM: f64 = 5.0;
/// How many of the log's standard errors apart the two must also be.
pub const DISAGREE_SE: f64 = 2.0;

/// Fewest log entries a clock correction is applied from.
pub const MIN_POINTS: usize = 10;
/// Largest standard error, ppm (1.7 s/d), a clock correction is applied
/// with: about the error a crystal typically has (10–50 ppm), which is
/// what leaving the correction out leaves in. A log every minute for
/// half an hour measures to about 13 ppm; a minute of entries, to
/// hundreds. The rule does not look at the error measured, so takes on
/// one card are treated alike however their noise happens to fall.
pub const MAX_SD_PPM: f64 = 20.0;

/// Least-squares line `y = a + b x`.
fn line(p: &[(f64, f64)]) -> (f64, f64) {
    let (a, b, _) = line_sd(p);
    (a, b)
}

/// Least-squares line `y = a + b x` and the standard error of `b`.
fn line_sd(p: &[(f64, f64)]) -> (f64, f64, f64) {
    let n = p.len() as f64;
    let mx = p.iter().map(|q| q.0).sum::<f64>() / n;
    let my = p.iter().map(|q| q.1).sum::<f64>() / n;
    let sxx: f64 = p.iter().map(|q| (q.0 - mx) * (q.0 - mx)).sum();
    let sxy: f64 = p.iter().map(|q| (q.0 - mx) * (q.1 - my)).sum();
    let b = if sxx > 0.0 { sxy / sxx } else { 1.0 };
    let a = my - b * mx;
    let ss: f64 = p.iter().map(|q| (q.1 - a - b * q.0).powi(2)).sum();
    let sd = if p.len() > 2 && sxx > 0.0 {
        (ss / (n - 2.0) / sxx).sqrt()
    } else {
        f64::INFINITY
    };
    (a, b, sd)
}

/// Standard error of the slope `b` shared by stretches that each have
/// their own intercept (split at `gaps`): from the scatter within each
/// stretch about its own mean, with a degree of freedom spent on each
/// stretch's offset. Where the stretches are short, it is far wider than
/// the error of one line through them all.
fn slope_sd_within(p: &[(f64, f64)], gaps: &[Break], b: f64) -> f64 {
    let mut per: Vec<Vec<(f64, f64)>> = vec![Vec::new(); gaps.len() + 1];
    for q in p {
        per[breaks::piece(gaps, q.0)].push(*q);
    }
    let (mut ss, mut sxx, mut used) = (0.0, 0.0, 0usize);
    for r in per.iter().filter(|r| !r.is_empty()) {
        let n = r.len() as f64;
        let mx = r.iter().map(|q| q.0).sum::<f64>() / n;
        let my = r.iter().map(|q| q.1).sum::<f64>() / n;
        for q in r {
            ss += (q.1 - my - b * (q.0 - mx)).powi(2);
            sxx += (q.0 - mx).powi(2);
        }
        used += 1;
    }
    let df = p.len() as f64 - used as f64 - 1.0;
    if df > 0.0 && sxx > 0.0 {
        (ss / df / sxx).sqrt()
    } else {
        f64::INFINITY
    }
}

/// Two log entries closer than this, in audio seconds, either side of a
/// step are the recorder's pair at a pause (its lines are otherwise 10 s
/// apart): a capture block is far shorter.
pub const PAUSE_PAIR_S: f64 = 1.0;

/// Longest span of audio a clock log may cover, seconds: a month, past
/// any power reserve. An entry further out is a broken line, and fitting
/// it would step drift knots across years.
pub const MAX_SPAN_S: f64 = 31.0 * 86400.0;

/// Runs longer than this get a mapping that follows drift.
const DRIFT_SPAN_S: f64 = 4.0 * 3600.0;
/// Half-width of each local fit when following drift.
const DRIFT_HALF_S: f64 = 3600.0;

impl ClockFit {
    /// Fit `(audio_s, true_s)` pairs. Entries more than 5 robust SDs off a
    /// first straight-line fit are dropped (a timestamp taken late because
    /// the logger was descheduled). Runs over four hours are mapped with
    /// local line fits over two hours, every ten minutes, so slow
    /// temperature drift of the crystal is followed.
    pub fn new(pairs: &[(f64, f64)]) -> Result<ClockFit, ClockError> {
        Self::fit_at(pairs, &[])
    }

    /// [`ClockFit::new`] for correcting rates: the clock error is applied
    /// only when the log measures it from at least [`MIN_POINTS`] entries
    /// with a standard error of at most [`MAX_SD_PPM`].
    /// Otherwise the fit keeps the log's breaks but corrects nothing, and
    /// says why in [`ClockFit::withheld`]. A few entries over a minute
    /// carry jitter of tens of ms each, so they can put the error at
    /// 63 ± 276 ppm: applied, that moves a rate by 5.5 s/d on noise.
    /// Left out, the rate keeps the crystal's own error, typically
    /// 10–50 ppm (1–4 s/d), which the measurement could not resolve.
    pub fn for_rates(pairs: &[(f64, f64)]) -> Result<ClockFit, ClockError> {
        let mut fit = Self::new(pairs)?;
        fit.withhold_if_unsure();
        Ok(fit)
    }

    /// Whether the fit corrects rates: from a log measured well enough,
    /// the recording's USB lines, or a correction given.
    pub fn corrects(&self) -> bool {
        self.withheld.is_none_or(|w| w.reason == "usb_lines")
    }

    /// Correct rates by the card's error read from the recording's USB
    /// lines (`ppm`, slow positive), keeping the log only for its breaks;
    /// what the log measured goes in [`ClockFit::withheld`] with reason
    /// `usb_lines`. The lines time the card against the host's USB clock
    /// to about 0.1 ppm in ten minutes, where a log of an entry a minute
    /// for half an hour measures it to about 13 ppm.
    pub fn rate_from_usb(&mut self, ppm: f64) {
        self.hold("usb_lines", ppm);
    }

    /// Where the USB lines set the rate ([`ClockFit::rate_from_usb`]) over
    /// a clock log that would have corrected on its own (at least
    /// [`MIN_POINTS`] entries, a standard error of at most
    /// [`MAX_SD_PPM`]), the two measurements when they differ by more than
    /// [`DISAGREE_PPM`] and by more than [`DISAGREE_SE`] of the log's
    /// standard error: a half-hour log measures the card to about 13 ppm,
    /// so a gap of that size is the log's own scatter. The correction is
    /// the USB lines' either way.
    pub fn sources_disagree(&self) -> Option<SourcesDisagree> {
        let w = self.withheld.filter(|w| w.reason == "usb_lines")?;
        let qualifies = w.points >= MIN_POINTS && w.ppm_sd <= MAX_SD_PPM;
        let gap = (self.ppm - w.ppm).abs();
        (qualifies && gap > DISAGREE_PPM && gap > DISAGREE_SE * w.ppm_sd).then_some(
            SourcesDisagree {
                usb_ppm: self.ppm,
                log_ppm: w.ppm,
                log_ppm_sd: w.ppm_sd,
            },
        )
    }

    /// What [`ClockFit::for_rates`] would withhold from this fit, and why;
    /// `None` when it would apply it. For a fit made with
    /// [`ClockFit::new`], as when measuring a card. A correction given
    /// ([`ClockFit::fixed`]) is not a measurement, and is never unsure.
    pub fn unsure(&self) -> Option<Withheld> {
        if self.withheld.is_some() {
            return self.withheld;
        }
        if self.entries.is_empty() {
            return None;
        }
        let reason = if self.points < MIN_POINTS {
            "few_entries"
        } else if self.ppm_sd.is_nan() || self.ppm_sd > MAX_SD_PPM {
            "unsure"
        } else {
            return None;
        };
        Some(Withheld {
            ppm: self.ppm,
            ppm_sd: self.ppm_sd,
            points: self.points,
            span_s: self.span_s,
            reason,
        })
    }

    fn withhold_if_unsure(&mut self) {
        if let Some(w) = self.unsure() {
            self.withhold(w.reason);
        }
    }

    /// Keep the breaks, and map audio time onto true time one for one
    /// between them, through the middle of the fitted line.
    fn withhold(&mut self, reason: &'static str) {
        self.hold(reason, 0.0);
    }

    /// Keep the breaks, and map audio time onto true time at a card
    /// `ppm` slow between them, through the middle of the fitted line;
    /// what the log measured (its first measurement, if held before) goes
    /// in `withheld` with `reason`.
    fn hold(&mut self, reason: &'static str, ppm: f64) {
        let log = self.withheld.unwrap_or(Withheld {
            ppm: self.ppm,
            ppm_sd: self.ppm_sd,
            points: self.points,
            span_s: self.span_s,
            reason,
        });
        self.withheld = Some(Withheld { reason, ..log });
        let b = 1.0 + ppm * 1e-6;
        if let (Some(&(x0, _)), Some(&(x1, _))) = (self.knots.first(), self.knots.last()) {
            let m = (x0 + x1) / 2.0;
            let y = self.map_gapless(m);
            self.knots = vec![(x0, y - (m - x0) * b), (x1, y + (x1 - m) * b)];
        }
        self.ppm = ppm;
        self.rate_error_s_per_day = rate_error_s_per_day(ppm);
        self.ppm_sd = 0.0;
        self.tracks_drift = false;
    }

    /// [`ClockFit::new`], with a break also at the first entry after each of
    /// `at` (audio seconds; an entry at the same second is the last one
    /// before it), whatever the size of its step.
    fn fit_at(pairs: &[(f64, f64)], at: &[f64]) -> Result<ClockFit, ClockError> {
        let mut p: Vec<(f64, f64)> = pairs
            .iter()
            .copied()
            .filter(|q| q.0.is_finite() && q.1.is_finite())
            .collect();
        if p.len() < 3 {
            return Err(ClockError::TooFewPoints(p.len()));
        }
        p.sort_by(|a, b| a.0.total_cmp(&b.0));
        let origin = p[0];
        for q in p.iter_mut() {
            *q = (q.0 - origin.0, q.1 - origin.1);
        }
        if p[p.len() - 1].0 <= 0.0 || p[p.len() - 1].1 <= 0.0 {
            return Err(ClockError::NotIncreasing);
        }
        if p[p.len() - 1].0 > MAX_SPAN_S {
            return Err(ClockError::TooLong(p[p.len() - 1].0));
        }
        // A pause in listening steps the system time ahead of the audio.
        // Fitted across, it would read as a huge clock error, so each
        // stretch is brought down by its own offset first (the gaps), and
        // the gaps are then set from where each stretch sits about one line.
        let at: Vec<f64> = at.iter().map(|a| a - origin.0).collect();
        let mut gaps: Vec<Break> = breaks::from_clock_at(&p, &at);
        let entry_before: Vec<f64> = gaps
            .iter()
            .map(|g| {
                p.iter()
                    .map(|q| q.0)
                    .filter(|&x| x < g.audio_s)
                    .fold(f64::NEG_INFINITY, f64::max)
                    + origin.0
            })
            .collect();
        let piece = |gaps: &[Break], x: f64| breaks::piece(gaps, x);
        let mut offset: Vec<f64> = std::iter::once(0.0)
            .chain(gaps.iter().scan(0.0, |acc, g| {
                *acc += g.gap_s;
                Some(*acc)
            }))
            .collect();
        if !gaps.is_empty() {
            // One slope for all stretches, each with its own intercept:
            // the slope from the scatter within each stretch alone.
            let mut per: Vec<Vec<(f64, f64)>> = vec![Vec::new(); offset.len()];
            for q in &p {
                per[piece(&gaps, q.0)].push(*q);
            }
            let means: Vec<(f64, f64)> = per
                .iter()
                .map(|r| {
                    let n = r.len().max(1) as f64;
                    (
                        r.iter().map(|q| q.0).sum::<f64>() / n,
                        r.iter().map(|q| q.1).sum::<f64>() / n,
                    )
                })
                .collect();
            let (mut sxy, mut sxx) = (0.0, 0.0);
            for (r, m) in per.iter().zip(&means) {
                for q in r {
                    sxy += (q.0 - m.0) * (q.1 - m.1);
                    sxx += (q.0 - m.0) * (q.0 - m.0);
                }
            }
            if sxx > 0.0 {
                let b = sxy / sxx;
                let a0 = means[0].1 - b * means[0].0;
                for k in 1..offset.len() {
                    if !per[k].is_empty() {
                        offset[k] = means[k].1 - b * means[k].0 - a0;
                    }
                }
            }
            for (k, g) in gaps.iter_mut().enumerate() {
                g.gap_s = offset[k + 1] - offset[k];
            }
            for q in p.iter_mut() {
                q.1 -= offset[piece(&gaps, q.0)];
            }
        }
        let total = p.len();
        let (a, b) = line(&p);
        let res: Vec<f64> = p.iter().map(|q| q.1 - (a + b * q.0)).collect();
        let sd = robust_sd(&res).max(1e-4);
        let mut med = res.clone();
        let m = median(&mut med);
        p = p
            .into_iter()
            .zip(&res)
            .filter(|(_, r)| (*r - m).abs() < 5.0 * sd)
            .map(|(q, _)| q)
            .collect();
        if p.len() < 3 {
            return Err(ClockError::TooFewPoints(p.len()));
        }
        let (a, b, mut b_sd) = line_sd(&p);
        if !gaps.is_empty() {
            b_sd = slope_sd_within(&p, &gaps, b);
        }
        let span = p[p.len() - 1].0 - p[0].0;
        let mut knots = Vec::new();
        if span > DRIFT_SPAN_S {
            let step = 600.0;
            let end = p[p.len() - 1].0;
            let mut k = Vec::new();
            for i in 0..=((end - p[0].0) / step).ceil() as usize {
                let x = (p[0].0 + i as f64 * step).min(end);
                let near: Vec<(f64, f64)> = p
                    .iter()
                    .copied()
                    .filter(|q| (q.0 - x).abs() <= DRIFT_HALF_S)
                    .collect();
                if near.len() >= 3 {
                    let (la, lb) = line(&near);
                    k.push((x, la + lb * x));
                }
            }
            knots = k;
        }
        // Entries too sparse for local fits (a stored correction gives
        // three points over the whole run) keep the one straight line.
        let tracks_drift = knots.len() >= 2;
        if !tracks_drift {
            let (x0, x1) = (p[0].0, p[p.len() - 1].0);
            knots = vec![(x0, a + b * x0), (x1, a + b * x1)];
        }
        let mut fit = ClockFit {
            points: p.len(),
            rejected: total - p.len(),
            span_s: span,
            ppm: (b - 1.0) * 1e6,
            rate_error_s_per_day: rate_error_s_per_day((b - 1.0) * 1e6),
            ppm_sd: b_sd * 1e6,
            residual_ms: 0.0,
            tracks_drift,
            breaks: Vec::new(),
            withheld: None,
            entry_before,
            entries: pairs.to_vec(),
            knots,
            origin,
        };
        let ss: f64 = p
            .iter()
            .map(|q| {
                let r = fit.map_gapless(q.0) - q.1;
                r * r
            })
            .sum();
        fit.residual_ms = (ss / p.len() as f64).sqrt() * 1e3;
        // A gap fitted a few ms below zero (a near-instant stop and resume
        // forced by a listed pause) counts as none, as [`breaks::wall`]
        // takes it, so true time never runs back at a break. A step between
        // two entries under [`PAUSE_PAIR_S`] apart is the recorder's pair
        // at a pause: its line for the last block before the pause, then
        // its line for the first block after the resume, written once that
        // block is in. The sound after the earlier line is already the
        // resumed sound, so the break goes there.
        fit.breaks = gaps
            .into_iter()
            .zip(&fit.entry_before)
            .map(|(g, &e)| {
                let audio_s = g.audio_s + origin.0;
                Break {
                    audio_s: if audio_s - e < PAUSE_PAIR_S {
                        e
                    } else {
                        audio_s
                    },
                    gap_s: g.gap_s.max(0.0),
                }
            })
            .collect();
        Ok(fit)
    }

    /// The fit for a sound card known to be `ppm` slow (negative: fast)
    /// over a run of `duration_s`: one straight line, no breaks. Built
    /// directly rather than from synthetic entries, which past about 13
    /// hours sit far enough apart to read as pauses in listening.
    pub fn fixed(ppm: f64, duration_s: f64) -> ClockFit {
        let b = 1.0 + ppm * 1e-6;
        let d = duration_s.max(3.0);
        ClockFit {
            points: 3,
            rejected: 0,
            span_s: d,
            ppm,
            rate_error_s_per_day: rate_error_s_per_day(ppm),
            ppm_sd: 0.0,
            residual_ms: 0.0,
            tracks_drift: false,
            breaks: Vec::new(),
            withheld: None,
            entry_before: Vec::new(),
            entries: Vec::new(),
            knots: vec![(0.0, 0.0), (d, d * b)],
            origin: (0.0, 0.0),
        }
    }

    /// True time relative to the first entry, with the gaps of the breaks
    /// before audio time `x` (also relative to the first entry).
    fn map_rel(&self, x: f64) -> f64 {
        self.map_gapless(x) + breaks::wall(&self.breaks, x + self.origin.0) - (x + self.origin.0)
    }

    /// True time relative to the first entry with the gaps taken out,
    /// piecewise linear, extended along the end segments.
    fn map_gapless(&self, x: f64) -> f64 {
        let k = &self.knots;
        if k.is_empty() {
            return x;
        }
        if k.len() == 1 {
            return k[0].1 + (x - k[0].0);
        }
        let i = match k.iter().position(|q| q.0 > x) {
            Some(0) => 0,
            Some(i) => i - 1,
            None => k.len() - 2,
        }
        .min(k.len() - 2);
        let (x0, y0) = k[i];
        let (x1, y1) = k[i + 1];
        y0 + (y1 - y0) * (x - x0) / (x1 - x0)
    }

    /// Move each break seen in the log to the break listed for it (from
    /// the take's `session.json`): the latest listed one at or before it,
    /// within [`breaks::JUMP_MATCH_S`] and at or after the log's last entry
    /// before its step. The recorder writes its last line before a pause
    /// at the listed frame, and its first line after the resume a block or
    /// more later, where the log alone puts the break; without this the
    /// audio in between is mapped as if before the gap. A step with no
    /// listed break in that span (the system clock set forward) stays
    /// where the log puts it. The fit itself is unchanged, since no entry
    /// falls in between, except where a listed break is shorter than the
    /// log can show as one ([`breaks::MIN_GAP_S`], as after a quick stop
    /// and resume) and no step the log shows matches it: the fit is then
    /// made again with a break at that step, so the step does not bend the
    /// clock's rate.
    pub fn place_breaks(&mut self, listed: &[Break]) {
        let missed: Vec<f64> = listed
            .iter()
            .filter(|l| l.gap_s.abs() < breaks::MIN_GAP_S)
            .map(|l| l.audio_s)
            .filter(|&a| {
                !self.breaks.iter().zip(&self.entry_before).any(|(b, &e)| {
                    let end = self.entry_after(e).max(b.audio_s);
                    a > e - 1e-6 && a <= end && end - a <= breaks::JUMP_MATCH_S
                })
            })
            .collect();
        if !missed.is_empty() && !self.entries.is_empty() {
            // Each break the fit already has, again at its own entry: the
            // first after the log's last entry before its step.
            let mut at: Vec<f64> = self.entry_before.clone();
            at.extend(&missed);
            if let Ok(f) = Self::fit_at(&self.entries, &at) {
                let held = self.withheld.map(|w| (w.reason, self.ppm));
                *self = f;
                if let Some((reason, ppm)) = held {
                    self.hold(reason, ppm);
                }
            }
        }
        let ends: Vec<f64> = self
            .breaks
            .iter()
            .zip(&self.entry_before)
            .map(|(b, &e)| self.entry_after(e).max(b.audio_s))
            .collect();
        for (i, b) in self.breaks.iter_mut().enumerate() {
            let after = self
                .entry_before
                .get(i)
                .copied()
                .unwrap_or(f64::NEG_INFINITY);
            let end = ends.get(i).copied().unwrap_or(b.audio_s);
            if let Some(l) = listed
                .iter()
                .filter(|l| {
                    l.audio_s > after - 1e-6
                        && l.audio_s <= end
                        && end - l.audio_s <= breaks::JUMP_MATCH_S
                })
                .max_by(|x, y| x.audio_s.total_cmp(&y.audio_s))
            {
                b.audio_s = l.audio_s;
            }
        }
    }

    /// The audio seconds of the log's first entry after `e`, or `e` when
    /// there is none.
    fn entry_after(&self, e: f64) -> f64 {
        let next = self
            .entries
            .iter()
            .map(|q| q.0)
            .filter(|&x| x > e + 1e-9)
            .fold(f64::INFINITY, f64::min);
        if next.is_finite() {
            next
        } else {
            e
        }
    }

    /// Map an audio time (seconds from the start of the recording) onto
    /// true time, in seconds from the start of the recording.
    pub fn map(&self, audio_s: f64) -> f64 {
        let zero = self.map_rel(-self.origin.0);
        self.map_rel(audio_s - self.origin.0) - zero
    }
}

/// Audio seconds per kept entry in a [`ClockTracker`].
const TRACK_BUCKET_S: f64 = 1.0;

/// Measures a live input's clock against the system clock as audio
/// arrives, without a log file.
///
/// A block's arrival time is late by however long the driver held it,
/// which varies from block to block but never makes a block early. So in
/// each second of audio only the block that arrived least late is kept,
/// and the line through those follows the sound card's clock to a
/// fraction of a ppm in twenty minutes, as long as the system clock is
/// kept by NTP.
#[derive(Debug, Clone)]
pub struct ClockTracker {
    fs: f64,
    frames: u64,
    /// The current second's least-late entry: bucket number, audio
    /// seconds and system seconds.
    best: Option<(u64, f64, f64)>,
    pairs: Vec<(f64, f64)>,
}

impl ClockTracker {
    pub fn new(sample_rate: u32) -> ClockTracker {
        ClockTracker {
            fs: sample_rate as f64,
            frames: 0,
            best: None,
            pairs: Vec::new(),
        }
    }

    /// Add a block of `samples` frames that arrived at `at` (its last
    /// sample's arrival).
    pub fn push(&mut self, samples: usize, at: SystemTime) {
        self.frames += samples as u64;
        let Ok(t) = at.duration_since(UNIX_EPOCH) else {
            return;
        };
        let audio = self.frames as f64 / self.fs;
        let sys = t.as_secs_f64();
        let bucket = (audio / TRACK_BUCKET_S) as u64;
        match self.best {
            Some((b, a, s)) if b == bucket => {
                if sys - audio < s - a {
                    self.best = Some((bucket, audio, sys));
                }
            }
            Some((_, a, s)) => {
                self.pairs.push((a, s));
                self.best = Some((bucket, audio, sys));
            }
            None => self.best = Some((bucket, audio, sys)),
        }
    }

    /// Seconds of audio counted so far.
    pub fn audio_s(&self) -> f64 {
        self.frames as f64 / self.fs
    }

    /// The fit so far; `None` until there are a few seconds of entries.
    pub fn fit(&self) -> Option<ClockFit> {
        ClockFit::new(&self.pairs).ok()
    }
}

fn is_time_name(c: &str) -> bool {
    ["unix_s", "unix", "time_s", "epoch", "ntp_s", "time"].contains(&c)
        || c.ends_with("_ns")
        || c.ends_with("_ms")
}

fn is_audio_name(c: &str) -> bool {
    [
        "audio_s",
        "audio_seconds",
        "frames",
        "samples",
        "frame",
        "sample",
    ]
    .contains(&c)
        || c.starts_with("bytes")
}

/// The clock fit of a take whose session file names the clock log `log`,
/// a file in the take's folder `dir`, without its USB lines: its log fitted
/// for rates, with the breaks the take lists placed on it. `files` are the
/// take's sound files, in order. Kept for the app until it opens takes
/// with [`crate::take::clock_of_files`] (#509), which also reads the USB
/// lines; it is that function.
pub fn of_take(
    dir: &std::path::Path,
    log: &str,
    files: &[std::path::PathBuf],
) -> Result<ClockFit, String> {
    crate::take::clock_of_files(dir, Some(log), files, None, None)?.ok_or_else(|| "no clock".into())
}

/// The sound card's error read from the USB lines in the first
/// `usbclock::MAX_DURATION_S` of the first of `files`, as `analyze` reads
/// it; `None` when the recording has none. With a clock log, pass its
/// `ppm` to [`ClockFit::rate_from_usb`].
pub fn usb_lines(
    files: &[std::path::PathBuf],
) -> Result<Option<crate::usbclock::UsbClock>, String> {
    let Some(first) = files.first() else {
        return Ok(None);
    };
    let named = |e: &dyn std::fmt::Display| format!("{}: {e}", first.display());
    let info = crate::audio::info(first).map_err(|e| named(&e))?;
    let keep = (crate::usbclock::MAX_DURATION_S * info.sample_rate as f64) as usize;
    let mut x: Vec<f32> = Vec::new();
    crate::audio::stream(first, 1 << 16, |b| {
        if x.len() < keep {
            x.extend_from_slice(&b[..b.len().min(keep - x.len())]);
        }
    })
    .map_err(|e| named(&e))?;
    Ok(crate::usbclock::measure(&x, info.sample_rate))
}

/// Parse a clock log: one entry per line, CSV or whitespace-separated,
/// `#` comments, with an optional header naming the columns. Other lines
/// that aren't all numbers (a note from the logger) are skipped.
///
/// - Audio position: `audio_s` (seconds), `frames` or `samples`, or
///   `bytes` (of audio data, `bytes_per_frame` to a frame).
/// - System time: `unix_s`, `unix`, `time_s` or `epoch` (seconds), or a
///   name ending in `_ns` or `_ms` for nanoseconds or milliseconds.
///
/// Without a header, the time column is the one that looks like a Unix
/// time (seconds, milliseconds or nanoseconds, told apart by size), the
/// audio column is the other one, and its unit (seconds, frames or bytes)
/// is the one that makes it advance at one second per second. Only the
/// slope of the mapping matters, so a constant offset in the audio count
/// (a file header, a pipe buffer) does no harm.
pub fn parse_log(
    text: &str,
    sample_rate: u32,
    bytes_per_frame: u32,
) -> Result<Vec<(f64, f64)>, String> {
    let fs = sample_rate as f64;
    let mut header: Option<Vec<String>> = None;
    let mut rows: Vec<Vec<f64>> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cells: Vec<&str> = line
            .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
            .filter(|c| !c.is_empty())
            .collect();
        let nums: Vec<Option<f64>> = cells.iter().map(|c| c.parse::<f64>().ok()).collect();
        if nums.iter().any(|v| v.is_none()) {
            // A header names a time and an audio column; any other line
            // that isn't all numbers (a note the logger wrote) is skipped.
            let names: Vec<String> = cells.iter().map(|c| c.to_ascii_lowercase()).collect();
            if header.is_none()
                && rows.is_empty()
                && names.iter().any(|c| is_time_name(c))
                && names.iter().any(|c| is_audio_name(c))
            {
                header = Some(names);
            }
            continue;
        }
        rows.push(nums.into_iter().map(|v| v.unwrap_or(f64::NAN)).collect());
    }
    if rows.len() < 2 {
        return Err(format!("{} entries; need at least 2", rows.len()));
    }
    let width = rows.iter().map(|r| r.len()).min().unwrap_or(0);
    let col = |i: usize| -> Vec<f64> { rows.iter().map(|r| r[i]).collect() };

    // Scale a time column to seconds from the size of its values.
    let time_scale = |v: &[f64]| -> Option<f64> {
        let x = v[0].abs();
        if (1e8..1e11).contains(&x) {
            Some(1.0)
        } else if (1e11..1e14).contains(&x) {
            Some(1e-3)
        } else if (1e17..1e20).contains(&x) {
            Some(1e-9)
        } else {
            None
        }
    };

    let (ai, ascale, ti, tscale) = if let Some(h) = &header {
        let find = |pred: &dyn Fn(&str) -> bool| h.iter().position(|c| pred(c));
        let ti = find(&|c| is_time_name(c))
            .ok_or_else(|| format!("header has no time column: {}", h.join(",")))?;
        let tscale = if h[ti].ends_with("_ns") {
            1e-9
        } else if h[ti].ends_with("_ms") {
            1e-3
        } else {
            1.0
        };
        let (ai, ascale) = if let Some(i) = find(&|c| c == "audio_s" || c == "audio_seconds") {
            (i, 1.0)
        } else if let Some(i) = find(&|c| ["frames", "samples", "frame", "sample"].contains(&c)) {
            (i, 1.0 / fs)
        } else if let Some(i) = find(&|c| c.starts_with("bytes")) {
            (i, 1.0 / (fs * bytes_per_frame as f64))
        } else {
            return Err(format!(
                "header has no audio column (audio_s, frames or bytes): {}",
                h.join(",")
            ));
        };
        (ai, ascale, ti, tscale)
    } else {
        if width < 2 {
            return Err("need two columns: system time and audio position".into());
        }
        let ti = (0..width)
            .find(|&i| time_scale(&col(i)).is_some())
            .ok_or("no column looks like a Unix time")?;
        let tscale = time_scale(&col(ti)).unwrap_or(1.0);
        let ai = (0..width)
            .find(|&i| i != ti)
            .ok_or("no audio position column")?;
        let (a, t) = (col(ai), col(ti));
        let (a0, a1) = (a[0], a[a.len() - 1]);
        let dt = (t[t.len() - 1] - t[0]) * tscale;
        if dt <= 0.0 {
            return Err("times do not increase".into());
        }
        // The median step, so a pause in the log (time jumps, audio does
        // not) doesn't hide the unit.
        let mut steps: Vec<f64> = a
            .windows(2)
            .zip(t.windows(2))
            .filter(|(_, tw)| tw[1] > tw[0])
            .map(|(aw, tw)| (aw[1] - aw[0]) / ((tw[1] - tw[0]) * tscale))
            .collect();
        let per_s = if steps.is_empty() {
            (a1 - a0) / dt
        } else {
            median(&mut steps)
        };
        let ascale = [1.0, fs, fs * bytes_per_frame as f64]
            .into_iter()
            .map(|u| (u, (per_s / u).ln().abs()))
            .filter(|c| c.1.is_finite() && c.1 < 0.05)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|c| 1.0 / c.0)
            .ok_or_else(|| {
                format!("audio column advances {per_s:.1} per second: not seconds, frames or bytes at {sample_rate} Hz")
            })?;
        (ai, ascale, ti, tscale)
    };
    if ai >= width || ti >= width {
        return Err("a row is missing a column".into());
    }
    Ok(rows
        .iter()
        .map(|r| (r[ai] * ascale, r[ti] * tscale))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::Rng;

    #[test]
    fn recovers_a_slow_sound_card() {
        // The sound card runs 20 ppm slow; timestamps have 2 ms of jitter
        // and one entry was logged 400 ms late.
        let mut rng = Rng::new(4);
        let mut pairs: Vec<(f64, f64)> = (0..120)
            .map(|i| {
                let audio = i as f64 * 60.0;
                (audio, 1.7e9 + audio * (1.0 + 20e-6) + 0.002 * rng.normal())
            })
            .collect();
        pairs[50].1 += 0.4;
        let fit = ClockFit::new(&pairs).unwrap();
        assert!((fit.ppm - 20.0).abs() < 0.5, "{}", fit.ppm);
        assert_eq!(fit.rejected, 1);
        assert!((fit.rate_error_s_per_day - 1.728).abs() < 0.05);
        assert!(fit.ppm_sd > 0.0 && fit.ppm_sd < 0.2, "{}", fit.ppm_sd);
        assert!((fit.map(3600.0) - 3600.0 * (1.0 + 20e-6)).abs() < 0.002);
        assert!(!fit.tracks_drift);
    }

    /// `n` entries `every` audio seconds from a card `ppm` slow, with
    /// `jitter_s` of timestamp noise, and a pause of `pause_s` after entry
    /// `pause_after` (none when 0).
    fn noisy_log(
        n: usize,
        every: f64,
        ppm: f64,
        jitter_s: f64,
        pause_after: usize,
        pause_s: f64,
        seed: u64,
    ) -> Vec<(f64, f64)> {
        let mut rng = Rng::new(seed);
        (0..n)
            .map(|i| {
                let audio = i as f64 * every;
                let gap = if pause_after > 0 && i > pause_after {
                    pause_s
                } else {
                    0.0
                };
                let noise = jitter_s * rng.normal();
                (audio, 1.7e9 + audio * (1.0 + ppm * 1e-6) + gap + noise)
            })
            .collect()
    }

    #[test]
    fn a_short_noisy_log_leaves_the_rate_as_heard() {
        // As on a 42 s take: 6 entries with tens of ms of jitter, from a
        // card 6 ppm slow. The line through them is hundreds of ppm out.
        let pairs = noisy_log(6, 8.4, 6.0, 0.027, 0, 0.0, 7);
        let measured = ClockFit::new(&pairs).unwrap();
        assert!(measured.ppm_sd > 100.0, "{}", measured.ppm_sd);
        let fit = ClockFit::for_rates(&pairs).unwrap();
        assert!(!fit.corrects());
        assert_eq!(fit.ppm, 0.0);
        assert_eq!(fit.rate_error_s_per_day, 0.0);
        assert!((fit.map(40.0) - fit.map(10.0) - 30.0).abs() < 1e-9);
        let w = fit.withheld.unwrap();
        assert_eq!(w.reason, "few_entries");
        assert_eq!((w.points, w.ppm), (6, measured.ppm));
        assert!(
            w.note()
                .starts_with("the clock log's 6 entries over 42 s measure the sound card at "),
            "{}",
            w.note()
        );
        let json = serde_json::to_value(&fit).unwrap();
        assert_eq!(json["withheld"]["reason"], "few_entries");
        assert_eq!(json["ppm"], 0.0);

        // Enough entries, but over two minutes they measure the error only
        // to over a hundred ppm.
        let fit = ClockFit::for_rates(&noisy_log(15, 10.0, 6.0, 0.027, 0, 0.0, 8)).unwrap();
        let w = fit.withheld.unwrap();
        assert_eq!(w.reason, "unsure");
        assert!(w.ppm_sd > MAX_SD_PPM, "{w:?}");
    }

    #[test]
    fn a_real_42_s_log_is_withheld() {
        // A 42 s take's log, captured at 48 kHz in 2048-frame callbacks.
        let log = "# system time (Unix seconds) against audio frames captured
unix_s,frames
1792994589.844721787,2048
1792994599.845988770,483328
1792994609.852497827,962560
1792994619.870397431,1443840
1792994629.899871926,1925120
1792994631.826017210,2017280
";
        let pairs = parse_log(log, 48000, 4).unwrap();
        let fit = ClockFit::for_rates(&pairs).unwrap();
        assert_eq!(fit.rate_error_s_per_day, 0.0);
        let w = fit.withheld.unwrap();
        assert_eq!(w.reason, "few_entries");
        assert!(w.ppm_sd > w.ppm.abs(), "{w:?}");
    }

    #[test]
    fn unsure_says_what_for_rates_would_withhold() {
        let short = ClockFit::new(&noisy_log(6, 8.4, 6.0, 0.027, 0, 0.0, 7)).unwrap();
        assert_eq!(short.unsure().unwrap().reason, "few_entries");
        assert_eq!(
            short.unsure(),
            ClockFit::for_rates(&noisy_log(6, 8.4, 6.0, 0.027, 0, 0.0, 7))
                .unwrap()
                .withheld
        );
        let long = ClockFit::new(&noisy_log(361, 10.0, 6.0, 0.002, 0, 0.0, 9)).unwrap();
        assert!(long.unsure().is_none());
        assert!(ClockFit::fixed(20.0, 60.0).unsure().is_none());
    }

    #[test]
    fn a_long_log_corrects_the_rate() {
        // An hour of entries every 10 s, 2 ms of jitter: the 6 ppm is
        // measured to about 0.1 ppm and applied.
        let pairs = noisy_log(361, 10.0, 6.0, 0.002, 0, 0.0, 9);
        let fit = ClockFit::for_rates(&pairs).unwrap();
        assert!(fit.corrects());
        assert!(fit.withheld.is_none());
        assert!((fit.ppm - 6.0).abs() < 0.5, "{}", fit.ppm);
        assert!(serde_json::to_value(&fit)
            .unwrap()
            .get("withheld")
            .is_none());
        // Half an hour of entries a minute apart with 34 ms of jitter, as
        // the corpus's 30 min takes have: measured to about 13 ppm, applied.
        let fit = ClockFit::for_rates(&noisy_log(30, 60.0, 22.0, 0.034, 0, 0.0, 12)).unwrap();
        assert!(fit.corrects(), "{:?}", fit.withheld);
        assert!(
            fit.ppm_sd > 5.0 && fit.ppm_sd < MAX_SD_PPM,
            "{}",
            fit.ppm_sd
        );
        // A well-measured card close to right is applied too.
        let fit = ClockFit::for_rates(&noisy_log(361, 10.0, 0.3, 0.002, 0, 0.0, 10)).unwrap();
        assert!(fit.corrects(), "{:?}", fit.withheld);
    }

    /// `stretches` stretches of `per` entries 10 s apart with `jitter_s`
    /// of timestamp noise from a card that is right, each after the first
    /// following a pause of `pause_s` (audio runs straight on).
    fn paused_log(
        stretches: usize,
        per: usize,
        jitter_s: f64,
        pause_s: f64,
        seed: u64,
    ) -> Vec<(f64, f64)> {
        let mut rng = Rng::new(seed);
        (0..stretches * per)
            .map(|i| {
                let audio = i as f64 * 10.0;
                let gap = (i / per) as f64 * pause_s;
                (audio, 1.7e9 + audio + gap + jitter_s * rng.normal())
            })
            .collect()
    }

    #[test]
    fn pauses_widen_the_standard_error() {
        // A Positions Test's log: six 2 min stretches, an entry every 10 s
        // with 2 ms of jitter, pauses between them. Each stretch's offset is
        // fitted, so only the scatter within each stretch measures the
        // card: about 6 ppm, where a log as long without pauses measures it
        // to about 1 ppm. Over 300 logs from a card that is right, the
        // reported standard error matches the scatter of what was measured.
        for pause_s in [30.0, 600.0] {
            let fits: Vec<ClockFit> = (0..300)
                .map(|seed| ClockFit::new(&paused_log(6, 13, 0.002, pause_s, seed)).unwrap())
                .collect();
            assert!(fits.iter().all(|f| f.breaks.len() == 5));
            let n = fits.len() as f64;
            let scatter = (fits.iter().map(|f| f.ppm * f.ppm).sum::<f64>() / n).sqrt();
            let sd = fits.iter().map(|f| f.ppm_sd).sum::<f64>() / n;
            assert!(
                (scatter - 6.0).abs() < 1.0,
                "{pause_s} s pauses: scatter {scatter}"
            );
            assert!(
                (sd / scatter - 1.0).abs() < 0.15,
                "{pause_s} s pauses: SE {sd}, scatter {scatter}"
            );
        }
        let unbroken = ClockFit::new(&paused_log(6, 13, 0.002, 0.0, 1)).unwrap();
        assert!(unbroken.breaks.is_empty());
        assert!(unbroken.ppm_sd < 1.5, "{}", unbroken.ppm_sd);
    }

    #[test]
    fn a_withheld_correction_keeps_the_pause() {
        // 15 noisy entries with a 30 s pause after the 8th: no correction,
        // but the audio after the pause still maps 30 s later.
        let pairs = noisy_log(15, 10.0, 6.0, 0.027, 7, 30.0, 11);
        let mut fit = ClockFit::for_rates(&pairs).unwrap();
        assert!(!fit.corrects());
        assert_eq!(fit.breaks.len(), 1);
        let across = fit.map(120.0) - fit.map(60.0);
        assert!((across - 90.0).abs() < 0.1, "{across}");
        // Placing a listed break refits: still withheld.
        fit.place_breaks(&[Break {
            audio_s: 30.0,
            gap_s: 0.2,
        }]);
        assert!(!fit.corrects());
        assert_eq!(fit.ppm, 0.0);
        assert!((fit.map(50.0) - fit.map(40.0) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn usb_lines_and_a_good_log_that_disagree_say_so_with_both_figures() {
        // An exact log, an entry a minute for half an hour, with the card
        // 20 ppm slow; USB lines put it 100 ppm off that (another
        // computer's USB clock), or within 5 ppm of it.
        let exact = noisy_log(31, 60.0, 20.0, 0.0, 0, 0.0, 1);
        let rated = |pairs: &[(f64, f64)], usb: f64| {
            let mut fit = ClockFit::for_rates(pairs).unwrap();
            fit.rate_from_usb(usb);
            fit
        };
        let fit = rated(&exact, 120.0);
        let d = fit.sources_disagree().expect("100 ppm apart");
        assert_eq!(d.usb_ppm, 120.0);
        assert!((d.log_ppm - 20.0).abs() < 1e-3, "{}", d.log_ppm);
        assert!(d.log_ppm_sd < 1e-3, "{}", d.log_ppm_sd);
        assert_eq!(
            d.note(),
            "the USB lines measure the sound card at +120.0 ppm and the clock log at +20 ± 0 ppm; the rates are corrected by the USB lines"
        );
        // The correction is the USB lines' all the same.
        assert_eq!(fit.ppm, 120.0);
        assert!(fit.corrects());
        // Lines and log that agree, to within 5 ppm: nothing said.
        assert_eq!(rated(&exact, 24.0).sources_disagree(), None);
        assert_eq!(rated(&exact, 16.0).sources_disagree(), None);
        // A log that would not correct on its own says nothing either way:
        // too few entries, or too unsure.
        assert_eq!(rated(&exact[..9], 120.0).sources_disagree(), None);
        let unsure = noisy_log(15, 10.0, 6.0, 0.027, 0, 0.0, 8);
        assert!(!ClockFit::for_rates(&unsure).unwrap().corrects());
        assert_eq!(rated(&unsure, 120.0).sources_disagree(), None);
        // A half-hour log an entry a minute with 34 ms of jitter measures
        // the card to about 13 ppm: USB lines 16 ppm off it are within its
        // scatter, 40 ppm off are not.
        let half_hour = noisy_log(30, 60.0, 22.0, 0.034, 0, 0.0, 12);
        let log = ClockFit::for_rates(&half_hour).unwrap();
        assert!(log.corrects() && log.ppm_sd > 8.0, "{}", log.ppm_sd);
        assert_eq!(rated(&half_hour, log.ppm + 16.0).sources_disagree(), None);
        assert!(rated(&half_hour, log.ppm + 40.0)
            .sources_disagree()
            .is_some());
        // Nor does a log that corrects, with no USB lines.
        assert_eq!(
            ClockFit::for_rates(&exact).unwrap().sources_disagree(),
            None
        );
    }

    #[test]
    fn usb_lines_set_the_rate_and_the_log_keeps_its_pause() {
        // A withheld log (15 noisy entries, a 30 s pause after the 8th):
        // the USB lines' 19.5 ppm sets the rate, the pause stays.
        let pairs = noisy_log(15, 10.0, 6.0, 0.027, 7, 30.0, 11);
        let mut fit = ClockFit::for_rates(&pairs).unwrap();
        let log = fit.withheld.unwrap();
        fit.rate_from_usb(19.5);
        assert!(fit.corrects());
        assert_eq!(fit.ppm, 19.5);
        assert_eq!(fit.rate_error_s_per_day, rate_error_s_per_day(19.5));
        assert_eq!(fit.breaks.len(), 1);
        let w = fit.withheld.unwrap();
        assert_eq!(w.reason, "usb_lines");
        assert_eq!((w.ppm, w.points), (log.ppm, log.points));
        assert!(
            w.note()
                .ends_with("; not applied, as the recording's USB lines measure it instead"),
            "{}",
            w.note()
        );
        let across = fit.map(120.0) - fit.map(60.0);
        assert!((across - (90.0 + 60.0 * 19.5e-6)).abs() < 0.1, "{across}");
        assert!((fit.map(50.0) - fit.map(40.0) - 10.0 * (1.0 + 19.5e-6)).abs() < 1e-9);
        // A refit for a listed break keeps the USB rate.
        fit.place_breaks(&[Break {
            audio_s: 30.0,
            gap_s: 0.2,
        }]);
        assert!(fit.corrects());
        assert_eq!(fit.ppm, 19.5);
        assert_eq!(fit.withheld.unwrap().reason, "usb_lines");
        assert!((fit.map(50.0) - fit.map(40.0) - 10.0 * (1.0 + 19.5e-6)).abs() < 1e-9);

        // A log that would correct on its own gives way too: an hour
        // measured at 6 ppm, the lines at 19.5.
        let mut fit = ClockFit::for_rates(&noisy_log(361, 10.0, 6.0, 0.002, 0, 0.0, 9)).unwrap();
        fit.rate_from_usb(19.5);
        assert!(fit.corrects());
        assert_eq!(fit.ppm, 19.5);
        assert!((fit.withheld.unwrap().ppm - 6.0).abs() < 0.5);
        let json = serde_json::to_value(&fit).unwrap();
        assert_eq!(json["withheld"]["reason"], "usb_lines");
    }

    #[test]
    fn follows_drift_on_long_runs() {
        // 24 h with the error drifting from 10 to 30 ppm.
        let pairs: Vec<(f64, f64)> = (0..1440)
            .map(|i| {
                // True time is the integral of 1 + (10 + 20 t / 86400) 1e-6.
                let a = i as f64 * 60.0;
                (a, a + 1e-6 * (10.0 * a + 10.0 * a * a / 86400.0))
            })
            .collect();
        let fit = ClockFit::new(&pairs).unwrap();
        assert!(fit.tracks_drift);
        assert!(fit.residual_ms < 1.0, "{}", fit.residual_ms);
        let want = |a: f64| a + 1e-6 * (10.0 * a + 10.0 * a * a / 86400.0);
        assert!((fit.map(43200.0) - want(43200.0)).abs() < 0.005);
    }

    #[test]
    fn sparse_entries_over_40_hours_keep_one_line() {
        // Entries every 2.5 hours over a 40 h run are too sparse for any
        // local fit (three entries within an hour): the run falls back to
        // the one straight line instead of failing.
        let k = 1.0 + 21.11e-6;
        let pairs: Vec<(f64, f64)> = (0..=16)
            .map(|i| {
                let a = i as f64 * 9000.0;
                (a, a * k)
            })
            .collect();
        let fit = ClockFit::new(&pairs).unwrap();
        assert!(fit.breaks.is_empty());
        assert!((fit.ppm - 21.11).abs() < 1e-6, "{}", fit.ppm);
        for a in [0.0, 8100.0, 72_000.0, 144_000.0, 144_600.0] {
            assert!((fit.map(a) - a * k).abs() < 1e-6, "{a}");
        }
    }

    #[test]
    fn a_fixed_correction_holds_over_40_hours() {
        let d = 40.0 * 3600.0;
        let k = 1.0 + 21.11e-6;
        let fit = ClockFit::fixed(21.11, d);
        assert!(fit.breaks.is_empty() && !fit.tracks_drift);
        assert!((fit.rate_error_s_per_day - 21.11e-6 * 86400.0).abs() < 1e-9);
        for a in [0.0, 8100.0, d / 2.0, d, d + 600.0] {
            assert!((fit.map(a) - a * k).abs() < 1e-6, "{a}");
        }
        // Over two hours it matches the fit of the same three entries.
        let short =
            ClockFit::new(&[(0.0, 0.0), (3600.0, 3600.0 * k), (7200.0, 7200.0 * k)]).unwrap();
        let fixed = ClockFit::fixed(21.11, 7200.0);
        assert!((short.ppm - fixed.ppm).abs() < 1e-6);
        assert!((short.map(5000.0) - fixed.map(5000.0)).abs() < 1e-9);
    }

    #[test]
    fn a_tracker_sees_through_late_blocks() {
        // A card 20 ppm slow delivering 10 ms blocks, each arriving 2 to 40
        // ms late (never early), for 20 minutes.
        let mut rng = Rng::new(7);
        let mut tr = ClockTracker::new(48_000);
        let start = UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000);
        for i in 1..=120_000u64 {
            let audio = i as f64 * 0.01;
            let late = 0.002 + 0.038 * rng.next_f64();
            let at = start + std::time::Duration::from_secs_f64(audio * (1.0 + 20e-6) + late);
            tr.push(480, at);
        }
        let fit = tr.fit().expect("fit");
        assert!((fit.ppm - 20.0).abs() < 0.3, "{} ppm", fit.ppm);
        assert!(fit.ppm_sd < 0.3, "{} ppm sd", fit.ppm_sd);
        assert!((tr.audio_s() - 1200.0).abs() < 1e-9);
    }

    #[test]
    fn parses_frames_and_unix() {
        let text = "# clock log\nunix_s,frames\n1759948294.5,0\n1759948354.5,2880000\n";
        let p = parse_log(text, 48000, 2).unwrap();
        assert_eq!(p, vec![(0.0, 1759948294.5), (60.0, 1759948354.5)]);
    }

    #[test]
    fn guesses_nanoseconds_and_bytes_without_a_header() {
        // System time in ns, then bytes of 16-bit mono at 48 kHz, plus a
        // 44-byte header counted in.
        let text = "1791484354282000000 5760044\n1791484414282100000 11520044\n1791484474282000000 17280044\n";
        let p = parse_log(text, 48000, 2).unwrap();
        assert!((p[1].0 - p[0].0 - 60.0).abs() < 1e-9);
        assert!((p[1].1 - p[0].1 - 60.0001).abs() < 1e-6);
        let fit = ClockFit::new(&p).unwrap();
        assert!(fit.ppm.abs() < 2.0, "{}", fit.ppm);
    }

    #[test]
    fn skips_a_note_line_and_reads_fractional_seconds() {
        // As written by the 2-hour capture's logger.
        let text = "arecord_start_utc 2026-10-08T18:31:34.281846890Z pid 2748966\n\
                    1791484294.286333874 44\n\
                    1791484354.301726276 5760044\n\
                    1791484414.300000000 11520044\n";
        let p = parse_log(text, 48000, 2).unwrap();
        assert_eq!(p.len(), 3);
        assert!((p[1].0 - p[0].0 - 60.0).abs() < 1e-9);
    }

    #[test]
    fn rejects_an_audio_column_of_unknown_unit() {
        let text = "1791484354 1000\n1791484414 2000\n";
        assert!(parse_log(text, 48000, 2).is_err());
    }

    #[test]
    fn refuses_an_audio_time_past_any_recording() {
        // Ten good lines and one whose audio time is 1e20 s: fitted, the
        // drift knots 600 s apart never reached it.
        let mut pairs: Vec<(f64, f64)> = (0..10)
            .map(|i| (i as f64 * 60.0, 1.79e9 + i as f64 * 60.0))
            .collect();
        pairs.push((1e20, 1_790_000_013.0));
        assert!(matches!(
            ClockFit::new(&pairs),
            Err(ClockError::TooLong(s)) if s > MAX_SPAN_S
        ));
        let err = ClockFit::new(&pairs).unwrap_err().to_string();
        assert!(err.contains("31 days"), "{err}");
    }

    #[test]
    fn rejects_a_log_whose_times_run_backwards() {
        let text = "1791484414 5760044\n1791484354 11520044\n";
        let err = parse_log(text, 48000, 2).unwrap_err().to_string();
        assert!(err.contains("times do not increase"), "{err}");
    }

    #[test]
    fn a_pause_is_a_gap_not_a_clock_error() {
        // A card 20 ppm slow, logged every 5 s for 10 min, paused for 60 s
        // after 5 min: audio runs straight on, system time jumps. Fitted
        // across, the jump read as about 646,000 ppm.
        let k = 1.0 + 20e-6;
        let pairs: Vec<(f64, f64)> = (0..=120)
            .map(|i| {
                let a = i as f64 * 5.0;
                (a, 1.79e9 + a * k + if a >= 300.0 { 60.0 } else { 0.0 })
            })
            .collect();
        let fit = ClockFit::new(&pairs).unwrap();
        assert!((fit.ppm - 20.0).abs() < 0.1, "ppm {}", fit.ppm);
        assert_eq!(fit.breaks.len(), 1);
        assert!((fit.breaks[0].audio_s - 300.0).abs() < 1e-9);
        assert!(
            (fit.breaks[0].gap_s - 60.0).abs() < 1e-3,
            "{:?}",
            fit.breaks
        );
        assert!((fit.map(200.0) - 200.0 * k).abs() < 1e-3);
        assert!((fit.map(400.0) - (400.0 * k + 60.0)).abs() < 1e-3);
    }

    #[test]
    fn a_listed_break_moves_the_logs_break_back() {
        // Logged every 10 s as the recorder does: paused at 303 s for 60 s,
        // the first entry after the resume one 1 s block later, at 304 s.
        let mut pairs: Vec<(f64, f64)> = (0..=30)
            .map(|i| (i as f64 * 10.0, 1.79e9 + i as f64 * 10.0))
            .collect();
        pairs.extend((0..30).map(|i| {
            let a = 304.0 + i as f64 * 10.0;
            (a, 1.79e9 + a + 60.0)
        }));
        let mut fit = ClockFit::new(&pairs).unwrap();
        assert!((fit.breaks[0].audio_s - 304.0).abs() < 1e-9);
        assert!((fit.map(303.5) - 303.5).abs() < 1e-3);
        fit.place_breaks(&[
            Break {
                audio_s: 303.0,
                gap_s: 60.0,
            },
            // Too far back to be this break.
            Break {
                audio_s: 100.0,
                gap_s: 5.0,
            },
        ]);
        assert_eq!(fit.breaks.len(), 1);
        assert!((fit.breaks[0].audio_s - 303.0).abs() < 1e-9);
        assert!((fit.map(303.5) - 363.5).abs() < 1e-3, "{}", fit.map(303.5));
        assert!((fit.map(302.5) - 302.5).abs() < 1e-3);
        assert!((fit.map(400.0) - 460.0).abs() < 1e-3);
    }

    #[test]
    fn a_clock_step_after_a_resume_stays_where_the_log_puts_it() {
        // Paused at 300 s for 60 s (the recorder's line at the pause frame,
        // then the resume line 10 ms on), and the system clock set 7 s
        // forward 3 s later: only the pause takes the listed break.
        let mut pairs: Vec<(f64, f64)> = (0..=30)
            .map(|i| (i as f64 * 10.0, 1.79e9 + i as f64 * 10.0))
            .collect();
        pairs.push((300.01, 1.79e9 + 300.01 + 60.0));
        pairs.extend((0..30).map(|i| {
            let a = 303.01 + i as f64 * 10.0;
            (a, 1.79e9 + a + 67.0)
        }));
        let mut fit = ClockFit::new(&pairs).unwrap();
        // The pause's pair puts its break at the pause line; the 7 s step,
        // between lines 3 s apart, stays at the line after it.
        let at: Vec<f64> = fit.breaks.iter().map(|b| b.audio_s).collect();
        assert_eq!(at.len(), 2, "{at:?}");
        assert!((at[0] - 300.0).abs() < 1e-9 && (at[1] - 303.01).abs() < 1e-9);
        fit.place_breaks(&[Break {
            audio_s: 300.0,
            gap_s: 60.0,
        }]);
        assert!(
            (fit.breaks[0].audio_s - 300.0).abs() < 1e-9,
            "{:?}",
            fit.breaks
        );
        assert!(
            (fit.breaks[1].audio_s - 303.01).abs() < 1e-9,
            "{:?}",
            fit.breaks
        );
    }

    #[test]
    fn a_pause_the_log_alone_shows_breaks_at_the_pause_line() {
        // The recorder's pair at a 60 s pause at 300 s, with nothing
        // listed: its line for the last block before the pause, then its
        // line for the first 0.1 s block after the resume. The sound in
        // that block is resumed sound, so it maps after the gap.
        let k = 1.0 + 20e-6;
        let mut pairs: Vec<(f64, f64)> = (0..=30)
            .map(|i| (i as f64 * 10.0, 1.79e9 + i as f64 * 10.0 * k))
            .collect();
        pairs.extend((0..30).map(|i| {
            let a = 300.1 + i as f64 * 10.0;
            (a, 1.79e9 + a * k + 60.0)
        }));
        let fit = ClockFit::new(&pairs).unwrap();
        assert_eq!(fit.breaks.len(), 1, "{:?}", fit.breaks);
        assert!(
            (fit.breaks[0].audio_s - 300.0).abs() < 1e-9,
            "{:?}",
            fit.breaks
        );
        assert!(
            (fit.breaks[0].gap_s - 60.0).abs() < 1e-3,
            "{:?}",
            fit.breaks
        );
        assert!((fit.map(300.05) - (300.05 * k + 60.0)).abs() < 1e-3);
        assert!((fit.map(299.95) - 299.95 * k).abs() < 1e-3);
        assert!((fit.ppm - 20.0).abs() < 0.1, "ppm {}", fit.ppm);
        // Listed at its frame, the same break is matched and stays put.
        let mut placed = fit.clone();
        placed.place_breaks(&[Break {
            audio_s: 300.0,
            gap_s: 60.0,
        }]);
        assert_eq!(placed.breaks, fit.breaks);
    }

    #[test]
    fn a_short_listed_pause_does_not_bend_the_rate() {
        // A card 20 ppm slow, logged every 10 s, stopped and resumed at
        // 300 s with 0.2 s lost: too small a step for the log alone to call
        // a break, so fitted straight through it bends the rate.
        let k = 1.0 + 20e-6;
        let mut pairs: Vec<(f64, f64)> = (0..=30)
            .map(|i| {
                let a = i as f64 * 10.0;
                (a, 1.79e9 + a * k)
            })
            .collect();
        pairs.extend((0..30).map(|i| {
            let a = 300.01 + i as f64 * 10.0;
            (a, 1.79e9 + a * k + 0.2)
        }));
        let mut fit = ClockFit::new(&pairs).unwrap();
        assert!(fit.breaks.is_empty());
        assert!((fit.ppm - 20.0).abs() > 100.0, "ppm {}", fit.ppm);
        fit.place_breaks(&[Break {
            audio_s: 300.0,
            gap_s: 0.2,
        }]);
        assert_eq!(fit.breaks.len(), 1, "{:?}", fit.breaks);
        assert!((fit.breaks[0].audio_s - 300.0).abs() < 1e-9);
        assert!((fit.breaks[0].gap_s - 0.2).abs() < 1e-3, "{:?}", fit.breaks);
        assert!((fit.ppm - 20.0).abs() < 0.1, "ppm {}", fit.ppm);
        assert!((fit.map(200.0) - 200.0 * k).abs() < 1e-3);
        assert!((fit.map(400.0) - (400.0 * k + 0.2)).abs() < 1e-3);
        // A second placing finds nothing more to do.
        let again = fit.clone();
        fit.place_breaks(&[Break {
            audio_s: 300.0,
            gap_s: 0.2,
        }]);
        assert_eq!(fit.breaks, again.breaks);
    }

    #[test]
    fn a_short_listed_pause_adds_no_other_break() {
        // As above, after a 60 s pause at 150 s that the log shows (the
        // line at the pause frame, then the resume line 10 ms on): the fit
        // made again keeps that break and adds only the short one.
        let k = 1.0 + 20e-6;
        let line = |a: f64, off: f64| (a, 1.79e9 + a * k + off);
        let mut pairs: Vec<(f64, f64)> = (0..=15).map(|i| line(i as f64 * 10.0, 0.0)).collect();
        pairs.extend((0..15).map(|i| line(150.01 + i as f64 * 10.0, 60.0)));
        pairs.push(line(300.0, 60.0));
        pairs.extend((0..30).map(|i| line(300.01 + i as f64 * 10.0, 60.2)));
        let mut fit = ClockFit::new(&pairs).unwrap();
        assert_eq!(fit.breaks.len(), 1, "{:?}", fit.breaks);
        fit.place_breaks(&[
            Break {
                audio_s: 150.0,
                gap_s: 60.0,
            },
            Break {
                audio_s: 300.0,
                gap_s: 0.2,
            },
        ]);
        let at: Vec<f64> = fit.breaks.iter().map(|b| b.audio_s).collect();
        assert_eq!(at, [150.0, 300.0], "{:?}", fit.breaks);
        assert!((fit.breaks[1].gap_s - 0.2).abs() < 1e-3, "{:?}", fit.breaks);
        assert!((fit.ppm - 20.0).abs() < 0.1, "ppm {}", fit.ppm);
    }

    #[test]
    fn a_forced_break_never_runs_true_time_back() {
        // A near-instant stop and resume listed at 300 s, after which the
        // resume line and the 29 lines after it are all stamped 3 ms early:
        // the break forced there takes no gap below zero, the map never
        // steps back, and the rate is fitted without the step.
        let k = 1.0 + 20e-6;
        let line = |a: f64, off: f64| (a, 1.79e9 + a * k + off);
        let mut pairs: Vec<(f64, f64)> = (0..=30).map(|i| line(i as f64 * 10.0, 0.0)).collect();
        pairs.extend((0..30).map(|i| line(300.01 + i as f64 * 10.0, -0.003)));
        let mut fit = ClockFit::new(&pairs).unwrap();
        fit.place_breaks(&[Break {
            audio_s: 300.0,
            gap_s: 0.0,
        }]);
        assert_eq!(fit.breaks.len(), 1, "{:?}", fit.breaks);
        assert!(fit.breaks[0].gap_s >= 0.0, "{:?}", fit.breaks);
        assert!(fit.map(300.0) >= fit.map(300.0 - 1e-6), "{:?}", fit.breaks);
        assert!((fit.ppm - 20.0).abs() < 0.5, "ppm {}", fit.ppm);
    }

    #[test]
    fn a_late_timestamp_is_not_a_pause() {
        let mut pairs: Vec<(f64, f64)> = (0..=60).map(|i| (i as f64, 1.79e9 + i as f64)).collect();
        pairs[30].1 += 0.8;
        let fit = ClockFit::new(&pairs).unwrap();
        assert!(fit.breaks.is_empty(), "{:?}", fit.breaks);
        assert!(fit.ppm.abs() < 1.0, "ppm {}", fit.ppm);
    }
}
