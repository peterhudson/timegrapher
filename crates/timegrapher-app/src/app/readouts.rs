//! The readouts and the status bar: the figures shown, the clocks behind them, the status line and notices, and what shows when nothing is open.

use super::*;
use timegrapher_core::library::pm_text;

impl TimegrapherApp {
    /// The reading cards across the top: Current Position, Listen, then
    /// Rate, Amplitude, Beat Error and Beat Rate. Every card is the same
    /// three rows (title, figure, controls) and one height at every width;
    /// where the full controls don't fit, the compact form of every card.
    /// Returns what the cards asked for, and where their controls go.
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    pub(super) fn readouts(&self, ui: &mut egui::Ui) -> Readouts {
        let pal = theme::pal(ui);
        let r = self.reading();
        let gap = theme::GAP;
        let total = ui.available_width();
        // The first layout that fits, from every card full down: Current
        // Position, Listen and Calibre keep their full width before the
        // reading cards keep theirs (a reading card needs little room for
        // its figure alone), then Listen, Position and Calibre go compact in
        // turn, the reading cards keeping room for their captions whole.
        let layouts = [
            (POSITION_W, LISTEN_W, CALIBRE_W, READING_FULL_W),
            (POSITION_W, LISTEN_W, CALIBRE_W, READING_ROOMY_W),
            (POSITION_W, LISTEN_COMPACT_W, CALIBRE_W, READING_ROOMY_W),
            (
                POSITION_COMPACT_W,
                LISTEN_COMPACT_W,
                CALIBRE_W,
                READING_ROOMY_W,
            ),
            (
                POSITION_COMPACT_W,
                LISTEN_COMPACT_W,
                CALIBRE_ROOMY_W,
                CAPTION_FIT_W,
            ),
            (
                POSITION_COMPACT_W,
                LISTEN_NARROWEST_W,
                CALIBRE_ROOMY_W,
                CAPTION_FIT_W,
            ),
        ];
        let rest = |(p, l, c, _): (f32, f32, f32, f32)| (total - p - l - c - 5.0 * gap) / 3.0;
        let (position_w, listen_w, calibre_w, _) = layouts
            .into_iter()
            .find(|&layout| rest(layout) >= layout.3)
            .unwrap_or((
                POSITION_COMPACT_W,
                LISTEN_NARROWEST_W,
                CALIBRE_COMPACT_W,
                CAPTION_FIT_W,
            ));
        let reading_w = rest((position_w, listen_w, calibre_w, 0.0)).max(60.0);
        let position_compact = position_w < POSITION_W;
        let listen_compact = listen_w < LISTEN_W;
        let compact = reading_w < READING_FULL_W;
        let widths = [
            position_w, listen_w, reading_w, reading_w, reading_w, calibre_w,
        ];
        let (block, _) = ui.allocate_exact_size(
            Vec2::new(total, theme::READINGS_CARD_H),
            egui::Sense::hover(),
        );
        // Each card's three rows, hung off the same three lines. `cards`
        // keeps the order above; left to right they read Position,
        // Calibre, Listen, Rate, Amplitude, Beat Error.
        let mut x = block.left();
        let mut cards = vec![CardRows::of(block); widths.len()];
        for i in CARD_ORDER {
            let w = widths[i];
            let rect =
                egui::Rect::from_min_size(egui::pos2(x, block.top()), Vec2::new(w, block.height()));
            x += w + gap;
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(theme::CARD_RADIUS), pal.card);
            cards[i] = CardRows::of(rect);
        }
        // A row of a card, laid out left to right and kept inside it.
        let row = |ui: &mut egui::Ui, rect: egui::Rect, salt: &str| {
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("card row", salt))
                    .max_rect(rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            child.set_clip_rect(rect.expand(2.0).intersect(ui.clip_rect()));
            child.spacing_mut().item_spacing.x = 8.0;
            child.spacing_mut().interact_size.y = theme::CONTROL_H;
            child
        };
        // The title row: the caption, its limits in a chip, and the "?" in
        // the corner. The caption is cut short before the chip is.
        let title = |ui: &mut egui::Ui,
                     rect: egui::Rect,
                     caption: &str,
                     help: &[&str],
                     limits: &[(String, String)],
                     note: Option<&(String, String)>| {
            let mut child = row(ui, rect, caption);
            child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Tight, so a reading card's caption stays whole at 800.
                ui.spacing_mut().item_spacing.x = 4.0;
                theme::help(ui, caption, help);
                let caption_w = ui.fonts_mut(|f| {
                    f.layout_no_wrap(caption.into(), theme::semibold(13.0), pal.text)
                        .size()
                        .x
                });
                let small_w = |ui: &mut egui::Ui, t: &str| {
                    ui.fonts_mut(|f| {
                        f.layout_no_wrap(t.into(), egui::FontId::proportional(12.0), pal.text)
                            .size()
                            .x
                    })
                };
                // A short fact in plain small text before the "?", where the
                // caption stays whole beside it.
                if let Some((text, hover)) = note {
                    if small_w(ui, text) + 4.0 + caption_w <= ui.available_width() {
                        ui.label(RichText::new(text).size(12.0).color(pal.text_secondary))
                            .on_hover_text(hover);
                    }
                }
                // Limits with no room for their words beside the whole
                // caption are in the caption's hover: a bare count says
                // nothing.
                let mut caption_hover = None;
                if let Some(first) = limits.first() {
                    let more = if limits.len() > 1 {
                        format!(" +{}", limits.len() - 1)
                    } else {
                        String::new()
                    };
                    let sentences: Vec<String> = limits.iter().map(|l| l.1.clone()).collect();
                    let words = format!("{}{more}", chip_words(&first.0));
                    if small_w(ui, &words) + 14.0 + 4.0 + caption_w <= ui.available_width() {
                        theme::chip(ui, &words, caption, &sentences, ui.available_width());
                    } else {
                        caption_hover = Some(sentences.join("\n\n"));
                    }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let r = ui.add(
                        egui::Label::new(
                            RichText::new(caption)
                                .font(theme::semibold(13.0))
                                .color(pal.text_secondary),
                        )
                        .truncate(),
                    );
                    if let Some(h) = caption_hover {
                        r.on_hover_text(h);
                    }
                });
            });
        };
        // A reading whose beats scatter like noise is no watch: no figures
        // then, only a word saying so where the unit goes.
        let heard = r.as_ref().is_none_or(|r| r.watch_heard || r.bph.is_none());
        let unheard_line = "No Regular Beat".to_string();
        let unheard_hover = format!(
            "{}.\n\n{}",
            r.as_ref().map(no_regular_beat).unwrap_or_default(),
            *help::NO_REGULAR_BEAT
        );
        let unheard = (!heard).then(|| (unheard_line.clone(), unheard_hover.clone()));
        let r = r.as_ref().filter(|_| heard);
        // The take's breaks not all its own list's, as session says too.
        let mut caveats = r.map_or_else(Vec::new, |r| r.caveats.clone());
        if let (Some(why), Some(_)) = (self.take_unlisted, r) {
            timegrapher_core::measure::add(&mut caveats, Caveat::BreaksUnlisted { why });
        }
        let caveats = &caveats[..];
        // Each reading's ±, as an analysis of the recording gives them
        // (core `measure`).
        let pm = r.and_then(|r| r.rate_error_s_per_day);
        let be_pm = r.and_then(|r| r.beat_error_unlock_error_ms);
        let lift_now = self.lift_deg;
        let beats = r.map_or(0, |r| r.beats_used);
        // Over the whole take, once it is stopped: `session`'s reading of
        // it beside the last window's figure.
        let whole = self.whole_take();
        let whole_breaks = whole.as_ref().map_or_else(String::new, whole_breaks_text);
        // The whole take's reading, for the figure's hover.
        let whole_text = |figure: String, spread: Option<String>, span: f64| {
            let spread = spread.map_or_else(String::new, |s| format!(", {s}"));
            format!(
                "\n\nWhole take {figure}{spread} over {}. The big figure is the last stretch \
                 of Average Over; this is the take in this position, settling left out, read \
                 as the command line reads it.{whole_breaks}",
                length_text(span)
            )
        };
        let mut slots = vec![(Slot::Position, cards[0].inner)];
        let calibre_limits;
        // Listen: its caption here, its buttons and Average Over in the
        // rows under it.
        // A compact Listen card has no room beside its buttons for the
        // take's length: it goes in the title row, before the "?".
        let length = listen_compact.then(|| {
            let length = length_text(self.live.as_ref().map_or(0.0, |l| l.duration_s()));
            (
                length.clone(),
                format!(
                    "{length} · {}: the take's length, the sound heard so far while \
                     listening, or the whole take opened",
                    self.state_words()
                ),
            )
        });
        title(
            ui,
            cards[1].title,
            "Listen",
            help::LISTEN,
            &[],
            length.as_ref(),
        );
        slots.push((Slot::Listen, cards[1].figure.union(cards[1].controls)));
        // Rate.
        {
            let rate = r.and_then(|r| r.rate_s_per_day);
            let rate_facts = reading_limits("Rate", caveats, lift_now, beats);
            title(ui, cards[2].title, "Rate", &help::RATE, &rate_facts, None);
            let unit = match pm.and_then(|e| pm_text(e, 1)) {
                Some(t) => format!("± {t} s/d"),
                None => "s/d".to_string(),
            };
            // The beats behind the figure, in its hover.
            let mut behind = r.map_or_else(String::new, |r| {
                let short = r.span_s + 1.0 < self.average_s;
                let mut t = if short {
                    format!(
                        "{} beats in {} of {}: there are fewer seconds of beats than \
                         Average Over asks for, early in a session or after a break in \
                         the sound, so the readings use what there is.",
                        r.beats_used,
                        fields::duration(r.span_s.round()),
                        fields::duration(self.average_s)
                    )
                } else {
                    format!(
                        "{} beats in {}: the beats the readings are fitted to, over \
                         Average Over.",
                        r.beats_used,
                        fields::duration(self.average_s)
                    )
                };
                if let Some(j) = r.jitter_us {
                    t += &format!(
                        "\n\nJitter {j:.0} µs: how far single beats land from the \
                         rate's straight line, the spread of the dots across the strip."
                    );
                }
                t
            });
            if let Some((w, v)) = whole.as_ref().and_then(|w| Some((w, w.m.rate_s_per_day?))) {
                let se = w.m.confidence.and_then(|c| c.rate.se);
                let pm = se
                    .and_then(|e| pm_text(e, 1))
                    .map_or_else(String::new, |t| format!(" ± {t}"));
                let spread =
                    w.m.rate_p05.zip(w.m.rate_p95).map(|(a, b)| {
                        format!("10 s rates {a:+.1} to {b:+.1} s/d").replace('-', "−")
                    });
                behind += &whole_text(format!("{v:+.1}{pm} s/d").replace('-', "−"), spread, w.span);
            }
            // In the unit's place: what the reading waits on, or why it is
            // the position before.
            let status: Option<(String, String)> = if r.is_none() {
                if let Some(left) = self.settling_left() {
                    Some((
                        format!("Settling, {:.0} s", left.ceil()),
                        format!(
                            "Settling in the new position for {:.0} s more.\n\n{}",
                            left.ceil(),
                            settle_hint(self.session.settle_len())
                        ),
                    ))
                } else if heard {
                    Some(("Waiting for Beats".into(), String::new()))
                } else {
                    unheard.clone()
                }
            } else {
                self.stopped_in_settle().map(|at| {
                    (
                        "Before the Move".to_string(),
                        format!(
                            "Stopped while the watch settled in its new position, so \
                             these are the last readings of the position before it, \
                             before the move at {}.",
                            length_text(at)
                        ),
                    )
                })
            };
            // While a clock is kept, the rate on it in the figure's hover:
            // first when the figure itself is corrected, as it says what
            // the figure is.
            if let Some(t) = rate.and_then(|rate| self.true_clock_words(rate)) {
                behind = if self.take_rate_ppm().is_some() {
                    format!("{t}\n\n{}", behind.trim_start())
                } else {
                    behind + &format!("\n\n{t}")
                };
            }
            figure_row(
                ui,
                cards[2].figure,
                rate.map(|v| format!("{v:+.1}")).as_deref(),
                &unit,
                status.as_ref(),
                behind.trim_start(),
                compact,
            );
        }
        // Amplitude. Its lift angle is on the Calibre card, which says
        // when it is the default, so that isn't said again in a chip.
        {
            let amp = r.and_then(|r| r.amplitude_deg);
            let facts = reading_limits("Amplitude", caveats, lift_now, beats);
            title(
                ui,
                cards[3].title,
                "Amplitude",
                help::AMPLITUDE,
                &facts,
                None,
            );
            // Whole degrees: a live reading's ± is about a degree, so a
            // decimal would claim more than the beats can say.
            let unit = r
                .and_then(|r| r.amplitude_error_deg)
                .and_then(|e| pm_text(e, 0))
                .map_or_else(String::new, |t| format!("± {t}°"));
            let mut behind = String::new();
            if let Some(&LiveReading {
                amplitude_even_deg: Some(a),
                amplitude_odd_deg: Some(b),
                ..
            }) = r
            {
                behind = format!("Tick {a:.0}°, Tock {b:.0}°: each side's own amplitude.");
            }
            if let Some((w, v)) = whole.as_ref().and_then(|w| Some((w, w.m.amplitude_deg?))) {
                let se = w.m.confidence.and_then(|c| c.amplitude.se);
                let pm = se
                    .and_then(|e| pm_text(e, 0))
                    .map_or_else(String::new, |t| format!(" ± {t}"));
                let spread =
                    w.m.amplitude_p05
                        .zip(w.m.amplitude_p95)
                        .map(|(a, b)| format!("2 s windows {a:.0}° to {b:.0}°"));
                behind += &whole_text(format!("{v:.0}{pm}°"), spread, w.span);
            }
            behind += &format!(
                " Worked out with a lift angle of {}°, on the Calibre card.",
                fields::plain(lift_now)
            );
            figure_row(
                ui,
                cards[3].figure,
                amp.map(|v| format!("{v:.0}°")).as_deref(),
                &unit,
                unheard.as_ref(),
                behind.trim_start(),
                compact,
            );
        }
        // Beat Error, the size only: which beat is the tick can flip
        // between runs.
        {
            let unlock = r.and_then(|r| r.beat_error_unlock_ms);
            let drop = r.and_then(|r| r.beat_error_ms);
            title(
                ui,
                cards[4].title,
                "Beat Error",
                help::BEAT_ERROR,
                &reading_limits("Beat Error", caveats, lift_now, beats),
                None,
            );
            let unit = match be_pm
                .filter(|_| unlock.is_some())
                .and_then(|e| pm_text(e, 2))
            {
                Some(t) => format!("± {t} ms"),
                None => "ms".to_string(),
            };
            // Which edge it is timed from, in the figure's hover.
            let mut from = match (unlock, drop) {
                (Some(_), Some(d)) => format!(
                    "From the unlock, the usual reading, as tg shows it: quote this one. \
                     From the drop, a second opinion: {:.2} ms.",
                    d.abs()
                ),
                (None, Some(_)) => "From the drop: the unlock couldn't be timed.".into(),
                _ => "From the unlock, the usual reading, as tg shows it.".into(),
            };
            if let Some((w, v)) = whole.as_ref().and_then(|w| Some((w, w.m.beat_error()?))) {
                let se = w.m.confidence.and_then(|c| c.beat_error.se);
                let pm = se
                    .and_then(|e| pm_text(e, 2))
                    .map_or_else(String::new, |t| format!(" ± {t}"));
                from += &whole_text(format!("{v:.2}{pm} ms"), None, w.span);
                if w.m.caveats.contains(&Caveat::BeatErrorAsSize) {
                    from += &format!(
                        " The {WHOLE_AS_SIZES}, so the sizes either side are combined \
                         rather than the signed values, as the command line does."
                    );
                }
            }
            figure_row(
                ui,
                cards[4].figure,
                core_session::beat_error_size(unlock, drop)
                    .map(|v| format!("{v:.2}"))
                    .as_deref(),
                &unit,
                unheard.as_ref(),
                &from,
                compact,
            );
        }
        // Calibre: what the readings are worked out for. The calibre on
        // the figure row, the beat rate and the lift angle under it; a
        // calibre picked that beats at another rate than the one heard says
        // so in a chip.
        {
            let other: Vec<(String, String)> = self
                .live
                .as_ref()
                .and_then(|l| l.bph())
                .zip(self.cal.bph(&self.calibre))
                .filter(|&(b, cb)| b != cb && self.take_shows_rate_and_lift())
                .map(|(b, cb)| {
                    (
                        format!("{} is {cb} bph", self.calibre),
                        format!(
                            "The {} beats at {cb} bph; this watch beats at {b}.",
                            self.calibre
                        ),
                    )
                })
                .into_iter()
                .collect();
            // Its title is the Watch line, drawn with the controls.
            calibre_limits = other;
            slots.push((Slot::WatchLine, cards[5].title));
            slots.push((Slot::Calibre, cards[5].figure));
            slots.push((Slot::RateAndLift, cards[5].controls));
        }
        Readouts {
            slots,
            position_compact,
            listen_compact,
            calibre_limits,
        }
    }

    /// The clock the readings of the take on show are put on, for a log
    /// `duration_s` long ([`reading_clock`]).
    pub(super) fn reading_clock(
        &self,
        duration_s: f64,
    ) -> Option<timegrapher_core::clock::ClockFit> {
        reading_clock(
            self.take_clock.as_ref(),
            self.take_breaks.is_some(),
            self.clock.as_ref().map(|c| c.ppm),
            duration_s,
        )
    }

    /// The breaks the readings of `log` are cut at: the take's own, with
    /// every other cut its beats show (a jump, a move, a settling's end, a
    /// mask's ends) as a break of no length, as core's `take::breaks`
    /// adds the jumps.
    pub(super) fn reading_breaks(&self, log: &BeatLog) -> Vec<timegrapher_core::breaks::Break> {
        steady::reading_breaks(self.take_breaks.clone().unwrap_or_default(), log)
    }

    /// The take's own clock error the readings are corrected by, ppm slow:
    /// none where its clock log measures the clock too loosely to correct
    /// by (core's `ClockFit::corrects`).
    pub(super) fn take_rate_ppm(&self) -> Option<f64> {
        self.take_rate_clock().map(|f| f.ppm)
    }

    /// The take's own clock the readings are corrected on, where it
    /// corrects: its mapping, which may drift over the take.
    pub(super) fn take_rate_clock(&self) -> Option<&timegrapher_core::clock::ClockFit> {
        Self::rate_clock(&self.take_clock)
    }

    /// A clock where it corrects (core's `ClockFit::corrects`), the one
    /// place that rule is applied; it takes the field, not `self`, so a
    /// setter can call it while it holds the live engine.
    pub(super) fn rate_clock(
        clock: &Option<timegrapher_core::clock::ClockFit>,
    ) -> Option<&timegrapher_core::clock::ClockFit> {
        clock.as_ref().filter(|f| f.corrects())
    }

    /// The rate `rate` on the clock kept for the input, for the Rate
    /// figure's hover, while one is kept. A take with its own clock log
    /// is read on it, the big figure included, so the hover says so.
    pub(super) fn true_clock_words(&self, rate: f64) -> Option<String> {
        if let Some(f) = &self.take_clock {
            // What a log measured but doesn't apply, and why, in core's words.
            let note = f.withheld.map(|w| {
                let note = w.note();
                let mut c = note.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            });
            // A log that measures the clock too loosely corrects nothing.
            if !f.corrects() {
                return Some(format!(
                    "{}. The rate stays on the sound card's clock, as tg reads the recording.",
                    note.unwrap_or_default()
                ));
            }
            let ppm = format!("{:.1}", f.ppm.abs());
            let way = if f.ppm >= 0.0 { "slow" } else { "fast" };
            let from = self
                .take_clock_source
                .as_deref()
                .map_or(String::new(), |s| format!(", {s}"));
            // The recording's USB lines set the rate over a log kept for its
            // breaks: what the log measured follows.
            let note = note.map_or(String::new(), |n| format!(" {n}."));
            return Some(format!(
                "The rate corrected for the sound card's own clock, {ppm} ppm {way}{from}, \
                 as the Positions table reads it.{note} tg reads the recording on the card's \
                 clock."
            ));
        }
        let c = self.clock.as_ref()?;
        let v = format!("{:+.1}", c.correct_rate(rate)).replace('-', "−");
        Some(format!(
            "True Clock {v} seconds per day, with the clock kept {}: the rate corrected \
             for this input's own clock. The big figure stays on the card's clock, the one \
             tg and an analysis of the recording read, so the two compare directly.",
            day_of(&c.measured_utc)
        ))
    }

    /// The input listening as Settings' Quartz Correction shows it being
    /// timed: how much sound it has heard, and once timed, the error,
    /// offered to keep. None while not listening.
    pub(super) fn clock_timing(&self) -> Option<settings_window::Timing> {
        let t = self.clock_track.as_ref()?;
        if !(self.running() && self.session.mic()) {
            return None;
        }
        let words = self.clock_words(self.clock_measured(), self.clock.as_ref());
        Some(settings_window::Timing {
            shown: t.shown.clone(),
            heard_s: t.tracker.audio_s(),
            needed_s: CLOCK_MEASURE_S,
            timed: self
                .clock_measured()
                .map(|_| (format!("Runs {}", words.value), words.hint)),
            offered: self.clock_offer().is_some(),
        })
    }

    /// The input's clock in words, for Settings' Quartz Correction: as
    /// timed and offered to keep, as kept, or not timed yet.
    fn clock_words(
        &self,
        measured: Option<&timegrapher_core::clock::ClockFit>,
        kept: Option<&clockstore::DeviceClock>,
    ) -> ClockWords {
        let words = |ppm: f64| {
            format!(
                "{:.1} ppm {}",
                ppm.abs(),
                if ppm >= 0.0 { "slow" } else { "fast" }
            )
        };
        if let Some(f) = measured {
            let slow = if f.ppm >= 0.0 { "slow" } else { "fast" };
            let mut hint = format!(
                "Timed against the computer's clock over {} of listening, this input's \
                 clock runs {:.1} ± {:.1} ppm {slow}: {:+.1} seconds per day on every rate. \
                 Keep it to show the corrected rate as True Clock from now on; the command \
                 line and the MCP server use it too. It is only right if the computer's \
                 clock is kept by internet time.",
                length_text(f.span_s),
                f.ppm.abs(),
                f.ppm_sd,
                f.rate_error_s_per_day
            );
            if let Some(c) = kept {
                let off = f.ppm - c.ppm;
                hint += &format!(
                    "\n\nThe clock kept for \"{}\" runs {}, kept {}. This timing is {:.1} ppm \
                     from it, {:.1} seconds per day on True Clock. An input reached by a \
                     shared name, such as the system default, may be a different sound card \
                     from the one timed then. Keep replaces the kept clock with this one.",
                    c.device,
                    words(c.ppm),
                    day_of(&c.measured_utc),
                    off.abs(),
                    timegrapher_core::clock::rate_error_s_per_day(off.abs())
                );
            }
            return ClockWords {
                value: words(f.ppm),
                hint,
            };
        }
        if let Some(c) = kept {
            return ClockWords {
                value: words(c.ppm),
                hint: format!(
                    "This input's own clock runs {} ({:+.1} s/d on every rate), measured {} \
                     from {}. The Rate figure's hover gives the rate corrected for it, as True \
                     Clock. The clock kept is under Settings > Microphone > Quartz Correction.",
                    words(c.ppm),
                    c.rate_error_s_per_day(),
                    c.measured_utc,
                    match c.source.as_str() {
                        "log" => "a recording's clock log",
                        "measure" => "listening against the system clock",
                        _ => "a value typed in",
                    }
                ),
            };
        }
        ClockWords {
            value: "Not Timed".into(),
            hint: format!(
                "This input's clock hasn't been timed, so the rate includes the sound card's \
                 own clock error. After {} of listening it is timed against the computer's \
                 clock and offered here to keep.",
                fields::duration(CLOCK_MEASURE_S)
            ),
        }
    }

    /// The microphone's clock as timed so far, once it has run
    /// [`CLOCK_MEASURE_S`] of audio and is known to half a ppm.
    pub(super) fn clock_measured(&self) -> Option<&timegrapher_core::clock::ClockFit> {
        self.clock_track
            .as_ref()?
            .fit
            .as_ref()
            .filter(|f| f.ppm_sd.is_finite() && f.ppm_sd < 0.5)
    }

    /// The clock timed while listening, offered to keep: when none is kept
    /// for the input, or the kept one is more than [`CLOCK_DIFFERS_PPM`]
    /// from it.
    pub(super) fn clock_offer(&self) -> Option<&timegrapher_core::clock::ClockFit> {
        self.clock_measured().filter(|f| {
            self.clock
                .as_ref()
                .is_none_or(|c| (f.ppm - c.ppm).abs() > CLOCK_DIFFERS_PPM)
        })
    }

    /// Keep the microphone's clock as timed: corrected rates from now on,
    /// for the command line and the MCP server too.
    pub(super) fn keep_clock(&mut self) {
        let (Some(fit), Some(t)) = (self.clock_measured(), self.clock_track.as_ref()) else {
            return;
        };
        let d = clockstore::DeviceClock::from_fit(
            &t.device,
            fit,
            timegrapher_core::recorder::utc(std::time::SystemTime::now()),
            "measure",
        );
        let saved = clockstore::ClockStore::load().and_then(|mut s| {
            s.set(d.clone());
            s.save()
        });
        match saved {
            Ok(p) => {
                self.info_msg(format!(
                    "Kept this input's clock, {:.1} ppm {}, in {}",
                    d.ppm.abs(),
                    if d.ppm >= 0.0 { "slow" } else { "fast" },
                    p.display()
                ));
                self.put_input_clock(Some(d));
            }
            Err(e) => self.error(format!("The input's clock couldn't be kept: {e}")),
        }
    }

    pub(super) fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            // The notices on the right first, so that what is on the left is
            // cut short with an ellipsis rather than running under them.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.status_notices(ui);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    // Nothing runs under the notices, even past an ellipsis.
                    ui.shrink_clip_rect(ui.max_rect());
                    self.status_left(ui);
                });
            });
        });
    }

    /// Where the take opened has USB lines and a clock log good enough to
    /// correct on its own that measure its sound card apart, core's
    /// caveat for it, short and long, in core's words. The rates stay
    /// corrected by the USB lines.
    pub(super) fn clock_disagreement(&self) -> Option<(String, String)> {
        let caveat = Caveat::from(self.take_clock.as_ref()?.sources_disagree()?);
        Some((
            caveat.short(self.lift_deg, 0)?,
            caveat.long(Some(self.lift_deg))?,
        ))
    }

    /// The status bar's notices, right to left: a word each for what used
    /// to push the window down from the top (an error, the app having
    /// closed unexpectedly, sound kept from an earlier session, a newer
    /// release) and the take's USB lines and clock log disagreeing, each
    /// opening its whole message and buttons over the window. The bar keeps its height whatever it shows.
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn status_notices(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        // A warning or a failure starts with its icon; the words are in the
        // text colour.
        let item = |ui: &mut egui::Ui, text: &str, alert: Option<theme::Alert>, hint: &str| {
            let color = if alert.is_some() {
                pal.text
            } else {
                pal.accent
            };
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let icon = |ui: &mut egui::Ui| {
                    if let Some(kind) = alert {
                        let (icon, _) =
                            ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
                        theme::alert_icon(ui.painter(), icon, kind, pal);
                    }
                };
                // The bar's right side runs right to left: the icon still
                // reads first.
                let rtl = ui.layout().prefer_right_to_left();
                if !rtl {
                    icon(ui);
                }
                let resp = ui
                    .add(
                        egui::Button::new(RichText::new(text).small().color(color))
                            .frame_when_inactive(false),
                    )
                    .on_hover_text(hint);
                if rtl {
                    icon(ui);
                }
                resp
            })
            .inner
        };
        let popover = |resp: &egui::Response, body: &mut dyn FnMut(&mut egui::Ui)| {
            egui::Popup::menu(resp)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    ui.set_width(440.0);
                    body(ui);
                });
        };
        if let Some(r) = self.updates.notice.clone() {
            let resp = item(
                ui,
                &format!("Timegrapher {} Available…", r.version),
                None,
                "A newer release is published: click for its page",
            );
            popover(&resp, &mut |ui| self.update_notice(ui));
        }
        if !self.kept.is_empty() {
            let resp = item(
                ui,
                "Unsaved Sound Found…",
                Some(theme::Alert::Warning),
                "Sound from an earlier session was never saved: click to open, move or delete it",
            );
            popover(&resp, &mut |ui| self.kept_notice(ui));
        }
        if self.crash.is_some() {
            let resp = item(
                ui,
                "Closed Unexpectedly…",
                Some(theme::Alert::Warning),
                "The timegrapher closed unexpectedly last time: click to make a report",
            );
            popover(&resp, &mut |ui| self.crash_notice(ui));
        }
        let error =
            self.session.recording_fault().is_some() || matches!(self.message, Some((_, true)));
        if error && !self.setup_sheet {
            let short = match (self.session.recording_fault(), &self.message) {
                (Some(_), _) => "Recording Stopped…".to_string(),
                (None, Some((m, _))) => {
                    let first = m.lines().next().unwrap_or_default();
                    if first.chars().count() > 48 {
                        format!("{}…", first.chars().take(47).collect::<String>())
                    } else {
                        format!("{first}…")
                    }
                }
                _ => String::new(),
            };
            let resp = item(
                ui,
                &short,
                Some(theme::Alert::Bad),
                "Click for the whole message",
            );
            popover(&resp, &mut |ui| self.error_notice(ui));
        }
        // On the right, so a long take name never hides it.
        if let Some((short, long)) = self.clock_disagreement() {
            let resp = item(
                ui,
                &format!("{short}…"),
                Some(theme::Alert::Warning),
                "The take's USB lines and clock log measure its sound card apart: click for both figures",
            );
            popover(&resp, &mut |ui| {
                ui.label(long.as_str());
            });
        }
    }

    /// The status bar's left side: what is showing, the signal, the kept
    /// sound and the latest message.
    fn status_left(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        ui.spacing_mut().item_spacing.x = 12.0;
        if let Some(job) = &self.analysis_save {
            let done = f64::from_bits(job.progress.load(std::sync::atomic::Ordering::Relaxed));
            // The sound is read twice: once for the beats, once more for
            // the profiles and the shape.
            let frac = (done / (2.0 * job.total_s).max(1e-9)).clamp(0.0, 1.0);
            ui.add(
                egui::ProgressBar::new(frac as f32)
                    .desired_width(240.0)
                    .desired_height(14.0)
                    .fill(pal.accent)
                    .text(RichText::new("Keeping the analysis").small()),
            );
            let cancel = ui
                .small_button("Cancel")
                .on_hover_text("Stop keeping the analysis; nothing is written")
                .clicked();
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
            if cancel {
                self.analysis_save = None;
            }
        }
        if let Some(s) = &self.saving {
            ui.add(egui::Spinner::new().size(12.0).color(pal.text_secondary));
            ui.label(RichText::new("Saving…").small().color(pal.text_secondary))
                .on_hover_text(format!(
                    "Copying {} of sound into {}. Listening carries on; the take saved before \
                     stays as it was until the copy is done.",
                    length_text(s.secs),
                    s.dest.display()
                ));
        }
        // Always on screen: where the take is going, and whether the strip
        // is live or a take opened. The take's name is cut short with "…"
        // where the facts after it need the room, keeping its watch and
        // state; hovered, it shows whole.
        let facts = {
            let mut sizing = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(ui.available_rect_before_wrap())
                    .layout(egui::Layout::left_to_right(egui::Align::Center))
                    .invisible()
                    .sizing_pass(),
            );
            self.status_facts(&mut sizing);
            sizing.min_rect().width()
        };
        let room = (ui.available_width() - facts - ui.spacing().item_spacing.x).max(0.0);
        let words = self.status_words();
        let (name, rest) = words.split_once(" · ").unwrap_or((&words, ""));
        let font = theme::semibold(13.5);
        let shown = elide(ui, name, &format!(" · {rest}"), &font, room);
        let hover = "The take on show, its watch, and whether it is listening";
        ui.scope(|ui| {
            ui.set_max_width(room);
            ui.add(egui::Label::new(RichText::new(&shown).font(font).color(pal.text)).truncate())
                .on_hover_text(if shown == words {
                    hover.to_string()
                } else {
                    format!("{words}\n\n{hover}")
                });
        });
        self.status_facts(ui);
        // What can be cut short with an ellipsis last, so that the facts
        // before it keep their room. Hovered, a word cut short shows
        // whole; one with no room for a word of it isn't drawn at all.
        let passing = |ui: &mut egui::Ui, text: &str| {
            let font = egui::TextStyle::Small.resolve(ui.style());
            let shown = elide(ui, text, "", &font, ui.available_width());
            if shown.starts_with('…') {
                return;
            }
            let label = ui.label(RichText::new(&shown).small().color(pal.text_secondary));
            if shown != text {
                label.on_hover_text(text);
            }
        };
        if self.batch.is_none() && self.running() && !self.source_label.is_empty() {
            // The input listened to; a take opened is named on the left
            // already.
            passing(ui, &format!("Listening to {}", self.source_label));
        }
        // Errors are a notice at the right instead.
        if let Some((m, false)) = &self.message {
            passing(ui, m);
        }
    }

    /// The status bar's facts after the take's name, none cut short: the
    /// analysis under way, unsaved changes, the recording's state and the
    /// signal.
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn status_facts(&self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        if let Some(b) = &self.batch {
            let done = f64::from_bits(b.progress.load(std::sync::atomic::Ordering::Relaxed));
            let frac = b.duration_s.map_or(0.0, |d| (done / d).clamp(0.0, 1.0));
            ui.add(
                egui::ProgressBar::new(frac as f32)
                    .desired_width(240.0)
                    .desired_height(14.0)
                    .fill(pal.accent)
                    .text(RichText::new(batch_words(frac, b.duration_s.is_some())).small()),
            );
        }
        // A take opened, its painting or masks changed: Save keeps them.
        if self.session.recorder.is_none() && self.marks_unsaved() {
            ui.label(RichText::new("Changes not saved").color(pal.text_secondary))
                .on_hover_text(SAVE_MARKS_HINT);
        }
        if let Some(r) = &self.session.recorder {
            let since = self.unsaved_s();
            let (text, color, hint) = if let Some(fault) = self.session.recording_fault() {
                (
                    "Recording stopped; recovery files retained".to_string(),
                    pal.text,
                    fault.description(),
                )
            } else {
                match self.session.saved_to() {
                    Some(p) => (
                        if since >= 1.0 {
                            format!("The last {} not saved", length_text(since))
                        } else if self.metadata_unsaved() {
                            "Settings not saved".to_string()
                        } else {
                            "Saved".to_string()
                        },
                        pal.text_secondary,
                        format!(
                            "Saved in {}. Sound since then is kept too; Save Again adds it. \
                         New Session and quitting ask first, and a quit that can't ask \
                         keeps it when there is more than 5 s of it, \
                         to offer back at the next start.",
                            p.display()
                        ),
                    ),
                    None if self.analysis_kept_to.is_some() => {
                        let (dir, through_s) = self.analysis_kept_to.clone().unwrap_or_default();
                        let kept = if r.duration_s() - through_s >= 1.0 {
                            format!("the analysis of its first {} kept", length_text(through_s))
                        } else {
                            "its analysis kept".to_string()
                        };
                        (
                            format!("Not saved; {kept}"),
                            pal.text_secondary,
                            format!(
                                "The analysis of the first {} was kept without the sound in {}. \
                                 The sound itself isn't saved: Save keeps it. New Session and \
                                 quitting ask before throwing it away, and a quit that can't \
                                 ask keeps it when there is more than 5 s of it, \
                                 to offer back at the next start.",
                                length_text(through_s),
                                dir.display()
                            ),
                        )
                    }
                    None => (
                        "Not saved".to_string(),
                        pal.text_secondary,
                        "The session's sound is kept as it goes, so Save can keep it at any \
                     point. New Session and quitting ask before throwing it away, and a quit that \
                     can't ask keeps it when there is more than 5 s of it, \
                     to offer back at the next start."
                            .to_string(),
                    ),
                }
            };
            let failed = self.session.recording_fault().is_some();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if failed {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
                    theme::alert_icon(ui.painter(), rect, theme::Alert::Bad, pal);
                } else {
                    theme::dot(
                        ui,
                        if self.running() {
                            pal.live
                        } else {
                            pal.text_tertiary
                        },
                    );
                }
                ui.label(RichText::new(text).small().color(color));
            })
            .response
            .on_hover_text(hint);
        }
        if self.running() {
            if let Some(s) = self.reading().and_then(|r| r.snr) {
                ui.label(
                    RichText::new(format!("signal {s:.0}×"))
                        .small()
                        .color(pal.text_secondary),
                )
                .on_hover_text(*help::SIGNAL);
            }
        }
    }

    /// The starting arrangement for a strip direction, keeping which panes
    /// are hidden.
    pub(super) fn relayout(&mut self, horizontal: bool) {
        let shown: Vec<(Pane, bool)> = Pane::ALL
            .into_iter()
            .map(|p| (p, pane_visible(&self.panes.tiles, p)))
            .collect();
        self.panes = default_layout(horizontal);
        self.panes_dragged.clear();
        for (p, on) in shown {
            set_pane_visible(&mut self.panes.tiles, p, on);
        }
    }

    /// What shows before there is anything to show: how to begin, and the
    /// two ways to.
    /// Before anything is open: the three ways in. While a recording is
    /// being analysed, how far it has got.
    pub(super) fn empty_state(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        // Space above the card, but never so much that the card (as tall as
        // on the last frame) runs out of the pane.
        let h_id = ui.id().with("empty state height");
        let last_h: f32 = ui.data(|d| d.get_temp(h_id)).unwrap_or(0.0);
        let room = (ui.available_height() - last_h).max(0.0);
        ui.add_space((ui.available_height() * 0.16).clamp(16.0, 110.0).min(room));
        ui.vertical_centered(|ui| {
            let w = 520.0_f32.min(ui.available_width());
            ui.allocate_ui(Vec2::new(w, 0.0), |ui| {
                let shown = theme::card().fill(pal.card).show(ui, |ui| {
                    ui.set_width(w - 28.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 10.0;
                        if self.batch.is_some() {
                            self.batch_progress(ui);
                        } else {
                            self.ways_in(ui);
                        }
                    });
                });
                let h = shown.response.rect.height();
                if h != last_h {
                    ui.data_mut(|d| d.insert_temp(h_id, h));
                    ui.ctx().request_repaint();
                }
            });
        });
    }

    /// Nothing that sounds like a watch has been heard in the take, after a
    /// few seconds of listening: one sentence then, not empty charts and
    /// cards of figures that would describe noise.
    pub(super) fn no_watch_yet(&self) -> bool {
        let Some(live) = &self.live else {
            return false;
        };
        self.batch.is_none()
            && live.duration_s() > 6.0
            && !self.trend.iter().any(|p| p.heard)
            && self.reading().is_none_or(|r| !r.watch_heard)
    }

    pub(super) fn no_watch(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let line = self.reading().map_or_else(
            || timegrapher_core::beats::no_regular_beat_text(None, None),
            |r| no_regular_beat(&r),
        );
        ui.add_space((ui.available_height() * 0.16).clamp(16.0, 110.0));
        ui.vertical_centered(|ui| {
            let w = 520.0_f32.min(ui.available_width());
            ui.allocate_ui(Vec2::new(w, 0.0), |ui| {
                theme::card().fill(pal.card).show(ui, |ui| {
                    ui.set_width(w - 28.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;
                        ui.label(RichText::new(&line).font(theme::semibold(17.0)))
                            .on_hover_text(*help::NO_REGULAR_BEAT);
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    "The sound so far has no beats at regular intervals. The \
                                     readings and charts appear once a regular beat is found.",
                                )
                                .small()
                                .color(pal.text_secondary),
                            )
                            .wrap(),
                        );
                    });
                });
            });
        });
    }

    /// Recording chosen while a stopped microphone session is in hand: the
    /// screen offers a recording instead of showing the session's figures
    /// as if they were one. The session is kept.
    pub(super) fn mic_session_set_aside(&self) -> bool {
        self.input == Input::File && self.session.mic() && !self.running()
    }

    /// Before anything is open: where to start, since the Watches view
    /// and the File menu hold the ways in.
    fn ways_in(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        ui.label(RichText::new("Timegrapher").font(theme::semibold(20.0)));
        let line = |ui: &mut egui::Ui, text: &str| {
            ui.add(egui::Label::new(RichText::new(text).color(pal.text_secondary)).wrap());
        };
        if self.mic_session_set_aside() {
            line(
                ui,
                "Your microphone session is set aside while this shows: Listen, on the \
                 Listen card, starts a new one.",
            );
        }
        line(
            ui,
            "Listen, on the Listen card, listens to the watch on the microphone without \
             picking a watch; Save keeps what it heard. For a watch's history, pick it on \
             the Calibre card's title, or in the Watches view, then Listen, \
             Positions Test or Power Reserve Test, and each take is saved into it. New \
             Watch… adds one.",
        );
        line(
            ui,
            "Recordings and folders open from the File menu, analysed all at once.",
        );
    }

    /// How far the analysis of a recording has got, and a way to stop it.
    fn batch_progress(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let Some(b) = &self.batch else { return };
        let done = f64::from_bits(b.progress.load(std::sync::atomic::Ordering::Relaxed));
        ui.label(RichText::new(format!("Analysing {}", b.label)).font(theme::semibold(18.0)));
        let frac = b.duration_s.map(|d| (done / d.max(1e-9)).clamp(0.0, 1.0));
        // The bar's track in the control colour, to stand out on the card.
        ui.scope(|ui| {
            ui.visuals_mut().extreme_bg_color = pal.control;
            ui.add(
                egui::ProgressBar::new(frac.unwrap_or(0.0) as f32)
                    .desired_width(ui.available_width())
                    .desired_height(16.0)
                    .fill(pal.accent)
                    .text(RichText::new(match frac {
                        Some(f) => format!("{:.0}%", 100.0 * f),
                        None => String::new(),
                    })),
            );
        });
        let elapsed = b.started.elapsed().as_secs_f64();
        let mut line = match b.duration_s {
            Some(d) => format!(
                "{} of {} of sound analysed",
                length_text(done),
                length_text(d)
            ),
            None => format!("{} of sound analysed", length_text(done)),
        };
        // Time left at the pace so far, once there is a pace to go by.
        if let (Some(d), true) = (b.duration_s, done > 0.0 && elapsed > 3.0) {
            let left = (d - done).max(0.0) * elapsed / done;
            line += &format!(" · about {} to go", length_text(left));
        }
        ui.label(RichText::new(line).color(pal.text_secondary));
        if b.files > 1 {
            ui.label(
                RichText::new(format!(
                    "{} files joined in order, {} to {}.",
                    b.files, b.first, b.last
                ))
                .small()
                .color(pal.text_tertiary),
            );
        }
        if let Some(n) = &b.order_note {
            theme::alert_label(ui, theme::Alert::Warning, n, true);
        }
        if ui
            .add(theme::secondary(ui, "Cancel"))
            .on_hover_text("Stop analysing this recording")
            .clicked()
        {
            self.batch = None;
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    }
}

