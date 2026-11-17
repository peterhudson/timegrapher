//! The Paper Strip pane: its trend, settings, fit and input.

use super::*;
use timegrapher_core::steady_views;

impl TimegrapherApp {
    /// Show the whole session so far along the strip, ending at its latest
    /// beat, and keep it so as beats arrive: Fit on.
    pub(super) fn fit_strip(&mut self) {
        // Still paper has nothing to fit: its Length is the lap.
        if self.strip.still {
            return;
        }
        self.strip_fit = true;
        self.follow_fit();
    }

    /// An opened take shows whole: the strip's length fits all of it, and
    /// its width is the narrowest of the offered widths that holds every
    /// beat without wrapping. The beats are centred between the earliest
    /// and the latest, or, with Auto-Centre on, the newest stay on the
    /// centre line as it keeps them. The odd stray dot (one beat in a
    /// thousand at each end) is left out of the width, so one bad
    /// detection can't flatten the trace. The view before is kept for the
    /// next Listen.
    pub(super) fn fit_take(&mut self) {
        let before = self.strip;
        if self.live_strip.is_none() {
            self.live_fit = self.strip_fit;
        }
        self.fit_view();
        let live = self.live_strip.map_or(before, |(live, _)| live);
        self.live_strip = Some((live, self.strip));
    }

    /// The strip's view a live session goes back to: the person's own
    /// width and length from before a take was fitted, each unless they
    /// set it by hand since, which stays (Hugh).
    pub(super) fn live_view(&self) -> StripView {
        let mut v = self.strip;
        if let Some((live, fitted)) = self.live_strip {
            if v.half_width_ms == fitted.half_width_ms {
                v.half_width_ms = live.half_width_ms;
            }
            if v.span_s == fitted.span_s {
                v.span_s = live.span_s;
            }
        }
        v
    }

    /// A new take, a test, or listening after an opened take: the strip
    /// goes back to its live view, and Fit to how it was if the length
    /// does.
    pub(super) fn restore_live_strip(&mut self) {
        if matches!(self.live_strip, Some((_, fitted)) if fitted.span_s == self.strip.span_s) {
            self.strip_fit = self.live_fit;
        }
        self.strip = self.live_view();
        self.live_strip = None;
    }

    /// Fit the strip's length and width to the take on show.
    fn fit_view(&mut self) {
        self.fit_strip();
        let Some(live) = &self.live else { return };
        let Some(bph) = live.bph() else { return };
        let period = 3600.0 / bph as f64;
        let beats = live.beats();
        let breaks = self.take_breaks.as_deref().unwrap_or_default();
        let clock = self.take_rate_clock();
        let centred = if self.follow {
            Anchor::centre_on_true(beats, period, 8, breaks, clock)
        } else {
            Anchor::centre_on_true(&beats[..beats.len().min(16)], period, 16, breaks, clock)
        };
        let Some(anchor) = centred else { return };
        let mut leads: Vec<f64> = beats
            .iter()
            .map(|b| anchor.lead_on_true(b, period, breaks, clock))
            .collect();
        leads.sort_by(f64::total_cmp);
        let stray = leads.len() / 1000;
        let (lo, hi) = (leads[stray], leads[leads.len() - 1 - stray]);
        let (anchor, half) = if self.follow {
            (anchor, lo.abs().max(hi.abs()))
        } else {
            // An earlier anchor time makes every beat read later by as
            // much, which puts the middle of the spread on the centre line.
            let mid = (lo + hi) / 2.0;
            let shifted = Anchor {
                time: anchor.time - mid / 1000.0,
                ..anchor
            };
            (shifted, (hi - lo) / 2.0)
        };
        self.anchor = Some(anchor);
        // A little room at the edges, so the outermost dots aren't cut.
        let need = half * 1.1;
        let width = WIDTHS
            .iter()
            .copied()
            .find(|&w| w >= need)
            .unwrap_or(StripView::MAX_HALF_WIDTH_MS);
        self.strip.set_half_width(width);
    }

