//! The whole pipeline: recording in, per-beat log and summary out.

use crate::amplitude::{AmplitudeConfig, AmplitudeWindow};
use crate::audio::Audio;
use crate::beats::{self, Beat};
use crate::confidence::{Confidence, Doubts};
use crate::dsp::percentile;
use crate::dsp::{envelope, EnvelopeConfig};
use crate::measure::NoWatchReason;
use crate::periodicity::{self, Component, Series};
use crate::timing::{self, TimingFit, WindowFit};
use crate::usbclock::{self, UsbClock};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct AnalysisConfig {
    /// Beat rate; `None` guesses it from the recording.
    pub bph: Option<u32>,
    /// Where `bph` came from (`None` with no `bph`: Auto).
    pub bph_from: Option<crate::set_rate::BphFrom>,
    pub envelope: EnvelopeConfig,
    pub amplitude: AmplitudeConfig,
    /// Window for the rate-over-time series, seconds.
    pub rate_window_s: f64,
    /// Window for amplitude measurements, seconds.
    pub amplitude_window_s: f64,
    pub escape_teeth: u32,
    /// The lift angle comes from the calibre or the user, not a default.
    pub lift_known: bool,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        AnalysisConfig {
            bph: None,
            bph_from: None,
            envelope: EnvelopeConfig::default(),
            amplitude: AmplitudeConfig::default(),
            rate_window_s: crate::measure::RATE_WINDOW_S,
            amplitude_window_s: crate::distribution::STRETCH_S,
            escape_teeth: crate::take::ESCAPE_TEETH,
            lift_known: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub duration_s: f64,
    pub sample_rate: u32,
    pub bph: u32,
    /// The supplied beat rate (option, calibre or session file), never a guess.
    /// With no supplied rate, no-watch text gives scatter only in ms (#857).
    pub bph_set: Option<u32>,
    /// Where `bph_set` came from; left out with no rate set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bph_set_from: Option<crate::set_rate::BphFrom>,
    /// The beat rate the recording sounds like, when a set `bph` differs
    /// from it: every reading is then measured against the set rate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bph_heard: Option<u32>,
    pub beats_found: usize,
    /// `beats_found` over the beats the recording's length holds at `bph`.
    pub beats_found_fraction: f64,
    /// False when a regular beat was looked for and not found, as on an
    /// empty stand: the readings below are then left out, since they would
    /// describe noise, and `jitter_us` gives the timing scatter. Regularity
    /// is assessed only with [`crate::measure::SHORT_SPAN_S`] of sound or
    /// more; a shorter recording keeps its readings and this stays true
    /// (see [`crate::measure::watch_heard`]).
    pub watch_heard: bool,
    /// Fit over the whole recording; its beat error is the drop's.
    pub overall: Option<TimingFit>,
    /// Signed beat error measured from the unlock, the figure checked
    /// against tg's beat error in regress: median over the amplitude
    /// windows, ms. Positive means Tick comes late; each take starts
    /// positive ([`crate::beats::SideCheck`]).
    pub beat_error_unlock_ms: Option<f64>,
    /// Source of the selected beat error, absent when neither event was read.
    pub beat_error_source: Option<crate::session::BeatErrorSource>,
    /// Spread of the windowed rate (5th and 95th percentiles), s/d.
    pub rate_p05: Option<f64>,
    pub rate_p95: Option<f64>,
    /// Median per-beat timing jitter (the timing scatter) within the rate
    /// windows, microseconds.
    pub jitter_us: Option<f64>,
    /// `jitter_us` as a share of the beat period at `bph`, 0 to 1 (see
    /// [`beats::timing_scatter_fraction`]); what `watch_heard` is decided on.
    pub timing_scatter_fraction: Option<f64>,
    /// Median amplitude over the recording, degrees.
    pub amplitude_deg: Option<f64>,
    pub amplitude_even_deg: Option<f64>,
    pub amplitude_odd_deg: Option<f64>,
    /// Standard errors of the three amplitudes above, degrees: their
    /// precision from the spread between windows, not their accuracy,
    /// which rests on the lift angle (see [`crate::confidence::standard_error`]).
    pub amplitude_se_deg: Option<f64>,
    pub amplitude_even_se_deg: Option<f64>,
    pub amplitude_odd_se_deg: Option<f64>,
    pub lift_deg: f64,
    /// The escapement read for (#2313), given only for a co-axial: its
    /// beat error is the unlock's or none, and its amplitude an estimate.
    #[serde(skip_serializing_if = "crate::escapement::Escapement::is_swiss_lever")]
    pub escapement: crate::escapement::Escapement,
    /// Strongest periodic components of the timing and the amplitude.
    pub timing_periods: Vec<Component>,
    pub amplitude_periods: Vec<Component>,
    /// How much of the recording is clipped.
    pub clipping: Clipping,
    /// The sound card's clock error from the USB lines in the recording,
    /// when it has them. Rates above stay on the card's clock, as tg reads
    /// them; [`Summary::rate_corrected_s_per_day`] is the corrected rate.
    /// A reading that doesn't apply ([`UsbClock::applies`]) is kept here
    /// with its `withheld` reason, corrects nothing, and is a caveat.
    pub usb_clock: Option<UsbClock>,
    /// The whole-recording rate on the card's clock, as tg reads it: the
    /// same figure as `overall`'s, given here beside the corrected one, s/d.
    pub rate_s_per_day: Option<f64>,
    /// Rate corrected onto true time with `usb_clock`, when it applies, s/d.
    pub rate_corrected_s_per_day: Option<f64>,
    /// Conditions detected, as flags; each set one is also a caveat.
    pub doubts: Doubts,
    /// Each reading's ± (standard error) for rate, amplitude and beat
    /// error; `None` when no regular beat is found.
    pub confidence: Option<Confidence>,
    /// Conditions detected that the readings rest on, each with its
    /// figure (see [`crate::measure`]); empty when none is.
    pub caveats: Vec<crate::measure::Caveat>,
    /// The shape of the Tick and Tock and the beat error early and late
    /// (see [`crate::shape::Measures`]); `None` when no readings are given.
    pub shape: Option<crate::shape::Measures>,
    /// The reading waits for its verdict, under 10 s of sound
    /// ([`crate::measure::awaiting_verdict`], #2333): no figures and no
    /// verdict either way. Not in the JSON, which says so by its missing
    /// figures and its `short_span` caveat.
    #[serde(skip)]
    pub awaiting: bool,
}

impl Summary {
    /// [`Self::withhold_readings`] while the reading waits for its
    /// verdict ([`Self::awaiting`]).
    pub fn withhold_while_awaiting(&mut self) {
        if self.awaiting {
            self.withhold_readings();
        }
    }

    /// Clear every reading, as when no regular beat is found: they would
    /// describe noise. What says so (`watch_heard`, `jitter_us`,
    /// `timing_scatter_fraction`, `beats_found`) and what describes the recording and the sound card
    /// stay.
    pub fn withhold_readings(&mut self) {
        self.overall = None;
        self.beat_error_unlock_ms = None;
        self.beat_error_source = None;
        self.caveats
            .retain(|c| !matches!(c, crate::measure::Caveat::BeatErrorFromDrop { .. }));
        self.rate_p05 = None;
        self.rate_p95 = None;
        self.amplitude_deg = None;
        self.amplitude_even_deg = None;
        self.amplitude_odd_deg = None;
        self.amplitude_se_deg = None;
        self.amplitude_even_se_deg = None;
        self.amplitude_odd_se_deg = None;
        self.timing_periods.clear();
        self.amplitude_periods.clear();
        self.rate_s_per_day = None;
        self.rate_corrected_s_per_day = None;
        self.confidence = None;
        self.shape = None;
    }

    /// Whether a watch is heard: its verdict says so and is not still
    /// waiting ([`Self::awaiting`]). A reading still waiting, under 10 s,
    /// has heard none yet (#2333), however `watch_heard` reads.
    pub fn heard_as_watch(&self) -> bool {
        self.watch_heard && !self.awaiting
    }

    /// Count no beats of a watch where none is heard (Norm on #2378), or
    /// none yet ([`Self::heard_as_watch`], #2333):
    /// the peaks found are noise, so `beats_found` and its share are 0
    /// ([`crate::measure::watch_beats`]), and `beats_missing` follows from
    /// that share by its one rule, with its caveat. No rate is heard in
    /// noise: `bph_heard` and the `set_rate_disagrees` caveat are left
    /// out, and with no rate set, so is `timing_scatter_fraction`, a
    /// share of a rate guessed from noise. `jitter_us` stays, the
    /// evidence. Only the documents that count beats this way call it
    /// (`analyze/3`, [`crate::analyze_doc::body`]); the verdict itself
    /// is made before, on the fraction at the rate tried (#857).
    pub fn count_no_watch(&mut self) {
        if self.heard_as_watch() {
            return;
        }
        let (found, bph) =
            crate::measure::watch_beats(false, self.beats_found, self.bph, self.bph_set);
        self.beats_found = found;
        self.beats_found_fraction = 0.0;
        self.bph_heard = None;
        if bph.is_none() {
            self.timing_scatter_fraction = None;
        }
        self.doubts.beats_missing = crate::measure::beats_missing(self.beats_found_fraction);
        self.caveats.retain(|c| {
            !matches!(
                c,
                crate::measure::Caveat::BeatsMissing { .. }
                    | crate::measure::Caveat::SetRateDisagrees { .. }
            )
        });
        if self.doubts.beats_missing {
            crate::measure::add(
                &mut self.caveats,
                crate::measure::Caveat::BeatsMissing {
                    fraction: self.beats_found_fraction,
                },
            );
        }
    }

    /// A reading in a lock judged off every standard rate on Auto
    /// ([`crate::timing::OffStandard::of`], Norm on #2223 and #2316): the
    /// rate readings and their ± are left out, and the caveat
    /// `off_standard_rate` gives the beat rate heard from `overall`, on
    /// the card's clock or corrected at `ppm` fast where the reader
    /// corrects it ([`crate::analyze_doc::body`]). A caveat made before is
    /// made again; the verdict is never judged again here. `overall`
    /// stays: it is the fit at `bph`, and the drop beat error is read
    /// from it.
    pub fn read_off_standard(&mut self, ppm: Option<f64>) {
        // A reading read before keeps its rate's ± in the caveat, as
        // `bph × σ / 86,400` ([`crate::timing::MeasuredRate::of`]).
        let before = crate::measure::off_standard(&self.caveats)
            .and_then(|o| o.bph_measured_se)
            .map(|s| s * 86_400.0 / f64::from(self.bph));
        self.caveats
            .retain(|c| !matches!(c, crate::measure::Caveat::OffStandardRate(_)));
        let rate = self.overall.as_ref().map(|f| {
            ppm.map_or(f.rate_s_per_day, |p| {
                crate::clockstore::correct_rate(f.rate_s_per_day, p)
            })
        });
        let se = self.confidence.as_ref().and_then(|c| c.rate.se).or(before);
        let Some(o) = crate::timing::OffStandard::reading(self.bph, rate, se) else {
            return;
        };
        self.rate_s_per_day = None;
        self.rate_corrected_s_per_day = None;
        (self.rate_p05, self.rate_p95) = (None, None);
        crate::measure::off_standard_withheld(o, self.confidence.as_mut(), &mut self.caveats);
    }

    /// The sound card's clock is known from elsewhere (a stored or given
    /// correction): the `clock_unknown` condition no longer holds.
    pub fn clock_known(&mut self) {
        self.doubts.clock_unknown = false;
        self.caveats
            .retain(|c| *c != crate::measure::Caveat::ClockUnknown);
    }

    /// The beat error this reading leads with, signed, ms, and where it
    /// is from: the one [`settle_beat_error`] chose (`beat_error_source`),
    /// read back, never chosen again, so every reader of a summary (the
    /// doctor's quick look among them, #2095) gives the same one.
    pub fn headline_beat_error(&self) -> Option<(f64, crate::session::BeatErrorSource)> {
        use crate::session::BeatErrorSource;
        let source = self.beat_error_source?;
        let value = match source {
            BeatErrorSource::Unlock => self.beat_error_unlock_ms,
            BeatErrorSource::Drop => self.overall.map(|f| f.beat_error_ms),
        }?;
        Some((value, source))
    }
}

/// How much of the recording is clipped: the samples, and the beats with a
/// clipped sample in their sound.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Clipping {
    /// Clipped samples (see [`crate::diagnose::clipped_indices`]).
    pub samples: usize,
    /// Beats with a clipped sample within their template window.
    pub beats: usize,
    /// `beats` as a fraction of the beats found.
    pub beat_fraction: f64,
    /// Highest sample, dB relative to full scale.
    pub peak_dbfs: f64,
}

