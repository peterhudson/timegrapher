//! The Power Reserve Test: the watch wound fully and left in one position
//! until it stops. This finds where it stopped and how long it ran, and
//! keeps the run's results in `power-reserve.json` beside the take's
//! standard files (its presence makes the take a Power Reserve take).
//!
//! A beat counts as heard when it matches the beat's sound well (quality
//! over 0.4, as the timing grid takes beats) and sits in a run of at
//! least [`RUN_BEATS`] such beats, each within one and a half beats of
//! the last, so a lone click in a quiet room is not taken for the watch.
//! The run ends at the last beat heard before [`STOP_AFTER_S`] with none;
//! beats that come back sooner join the run, and each such silence of
//! [`PAUSE_S`] or more is listed as a stop on the way. Run time is the
//! true time from the first beat heard to the last, through the take's
//! clock correction.

use crate::beats::Beat;
use serde::{Deserialize, Serialize};

/// The results file's name, in the take's folder.
pub const FILE: &str = "power-reserve.json";
/// Its schema, for readers.
pub const SCHEMA: &str = "power-reserve/1";
/// Seconds without a beat heard that end the run.
pub const STOP_AFTER_S: f64 = 600.0;
/// The longest a test runs, seconds, for a watch that never seems to stop.
pub const LIMIT_S: f64 = 80.0 * 3600.0;
/// Seconds without a beat heard, inside the run, listed as a stop.
pub const PAUSE_S: f64 = 30.0;
/// Good beats in a row for them to count as the watch.
pub const RUN_BEATS: usize = 8;
/// The beat rate a test reads its beats at before one is heard, bph.
pub const UNHEARD_BPH: u32 = 18_000;
/// The least run, seconds of true time from the first beat heard to the
/// last, for a take that isn't a Power Reserve Test to have its run read
/// as one ([`from_long_take`]).
pub const LONG_TAKE_S: f64 = 6.0 * 3600.0;
/// Seconds of sound between readings of a run's hours while it is still
/// being heard: a running test's hours so far, and a long take's run so
/// far while listening.
pub const READ_AGAIN_S: f64 = 60.0;

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ending {
    /// No beat was heard for [`STOP_AFTER_S`] after the last.
    Stopped,
    /// The test was stopped by hand while the watch still ran.
    ByHand,
    /// The test reached [`LIMIT_S`].
    Limit,
    /// A take that wasn't a Power Reserve Test ended while the watch
    /// still ran ([`from_long_take`]).
    TakeEnded,
    /// A take that isn't a Power Reserve Test, read while still being
    /// heard, with no stop found yet ([`STOP_AFTER_S`] of silence not yet
    /// passed; [`Results::still_listening`]): the run so far.
    Listening,
    /// The recording failed (a full disk, say) while the watch still ran:
    /// the run is at least as long as heard, never a stop of the watch.
    RecordingFailed,
}

impl Ending {
    /// How the run ended, as a sentence without its full stop, the same
    /// on every surface: "The watch stopped", "Stopped by hand while the
    /// watch still ran", "Reached the 80 h limit while the watch still
    /// ran".
    pub fn text(self) -> String {
        match self {
            Ending::Stopped => "The watch stopped".to_string(),
            Ending::ByHand => "Stopped by hand while the watch still ran".to_string(),
            Ending::Limit => format!(
                "Reached the {} limit while the watch still ran",
                limit_text()
            ),
            Ending::TakeEnded => "The take ended while the watch still ran".to_string(),
            Ending::Listening => "Still listening, no stop found yet".to_string(),
            Ending::RecordingFailed => "The recording failed while the watch still ran".to_string(),
        }
    }

    /// How the run ended in a word or two, as a plot's title gives it
    /// after the run time ([`title`]): "stopped".
    pub fn short(self) -> String {
        match self {
            Ending::Stopped => "stopped".to_string(),
            Ending::ByHand => "stopped by hand".to_string(),
            Ending::Limit => format!("reached the {} limit", limit_text()),
            Ending::TakeEnded => "take ended".to_string(),
            Ending::Listening => "still listening".to_string(),
            Ending::RecordingFailed => "recording failed".to_string(),
        }
    }
}

/// Where a run's results came from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A Power Reserve Test, kept in its take's [`FILE`].
    #[default]
    Test,
    /// Read from a long take that wasn't one ([`from_long_take`]); never
    /// kept in a file.
    LongTake,
}

impl Source {
    /// Whether the results are a Power Reserve Test's.
    pub fn is_test(&self) -> bool {
        *self == Source::Test
    }

    /// What the results are read from, as words say it, the same on
    /// every surface: none for a test, whose pane and file say so.
    pub fn text(self) -> Option<&'static str> {
        match self {
            Source::Test => None,
            Source::LongTake => Some("Read from a long take, not a Power Reserve Test"),
        }
    }

    /// [`Source::text`] in short, for a chip under a title (#1528).
    pub fn chip(self) -> Option<&'static str> {
        match self {
            Source::Test => None,
            Source::LongTake => Some("From a long take, not a Power Reserve Test"),
        }
    }
}

/// An ended run in one line, as the Power Reserve pane's title gives it
/// (#1528): "Ran 9 h 46 min · stopped", then how many times the beats
/// stopped and came back on the way, when they did ("· restarted 2
/// times"). The run sentence ([`sentence`]) says it in full.
pub fn title(run_s: f64, ending: Ending, source: Source, restarts: usize) -> String {
    let mut line = format!(
        "Ran {} \u{b7} {}",
        run_words(run_s, ending, source),
        ending.short()
    );
    if restarts > 0 {
        line += &format!(" \u{b7} {}", restarted_text(restarts));
    }
    line
}