    /// The take's span on every stacked time pane's axis, the fitted
    /// strip's, the charts' and Readings Over Time's: from its start to its
    /// end or now, its newest beat or its newest reading, whichever is
    /// later (a reading's time is its window's end). None before there is
    /// any (Hugh, #1252).
    pub(super) fn take_span(&self) -> Option<(f64, f64)> {
        let end = self.trend.last().map_or(0.0, |p| p.t).max(self.latest_s());
        (end > 0.0).then_some((0.0, end))
    }

    /// The time at the strip's newest edge: with Fit on, the take's end,
    /// as the charts under it end (#1256).
    pub(super) fn strip_end_s(&self) -> f64 {
        let latest = if self.strip_fitted() {
            self.take_span().map_or(0.0, |(_, end)| end)
        } else {
            self.latest_s()
        };
        self.strip_end.unwrap_or(latest).min(latest)
    }

    /// Fit is on and the paper moves: still paper has nothing to fit, so
    /// Fit waits greyed until the paper moves again.
    pub(super) fn strip_fitted(&self) -> bool {
        self.strip_fit && !self.strip.still
    }

    /// The stretches the strip draws faintly, each with what the ticker
    /// says over it: every settling, as the engine culls it
    /// ([`super::masks::settlings`]), and during and after a Positions
    /// Test the rest of the time it didn't use. Settling wins where the
    /// two overlap: what is shaded as settling is not read, and what is
    /// culled is shaded (Hugh).
    pub(super) fn strip_settles(&self) -> Vec<(f64, f64, &'static str)> {
        let settling = super::masks::settlings(&self.session, &self.guided_unused);
        let rest = self
            .guided_unused
            .iter()
            .filter(|s| s.2 != guided::SETTLING)
            .flat_map(|&(a, b, kind)| {
                crate::strip::showing(a, b, settling.iter().copied())
                    .into_iter()
                    .map(move |(x, y)| (x, y, kind))
            });
        let mut out: Vec<(f64, f64, &'static str)> = settling
            .iter()
            .map(|&(a, b)| (a, b, guided::SETTLING))
            .chain(rest)
            .collect();
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    }

    /// The strip settings' Draw Over the Strip switches: Rate, Amplitude,
    /// Beat Error.
    fn strip_overlays(&mut self, ui: &mut egui::Ui, pal: &theme::Palette) {
        let overlays = &mut self.overlays;
        for (icon, name) in [
            (Icon::Rate, "Rate"),
            (Icon::Amplitude, "Amplitude"),
            (Icon::BeatError, "Beat Error"),
        ] {
            theme::note_icon(ui, icon, &format!("Draw Over the Strip: {name}"));
        }
        theme::bar_item(ui, "Draw Over the Strip", |ui| {
            let over = "Readings drawn as lines over the beats against the same time, each \
                        on its own scale";
            // Rate, Amplitude, Beat Error, as everywhere; `overlays` keeps
            // its own order (amplitude, rate, beat error) for old settings.
            let lines = [
                (
                    1,
                    Icon::Rate,
                    pal.trace_rate,
                    "Rate",
                    "The rate readings as a magenta line against the same time, on their own \
                     scale",
                ),
                (
                    0,
                    Icon::Amplitude,
                    pal.trace_amplitude,
                    "Amplitude",
                    "The amplitude readings as a violet line against the same time, on \
                     their own scale",
                ),
                (
                    2,
                    Icon::BeatError,
                    pal.trace_beat_error,
                    "Beat Error",
                    "The beat error readings, from the unlock, as an amber line against the \
                     same time, on their own scale",
                ),
            ];
            if theme::in_toolbar(ui) {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, icon, colour, name, hint) in lines {
                    if theme::icon_toggle(ui, icon, overlays[i], colour)
                        .on_hover_text(format!("Draw Over the Strip: {name}: {hint}"))
                        .clicked()
                    {
                        overlays[i] = !overlays[i];
                    }
                }
            } else {
                ui.label(RichText::new("Draw Over the Strip").strong())
                    .on_hover_text(over);
                for (i, _, _, name, hint) in lines {
                    theme::switch_row(ui, name, &mut overlays[i], hint);
                }
            }
        });
    }