impl Clipping {
    /// Above this share of clipped beats the `clipped` condition is named
    /// among the caveats.
    pub const NOTED_FRACTION: f64 = 0.01;

    pub fn measure(x: &[f32], fs: f64, beats: &[Beat]) -> Clipping {
        let idx = crate::diagnose::clipped_indices(x, CLIP_LEVEL);
        let (pre, post) = (
            (beats::PRE_S * fs).round() as usize,
            (beats::POST_S * fs).round() as usize,
        );
        let hit = |b: &Beat| {
            let c = (b.time * fs).round().max(0.0) as usize;
            let k = idx.partition_point(|&i| i < c.saturating_sub(pre));
            idx.get(k).is_some_and(|&i| i <= c + post)
        };
        let n = beats.iter().filter(|b| hit(b)).count();
        let peak = x.iter().fold(0f32, |m, v| m.max(v.abs()));
        Clipping::of(idx.len(), n, beats.len(), peak)
    }

    /// The clipping of `samples` clipped samples and `clipped` clipped
    /// beats of the `beats` found, with `peak` the loudest sample's size
    /// (full scale 1): as [`Self::measure`] counts it in a recording, or a
    /// streamed reading ([`crate::stream::BeatLog::clipped_samples`] and
    /// `clipped_beats`) counts it in a whole take.
    pub fn of(samples: usize, clipped: usize, beats: usize, peak: f32) -> Clipping {
        Clipping {
            samples,
            beats: clipped,
            beat_fraction: if beats == 0 {
                0.0
            } else {
                clipped as f64 / beats as f64
            },
            peak_dbfs: 20.0 * f64::from(peak).max(1e-10).log10(),
        }
    }