/// How many times the beats stopped and came back on the way, in a few
/// words: "restarted 2 times".
pub fn restarted_text(restarts: usize) -> String {
    let times = if restarts == 1 {
        "once".to_string()
    } else {
        format!("{restarts} times")
    };
    format!("restarted {times}")
}

/// The longest run, as words say it: "80 h".
pub fn limit_text() -> String {
    format!("{} h", LIMIT_S / 3600.0)
}

/// A silence inside the run, after which the beats came back.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pause {
    /// The last beat before it, audio seconds.
    pub from_s: f64,
    /// The first beat after it, audio seconds.
    pub to_s: f64,
}

/// Where the beats heard so far put the run.
#[derive(Debug, Clone, PartialEq)]
pub struct Heard {
    /// The first and last beats heard, audio seconds; `None` before any.
    pub first_s: Option<f64>,
    pub last_s: Option<f64>,
    /// Silences of [`PAUSE_S`] or more inside the run.
    pub pauses: Vec<Pause>,
}

impl Heard {
    /// Whether the watch has stopped by `now_s` (audio seconds): no beat
    /// heard for [`STOP_AFTER_S`] since the last. Never before a first.
    pub fn stopped(&self, now_s: f64) -> bool {
        self.last_s.is_some_and(|l| now_s - l >= STOP_AFTER_S)
    }

    /// How a test that began at `began_s` has ended by `now_s` (audio
    /// seconds), while it is listening: the watch stopped, or the test
    /// reached [`LIMIT_S`]; `None` while it runs. A test ended before
    /// either is ended by hand ([`Ending::ByHand`]).
    pub fn ending(&self, now_s: f64, began_s: f64) -> Option<Ending> {
        if self.stopped(now_s) {
            Some(Ending::Stopped)
        } else if now_s - began_s >= LIMIT_S {
            Some(Ending::Limit)
        } else {
            None
        }
    }

    /// Seconds since the last beat heard, at `now_s`.
    pub fn silent_s(&self, now_s: f64) -> Option<f64> {
        self.last_s.map(|l| (now_s - l).max(0.0))
    }
}

/// The length of one beat, seconds, at the beat rate heard (`bph`), or
/// at [`UNHEARD_BPH`] before one is: what [`heard`] takes.
pub fn beat_s(bph: Option<u32>) -> f64 {
    3600.0 / f64::from(bph.unwrap_or(UNHEARD_BPH).max(1))
}

/// The beats heard in `beats` (in time order) at `beat_s` seconds a beat.
pub fn heard(beats: &[Beat], beat_s: f64) -> Heard {
    let gap = 1.5 * beat_s;
    let mut out = Heard {
        first_s: None,
        last_s: None,
        pauses: Vec::new(),
    };
    let mut run: Vec<f64> = Vec::new();
    let close = |run: &mut Vec<f64>, out: &mut Heard| {
        if run.len() >= RUN_BEATS {
            let (a, b) = (run[0], run[run.len() - 1]);
            if let Some(l) = out.last_s {
                if a - l >= PAUSE_S {
                    out.pauses.push(Pause { from_s: l, to_s: a });
                }
            }
            out.first_s.get_or_insert(a);
            out.last_s = Some(b);
        }
        run.clear();
    };
    for b in beats.iter().filter(|b| b.is_good()) {
        if run.last().is_some_and(|&l| b.time - l > gap) {
            close(&mut run, &mut out);
        }
        run.push(b.time);
    }
    close(&mut run, &mut out);
    out
}

/// One hour of the run (or its last part hour), as read from the beats.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hour {
    /// Hours since the first beat heard at its start (0, 1, 2, ...).
    pub hour: u32,
    /// Its length, seconds of true time: 3600 but for the last.
    pub span_s: f64,
    pub rate_s_per_day: Option<f64>,
    pub amplitude_deg: Option<f64>,
    /// Signed, Tick less Tock, as the take's readings give it
    /// ([`crate::session::beat_error_signed`]).
    pub beat_error_ms: Option<f64>,
    /// Source of the selected beat error; absent in older saved readings.
    #[serde(default)]
    pub beat_error_source: Option<crate::session::BeatErrorSource>,
    /// Factual conditions carried from the core measurement.
    #[serde(default)]
    pub caveats: Vec<crate::measure::Caveat>,
    /// Seconds of it read, out of `span_s`.
    pub read_s: f64,
    /// The beat rate it was read at, for its BE Angle; absent in older
    /// saved results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bph: Option<u32>,
}

impl Hour {
    /// Its beat error as an angle ([`crate::be_angle`]), from its own
    /// amplitude and beat error at its beat rate, as the table writes
    /// them (no ±). None without an amplitude, a beat error or a rate.
    pub fn be_angle(&self) -> Option<crate::be_angle::Angle> {
        crate::be_angle::Angle::of(
            (self.amplitude_deg?, None),
            (self.beat_error_ms?, None),
            self.bph?,
        )
    }
}

impl Hour {
    /// A whole hour, not the run's last part: its span is 3600 s, within
    /// a second for the sums that make it.
    pub fn whole(&self) -> bool {
        self.span_s >= 3599.0
    }

    /// How much of the hour its whole 10 s readings cover, 0 to 1, or
    /// none when its figures can't be an hour's: no length, a negative
    /// read, or more read than the hour and a second's slack. [`hours`]
    /// never writes those, but a `power-reserve.json` from elsewhere might,
    /// and a made-up 100% or 0% would read as measured (Norm, #1084).
    pub fn read_share(&self) -> Option<f64> {
        (self.span_s > 0.0 && self.read_s >= 0.0 && self.read_s <= self.span_s + 1.0)
            .then(|| (self.read_s / self.span_s).min(1.0))
    }
}