    /// The strip settings' Width and Length fields and the Fit switch.
    fn strip_span_fields(&mut self, ui: &mut egui::Ui, pal: &theme::Palette) {
        theme::tool_field(
            ui,
            "Width",
            "Milliseconds either side of the centre line: type it, or pick one with ⏷. \
             Alt (Option on a Mac) and the mouse wheel on the strip change it too.",
            |ui| {
                let mut w = self.strip.half_width_ms;
                let (_, changed) = fields::combo(
                    ui,
                    "width",
                    &mut w,
                    StripView::MIN_HALF_WIDTH_MS..=StripView::MAX_HALF_WIDTH_MS,
                    &HALF_WIDTH,
                    &WIDTHS,
                    width_label,
                    "",
                    // Holds the widest width whole, such as ±888.8 ms.
                    112.0,
                );
                if changed {
                    self.strip.set_half_width(w);
                }
            },
        );
        theme::tool_field(
            ui,
            "Length",
            &format!(
                "Time shown along the strip, such as 30 s, 5 min or 2 h, and no less than \
                 {}: type it, or pick one with ⏷. Cmd (Mac) or Ctrl and the mouse wheel on \
                 the strip change it too, out to the whole take.",
                fields::duration(StripView::MIN_SPAN_S)
            ),
            |ui| {
                let mut s = self.strip.span_s;
                let (_, changed) = fields::combo(
                    ui,
                    "span",
                    &mut s,
                    StripView::MIN_SPAN_S..=StripView::MAX_SPAN_S,
                    &DURATION,
                    &SPANS,
                    fields::duration,
                    "",
                    92.0,
                );
                if changed {
                    self.strip_fit = false;
                    self.strip.set_span(s);
                }
            },
        );
        let mut fit = self.strip_fitted();
        if theme::tool_switch_unless(
            ui,
            Icon::Fit,
            pal.accent,
            "Fit",
            &mut fit,
            "Show the whole session along the strip, up to its latest beat (its last 80 hours \
             when it is longer), and keep it in view as beats arrive. Zooming or dragging the \
             strip's time turns it off.",
            self.strip
                .still
                .then_some("The paper is still, so there's nothing to fit"),
        ) {
            if fit {
                self.fit_strip();
            } else {
                self.strip_fit = false;
            }
        }
    }

    /// The strip settings' Mask tool and Position Brush, which exclude each
    /// other.
    fn strip_mask_brush(&mut self, ui: &mut egui::Ui, pal: &theme::Palette) {
        let live = self.live.is_some();
        theme::note_icon(ui, Icon::Masks, "Mask");
        theme::bar_item(ui, "Mask", |ui| {
            ui.add_enabled_ui(live, |ui| {
                if theme::in_toolbar(ui) {
                    let r = theme::icon_toggle(ui, Icon::Masks, self.masking, pal.accent);
                    if r.clicked() {
                        self.masking = !self.masking;
                    }
                    r.on_hover_text(format!("Mask: {MASK_HINT}{}", self.masked_words()))
                        .on_disabled_hover_text(
                            "Mask: Nothing to mask yet. Listen, or open a recording.",
                        );
                } else {
                    theme::switch_row(ui, "Mask", &mut self.masking, MASK_HINT);
                }
            });
        });
        // The Mask tool and the Position Brush exclude each other.
        if self.masking {
            self.brush = None;
        }
        theme::note_icon(ui, Icon::Highlighter, "Position Brush");
        theme::bar_item(ui, "Position Brush", |ui| self.brush_control(ui, live));
        if self.brush.is_some() {
            self.masking = false;
        }
    }

