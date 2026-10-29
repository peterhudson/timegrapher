//! Breaks in a take: pauses in listening, and resumes of a saved take.
//!
//! A take's audio runs straight on across a break, while true time jumps
//! by however long listening stopped. The beats on either side keep the
//! watch's rate but not its phase, and after a long gap not its rate
//! either (the mainspring ran down meanwhile), so each stretch between
//! breaks is fitted on its own: see [`crate::timing::fit_pieces`].
//!
//! A break is known from the take's `session.json` (`breaks`, one entry per
//! resume or pause) or, for takes saved before that list existed, from the
//! step it leaves in the clock log (see [`crate::clock::ClockFit`]).

use crate::amplitude::AmplitudeWindow;
use crate::beats::Beat;
use crate::capture::Block;
use serde::Serialize;
use std::time::SystemTime;

/// One break: where it falls in the take's audio, and how long listening
/// stopped there.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Break {
    /// Audio seconds before the break, on the sound card's clock.
    pub audio_s: f64,
    /// True seconds that passed with no audio, as near as the clock log or
    /// the recorder could tell.
    pub gap_s: f64,
}

/// In the clock log, a step of the system time over the audio time of at
/// least this many seconds between two entries is a break. A late
/// timestamp (the logger descheduled) is a spike instead, and is told
/// apart by the entries after it falling back.
pub const MIN_GAP_S: f64 = 0.5;

/// How many entries, from the one after a step on, say whether the step
/// was kept: their median offset (see [`held_step`]).
pub const HELD_ENTRIES: usize = 4;

/// How much of a step in the offset between the clocks the entries from
/// the one after it on keep: the median of their offsets (at most
/// [`HELD_ENTRIES`] of them) less the offset before the step. A break keeps
/// at least half its step; a late entry (a spike) keeps little of it.
pub fn held_step(before: f64, later: &[f64]) -> f64 {
    let mut v = later[..later.len().min(HELD_ENTRIES)].to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2] - before
}

/// The gap a step of at least [`MIN_GAP_S`] in the offset between the
/// clocks leaves, when the entries after it keep at least half of it
/// (`held`, from [`held_step`]): what they keep, never more than the step.
/// `None` for a smaller step, or a spike the entries fall back from. The
/// one rule for a break, live ([`GapWatch`]) and in a clock log
/// ([`from_clock_at`]).
pub fn kept_gap(step: f64, held: f64) -> Option<f64> {
    (step >= MIN_GAP_S && held >= 0.5 * step).then(|| held.min(step))
}

/// What a live input's blocks become once [`GapWatch`] has seen them: the
/// blocks, in order, with a break before the first block after a gap.
#[derive(Debug)]
pub enum Live {
    Block(Block),
    /// The true seconds that passed with no audio before the next block:
    /// the rise in the offset since the clock log's last line that the
    /// blocks after it keep ([`kept_gap`]), which
    /// [`crate::recorder::Recorder::sound_lost`] lists for the break.
    Break(f64),
}

/// Audio seconds after a rise in a live input's offset that are held back
/// before judging it, or the rise itself when it is shorter: longer than
/// an input's own buffer, so when the input was only late, its backlog has
/// come and the offset is back down by then.
pub const HOLD_S: f64 = 5.0;

/// The breaks of a live input, found as a saved take's clock log finds
/// them ([`from_clock_at`]). Each block's offset is its arrival less the
/// audio up to its end. A break is a rise of at least [`MIN_GAP_S`] in that
/// offset since the clock log's last line (the blocks the recorder logs:
/// [`crate::recorder::clock_line_due`]) which the blocks after it keep
/// ([`held_step`] over the [`HELD_ENTRIES`] blocks after those that
/// cover the rise, or [`HOLD_S`], in audio). So small losses add up between lines
/// as they do in the log, and a late block whose sound is all there is no
/// break: the backlog after it brings the offset back. Blocks after a
/// rise are held back until then, so a break is known before the block
/// after it is written, and [`crate::recorder::Recorder::sound_lost`]
/// lists it with the same gap.
#[derive(Debug)]
pub struct GapWatch {
    rate: f64,
    frames: u64,
    /// The offset at the clock log's last line, and that block's arrival.
    line: Option<(f64, SystemTime)>,
    /// Blocks held back after a rise, with their offsets and the audio
    /// seconds by their end.
    held: Vec<(Block, f64, f64)>,
}

