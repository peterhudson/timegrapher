//! Possible escapement faults, read from the shape of the Tick and Tock.
//!
//! Each rule looks at one measure from [`crate::shape`] on each side and
//! names the fault in the words of `docs/fault-signatures.md`. No recording
//! of a watch with a confirmed fault has passed through these rules yet, so
//! every finding is [`Strength::Possible`], and each limit sits well outside
//! what the 16 healthy takes of the public corpus measure (see the note on
//! each constant). Findings are only made when a watch is heard, no beats
//! are clipped (clipping flattens the drop every level is measured
//! against), and the amplitude and beat error are steady enough to grade
//! Fair from their spread alone: the shape is read from the same templates,
//! while the lift angle and the sound card's clock do not touch it. A beat
//! error with no spread at all is let through, since a side whose unlock
//! is lost (a fault this module looks for) has no unlock beat error.

use crate::confidence::{self, Confidence, Doubts};
use crate::shape::{ShapeReport, SideReport};
use serde::Serialize;
use std::collections::BTreeMap;

/// Which beat a finding is on. The Tick is the even beat, as in
/// [`crate::profile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Tick,
    Tock,
    Both,
}

impl Side {
    pub fn word(self) -> &'static str {
        match self {
            Side::Tick => "Tick",
            Side::Tock => "Tock",
            Side::Both => "Tick and Tock",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Strength {
    /// The measure is well outside healthy watches; worth checking.
    Possible,
    /// As possible, and the rule has been confirmed on a real fault.
    Likely,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    /// Stable name for programs, such as `unlock_too_strong`.
    pub id: &'static str,
    /// The fault in a watchmaker's words.
    pub label: &'static str,
    pub side: Side,
    pub strength: Strength,
    /// The measures behind the finding, by name.
    pub evidence: BTreeMap<String, f64>,
    /// Where the fault is described.
    pub see: &'static str,
}

const SEE: &str = "docs/fault-signatures.md#scope-shape-of-one-beat";

/// Fewest 2 s windows measured on a side before it is judged.
pub const MIN_WINDOWS: usize = 5;
/// Sound 1 at least this loud against sound 3. The corpus reaches 0.78
/// (the Patek 324 clone's Tick).
pub const UNLOCK_RATIO: f64 = 1.0;
/// Longer 1-to-2 interval over the shorter, Tick against Tock. The corpus
/// reaches 1.46 per file (the DD 3235), and a plastic holder alone moved
/// the ETA 2824 from 1.03 to 1.40, so only a large difference counts.
/// An extra sound shortly before the unlock also lands here: it is taken
/// for sound 1, which then sits far ahead of sound 2 (see the synthetic
/// tests), so the label names both causes.
pub const UNLOCK_UNEVEN_RATIO: f64 = 1.8;
/// Sound 2 separated from sound 1 in fewer than this share of windows.
/// The corpus never falls below 0.94.
pub const MIN_SHARE_WITH_2: f64 = 0.5;
/// Sound 1 found in fewer than this share of windows. The corpus never
/// falls below 0.65 (the Reverso's quiet unlock).
pub const MIN_SHARE_WITH_1: f64 = 0.3;
/// With no sound 2, a 1-to-3 interval below this share of the other
/// side's means the impulse was taken for the unlock: there is no distinct
/// unlock. The corpus's two sides differ by at most about 10%.
pub const SHORT_1_TO_3: f64 = 0.75;
/// Mean level 3 to 15 ms after the drop against the drop, one side over
/// the other, and the louder side's least level. Case and holder ringing
/// lengthen both sides' tails (up to 0.33 on the ETA 2824 in its holder),
/// so only a tail on one side counts; the corpus reaches a ratio of about
/// 1.7.
pub const TAIL_SIDE_RATIO: f64 = 3.0;
pub const MIN_TAIL: f64 = 0.1;

fn share(n: usize, of: usize) -> f64 {
    n as f64 / of.max(1) as f64
}

fn finding(
    id: &'static str,
    label: &'static str,
    side: Side,
    evidence: &[(&'static str, f64)],
) -> Finding {
    Finding {
        id,
        label,
        side,
        strength: Strength::Possible,
        evidence: evidence.iter().map(|&(k, v)| (k.to_string(), v)).collect(),
        see: SEE,
    }
}

/// One side's findings.
fn side_findings(r: &SideReport, other: &SideReport, side: Side, out: &mut Vec<Finding>) {
    let m = r.windows_measured;
    let w = &r.windows;
    let with1 = share(r.windows_with_1, m);
    let with2 = share(r.windows_with_2, m);
    if with1 < MIN_SHARE_WITH_1 {
        out.push(finding(
            "no_unlock_sound",
            "No distinct unlock: an escape tooth may land on the impulse plane",
            side,
            &[("share_with_sound_1", with1)],
        ));
    } else if with2 < MIN_SHARE_WITH_2 {
        let short = w
            .i13_ms
            .zip(other.windows.i13_ms)
            .filter(|&(a, b)| a < SHORT_1_TO_3 * b);
        if let Some((a, b)) = short {
            out.push(finding(
                "no_unlock_sound",
                "No distinct unlock: an escape tooth may land on the impulse plane",
                side,
                &[
                    ("share_with_sound_2", with2),
                    ("1_to_3_ms", a),
                    ("other_side_1_to_3_ms", b),
                ],
            ));
            return;
        }
        out.push(finding(
            "escapement_friction",
            "Unlock and impulse not separable: extra friction in the escapement",
            side,
            &[("share_with_sound_2", with2)],
        ));
    }
    if let Some(r13) = w.ratio13.filter(|&v| v >= UNLOCK_RATIO) {
        out.push(finding(
            "unlock_too_strong",
            "Unlocking too strong",
            side,
            &[("ratio_1_to_3", r13)],
        ));
    }
}

/// The larger of two measures over the smaller, and the side of the larger.
fn uneven(tick: f64, tock: f64) -> Option<(f64, Side)> {
    let lo = tick.min(tock);
    (lo > 0.0).then(|| {
        (
            tick.max(tock) / lo,
            if tick > tock { Side::Tick } else { Side::Tock },
        )
    })
}

/// Whether the shape can be judged at all (see the module notes).
pub fn judged(confidence: Option<&Confidence>, doubts: Doubts) -> bool {
    let within = |se: Option<f64>, limit: f64| se.is_some_and(|v| v <= limit);
    confidence.is_some_and(|c| {
        !doubts.clipped
            && within(c.amplitude.se, confidence::AMPLITUDE_SE.1)
            && !matches!(c.beat_error.se, Some(v) if v > confidence::BEAT_ERROR_SE.1)
    })
}

/// Findings from the shape of both sides, or none when the readings are
/// too poor to judge.
pub fn from_shape(
    r: &ShapeReport,
    confidence: Option<&Confidence>,
    doubts: Doubts,
) -> Vec<Finding> {
    if !judged(confidence, doubts) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let sides = [(&r.even, Side::Tick), (&r.odd, Side::Tock)];
    for (k, (s, side)) in sides.iter().enumerate() {
        if s.windows_measured >= MIN_WINDOWS {
            side_findings(s, sides[1 - k].0, *side, &mut out);
        }
    }
    if sides.iter().all(|(s, _)| s.windows_measured >= MIN_WINDOWS) {
        let (t, k) = (&r.even.windows, &r.odd.windows);
        if let Some((ratio, side)) = t.i12_ms.zip(k.i12_ms).and_then(|(a, b)| uneven(a, b)) {
            if ratio >= UNLOCK_UNEVEN_RATIO {
                out.push(finding(
                    "unlock_uneven",
                    "Unlock far earlier on one side: uneven lock, or an extra sound before the unlock (safety pin or impulse pin)",
                    side,
                    &[
                        ("tick_1_to_2_ms", t.i12_ms.unwrap_or(f64::NAN)),
                        ("tock_1_to_2_ms", k.i12_ms.unwrap_or(f64::NAN)),
                        ("ratio", ratio),
                    ],
                ));
            }
        }
        if let Some((ratio, side)) = uneven(t.tail_ratio, k.tail_ratio) {
            if ratio >= TAIL_SIDE_RATIO && t.tail_ratio.max(k.tail_ratio) >= MIN_TAIL {
                out.push(finding(
                    "rubbing_after_drop",
                    "Noise after the drop on one side: balance or hairspring grazing",
                    side,
                    &[
                        ("tick_tail_ratio", t.tail_ratio),
                        ("tock_tail_ratio", k.tail_ratio),
                        ("ratio", ratio),
                    ],
                ));
            }
        }
    }
    // A fault seen on both sides is one finding, its evidence named by side.
    let mut merged: Vec<Finding> = Vec::new();
    for f in out {
        if let Some(g) = merged
            .iter_mut()
            .find(|g| g.id == f.id && g.side == Side::Tick)
        {
            g.side = Side::Both;
            let tick = std::mem::take(&mut g.evidence);
            g.evidence
                .extend(tick.into_iter().map(|(k, v)| (format!("tick_{k}"), v)));
            g.evidence.extend(
                f.evidence
                    .into_iter()
                    .map(|(k, v)| (format!("tock_{k}"), v)),
            );
        } else {
            merged.push(f);
        }
    }
    merged
}
