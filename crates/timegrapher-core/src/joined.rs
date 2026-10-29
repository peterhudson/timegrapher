//! One take's measured stretches joined end to end, so the steadiness
//! tests and the long-run analysis read them as one run: the app's
//! Steadiness panes join the stretches in the position the watch is in,
//! and `timegrapher series` and `timegrapher long` join a take's entries,
//! each after its settling.

use crate::beats::Beat;
use crate::breaks::{self, Break};
use crate::clock::ClockFit;
use crate::session::Manifest;
use crate::stream::BeatLog;
use crate::take;

/// Seconds of beats the steadiness tests need before they can say
/// anything: 30 rate readings of 10 s.
pub const MIN_S: f64 = 300.0;

/// Room left between two stretches when they are joined, seconds: longer
/// than a gap the readings fit across.
pub const JOIN_S: f64 = 5.0;

/// The take's clock as [`join`] reads a time on it: `t` itself without
/// one.
fn on_clock(clock: Option<&ClockFit>, t: f64) -> f64 {
    clock.map_or(t, |c| c.map(t))
}

/// The length of the stretches `spans` (start, end; the last may run on)
/// once joined end to end, up to `end_s`, seconds, as [`join`] gives it on
/// `clock`.
pub fn joined_duration(spans: &[(f64, f64)], end_s: f64, clock: Option<&ClockFit>) -> f64 {
    let lens: Vec<f64> = spans
        .iter()
        .map(|&(a, b)| (a.max(0.0), b.min(end_s)))
        .filter(|(a, b)| b > a)
        .map(|(a, b)| on_clock(clock, b) - on_clock(clock, a))
        .collect();
    lens.iter().sum::<f64>() + JOIN_S * lens.len().saturating_sub(1) as f64
}

/// The stretches of a take that are read, audio seconds, in time order:
/// each entry's from the end of its settling, as `timegrapher session`
/// reads it ([`take::measured`]), on a take whose entries are all one
/// recording `duration_s` long and name their positions. None when that
/// leaves the whole recording, or the entries are not like that: the take
/// is then read whole.
pub fn take_spans(m: &Manifest, duration_s: f64) -> Option<Vec<(f64, f64)>> {
    let first = m.recordings.first()?;
    if m.recordings
        .iter()
        .any(|e| e.file != first.file || e.position.is_none())
    {
        return None;
    }
    let mut spans: Vec<(f64, f64)> = m
        .recordings
        .iter()
        .filter(|e| !take::reads_nothing(e))
        .map(|e| {
            let start = e.start_s.unwrap_or(0.0).max(0.0);
            let stop = e.end_s.unwrap_or(duration_s).min(duration_s);
            let (settle_s, measure_s) = take::entry_times(e, m);
            take::measured(start, stop, settle_s, measure_s)
        })
        .filter(|(a, b)| b > a)
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    match spans.as_slice() {
        [(a, b)] if *a <= 0.0 && *b >= duration_s => None,
        _ => Some(spans),
    }
}