    /// The strip settings' Direction: Down or Across. In the toolbar the
    /// segment's words say it with no caption (Hugh, so the row fits with
    /// Still Paper at 1000 pt); in the "…" menu it keeps its name.
    fn strip_direction(&mut self, ui: &mut egui::Ui) {
        let mut h = self.strip.horizontal;
        let choices = [
            (
                false,
                "Down",
                "Direction the paper runs: Down, newest beats at the top, the strip beside the \
                 charts",
            ),
            (
                true,
                "Across",
                "Direction the paper runs: Across, newest beats at the right, the strip above \
                 the charts",
            ),
        ];
        theme::tool_words(
            ui,
            "Direction",
            "Direction the paper runs: Down or Across",
            |ui| theme::segmented(ui, &mut h, &choices),
        );
        if h != self.strip.horizontal {
            self.strip.horizontal = h;
            self.relayout(h);
        }
    }

    /// The strip settings' Still Paper switch, after Direction (Hugh: a
    /// toggle, so the row fits at 1000 pt).
    fn strip_paper(&mut self, ui: &mut egui::Ui, pal: &theme::Palette) {
        let hint = if self.strip.still {
            "On. The pen wraps every Length; Fit is off"
        } else {
            "Hold the paper still and sweep the pen across it, wrapping every Length"
        };
        theme::tool_switch(
            ui,
            Icon::StillPaper,
            pal.accent,
            "Still Paper",
            &mut self.strip.still,
            hint,
        );
    }

    pub(super) fn strip_settings(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        self.strip_overlays(ui, pal);
        theme::bar_gap(ui);
        self.strip_span_fields(ui, pal);
        theme::bar_gap(ui);
        if theme::tool_button(
            ui,
            Icon::Truing,
            "Centre",
            "Put the latest beats on the centre line",
            None,
        ) {
            self.centre();
        }
        theme::tool_switch(
            ui,
            Icon::AutoCentre,
            pal.accent,
            "Auto-Centre",
            &mut self.follow,
            "Keep the newest beats on the centre line and slide the rest",
        );
        theme::bar_gap(ui);
        theme::tool_switch(
            ui,
            Icon::RateLine,
            pal.trace_rate,
            "Rate Line",
            &mut self.rate_line,
            "Draw the Rate reading as a magenta line over the beats it was fitted to, to check \
             that it follows the dots",
        );
        theme::tool_switch(
            ui,
            Icon::Rails,
            pal.trace_rate,
            "Parallel Guides",
            &mut self.rate_guides,
            "Faint lines at the Rate reading's slope across the whole strip, so it is easy \
             to see whether the dots run parallel to it. They show with or without the Rate \
             Line.",
        );
        theme::bar_gap(ui);
        self.strip_mask_brush(ui, pal);
        theme::bar_gap(ui);
        self.strip_direction(ui);
        self.strip_paper(ui, pal);
        theme::bar_gap(ui);
        self.export_button(ui, Pane::Strip);
    }