/// A run's results, as `power-reserve.json` keeps them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Results {
    pub schema: String,
    /// When the test started, UTC, RFC 3339.
    pub started_utc: String,
    /// The one position it ran in: DU, DD, CU, CD, CL or CR.
    pub position: String,
    /// Whether the take kept its sound or its analysis only.
    pub sound_kept: bool,
    pub ending: Ending,
    /// The first and last beats heard, audio seconds of the take.
    pub first_beat_s: f64,
    pub last_beat_s: f64,
    /// True time from the first beat heard to the last, seconds.
    pub run_s: f64,
    /// Silences inside the run after which the beats came back.
    #[serde(default)]
    pub pauses: Vec<Pause>,
    /// The run hour by hour; empty until the take's bins are read.
    #[serde(default)]
    pub hours: Vec<Hour>,
    /// What the results are read from; left out for a test.
    #[serde(default, skip_serializing_if = "Source::is_test")]
    pub source: Source,
}

impl Results {
    /// The results of a run whose beats were `heard`, ended as `ending`,
    /// with `to_true` mapping audio seconds to true seconds (the take's
    /// clock fit, or the audio seconds as they are). `None` when no beat
    /// was heard.
    pub fn of(
        heard: &Heard,
        ending: Ending,
        started_utc: &str,
        position: &str,
        sound_kept: bool,
        to_true: impl Fn(f64) -> f64,
    ) -> Option<Results> {
        let (a, b) = (heard.first_s?, heard.last_s?);
        Some(Results {
            schema: SCHEMA.into(),
            started_utc: started_utc.into(),
            position: position.into(),
            sound_kept,
            ending,
            first_beat_s: a,
            last_beat_s: b,
            run_s: to_true(b) - to_true(a),
            pauses: heard.pauses.clone(),
            hours: Vec::new(),
            source: Source::Test,
        })
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("results serialise") + "\n"
    }

    pub fn from_json(text: &str) -> Result<Results, String> {
        let r: Results =
            serde_json::from_str(text).map_err(|e| format!("Couldn't read {FILE}: {e}"))?;
        if r.schema != SCHEMA {
            return Err(format!("{FILE} is {}, not {SCHEMA}", r.schema));
        }
        Ok(r)
    }
}

/// The run from audio second `first_s` to `last_s` of `log`, hour by
/// hour of true time from the first beat (the last hour as long as is
/// left): hour `k` is true seconds `3600 k` to `3600 (k + 1)` on from the
/// first beat's true time, wherever a pause or a card running fast or
/// slow puts it in the sound (#642). Each hour's ends are taken back to
/// the sound's seconds ([`crate::clean::Cuts::audio_time`]) only to pick
/// the sound it holds; an hour wholly inside a pause holds none, so it
/// reads nothing and has no figures. Each hour
/// is measured as `timegrapher session` measures a stretch
/// ([`crate::session::measure_masked`], with the take's `clock`, `breaks`
/// and `masked` stretches), and `read_s` counts its whole 10 s bins
/// ([`crate::clean::Cuts::bins`]), so a mostly cut hour says how little
/// of it was read.
pub fn hours(
    log: &crate::stream::BeatLog,
    clock: Option<&crate::clock::ClockFit>,
    breaks: &[crate::breaks::Break],
    masked: &[(f64, f64)],
    first_s: f64,
    last_s: f64,
) -> Vec<Hour> {
    hours_from(log, clock, breaks, masked, first_s, last_s, 0)
}

/// [`hours`] from hour `from` on (counted from 0 at the first beat), on
/// the same grid of true hours: the hours before it are not measured, so
/// a chart that keeps its whole hours measures only the rest again.
pub fn hours_from(
    log: &crate::stream::BeatLog,
    clock: Option<&crate::clock::ClockFit>,
    breaks: &[crate::breaks::Break],
    masked: &[(f64, f64)],
    first_s: f64,
    last_s: f64,
    from: u32,
) -> Vec<Hour> {
    let cuts = crate::clean::Cuts::new(clock, breaks, masked);
    let on_true = cuts.beats(&log.beats);
    let t0 = cuts.true_time(first_s);
    let t_end = cuts.true_time(last_s);
    let mut out = Vec::new();
    let mut hour = from;
    loop {
        let ta = t0 + 3600.0 * f64::from(hour);
        // A microsecond's slack, so the sums that make the grid don't
        // leave an hour of nothing at the end.
        if ta >= t_end - 1e-6 {
            break;
        }
        let tb = (ta + 3600.0).min(t_end);
        let a = if hour == 0 {
            first_s
        } else {
            cuts.audio_time(ta, first_s, last_s)
        };
        let b = if tb < t_end {
            cuts.audio_time(tb, first_s, last_s)
        } else {
            last_s
        };
        let n = ((tb - ta) / 10.0).floor() as usize;
        let read_s = cuts
            .bins(&on_true, ta, 10.0, n, 2.0)
            .iter()
            .filter(|x| x.whole)
            .count() as f64
            * 10.0;
        let m = (b > a).then(|| crate::session::measure_masked(log, clock, breaks, masked, a, b));
        out.push(Hour {
            hour,
            span_s: tb - ta,
            rate_s_per_day: m.as_ref().and_then(|m| m.rate_s_per_day),
            amplitude_deg: m.as_ref().and_then(|m| m.amplitude_deg),
            beat_error_ms: m.as_ref().and_then(|m| m.beat_error_signed()),
            beat_error_source: m.as_ref().and_then(|m| m.beat_error_source),
            caveats: m.as_ref().map(|m| m.caveats.clone()).unwrap_or_default(),
            read_s,
            bph: Some(log.bph),
        });
        hour += 1;
    }
    out
}

