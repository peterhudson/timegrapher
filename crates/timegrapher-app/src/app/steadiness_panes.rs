//! The Steadiness and Isochronism panes: their settings, jobs and drawing.

use super::*;

impl TimegrapherApp {
    /// A Steadiness pane's Rate, Amplitude and Beat Error switches.
    /// The series a steadiness pane shows: its own switches, or for the
    /// two maps their one shared choice, one map at a time.
    pub(super) fn series_shown(&self, view: steady::View) -> [bool; 3] {
        if view.is_map() {
            steady::one_series(
                self.steady_series
                    .get(&steady::View::Stft)
                    .copied()
                    .unwrap_or(RATE_ONLY),
            )
        } else {
            self.steady_series.get(&view).copied().unwrap_or(RATE_ONLY)
        }
    }

    pub(super) fn steady_settings(&mut self, ui: &mut egui::Ui, pane: Pane) {
        let Some(view) = pane.steady_view() else {
            return;
        };
        if view.is_map() {
            steady::series_choice(
                ui,
                self.steady_series
                    .entry(steady::View::Stft)
                    .or_insert(RATE_ONLY),
            );
        } else {
            steady::series_switches(ui, self.steady_series.entry(view).or_insert(RATE_ONLY));
        }
        let accent = theme::pal(ui).accent;
        if view == steady::View::Periods {
            theme::bar_gap(ui);
            theme::tool_switch(
                ui,
                Icon::Db,
                accent,
                "dB Scale",
                &mut self.periods_db,
                "Show the scores in decibels. Off: linear.",
            );
        }
        // Timing changes only how rate is drawn, so it can be used only
        // while rate is.
        if view == steady::View::Fft {
            theme::bar_gap(ui);
            let rate_shown = self.series_shown(view)[0];
            theme::tool_switch_unless(
                ui,
                Icon::Stopwatch,
                accent,
                "Timing",
                &mut self.fft_timing,
                "Draw rate as the beat timing's spectrum, in ms, at every period, rather than \
                 the swing it makes in 10 s rate readings, in s/d",
                (!rate_shown).then_some("Turn Rate on to draw it as the beat timing's spectrum."),
            );
        }
        if matches!(view, steady::View::Stft | steady::View::Wavelet) {
            theme::bar_gap(ui);
            let mut db = self.maps_db.on(view);
            theme::tool_switch(
                ui,
                Icon::Db,
                accent,
                "dB Scale",
                &mut db,
                "Show the sizes in decibels. Off: linear.",
            );
            self.maps_db.set(view, db);
        }
        if matches!(
            view,
            steady::View::Periods | steady::View::Fft | steady::View::Stft | steady::View::Wavelet
        ) {
            let mut wheels = self.show_wheels.on(view);
            theme::tool_switch(
                ui,
                Icon::Wheel,
                accent,
                "Wheel Periods",
                &mut wheels,
                steady::WHEELS_HINT,
            );
            self.show_wheels.set(view, wheels);
        }
    }