    /// More than [`Self::NOTED_FRACTION`] of the beats clipped.
    pub fn noted(&self) -> bool {
        Self::noted_fraction(self.beat_fraction)
    }

    /// Whether `fraction` of the beats clipped is enough to name the
    /// `clipped` caveat: more than [`Self::NOTED_FRACTION`]. The one rule
    /// every surface asks, from a [`Clipping`] or from a stored fraction.
    pub fn noted_fraction(fraction: f64) -> bool {
        fraction > Self::NOTED_FRACTION
    }
}

/// A sample at or above this magnitude counts as clipped, as in `doctor`.
pub(crate) const CLIP_LEVEL: f32 = 0.99;

#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub summary: Summary,
    pub beats: Vec<Beat>,
    /// Each beat's offset from the whole-recording constant-rate fit, seconds.
    pub residuals: Vec<f64>,
    pub rate_windows: Vec<WindowFit>,
    pub amplitude_windows: Vec<AmplitudeWindow>,
    /// The rate of the lock read ([`crate::timing::lock_rate`]), from
    /// its first beats after any lead, with its jumps as cuts: what
    /// [`analyze`] judges the lock on (Norm on #2316).
    #[serde(skip)]
    pub(crate) lock_rate: Option<f64>,
}

pub fn analyze(audio: &Audio, cfg: &AnalysisConfig) -> Analysis {
    let fs = audio.sample_rate as f64;
    let env = envelope(&audio.samples, fs, &cfg.envelope);
    let read = analyze_guessed(audio, cfg, &env);
    judge_lock(audio, cfg, &env, read)
}

/// The lock [`analyze`] read on Auto, judged once on its first
/// [`beats::GUESS_SPAN_S`] of beats ([`crate::timing::lock_rate`], Norm on
/// #2316): read again at the standard rate nearest the beat rate heard
/// when that is in reach and is not the rate guessed, as if it were set
/// (its `bph_set` stays none); off every standard rate
/// ([`Summary::read_off_standard`]) when none is in reach.
fn judge_lock(audio: &Audio, cfg: &AnalysisConfig, env: &[f32], mut read: Analysis) -> Analysis {
    let s = &read.summary;
    if cfg.bph.is_some() || !s.watch_heard {
        return read;
    }
    let bph = s.bph;
    let Some(rate) = read.lock_rate else {
        return read;
    };
    match crate::timing::OffStandard::nearest_in_reach(bph, rate) {
        Some(n) if n != bph => {
            let again = read_take(audio, cfg, Some(n));
            let again = analyze_read(audio, cfg, env, again, (n, n));
            if again.summary.watch_heard {
                return again;
            }
        }
        Some(_) => {}
        None => read.summary.read_off_standard(None),
    }
    read
}

/// [`analyze`]'s read at the rate set, or guessed on Auto, before its
/// lock is judged.
fn analyze_guessed(audio: &Audio, cfg: &AnalysisConfig, env: &[f32]) -> Analysis {
    let fs = audio.sample_rate as f64;
    let first_guess = beats::guess_bph(env, fs);
    let bph = cfg.bph.unwrap_or(first_guess);
    let (log, lead_s) = read_take(audio, cfg, Some(bph));
    // The rate heard is guessed where the watch starts, after any lead of
    // noise, which is not taken for a rate (#1423).
    let after_lead =
        lead_s.map(|t| beats::guess_bph(&env[((t * fs) as usize).min(env.len())..], fs));
    let guessed = match cfg.bph {
        Some(_) => after_lead.unwrap_or(first_guess),
        None => bph,
    };
    let first = analyze_read(audio, cfg, env, (log, lead_s), (bph, guessed));
    let doubted = !first.summary.watch_heard || after_lead.is_some_and(|g| g != bph);
    if cfg.bph.is_some() || !doubted {
        return first;
    }
    // No watch at the rate guessed from the first 10 s of sound, or a
    // watch after a lead that sounds like another rate: those 10 s can be
    // a lead of stand noise before a slow watch (#1423). The recording is
    // read again at the rate it sounds like where the watch starts or,
    // with no watch heard, at the rate guessed as session reads a file,
    // afresh at each chunk's sound until a watch is heard.
    let settled = match after_lead {
        Some(g) if g != bph => g,
        _ => read_take(audio, cfg, None).0.bph,
    };
    if settled == bph {
        return first;
    }
    let again = read_take(audio, cfg, Some(settled));
    let again = analyze_read(audio, cfg, env, again, (settled, settled));
    if reads_better(&again.summary, &first.summary) {
        again
    } else {
        first
    }
}

