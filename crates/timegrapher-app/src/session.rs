//! A session's life: listening, paused, the positions the watch was put
//! in, the settling after each move, and what has been saved. The app asks
//! the session to act and draws what it says; the stretches, the settling,
//! the unsaved sound and the saved take's position ranges are all worked
//! out from its events and recording timeline.
//!
//! Event times are the session's seconds, the clock the strip and the
//! charts run on (it counts pauses). A move also keeps the seconds of
//! saved sound at that moment. Pause and resume anchors map elapsed
//! seconds to saved sound, which leaves pauses out, for `session.toml`.
//!
//! What each state allows ([`Session::allows`]):
//!
//! | State          | Pause | Resume | Move | Save | Stop |
//! |----------------|-------|--------|------|------|------|
//! | Idle           |       |        |      |      |      |
//! | Listening      | yes   |        | yes  | yes  | yes  |
//! | Paused         |       | yes    | yes  | yes  |      |
//! | Replaying      |       |        | yes  |      | yes  |
//! | ReplayStopped  |       |        |      |      |      |
//! | Analysing      |       |        |      |      | yes  |
//! | Analysed       |       |        |      |      |      |
//!
//! Listen, Open and New Session are allowed in every state: each stops
//! what is under way first. Replaying and ReplayStopped are reached only
//! from tests, which play a recording through as if live. Save needs the sound kept as well. A recording
//! fault overrides Resume, Move and Save until the recorder is released; its
//! files are retained for manual recovery rather than treated as a valid take.

use crate::app::Position;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;
use timegrapher_core::recorder::Recorder;

/// A layer of positions: the changes, oldest first, and the settling.
pub type Layer = (Vec<(f64, Position)>, Vec<(f64, f64)>);

/// A stretch in one position as a session file lists it: start, end and
/// the position's code.
pub type Range = (Option<f64>, Option<f64>, String);

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The watch is in `pos` from `at_s`; `audio_s` is the saved sound's
    /// length then, when the session is being kept.
    Position {
        at_s: f64,
        audio_s: Option<f64>,
        pos: Position,
    },
    /// The watch settling after a move: nothing is read from `from_s` to
    /// `to_s`.
    Settle { from_s: f64, to_s: f64 },
    /// A copy of the sound up to `through_s` seconds of it was saved in
    /// `to`.
    Saved { through_s: f64, to: PathBuf },
}

/// User-controlled inputs to saved metadata. Audio-derived duration, detected
/// Auto BPH and headline readings are deliberately absent: this tracks edits,
/// not byte equality with a snapshot taken before more sound arrived.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Metadata {
    pub watch: String,
    pub calibre: String,
    pub position: String,
    pub bph: Option<u32>,
    pub lift_deg: f64,
    pub lift_known: bool,
    pub wheels: std::collections::BTreeMap<String, f64>,
    pub guided_ranges: Vec<(Position, f64, f64)>,
    pub guided_wind_h: Option<f64>,
    pub full_wind: Option<crate::guided::FullWind>,
    /// Stretches masked out on the strip, in the session's seconds.
    pub masked: Vec<(f64, f64)>,
}

/// The metadata inputs captured before a save transaction begins.
pub struct MetadataSnapshot {
    key: Metadata,
    events: Vec<Event>,
    notes_revision: u64,
}

/// A recording whose last write/close may be incomplete. Keep its files for
/// manual recovery; it cannot safely resume or replace a known-good saved take.
#[derive(Debug, Clone)]
pub struct RecordingFault {
    pub reason: String,
    pub recovery_dir: PathBuf,
    pub marker_error: Option<String>,
}

impl RecordingFault {
    pub fn description(&self) -> String {
        let mut text = format!(
            "Recording stopped: {}. Remaining files may be incomplete and need manual recovery: {}. \
             This session cannot Resume or Save. New Session or quitting leaves these files on disk.",
            self.reason, self.recovery_dir.display()
        );
        if let Some(error) = &self.marker_error {
            text.push_str(&format!(
                " Couldn't write the recording-error marker: {error}."
            ));
        }
        text
    }
}

/// Where a session is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum State {
    /// Nothing heard yet, or a new session.
    #[default]
    Idle,
    /// Listening to the microphone.
    Listening,
    /// A microphone session let go for now, since then: it can carry on
    /// or be saved.
    Paused { since: Instant },
    /// A recording played through at its own pace. The app offers no
    /// replay; tests use one to feed the live path from a file.
    #[cfg_attr(not(test), allow(dead_code))]
    Replaying,
    /// A replay at its end, or stopped.
    ReplayStopped,
    /// A recording or a take being analysed whole.
    Analysing,
    /// A recording or a take analysed whole.
    Analysed,
}

/// The actions whose state matters; see the module's table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Pause,
    Resume,
    Move,
    Save,
    Stop,
}

/// An action the session's state doesn't allow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Refused {
    pub action: Action,
    pub state: State,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} isn't possible while {:?}", self.action, self.state)
    }
}

/// An elapsed-time boundary, anchored to the actual recorded sample count.
struct AudioAnchor {
    at_s: f64,
    audio_s: f64,
    advancing: bool,
}

/// The shortest stretch in one position a change can be dragged to leave.
pub const MIN_STRETCH_S: f64 = 0.5;

#[derive(Default)]
pub struct Session {
    state: State,
    /// The sound kept as it comes in, for saving.
    pub recorder: Option<Recorder>,
    /// Moved while paused: the settling starts on Resume.
    settle_pending: bool,
    /// The end of the settling under way, when the readings start afresh.
    settle_to: Option<f64>,
    /// How long the watch settles after every change of position: the
    /// Positions Test pane's Settle, for free takes and guided tests alike.
    settle_s: Option<f64>,
    events: Vec<Event>,
    /// The positions and settling as recorded, kept under what the
    /// Position Brush painted over them: `None` while nothing painted
    /// shows, when they are the positions in `events`.
    recorded: Option<Vec<Event>>,
    audio_timeline: Vec<AudioAnchor>,
    saved_metadata: Option<MetadataSnapshot>,
    notes_revision: u64,
    recording_fault: Option<RecordingFault>,
    /// Loaded history is immutable while replaying; position choices filter it.
    imported_context: bool,
}