/// The run of a take that wasn't a Power Reserve Test, read as one: its
/// beats heard ([`heard`], at the take's beat rate), the run from the
/// first to the last on `clock`, ended [`Ending::Stopped`] when the take
/// goes on [`STOP_AFTER_S`] past the last beat, else
/// [`Ending::TakeEnded`], and hour by hour ([`hours`], on the take's
/// `clock`, `breaks` and `masked` stretches). `positions` are the ones the
/// take lists. `None` for a take in more than one position (its hours would
/// mix them), with no beat heard, or whose run is shorter than
/// [`LONG_TAKE_S`]. The take's start is not known to be at full wind, so
/// the results say where they came from ([`Source::LongTake`]).
pub fn from_long_take(
    log: &crate::stream::BeatLog,
    clock: Option<&crate::clock::ClockFit>,
    breaks: &[crate::breaks::Break],
    masked: &[(f64, f64)],
    positions: &[&str],
    started_utc: &str,
    sound_kept: bool,
) -> Option<Results> {
    let position = match positions.split_first() {
        None => "",
        Some((p, rest)) if rest.iter().all(|q| q == p) => p,
        Some(_) => return None,
    };
    let heard = heard(&log.beats, beat_s(Some(log.bph)));
    let ending = if heard.stopped(log.duration_s) {
        Ending::Stopped
    } else {
        Ending::TakeEnded
    };
    let mut r = Results::of(&heard, ending, started_utc, position, sound_kept, |s| {
        clock.map_or(s, |c| c.map(s))
    })?;
    if r.run_s < LONG_TAKE_S {
        return None;
    }
    r.source = Source::LongTake;
    r.hours = hours(log, clock, breaks, masked, r.first_beat_s, r.last_beat_s);
    Some(r)
}

/// A run time in words ([`run_text`] of `run_s`), "at least" for a long
/// take that ended, or is still being heard, before the watch stopped, or
/// a run whose recording failed: "41 h 12 min", "at least 65 h 03 min".
pub fn run_words(run_s: f64, ending: Ending, source: Source) -> String {
    let run = run_text(run_s);
    if (source == Source::LongTake && matches!(ending, Ending::TakeEnded | Ending::Listening))
        || ending == Ending::RecordingFailed
    {
        format!("at least {run}")
    } else {
        run
    }
}

/// A run in sentences, with `position` as the surface names it, the same
/// on every surface: "Power reserve 41 h 12 min in DU. The watch
/// stopped." for a test. A long take, not known to start fully wound, has
/// its run named a run from the take's start, never a power reserve, and
/// says where it came from.
pub fn sentence(run_s: f64, ending: Ending, source: Source, position: &str) -> String {
    let run = run_words(run_s, ending, source);
    match source.text() {
        None => format!("Power reserve {run} in {position}. {}.", ending.text()),
        Some(from) => format!(
            "Ran {run} in {position} from the take's start. {}. {from}.",
            ending.text()
        ),
    }
}

impl Results {
    /// [`run_words`] of this run.
    pub fn run_words(&self) -> String {
        run_words(self.run_s, self.ending, self.source)
    }

    /// What the run time is, as a table's row names it: "Power reserve"
    /// for a test; for a long take, not known to start fully wound, "Ran
    /// from the take's start".
    pub fn run_label(&self) -> &'static str {
        match self.source {
            Source::Test => "Power reserve",
            Source::LongTake => "Ran from the take's start",
        }
    }

    /// The run as a line begins: "Ran 41 h 12 min" for a test, "Ran at
    /// least 65 h 03 min from the take's start" for a long take.
    pub fn ran(&self) -> String {
        match self.source {
            Source::Test => format!("Ran {}", self.run_words()),
            Source::LongTake => format!("Ran {} from the take's start", self.run_words()),
        }
    }

    /// [`sentence`] of this run.
    pub fn sentence(&self, position: &str) -> String {
        sentence(self.run_s, self.ending, self.source, position)
    }

    /// A long take's run read while the take is still being heard: one
    /// whose watch still ran at its last sound ([`Ending::TakeEnded`])
    /// is a run so far ([`Ending::Listening`]), since the take hasn't
    /// ended. A stop already found stands.
    pub fn still_listening(mut self) -> Results {
        if self.ending == Ending::TakeEnded {
            self.ending = Ending::Listening;
        }
        self
    }
}

/// A run in brief, as a take's listing gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub run_s: f64,
    pub ending: Ending,
    pub position: String,
}

impl Results {
    pub fn summary(&self) -> Summary {
        Summary {
            run_s: self.run_s,
            ending: self.ending,
            position: self.position.clone(),
        }
    }
}

/// Write `r` into the take `dir` as [`FILE`], through a temporary file
/// and a rename, so a reader never sees half of it.
pub fn write(dir: &std::path::Path, r: &Results) -> std::io::Result<()> {
    let tmp = dir.join(format!(".{FILE}.part"));
    std::fs::write(&tmp, r.to_json())?;
    crate::recorder::rename_settled(&tmp, &dir.join(FILE))
}

/// The whole seconds a run time counts, so every place that gives it
/// agrees with its words and none reads a second over.
pub fn whole_s(s: f64) -> f64 {
    s.max(0.0).floor()
}

/// A run time in words: hours and minutes from an hour ("46 h 12 min"),
/// minutes and seconds under it ("1 min 52 s"), seconds under a minute
/// ("13 s"), so a short run never reads a minute short.
pub fn run_text(s: f64) -> String {
    let s = whole_s(s);
    if s < 60.0 {
        format!("{} s", s as u64)
    } else if s < 3600.0 {
        let s = s as u64;
        format!("{} min {:02} s", s / 60, s % 60)
    } else {
        let m = (s / 60.0).floor() as i64;
        format!("{} h {:02} min", m / 60, m % 60)
    }
}

#[cfg(test)]
mod title_tests {
    use super::*;