/// Whether a second read of a recording (`again`, at another rate) is
/// kept over the first (#1423): when it hears a watch and the first does
/// not. When both do: the read that kept the larger share of the beats
/// its span holds (`kept_of`), by more than 2 standard errors of the
/// difference; within that, at exactly 2:1, the higher rate, as a read at
/// half the watch's rate keeps every other beat at a full share; else the
/// one whose rate fit's residuals (robust SD, seconds, over the beats
/// kept) are smaller. A read at a harmonic of the watch's rate sets beats
/// on the wrong sound or between beats, or keeps only the cleanest few.
/// A tie keeps the first.
pub fn reads_better(again: &Summary, first: &Summary) -> bool {
    if !again.watch_heard {
        return false;
    }
    if !first.watch_heard {
        return true;
    }
    let (Some((k2, n2)), Some((k1, n1))) = (kept_of(again), kept_of(first)) else {
        return again.overall.is_some() && first.overall.is_none();
    };
    if let Some(higher) = crate::confidence::share_differs(k1, n1, k2, n2) {
        return higher;
    }
    if again.bph == 2 * first.bph || first.bph == 2 * again.bph {
        return again.bph > first.bph;
    }
    let sd = |s: &Summary| s.overall.map_or(f64::INFINITY, |f| f.jitter_us);
    sd(again) < sd(first)
}

/// A read's beats kept by its rate fit (at most as many as it holds), and
/// the beats its span after any lead holds at its rate, counting the
/// beats at both ends as the kept ones are counted.
fn kept_of(s: &Summary) -> Option<(f64, f64)> {
    let lead = s.caveats.iter().find_map(|c| match c {
        crate::measure::Caveat::LeadLeftOut { seconds } => Some(*seconds),
        _ => None,
    });
    let span = s.duration_s - lead.unwrap_or(0.0);
    let held = crate::beats::expected_beats(span, s.bph) + 1.0;
    s.overall
        .filter(|_| span > 0.0)
        .map(|f| ((f.beats_used as f64).min(held), held))
}

/// [`analyze`] of a recording read at `bph` (`log`, with any lead of
/// noise left out up to `lead_s`), `guessed` the rate it sounds like.
fn analyze_read(
    audio: &Audio,
    cfg: &AnalysisConfig,
    env: &[f32],
    (log, lead_s): (crate::stream::BeatLog, Option<f64>),
    (bph, guessed): (u32, u32),
) -> Analysis {
    let fs = audio.sample_rate as f64;
    let from_s = lead_s.unwrap_or(0.0);
    let lock_rate = crate::timing::lock_rate(&log.beats, bph, &log.jumps);
    // Under 10 s the beats' averaged sound says whether a watch is heard.
    let beat_sound = log.beat_sound(&log.beats, &cfg.amplitude);
    let beats = log.beats;
    let amplitude_windows = log.amplitude_windows;
    let fits = fit_take(
        &beats,
        &amplitude_windows,
        &log.jumps,
        bph,
        from_s,
        audio.duration(),
        cfg,
    );
    let rates = fits.rates();
    let duration = audio.duration();
    let (timing_periods, amplitude_periods) =
        periods(&beats, &fits.residuals, &fits.clear, bph, cfg);

    // The whole take: its own values are all there are to borrow (#1470).
    let take: Vec<&AmplitudeWindow> = fits.clear.iter().collect();
    let se = |f: &dyn Fn(&AmplitudeWindow) -> Option<f64>| {
        crate::confidence::amplitude_windows_se(take.iter().copied(), &take, f)
    };
    let heard = hear(audio, &beats, bph, &fits, from_s, beat_sound);
    let watch_heard = heard.no_watch.is_none();
    // A withheld figure has no ± (#1429).
    let f = &fits.figures;
    let amplitude_se_deg = f.amplitude_deg.and(se(&|w| w.mean()));
    let assessment = assess_take(
        cfg,
        &beats,
        (&log.jumps, log.by_count.len()),
        (bph, guessed),
        &fits,
        &heard,
        lead_s,
    );
    let mut confidence = assessment.confidence;
    if let Some(c) = confidence.as_mut() {
        f.floor_confidence(c);
    }
    let mut caveats = assessment.caveats;
    if log.lock_template_kept {
        crate::measure::add(&mut caveats, crate::measure::Caveat::LockTemplateKept);
    }
    let shape = (watch_heard && !heard.awaiting).then(|| take_shape(env, fs, &beats, &fits, bph));
    let mut summary = Summary {
        bph_set: cfg.bph,
        bph_set_from: cfg.bph.and(cfg.bph_from),
        duration_s: duration,
        sample_rate: audio.sample_rate,
        bph,
        bph_heard: (bph != guessed).then_some(guessed),
        beats_found: beats.len(),
        beats_found_fraction: if heard.expected > 0.0 {
            beats.len() as f64 / heard.expected
        } else {
            0.0
        },
        watch_heard,
        overall: fits.overall,
        beat_error_source: None,
        beat_error_unlock_ms: fits.figures.beat_error_unlock_ms,
        rate_p05: percentile(&rates, 0.05),
        rate_p95: percentile(&rates, 0.95),
        jitter_us: heard.jitter_us,
        timing_scatter_fraction: beats::timing_scatter_fraction(heard.jitter_us, bph),
        amplitude_deg: fits.figures.amplitude_deg,
        amplitude_even_deg: fits.figures.amplitude_even_deg,
        amplitude_odd_deg: fits.figures.amplitude_odd_deg,
        amplitude_se_deg,
        amplitude_even_se_deg: f.amplitude_even_deg.and(se(&|w| w.even_deg)),
        amplitude_odd_se_deg: f.amplitude_odd_deg.and(se(&|w| w.odd_deg)),
        lift_deg: cfg.amplitude.lift_deg,
        escapement: cfg.amplitude.escapement,
        timing_periods,
        amplitude_periods,
        clipping: heard.clipping,
        usb_clock: heard.usb_clock,
        rate_s_per_day: fits.overall.map(|f| f.rate_s_per_day),
        rate_corrected_s_per_day: heard.usb_clock.and_then(UsbClock::applied).and_then(|c| {
            fits.overall
                .map(|f| crate::clockstore::correct_rate(f.rate_s_per_day, c.ppm))
        }),
        doubts: assessment.doubts,
        confidence,
        caveats,
        shape,
        awaiting: heard.awaiting,
    };
    summary.withhold_while_awaiting();
    settle_beat_error(&mut summary, fits.figures.unlock_inconsistent);
    Analysis {
        summary,
        beats,
        residuals: fits.residuals,
        rate_windows: fits.rate_windows,
        amplitude_windows,
        lock_rate,
    }
}

