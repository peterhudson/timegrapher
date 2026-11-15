//! The recording's USB-line clock read as its sound is heard (#1721):
//! live readings are put on the clock the take would be read on, saved
//! and opened again at that moment, core's `take::live_clock`.

use super::*;
use std::sync::mpsc::Receiver;
use timegrapher_core::usbclock::{self, LiveClock};

/// The USB lines of the sound heard, measured off the window's thread.
pub(super) struct LiveUsb {
    live: usbclock::Live,
    sample_rate: u32,
    /// A measurement under way, of the first so many seconds.
    job: Option<(f64, Receiver<LiveClock>)>,
    /// The clock and kept correction the readings were last put on.
    applied: Option<(LiveClock, Option<f64>)>,
}

impl LiveUsb {
    pub(super) fn new(sample_rate: u32) -> Self {
        LiveUsb {
            live: usbclock::Live::new(sample_rate),
            sample_rate,
            job: None,
            applied: None,
        }
    }

    /// The samples after those heard so far.
    pub(super) fn push(&mut self, block: &[f32]) {
        self.live.push(block);
    }

    /// Take a finished measurement in, and start the one due.
    fn follow(&mut self) {
        if let Some((over_s, rx)) = &self.job {
            match rx.try_recv() {
                Ok(c) => self.live.set(c),
                // A read that failed found no lines, so it isn't started
                // again every frame (Percy on #1803).
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.live.set(LiveClock::NoLines { over_s: *over_s });
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
            }
            self.job = None;
        }
        if let Some(over_s) = self.live.due() {
            let (x, fs) = (self.live.samples(over_s).to_vec(), self.sample_rate);
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(usbclock::measure_at(&x, fs, over_s));
            });
            self.job = Some((over_s, rx));
        }
    }

    /// Where the clock stands.
    pub(super) fn clock(&self) -> LiveClock {
        self.live.clock()
    }
}

impl TimegrapherApp {
    /// Follow the USB-line clock of the sound being heard: once a
    /// measurement or the kept correction changes, the readings are put
    /// on the clock a reopened take would read (#1721).
    pub(super) fn follow_live_usb(&mut self) {
        let ppm = self.clock.as_ref().map(|c| c.ppm);
        let duration_s = self.live.as_ref().map_or(0.0, |l| l.duration_s());
        let Some(u) = self.live_usb.as_mut() else {
            return;
        };
        u.follow();
        let now = (u.clock(), ppm);
        // Not measured yet changes every second it is heard; only a
        // change of state matters to the clock.
        let same = |a: &(LiveClock, Option<f64>)| match (a.0, now.0) {
            (LiveClock::NotMeasured { .. }, LiveClock::NotMeasured { .. }) => a.1 == now.1,
            _ => *a == now,
        };
        if u.applied.as_ref().is_some_and(same) {
            return;
        }
        u.applied = Some(now);
        let fit = timegrapher_core::take::live_clock(&now.0, ppm, duration_s);
        let source = match now.0 {
            LiveClock::Measured { clock, .. } => Some(clock.correction().1),
            _ => self.clock.as_ref().map(|c| c.source_text()),
        };
        let source = source.filter(|_| fit.is_some());
        self.put_take_clock(fit, source);
    }

    /// What the Rate must say while the recording's own clock is still to
    /// be read, in core's words; none once it is read.
    pub(super) fn live_usb_not_yet(&self) -> Option<String> {
        let u = self.live_usb.as_ref()?;
        u.clock().not_yet(self.clock.is_some())
    }
}

/// The clock readings are put on, for a log `duration_s` long: for a
/// take opened (`opened`), its own clock as core read it, or none; for a
/// session heard here, the clock it is read on as it is heard (#1721),
/// else the clock kept for the input (`ppm` fast).
pub(super) fn reading_clock(
    take_clock: Option<&timegrapher_core::clock::ClockFit>,
    opened: bool,
    ppm: Option<f64>,
    duration_s: f64,
) -> Option<timegrapher_core::clock::ClockFit> {
    if opened || take_clock.is_some() {
        take_clock.cloned()
    } else {
        // The kept correction, as core puts a take with no clock log on it.
        timegrapher_core::take::clock(None, 0, 0, None, ppm, duration_s)
            .ok()
            .flatten()
    }
}

#[cfg(test)]
impl LiveUsb {
    /// A measurement's answer, as the background read gives it.
    fn measured(&mut self, c: LiveClock) {
        self.live.set(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use timegrapher_core::usbclock::UsbClock;

    /// The Positions table's mean rate of the take heard, once read.
    fn table_rate(app: &TimegrapherApp) -> f64 {
        let ctx = egui::Context::default();
        for _ in 0..1000 {
            if let Some(t) = app.free_table_shown(&ctx) {
                return t.mean[0].expect("a mean rate");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the table was never read");
    }

    /// Heard here, the readings are on the clock the take would be read on
    /// saved and opened again (#1721): the card's until the USB lines are
    /// read, the Rate saying so meanwhile, then the lines' (19.3 ppm slow,
    /// 1.67 s/d), as core's `take::clock` puts an opened take on them.
    #[test]
    fn a_live_rate_takes_the_usb_lines_a_reopened_take_takes() {
        let mut app = super::super::tests::synthetic(40.0, 20.0);
        assert!(app.live_usb.is_some(), "a session heard here reads them");
        app.follow_live_usb();
        let not_yet = app.live_usb_not_yet().expect("said before they are read");
        assert!(not_yet.contains("USB lines"), "{not_yet}");
        let rate = |app: &TimegrapherApp| {
            let l = app.live.as_ref().unwrap();
            l.reading(app.average_s).rate_s_per_day.expect("a rate")
        };
        let heard = rate(&app);
        let before = table_rate(&app);
        let lines = UsbClock {
            ppm: -19.3,
            lines: 8,
            spread_ppm: 0.2,
        };
        let over_s = usbclock::MAX_DURATION_S;
        let u = app.live_usb.as_mut().unwrap();
        u.measured(LiveClock::Measured {
            clock: lines,
            over_s,
        });
        app.follow_live_usb();
        assert_eq!(app.live_usb_not_yet(), None);
        let d = app.live.as_ref().unwrap().duration_s();
        let opened = timegrapher_core::take::clock(None, 0, 0, Some(-19.3), None, d)
            .unwrap()
            .unwrap();
        let fit = reading_clock(app.take_clock.as_ref(), false, None, d).expect("the lines'");
        assert_eq!(format!("{fit:?}"), format!("{opened:?}"));
        // A card 19.3 ppm slow reads the watch 1.67 s/d fast: on the lines'
        // clock the rate falls by that, as in Paul's report (#1720).
        let shift = rate(&app) - heard;
        let want = -timegrapher_core::clock::rate_error_s_per_day(-19.3).abs();
        assert!((shift - want).abs() < 0.05, "{heard} then {}", rate(&app));
        // The table read before is read again on the new clock (Norm).
        let after = table_rate(&app);
        assert!(
            (after - before - want).abs() < 0.05,
            "{before} then {after}"
        );
        assert!(
            (shift.abs() - want.abs()).abs() < 0.05,
            "{heard} then {}",
            rate(&app)
        );
    }
}