impl GapWatch {
    pub fn new(sample_rate: u32) -> Self {
        GapWatch {
            rate: sample_rate.max(1) as f64,
            frames: 0,
            line: None,
            held: Vec::new(),
        }
    }

    /// Take the next block; give back what can be let through now.
    pub fn push(&mut self, b: Block) -> Vec<Live> {
        self.frames += b.samples.len() as u64;
        let end_s = self.frames as f64 / self.rate;
        let at =
            b.at.duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0.0, |d| d.as_secs_f64());
        let mut out = Vec::new();
        self.take(b, at - end_s, end_s, &mut out);
        out
    }

    /// The blocks still held back, at the end of the input, judged on the
    /// blocks there are, as the clock log's last entries are.
    pub fn finish(&mut self) -> Vec<Live> {
        let mut out = Vec::new();
        while !self.held.is_empty() {
            self.settle(&mut out);
        }
        out
    }

    fn let_through(&mut self, b: Block, off: f64, end_s: f64, out: &mut Vec<Live>) {
        if crate::recorder::clock_line_due(end_s, self.line.map(|l| l.1), b.at) {
            self.line = Some((off, b.at));
        }
        out.push(Live::Block(b));
    }

    fn take(&mut self, b: Block, off: f64, end_s: f64, out: &mut Vec<Live>) {
        let line = self.line.map(|l| l.0);
        if self.held.is_empty() && line.is_none_or(|o| off - o < MIN_GAP_S) {
            self.let_through(b, off, end_s, out);
            return;
        }
        self.held.push((b, off, end_s));
        // Held until the blocks cover the rise (or HOLD_S), so a backlog
        // has come, and then HELD_ENTRIES more to judge it on.
        let rise = self.held[0].1 - line.unwrap_or(off);
        let start_s = self.held[0].2 - self.held[0].0.samples.len() as f64 / self.rate;
        let covered = self
            .held
            .iter()
            .position(|h| h.2 - start_s >= rise.min(HOLD_S) - 1e-9);
        if covered.is_some_and(|j| self.held.len() > j + HELD_ENTRIES) {
            self.settle(out);
        }
    }

    /// Judge the first held block's rise on the last blocks held, let it
    /// through, and take the rest again in order.
    fn settle(&mut self, out: &mut Vec<Live>) {
        let held = std::mem::take(&mut self.held);
        let line = self.line.map_or(held[0].1, |l| l.0);
        let step = held[0].1 - line;
        let offs: Vec<f64> = held.iter().map(|h| h.1).collect();
        let kept = held_step(line, &offs[offs.len().saturating_sub(HELD_ENTRIES)..]);
        if let Some(gap) = kept_gap(step, kept) {
            out.push(Live::Break(gap));
            // The recorder logs the first block after a break.
            self.line = None;
        }
        let mut rest = held.into_iter();
        let (first, off, end_s) = rest.next().expect("a held block");
        self.let_through(first, off, end_s, out);
        for (b, off, end_s) in rest {
            self.take(b, off, end_s, out);
        }
    }
}

/// Breaks seen in a clock log's `(audio_s, true_s)` entries, sorted by
/// audio time: a step in the offset between the two clocks that the next
/// entries keep, with a break also at each of `at` (audio seconds)
/// whatever the size of its step: at the first entry after it, its gap the
/// step the next entries keep. An entry at the same second is the last one
/// before the break, as the recorder writes it at a pause.
pub fn from_clock_at(pairs: &[(f64, f64)], at: &[f64]) -> Vec<Break> {
    let off: Vec<f64> = pairs.iter().map(|q| q.1 - q.0).collect();
    let mut out = Vec::new();
    for i in 1..pairs.len() {
        let step = off[i] - off[i - 1];
        if at
            .iter()
            .any(|&a| pairs[i - 1].0 <= a + 1e-6 && a + 1e-6 < pairs[i].0)
        {
            out.push(Break {
                audio_s: pairs[i].0,
                gap_s: held_step(off[i - 1], &off[i..]),
            });
            continue;
        }
        // Kept: the offset a few entries on stays at least halfway up the
        // step, or there are no entries after this one to say otherwise.
        if step < MIN_GAP_S {
            continue;
        }
        if let Some(gap_s) = kept_gap(step, held_step(off[i - 1], &off[i..])) {
            out.push(Break {
                audio_s: pairs[i].0,
                gap_s,
            });
        }
    }
    out
}