    /// The readings averaged over `seconds` for the strip's window from
    /// `from` to `end`, and how far apart they are. The run kept for the
    /// whole take serves when it is close enough there; a long take opened
    /// whole keeps one only every take/4000 s (6.7 s over 7½ hours), too
    /// sparse for a short strip, so they are worked out afresh.
    pub(super) fn strip_trend(
        &mut self,
        from: f64,
        end: f64,
        seconds: f64,
    ) -> (Vec<TrendPoint>, f64) {
        let step = strip_step(end - from);
        let kept: Vec<TrendPoint> = self
            .trend
            .iter()
            .filter(|p| p.t >= from && p.t <= end)
            .copied()
            .collect();
        let spacing = if kept.len() > 1 {
            (kept[kept.len() - 1].t - kept[0].t) / (kept.len() - 1) as f64
        } else {
            f64::INFINITY
        };
        let Some(live) = &self.live else {
            return (kept, spacing.min(step));
        };
        // The kept run serves only with no hole in it: one worked out
        // again while listening is sparse up to then and dense since, and
        // its average spacing hides the stretch with none.
        let around = (
            self.trend.first().map_or(from, |p| p.t),
            self.trend.last().map_or(end, |p| p.t),
        );
        if spacing <= 2.0 * step && widest_hole(&kept, from, end, around) <= 2.0 * step {
            return (kept, spacing);
        }
        let key: StripKey = (
            (from / step).floor() as i64,
            (end / step).ceil() as i64,
            trend_key(seconds),
            step.to_bits(),
            // While listening, in whole steps: the readings lie on steps,
            // so they are worked out again as each new one comes in, and
            // the newest catch up with the sound behind them a step later.
            // Keyed on the sound itself, every block of it worked the
            // window's 70 readings out again on every frame while
            // listening with the run too sparse to serve, as after the
            // watch stops (30 ms a frame, #1825). Stopped or paused, the
            // sound's own length, so the last sound heard counts (Trevor).
            if self.running() {
                (live.duration_s() / step).floor().to_bits()
            } else {
                live.duration_s().to_bits()
            },
            self.rev,
        );
        if let Some((k, pts)) = &self.strip_points {
            if *k == key {
                return (pts.clone(), step);
            }
        }
        let mut pts = Vec::new();
        let mut t = key.0 as f64 * step;
        while t <= (key.1 as f64 * step).min(live.duration_s()) + 1e-9 {
            if t > 0.0 && self.session.replay_reading_allowed(t) {
                pts.push(TrendPoint::of(t, &live.reading_at(t, seconds)));
            }
            t += step;
        }
        self.strip_points = Some((key, pts.clone()));
        (pts, step)
    }

    pub(super) fn strip_pane(&mut self, ui: &mut egui::Ui) {
        // Before there is a take to show, or while nothing in it beats
        // regularly: where things stand, in the strip's place.
        if self.live.is_none() || self.mic_session_set_aside() {
            // Scrolling where the strip is too short for it, never cut off.
            egui::ScrollArea::vertical()
                .id_salt("empty state")
                .auto_shrink([false, false])
                .show(ui, |ui| self.empty_state(ui));
            return;
        }
        // No regular beat yet: the peaks heard, faint, if any.
        let guessing = self.no_watch_yet();
        let candidates = match &self.live {
            Some(l) if guessing => l.candidates(),
            _ => Vec::new(),
        };
        if guessing && candidates.is_empty() {
            return self.no_watch(ui);
        }
        self.note_need(ui, Pane::Strip, STRIP_MIN_H);
        self.follow_fit();
        if self.follow {
            self.centre();
        }
        let end = self.strip_end_s();
        let looking_back = self.strip_end.is_some() && end < self.latest_s() - 0.5;
        let note = strip_note(guessing, looking_back, end);
        let from = end - self.strip.span_s - 5.0;
        let (trend, _) = self.strip_trend(from, end, self.average_s);
        let Some(live) = &self.live else { return };
        let period = live.bph().map_or(0.125, |b| 3600.0 / b as f64);
        let rate_line = self.strip_rate_line();
        let mut overlays = strip_overlays_drawn(self.overlays, &trend, from, end, theme::pal(ui));
        let bands = self.position_bands();
        let (track, painted) = self.strip_tracks();
        let mut settles = self.strip_settles();
        if guessing {
            // Drawn faintly, as the beats the readings leave out are.
            overlays.clear();
            settles = vec![(from, end + 1.0, "Possible Beats")];
        }
        let rate_line = rate_line.filter(|_| !guessing);
        let extras = strip::Extras {
            note: note.as_deref(),
            rate_line,
            line: self.rate_line,
            guides: self.rate_guides,
            overlays: &overlays,
            cursor_t: self.cursor_t,
            gutter: theme::time_gutter(ui.ctx()),
            bands: &bands,
            settles: &settles,
            measured: &self.guided_measured,
            masked: &self.masked,
            masking: self.masking,
            track: &track,
            brush: self.brush.map(Position::code),
            painted: &painted,
            breaks: if guessing {
                &[]
            } else {
                self.take_breaks.as_deref().unwrap_or_default()
            },
            // The clock the rate line's reading is on: the live engine's.
            clock: self.take_rate_clock(),
            // The peaks heard while guessing are no beats of the log.
            beats_gen: self
                .live
                .as_ref()
                .filter(|_| !guessing)
                .map(|l| l.beats_gen()),
        };
        let Some(live) = &self.live else { return };
        let candidates = numbered_candidates(&candidates, period);
        let anchor = if guessing {
            candidates.first().map(|b| strip::Anchor {
                index: b.index,
                time: b.time,
            })
        } else {
            self.anchor
        };
        let input = strip::draw_strip(
            ui,
            if guessing { &candidates } else { live.beats() },
            period,
            anchor,
            end,
            &self.strip,
            &extras,
            ui.available_size(),
        );
        if input.hover_t.is_some() {
            self.cursor_next = input.hover_t;
        }
        self.apply_strip_input(input, end);
    }