/// Where a galley's first line sits: its baseline, down from its top.
fn baseline(g: &egui::Galley) -> f32 {
    g.rows
        .first()
        .and_then(|r| Some(r.pos.y + r.row.glyphs.first()?.pos.y))
        .unwrap_or(g.size().y * 0.8)
}

/// The least a card's figure is made, as a share of its full size, to
/// keep its ± whole beside it before the ± stacks or leaves (#825).
const FIGURE_LEAST: f32 = 0.75;

/// A readings card's figure row: the big figure ("—" while there is none),
/// then its ± and unit on the figure's baseline, or what the reading waits
/// on in their place. On a compact card the figure is smaller and the ±
/// sits over the unit beside it. A figure too wide for the card (noise
/// read as beats can give thousands of s/d) is made smaller to show whole.
pub(super) fn figure_row(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    value: Option<&str>,
    unit: &str,
    status: Option<&(String, String)>,
    hover: &str,
    compact: bool,
) {
    let pal = theme::pal(ui);
    let (text, colour) = match value {
        // A true minus sign, as wide as the plus.
        Some(v) => (v.replace('-', "−"), pal.text),
        None => ("—".to_string(), pal.text_tertiary),
    };
    let side_text = status.map_or(unit, |s| s.0.as_str());
    let mut lines = side_lines(side_text, status.is_some(), compact);
    let mut side_font = egui::FontId::proportional(if compact { 12.0 } else { 15.0 });
    let lay_side = |font: &egui::FontId, lines: &[String]| -> Vec<_> {
        lines
            .iter()
            .map(|l| {
                ui.painter()
                    .layout_no_wrap(l.clone(), font.clone(), pal.text_secondary)
            })
            .collect()
    };
    let mut side = lay_side(&side_font, &lines);
    let mut side_w = side.iter().map(|g| g.size().x).fold(0.0, f32::max);
    // Words in the figure's place ("No Regular Beat") show whole: with no
    // figure, the dash gives them its room, and they are made smaller
    // only when even that is too narrow.
    let figure_w = ui
        .painter()
        .layout_no_wrap(
            text.clone(),
            theme::display(if compact {
                theme::FIGURE_COMPACT_PT
            } else {
                theme::FIGURE_PT
            }),
            colour,
        )
        .size()
        .x;
    let no_dash = value.is_none() && status.is_some() && figure_w + 6.0 + side_w > rect.width();
    if no_dash && side_w > rect.width() {
        side_font.size = (side_font.size * rect.width() / side_w).floor().max(9.0);
        side = lay_side(&side_font, &lines);
        side_w = side.iter().map(|g| g.size().x).fold(0.0, f32::max);
    }
    // A ± too wide to sit beside the figure, even with the figure made a
    // little smaller, stacks over its unit, and, still too wide, leaves
    // the card for the hover: never cut into a number that reads as zero
    // (#825).
    let mut moved = None;
    if status.is_none() && value.is_some() && side_text.starts_with('±') {
        let fits = |l: &[String]| {
            let w = lay_side(&side_font, l)
                .iter()
                .map(|g| g.size().x)
                .fold(0.0, f32::max);
            figure_w * FIGURE_LEAST + 6.0 + w <= rect.width()
        };
        let (fitted, cut) = pm_fitted(side_text, lines, fits);
        lines = fitted;
        moved = cut.then_some(side_text);
        side = lay_side(&side_font, &lines);
        side_w = side.iter().map(|g| g.size().x).fold(0.0, f32::max);
    }
    let room = rect.width() - if side_w > 0.0 { side_w + 6.0 } else { 0.0 };
    let full = if compact {
        theme::FIGURE_COMPACT_PT
    } else {
        theme::FIGURE_PT
    };
    let lay = |size: f32| {
        ui.painter()
            .layout_no_wrap(text.clone(), theme::display(size), colour)
    };
    let mut big = lay(full);
    if big.size().x > room && value.is_some() {
        big = lay((full * room / big.size().x).floor().max(12.0));
    }
    let top = rect.center().y - big.size().y / 2.0;
    let base = top + baseline(&big);
    let painter = ui.painter().with_clip_rect(rect.expand(2.0));
    let x = if no_dash {
        rect.left()
    } else {
        rect.left() + big.size().x + 6.0
    };
    if !no_dash {
        painter.galley(egui::pos2(rect.left(), top), big, colour);
    }
    let step = side_font.size * 1.2;
    let n = side.len();
    for (i, g) in side.into_iter().enumerate() {
        let line_base = base - (n - 1 - i) as f32 * step;
        let y = line_base - baseline(&g);
        painter.galley(egui::pos2(x, y), g, pal.text_secondary);
    }
    let hover = figure_hover(status, hover, moved.map(|pm| format!("{text} {pm}")));
    if !hover.is_empty() {
        ui.interact(
            rect,
            ui.id().with(("figure", rect.left() as i32)),
            egui::Sense::hover(),
        )
        .on_hover_text(hover);
    }
}