impl Session {
    /// Forget everything but the recorder, which the caller discards or
    /// keeps.
    pub fn clear(&mut self) {
        // Clearing views cannot make a damaged recorder usable again. Only
        // releasing that recorder (discard_recording) resets its fault.
        if self.recorder.is_none() {
            self.recording_fault = None;
        }
        self.state = State::Idle;
        self.imported_context = false;
        self.settle_pending = false;
        self.settle_to = None;
        self.events.clear();
        self.recorded = None;
        self.audio_timeline.clear();
        self.saved_metadata = None;
        self.notes_revision = 0;
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// Whether `action` is allowed now.
    pub fn allows(&self, action: Action) -> bool {
        if self.recording_fault.is_some()
            && matches!(action, Action::Resume | Action::Save | Action::Move)
        {
            return false;
        }
        use State::*;
        match action {
            Action::Pause => self.state == Listening,
            Action::Resume => matches!(self.state, Paused { .. }),
            Action::Move => {
                matches!(self.state, Listening | Paused { .. } | Replaying)
                    && !self.has_replay_context()
            }
            Action::Save => {
                matches!(self.state, Listening | Paused { .. }) && self.recorder.is_some()
            }
            Action::Stop => matches!(self.state, Listening | Replaying | Analysing),
        }
    }

    fn check(&self, action: Action) -> Result<(), Refused> {
        if self.allows(action) {
            Ok(())
        } else {
            Err(Refused {
                action,
                state: self.state,
            })
        }
    }

    /// A microphone session, which can be paused and carried on; a
    /// recording opened is not one.
    pub fn mic(&self) -> bool {
        matches!(self.state, State::Listening | State::Paused { .. })
    }

    pub fn paused(&self) -> bool {
        matches!(self.state, State::Paused { .. })
    }

    /// A new microphone session is listening (after [`Session::clear`]).
    pub fn listen(&mut self) {
        self.state = State::Listening;
    }

    /// A recording is playing through (tests only; see `State::Replaying`).
    #[cfg(test)]
    pub fn replay(&mut self) {
        self.state = State::Replaying;
    }

    /// A recording or take is being analysed whole.
    pub fn analyse(&mut self) {
        self.state = State::Analysing;
    }

    /// The analysis is done (from a file, or a log read back).
    pub fn analysed(&mut self) {
        self.state = State::Analysed;
    }

    /// Stop what is under way: a microphone session pauses, a replay or
    /// an analysis stops where it is.
    pub fn stop(&mut self, now: f64) -> Result<io::Result<()>, Refused> {
        self.check(Action::Stop)?;
        match self.state {
            State::Listening => return self.pause(now),
            State::Replaying => self.state = State::ReplayStopped,
            _ => self.state = State::Idle,
        }
        Ok(Ok(()))
    }

    /// Paused now (the microphone let go). The inner error is the
    /// recorder's, when it couldn't close the stretch.
    pub fn pause(&mut self, now: f64) -> Result<io::Result<()>, Refused> {
        self.check(Action::Pause)?;
        self.anchor_audio(now, false);
        self.state = State::Paused {
            since: Instant::now(),
        };
        Ok(self.recorder.as_mut().map_or(Ok(()), Recorder::pause))
    }

    /// Carrying on at `device`: the seconds the pause lasted, for the
    /// session's clock.
    pub fn resume(&mut self, device: &str) -> Result<f64, Refused> {
        self.check(Action::Resume)?;
        let State::Paused { since } = self.state else {
            unreachable!("checked")
        };
        self.state = State::Listening;
        // The recorder starts a break, on this input.
        if let Some(r) = self.recorder.as_mut() {
            r.resume(device);
        }
        Ok(since.elapsed().as_secs_f64())
    }

    /// After [`Session::resume`] has moved the session's clock on to `now`:
    /// anchor the recording timeline and start any settling left waiting.
    pub fn resumed_at(&mut self, now: f64) {
        self.anchor_audio(now, true);
        if self.settle_pending {
            self.start_settle(now);
        }
    }

    fn anchor_audio(&mut self, at_s: f64, advancing: bool) {
        let Some(r) = self.recorder.as_ref() else {
            return;
        };
        // A zero-length pause/resume has one boundary; the latest state wins.
        if self.audio_timeline.last().is_some_and(|a| a.at_s == at_s) {
            self.audio_timeline.pop();
        }
        self.audio_timeline.push(AudioAnchor {
            at_s,
            audio_s: r.duration_s(),
            advancing,
        });
    }

    /// For a test: as if in `state`.
    #[cfg(test)]
    pub fn set_state(&mut self, state: State) {
        self.state = state;
    }

    /// The watch put in `pos` at `at` (it was in `old`), while listening,
    /// replaying or paused.
    pub fn move_to(&mut self, at: f64, old: Position, pos: Position) -> Result<(), Refused> {
        self.check(Action::Move)?;
        let audio = (
            self.recorder.as_ref().map(|_| 0.0),
            self.recorder.as_ref().map(Recorder::duration_s),
        );
        record_move(&mut self.events, at, audio, old, pos);
        // A move is recorded under any painting, too.
        if let Some(r) = self.recorded.as_mut() {
            record_move(r, at, audio, old, pos);
        }
        // The watch settles in its new position, from now while
        // listening, else from Resume.
        if self.paused() {
            self.settle_pending = true;
        } else {
            self.start_settle(at);
        }
        Ok(())
    }

    /// Start the settling at `t`.
    pub fn start_settle(&mut self, t: f64) {
        self.settle_pending = false;
        let len = self.settle_len();
        record_settle(&mut self.events, t, len);
        if let Some(r) = self.recorded.as_mut() {
            record_settle(r, t, len);
        }
        self.settle_to = Some(t + len);
    }

    /// Set how long the watch settles after a change of position, from the
    /// next change on.
    pub fn set_settle_s(&mut self, s: f64) {
        self.settle_s = Some(s);
    }

    /// Settle set to `s` on the Positions Test pane: from the next change
    /// on, and every settling already marked redrawn to it, each still
    /// ending by the next change of position.
    pub fn resettle(&mut self, s: f64) {
        self.set_settle_s(s);
        let under_way = self.settle_to.is_some();
        match self.recorded.as_mut() {
            Some(r) => {
                redraw_settles(r, s);
                self.compose();
            }
            None => redraw_settles(&mut self.events, s),
        }
        // A settling under way ends where its band now does.
        if under_way {
            self.settle_to = self.settles().last().map(|x| x.1);
        }
    }

    /// How long the watch settles after a change of position.
    pub fn settle_len(&self) -> f64 {
        self.settle_s.unwrap_or(crate::guided::SETTLE_S)
    }

    /// While listening at `now`: `Some(true)` while settling (read
    /// nothing), `Some(false)` once, as it ends (start the readings
    /// afresh), `None` otherwise.
    pub fn settling_now(&mut self, now: f64) -> Option<bool> {
        let end = self.settle_to?;
        if now < end {
            return Some(true);
        }
        self.settle_to = None;
        Some(false)
    }

    /// The end of the settling under way.
    #[cfg(test)]
    pub fn settle_to(&self) -> Option<f64> {
        self.settle_to
    }

    /// Whether `t` falls in a settling after a move.
    pub fn settling_at(&self, t: f64) -> bool {
        self.settles().iter().any(|s| t > s.0 && t <= s.1)
    }

    /// Seconds of settling still to come at `t`.
    pub fn settling_left(&self, t: f64) -> Option<f64> {
        self.settles()
            .iter()
            .find(|s| t >= s.0 && t < s.1)
            .map(|s| s.1 - t)
    }

    /// Where the watch was put in a new position: the time and the
    /// position from then on, oldest first. Empty while it has stayed put.
    pub fn stretches(&self) -> Vec<(f64, Position)> {
        changes_in(&self.events)
    }

    /// The changes of position: the time and the position from then on,
    /// oldest first, the first at the start; one while the watch stayed
    /// put in `now`.
    fn changes(&self, now: Position) -> Vec<(f64, Position)> {
        let s = self.stretches();
        if s.is_empty() {
            vec![(0.0, now)]
        } else {
            s
        }
    }

    /// Paint the stretch from `from` to `to` as position `pos`, the watch
    /// being in `now` and the take reaching `end`: what the Position Brush
    /// does. What came after `to` carries on there, unless the stretch
    /// runs to the end, which puts the watch in `pos` from then on.
    /// Painting is a correction, so it starts no settling; a change painted
    /// over goes with its settling, and one painted from inside a settling
    /// cuts it short. What was recorded stays underneath, for
    /// [`Session::remove_painting`]. Returns the position at the end.
    pub fn paint(
        &mut self,
        from: f64,
        to: f64,
        pos: Position,
        now: Position,
        end: f64,
    ) -> Position {
        self.keep_recorded(now);
        let old = self.changes(now);
        let at = |t: f64| position_at(&old, t);
        let from = from.max(0.0);
        let to_end = to >= end - 1e-6;
        let after = at(to);
        let mut new: Vec<(f64, Position)> = old
            .iter()
            .copied()
            .filter(|c| c.0 < from || !to_end && c.0 > to)
            .collect();
        new.push((from, pos));
        if !to_end {
            new.push((to, after));
        }
        self.set_changes(new, end)
    }

    /// Move the change of position at `was` to `to`, between the changes
    /// either side of it, its settling with it; the take reaches `end`.
    /// Returns the position at the end.
    pub fn move_change(&mut self, was: f64, to: f64, now: Position, end: f64) -> Position {
        let old = self.changes(now);
        let Some(i) = (1..old.len()).find(|&i| (old[i].0 - was).abs() < 1e-6) else {
            return old.last().map_or(now, |c| c.1);
        };
        self.keep_recorded(now);
        let recorded = changes_in(self.recorded.as_deref().unwrap_or_default());
        // A recorded change moves in both layers, so it stays short of the
        // recorded changes either side as well.
        let j = (1..recorded.len()).find(|&j| (recorded[j].0 - was).abs() < 1e-6);
        let mut lo = old[i - 1].0 + MIN_STRETCH_S;
        let mut hi = old.get(i + 1).map_or(end, |c| c.0 - MIN_STRETCH_S);
        if let Some(j) = j {
            lo = lo.max(recorded[j - 1].0 + MIN_STRETCH_S);
            hi = hi.min(recorded.get(j + 1).map_or(end, |c| c.0 - MIN_STRETCH_S));
        }
        if hi < lo {
            return old.last().map_or(now, |c| c.1);
        }
        let to = to.clamp(lo, hi);
        let shift = to - old[i].0;
        if j.is_some() {
            let audio = self.recorder.as_ref().and_then(|_| self.audio_of(to));
            for e in self.recorded.iter_mut().flatten() {
                match e {
                    Event::Position { at_s, audio_s, .. } if (*at_s - was).abs() < 1e-6 => {
                        *at_s = to;
                        *audio_s = audio;
                    }
                    Event::Settle { from_s, to_s } if (*from_s - was).abs() < 1e-6 => {
                        *from_s += shift;
                        *to_s += shift;
                    }
                    _ => {}
                }
            }
        }
        let mut new = old;
        new[i].0 = to;
        self.set_changes(new, end)
    }

    /// Take the painting off the painted stretch over `t`, the watch being
    /// in `now` and the take reaching `end`: the positions recorded there
    /// come back, with their changes and settling. Returns the position at
    /// the end, or `None` where nothing painted shows at `t`.
    pub fn remove_painting(&mut self, t: f64, now: Position, end: f64) -> Option<Position> {
        let (from, to) = self
            .painted_spans(now)
            .into_iter()
            .find(|&(a, b, _)| t >= a && t < b)
            .map(|(a, b, _)| (a, b))?;
        let shown = self.changes(now);
        let recorded = changes_in(self.recorded.as_deref()?);
        let mut times: Vec<f64> = shown
            .iter()
            .chain(&recorded)
            .map(|c| c.0)
            .chain([from, to])
            .filter(|t| t.is_finite())
            .collect();
        times.sort_by(f64::total_cmp);
        times.dedup();
        let new = times
            .into_iter()
            .map(|x| {
                let layer = if x >= from && x < to {
                    &recorded
                } else {
                    &shown
                };
                (x, position_at(layer, x))
            })
            .collect();
        Some(self.set_changes(new, end))
    }

    /// Where painting shows over what was recorded: start, end and the
    /// position painted, oldest first; a stretch painted to the end ends
    /// at infinity.
    pub fn painted_spans(&self, now: Position) -> Vec<(f64, f64, Position)> {
        let Some(r) = self.recorded.as_deref() else {
            return Vec::new();
        };
        let shown = self.changes(now);
        let recorded = changes_in(r);
        let mut times: Vec<f64> = shown.iter().chain(&recorded).map(|c| c.0).collect();
        times.sort_by(f64::total_cmp);
        times.dedup();
        let mut spans: Vec<(f64, f64, Position)> = Vec::new();
        for (k, &a) in times.iter().enumerate() {
            let b = times.get(k + 1).copied().unwrap_or(f64::INFINITY);
            let p = position_at(&shown, a);
            if p == position_at(&recorded, a) {
                continue;
            }
            match spans.last_mut() {
                Some(last) if last.1 == a && last.2 == p => last.1 = b,
                _ => spans.push((a, b, p)),
            }
        }
        spans
    }

    /// The positions and settling as recorded, under any painting: the
    /// changes and the settling, as [`Session::stretches`] and
    /// [`Session::settles`] give them; `None` while nothing painted shows.
    pub fn recorded(&self) -> Option<Layer> {
        let r = self.recorded.as_deref()?;
        Some((changes_in(r), settles_in(r)))
    }

    /// Before painting: what was recorded, kept to paint over, the watch
    /// being in `now`.
    fn keep_recorded(&mut self, now: Position) {
        if self.recorded.is_some() {
            return;
        }
        let mut r: Vec<Event> = self
            .events
            .iter()
            .filter(|e| matches!(e, Event::Position { .. } | Event::Settle { .. }))
            .cloned()
            .collect();
        if self.stretches().is_empty() {
            r.insert(
                0,
                Event::Position {
                    at_s: 0.0,
                    audio_s: self.recorder.as_ref().map(|_| 0.0),
                    pos: now,
                },
            );
        }
        self.recorded = Some(r);
    }

    /// The settling shown, from what was recorded: a recorded settling
    /// stays where the change that began it still shows and nothing is
    /// painted where it starts, cut short at the next change shown. Where
    /// the positions shown are the recorded ones, the painting is gone.
    fn compose(&mut self) {
        let Some(r) = self.recorded.as_deref() else {
            return;
        };
        let shown = self.stretches();
        let recorded = changes_in(r);
        let settles: Vec<(f64, f64)> = settles_in(r)
            .into_iter()
            .filter_map(|(a, b)| {
                let began = recorded.iter().rposition(|c| c.0 <= a + 1e-9);
                let kept = began.is_none_or(|j| {
                    j == 0
                        || shown
                            .iter()
                            .skip(1)
                            .any(|c| (c.0 - recorded[j].0).abs() < 1e-6)
                });
                let unpainted =
                    shown.is_empty() || position_at(&shown, a) == position_at(&recorded, a);
                let next = shown
                    .iter()
                    .map(|c| c.0)
                    .find(|&c| c > a + 1e-9)
                    .unwrap_or(f64::INFINITY);
                let b = b.min(next);
                (kept && unpainted && b > a).then_some((a, b))
            })
            .collect();
        let same = |x: &[(f64, Position)]| {
            let mut x = x.to_vec();
            x.dedup_by(|b, a| a.1 == b.1);
            x
        };
        let back = same(&shown) == same(&recorded);
        self.events.retain(|e| !matches!(e, Event::Settle { .. }));
        self.events.extend(
            settles
                .iter()
                .map(|&(from_s, to_s)| Event::Settle { from_s, to_s }),
        );
        if back {
            self.recorded = None;
        }
    }

    /// Make `changes` the positions shown over what was recorded, the take
    /// reaching `end`: a change to the position already in place goes, and
    /// the settling is what [`Session::compose`] keeps of the recorded
    /// settling. Returns the position at the end.
    fn set_changes(&mut self, mut changes: Vec<(f64, Position)>, end: f64) -> Position {
        changes.sort_by(|a, b| a.0.total_cmp(&b.0));
        changes.dedup_by(|b, a| a.1 == b.1);
        let audio = |s: &Self, t: f64| s.recorder.as_ref().and_then(|_| s.audio_of(t));
        let positions: Vec<Event> = changes
            .iter()
            .map(|&(at_s, pos)| Event::Position {
                at_s,
                audio_s: audio(self, at_s),
                pos,
            })
            .collect();
        self.events.retain(|e| !matches!(e, Event::Position { .. }));
        self.events.extend(positions);
        self.compose();
        // A settling still under way at the end goes on; one painted over
        // ends.
        self.settle_to = self.settles().iter().map(|s| s.1).find(|&to| to > end);
        changes.last().map_or(Position::DialUp, |c| c.1)
    }

    /// The settling after each move: start and end, oldest first.
    pub fn settles(&self) -> Vec<(f64, f64)> {
        settles_in(&self.events)
    }

    /// The positions and settling a take's session file gives, for a
    /// recording opened.
    pub fn load(&mut self, stretches: &[(f64, Position)], settles: &[(f64, f64)]) {
        self.imported_context = !stretches.is_empty() || !settles.is_empty();
        self.settle_pending = false;
        self.settle_to = None;
        self.recorded = None;
        self.events.retain(|e| matches!(e, Event::Saved { .. }));
        for &(at_s, pos) in stretches {
            self.events.push(Event::Position {
                at_s,
                audio_s: None,
                pos,
            });
        }
        for &(from_s, to_s) in settles {
            self.events.push(Event::Settle { from_s, to_s });
        }
    }

    /// The positions and settling a take's session file gives as recorded,
    /// under the painting [`Session::load`] loaded, the watch being in
    /// `now`.
    pub fn load_recorded(
        &mut self,
        stretches: &[(f64, Position)],
        settles: &[(f64, f64)],
        now: Position,
    ) {
        let mut r: Vec<Event> = stretches
            .iter()
            .map(|&(at_s, pos)| Event::Position {
                at_s,
                audio_s: None,
                pos,
            })
            .collect();
        r.extend(
            settles
                .iter()
                .map(|&(from_s, to_s)| Event::Settle { from_s, to_s }),
        );
        if r.is_empty() {
            return;
        }
        self.recorded = Some(r);
        // Kept only where it differs from what shows.
        let shown = self.changes(now);
        let same = |x: Vec<(f64, Position)>| {
            let mut x = x;
            x.dedup_by(|b, a| a.1 == b.1);
            x
        };
        if same(shown) == same(stretches.to_vec()) && settles == self.settles() {
            self.recorded = None;
        }
    }

    /// A replay's positions came from its manifest, rather than manual moves.
    pub fn has_replay_context(&self) -> bool {
        self.imported_context && matches!(self.state, State::Replaying | State::ReplayStopped)
    }

    /// Saved moves and settling ends bound readings in audio time, including
    /// boundaries the replay has not reached yet. Never create a live move.
    #[cfg(test)]
    pub fn replay_breaks(&self) -> Option<Vec<f64>> {
        if !self.has_replay_context() {
            return None;
        }
        let mut breaks: Vec<f64> = self
            .stretches()
            .iter()
            .skip(1)
            .map(|s| s.0)
            .chain(self.settles().iter().map(|s| s.1))
            .collect();
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        Some(breaks)
    }

    /// Imported settling intervals apply to the reading's timestamp, which
    /// can lag received audio. Other session modes keep their existing policy.
    pub fn replay_reading_allowed(&self, at_s: f64) -> bool {
        !self.has_replay_context() || !self.settling_at(at_s)
    }

    /// Each stretch in one position: start, end and position, with `now`
    /// the position the watch is in. One stretch over everything while it
    /// stayed put.
    pub fn position_spans(&self, now: Position) -> Vec<(f64, f64, Position)> {
        let stretches = self.stretches();
        if stretches.is_empty() {
            return vec![(f64::NEG_INFINITY, f64::INFINITY, now)];
        }
        (0..stretches.len())
            .map(|i| {
                let (a, p) = stretches[i];
                let b = stretches.get(i + 1).map_or(f64::INFINITY, |s| s.0);
                (if i == 0 { f64::NEG_INFINITY } else { a }, b, p)
            })
            .collect()
    }

    /// The stretches in position `now`, each without the settling it began
    /// with.
    pub fn current_spans(&self, now: Position) -> Vec<(f64, f64)> {
        self.settled_spans(now, now)
    }

    /// The stretches in position `of`, each without the settling it began
    /// with, the watch being in position `now`.
    pub fn settled_spans(&self, now: Position, of: Position) -> Vec<(f64, f64)> {
        let settles = self.settles();
        self.position_spans(now)
            .into_iter()
            .filter(|s| s.2 == of)
            .filter_map(|(a, b, _)| {
                let a = settles
                    .iter()
                    .filter(|s| s.0 >= a && s.0 < b)
                    .fold(a, |a, s| a.max(s.1));
                (a < b).then_some((a, b))
            })
            .collect()
    }

    /// Whether a reading ending at `t` comes from position `now`, settled.
    pub fn in_position(&self, t: f64, now: Position) -> bool {
        self.stretches().is_empty()
            || !self.settling_at(t)
                && self
                    .position_spans(now)
                    .iter()
                    .any(|s| s.2 == now && t > s.0 && t <= s.1)
    }

    /// The stretches to mark on the strip and the charts, once there is
    /// more than one: start, end and the position's label.
    pub fn position_bands(&self, now: Position) -> Vec<(f64, f64, &'static str)> {
        if self.stretches().len() < 2 {
            return Vec::new();
        }
        self.position_spans(now)
            .into_iter()
            .map(|(a, b, p)| (a.max(0.0), b, p.label()))
            .collect()
    }

    /// Where the sound was last saved.
    pub fn saved_to(&self) -> Option<&Path> {
        self.events.iter().rev().find_map(|e| match e {
            Event::Saved { to, .. } => Some(to.as_path()),
            _ => None,
        })
    }

    /// A take saved to was moved from `from` to `to`: Save Again follows.
    pub fn saved_moved(&mut self, from: &Path, to: &Path) {
        for e in &mut self.events {
            if let Event::Saved { to: at, .. } = e {
                if let Ok(rest) = at.strip_prefix(from) {
                    *at = to.join(rest);
                }
            }
        }
    }

    fn saved_through_s(&self) -> f64 {
        self.events
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::Saved { through_s, .. } => Some(*through_s),
                _ => None,
            })
            .unwrap_or(0.0)
    }

    /// A position note is append-only even if the user moves back before any
    /// more audio arrives, leaving the current position/ranges unchanged.
    pub fn note_metadata_change(&mut self) {
        self.notes_revision += 1;
    }

    pub fn metadata_snapshot(&self, key: Metadata) -> MetadataSnapshot {
        MetadataSnapshot {
            key,
            events: self.metadata_events().cloned().collect(),
            notes_revision: self.notes_revision,
        }
    }

    fn metadata_events(&self) -> impl Iterator<Item = &Event> {
        self.events
            .iter()
            .filter(|e| !matches!(e, Event::Saved { .. }))
    }

    /// Edits since a successful save, independently of the sound's duration.
    pub fn metadata_changed(&self, key: &Metadata) -> bool {
        self.recorder.is_some()
            && self.saved_metadata.as_ref().is_some_and(|saved| {
                saved.key != *key
                    || saved.notes_revision != self.notes_revision
                    || !self.metadata_events().eq(saved.events.iter())
            })
    }

    pub fn mark_saved(&mut self, through_s: f64, to: PathBuf, metadata: MetadataSnapshot) {
        self.events.push(Event::Saved { through_s, to });
        self.saved_metadata = Some(metadata);
    }

    /// Kept sound not in the last saved copy, seconds.
    pub fn unsaved_s(&self) -> f64 {
        match &self.recorder {
            Some(r) => (r.duration_s() - self.saved_through_s()).max(0.0),
            None => 0.0,
        }
    }

    pub fn recording_fault(&self) -> Option<&RecordingFault> {
        self.recording_fault.as_ref()
    }

    /// Stop the session without discarding a possibly usable prefix. A marker
    /// is best effort: a full/broken filesystem may reject even that write.
    pub fn recording_failed(&mut self, now: f64, reason: String) {
        let Some(r) = &self.recorder else { return };
        let recovery_dir = r.dir().to_path_buf();
        self.anchor_audio(now, false);
        self.state = State::Paused {
            since: Instant::now(),
        };
        let mut fault = RecordingFault {
            reason,
            recovery_dir,
            marker_error: None,
        };
        if let Err(e) = std::fs::write(
            fault.recovery_dir.join("RECORDING-ERROR.txt"),
            fault.description() + "\n",
        ) {
            fault.marker_error = Some(e.to_string());
        }
        self.recording_fault = Some(fault);
    }

    /// Release the recorder, finishing its files and leaving them where
    /// they are, for the next start to offer back (the app is quitting
    /// without having asked about them).
    pub fn keep_recording(&mut self) {
        if let Some(r) = self.recorder.take() {
            if self.recording_fault.is_some() {
                drop(r);
            } else {
                let _ = r.finish(None);
            }
        }
    }

    /// Release the recorder and forget the saves. Healthy unsaved scratch is
    /// deleted; faulted scratch is left untouched for manual recovery.
    pub fn discard_recording(&mut self) {
        if let Some(r) = self.recorder.take() {
            if self.recording_fault.is_some() {
                // Drop closes handles best-effort, without writing a normal
                // completed session.json or deleting uncertain audio.
                drop(r);
            } else if let Ok(dir) = r.finish(None) {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
        self.recording_fault = None;
        self.events.retain(|e| !matches!(e, Event::Saved { .. }));
        self.audio_timeline.clear();
        self.saved_metadata = None;
        self.notes_revision = 0;
    }

    /// `t` elapsed seconds as seconds of kept sound. Time stands still in
    /// the sound during a pause, independently of any position changes.
    /// Endpoints beyond the sound kept so far are bounded by its duration.
    pub fn audio_of(&self, t: f64) -> Option<f64> {
        let r = self.recorder.as_ref()?;
        let audio_s = match self.audio_timeline.iter().rev().find(|a| a.at_s <= t) {
            Some(a) if a.advancing => a.audio_s + (t - a.at_s),
            Some(a) => a.audio_s,
            None => t,
        };
        Some(audio_s.clamp(0.0, r.duration_s()))
    }

    /// The saved take's positions in its sound's own seconds (which leave
    /// out the pauses): start, end and position code. Each later position
    /// starts once it has settled; one entry with no times while the watch
    /// stayed put.
    pub fn take_ranges(&self) -> Vec<Range> {
        self.ranges_of(&self.events)
    }

    /// The positions as recorded under the painting, as
    /// [`Session::take_ranges`] gives those shown; `None` while nothing
    /// painted shows.
    pub fn recorded_ranges(&self) -> Option<Vec<Range>> {
        Some(self.ranges_of(self.recorded.as_deref()?))
    }

    fn ranges_of(&self, events: &[Event]) -> Vec<Range> {
        let Some(r) = self.recorder.as_ref() else {
            return Vec::new();
        };
        let settles = settles_in(events);
        let mut stretches = vec![(0.0, 0.0, r.first_position().to_string())];
        for e in events {
            if let Event::Position {
                at_s,
                audio_s: Some(a),
                pos,
            } = e
            {
                let code = pos.code().to_string();
                if stretches.last().is_some_and(|s| s.1 >= *a) {
                    stretches.pop();
                }
                if stretches.last().is_none_or(|s| s.2 != code) {
                    stretches.push((*at_s, *a, code));
                }
            }
        }
        let one = stretches.len() == 1;
        (0..stretches.len())
            .map(|i| {
                let (at, audio, code) = &stretches[i];
                let next = stretches.get(i + 1);
                let end = next.map(|s| s.1);
                let start = if i == 0 {
                    *audio
                } else {
                    let until = next.map_or(f64::INFINITY, |s| s.0);
                    // The settling that began this position may start at
                    // Resume, or finish during a pause. Map its real end.
                    let settled = settles
                        .iter()
                        .find(|s| s.0 >= *at && s.0 < until)
                        .map_or(*at, |s| s.1);
                    self.audio_of(settled).expect("recorder present")
                }
                .min(end.unwrap_or(r.duration_s()));
                ((!one).then_some(start), end, code.clone())
            })
            .collect()
    }
}

/// The changes of position in `events`, oldest first.
fn changes_in(events: &[Event]) -> Vec<(f64, Position)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Position { at_s, pos, .. } => Some((*at_s, *pos)),
            _ => None,
        })
        .collect()
}