    /// The rate line the strip draws, when Rate Line or Parallel Guides
    /// is on: the guides follow its slope, so it is worked out for either.
    fn strip_rate_line(&self) -> Option<strip::RateLine> {
        (self.rate_line || self.rate_guides)
            .then(|| self.reading_over(self.average_s))
            .flatten()
            .and_then(|r| {
                Some(strip::RateLine {
                    from_s: r.time_s - r.span_s,
                    to_s: r.time_s,
                    rate_s_per_day: r.rate_s_per_day?,
                })
            })
    }

    /// The position track along the strip and the stretches painted with
    /// the Position Brush.
    fn strip_tracks(&self) -> (StripTrack, Vec<(f64, f64)>) {
        let track: StripTrack = self
            .session
            .position_spans(self.position)
            .into_iter()
            .map(|(a, b, p)| (a.max(0.0), b, p.label(), p.code()))
            .collect();
        let painted: Vec<(f64, f64)> = self
            .session
            .painted_spans(self.position)
            .into_iter()
            .map(|(a, b, _)| (a, b))
            .collect();
        (track, painted)
    }

    pub(super) fn apply_strip_input(&mut self, input: StripInput, end: f64) {
        if let Some((a, b)) = input.mask {
            self.mask(a, b);
        }
        if let Some((a, b)) = input.paint {
            self.paint_position(a, b);
        }
        if let Some((was, now)) = input.move_change {
            self.move_position_change(was, now);
        }
        if let Some(t) = input.unmask_at {
            self.unmask_at(t);
        }
        if let Some(t) = input.unpaint_at {
            self.remove_painting(t);
        }
        if let Some((was, now)) = input.reshape {
            self.reshape_mask(was, now);
        }
        if input.span_factor != 1.0 || input.time_shift_s != 0.0 {
            // Zooming or panning time is the strip by hand: Fit goes off.
            self.strip_fit = false;
        }
        if input.span_factor != 1.0 {
            let was = self.strip.span_s;
            // Zoomed out no further than the take: past it is only blank
            // paper.
            let take = was.max(self.latest_s());
            // Small zooms add up, while the strip shows the round length
            // nearest where they are heading.
            let from = self
                .strip_zoom_s
                .filter(|&w| round_span(w) == was)
                .unwrap_or(was);
            let wish = (from * input.span_factor).clamp(StripView::MIN_SPAN_S, take);
            self.strip_zoom_s = Some(wish);
            self.strip.set_span(round_span(wish).min(take));
            // Looking back, the time under the pointer stays under it;
            // live, the newest beat stays at the edge, so a zoom never
            // moves the moment the cards and the Profile show.
            if let (Some(t), Some(_)) = (input.zoom_at, self.strip_end) {
                let latest = self.latest_s();
                let to = t + (end - t) * self.strip.span_s / was;
                self.strip_end = if to >= latest {
                    None
                } else {
                    Some(to.max(self.strip.span_s.min(latest)))
                };
            }
        }
        if input.width_factor != 1.0 {
            self.strip
                .set_half_width(self.strip.half_width_ms * input.width_factor);
        }
        if input.time_shift_s != 0.0 {
            let latest = self.latest_s();
            let to = end + input.time_shift_s;
            self.strip_end = if to >= latest {
                None
            } else {
                Some(to.max(self.strip.span_s.min(latest)))
            };
        }
        if input.lead_shift_ms != 0.0 {
            if let Some(a) = self.anchor.as_mut() {
                a.time += input.lead_shift_ms / 1000.0;
                // Moving the trace by hand ends auto-centring.
                self.follow = false;
            }
        }
        if input.reset {
            self.strip_end = None;
            self.centre();
        }
    }
}