/// The lines beside a card's figure: one line in Body, or on a compact
/// card two small lines, the ± over the unit, or a status's words split
/// as evenly as they allow.
fn side_lines(side_text: &str, status: bool, compact: bool) -> Vec<String> {
    // Beside the figure: one line in Body, or on a compact card two small
    // lines, the ± over the unit.
    if side_text.is_empty() {
        Vec::new()
    } else if !compact {
        vec![side_text.to_string()]
    } else if status {
        // Two lines as even as the words allow: "Settling," over "12 s",
        // "Waiting for" over "Beats".
        let splits = side_text.match_indices(' ').map(|(i, _)| i);
        match splits.min_by_key(|&i| i.abs_diff(side_text.len() - i)) {
            Some(i) => vec![side_text[..i].to_string(), side_text[i + 1..].to_string()],
            None => vec![side_text.to_string()],
        }
    } else {
        // "± 8.6 s/d" stacks; "± 0.8°" and "bph" stay one line.
        match side_text.rsplit_once(' ') {
            Some((a, b)) if a.starts_with('±') && a.contains(' ') => {
                vec![a.to_string(), b.to_string()]
            }
            _ => vec![side_text.to_string()],
        }
    }
}

/// The lines of a ± and its unit (`side_text`, laid out as `lines`) that
/// `fits` beside the figure: as they are, else the ± stacked over the
/// unit, else the unit alone; and whether the ± was left out.
fn pm_fitted(
    side_text: &str,
    lines: Vec<String>,
    fits: impl Fn(&[String]) -> bool,
) -> (Vec<String>, bool) {
    if fits(&lines) {
        return (lines, false);
    }
    let (pm, unit) = match side_text.rsplit_once(' ') {
        Some((a, b)) if a.contains(' ') => (a, b),
        _ => (side_text, ""),
    };
    let stacked = vec![pm.to_string(), unit.to_string()];
    if lines.len() == 1 && !unit.is_empty() && fits(&stacked) {
        return (stacked, false);
    }
    let alone = if unit.is_empty() {
        Vec::new()
    } else {
        vec![unit.to_string()]
    };
    (alone, true)
}

/// A card figure's hover: what the reading waits on, then `hover`, with
/// the figure and the ± the card had no room for (`moved`) first.
fn figure_hover(status: Option<&(String, String)>, hover: &str, moved: Option<String>) -> String {
    let hover = match status {
        Some((_, h)) if !h.is_empty() && !hover.is_empty() => format!("{h}\n\n{hover}"),
        Some((_, h)) if !h.is_empty() => h.clone(),
        _ => hover.to_string(),
    };
    match moved {
        Some(m) if hover.is_empty() => m,
        Some(m) => format!("{m}\n\n{hover}"),
        None => hover,
    }
}