/// What the beats say of whether a watch is heard, and how the sound was
/// taken.
struct Heard {
    /// The median timing scatter of the rate windows, in µs.
    jitter_us: Option<f64>,
    /// Why no watch is heard; none when one is.
    no_watch: Option<NoWatchReason>,
    /// No verdict yet: under 10 s of sound
    /// ([`crate::measure::awaiting_verdict`]). No figures then.
    awaiting: bool,
    /// The sound heard after any lead of noise.
    listened_s: f64,
    /// The beats expected over it.
    expected: f64,
    clipping: Clipping,
    usb_clock: Option<UsbClock>,
}

/// The shape of a take's beats in `env` at `fs`, on its fitted beat period.
fn take_shape(
    env: &[f32],
    fs: f64,
    beats: &[Beat],
    fits: &Fits,
    bph: u32,
) -> crate::shape::Measures {
    let r = beats_shape(env, fs, beats, fits.overall.as_ref(), bph);
    crate::shape::Measures::of(&r, &fits.clear)
}

/// The shape of `beats` in `env` at `fs`, measured on the beat period of
/// their fit `overall` (or 3600/`bph` with none).
fn beats_shape(
    env: &[f32],
    fs: f64,
    beats: &[Beat],
    overall: Option<&TimingFit>,
    bph: u32,
) -> crate::shape::ShapeReport {
    let beat_s = crate::timing::beat_period_s(overall, bph);
    crate::shape::analyze(env, fs, beats, beat_s, &Default::default())
}

/// The shape of the beats [`analyze`] read from `audio` with `cfg` (`a`):
/// on `cfg`'s envelope, on
/// `a`'s beats, whose sides are named as its beat error names them
/// ([`beats::SideCheck`]), and on their fitted period. The one reading
/// behind `analyze`'s shape and `timegrapher shape`, so both give each
/// side the same sounds (#2104). Where `analyze` reads no shape (no
/// regular beat found, or no verdict yet), it is read on no beats: no
/// window and no figure, so nothing is measured on noise (#2371).
pub fn shape_report(
    audio: &Audio,
    cfg: &AnalysisConfig,
    a: &Analysis,
) -> crate::shape::ShapeReport {
    let fs = audio.sample_rate as f64;
    let env = envelope(&audio.samples, fs, &cfg.envelope);
    let s = &a.summary;
    let beats: &[Beat] = if s.shape.is_some() { &a.beats } else { &[] };
    beats_shape(&env, fs, beats, s.overall.as_ref(), s.bph)
}

/// Whether a watch is heard in `beats` at `bph`, listened to from
/// `from_s`, and how the sound was taken.
fn hear(
    audio: &Audio,
    beats: &[Beat],
    bph: u32,
    fits: &Fits,
    from_s: f64,
    beat_sound: Option<bool>,
) -> Heard {
    let fs = audio.sample_rate as f64;
    let listened_s = audio.duration() - from_s;
    let jitter_us = crate::session::scatter_jitter_us(&fits.rate_windows);
    let assessed = crate::measure::regularity_assessed(listened_s, fits.holds_window);
    // Its beats are the whole take's, every run counted.
    let (no_watch, awaiting) = crate::measure::verdict_heard(
        audio.duration(),
        crate::measure::no_watch_reason_of(
            jitter_us,
            bph,
            (assessed, beat_sound),
            (beats, f64::NEG_INFINITY),
        ),
    );
    Heard {
        jitter_us,
        no_watch,
        awaiting,
        listened_s,
        expected: crate::beats::expected_beats(listened_s, bph),
        clipping: Clipping::measure(&audio.samples, fs, beats),
        usb_clock: usbclock::measure(&audio.samples, audio.sample_rate),
    }
}

/// A whole take's assessment, from its `jumps` and the jumps that name
/// Tick and Tock by count (`by_count`), the rate set and the rate guessed,
/// its fits, and whether a watch is heard; with a caveat for a lead of
/// noise left out up to `lead_s`.
fn assess_take(
    cfg: &AnalysisConfig,
    beats: &[Beat],
    (jumps, by_count): (&[f64], usize),
    (bph, guessed): (u32, u32),
    fits: &Fits,
    heard: &Heard,
    lead_s: Option<f64>,
) -> crate::measure::Assessment {
    let context = crate::measure::Context {
        watch_heard: heard.no_watch.is_none(),
        no_watch_reason: heard.no_watch.unwrap_or_default(),
        bph_heard: (bph != guessed).then_some(guessed),
        lift_known: cfg.lift_known,
        clock_known: heard.usb_clock.and_then(UsbClock::applied).is_some(),
        beats: beats.len(),
        expected: heard.expected,
        clipped_fraction: heard.clipping.beat_fraction,
        span_s: beats_span(beats),
        listened_s: heard.listened_s,
        breaks: crate::breaks::events(jumps, beats, bph),
        sides_by_count: by_count,
        beat_error_as_size: fits.turned && fits.overall.is_some(),
        average_s: None,
    };
    let mut assessment = crate::measure::assess(
        &fits.rate_windows,
        &fits.clear.iter().collect::<Vec<_>>(),
        &context,
    );
    if let Some(seconds) = lead_s {
        crate::measure::add(
            &mut assessment.caveats,
            crate::measure::Caveat::LeadLeftOut { seconds },
        );
    }
    if let Some(c) = heard.usb_clock.filter(|c| !c.applies()) {
        crate::measure::add(
            &mut assessment.caveats,
            crate::measure::Caveat::UsbLinesNotUsed { ppm: c.ppm },
        );
    }
    assessment
}