    pub(super) fn update_steady(&mut self, ctx: &egui::Context) -> f64 {
        // What was read before the take's settings, clock, breaks or masks
        // last changed is read again.
        if self.steady_job.as_ref().is_some_and(|j| j.0 != self.rev) {
            self.steady_job = None;
        }
        if self.steady.as_ref().is_some_and(|s| s.0 != self.rev) {
            self.steady = None;
        }
        if let Some((rev, rx)) = &self.steady_job {
            match rx.try_recv() {
                Ok(s) => {
                    self.steady = Some((*rev, s));
                    self.steady_job = None;
                }
                Err(TryRecvError::Disconnected) => self.steady_job = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        // Only the beats in the position the watch is in now, joined.
        let runs = self.current_runs();
        let clock = Self::rate_clock(&self.take_clock).cloned();
        let joined = !self.session.stretches().is_empty();
        let dur = self.live.as_ref().map_or(0.0, |l| {
            steady::joined_duration(
                &runs.read,
                l.duration_s(),
                clock.as_ref().filter(|_| joined),
            )
        });
        let upto = self.steady.as_ref().map(|s| s.1.upto_s);
        let due = match upto {
            None => true,
            Some(u) if self.running() => dur - u >= steady::RERUN_S,
            Some(u) => dur - u > 1.0,
        };
        if self.steady_job.is_none() && due && dur >= steady::MIN_S {
            let log = self.live.as_ref().and_then(steady::log_of);
            if let Some((log, breaks)) = log.map(|l| self.steady_log(l, &runs, clock.as_ref())) {
                let wheels = self.calibre_wheels();
                let meshes = self.calibre_meshes();
                let clock = clock.filter(|_| !joined); // A joined log is on it.
                let (tx, rx) = std::sync::mpsc::channel();
                let ctx = ctx.clone();
                let stage = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
                self.steady_work = (stage.clone(), std::time::Instant::now());
                std::thread::spawn(move || {
                    let mut s = steady::compute_with(&log, &breaks, wheels, clock.as_ref(), &stage);
                    s.meshes = meshes;
                    let _ = tx.send(s);
                    ctx.request_repaint();
                });
                self.steady_job = Some((self.rev, rx));
            }
        }
        dur
    }

    /// What the steadiness tests read of `log`, and the cuts they stop at:
    /// a take with no position marked whole, at its own breaks with the time
    /// each gap held and every other jump, as `timegrapher series` reads it;
    /// else the position's `runs` joined end to end on the take's `clock`
    /// ([`steady::join`]), each hole in a run left out and cut at both ends.
    pub(super) fn steady_log(
        &self,
        mut log: BeatLog,
        runs: &timegrapher_core::take_context::Stretches<Position>,
        clock: Option<&timegrapher_core::clock::ClockFit>,
    ) -> (BeatLog, Vec<timegrapher_core::breaks::Break>) {
        let mut opened = self.take_breaks.clone().unwrap_or_default();
        if self.session.stretches().is_empty() {
            let breaks = steady::reading_breaks(opened, &log);
            (log, breaks)
        } else {
            // A hole in a run is left out and cut at both ends, as `series`
            // reads a take's holes (#1156). The masks by hand are already
            // out of `log`, cut among its jumps (`steady::log_of`).
            let mut no_masks = Vec::new();
            timegrapher_core::session::leave_out(&mut log, &mut opened, &mut no_masks, &runs.holes);
            steady::join(&log, &runs.read, &opened, clock)
        }
    }

    /// While the steadiness tests run: a spinner, the step they're on and
    /// how long they've taken, for the Steadiness and Isochronism panes.
    /// Nothing when they aren't running.
    fn steady_working(&self, ui: &mut egui::Ui) {
        if self.steady_job.is_none() {
            return;
        }
        let pal = theme::pal(ui);
        let (stage, since) = &self.steady_work;
        let stage = stage.load(std::sync::atomic::Ordering::Relaxed);
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(12.0).color(pal.text_secondary));
            ui.label(
                RichText::new(format!(
                    "{}… {}",
                    steady::stage_text(stage),
                    length_text(since.elapsed().as_secs_f64())
                ))
                .small()
                .color(pal.text_secondary),
            );
        })
        .response
        .on_hover_text(
            "The tests run in the background over every reading of the session, so a \
             session of many hours takes a while; what is shown meanwhile is the last \
             answer, if there is one.",
        );
    }

    pub(super) fn isochronism_pane(&mut self, ui: &mut egui::Ui) {
        let dur = self.update_steady(ui.ctx());
        let pal = theme::pal(ui);
        self.steady_working(ui);
        match &self.steady {
            Some((_, s)) => {
                let walk = s
                    .report
                    .walk
                    .as_ref()
                    .and_then(timegrapher_core::steady_views::walk_view);
                let view = self.iso_view;
                isochronism::ui(ui, s.report.isochronism.as_ref(), walk.as_ref(), view);
            }
            None if self.steady_job.is_some() => {}
            None => {
                ui.label(
                    RichText::new(self.steady_wait(dur))
                        .small()
                        .color(pal.text_secondary),
                );
            }
        }
    }

    /// What the steadiness tests wait for, in the terms of what is being
    /// done: a Positions Test never stays long enough in one position.
    fn steady_wait(&self, dur: f64) -> String {
        if self.guided.is_some() {
            "Needs 5 minutes of beats in one position, longer than a Positions Test \
             stays in each. Record one position for 5 minutes or more to see it."
                .to_string()
        } else {
            format!(
                "Needs 5 minutes of beats in one position; {} so far.",
                length_text(dur)
            )
        }
    }

    pub(super) fn steady_pane(&mut self, ui: &mut egui::Ui, view: steady::View) {
        let dur = self.update_steady(ui.ctx());
        let upto = self.steady.as_ref().map(|s| s.1.upto_s);
        let working = self.steady_job.is_some();
        let joined = !self.session.stretches().is_empty();
        let of = if !joined {
            String::new()
        } else {
            format!("{} ", self.position.name())
        };
        self.steady_working(ui);
        let status = match upto {
            Some(u) => format!("Tested over {} of {of}beats", length_text(u)),
            None if working => String::new(),
            None => self.steady_wait(dur),
        };
        steady::draw(
            ui,
            self.steady.as_ref().map(|s| &s.1),
            &status,
            view,
            &self.series_shown(view),
            self.steady_opts(view),
        );
    }

    /// How a steadiness pane draws `view`. On the take's own clock its
    /// time is the take's span, the strip's; a position's stretches joined
    /// end to end are on a clock of their own, named on the axis (#1256).
    pub(super) fn steady_opts(&self, view: steady::View) -> steady::Opts {
        let joined = !self.session.stretches().is_empty();
        steady::Opts {
            periods_db: self.periods_db,
            fft_timing: self.fft_timing,
            maps_db: self.maps_db.on(view),
            wheels: self.show_wheels.on(view),
            info: self.steady_info.on(view),
            take_span: self.take_span().filter(|_| !joined),
            joined_in: joined.then(|| self.position.code()),
        }
    }
}