/// Why a take's breaks are not all from the list its `session.json`
/// keeps, but from its clock log ([`crate::clock::ClockFit`]) instead: the
/// caveat [`crate::measure::Caveat::BreaksUnlisted`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unlisted {
    /// The take's `session.json` could not be read, or is not JSON.
    Unreadable,
    /// Some of its listed breaks give no place or no gap, so the clock
    /// log stands in for them.
    EntriesSkipped,
    /// The files are not the take's segments in order (one opened alone
    /// out of several), so its list, for the whole take, is not used.
    NotTheTake,
}

/// A take's breaks, and why they are not all its own list's.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    pub breaks: Vec<Break>,
    pub unlisted: Option<Unlisted>,
}

/// Breaks from a take's `session.json`, sorted: each entry's `frame`
/// (frames before the break) over the sample rate, else its `audio_s`, and
/// its `gap_s`; and how many entries give neither place nor a finite gap,
/// left out. An absent or empty list gives none.
pub fn from_session(json: &serde_json::Value, sample_rate: u32) -> (Vec<Break>, usize) {
    let list = json
        .get("breaks")
        .and_then(|b| b.as_array())
        .map_or(&[][..], |l| l.as_slice());
    let mut out: Vec<Break> = list
        .iter()
        .filter_map(|e| {
            let audio_s = match e.get("frame").and_then(|f| f.as_u64()) {
                Some(f) if sample_rate > 0 => f as f64 / sample_rate as f64,
                _ => e.get("audio_s")?.as_f64()?,
            };
            let gap_s = match e.get("gap_s") {
                None | Some(serde_json::Value::Null) => 0.0,
                Some(g) => g.as_f64()?,
            };
            (audio_s.is_finite() && gap_s.is_finite()).then_some(Break { audio_s, gap_s })
        })
        .collect();
    let skipped = list.len() - out.len();
    out.sort_by(|a, b| a.audio_s.total_cmp(&b.audio_s));
    (out, skipped)
}

/// A take's breaks: those its `session.json` lists, when it lists any,
/// else those its clock log shows. An entry the list cannot place is left
/// to the clock log, and said ([`Unlisted::EntriesSkipped`]).
pub fn for_take(
    session_json: Option<&serde_json::Value>,
    clock: Option<&crate::clock::ClockFit>,
    sample_rate: u32,
) -> Found {
    let (listed, skipped) = session_json.map_or((Vec::new(), 0), |v| from_session(v, sample_rate));
    let unlisted = (skipped > 0).then_some(Unlisted::EntriesSkipped);
    let breaks = if listed.is_empty() {
        clock.map_or_else(Vec::new, |c| c.breaks.clone())
    } else {
        listed
    };
    Found { breaks, unlisted }
}

/// The breaks of the take that `files` (in order) make up: from the
/// `session.json` beside the first file when the files are that take's
/// segments, else from the clock fit. A file given on its own out of a
/// take of several segments takes its breaks from the clock alone, and a
/// `session.json` that cannot be read leaves them to the clock too; each
/// is said ([`Unlisted`]). A take with no `session.json` (a plain sound
/// file) lists none, so its clock's breaks are all it has.
pub fn of_files(
    files: &[&std::path::Path],
    clock: Option<&crate::clock::ClockFit>,
    sample_rate: u32,
) -> Found {
    let path = files
        .first()
        .and_then(|f| f.parent())
        .map(|d| d.join(crate::take::RECORD));
    let json: Result<Option<serde_json::Value>, ()> = match path {
        Some(p) if p.exists() => std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .map(Some)
            .ok_or(()),
        _ => Ok(None),
    };
    let Ok(json) = json else {
        return Found {
            breaks: clock.map_or_else(Vec::new, |c| c.breaks.clone()),
            unlisted: Some(Unlisted::Unreadable),
        };
    };
    let names: Vec<String> = files
        .iter()
        .filter_map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    let whole = json
        .as_ref()
        .is_some_and(|v| match v.get("segments").and_then(|s| s.as_array()) {
            Some(list) => list
                .iter()
                .filter_map(|s| s.as_str())
                .eq(names.iter().map(|n| n.as_str())),
            None => names.len() == 1,
        });
    let mut found = for_take(json.as_ref().filter(|_| whole), clock, sample_rate);
    if json.is_some() && !whole {
        found.unlisted = Some(Unlisted::NotTheTake);
    }
    found
}