/// The timing and amplitude fits of a take's beats, with its jumps as cuts.
struct Fits {
    /// The amplitude windows clear of the jumps.
    clear: Vec<AmplitudeWindow>,
    /// Rate and beat error over the whole take, fitted piece by piece,
    /// and the clear windows' figures, as every reading of a stretch takes
    /// them ([`crate::session::stretch_fits`]).
    overall: Option<TimingFit>,
    /// Whether the beat error's sign turns between pieces.
    turned: bool,
    figures: crate::session::WindowFigures,
    /// Each beat's timing residual.
    residuals: Vec<f64>,
    /// The rate windows from `from_s` on.
    rate_windows: Vec<WindowFit>,
    /// Whether a whole rate window fits from `from_s` on, clear of the
    /// jumps ([`crate::session::holds_scatter_window`]), so regularity is
    /// judged (#1688).
    holds_window: bool,
}

impl Fits {
    /// Each rate window's rate.
    fn rates(&self) -> Vec<f64> {
        self.rate_windows
            .iter()
            .map(|w| w.fit.rate_s_per_day)
            .collect()
    }
}

/// The fits of `beats` at `bph` from `from_s` to `duration`.
fn fit_take(
    beats: &[Beat],
    amplitude_windows: &[AmplitudeWindow],
    jumps: &[f64],
    bph: u32,
    from_s: f64,
    duration: f64,
    cfg: &AnalysisConfig,
) -> Fits {
    // Where listening stopped and started again the beats jump, and no
    // count of beats across the hole is known: rate and timing are fitted
    // on each stretch between jumps, and no window spans one or takes in a
    // stretch with no beats.
    let jumps = jump_breaks(jumps);
    let holes = crate::clean::Cuts::new(None, &jumps, &[]);
    let cuts = holes.piece_cuts();
    // The readings take only the amplitude windows clear of the jumps, as
    // `session` does: the windows themselves are kept whole.
    let clear: Vec<AmplitudeWindow> = holes
        .amplitude_windows(amplitude_windows)
        .into_iter()
        .cloned()
        .collect();
    let crate::session::StretchFits {
        fit: overall,
        turned,
        figures,
    } = crate::session::stretch_fits(beats, Some(bph), &cuts, &clear.iter().collect::<Vec<_>>());
    let residuals =
        timing::residuals_pieces(beats, &cuts).unwrap_or_else(|| vec![f64::NAN; beats.len()]);
    let part = crate::session::window_part_s(amplitude_windows);
    // Too few windows give no window figure, as in `session` (#1429): the
    // figures' own windows (none before `from_s`, which the lead's
    // `keep_from` left out) against the cells from `from_s` on.
    let cells = crate::session::window_cells(&holes, duration, part, from_s, duration);
    let figures = crate::session::floor_figures(figures, &clear.iter().collect::<Vec<_>>(), cells);
    let rate_windows = holes.windows(
        beats,
        bph,
        from_s,
        duration,
        cfg.rate_window_s,
        cfg.rate_window_s / 2.0,
        part,
    );
    let holds_window = crate::session::holds_scatter_window(
        &holes,
        from_s,
        duration,
        cfg.rate_window_s,
        cfg.rate_window_s / 2.0,
    );
    Fits {
        clear,
        overall,
        turned,
        figures,
        residuals,
        rate_windows,
        holds_window,
    }
}

/// The time from the first beat to the last; 0 with none.
fn beats_span(beats: &[Beat]) -> f64 {
    match (beats.first(), beats.last()) {
        (Some(a), Some(b)) => b.time - a.time,
        _ => 0.0,
    }
}

/// The take's beats and amplitude windows at `bph`, with any lead of noise
/// before the watch left out, and where the watch starts if there was one.
fn read_take(
    audio: &Audio,
    cfg: &AnalysisConfig,
    bph: Option<u32>,
) -> (crate::stream::BeatLog, Option<f64>) {
    // The beats and the amplitude windows come from the chunked stream,
    // the one beat pipeline every reading of a recording rests on (session,
    // long, series and regress read files through it).
    let mut log = crate::stream::analyze_samples(
        &audio.samples,
        audio.sample_rate,
        &crate::stream::StreamConfig {
            analysis: AnalysisConfig { bph, ..cfg.clone() },
            ..Default::default()
        },
    );
    // A lead of noise before the watch is left out (#844).
    let lead_s = lead_of(&log, audio.duration());
    if let Some(t) = lead_s {
        log.keep_from(t);
    }
    (log, lead_s)
}

/// Each jump in the beats as a break of no width.
fn jump_breaks(jumps: &[f64]) -> Vec<crate::breaks::Break> {
    jumps
        .iter()
        .map(|&audio_s| crate::breaks::Break {
            audio_s,
            gap_s: 0.0,
        })
        .collect()
}

