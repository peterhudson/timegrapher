//! Which beats, stretches and bins a reading may use: the one place the
//! cuts in a take are decided.
//!
//! A take is cut wherever its beats on either side can't be read as one
//! run: a pause in listening and the gap it leaves in true time, a jump in
//! the beats, a stretch masked by hand, and a step in the clock log that no
//! listed pause explains. [`Cuts`] holds every one of them as a hole on
//! true time and answers, for every reading:
//!
//! - which beats count ([`Cuts::beats`]): those outside every mask, on true
//!   time;
//! - where the whole stretches are ([`Cuts::piece_cuts`]), each fitted on
//!   its own and combined by [`crate::timing::fit_pieces`];
//! - whether a bin is whole ([`Cuts::whole`], [`Cuts::bins`]): it reaches
//!   into no hole, even at its edge, and each of its parts holds a beat;
//! - which amplitude windows count ([`Cuts::amplitude_windows`]).
//!
//! The rule: no reading combines beats from both sides of a cut, and a bin
//! that only partly overlaps one is left out, never filled in from what is
//! left.

use crate::amplitude::AmplitudeWindow;
use crate::beats::Beat;
use crate::breaks::{Break, JUMP_MATCH_S};
use crate::clock::ClockFit;
use crate::timing::WindowFit;
use serde::Serialize;

/// Seconds a bin or a hole's edge may be off by and still count as
/// meeting it rather than reaching across.
const EDGE_S: f64 = 1e-6;

/// What cut the take at a hole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HoleKind {
    /// A pause in listening: from the last sound before it to the first
    /// after it. A break with no known length (a jump in the beats, a new
    /// position) is a hole of no width.
    Gap,
    /// A stretch masked by hand.
    Mask,
    /// A step in the clock log that no listed break explains.
    ClockStep,
}

/// One hole in a take, on true time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Hole {
    pub start_s: f64,
    pub end_s: f64,
    pub kind: HoleKind,
}

/// One bin of [`Cuts::bins`], on true time, and whether it is whole.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Bin {
    pub start_s: f64,
    pub end_s: f64,
    pub whole: bool,
    /// Beats in the bin, from its start up to its end.
    pub beats: usize,
}

/// Every cut in a take, and the answers that follow from them.
#[derive(Debug, Clone)]
pub struct Cuts<'a> {
    clock: Option<&'a ClockFit>,
    /// The breaks, with a negative gap taken as none.
    breaks: Vec<Break>,
    /// Masked stretches, audio seconds, cleaned.
    masked: Vec<(f64, f64)>,
    /// On true time, sorted by start.
    holes: Vec<Hole>,
}