/// [`of_files`], placed on the clock with [`on_clock`].
pub fn of_files_on_clock(
    files: &[&std::path::Path],
    clock: Option<&mut crate::clock::ClockFit>,
    sample_rate: u32,
) -> Found {
    let found = of_files(files, clock.as_deref(), sample_rate);
    Found {
        breaks: on_clock(found.breaks, clock),
        ..found
    }
}

/// The listed breaks with the clock fit's breaks moved to them
/// ([`crate::clock::ClockFit::place_breaks`]), so the clock and the breaks
/// put each gap at the same audio second, and with every step in the
/// clock log that no listed break explains (a system clock step, or a
/// dropout the recorder did not list) as a break of its own, as it is
/// when nothing is listed. Sorted.
pub fn on_clock(listed: Vec<Break>, clock: Option<&mut crate::clock::ClockFit>) -> Vec<Break> {
    let Some(c) = clock else {
        return listed;
    };
    c.place_breaks(&listed);
    let mut out = listed.clone();
    out.extend(
        c.breaks
            .iter()
            .filter(|cb| !listed.iter().any(|b| (b.audio_s - cb.audio_s).abs() < 1e-9))
            .copied(),
    );
    out.sort_by(|a, b| a.audio_s.total_cmp(&b.audio_s));
    out
}

/// How near a jump in the beats must be to a listed break to be that break,
/// seconds: the beats can take a second or two to be found again after it.
pub const JUMP_MATCH_S: f64 = 5.0;

/// The breaks with the jumps the beats show ([`crate::stream::BeatLog`]'s
/// `jumps`) where none is listed: a jump within [`JUMP_MATCH_S`] of a
/// listed break is that break; any other is a break of unknown length,
/// with `gap_s` 0.
pub fn with_jumps(listed: Vec<Break>, jumps: &[f64]) -> Vec<Break> {
    let mut out = listed.clone();
    for &j in jumps {
        if !listed.iter().any(|b| (b.audio_s - j).abs() <= JUMP_MATCH_S) {
            out.push(Break {
                audio_s: j,
                gap_s: 0.0,
            });
        }
    }
    out.sort_by(|a, b| a.audio_s.total_cmp(&b.audio_s));
    out
}

/// How many events the breaks at `at` (audio seconds, sorted) are: breaks
/// with under [`JUMP_MATCH_S`] of the watch heard between them, by the good
/// beats (better than [`crate::beats::SIDE_QUALITY`]) of `beats` between
/// them at `bph`, are one, as after a silence the beats can step more than
/// once before they are found again. This is the count the `breaks` caveat
/// gives.
pub fn events(at: &[f64], beats: &[Beat], bph: u32) -> usize {
    let period = 3600.0 / bph.max(1) as f64;
    let heard = |a: f64, b: f64| {
        let (i, j) = (
            beats.partition_point(|x| x.time <= a),
            beats.partition_point(|x| x.time < b),
        );
        beats[i..j.max(i)]
            .iter()
            .filter(|x| x.quality > crate::beats::SIDE_QUALITY)
            .count() as f64
            * period
    };
    let new = at
        .windows(2)
        .filter(|p| heard(p[0], p[1]) >= JUMP_MATCH_S)
        .count();
    if at.is_empty() {
        0
    } else {
        new + 1
    }
}

