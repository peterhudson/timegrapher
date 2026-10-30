//! The Paper Strip pane: its trend, settings, fit and input.

use super::*;

impl TimegrapherApp {
    /// Show the whole session so far along the strip, ending at its latest
    /// beat, and keep it so as beats arrive: Fit on.
    pub(super) fn fit_strip(&mut self) {
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

    /// The strip's view a live session goes back to: the one before a take
    /// was fitted, unless the strip was changed by hand since.
    pub(super) fn live_view(&self) -> StripView {
        match self.live_strip {
            Some((live, fitted)) if fitted == self.strip => live,
            _ => self.strip,
        }
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

    /// The time at the strip's newest edge.
    pub(super) fn strip_end_s(&self) -> f64 {
        let latest = self.latest_s();
        self.strip_end.unwrap_or(latest).min(latest)
    }

    /// The stretches the strip draws faintly, each with what the ticker
    /// says over it: the settling after each move, and during and after a
    /// Positions Test the time it didn't use, in place of the moves'
    /// settling it covers.
    pub(super) fn strip_settles(&self) -> Vec<(f64, f64, &'static str)> {
        let test = &self.guided_unused;
        let mut out: Vec<(f64, f64, &'static str)> = self
            .session
            .settles()
            .into_iter()
            .filter(|s| !test.iter().any(|t| s.0 < t.1 && t.0 < s.1))
            .map(|(a, b)| (a, b, guided::SETTLING))
            .chain(test.iter().copied())
            .collect();
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    }

    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    pub(super) fn strip_settings(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
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
        theme::bar_gap(ui);
        theme::tool_field(
            ui,
            "Width",
            "Milliseconds either side of the centre line: type it, or pick one with ⏷. \
             Shift or Ctrl and the mouse wheel on the strip change it too.",
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
                    104.0,
                );
                if changed {
                    self.strip.set_half_width(w);
                }
            },
        );
        theme::tool_field(
            ui,
            "Length",
            "Time shown along the strip, such as 30 s, 5 min or 2 h: type it, or pick one \
             with ⏷. The mouse wheel on the strip changes it too.",
            |ui| {
                let mut s = self.strip.span_s;
                let (_, changed) = fields::combo(
                    ui,
                    "span",
                    &mut s,
                    StripView::MIN_SPAN_S..=StripView::MAX_SPAN_S,
                    &DURATION,
                    &SPANS,
                    span_label,
                    "",
                    92.0,
                );
                if changed {
                    self.strip_fit = false;
                    self.strip.set_span(s);
                }
            },
        );
        let mut fit = self.strip_fit;
        if theme::tool_switch(
            ui,
            Icon::Fit,
            pal.accent,
            "Fit",
            &mut fit,
            "Show the whole session along the strip, up to its latest beat (its last 80 hours \
             when it is longer), and keep it in view as beats arrive. Zooming or dragging the \
             strip's time turns it off.",
        ) {
            if fit {
                self.fit_strip();
            } else {
                self.strip_fit = false;
            }
        }
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
        theme::bar_gap(ui);
        let mut h = self.strip.horizontal;
        theme::tool_field(
            ui,
            "Direction",
            "Which way time runs along the strip",
            |ui| {
                theme::segmented(
                    ui,
                    &mut h,
                    &[
                        (
                            false,
                            "Down",
                            "Down: newest beats at the top, the strip beside the charts",
                        ),
                        (
                            true,
                            "Across",
                            "Across: newest beats at the right, the strip above the charts",
                        ),
                    ],
                )
            },
        );
        if h != self.strip.horizontal {
            self.strip.horizontal = h;
            self.relayout(h);
        }
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
        if spacing <= 2.0 * step {
            return (kept, spacing);
        }
        let key: StripKey = (
            (from / step).floor() as i64,
            (end / step).ceil() as i64,
            trend_key(seconds),
            step.to_bits(),
            live.duration_s().to_bits(),
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

    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
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
        let note = if guessing {
            Some(
                "No regular beat found yet. The faint dots are sounds that might be beats."
                    .to_string(),
            )
        } else {
            looking_back
                .then(|| format!("at {}, double-click for the newest beats", length_text(end)))
        };
        let from = end - self.strip.span_s - 5.0;
        let (trend, _) = self.strip_trend(from, end, self.average_s);
        let Some(live) = &self.live else { return };
        let period = live.bph().map_or(0.125, |b| 3600.0 / b as f64);
        // The guides follow the rate line's slope, so it is worked out for
        // either.
        let rate_line = (self.rate_line || self.rate_guides)
            .then(|| self.reading_over(self.average_s))
            .flatten()
            .and_then(|r| {
                Some(strip::RateLine {
                    from_s: r.time_s - r.span_s,
                    to_s: r.time_s,
                    rate_s_per_day: r.rate_s_per_day?,
                })
            });
        let pal = theme::pal(ui);
        let series = |f: fn(&TrendPoint) -> Option<f64>| -> Vec<[f64; 2]> {
            trend
                .iter()
                .filter(|p| p.t >= from && p.t <= end)
                .filter_map(|p| f(p).map(|v| [p.t, v]))
                .collect()
        };
        let mut overlays = Vec::new();
        // Rate, Amplitude, Beat Error, as the toolbar orders them.
        if self.overlays[1] {
            overlays.push(strip::Overlay {
                name: "Rate",
                unit: " s/d",
                decimals: 1,
                color: pal.trace_rate,
                min_span: 4.0,
                points: series(|p| p.rate),
            });
        }
        if self.overlays[0] {
            overlays.push(strip::Overlay {
                name: "Amplitude",
                unit: "°",
                decimals: 0,
                color: pal.trace_amplitude,
                min_span: 10.0,
                points: series(|p| p.amplitude),
            });
        }
        if self.overlays[2] {
            overlays.push(strip::Overlay {
                name: "Beat Error",
                unit: " ms",
                decimals: 2,
                color: pal.trace_beat_error,
                min_span: 0.2,
                points: series(TrendPoint::beat_error),
            });
        }
        let bands = self.position_bands();
        let track: Vec<(f64, f64, &str, &str)> = self
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
            gutter: GUTTER,
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
        };
        let Some(live) = &self.live else { return };
        // The possible beats numbered at the strip's beat period from the
        // first, so each lands on the strip, wrapped as the beats are.
        let candidates: Vec<timegrapher_core::beats::Beat> = candidates
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
            .unwrap_or_default();
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
            self.strip.set_span(was * input.span_factor);
            // The time under the pointer stays under it.
            if let Some(t) = input.zoom_at {
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
