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
use serde::Serialize;

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

/// Breaks seen in a clock log's `(audio_s, true_s)` entries, sorted by
/// audio time: a step in the offset between the two clocks that the next
/// entries keep.
pub fn from_clock(pairs: &[(f64, f64)]) -> Vec<Break> {
    from_clock_at(pairs, &[])
}

/// [`from_clock`], with a break also at each of `at` (audio seconds)
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
            let later = &off[i..(i + 4).min(off.len())];
            let mut v = later.to_vec();
            v.sort_by(|a, b| a.total_cmp(b));
            out.push(Break {
                audio_s: pairs[i].0,
                gap_s: v[v.len() / 2] - off[i - 1],
            });
            continue;
        }
        if step < MIN_GAP_S {
            continue;
        }
        // Kept: the offset a few entries on stays at least halfway up the
        // step, or there are no entries after this one to say otherwise.
        let later = &off[i..(i + 4).min(off.len())];
        let mut v = later.to_vec();
        v.sort_by(|a, b| a.total_cmp(b));
        let held = v[v.len() / 2] - off[i - 1];
        if held >= 0.5 * step {
            out.push(Break {
                audio_s: pairs[i].0,
                gap_s: held.min(step),
            });
        }
    }
    out
}

/// Breaks from a take's `session.json`, sorted: each entry's `frame`
/// (frames before the break) over the sample rate, else its `audio_s`, and
/// its `gap_s`. An absent or empty list gives none.
pub fn from_session(json: &serde_json::Value, sample_rate: u32) -> Vec<Break> {
    let mut out: Vec<Break> = json
        .get("breaks")
        .and_then(|b| b.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|e| {
                    let audio_s = match e.get("frame").and_then(|f| f.as_u64()) {
                        Some(f) if sample_rate > 0 => f as f64 / sample_rate as f64,
                        _ => e.get("audio_s")?.as_f64()?,
                    };
                    let gap_s = e.get("gap_s").and_then(|g| g.as_f64()).unwrap_or(0.0);
                    (audio_s.is_finite() && gap_s.is_finite()).then_some(Break { audio_s, gap_s })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.audio_s.total_cmp(&b.audio_s));
    out
}

/// A take's breaks: those its `session.json` lists, when it lists any,
/// else those its clock log shows.
pub fn for_take(
    session_json: Option<&serde_json::Value>,
    clock: Option<&crate::clock::ClockFit>,
    sample_rate: u32,
) -> Vec<Break> {
    let listed = session_json.map_or_else(Vec::new, |v| from_session(v, sample_rate));
    if listed.is_empty() {
        clock.map_or_else(Vec::new, |c| c.breaks.clone())
    } else {
        listed
    }
}

/// The breaks of the take that `files` (in order) make up: from the
/// `session.json` beside the first file when the files are that take's
/// segments, else from the clock fit. A file given on its own out of a
/// take of several segments takes its breaks from the clock alone.
pub fn of_files(
    files: &[&std::path::Path],
    clock: Option<&crate::clock::ClockFit>,
    sample_rate: u32,
) -> Vec<Break> {
    let json: Option<serde_json::Value> = files
        .first()
        .and_then(|f| f.parent())
        .and_then(|d| std::fs::read_to_string(d.join("session.json")).ok())
        .and_then(|t| serde_json::from_str(&t).ok());
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
    for_take(json.as_ref().filter(|_| whole), clock, sample_rate)
}

/// [`of_files`], with the clock fit's breaks moved to the listed ones
/// ([`crate::clock::ClockFit::place_breaks`]), so the clock and the breaks
/// put each gap at the same audio second.
pub fn of_files_on_clock(
    files: &[&std::path::Path],
    clock: Option<&mut crate::clock::ClockFit>,
    sample_rate: u32,
) -> Vec<Break> {
    let listed = of_files(files, clock.as_deref(), sample_rate);
    if let Some(c) = clock {
        c.place_breaks(&listed);
    }
    listed
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

/// True seconds from the start of the take for audio second `t`, adding
/// the gaps of the breaks before it.
pub fn wall(breaks: &[Break], t: f64) -> f64 {
    t + breaks
        .iter()
        .filter(|b| b.audio_s <= t)
        .map(|b| b.gap_s)
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
/// stretch after a break is renumbered by the beats the gap held, rounded
/// to keep the sides, and turned over by one beat where its sides came out
/// the other way round (told by the sign of its beat error against the
/// first stretch's, when both are clear of zero). Amplitude windows that
/// span a break are dropped, and those of a turned stretch swap sides.
pub fn renumber(
    beats: &mut [Beat],
    windows: &mut Vec<AmplitudeWindow>,
    breaks: &[Break],
    bph: u32,
) {
    if breaks.is_empty() {
        return;
    }
    windows.retain(|w| !straddles(breaks, w.start_s, w.end_s));
    let period = 3600.0 / bph.max(1) as f64;
    let be: Vec<Option<f64>> = (0..=breaks.len())
        .map(|k| {
            let run: Vec<Beat> = beats
                .iter()
                .filter(|b| piece(breaks, b.time) == k)
                .copied()
                .collect();
            crate::timing::beat_error_ms(&run).filter(|v| v.abs() >= TURN_MIN_MS)
        })
        .collect();
    let first = be[0];
    let mut shift = 0i64;
    for (k, b) in breaks.iter().enumerate() {
        let k = k + 1;
        shift += 2 * (b.gap_s / (2.0 * period)).round() as i64;
        let turned = matches!((first, be[k]), (Some(a), Some(c)) if a.signum() != c.signum());
        let s = shift + i64::from(turned);
        let end = breaks.get(k).map_or(f64::INFINITY, |n| n.audio_s);
        for beat in beats
            .iter_mut()
            .filter(|x| x.time >= b.audio_s && x.time < end)
        {
            beat.index += s;
        }
        if turned {
            for w in windows
                .iter_mut()
                .filter(|w| w.start_s >= b.audio_s && w.end_s <= end)
            {
                w.swap_sides();
            }
        }
    }
}

/// A stretch's beat error must be at least this far from zero, ms, before
/// its sign is trusted to say which side is which.
const TURN_MIN_MS: f64 = 0.05;