    /// The title gives the run and its ending in short, and the restarts
    /// only when there were any (#1528).
    #[test]
    fn the_title_gives_the_run_and_its_ending_in_short() {
        let s = 9.0 * 3600.0 + 46.0 * 60.0;
        assert_eq!(
            title(s, Ending::Stopped, Source::Test, 0),
            "Ran 9 h 46 min \u{b7} stopped"
        );
        assert_eq!(
            title(s, Ending::Stopped, Source::LongTake, 2),
            "Ran 9 h 46 min \u{b7} stopped \u{b7} restarted 2 times"
        );
        assert!(title(s, Ending::TakeEnded, Source::LongTake, 1)
            .starts_with("Ran at least 9 h 46 min \u{b7} take ended \u{b7} restarted once"));
        assert_eq!(
            Ending::Limit.short(),
            format!("reached the {} limit", limit_text())
        );
        assert_eq!(Source::Test.chip(), None);
        assert!(Source::LongTake.chip().is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One beat lasts 3600 / bph seconds, at the rate heard or, before
    /// one is, at the unheard rate.
    #[test]
    fn a_beat_lasts_an_hour_over_the_beat_rate() {
        assert_eq!(beat_s(Some(28_800)), 0.125);
        assert_eq!(beat_s(None), 3600.0 / f64::from(UNHEARD_BPH));
        assert!(beat_s(Some(0)).is_finite());
    }

    #[test]
    fn an_ending_reads_the_same_everywhere() {
        assert_eq!(Ending::Stopped.text(), "The watch stopped");
        assert_eq!(
            Ending::ByHand.text(),
            "Stopped by hand while the watch still ran"
        );
        assert_eq!(
            Ending::Limit.text(),
            "Reached the 80 h limit while the watch still ran"
        );
        assert_eq!(limit_text(), "80 h");
        assert_eq!(
            Ending::RecordingFailed.text(),
            "The recording failed while the watch still ran"
        );
    }

    /// A run whose recording failed is at least as long as heard, and its
    /// file names how it ended (#1380).
    #[test]
    fn a_run_whose_recording_failed_reads_at_least() {
        assert_eq!(
            sentence(7260.0, Ending::RecordingFailed, Source::Test, "DU"),
            "Power reserve at least 2 h 01 min in DU. The recording failed while the watch \
             still ran."
        );
        let v: serde_json::Value = serde_json::to_value(Ending::RecordingFailed).unwrap();
        assert_eq!(v, "recording_failed");
    }

    fn beats(from: f64, to: f64, step: f64, quality: f32) -> Vec<Beat> {
        let n = ((to - from) / step).round() as i64;
        (0..n)
            .map(|i| Beat {
                index: i,
                time: from + i as f64 * step,
                quality,
            })
            .collect()
    }

    /// A take of `run_s` of beats at 18,000 bph from 5 s in, then
    /// `after_s` of silence.
    fn take_of(run_s: f64, after_s: f64) -> crate::stream::BeatLog {
        crate::stream::BeatLog {
            sample_rate: 48_000,
            duration_s: 5.0 + run_s + after_s,
            bph: 18_000,
            lift_deg: 52.0,
            beats: beats(5.0, 5.0 + run_s, 0.2, 1.0),
            amplitude_windows: vec![],
            clipped_beats: vec![],
            clipped_samples: 0,
            jumps: vec![],
            sided: vec![],
            by_count: vec![],
            snippets: vec![],
        }
    }

    /// A take of six hours or more in one position is read as a run, the
    /// stop found as a test finds it, and says where it came from.
    #[test]
    fn a_long_take_in_one_position_is_read_as_a_run() {
        let run = LONG_TAKE_S + 1800.0;
        let still = take_of(run, 60.0);
        let r = from_long_take(
            &still,
            None,
            &[],
            &[],
            &["DU"],
            "2026-11-01T00:00:00Z",
            true,
        )
        .expect("a long take");
        assert_eq!(r.source, Source::LongTake);
        assert_eq!(r.ending, Ending::TakeEnded);
        assert_eq!(r.position, "DU");
        assert!((r.run_s - (run - 0.2)).abs() < 1e-6, "{}", r.run_s);
        assert_eq!(r.hours.len(), 7, "six whole hours and the half left");
        assert!(r.hours[..6].iter().all(Hour::whole));
        let stopped = take_of(run, STOP_AFTER_S + 1.0);
        let r = from_long_take(&stopped, None, &[], &[], &["DU", "DU"], "", false).unwrap();
        assert_eq!(r.ending, Ending::Stopped);
        assert!(!r.sound_kept);
        // No position listed: the run is still read, its position unnamed.
        assert_eq!(
            from_long_take(&stopped, None, &[], &[], &[], "", true).map(|r| r.position),
            Some(String::new())
        );
    }

    /// A long take's settling, left out as a mask is
    /// ([`crate::session::with_holes`]), is not read in its first hour, and
    /// the hours still count from the first beat heard (#1233).
    #[test]
    fn a_settling_is_left_out_of_the_first_hour_not_the_run() {
        let take = take_of(LONG_TAKE_S + 1800.0, 60.0);
        let read = |masked: &[(f64, f64)]| {
            from_long_take(&take, None, &[], masked, &["DU"], "", true).unwrap()
        };
        let (whole, settled) = (
            read(&[]),
            read(&crate::session::with_holes(&[], &[(0.0, 125.0)])),
        );
        assert_eq!(settled.first_beat_s, whole.first_beat_s);
        assert_eq!(settled.run_s, whole.run_s);
        assert_eq!(settled.hours.len(), whole.hours.len());
        let lost = whole.hours[0].read_s - settled.hours[0].read_s;
        assert!(lost >= 110.0, "{lost}");
        assert_eq!(settled.hours[1].read_s, whole.hours[1].read_s);
    }

    /// A take shorter than [`LONG_TAKE_S`], or in more than one position,
    /// or with no beat heard, has no run.
    #[test]
    fn a_short_or_moving_take_has_no_run() {
        let short = take_of(LONG_TAKE_S - 60.0, 0.0);
        assert_eq!(
            from_long_take(&short, None, &[], &[], &["DU"], "", true),
            None
        );
        let long = take_of(LONG_TAKE_S + 60.0, 0.0);
        assert!(from_long_take(&long, None, &[], &[], &["DU"], "", true).is_some());
        assert_eq!(
            from_long_take(&long, None, &[], &[], &["DU", "CU"], "", true),
            None
        );
        let mut silent = take_of(LONG_TAKE_S + 60.0, 0.0);
        silent.beats.clear();
        assert_eq!(
            from_long_take(&silent, None, &[], &[], &["DU"], "", true),
            None
        );
    }

    /// A test's file is as it was, with no `source`; a long take's run
    /// says it was read from one, and reads back.
    #[test]
    fn only_a_long_take_names_its_source() {
        let long = take_of(LONG_TAKE_S + 60.0, 0.0);
        let mut r = from_long_take(&long, None, &[], &[], &["DU"], "", true).unwrap();
        let v: serde_json::Value = serde_json::from_str(&r.to_json()).unwrap();
        assert_eq!(v["source"], "long_take");
        assert_eq!(v["ending"], "take_ended");
        let back = Results::from_json(&r.to_json()).unwrap();
        assert_eq!(
            (back.source, back.ending),
            (Source::LongTake, Ending::TakeEnded)
        );
        r.source = Source::Test;
        let v: serde_json::Value = serde_json::from_str(&r.to_json()).unwrap();
        assert!(v.get("source").is_none());
        assert_eq!(Source::Test.text(), None);
        assert_eq!(
            Source::LongTake.text(),
            Some("Read from a long take, not a Power Reserve Test")
        );
        assert_eq!(
            Ending::TakeEnded.text(),
            "The take ended while the watch still ran"
        );
    }

    /// A long take's run read while still being heard is a run so far,
    /// "at least" and never "the take ended"; a stop already found stands.
    #[test]
    fn a_long_take_still_heard_reads_as_a_run_so_far() {
        let long = take_of(LONG_TAKE_S + 60.0, 0.0);
        let r = from_long_take(&long, None, &[], &[], &["DU"], "", true)
            .unwrap()
            .still_listening();
        assert_eq!(r.ending, Ending::Listening);
        assert_eq!(
            r.sentence("DU"),
            format!(
                "Ran at least {} in DU from the take's start. Still listening, no stop \
                 found yet. Read from a long take, not a Power Reserve Test.",
                run_text(r.run_s)
            )
        );
        let stopped = take_of(LONG_TAKE_S + 60.0, STOP_AFTER_S + 60.0);
        let r = from_long_take(&stopped, None, &[], &[], &["DU"], "", true).unwrap();
        assert_eq!(r.ending, Ending::Stopped);
        assert_eq!(r.still_listening().ending, Ending::Stopped);
    }

    /// A long take's run is named a run from the take's start, "at least"
    /// where the take ended first, and never a power reserve; a test's
    /// reads as it did.
    #[test]
    fn only_a_test_names_its_run_a_power_reserve() {
        let long = take_of(LONG_TAKE_S + 60.0, 0.0);
        let mut r = from_long_take(&long, None, &[], &[], &["DU"], "", true).unwrap();
        r.run_s = 65.0 * 3600.0 + 180.0;
        assert_eq!(r.run_words(), "at least 65 h 03 min");
        assert_eq!(r.ran(), "Ran at least 65 h 03 min from the take's start");
        assert_eq!(r.run_label(), "Ran from the take's start");
        assert_eq!(
            r.sentence("DU"),
            "Ran at least 65 h 03 min in DU from the take's start. The take ended while \
             the watch still ran. Read from a long take, not a Power Reserve Test."
        );
        r.ending = Ending::Stopped;
        assert_eq!(
            r.sentence("DU"),
            "Ran 65 h 03 min in DU from the take's start. The watch stopped. Read from a \
             long take, not a Power Reserve Test."
        );
        r.source = Source::Test;
        assert_eq!(r.ran(), "Ran 65 h 03 min");
        assert_eq!(r.run_label(), "Power reserve");
        assert_eq!(
            r.sentence("DU"),
            "Power reserve 65 h 03 min in DU. The watch stopped."
        );
        for surface in [r.sentence("DU"), r.ran()] {
            assert!(!surface.contains("at least"));
        }
        r.source = Source::LongTake;
        assert!(!r.sentence("DU").contains("Power reserve"));
        assert!(!r.ran().contains("Power reserve"));
    }

    #[test]
    fn the_run_ends_at_the_last_beat_before_ten_minutes_of_silence() {
        let step = 0.125;
        let mut b = beats(2.0, 100.0, step, 1.0);
        // A 40 s stop on the way, then the watch runs on to 200 s.
        b.extend(beats(140.0, 200.0, step, 1.0));
        // A lone click and a short burst of noise later: not the watch.
        b.push(Beat {
            index: 0,
            time: 400.0,
            quality: 0.9,
        });
        b.extend(beats(500.0, 500.5, step, 0.9));
        let h = heard(&b, step);
        assert_eq!(h.first_s, Some(2.0));
        let last = 200.0 - step;
        assert!((h.last_s.unwrap() - last).abs() < 1e-9);
        assert_eq!(h.pauses.len(), 1);
        assert!((h.pauses[0].to_s - 140.0).abs() < 1e-9);
        assert!(!h.stopped(last + STOP_AFTER_S - 1.0));
        assert!(h.stopped(last + STOP_AFTER_S));
        assert!(!heard(&[], step).stopped(1e6), "never before a first beat");
    }

    #[test]
    fn a_test_ends_when_the_watch_stops_or_at_the_limit() {
        let step = 0.125;
        let h = heard(&beats(100.0, 200.0, step, 1.0), step);
        let last = h.last_s.unwrap();
        // Begun at 100 s: running, then stopped ten minutes after the last beat.
        assert_eq!(h.ending(last + STOP_AFTER_S - 1.0, 100.0), None);
        assert_eq!(h.ending(last + STOP_AFTER_S, 100.0), Some(Ending::Stopped));
        // Still beating at the limit, counted from when it began.
        let h = heard(&beats(100.0, 300.0, step, 1.0), step);
        let n = 300.0 - step;
        assert_eq!(h.ending(n, n - LIMIT_S + 1.0), None);
        assert_eq!(h.ending(n, n - LIMIT_S), Some(Ending::Limit));
    }

    #[test]
    fn poor_matches_are_not_heard_and_short_gaps_are_no_stop() {
        let step = 0.2;
        let mut b = beats(0.0, 10.0, step, 1.0);
        b.extend(beats(10.0, 20.0, step, 0.3));
        b.extend(beats(25.0, 30.0, step, 1.0));
        let h = heard(&b, step);
        assert!((h.last_s.unwrap() - (30.0 - step)).abs() < 1e-9);
        assert!(h.pauses.is_empty(), "15 s is under {PAUSE_S} s");
    }

    /// An hour of a run whose unlock is never found gives its beat error
    /// from the drop, as `session` reads the stretch.
    #[test]
    fn an_hour_whose_unlock_is_never_found_gives_the_drops_beat_error() {
        let audio = crate::synth::unlock_unheard(60.0, 0.8);
        let log = crate::stream::analyze_samples(
            &audio.samples,
            audio.sample_rate,
            &crate::stream::StreamConfig::default(),
        );
        let (first, last) = (log.beats[0].time, log.beats[log.beats.len() - 1].time);
        let h = hours(&log, None, &[], &[], first, last);
        let m = crate::session::measure_masked(&log, None, &[], &[], first, last);
        assert_eq!(m.beat_error_unlock_ms, None);
        assert!(m.beat_error_ms.is_some());
        assert_eq!(h[0].beat_error_ms, m.beat_error_ms);
    }

    /// Hours are cut on true time: a listed 30 min pause half an hour in
    /// makes the first hour 30 min of sound, not 60, and every whole hour
    /// spans 3600 s of true time (#642).
    #[test]
    fn hours_are_cut_on_true_time_across_a_pause() {
        let p = 0.125;
        let log = crate::stream::BeatLog {
            sample_rate: 48_000,
            duration_s: 5400.0,
            bph: 28_800,
            lift_deg: 52.0,
            beats: beats(0.0, 5400.0, p, 1.0),
            amplitude_windows: Vec::new(),
            clipped_beats: Vec::new(),
            clipped_samples: 0,
            jumps: Vec::new(),
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        };
        let pause = [crate::breaks::Break {
            audio_s: 1800.0,
            gap_s: 1800.0,
        }];
        let h = hours(&log, None, &pause, &[], 0.0, 5000.0);
        let spans: Vec<f64> = h.iter().map(|x| x.span_s).collect();
        assert_eq!(h.len(), 2, "{spans:?}");
        assert!((h[0].span_s - 3600.0).abs() < 1e-3, "{spans:?}");
        assert!(h[0].whole() && !h[1].whole(), "{spans:?}");
        let total: f64 = spans.iter().sum();
        assert!((total - (5000.0 + 1800.0)).abs() < 1e-3, "{spans:?}");
    }

    /// A run of `duration_s` seconds of beats, every one heard.
    fn steady_log(duration_s: f64) -> crate::stream::BeatLog {
        crate::stream::BeatLog {
            sample_rate: 48_000,
            duration_s,
            bph: 28_800,
            lift_deg: 52.0,
            beats: beats(0.0, duration_s, 0.125, 1.0),
            amplitude_windows: Vec::new(),
            clipped_beats: Vec::new(),
            clipped_samples: 0,
            jumps: Vec::new(),
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        }
    }

    /// A pause that ends inside an hour, or covers whole hours, keeps
    /// every hour on the grid of true hours from the first beat: an hour
    /// wholly inside the pause is kept, reading nothing and with no
    /// figures (#642; the outside review of #876).
    #[test]
    fn hours_keep_the_true_grid_across_a_pause_of_any_length() {
        let log = steady_log(5000.0);
        for (gap_s, want) in [
            (2700.0, vec![3600.0, 3600.0, 500.0]),
            (7200.0, vec![3600.0, 3600.0, 3600.0, 1400.0]),
        ] {
            let pause = [crate::breaks::Break {
                audio_s: 1800.0,
                gap_s,
            }];
            let h = hours(&log, None, &pause, &[], 0.0, 5000.0);
            let spans: Vec<f64> = h.iter().map(|x| x.span_s).collect();
            assert_eq!(spans.len(), want.len(), "gap {gap_s}: {spans:?}");
            for (got, want) in spans.iter().zip(&want) {
                assert!((got - want).abs() < 1e-3, "gap {gap_s}: {spans:?}");
            }
            assert!(h[0].rate_s_per_day.is_some(), "{:?}", h[0]);
            if gap_s > 3600.0 {
                assert_eq!(h[1].read_s, 0.0, "{:?}", h[1]);
                assert_eq!(h[1].rate_s_per_day, None, "{:?}", h[1]);
                assert_eq!(h[1].amplitude_deg, None, "{:?}", h[1]);
            }
        }
    }

    /// A card running fast or slow by a steady amount makes each whole
    /// hour 3600 s of true time, and the run its true length.
    #[test]
    fn hours_are_true_hours_on_a_card_running_fast_or_slow() {
        let log = steady_log(7500.0);
        for ppm in [-50.0, 0.0, 50.0] {
            let clock = crate::clock::ClockFit::fixed(ppm, log.duration_s);
            let h = hours(&log, Some(&clock), &[], &[], 2.375, 7402.375);
            assert_eq!(h.len(), 3, "{ppm}");
            assert!((h[0].span_s - 3600.0).abs() < 1e-3, "{ppm}: {h:?}");
            assert!((h[1].span_s - 3600.0).abs() < 1e-3, "{ppm}: {h:?}");
            let total: f64 = h.iter().map(|x| x.span_s).sum();
            assert!(
                (total - 7400.0 * crate::clock::card_scale(ppm)).abs() < 1e-3,
                "{ppm}"
            );
        }
    }

    /// Hours from a later hour on are those same hours of the whole run,
    /// so a chart that keeps its whole hours measures the rest on the
    /// same grid.
    #[test]
    fn hours_from_a_later_hour_are_the_whole_runs() {
        let log = steady_log(9000.0);
        let pause = [crate::breaks::Break {
            audio_s: 1800.0,
            gap_s: 2700.0,
        }];
        let all = hours(&log, None, &pause, &[], 0.0, 9000.0);
        for from in 0..=all.len() as u32 {
            let rest = hours_from(&log, None, &pause, &[], 0.0, 9000.0, from);
            assert_eq!(rest, all[from as usize..], "from hour {from}");
        }
    }

    #[test]
    fn a_run_is_read_hour_by_hour_as_session_reads_a_stretch() {
        use crate::synth::{generate, SynthConfig};
        let cfg = SynthConfig {
            duration_s: 300.0,
            rate_s_per_day: 6.0,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 250.0, |_| 0.0);
        let log = crate::stream::analyze_samples(
            &audio.samples,
            audio.sample_rate,
            &crate::stream::StreamConfig::default(),
        );
        let first = log.beats[0].time;
        let last = log.beats[log.beats.len() - 1].time;
        // A short run, one part hour, read as the stretch is measured,
        // with a mask leaving its bins out.
        let h = hours(&log, None, &[], &[(150.0, 170.0)], first, last);
        assert_eq!(h.len(), 1, "a run under an hour is one part hour");
        let m = crate::session::measure_masked(&log, None, &[], &[(150.0, 170.0)], first, last);
        assert_eq!(h[0].rate_s_per_day, m.rate_s_per_day);
        assert_eq!(h[0].amplitude_deg, m.amplitude_deg);
        assert!((h[0].rate_s_per_day.unwrap() - 6.0).abs() < 1.0, "{h:?}");
        assert!((h[0].span_s - (last - first)).abs() < 1e-9);
        // The masked 20 s and the bins it touches are not read.
        assert!(h[0].read_s <= h[0].span_s - 20.0, "{h:?}");
        assert!(h[0].read_s >= h[0].span_s - 50.0, "{h:?}");
    }

    #[test]
    fn results_keep_true_time_and_read_back() {
        let h = Heard {
            first_s: Some(10.0),
            last_s: Some(10.0 + 166_320.0),
            pauses: vec![],
        };
        let r = Results::of(
            &h,
            Ending::Stopped,
            "2026-10-22T09:15:00Z",
            "DU",
            false,
            |s| s * (1.0 + 10e-6),
        )
        .unwrap();
        assert!((r.run_s - 166_320.0 * 1.00001).abs() < 1e-6);
        assert_eq!(run_text(166_320.0), "46 h 12 min");
        assert_eq!(Results::from_json(&r.to_json()).unwrap(), r);
        assert!(r.to_json().contains("\"ending\": \"stopped\""));
        assert!(Results::of(&heard(&[], 0.2), Ending::ByHand, "", "DU", true, |s| s).is_none());
    }

    /// A run under a minute reads its seconds, under an hour its minutes
    /// and seconds, never a minute short; an hour or more, hours and
    /// minutes.
    #[test]
    fn a_short_run_reads_its_seconds() {
        assert_eq!(run_text(112.4), "1 min 52 s");
        assert_eq!(run_text(59.0), "59 s");
        assert_eq!(run_text(13.7), "13 s");
        assert_eq!(run_text(3600.0), "1 h 00 min");
        assert_eq!(run_text(166_320.0), "46 h 12 min");
    }

    /// Whole seconds are counted down, as the words count them, and an
    /// hour is whole within a second of 3600 s.
    #[test]
    fn whole_seconds_and_whole_hours() {
        assert_eq!(whole_s(113.6), 113.0);
        assert_eq!(whole_s(-2.0), 0.0);
        let hour = |span_s| Hour {
            hour: 0,
            span_s,
            rate_s_per_day: None,
            amplitude_deg: None,
            beat_error_ms: None,
            beat_error_source: None,
            caveats: Vec::new(),
            read_s: 0.0,
            bph: Some(28_800),
        };
        assert!(hour(3600.0).whole());
        assert!(hour(3599.4).whole());
        assert!(!hour(3598.0).whole());
    }

    /// An hour's share read is between none and the whole hour, and an
    /// hour whose figures can't be an hour's has none to give.
    #[test]
    fn an_hours_share_read_is_between_none_and_all() {
        let hour = |span_s: f64, read_s: f64| Hour {
            hour: 0,
            span_s,
            rate_s_per_day: None,
            amplitude_deg: None,
            beat_error_ms: None,
            beat_error_source: None,
            caveats: Vec::new(),
            read_s,
            bph: Some(28_800),
        };
        assert_eq!(hour(3600.0, 3500.0).read_share(), Some(3500.0 / 3600.0));
        // Within a second of the hour: the whole of it.
        assert_eq!(hour(3600.0, 3600.5).read_share(), Some(1.0));
        // Past the second's slack: none.
        assert_eq!(hour(3600.0, 3602.0).read_share(), None);
        assert_eq!(hour(1800.0, 3500.0).read_share(), None);
        assert_eq!(hour(3600.0, -10.0).read_share(), None);
        assert_eq!(hour(0.0, 10.0).read_share(), None);
    }
}