/// The settling in `events`, oldest first.
fn settles_in(events: &[Event]) -> Vec<(f64, f64)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Settle { from_s, to_s } => Some((*from_s, *to_s)),
            _ => None,
        })
        .collect()
}

/// The position at `t` given `changes`, oldest first, the first from the
/// start.
fn position_at(changes: &[(f64, Position)], t: f64) -> Position {
    changes
        .iter()
        .rev()
        .find(|c| c.0 <= t)
        .or(changes.first())
        .map_or(Position::DialUp, |c| c.1)
}

/// The watch put in `pos` at `at` (it was in `old`), in `events`; `audio`
/// is the sound's length at the start and now.
fn record_move(
    events: &mut Vec<Event>,
    at: f64,
    audio: (Option<f64>, Option<f64>),
    old: Position,
    pos: Position,
) {
    if !events.iter().any(|e| matches!(e, Event::Position { .. })) {
        events.push(Event::Position {
            at_s: 0.0,
            audio_s: audio.0,
            pos: old,
        });
    }
    // A position changed back before any beats came is undone.
    if let Some(i) = events
        .iter()
        .rposition(|e| matches!(e, Event::Position { .. }))
    {
        if let Event::Position { at_s, .. } = events[i] {
            if at_s >= at {
                events.remove(i);
            }
        }
    }
    if changes_in(events).last().is_none_or(|s| s.1 != pos) {
        events.push(Event::Position {
            at_s: at,
            audio_s: audio.1,
            pos,
        });
    }
}