/// The round strip length nearest `s`: whole seconds below 10 s, then
/// 5 s steps to 2 min, whole minutes to 2 h, then quarter hours, so the
/// Length field never reads such as 3:01 after a zoom.
fn round_span(s: f64) -> f64 {
    const STEPS: [(f64, f64); 4] = [(10.0, 1.0), (120.0, 5.0), (7200.0, 60.0), (f64::MAX, 900.0)];
    let k = STEPS
        .iter()
        .find(|(below, _)| s < *below)
        .map_or(1.0, |&(_, k)| k);
    (s / k).round() * k
}

/// The position track along the strip: (from s, to s, label, code).
type StripTrack = Vec<(f64, f64, &'static str, &'static str)>;

/// The line over the strip: what the faint dots are while no regular beat
/// is found, or where the view is when looking back.
fn strip_note(guessing: bool, looking_back: bool, end: f64) -> Option<String> {
    if guessing {
        Some(
            "No regular beat found yet. The faint dots are sounds that might be beats.".to_string(),
        )
    } else {
        looking_back.then(|| format!("at {}, double-click for the newest beats", length_text(end)))
    }
}

/// The longest stretch of the window from `from` to `end` that `kept`
/// (sorted by time) leaves without a reading, counting the window's ends
/// only within the run they were taken from, `run` (its first and last
/// times): before a take's first reading there is none to miss.
fn widest_hole(kept: &[TrendPoint], from: f64, end: f64, run: (f64, f64)) -> f64 {
    let (Some(first), Some(last)) = (kept.first(), kept.last()) else {
        return f64::INFINITY;
    };
    let lead = first.t - from.max(run.0);
    let tail = end.min(run.1) - last.t;
    kept.windows(2)
        .map(|w| w[1].t - w[0].t)
        .fold(lead.max(tail), f64::max)
}

/// The readings drawn over the strip, in the toolbar's order: Rate,
/// Amplitude, Beat Error. `on` keeps the settings' order (amplitude, rate,
/// beat error).
fn strip_overlays_drawn(
    on: [bool; 3],
    trend: &[TrendPoint],
    from: f64,
    end: f64,
    pal: &theme::Palette,
) -> Vec<strip::Overlay> {
    let series = |f: fn(&TrendPoint) -> Option<f64>| -> Vec<[f64; 2]> {
        trend
            .iter()
            .filter(|p| p.t >= from && p.t <= end)
            .filter_map(|p| f(p).map(|v| [p.t, v]))
            .collect()
    };
    let mut overlays = Vec::new();
    // Rate, Amplitude, Beat Error, as the toolbar orders them.
    if on[1] {
        overlays.push(strip::Overlay {
            name: "Rate",
            short: "Rate",
            unit: " s/d",
            color: pal.trace_rate,
            min_span: 4.0,
            points: series(|p| p.rate_s_per_day),
        });
    }
    if on[0] {
        overlays.push(strip::Overlay {
            name: "Amplitude",
            short: steady_views::AMPLITUDE_SHORT,
            unit: "°",
            color: pal.trace_amplitude,
            min_span: 10.0,
            points: series(|p| p.amplitude_deg),
        });
    }
    if on[2] {
        use core_session::BeatErrorSource::{Drop, Unlock};
        for (name, short, pick, color) in [
            (
                "Beat Error from the Unlock",
                steady_views::BEAT_ERROR_UNLOCK_SHORT,
                (|p: &TrendPoint| p.beat_error_from(Unlock)) as fn(&TrendPoint) -> Option<f64>,
                pal.trace_beat_error,
            ),
            (
                "Beat Error from the Drop",
                steady_views::BEAT_ERROR_DROP_SHORT,
                (|p: &TrendPoint| p.beat_error_from(Drop)) as fn(&TrendPoint) -> Option<f64>,
                pal.trace_beat_error.gamma_multiply(0.6),
            ),
        ] {
            let points: Vec<_> = trend
                .iter()
                .filter(|p| p.t >= from && p.t <= end)
                .map(|p| [p.t, pick(p).unwrap_or(f64::NAN)])
                .collect();
            if points.iter().any(|p| p[1].is_finite()) {
                overlays.push(strip::Overlay {
                    name,
                    short,
                    unit: " ms",
                    color,
                    min_span: 0.2,
                    points,
                });
            }
        }
    }
    overlays
}