impl<'a> Cuts<'a> {
    /// A take with no cuts.
    pub fn none() -> Cuts<'static> {
        Cuts {
            clock: None,
            breaks: Vec::new(),
            masked: Vec::new(),
            holes: Vec::new(),
        }
    }

    /// The cuts of a take: its clock fit (if it has a clock log), its
    /// breaks (listed pauses, jumps in the beats, new positions; see
    /// [`crate::breaks`]) and its masked stretches (audio seconds, any
    /// order). A break with a negative gap (the recorder stamped the first
    /// sound after a pause before the last one before it) has none, since
    /// true time does not run back.
    pub fn new(clock: Option<&'a ClockFit>, breaks: &[Break], masked: &[(f64, f64)]) -> Self {
        let mut breaks: Vec<Break> = breaks
            .iter()
            .filter(|b| b.audio_s.is_finite())
            .map(|b| Break {
                gap_s: if b.gap_s.is_finite() {
                    b.gap_s.max(0.0)
                } else {
                    0.0
                },
                ..*b
            })
            .collect();
        breaks.sort_by(|a, b| a.audio_s.total_cmp(&b.audio_s));
        let pairs: Vec<[f64; 2]> = masked.iter().map(|&(a, b)| [a, b]).collect();
        let masked = crate::session::masked_spans(&pairs);
        let mut cuts = Cuts {
            clock,
            breaks,
            masked,
            holes: Vec::new(),
        };
        cuts.holes = cuts.find_holes();
        cuts
    }

    fn find_holes(&self) -> Vec<Hole> {
        let clock_breaks = self.clock.map_or(&[][..], |c| &c.breaks[..]);
        let mut holes: Vec<Hole> = self
            .breaks
            .iter()
            .map(|b| {
                if b.gap_s <= 0.0 {
                    let p = self.true_time(b.audio_s);
                    return (p, p);
                }
                // From the last sound before the pause to the first after
                // it. A clock fit whose break was not moved to the listed
                // one ([`ClockFit::place_breaks`]) puts it at the log's
                // first line after the resume, so the sound between the two
                // lies in the gap.
                let start = self.true_time(b.audio_s - EDGE_S);
                let resume = clock_breaks
                    .iter()
                    .map(|c| c.audio_s)
                    .find(|&c| c >= b.audio_s && c - b.audio_s <= JUMP_MATCH_S)
                    .unwrap_or(b.audio_s);
                (start, self.true_time(resume).max(start))
            })
            .map(|(start_s, end_s)| Hole {
                start_s,
                end_s,
                kind: HoleKind::Gap,
            })
            .collect();
        holes.extend(
            clock_breaks
                .iter()
                // Explained: a listed break at the same second, or one
                // whose gap runs to it (above).
                .filter(|c| {
                    !self.breaks.iter().any(|b| {
                        (b.audio_s - c.audio_s).abs() < 1e-9
                            || (b.gap_s > 0.0
                                && c.audio_s >= b.audio_s
                                && c.audio_s - b.audio_s <= JUMP_MATCH_S)
                    })
                })
                .map(|c| Hole {
                    start_s: self.true_time(c.audio_s - EDGE_S),
                    end_s: self.true_time(c.audio_s),
                    kind: HoleKind::ClockStep,
                }),
        );
        holes.extend(self.masked.iter().map(|&(a, b)| Hole {
            start_s: self.true_time(a),
            end_s: self.true_time(b),
            kind: HoleKind::Mask,
        }));
        holes.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
        holes
    }

    /// True seconds from the start of the take for audio second `t`: the
    /// clock fit's map, with the gaps in. Without a clock fit, or with one
    /// that saw no pause, each break's gap is added after it.
    pub fn true_time(&self, t: f64) -> f64 {
        let base = self.clock.map_or(t, |c| c.map(t));
        if self.clock.is_some_and(|c| !c.breaks.is_empty()) {
            base
        } else {
            base + self
                .breaks
                .iter()
                .filter(|b| b.audio_s <= t)
                .map(|b| b.gap_s)
                .sum::<f64>()
        }
    }

    /// The breaks, sorted, a negative gap taken as none.
    pub fn breaks(&self) -> &[Break] {
        &self.breaks
    }

    /// The masked stretches, audio seconds, cleaned and joined.
    pub fn masked(&self) -> &[(f64, f64)] {
        &self.masked
    }

    /// Every hole, on true time, sorted by start.
    pub fn holes(&self) -> &[Hole] {
        &self.holes
    }

    /// Whether audio second `t` falls in a masked stretch, ends included.
    pub fn is_masked(&self, t: f64) -> bool {
        crate::session::in_mask(&self.masked, t)
    }

    /// The beats a reading may use: those outside every masked stretch,
    /// their times moved onto true time.
    pub fn beats(&self, beats: &[Beat]) -> Vec<Beat> {
        beats
            .iter()
            .filter(|b| !self.is_masked(b.time))
            .map(|b| Beat {
                time: self.true_time(b.time),
                ..*b
            })
            .collect()
    }

    /// Where the whole stretches are cut, on true time, for
    /// [`crate::timing::fit_pieces`], [`crate::timing::sign_turns`] and
    /// [`crate::timing::residuals_pieces`]: each stretch ends where a
    /// hole starts and the next starts where it ends, so beats inside a
    /// hole belong to neither. A hole of no width is one cut.
    pub fn piece_cuts(&self) -> Vec<f64> {
        let mut cuts: Vec<f64> = self
            .holes
            .iter()
            .flat_map(|h| {
                if h.end_s > h.start_s + 2.0 * EDGE_S {
                    vec![h.start_s - EDGE_S, h.end_s - EDGE_S]
                } else {
                    vec![h.start_s - EDGE_S]
                }
            })
            .collect();
        cuts.sort_by(f64::total_cmp);
        cuts
    }

    /// Whether a stretch from `start` to `end` on true time reaches into,
    /// or across, no hole. Meeting a hole's edge is not reaching into it.
    pub fn clear(&self, start: f64, end: f64) -> bool {
        !self
            .holes
            .iter()
            .any(|h| start < h.end_s - EDGE_S && end > h.start_s + EDGE_S)
    }

    /// Whether a bin from `start` to `end` on true time is whole: it is
    /// [`clear`](Self::clear) of every hole, and each of its stretches of
    /// about `part` seconds holds one of `beats` (on true time, sorted), so
    /// a dropout that no break lists leaves it out too.
    pub fn whole(&self, beats: &[Beat], start: f64, end: f64, part: f64) -> bool {
        self.clear(start, end) && covered(beats, start, end, part)
    }

    /// `n` bins of `len` seconds on true time, laid end to end from `t0`,
    /// each marked whole or not as [`whole`](Self::whole) says. A bin
    /// that is not whole is left out of a reading, never filled in.
    pub fn bins(&self, beats: &[Beat], t0: f64, len: f64, n: usize, part: f64) -> Vec<Bin> {
        (0..n)
            .map(|i| {
                let start_s = t0 + i as f64 * len;
                let end_s = start_s + len;
                let lo = beats.partition_point(|b| b.time < start_s);
                let hi = beats.partition_point(|b| b.time < end_s);
                Bin {
                    start_s,
                    end_s,
                    whole: self.whole(beats, start_s, end_s, part),
                    beats: hi - lo,
                }
            })
            .collect()
    }

    /// Fits over whole bins of `len` seconds, one every `step` seconds
    /// from the first beat at or after `from` up to `to` (true time). A
    /// bin is fitted on the beats from its start up to its end when it is
    /// [`whole`](Self::whole) with parts of `part` seconds, so none spans
    /// a hole or a dropout, and none needs a beat at or past its end.
    /// `beats` on true time, sorted.
    #[allow(clippy::too_many_arguments)]
    pub fn windows(
        &self,
        beats: &[Beat],
        bph: u32,
        from: f64,
        to: f64,
        len: f64,
        step: f64,
        part: f64,
    ) -> Vec<WindowFit> {
        let mut out = Vec::new();
        let Some(first) = beats.get(beats.partition_point(|x| x.time < from)) else {
            return out;
        };
        let mut s = first.time;
        while s + len <= to + 1e-9 {
            if self.whole(beats, s, s + len, part) {
                let lo = beats.partition_point(|x| x.time < s);
                let hi = beats.partition_point(|x| x.time < s + len);
                if let Some(fit) = crate::timing::fit(&beats[lo..hi], bph) {
                    out.push(WindowFit {
                        start_s: s,
                        end_s: s + len,
                        fit,
                    });
                }
            }
            s += step;
        }
        out
    }

    /// The amplitude windows a reading may use: those whose span on true
    /// time is [`clear`](Self::clear) of every hole.
    pub fn amplitude_windows<'w>(
        &self,
        windows: &'w [AmplitudeWindow],
    ) -> Vec<&'w AmplitudeWindow> {
        windows
            .iter()
            .filter(|w| self.clear(self.true_time(w.start_s), self.true_time(w.end_s)))
            .collect()
    }
}