/// A settling `len` long from `t`, in `events`: a move during a settling
/// starts it again, the first ending there.
fn record_settle(events: &mut Vec<Event>, t: f64, len: f64) {
    for e in events.iter_mut().rev() {
        if let Event::Settle { from_s, to_s } = e {
            if *to_s > t {
                *to_s = t.max(*from_s);
            }
            break;
        }
    }
    events.retain(|e| !matches!(e, Event::Settle { from_s, to_s } if to_s <= from_s));
    events.push(Event::Settle {
        from_s: t,
        to_s: t + len,
    });
}

/// Every settling in `events` redrawn `s` long, each still ending by the
/// next change of position.
fn redraw_settles(events: &mut [Event], s: f64) {
    let changes: Vec<f64> = changes_in(events).iter().map(|c| c.0).collect();
    for e in events.iter_mut() {
        if let Event::Settle { from_s, to_s } = e {
            let next = changes
                .iter()
                .copied()
                .find(|&c| c > *from_s + 1e-9)
                .unwrap_or(f64::INFINITY);
            *to_s = (*from_s + s).min(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The settling these tests were written for: the pane's Settle set
    /// to 10 s.
    const SETTLE_S: f64 = 10.0;

    fn ten() -> Session {
        let mut s = Session::default();
        s.set_settle_s(SETTLE_S);
        s
    }

    /// The settling after a move is the Positions Test pane's Settle: 20 s
    /// unless set, and whatever it is set to from the next move on.
    #[test]
    fn the_settling_after_a_move_is_the_settle_setting() {
        let mut s = Session::default();
        s.listen();
        assert_eq!(s.settle_len(), crate::guided::SETTLE_S);
        s.move_to(30.0, Position::DialUp, Position::DialDown)
            .unwrap();
        assert_eq!(s.settles(), [(30.0, 50.0)]);
        s.set_settle_s(45.0);
        s.move_to(100.0, Position::DialDown, Position::CrownUp)
            .unwrap();
        assert_eq!(s.settles(), [(30.0, 50.0), (100.0, 145.0)]);
    }

    /// Settle changed on the pane redraws every settle band already
    /// marked, each still ending by the next change of position.
    #[test]
    fn a_new_settle_redraws_every_settle_band() {
        let mut s = Session::default();
        s.listen();
        s.move_to(30.0, Position::DialUp, Position::DialDown)
            .unwrap();
        s.move_to(60.0, Position::DialDown, Position::CrownUp)
            .unwrap();
        assert_eq!(s.settles(), [(30.0, 50.0), (60.0, 80.0)]);
        s.resettle(10.0);
        assert_eq!(s.settles(), [(30.0, 40.0), (60.0, 70.0)]);
        // Longer than the time to the next change: it ends at the change.
        s.resettle(45.0);
        assert_eq!(s.settles(), [(30.0, 60.0), (60.0, 105.0)]);
        assert_eq!(s.settle_len(), 45.0);
    }

    /// The Position Brush: a stretch painted in the middle of a take
    /// puts what was there back after it, with no settling, since painting
    /// is a correction; one painted to the end leaves the watch in the
    /// painted position; a stretch painted over a change takes the change
    /// away.
    #[test]
    fn painting_a_stretch_sets_its_position() {
        use Position::*;
        let mut s = ten();
        s.listen();
        // Never moved: painted in the middle.
        assert_eq!(s.paint(100.0, 200.0, CrownDown, DialUp, 600.0), DialUp);
        assert_eq!(
            s.stretches(),
            [(0.0, DialUp), (100.0, CrownDown), (200.0, DialUp)]
        );
        assert!(s.settles().is_empty());
        // Painted over the change back to Dial Up and on to the end.
        assert_eq!(s.paint(150.0, 600.0, CrownDown, DialUp, 600.0), CrownDown);
        assert_eq!(s.stretches(), [(0.0, DialUp), (100.0, CrownDown)]);
        assert!(s.settles().is_empty());
        // Painted from the start: the first position changes, and painted
        // the same as what follows, the change goes.
        assert_eq!(s.paint(0.0, 100.0, CrownDown, CrownDown, 600.0), CrownDown);
        assert_eq!(s.stretches(), [(0.0, CrownDown)]);
        assert!(s.settles().is_empty());
        assert!(s.position_bands(CrownDown).is_empty());
    }

    /// A change of position dragged moves with its settling, and stops
    /// short of the changes either side.
    #[test]
    fn a_change_of_position_can_be_moved() {
        use Position::*;
        let mut s = ten();
        s.listen();
        s.move_to(100.0, DialUp, CrownDown).unwrap();
        s.move_to(200.0, CrownDown, DialUp).unwrap();
        assert_eq!(s.move_change(100.0, 80.0, DialUp, 600.0), DialUp);
        assert_eq!(
            s.stretches(),
            [(0.0, DialUp), (80.0, CrownDown), (200.0, DialUp)]
        );
        assert_eq!(s.settles(), [(80.0, 90.0), (200.0, 210.0)]);
        // Past the next change: stops half a second before it.
        s.move_change(200.0, 50.0, DialUp, 600.0);
        assert_eq!(s.stretches()[2], (80.0 + MIN_STRETCH_S, DialUp));
        assert_eq!(s.settles()[0], (80.0, 80.0 + MIN_STRETCH_S));
        // The start of the take is not a change.
        s.move_change(0.0, 30.0, DialUp, 600.0);
        assert_eq!(s.stretches()[0], (0.0, DialUp));
    }

    /// Painting over recorded switches (the design guide's marks 49 to
    /// 51): a recorded switch keeps its settling unless painting covers
    /// it, painting from inside a settling cuts it short, painting the
    /// position already recorded changes nothing, and Remove Painting
    /// brings back what was recorded.
    #[test]
    fn painting_keeps_the_recorded_positions_underneath() {
        use Position::*;
        let recorded = || {
            let mut s = ten();
            s.listen();
            s.move_to(60.0, DialUp, DialDown).unwrap();
            s.move_to(120.0, DialDown, CrownUp).unwrap();
            s
        };
        let mut s = recorded();
        let was = (s.stretches(), s.settles());
        assert_eq!(was.1, [(60.0, 70.0), (120.0, 130.0)]);
        assert!(s.recorded().is_none());
        // (50) Across the switch at 120 s: its settling goes, and nothing
        // painted settles.
        s.paint(90.0, 150.0, CrownLeft, CrownUp, 300.0);
        assert_eq!(
            s.stretches(),
            [
                (0.0, DialUp),
                (60.0, DialDown),
                (90.0, CrownLeft),
                (150.0, CrownUp)
            ]
        );
        assert_eq!(s.settles(), [(60.0, 70.0)]);
        assert_eq!(s.painted_spans(CrownUp), [(90.0, 150.0, CrownLeft)]);
        assert_eq!(s.recorded(), Some(was.clone()));
        // Remove Painting: the switch and its settling come back.
        assert_eq!(s.remove_painting(100.0, CrownUp, 300.0), Some(CrownUp));
        assert_eq!((s.stretches(), s.settles()), was);
        assert!(s.recorded().is_none() && s.painted_spans(CrownUp).is_empty());
        assert_eq!(s.remove_painting(100.0, CrownUp, 300.0), None);
        // (51) From inside a settling: the band is cut where the paint
        // begins.
        s.paint(65.0, 100.0, CrownRight, CrownUp, 300.0);
        assert_eq!(s.settles(), [(60.0, 65.0), (120.0, 130.0)]);
        // (49) Inside a recorded stretch: neither painted edge settles.
        let mut s = recorded();
        s.paint(20.0, 40.0, CrownDown, CrownUp, 300.0);
        assert_eq!(
            s.stretches()[..3],
            [(0.0, DialUp), (20.0, CrownDown), (40.0, DialUp)]
        );
        assert_eq!(s.settles(), [(60.0, 70.0), (120.0, 130.0)]);
        // The position already recorded, painted: nothing changes.
        let mut s = recorded();
        s.paint(70.0, 110.0, DialDown, CrownUp, 300.0);
        assert_eq!((s.stretches(), s.settles()), was);
        assert!(s.recorded().is_none());
        // A move while a painting shows is recorded under it too.
        let mut s = recorded();
        s.paint(130.0, 300.0, CrownLeft, CrownUp, 300.0);
        s.move_to(310.0, CrownLeft, DialUp).unwrap();
        let (under, settles) = s.recorded().expect("still painted");
        assert_eq!(under.last(), Some(&(310.0, DialUp)));
        assert_eq!(settles.last(), Some(&(310.0, 320.0)));
        assert_eq!(s.settles().last(), Some(&(310.0, 320.0)));
    }

    /// The positions recorded, from a session file, load under the
    /// painting; where they are what shows, nothing is kept.
    #[test]
    fn the_positions_recorded_load_under_the_painting() {
        use Position::*;
        let mut s = ten();
        let shown = [(0.0, DialUp), (20.0, CrownDown), (40.0, DialUp)];
        s.load(&shown, &[]);
        s.load_recorded(&[(0.0, DialUp)], &[], DialUp);
        assert_eq!(s.painted_spans(DialUp), [(20.0, 40.0, CrownDown)]);
        assert_eq!(s.remove_painting(30.0, DialUp, 60.0), Some(DialUp));
        assert_eq!(s.stretches(), [(0.0, DialUp)]);
        let mut s = ten();
        s.load(&shown, &[]);
        s.load_recorded(&shown, &[], DialUp);
        assert!(s.recorded().is_none());
    }

    // These are recorder/timeline tests only: no capture device or DSP.
    fn recording(name: &str) -> Session {
        let dir = std::env::temp_dir().join(format!("tg-session-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let info = timegrapher_core::recorder::SessionInfo {
            watch: "test".into(),
            position: "DU".into(),
            lift_deg: 52.0,
            bph: None,
            device: "test".into(),
            sample_rate: 100,
            bits: 16,
            software: "test".into(),
        };
        let mut s = ten();
        s.listen();
        s.recorder = Some(Recorder::start(&dir, info).unwrap());
        s
    }

    fn record(s: &mut Session, seconds: usize, elapsed_end: u64) {
        s.recorder
            .as_mut()
            .unwrap()
            .write(&timegrapher_core::capture::Block {
                samples: vec![0.0; seconds * 100],
                at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(elapsed_end),
            })
            .unwrap();
    }

    fn resume_at(s: &mut Session, now: f64) {
        s.resume("test").unwrap();
        s.resumed_at(now);
    }

    #[test]
    fn imported_replay_history_is_immutable_and_uses_saved_settling_times() {
        let mut s = ten();
        s.replay();
        let positions = [
            (0.0, Position::DialUp),
            (30.0, Position::DialDown),
            (60.0, Position::DialUp),
        ];
        s.load(&positions, &[(30.0, 40.0), (60.0, 70.0), (60.0, 70.0)]);
        assert!(s.has_replay_context());
        assert_eq!(s.replay_breaks(), Some(vec![30.0, 40.0, 60.0, 70.0]));
        assert!(s
            .move_to(10.0, Position::DialUp, Position::CrownUp)
            .is_err());
        assert_eq!(s.stretches(), positions);
        assert_eq!(
            s.settling_now(35.0),
            None,
            "imported settling is not a live timer"
        );
        for (time, allowed) in [
            (30.0, true),
            (30.001, false),
            (40.0, false),
            (40.001, true),
            (60.001, false),
            (70.001, true),
        ] {
            assert_eq!(s.replay_reading_allowed(time), allowed, "{time}");
        }
        s.stop(45.0).unwrap().unwrap();
        assert!(s.has_replay_context());
        assert!(!s.replay_reading_allowed(35.0));
        s.clear();
        s.replay();
        assert!(!s.has_replay_context());
        s.move_to(10.0, Position::DialUp, Position::DialDown)
            .unwrap();
        assert!(
            !s.has_replay_context(),
            "manual replay history is not imported"
        );
        assert_eq!(s.settling_now(15.0), Some(true));
        s.load(&[], &[]);
        assert_eq!(s.settling_now(15.0), None);
        assert_eq!(s.replay_breaks(), None);
    }

    #[test]
    fn a_single_imported_position_is_still_a_replay_filter() {
        let mut s = ten();
        s.replay();
        s.load(&[(0.0, Position::DialDown)], &[]);
        assert!(s.has_replay_context());
        assert_eq!(s.replay_breaks(), Some(vec![]));
        assert!(!s.allows(Action::Move));
        assert!(s.replay_reading_allowed(20.0));
        s.analysed();
        assert!(!s.has_replay_context(), "whole-take policy is unchanged");
    }

    #[test]
    fn recording_fault_survives_view_reset_and_retains_files_on_release() {
        let mut s = recording("fault-retention");
        record(&mut s, 1, 1);
        let dir = s.recorder.as_ref().unwrap().dir().to_path_buf();
        s.recording_failed(1.0, "write failed".into());
        assert!(s.paused());
        assert!(!s.allows(Action::Save));
        assert!(s.resume("test").is_err());
        assert!(dir.join("RECORDING-ERROR.txt").is_file());
        s.clear();
        assert!(s.recording_fault().is_some());
        s.discard_recording();
        assert!(s.recording_fault().is_none());
        assert!(dir.join("audio-001.wav").is_file());
        assert!(dir.join("clock.csv").is_file());
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn recovery_marker_failure_does_not_erase_the_recording_fault() {
        let mut s = recording("fault-marker");
        record(&mut s, 1, 1);
        let dir = s.recorder.as_ref().unwrap().dir().to_path_buf();
        std::fs::create_dir(dir.join("RECORDING-ERROR.txt")).unwrap();
        s.recording_failed(1.0, "original write failure".into());
        let fault = s.recording_fault().unwrap();
        assert_eq!(fault.reason, "original write failure");
        assert!(fault.marker_error.is_some());
        assert!(fault
            .description()
            .contains("Couldn't write the recording-error marker"));
        assert!(!s.allows(Action::Resume));
        s.discard_recording();
        assert!(dir.is_dir());
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn metadata_edits_reverts_and_audio_have_independent_saved_state() {
        let mut s = recording("metadata");
        record(&mut s, 6, 6);
        let original = Metadata {
            watch: "first".into(),
            ..Default::default()
        };
        assert!(!s.metadata_changed(&original), "no prior saved metadata");
        s.mark_saved(6.0, "saved".into(), s.metadata_snapshot(original.clone()));
        assert!(!s.metadata_changed(&original));
        let edited = Metadata {
            watch: "edited".into(),
            ..original.clone()
        };
        assert!(s.metadata_changed(&edited));
        assert_eq!(s.unsaved_s(), 0.0);
        assert!(
            !s.metadata_changed(&original),
            "reverting the edit restores saved meaning"
        );
        record(&mut s, 1, 7);
        assert_eq!(s.unsaved_s(), 1.0);
        assert!(
            !s.metadata_changed(&original),
            "audio alone is not a metadata edit"
        );

        let captured = s.metadata_snapshot(edited.clone());
        // Preparing a save does not commit it: a failure would leave this dirty.
        assert!(s.metadata_changed(&edited));
        s.note_metadata_change();
        s.mark_saved(7.0, "saved".into(), captured);
        assert!(
            s.metadata_changed(&edited),
            "an edit after the captured snapshot remains dirty"
        );
        s.mark_saved(7.0, "saved".into(), s.metadata_snapshot(edited.clone()));
        assert!(!s.metadata_changed(&edited));
        s.clear();
        assert!(!s.metadata_changed(&original));
        s.discard_recording();
    }

    #[test]
    fn extending_a_clipped_settling_range_is_not_a_metadata_edit() {
        let mut s = recording("metadata-settling");
        record(&mut s, 6, 6);
        s.move_to(6.0, Position::DialUp, Position::DialDown)
            .unwrap();
        record(&mut s, 1, 7);
        let key = Metadata::default();
        s.mark_saved(7.0, "saved".into(), s.metadata_snapshot(key.clone()));
        let before = s.take_ranges();
        record(&mut s, 1, 8);
        assert_ne!(
            s.take_ranges(),
            before,
            "the saved range's clipped endpoint advances"
        );
        assert_eq!(s.unsaved_s(), 1.0);
        assert!(!s.metadata_changed(&key));
        s.pause(8.0).unwrap().unwrap();
        s.move_to(8.0, Position::DialDown, Position::CrownUp)
            .unwrap();
        assert!(
            s.metadata_changed(&key),
            "a real position event is different"
        );
        s.discard_recording();
        assert!(!s.metadata_changed(&key));
    }

    #[test]
    fn resuming_a_saved_pending_move_adds_unsaved_settling_metadata() {
        for moved in [false, true] {
            let mut s = recording(if moved {
                "metadata-resume-move"
            } else {
                "metadata-resume"
            });
            record(&mut s, 6, 6);
            s.pause(6.0).unwrap().unwrap();
            if moved {
                s.move_to(6.0, Position::DialUp, Position::DialDown)
                    .unwrap();
            }
            let key = Metadata::default();
            s.mark_saved(6.0, "saved".into(), s.metadata_snapshot(key.clone()));
            assert!(!s.metadata_changed(&key));
            resume_at(&mut s, 12.0);
            assert_eq!(s.unsaved_s(), 0.0);
            // Resume without a move only changes the audio clock. A pending
            // move starts a new settling interval, which is new metadata even
            // before another sample arrives, unlike a clipped endpoint growing.
            assert_eq!(s.metadata_changed(&key), moved);
            assert_eq!(s.settles().len(), usize::from(moved));
            s.discard_recording();
        }
    }

    #[test]
    fn audio_time_leaves_out_pauses_without_position_changes() {
        let mut s = recording("pause");
        record(&mut s, 60, 60);
        s.pause(60.0).unwrap().unwrap();
        assert_eq!(s.audio_of(90.0), Some(60.0));
        resume_at(&mut s, 120.0);
        assert_eq!(s.audio_of(130.0), Some(60.0), "no new sound yet");
        record(&mut s, 10, 130);
        for (elapsed, audio) in [
            (-1.0, 0.0),
            (0.0, 0.0),
            (59.5, 59.5),
            (60.0, 60.0),
            (90.0, 60.0),
            (120.0, 60.0),
            (125.0, 65.0),
            (130.0, 70.0),
            (200.0, 70.0),
        ] {
            assert_eq!(s.audio_of(elapsed), Some(audio), "elapsed {elapsed}");
        }
        assert_eq!(s.take_ranges(), [(None, None, "DU".into())]);
        s.discard_recording();
        assert_eq!(s.audio_of(130.0), None);
    }

    #[test]
    fn repeated_pauses_and_zero_length_boundaries_keep_audio_time() {
        let mut s = recording("repeated");
        record(&mut s, 60, 60);
        s.pause(60.0).unwrap().unwrap();
        resume_at(&mut s, 120.0);
        record(&mut s, 10, 130);
        s.pause(130.0).unwrap().unwrap();
        resume_at(&mut s, 150.0);
        // Pause again before a single new frame arrives.
        s.pause(150.0).unwrap().unwrap();
        resume_at(&mut s, 160.0);
        record(&mut s, 10, 170);
        s.pause(170.0).unwrap().unwrap();
        resume_at(&mut s, 170.0);
        record(&mut s, 10, 180);
        for (elapsed, audio) in [
            (60.0, 60.0),
            (120.0, 60.0),
            (130.0, 70.0),
            (140.0, 70.0),
            (150.0, 70.0),
            (155.0, 70.0),
            (160.0, 70.0),
            (165.0, 75.0),
            (170.0, 80.0),
            (175.0, 85.0),
            (180.0, 90.0),
        ] {
            assert_eq!(s.audio_of(elapsed), Some(audio), "elapsed {elapsed}");
        }
        s.clear();
        assert_eq!(s.audio_of(80.0), Some(80.0), "new session forgets pauses");
        s.discard_recording();
    }

    #[test]
    fn position_settling_ends_in_elapsed_time_even_across_a_pause() {
        for moved_at in [55, 60] {
            let mut s = recording(&format!("settle-{moved_at}"));
            record(&mut s, moved_at, moved_at as u64);
            s.move_to(moved_at as f64, Position::DialUp, Position::DialDown)
                .unwrap();
            if moved_at < 60 {
                record(&mut s, 60 - moved_at, 60);
            }
            s.pause(60.0).unwrap().unwrap();
            resume_at(&mut s, 120.0);
            record(&mut s, 10, 130);
            assert_eq!(
                s.take_ranges(),
                [
                    (Some(0.0), Some(moved_at as f64), "DU".into()),
                    (Some(60.0), None, "DD".into()),
                ],
                "moved at {moved_at}"
            );
            s.discard_recording();
        }
    }

    #[test]
    fn uninterrupted_position_ranges_keep_their_settling_and_undo_behavior() {
        let mut s = recording("uninterrupted");
        record(&mut s, 30, 30);
        s.move_to(30.0, Position::DialUp, Position::DialDown)
            .unwrap();
        record(&mut s, 30, 60);
        s.move_to(60.0, Position::DialDown, Position::CrownUp)
            .unwrap();
        s.move_to(60.0, Position::CrownUp, Position::DialDown)
            .unwrap();
        record(&mut s, 30, 90);
        assert_eq!(
            s.take_ranges(),
            [
                (Some(0.0), Some(30.0), "DU".into()),
                (Some(40.0), None, "DD".into()),
            ]
        );
        s.discard_recording();
    }

    #[test]
    fn position_moved_while_paused_settles_from_resume() {
        let mut s = recording("paused-move");
        record(&mut s, 60, 60);
        s.pause(60.0).unwrap().unwrap();
        s.move_to(60.0, Position::DialUp, Position::CrownUp)
            .unwrap();
        s.move_to(60.0, Position::CrownUp, Position::DialDown)
            .unwrap();
        assert_eq!(
            s.take_ranges(),
            [
                (Some(0.0), Some(60.0), "DU".into()),
                (Some(60.0), None, "DD".into()),
            ]
        );
        resume_at(&mut s, 120.0);
        record(&mut s, 5, 125);
        assert_eq!(
            s.take_ranges()[1].0,
            Some(65.0),
            "unfinished settling is bounded by kept sound"
        );
        record(&mut s, 15, 140);
        assert_eq!(s.settles(), [(120.0, 130.0)]);
        assert_eq!(
            s.take_ranges(),
            [
                (Some(0.0), Some(60.0), "DU".into()),
                (Some(70.0), None, "DD".into()),
            ]
        );
        s.discard_recording();
    }

    /// The events give the stretches, the settling and the spans; a move
    /// back before any beats undoes the move; a move while paused settles
    /// from Resume.
    #[test]
    fn events_give_stretches_settles_and_spans() {
        let mut s = ten();
        s.listen();
        assert_eq!(
            s.position_spans(Position::DialUp),
            [(f64::NEG_INFINITY, f64::INFINITY, Position::DialUp)]
        );
        s.move_to(30.0, Position::DialUp, Position::DialDown)
            .unwrap();
        assert_eq!(
            s.stretches(),
            [(0.0, Position::DialUp), (30.0, Position::DialDown)]
        );
        assert_eq!(s.settles(), [(30.0, 30.0 + SETTLE_S)]);
        assert_eq!(s.settling_now(35.0), Some(true));
        assert_eq!(s.settling_now(41.0), Some(false));
        assert_eq!(s.settling_now(42.0), None);
        assert_eq!(
            s.current_spans(Position::DialDown),
            [(30.0 + SETTLE_S, f64::INFINITY)]
        );
        assert!(!s.in_position(35.0, Position::DialDown));
        assert!(s.in_position(45.0, Position::DialDown));
        // Moved again and straight back before a beat: undone.
        s.move_to(60.0, Position::DialDown, Position::CrownUp)
            .unwrap();
        s.move_to(60.0, Position::CrownUp, Position::DialDown)
            .unwrap();
        assert_eq!(s.stretches().len(), 2, "{:?}", s.stretches());
        // Paused, moved: the settling waits for Resume.
        let mut p = ten();
        p.listen();
        p.pause(20.0).unwrap().unwrap();
        p.move_to(20.0, Position::DialUp, Position::CrownLeft)
            .unwrap();
        assert!(p.settles().is_empty());
        p.resumed_at(80.0);
        assert_eq!(p.settles(), [(80.0, 80.0 + SETTLE_S)]);
        assert_eq!(
            p.position_bands(Position::CrownLeft)
                .iter()
                .map(|b| b.2)
                .collect::<Vec<_>>(),
            ["Dial Up (DU)", "Crown Left (CL, 6H)"]
        );
    }

    /// Each action is done where the table allows it and refused, leaving
    /// the state as it was, everywhere else.
    #[test]
    fn every_action_in_every_state() {
        let since = Instant::now();
        let states = [
            State::Idle,
            State::Listening,
            State::Paused { since },
            State::Replaying,
            State::ReplayStopped,
            State::Analysing,
            State::Analysed,
        ];
        let actions = [
            Action::Pause,
            Action::Resume,
            Action::Move,
            Action::Save,
            Action::Stop,
        ];
        let allowed = |st: State, a: Action| match a {
            Action::Pause => st == State::Listening,
            Action::Resume => st == State::Paused { since },
            Action::Move => matches!(
                st,
                State::Listening | State::Paused { .. } | State::Replaying
            ),
            // No sound kept here, so never.
            Action::Save => false,
            Action::Stop => matches!(st, State::Listening | State::Replaying | State::Analysing),
        };
        for st in states {
            for a in actions {
                let mut s = ten();
                s.set_state(st);
                assert_eq!(s.allows(a), allowed(st, a), "{a:?} in {st:?}");
                let done = match a {
                    Action::Pause => s.pause(5.0).map(|_| ()),
                    Action::Resume => s.resume("test").map(|_| ()),
                    Action::Move => s.move_to(5.0, Position::DialUp, Position::DialDown),
                    Action::Save => continue,
                    Action::Stop => s.stop(5.0).map(|_| ()),
                };
                assert_eq!(done.is_ok(), allowed(st, a), "{a:?} in {st:?}");
                if done.is_err() {
                    assert_eq!(s.state(), st, "{a:?} in {st:?}");
                }
            }
        }
        // Where each one leads.
        let mut s = ten();
        s.listen();
        s.stop(5.0).unwrap().unwrap();
        assert!(s.paused() && s.mic());
        s.resume("test").unwrap();
        assert_eq!(s.state(), State::Listening);
        s.replay();
        s.stop(5.0).unwrap().unwrap();
        assert_eq!(s.state(), State::ReplayStopped);
        s.analyse();
        s.stop(5.0).unwrap().unwrap();
        assert_eq!(s.state(), State::Idle);
    }
}