/// The possible beats numbered at the strip's beat period from the
/// first, so each lands on the strip, wrapped as the beats are.
fn numbered_candidates(
    candidates: &[timegrapher_core::beats::Beat],
    period: f64,
) -> Vec<timegrapher_core::beats::Beat> {
    candidates
        .first()
        .map(|first| {
            candidates
                .iter()
                .map(|b| timegrapher_core::beats::Beat {
                    index: first.index + ((b.time - first.time) / period).round() as i64,
                    time: b.time,
                    quality: 1.0,
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Readings every half second from `a` to `b`.
    fn every_half(a: f64, b: f64) -> Vec<TrendPoint> {
        let n = ((b - a) / 0.5).round() as usize;
        (0..=n)
            .map(|i| TrendPoint {
                t: a + 0.5 * i as f64,
                rate_s_per_day: Some(1.0),
                amplitude_deg: None,
                amplitude_even_deg: None,
                amplitude_odd_deg: None,
                beat_error_unlock_ms: None,
                beat_error_ms: None,
                rate_error_s_per_day: None,
                amplitude_error_deg: None,
                beat_error_unlock_error_ms: None,
                heard: true,
            })
            .collect()
    }

    /// A stretch with none at the window's start or end, inside the take,
    /// is a hole; the window reaching past the take's own first or last
    /// reading is not.
    #[test]
    fn a_hole_at_either_end_counts_only_inside_the_take() {
        let kept = every_half(30.0, 50.0);
        assert_eq!(widest_hole(&kept, 20.0, 50.0, (0.0, 60.0)), 10.0);
        assert_eq!(widest_hole(&kept, 30.0, 60.0, (0.0, 60.0)), 10.0);
        assert_eq!(widest_hole(&kept, 20.0, 60.0, (30.0, 50.0)), 0.5);
        assert_eq!(widest_hole(&[], 20.0, 60.0, (30.0, 50.0)), f64::INFINITY);
    }

    #[test]
    fn strip_beat_error_overlays_keep_sources_apart_and_preserve_holes() {
        let trend: Vec<_> = (0..4)
            .map(|i| TrendPoint {
                t: i as f64,
                rate_s_per_day: None,
                amplitude_deg: None,
                amplitude_even_deg: None,
                amplitude_odd_deg: None,
                beat_error_unlock_ms: (i != 1).then_some(0.2),
                beat_error_ms: Some(0.8),
                rate_error_s_per_day: None,
                amplitude_error_deg: None,
                beat_error_unlock_error_ms: None,
                heard: true,
            })
            .collect();
        let overlays = strip_overlays_drawn([false, false, true], &trend, 0.0, 3.0, &theme::DARK);
        assert_eq!(overlays.len(), 2);
        assert_eq!(overlays[0].name, "Beat Error from the Unlock");
        assert_eq!(overlays[1].name, "Beat Error from the Drop");
        assert_eq!(overlays[0].points.len(), 4);
        assert!(overlays[0].points[1][1].is_nan());
        assert_eq!(overlays[1].points[1], [1.0, 0.8]);
        assert!(overlays[1].points[0][1].is_nan());
        assert!(overlays[1].points[2][1].is_nan());
    }
}