/// True seconds from the start of the take for audio second `t`, adding
/// the gaps of the breaks before it (a negative gap counts as none).
pub fn wall(breaks: &[Break], t: f64) -> f64 {
    t + breaks
        .iter()
        .filter(|b| b.audio_s <= t)
        .map(|b| b.gap_s.max(0.0))
        .sum::<f64>()
}

/// Whether a stretch from `start` to `end` (audio seconds) spans a break.
pub fn straddles(breaks: &[Break], start: f64, end: f64) -> bool {
    breaks.iter().any(|b| start < b.audio_s && b.audio_s < end)
}

/// Which stretch between breaks audio second `t` falls in, from 0.
pub fn piece(breaks: &[Break], t: f64) -> usize {
    breaks.iter().filter(|b| b.audio_s <= t).count()
}

/// Make beat numbers run on across the breaks as the watch did: each
/// stretch after a break is renumbered by the beats the gap held (none for
/// a negative gap), rounded to keep the sides. Where the side sounds told
/// the sides apart at a jump within [`JUMP_MATCH_S`] of the break (`sided`,
/// [`crate::stream::BeatLog`]'s), the numbers already keep them; elsewhere
/// the sides are kept by the beat count alone, and those breaks are
/// returned (audio seconds) for [`crate::measure::Caveat::SidesByCount`].
/// Amplitude windows that span a break are dropped.
pub fn renumber(
    beats: &mut [Beat],
    windows: &mut Vec<AmplitudeWindow>,
    breaks: &[Break],
    sided: &[f64],
    bph: u32,
) -> Vec<f64> {
    windows.retain(|w| !straddles(breaks, w.start_s, w.end_s));
    let shifts = index_shifts(breaks, bph);
    for beat in beats.iter_mut() {
        beat.index += index_shift_at(&shifts, beat.time);
    }
    by_count(breaks.iter().map(|b| b.audio_s), sided)
}

/// How far [`renumber`] moves the beat numbers from each break on: the
/// break's audio second, with the beats the gaps up to it held (none for
/// a negative gap), each gap's rounded to keep the sides. A reader drawing
/// beats on audio seconds takes it off again.
pub fn index_shifts(breaks: &[Break], bph: u32) -> Vec<(f64, i64)> {
    let period = 3600.0 / bph.max(1) as f64;
    let mut shift = 0i64;
    breaks
        .iter()
        .map(|b| {
            shift += 2 * (b.gap_s.max(0.0) / (2.0 * period)).round() as i64;
            (b.audio_s, shift)
        })
        .collect()
}

/// The shift of [`index_shifts`] for a beat at audio second `t`: the last
/// break's at or before it, none before the first.
pub fn index_shift_at(shifts: &[(f64, i64)], t: f64) -> i64 {
    shifts
        .iter()
        .take_while(|s| s.0 <= t)
        .last()
        .map_or(0, |s| s.1)
}