/// The periods found in the timing residuals and in the amplitude, in 1 s
/// bins.
fn periods(
    beats: &[Beat],
    residuals: &[f64],
    clear: &[AmplitudeWindow],
    bph: u32,
    cfg: &AnalysisConfig,
) -> (Vec<Component>, Vec<Component>) {
    // Periodicity: timing residuals and amplitude, in 1 s bins.
    let wheels = periodicity::standard_wheels(bph, cfg.escape_teeth);
    let good: Vec<(f64, f64)> = beats
        .iter()
        .zip(residuals)
        .filter(|(b, r)| b.is_good() && r.is_finite())
        .map(|(b, &r)| (b.time, r))
        .collect();
    let search = |s: Series| -> Vec<Component> {
        if s.t.len() < 30 {
            return Vec::new();
        }
        let s = periodicity::detrend(&s);
        let span = s.t[s.t.len() - 1] - s.t[0];
        let periods = periodicity::log_periods(4.0, (span / 3.0).max(8.0), 800);
        let power = periodicity::lomb_scargle(&s, &periods);
        periodicity::peaks(&periods, &power, &wheels, 5)
    };
    let timing_series = periodicity::bin_median(
        &good.iter().map(|g| g.0).collect::<Vec<_>>(),
        &good.iter().map(|g| g.1).collect::<Vec<_>>(),
        1.0,
    );
    let amp_series = Series {
        t: clear
            .iter()
            .filter(|w| w.mean().is_some())
            .map(|w| (w.start_s + w.end_s) / 2.0)
            .collect(),
        y: clear.iter().filter_map(|w| w.mean()).collect(),
    };
    (search(timing_series), search(amp_series))
}

/// Withholds the readings where no watch is heard, and chooses the beat
/// error shown, with its caveat: why it is from the drop where it is, the
/// unlock found `inconsistent` or not found (#1439); never the drop on a
/// co-axial, whose amplitude carries its caveat (#2313).
fn settle_beat_error(summary: &mut Summary, inconsistent: bool) {
    if !summary.watch_heard {
        summary.withhold_readings();
    }
    let escapement = summary.escapement;
    summary.beat_error_source = crate::session::chosen_beat_error(
        summary.beat_error_unlock_ms,
        escapement.drop_beat_error(summary.overall.map(|f| f.beat_error_ms)),
    )
    .map(|(_, source)| source);
    if let Some(caveat) =
        escapement.beat_error_caveat(summary.beat_error_source, inconsistent, summary.watch_heard)
    {
        crate::measure::add(&mut summary.caveats, caveat);
    }
    if let Some(caveat) = escapement.amplitude_caveat(summary.amplitude_deg.is_some()) {
        crate::measure::add(&mut summary.caveats, caveat);
    }
}

/// The rate of the lock a beat log on Auto was read at
/// ([`crate::timing::lock_rate`]): over its first
/// [`beats::GUESS_SPAN_S`] of beats after any lead of noise ([`lead_of`]),
/// as [`analyze`] judges its read. A take's reading judges its lock on it.
pub fn lock_rate_of(log: &crate::stream::BeatLog) -> Option<f64> {
    let from = lead_of(log, log.duration_s).unwrap_or(f64::NEG_INFINITY);
    let i = log.beats.partition_point(|b| b.time < from);
    crate::timing::lock_rate(&log.beats[i..], log.bph, &log.jumps)
}

/// Whether the lock a beat log `log` was read at is judged off every
/// standard rate (Norm on #2223 and #2316): on Auto (`bph_set` none),
/// its rate over its first [`beats::GUESS_SPAN_S`] ([`lock_rate_of`]) out
/// of reach of every standard rate ([`crate::timing::OffStandard::of`]).
/// The one verdict for every reading of a take's beats: a saved take's,
/// and the app's whole take, stopped or opened, and its positions
/// (#2253, #2327). Such a reading is then read with its
/// `read_off_standard`.
pub fn lock_off_standard(bph_set: Option<u32>, log: &crate::stream::BeatLog) -> bool {
    bph_set.is_none() && crate::timing::OffStandard::of(None, log.bph, lock_rate_of(log)).is_some()
}