/// The part of `log` inside `spans`, in time order, with the time between
/// them taken out: one position's beats over a session that moved the
/// watch, joined so the tests see them as one run, with a short gap at each
/// join that no reading fits across. With it, the cuts the joined readings
/// stop at: each of `breaks` inside a span, on the joined times and with
/// the time its gap held, each join, with none, and every jump.
///
/// The joined times are on the take's `clock`: each beat is put on it
/// before the time between stretches is taken out, so the joined log is
/// read with no clock after. A clock that puts the pauses back itself
/// ([`ClockFit::breaks`]) leaves the breaks no gap of their own, and each
/// of its steps inside a span is a cut.
pub fn join(
    log: &BeatLog,
    spans: &[(f64, f64)],
    breaks: &[Break],
    clock: Option<&ClockFit>,
) -> (BeatLog, Vec<Break>) {
    let time = |t: f64| on_clock(clock, t);
    let pauses_back = clock.is_some_and(|c| !c.breaks.is_empty());
    let steps = clock.map_or(&[][..], |c| &c.breaks[..]);
    let mut cuts = Vec::new();
    let beat = 3600.0 / log.bph as f64;
    let mut out = BeatLog {
        sample_rate: log.sample_rate,
        duration_s: 0.0,
        bph: log.bph,
        lift_deg: log.lift_deg,
        beats: Vec::new(),
        amplitude_windows: Vec::new(),
        clipped_beats: Vec::new(),
        clipped_samples: log.clipped_samples,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    // Times in [a, b), on the clock and with this span's shift taken off.
    let within = |at: &[f64], a: f64, b: f64, shift_t: f64| -> Vec<f64> {
        at.iter()
            .filter(|&&t| t >= a && t < b)
            .map(|&t| time(t) - shift_t)
            .collect()
    };
    // The last kept span's end and last beat (time, number), joined.
    let mut prev: Option<(f64, f64, i64)> = None;
    for &(a, b) in spans {
        let b = b.min(log.duration_s);
        let lo = log.beats.partition_point(|x| x.time < a);
        let hi = log.beats.partition_point(|x| x.time < b);
        let Some(first) = log.beats.get(lo).filter(|_| lo < hi) else {
            continue;
        };
        // Taken off this span's times and beat numbers: it starts JOIN_S
        // after the last one ended, its beats numbered on by the beats that
        // would fit between.
        let (shift_t, shift_i) = match prev {
            None => (time(a.max(0.0)), first.index),
            Some((end, last_t, last_i)) => {
                let shift_t = time(a) - (end + JOIN_S);
                let n = ((time(first.time) - shift_t - last_t) / beat)
                    .round()
                    .max(1.0) as i64;
                (shift_t, first.index - (last_i + n))
            }
        };
        if prev.is_some() {
            out.jumps.push(time(a) - shift_t);
            cuts.push(Break {
                audio_s: time(a) - shift_t,
                gap_s: 0.0,
            });
        }
        cuts.extend(
            breaks
                .iter()
                .filter(|c| c.audio_s > a && c.audio_s < b)
                .map(|c| Break {
                    audio_s: time(c.audio_s) - shift_t,
                    gap_s: if pauses_back { 0.0 } else { c.gap_s },
                }),
        );
        // Already among `breaks` when they came from core's take reader
        // ([`breaks::on_clock`]).
        cuts.extend(
            steps
                .iter()
                .filter(|c| c.audio_s > a && c.audio_s < b)
                .filter(|c| !breaks.iter().any(|l| (l.audio_s - c.audio_s).abs() < 1e-9))
                .map(|c| Break {
                    audio_s: time(c.audio_s) - shift_t,
                    gap_s: 0.0,
                }),
        );
        out.jumps.extend(within(&log.jumps, a, b, shift_t));
        out.sided.extend(within(&log.sided, a, b, shift_t));
        out.by_count.extend(within(&log.by_count, a, b, shift_t));
        out.beats.extend(log.beats[lo..hi].iter().map(|x| Beat {
            time: time(x.time) - shift_t,
            index: x.index - shift_i,
            ..*x
        }));
        out.amplitude_windows.extend(
            log.amplitude_windows
                .iter()
                .filter(|w| w.start_s >= a && w.end_s <= b)
                .map(|w| {
                    let mut w = w.clone();
                    w.start_s = time(w.start_s) - shift_t;
                    w.end_s = time(w.end_s) - shift_t;
                    // Beat indices changed sides; keep the existing side
                    // measurements and signed errors on those same beats.
                    if shift_i % 2 != 0 {
                        w.swap_sides();
                    }
                    w
                }),
        );
        out.clipped_beats
            .extend(within(&log.clipped_beats, a, b, shift_t));
        let last = out.beats.last().unwrap();
        out.duration_s = time(b) - shift_t;
        prev = Some((out.duration_s, last.time, last.index));
    }
    for at in [&mut out.jumps, &mut out.sided, &mut out.by_count] {
        at.sort_by(f64::total_cmp);
        at.dedup();
    }
    let cuts = breaks::with_jumps(cuts, &out.jumps);
    (out, cuts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 200 s of an 18,000 bph watch, a beat every 0.2 s.
    fn log() -> BeatLog {
        BeatLog {
            sample_rate: 48_000,
            duration_s: 200.0,
            bph: 18_000,
            lift_deg: 52.0,
            beats: (0..1000)
                .map(|i| Beat {
                    index: i,
                    time: i as f64 * 0.2,
                    quality: 1.0,
                })
                .collect(),
            amplitude_windows: Vec::new(),
            clipped_beats: Vec::new(),
            clipped_samples: 0,
            jumps: Vec::new(),
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        }
    }

    /// A jump whose sides were named by the beat count alone stays one in
    /// the joined log, where its stretch is kept, so its readings carry
    /// the caveat; one in the time taken out goes with it.
    #[test]
    fn a_join_keeps_the_sides_named_by_count() {
        let mut log = log();
        log.jumps = vec![30.0, 50.0, 120.0];
        log.sided = vec![120.0];
        log.by_count = vec![30.0, 50.0];
        let (joined, _) = join(&log, &[(10.0, 40.0), (100.0, 190.0)], &[], None);
        // 30 s is 20 s into the first stretch; 120 s is 20 s into the
        // second, which starts 35 s into the joined log.
        assert_eq!(joined.by_count, [20.0]);
        assert_eq!(joined.sided, [55.0]);
    }

    /// On a card 100 ppm slow, the joined beats are on the take's clock:
    /// a second of sound is 1.0001 s, and the joined log runs as long as
    /// [`joined_duration`] says on that clock.
    #[test]
    fn a_join_puts_the_beats_on_the_takes_clock() {
        let clock = ClockFit::fixed(100.0, 200.0);
        let spans = [(10.0, 60.0), (100.0, 190.0)];
        let (joined, cuts) = join(&log(), &spans, &[], Some(&clock));
        let first = &joined.beats[..250];
        let steps: Vec<f64> = first.windows(2).map(|b| b[1].time - b[0].time).collect();
        assert!(
            steps.iter().all(|s| (s - 0.2 * 1.0001).abs() < 1e-9),
            "{:?}",
            &steps[..3]
        );
        let want = joined_duration(&spans, 200.0, Some(&clock));
        assert!(
            (joined.duration_s - want).abs() < 1e-9,
            "{}",
            joined.duration_s
        );
        assert!((want - (140.0 * 1.0001 + JOIN_S)).abs() < 1e-9, "{want}");
        // The join is a cut, at the second stretch's start on the clock.
        assert_eq!(cuts.len(), 1, "{cuts:?}");
        assert!((cuts[0].audio_s - (50.0 * 1.0001 + JOIN_S)).abs() < 1e-9);
    }

    fn manifest(text: &str) -> Manifest {
        toml::from_str(text).unwrap()
    }

    /// A take recorded in one position is read from the end of its
    /// settling, as `session` reads it; one the app saved from its start
    /// (`settle_s = 0`), or one whose entries name no position, whole.
    #[test]
    fn a_take_is_read_from_the_end_of_each_settling() {
        let one = manifest("[[recording]]\nfile = \"a.flac\"\nposition = \"DU\"\n");
        assert_eq!(
            take_spans(&one, 3600.0),
            Some(vec![(take::SETTLE_S, 3600.0)])
        );
        let two = manifest(
            "settle_s = 30.0\n\
             [[recording]]\nfile = \"a.flac\"\nposition = \"DU\"\nend_s = 600.0\n\
             [[recording]]\nfile = \"a.flac\"\nposition = \"CU\"\nstart_s = 600.0\n",
        );
        assert_eq!(
            take_spans(&two, 1200.0),
            Some(vec![(30.0, 600.0), (630.0, 1200.0)])
        );
        let from_start =
            manifest("[[recording]]\nfile = \"a.flac\"\nposition = \"DU\"\nsettle_s = 0.0\n");
        assert_eq!(take_spans(&from_start, 3600.0), None);
        let unnamed = manifest("[[recording]]\nfile = \"a.flac\"\n");
        assert_eq!(take_spans(&unnamed, 3600.0), None);
        let files = manifest(
            "[[recording]]\nfile = \"a.flac\"\nposition = \"DU\"\n\
             [[recording]]\nfile = \"b.flac\"\nposition = \"CU\"\n",
        );
        assert_eq!(take_spans(&files, 3600.0), None);
    }
}