/// Whether every stretch of about `part` seconds from `start` to `end`
/// holds a beat (`beats` sorted by time): the span is split into as many
/// equal parts as `part` fits, at least one.
pub fn covered(beats: &[Beat], start: f64, end: f64, part: f64) -> bool {
    let k = ((end - start) / part).round().max(1.0) as usize;
    let len = (end - start) / k as f64;
    (0..k).all(|j| {
        let a = start + j as f64 * len;
        let i = beats.partition_point(|b| b.time < a);
        beats.get(i).is_some_and(|b| b.time < a + len)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(start_s: f64, end_s: f64) -> AmplitudeWindow {
        AmplitudeWindow {
            start_s,
            end_s,
            even_deg: Some(280.0),
            odd_deg: Some(280.0),
            beat_error_ms: None,
            beat_error_unlock_ms: None,
            even_unlock_ms: None,
            odd_unlock_ms: None,
            even_drop_ms: None,
            odd_drop_ms: None,
        }
    }

    fn brk(audio_s: f64, gap_s: f64) -> Break {
        Break { audio_s, gap_s }
    }

    /// Beats every 0.2 s from 0 to `to` audio seconds.
    fn beats_to(to: f64) -> Vec<Beat> {
        (0..)
            .map(|i| Beat {
                index: i,
                time: i as f64 * 0.2,
                quality: 1.0,
            })
            .take_while(|b| b.time < to)
            .collect()
    }

    /// A clock log every 10 s with a 30 s pause after 305 s of audio and
    /// no line at the pause, so the clock fit's break is at 305.1 s.
    fn paused_clock() -> ClockFit {
        let k = 1.0 + 20e-6;
        let mut pairs: Vec<(f64, f64)> = (0..=30)
            .map(|i| (i as f64 * 10.0, 1.79e9 + i as f64 * 10.0 * k))
            .collect();
        pairs.extend((0..30).map(|i| {
            let t = 305.1 + i as f64 * 10.0;
            (t, 1.79e9 + t * k + 30.0)
        }));
        ClockFit::new(&pairs).unwrap()
    }

    #[test]
    fn gaps_are_from_the_last_sound_to_the_first_after() {
        let held = |breaks: &[Break]| -> Vec<(f64, f64)> {
            Cuts::new(None, breaks, &[])
                .holes()
                .iter()
                .map(|h| (h.start_s, h.end_s))
                .collect()
        };
        assert!(held(&[]).is_empty());
        // A jump, or a gap under 0 s, is a point.
        assert_eq!(held(&[brk(100.0, 0.0)]), [(100.0, 100.0)]);
        assert_eq!(held(&[brk(100.0, -0.2)]), [(100.0, 100.0)]);
        // A pause runs from just before its audio second to 30 s later on
        // true time, and moves the jump after it on by its gap.
        let h = held(&[brk(100.0, 30.0), brk(400.0, 0.0)]);
        assert_eq!(h.len(), 2);
        assert!(
            (h[0].0 - (100.0 - EDGE_S)).abs() < 1e-9 && h[0].1 == 130.0,
            "{h:?}"
        );
        assert_eq!(h[1], (430.0, 430.0));
        let cuts = Cuts::new(None, &[brk(100.0, 30.0)], &[]);
        assert_eq!(cuts.true_time(99.0), 99.0);
        assert_eq!(cuts.true_time(100.0), 130.0);
        assert!(cuts.clear(90.0, 100.0 - EDGE_S) && cuts.clear(130.0, 140.0));
        assert!(!cuts.clear(95.0, 105.0) && !cuts.clear(125.0, 135.0));
        // With a clock fit whose break sits at the log's first line after
        // the resume (305.1 s), the gap runs from the last sound before the
        // listed pause to there; moved to the listed break, to the resume.
        let clock = paused_clock();
        let listed = [brk(305.0, 30.0)];
        let cuts = Cuts::new(Some(&clock), &listed, &[]);
        assert_eq!(cuts.holes().len(), 1, "{:?}", cuts.holes());
        let h = cuts.holes()[0];
        assert!(
            (h.start_s - clock.map(305.0 - EDGE_S)).abs() < 1e-9,
            "{h:?}"
        );
        assert!((h.end_s - clock.map(305.1)).abs() < 1e-9, "{h:?}");
        assert!((h.end_s - h.start_s - 30.1).abs() < 0.01, "{h:?}");
        assert!(!cuts.clear(clock.map(305.05), clock.map(305.05) + 10.0));
        let mut placed = clock.clone();
        placed.place_breaks(&listed);
        let cuts = Cuts::new(Some(&placed), &listed, &[]);
        let h = cuts.holes()[0];
        assert!((h.end_s - placed.map(305.0)).abs() < 1e-9, "{h:?}");
        assert!(cuts.true_time(305.05) > h.end_s);
    }

    #[test]
    fn covered_needs_a_beat_in_every_part() {
        // Beats every 0.2 s with none from 52 s to 55 s.
        let beats: Vec<Beat> = beats_to(100.0)
            .into_iter()
            .filter(|b| !(52.0..55.0).contains(&b.time))
            .collect();
        assert!(covered(&beats, 40.0, 50.0, 2.0));
        assert!(!covered(&beats, 50.0, 60.0, 2.0));
        // Parts of 5 s each hold a beat; the hole is under one part.
        assert!(covered(&beats, 50.0, 60.0, 5.0));
        // A span not a whole number of parts is split into equal ones:
        // 11 s at about 2 s: six parts of 1.83 s, the second empty.
        assert!(!covered(&beats, 51.0, 62.0, 2.0));
        // Past the last beat, or with no beats, nothing is covered.
        assert!(!covered(&beats, 95.0, 105.0, 2.0));
        assert!(!covered(&[], 0.0, 10.0, 2.0));
        // A beat on a part's start counts; one on its end is the next's.
        let two = [
            Beat {
                index: 0,
                time: 0.0,
                quality: 1.0,
            },
            Beat {
                index: 1,
                time: 2.0,
                quality: 1.0,
            },
        ];
        assert!(covered(&two, 0.0, 4.0, 2.0));
        assert!(!covered(&two, -2.0, 2.0, 2.0));
    }

    #[test]
    fn no_bin_reaches_into_a_hole() {
        // A 30 s pause after 100 s, a jump at 200 s, and a mask from 250 s
        // to 262 s: no 10 s bin on true time that reaches into any of them
        // is whole, nor one that holds beats from both sides.
        let breaks = [brk(100.0, 30.0), brk(200.0, 0.0)];
        let masked = [(250.0, 262.0)];
        let cuts = Cuts::new(None, &breaks, &masked);
        let beats = cuts.beats(&beats_to(400.0));
        let kinds: Vec<HoleKind> = cuts.holes().iter().map(|h| h.kind).collect();
        assert_eq!(kinds, [HoleKind::Gap, HoleKind::Gap, HoleKind::Mask]);
        let bins = cuts.bins(&beats, 0.0, 10.0, 44, 2.0);
        let whole: Vec<f64> = bins.iter().filter(|b| b.whole).map(|b| b.start_s).collect();
        // The pause is 100 to 130 s on true time, the jump at 230 s and
        // the mask 280 to 292 s.
        for b in bins.iter().filter(|b| b.whole) {
            for (a, z) in [(100.0, 130.0), (230.0, 230.0), (280.0, 292.0)] {
                assert!(
                    b.end_s <= a + EDGE_S || b.start_s >= z - EDGE_S,
                    "{b:?} reaches into {a}..{z}"
                );
            }
        }
        // A bin ending at a hole, or starting where one ends, is whole.
        assert!(whole.contains(&90.0) && whole.contains(&130.0));
        assert!(whole.contains(&220.0) && whole.contains(&230.0));
        // A bin ending where the mask starts is whole; those holding its
        // inside or its end are not.
        assert!(whole.contains(&270.0));
        assert!(!whole.contains(&280.0) && !whole.contains(&290.0));
        assert!(whole.contains(&300.0));
        // Past the last beat (430 s on true time) nothing is whole.
        assert!(whole.contains(&420.0) && !whole.contains(&430.0));
    }

    #[test]
    fn a_dropout_no_break_lists_leaves_its_bins_out() {
        // 3 s with no beats at 52 s: the bin holding it is not whole, its
        // neighbours are, and nothing is filled in.
        let cuts = Cuts::none();
        let beats: Vec<Beat> = beats_to(100.0)
            .into_iter()
            .filter(|b| !(52.0..55.0).contains(&b.time))
            .collect();
        let bins = cuts.bins(&beats, 0.0, 10.0, 10, 2.0);
        let out: Vec<f64> = bins
            .iter()
            .filter(|b| !b.whole)
            .map(|b| b.start_s)
            .collect();
        assert_eq!(out, [50.0]);
        assert_eq!(bins[0].beats, 50);
        assert_eq!(bins[5].beats, 35);
        // The same bins an hour long are cut by the same rule.
        let hour: Vec<Beat> = (0..)
            .map(|i| Beat {
                index: i,
                time: i as f64 * 0.2,
                quality: 1.0,
            })
            .take_while(|b| b.time < 3.0 * 3600.0)
            .filter(|b| !(5000.0..5003.0).contains(&b.time))
            .collect();
        let hours = cuts.bins(&hour, 0.0, 3600.0, 3, 2.0);
        assert_eq!(
            hours.iter().map(|b| b.whole).collect::<Vec<_>>(),
            [true, false, true]
        );
    }

    #[test]
    fn masked_beats_and_windows_are_left_out() {
        let cuts = Cuts::new(None, &[brk(100.0, 30.0)], &[(40.0, 50.0), (45.0, 60.0)]);
        // Overlapping masks are joined.
        assert_eq!(cuts.masked(), [(40.0, 60.0)]);
        let beats = cuts.beats(&beats_to(200.0));
        assert!(beats.iter().all(|b| !(40.0..=60.0).contains(&b.time)));
        // After the pause, beats are on true time.
        assert!(beats.iter().any(|b| (b.time - 130.0).abs() < 1e-9));
        let windows: Vec<AmplitudeWindow> = (0..100)
            .map(|i| window(i as f64 * 2.0, i as f64 * 2.0 + 2.0))
            .collect();
        let kept: Vec<f64> = cuts
            .amplitude_windows(&windows)
            .iter()
            .map(|w| w.start_s)
            .collect();
        assert!(kept.contains(&38.0) && kept.contains(&60.0) && kept.contains(&96.0));
        assert!(!kept.iter().any(|&s| (40.0..60.0).contains(&s)));
        // A window over the pause's audio second straddles it.
        let over = window(99.0, 101.0);
        assert_eq!(cuts.amplitude_windows(std::slice::from_ref(&over)).len(), 0);
    }

    #[test]
    fn whole_stretches_are_cut_at_both_ends_of_each_hole() {
        let cuts = Cuts::new(
            None,
            &[brk(100.0, 30.0), brk(200.0, 0.0)],
            &[(250.0, 262.0)],
        );
        let c = cuts.piece_cuts();
        // The pause from the last sound before it (just under 100 s) to
        // 130 s on true time; the jump at 230 s; the mask 280 to 292 s.
        let want = [100.0 - EDGE_S, 130.0, 230.0, 280.0, 292.0].map(|t| t - EDGE_S);
        assert_eq!(c.len(), want.len(), "{c:?}");
        assert!(
            c.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9),
            "{c:?}"
        );
    }

    #[test]
    fn a_clock_step_no_break_lists_is_a_hole() {
        let clock = paused_clock();
        let cuts = Cuts::new(Some(&clock), &[], &[]);
        assert_eq!(cuts.holes().len(), 1);
        let h = cuts.holes()[0];
        assert_eq!(h.kind, HoleKind::ClockStep);
        assert!(!cuts.clear(h.start_s - 5.0, h.end_s + 5.0));
        assert!(cuts.clear(h.end_s, h.end_s + 10.0));
    }

    #[test]
    fn windows_lie_inside_each_stretch_and_need_no_beat_past_their_end() {
        // Beats every 0.125 s for exactly 10 s: one whole 10 s window,
        // though no beat falls at or after its end.
        let beats: Vec<Beat> = (0..80)
            .map(|i| Beat {
                index: i,
                time: i as f64 * 0.125,
                quality: 1.0,
            })
            .collect();
        let w = Cuts::none().windows(&beats, 28800, 0.0, 10.0, 10.0, 5.0, 2.0);
        assert_eq!(w.len(), 1);
        assert_eq!(
            (w[0].start_s, w[0].end_s, w[0].fit.beats_used),
            (0.0, 10.0, 80)
        );
        // A pause after 100 s and a mask: the windows keep one grid from
        // the first beat, none reaches into a hole, and a window over a
        // dropout no break lists is left out.
        let cuts = Cuts::new(None, &[brk(100.0, 30.0)], &[(150.0, 160.0)]);
        let beats: Vec<Beat> = cuts
            .beats(&beats_to(300.0))
            .into_iter()
            .filter(|b| !(250.0..253.0).contains(&b.time))
            .collect();
        let w = cuts.windows(&beats, 18000, 0.0, 330.0, 10.0, 5.0, 2.0);
        let starts: Vec<f64> = w.iter().map(|w| w.start_s).collect();
        let want: Vec<f64> = (0..65)
            .map(|i| i as f64 * 5.0)
            .filter(|&s| cuts.clear(s, s + 10.0) && !(241.0..253.0).contains(&s))
            .collect();
        assert_eq!(starts, want);
        assert!(starts.contains(&90.0) && starts.contains(&130.0) && starts.contains(&320.0));
        assert!(!starts.contains(&95.0) && !starts.contains(&175.0) && !starts.contains(&185.0));
    }
}