/// Of the breaks at `at` (audio seconds), those where the sides were not
/// told apart by their sound (no time in `sided` within [`JUMP_MATCH_S`]):
/// their sides are kept by the count.
pub fn by_count(at: impl IntoIterator<Item = f64>, sided: &[f64]) -> Vec<f64> {
    at.into_iter()
        .filter(|&t| !sided.iter().any(|&j| (j - t).abs() <= JUMP_MATCH_S))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_step_is_a_break_of_what_is_kept() {
        assert_eq!(kept_gap(MIN_GAP_S - 0.01, 10.0), None);
        assert_eq!(kept_gap(3.0, 3.0), Some(3.0));
        assert_eq!(kept_gap(3.0, 2.0), Some(2.0));
        assert_eq!(kept_gap(3.0, 4.0), Some(3.0));
        assert_eq!(kept_gap(3.0, 1.4), None);
    }

    #[test]
    fn a_live_gap_breaks_only_where_the_sound_was_lost() {
        // Blocks of 0.1 s at 48 kHz, each arriving at `arrive(k)` seconds.
        let t0 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000);
        let run = |arrive: &dyn Fn(usize) -> f64| {
            let mut w = GapWatch::new(48_000);
            let mut out = Vec::new();
            for k in 0..200 {
                out.extend(w.push(Block {
                    samples: vec![k as f32; 4_800],
                    at: t0 + std::time::Duration::from_secs_f64(arrive(k)),
                }));
            }
            out.extend(w.finish());
            // Every block comes out, in order; the breaks as (blocks
            // before, gap).
            let mut blocks = 0;
            let mut found = Vec::new();
            for l in out {
                match l {
                    Live::Block(b) => {
                        assert_eq!(b.samples[0], blocks as f32);
                        blocks += 1;
                    }
                    Live::Break(g) => found.push((blocks, g)),
                }
            }
            assert_eq!(blocks, 200);
            found
        };
        let near = |f: Vec<(usize, f64)>, want: &[(usize, f64)]| {
            assert_eq!(f.len(), want.len(), "{f:?}");
            for (a, b) in f.iter().zip(want) {
                assert_eq!(a.0, b.0, "{f:?}");
                assert!((a.1 - b.1).abs() < 1e-6, "{f:?}");
            }
        };
        // Steady, with a little jitter: no break.
        near(run(&|k| 0.1 * (k + 1) as f64 + 0.03 * (k % 3) as f64), &[]);
        // 3 s with no sound before block 50 (the computer slept): a break.
        near(
            run(&|k| 0.1 * (k + 1) as f64 + if k >= 50 { 3.0 } else { 0.0 }),
            &[(50, 3.0)],
        );
        // One gap just at MIN_GAP_S and one just under it.
        near(
            run(&|k| {
                0.1 * (k + 1) as f64
                    + if k >= 40 { MIN_GAP_S } else { 0.0 }
                    + if k >= 120 { MIN_GAP_S - 0.01 } else { 0.0 }
            }),
            &[(40, MIN_GAP_S)],
        );
        // The input stalled 2 s and then brought every block it held at
        // once: nothing was lost, so no break.
        near(
            run(&|k| (0.1 * (k + 1) as f64).max(if k >= 50 { 7.1 } else { 0.0 })),
            &[],
        );
        // A single block late, the next ones on time: no break.
        near(
            run(&|k| 0.1 * (k + 1) as f64 + if k == 80 { 1.5 } else { 0.0 }),
            &[],
        );
        // A gap in the last blocks is judged on the blocks there are.
        near(
            run(&|k| 0.1 * (k + 1) as f64 + if k >= 198 { 2.0 } else { 0.0 }),
            &[(198, 2.0)],
        );
    }

    /// The shifts run on over the gaps, each rounded to whole tick-tock
    /// pairs, none for a negative gap; renumber moves each beat by the
    /// shift at its second, and a reader taking it off gets the numbers
    /// back.
    #[test]
    fn the_index_shift_is_the_beats_the_gaps_held() {
        let breaks = [
            Break {
                audio_s: 10.0,
                gap_s: 1.0,
            },
            Break {
                audio_s: 20.0,
                gap_s: -0.5,
            },
            Break {
                audio_s: 30.0,
                gap_s: 0.3,
            },
        ];
        // 28800 bph: 0.125 s a beat, 0.25 s a tick-tock pair.
        let shifts = index_shifts(&breaks, 28800);
        assert_eq!(shifts, [(10.0, 8), (20.0, 8), (30.0, 10)]);
        assert_eq!(index_shift_at(&shifts, 9.9), 0);
        assert_eq!(index_shift_at(&shifts, 10.0), 8);
        assert_eq!(index_shift_at(&shifts, 25.0), 8);
        assert_eq!(index_shift_at(&shifts, 31.0), 10);
        let mut beats: Vec<Beat> = [5.0, 10.0, 15.0, 29.0, 35.0]
            .iter()
            .enumerate()
            .map(|(i, &time)| Beat {
                index: i as i64,
                time,
                quality: 1.0,
            })
            .collect();
        let was = beats.clone();
        renumber(&mut beats, &mut Vec::new(), &breaks, &[], 28800);
        let moved: Vec<i64> = beats.iter().map(|b| b.index).collect();
        assert_eq!(moved, [0, 9, 10, 11, 14]);
        for (b, w) in beats.iter().zip(&was) {
            assert_eq!(b.index - index_shift_at(&shifts, b.time), w.index);
        }
    }
}
