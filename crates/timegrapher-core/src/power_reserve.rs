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
/// A beat's quality over this is a good match to the beat's sound.
const GOOD: f32 = 0.4;

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

    /// Seconds since the last beat heard, at `now_s`.
    pub fn silent_s(&self, now_s: f64) -> Option<f64> {
        self.last_s.map(|l| (now_s - l).max(0.0))
    }
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
    for b in beats.iter().filter(|b| b.quality > GOOD) {
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
    /// Signed, Tick less Tock, as the take's readings give it.
    pub beat_error_ms: Option<f64>,
    /// Seconds of it read, out of `span_s`.
    pub read_s: f64,
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
    std::fs::rename(&tmp, dir.join(FILE))
}

/// A run time as hours and minutes: "46 h 12 min".
pub fn run_text(s: f64) -> String {
    let m = (s / 60.0).floor() as i64;
    format!("{} h {:02} min", m / 60, m % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn poor_matches_are_not_heard_and_short_gaps_are_no_stop() {
        let step = 0.2;
        let mut b = beats(0.0, 10.0, step, 1.0);
        b.extend(beats(10.0, 20.0, step, 0.3));
        b.extend(beats(25.0, 30.0, step, 1.0));
        let h = heard(&b, step);
        assert!((h.last_s.unwrap() - (30.0 - step)).abs() < 1e-9);
        assert!(h.pauses.is_empty(), "15 s is under {PAUSE_S} s");
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
}
