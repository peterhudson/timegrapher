//! A session's life: listening, paused, the positions the watch was put
//! in, the settling after each move, and what has been saved. The app asks
//! the session to act and draws what it says; the stretches, the settling,
//! the unsaved sound and the saved take's position ranges are all worked
//! out from one list of events, so they can't drift apart.
//!
//! Event times are the session's seconds, the clock the strip and the
//! charts run on (it counts pauses). A move also keeps the seconds of
//! saved sound at that moment, which leave pauses out, for the take's
//! `session.toml`.
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
//! Listen, Replay, Open and New Session are allowed in every state: each
//! stops what is under way first. Save needs the sound kept as well.

use crate::app::Position;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;
use timegrapher_core::recorder::{Recorder, SETTLE_S};

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
    /// A recording played through at its own pace.
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

#[derive(Default)]
pub struct Session {
    state: State,
    /// The sound kept as it comes in, for saving.
    pub recorder: Option<Recorder>,
    /// Moved while paused: the settling starts on Resume.
    settle_pending: bool,
    /// The end of the settling under way, when the readings start afresh.
    settle_to: Option<f64>,
    events: Vec<Event>,
}

impl Session {
    /// Forget everything but the recorder, which the caller discards or
    /// keeps.
    pub fn clear(&mut self) {
        self.state = State::Idle;
        self.settle_pending = false;
        self.settle_to = None;
        self.events.clear();
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// Whether `action` is allowed now.
    pub fn allows(&self, action: Action) -> bool {
        use State::*;
        match action {
            Action::Pause => self.state == Listening,
            Action::Resume => matches!(self.state, Paused { .. }),
            Action::Move => matches!(self.state, Listening | Paused { .. } | Replaying),
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

    /// A recording is playing through.
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
    pub fn stop(&mut self) -> Result<io::Result<()>, Refused> {
        self.check(Action::Stop)?;
        match self.state {
            State::Listening => return self.pause(),
            State::Replaying => self.state = State::ReplayStopped,
            _ => self.state = State::Idle,
        }
        Ok(Ok(()))
    }

    /// Paused now (the microphone let go). The inner error is the
    /// recorder's, when it couldn't close the stretch.
    pub fn pause(&mut self) -> Result<io::Result<()>, Refused> {
        self.check(Action::Pause)?;
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
    /// start a settling that a move while paused left waiting.
    pub fn resumed_at(&mut self, now: f64) {
        if self.settle_pending {
            self.start_settle(now);
        }
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
        if !self
            .events
            .iter()
            .any(|e| matches!(e, Event::Position { .. }))
        {
            self.events.push(Event::Position {
                at_s: 0.0,
                audio_s: self.recorder.as_ref().map(|_| 0.0),
                pos: old,
            });
        }
        // A position changed back before any beats came is undone.
        if let Some(i) = self.last_position_index() {
            if let Event::Position { at_s, .. } = self.events[i] {
                if at_s >= at {
                    self.events.remove(i);
                }
            }
        }
        if self.stretches().last().is_none_or(|s| s.1 != pos) {
            self.events.push(Event::Position {
                at_s: at,
                audio_s: self.recorder.as_ref().map(Recorder::duration_s),
                pos,
            });
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

    fn last_position_index(&self) -> Option<usize> {
        self.events
            .iter()
            .rposition(|e| matches!(e, Event::Position { .. }))
    }

    /// Start the settling at `t`.
    pub fn start_settle(&mut self, t: f64) {
        self.settle_pending = false;
        // A move during a settling starts it again: the first ends there.
        for e in self.events.iter_mut().rev() {
            if let Event::Settle { from_s, to_s } = e {
                if *to_s > t {
                    *to_s = t.max(*from_s);
                }
                break;
            }
        }
        self.events
            .retain(|e| !matches!(e, Event::Settle { from_s, to_s } if to_s <= from_s));
        self.events.push(Event::Settle {
            from_s: t,
            to_s: t + SETTLE_S,
        });
        self.settle_to = Some(t + SETTLE_S);
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
        self.events
            .iter()
            .filter_map(|e| match e {
                Event::Position { at_s, pos, .. } => Some((*at_s, *pos)),
                _ => None,
            })
            .collect()
    }

    /// The settling after each move: start and end, oldest first.
    pub fn settles(&self) -> Vec<(f64, f64)> {
        self.events
            .iter()
            .filter_map(|e| match e {
                Event::Settle { from_s, to_s } => Some((*from_s, *to_s)),
                _ => None,
            })
            .collect()
    }

    /// The positions and settling a take's session file gives, for a
    /// recording opened.
    pub fn load(&mut self, stretches: &[(f64, Position)], settles: &[(f64, f64)]) {
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
        let settles = self.settles();
        self.position_spans(now)
            .into_iter()
            .filter(|s| s.2 == now)
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

    pub fn mark_saved(&mut self, through_s: f64, to: PathBuf) {
        self.events.push(Event::Saved { through_s, to });
    }

    /// Kept sound not in the last saved copy, seconds.
    pub fn unsaved_s(&self) -> f64 {
        match &self.recorder {
            Some(r) => (r.duration_s() - self.saved_through_s()).max(0.0),
            None => 0.0,
        }
    }

    /// Close and delete the sound kept for saving, and forget the saves.
    pub fn discard_recording(&mut self) {
        if let Some(r) = self.recorder.take() {
            if let Ok(dir) = r.finish(None) {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
        self.events.retain(|e| !matches!(e, Event::Saved { .. }));
    }

    /// `t` seconds of the session as seconds of the kept sound, from the
    /// last position change at or before it (the sound leaves out the
    /// pauses the session's clock counts).
    pub fn audio_of(&self, t: f64) -> Option<f64> {
        self.recorder.as_ref()?;
        let (at, a) = self
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Position {
                    at_s,
                    audio_s: Some(a),
                    ..
                } if *at_s <= t + 1e-9 => Some((*at_s, *a)),
                _ => None,
            })
            .next_back()
            .unwrap_or((0.0, 0.0));
        Some(a + (t - at))
    }

    /// The saved take's positions in its sound's own seconds (which leave
    /// out the pauses): start, end and position code. Each later position
    /// starts once it has settled; one entry with no times while the watch
    /// stayed put.
    pub fn take_ranges(&self) -> Vec<(Option<f64>, Option<f64>, String)> {
        let Some(r) = self.recorder.as_ref() else {
            return Vec::new();
        };
        let mut stretches: Vec<(f64, String)> = vec![(0.0, r.first_position().to_string())];
        for e in &self.events {
            if let Event::Position {
                audio_s: Some(a),
                pos,
                ..
            } = e
            {
                let code = pos.code().to_string();
                if stretches.last().is_some_and(|s| s.0 >= *a) {
                    stretches.pop();
                }
                if stretches.last().is_none_or(|s| s.1 != code) {
                    stretches.push((*a, code));
                }
            }
        }
        let one = stretches.len() == 1;
        (0..stretches.len())
            .map(|i| {
                let end = stretches.get(i + 1).map(|s| s.0);
                let settle = if i == 0 { 0.0 } else { SETTLE_S };
                let start = (stretches[i].0 + settle).min(end.unwrap_or(f64::INFINITY));
                ((!one).then_some(start), end, stretches[i].1.clone())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The events give the stretches, the settling and the spans; a move
    /// back before any beats undoes the move; a move while paused settles
    /// from Resume.
    #[test]
    fn events_give_stretches_settles_and_spans() {
        let mut s = Session::default();
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
        let mut p = Session::default();
        p.listen();
        p.pause().unwrap().unwrap();
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
                let mut s = Session::default();
                s.set_state(st);
                assert_eq!(s.allows(a), allowed(st, a), "{a:?} in {st:?}");
                let done = match a {
                    Action::Pause => s.pause().map(|_| ()),
                    Action::Resume => s.resume("test").map(|_| ()),
                    Action::Move => s.move_to(5.0, Position::DialUp, Position::DialDown),
                    Action::Save => continue,
                    Action::Stop => s.stop().map(|_| ()),
                };
                assert_eq!(done.is_ok(), allowed(st, a), "{a:?} in {st:?}");
                if done.is_err() {
                    assert_eq!(s.state(), st, "{a:?} in {st:?}");
                }
            }
        }
        // Where each one leads.
        let mut s = Session::default();
        s.listen();
        s.stop().unwrap().unwrap();
        assert!(s.paused() && s.mic());
        s.resume("test").unwrap();
        assert_eq!(s.state(), State::Listening);
        s.replay();
        s.stop().unwrap().unwrap();
        assert_eq!(s.state(), State::ReplayStopped);
        s.analyse();
        s.stop().unwrap().unwrap();
        assert_eq!(s.state(), State::Idle);
    }
}