/// Where the watch starts in `log` after a lead of noise, judged as a
/// take's reading judges it ([`crate::session::lead_s`]), with the beats'
/// jumps as its breaks.
fn lead_of(log: &crate::stream::BeatLog, duration: f64) -> Option<f64> {
    let jumps = jump_breaks(&log.jumps);
    crate::session::lead_s(log, None, &jumps, &[], 0.0, duration)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{generate, SynthConfig};

    /// One rule names the `clipped` caveat: strictly more than
    /// `NOTED_FRACTION`, from a stored fraction or a [`Clipping`].
    #[test]
    fn clipping_noted_above_the_fraction_only() {
        assert!(!Clipping::noted_fraction(0.0));
        assert!(!Clipping::noted_fraction(Clipping::NOTED_FRACTION));
        assert!(Clipping::noted_fraction(Clipping::NOTED_FRACTION + 1e-9));
        for f in [0.0, 0.01, 0.0101, 0.3] {
            let c = Clipping {
                samples: 0,
                beats: 0,
                beat_fraction: f,
                peak_dbfs: 0.0,
            };
            assert_eq!(c.noted(), Clipping::noted_fraction(f), "{f}");
        }
    }

    /// Clipping in figures (#1417): the clipped beats as a share of the
    /// beats found, none of none, and the peak in dB of full scale, a half
    /// scale peak at −6.02 dBFS and digital silence floored at −200.
    #[test]
    fn clipping_is_a_share_of_the_beats_and_a_peak_in_dbfs() {
        let c = Clipping::of(40, 3, 12, 0.5);
        assert_eq!((c.samples, c.beats), (40, 3));
        assert!((c.beat_fraction - 0.25).abs() < 1e-12, "{c:?}");
        assert!((c.peak_dbfs + 6.0206).abs() < 1e-4, "{c:?}");
        assert_eq!(Clipping::of(0, 0, 0, 1.0).beat_fraction, 0.0);
        assert!(Clipping::of(0, 0, 0, 1.0).peak_dbfs.abs() < 1e-12);
        assert!((Clipping::of(0, 0, 8, 0.0).peak_dbfs + 200.0).abs() < 1e-9);
        // Three beats at 1, 2 and 3 s, one clipped run at the second.
        let fs = 8000.0;
        let mut x = vec![0.1f32; 4 * 8000];
        for v in &mut x[16_000..16_004] {
            *v = 1.0;
        }
        let beats: Vec<Beat> = (1..=3)
            .map(|i| Beat {
                index: i,
                time: i as f64,
                quality: 1.0,
            })
            .collect();
        let m = Clipping::measure(&x, fs, &beats);
        assert_eq!(m.beats, 1, "{m:?}");
        assert!((m.beat_fraction - 1.0 / 3.0).abs() < 1e-12, "{m:?}");
        assert!(m.peak_dbfs.abs() < 1e-9, "{m:?}");
    }

    #[test]
    fn analyze_reads_the_same_amplitude_windows_as_session() {
        // 90 s at 28,800 bph with 6 ms of silence every 11.1 s: listening
        // stopped and started, so the beats jump there. A window across a
        // jump reads a beat error far off (−0.65 ms where the watch has
        // −0.40), and neither reading takes one.
        let a = generate(
            &SynthConfig {
                duration_s: 90.0,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        );
        let fs = a.sample_rate as f64;
        let mut samples = Vec::new();
        for (k, c) in a.samples.chunks((fs * 11.1).round() as usize).enumerate() {
            if k > 0 {
                samples.extend(vec![0.0; (fs * 0.006) as usize]);
            }
            samples.extend_from_slice(c);
        }
        let audio = Audio {
            sample_rate: a.sample_rate,
            samples,
        };
        let cfg = AnalysisConfig {
            bph: Some(28800),
            ..Default::default()
        };
        let summary = analyze(&audio, &cfg).summary;
        let log = crate::stream::analyze_samples(
            &audio.samples,
            audio.sample_rate,
            &crate::stream::StreamConfig {
                analysis: cfg.clone(),
                ..Default::default()
            },
        );
        assert!(log.jumps.len() >= 7, "{:?}", log.jumps);
        let breaks: Vec<crate::breaks::Break> = log
            .jumps
            .iter()
            .map(|&audio_s| crate::breaks::Break {
                audio_s,
                gap_s: 0.0,
            })
            .collect();
        let m = crate::session::measure_with_breaks(&log, None, &breaks, 0.0, log.duration_s);
        assert_eq!(summary.amplitude_deg, m.amplitude_deg);
        assert_eq!(summary.beat_error_unlock_ms, m.beat_error_unlock_ms);
    }
    /// A take whose unlock reads −0.9 ms in three windows of five and the
    /// watch's +0.6 ms in the rest (#1439): `analyze`, as session does,
    /// gives the drop's beat error and says the unlock was inconsistent,
    /// not that it was not found (Trevor's mutant on #1458).
    #[test]
    fn analyze_says_an_inconsistent_unlock_falls_back_to_the_drop() {
        use crate::measure::{Caveat, DropWhy};
        use crate::session::BeatErrorSource;
        let take = |even_unlock_lead_s: f64| {
            let cfg = SynthConfig {
                duration_s: 90.0,
                beat_error_ms: 0.6,
                even_unlock_lead_s,
                ..Default::default()
            };
            generate(&cfg, |_| 270.0, |_| 0.0)
        };
        let (far, near) = (take(0.0015), take(0.0));
        let fs = far.sample_rate as f64;
        // Spliced between beats, 6 s of every 10 from the far take.
        let samples = (0..far.samples.len())
            .map(|i| match (i as f64 / fs - 0.1125).rem_euclid(10.0) < 6.0 {
                true => far.samples[i],
                false => near.samples[i],
            })
            .collect();
        let audio = Audio {
            sample_rate: far.sample_rate,
            samples,
        };
        let cfg = AnalysisConfig {
            bph: Some(28800),
            ..Default::default()
        };
        let s = analyze(&audio, &cfg).summary;
        assert_eq!(s.beat_error_unlock_ms, None, "{s:?}");
        assert_eq!(s.beat_error_source, Some(BeatErrorSource::Drop));
        assert!((s.overall.unwrap().beat_error_ms - 0.6).abs() < 0.05);
        assert!(s.caveats.contains(&Caveat::BeatErrorFromDrop {
            why: DropWhy::UnlockInconsistent
        }));
        assert!(!s.caveats.contains(&Caveat::FROM_DROP));
    }

    /// The timing's periods are searched on the good beats alone: moving
    /// the faint beats of a quiet stretch (−20 dB, 12 s) by 10 ms leaves
    /// them as they are, and the watch's 10 s swing in rate is found.
    #[test]
    fn the_timing_periods_are_searched_on_good_beats_alone() {
        let a = generate(
            &SynthConfig {
                duration_s: 60.0,
                ..Default::default()
            },
            |_| 270.0,
            |t| 30.0 * (std::f64::consts::TAU * t / 10.0).sin(),
        );
        let fs = a.sample_rate as f64;
        let periods = |shift: usize| {
            let mut b = a.clone();
            for (i, v) in b.samples.iter_mut().enumerate() {
                if (20.0..32.0).contains(&(i as f64 / fs)) {
                    *v = 0.1 * a.samples[i - shift];
                }
            }
            let r = analyze(&b, &AnalysisConfig::default());
            assert!(r.beats.iter().any(|b| !b.is_good()));
            r.summary
                .timing_periods
                .iter()
                .map(|c| c.period_s)
                .collect::<Vec<_>>()
        };
        let still = periods(0);
        assert!((still[0] - 10.0).abs() < 0.1, "{still:?}");
        assert_eq!(periods((0.01 * fs) as usize), still);
    }

    /// The headline beat error reads back the source chosen: from the
    /// drop, the whole-recording fit's, never the unlock's figure.
    #[test]
    fn the_headline_beat_error_reads_back_the_source_chosen() {
        use crate::session::BeatErrorSource;
        let a = generate(
            &SynthConfig {
                duration_s: 30.0,
                beat_error_ms: 0.6,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        );
        let mut s = analyze(&a, &AnalysisConfig::default()).summary;
        let drop = s.overall.unwrap().beat_error_ms;
        let unlock = s.beat_error_unlock_ms.unwrap();
        assert_eq!(
            s.headline_beat_error(),
            Some((unlock, BeatErrorSource::Unlock))
        );
        s.beat_error_source = Some(BeatErrorSource::Drop);
        s.beat_error_unlock_ms = Some(9.0);
        assert_eq!(s.headline_beat_error(), Some((drop, BeatErrorSource::Drop)));
        s.beat_error_source = None;
        assert_eq!(s.headline_beat_error(), None);
    }
}
