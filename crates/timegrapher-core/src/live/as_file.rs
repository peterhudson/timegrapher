//! Until live builds a file's first-chunk template again (#1327), its
//! own beats are timed on the template it locked on with, so its figures
//! would part from a saved take's of the same sound (#1987). Until then,
//! everything heard is read again as a file reads it
//! ([`crate::stream::analyze_samples`]), back to back on one background
//! thread, and the figures come only from the newest read finished: a
//! reading states the end of the sound that read covered, and equals the
//! saved take of that length. Live's own beats still draw the strip.
//! A stop reads everything heard once more, so the stopped figures are
//! the saved take's (Norm's ruling on #1987).
//! Once the first chunk and its margin are heard, each read resumes from
//! the state the last read left after that chunk and reads only the sound
//! after it ([`crate::stream::analyze_samples_resumed`]): the same log to
//! the bit as a read from the start (#2261).

use super::*;
use crate::stream::Resume;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// A read as a file: its log, and the state to resume the next read from
/// ([`crate::stream::analyze_samples_resumed`]).
type Read = (BeatLog, Option<Arc<Resume>>);

/// How much sound, seconds, is read again as a file at most: twice a
/// file's first chunk. Past it the figures are live's own even if the
/// template was never built again (the watch first heard late, or never),
/// so a read never grows past what the lag allows.
pub const AS_FILE_MAX_S: f64 = 2.0 * crate::stream::CHUNK_S;

/// How far, seconds, the figures may fall behind the sound heard, while
/// it is pushed faster than real time by [`LiveAnalyzer::push`] (a test, a
/// headless read): the newest read's end against all heard. Past it the
/// figures, once asked for, are read of everything heard there and then,
/// so such a feed never waits on reads it doesn't use and the figures it
/// is given are never further behind. Heard as it comes, nothing waits,
/// so the strip never trails the watch; the figures then trail it by two
/// reads at most, about 1.2 s for the most heard before the template is
/// built again. The app's feed never waits (Norm's ruling on #1987).
pub const READ_LAG_CAP_S: f64 = 0.5;

/// The sound read again as a file, the read in flight, and the newest
/// finished, ready to give the figures.
#[derive(Default)]
pub(super) struct AsFile {
    /// Off on a copy that isn't the reading shown (the template's reread,
    /// a reading held from before a rate change), and once the figures
    /// are live's own for good.
    on: bool,
    /// Everything heard since the start.
    heard: Vec<f32>,
    /// The read in flight, and how many samples of `heard` it reads.
    job: Option<(JoinHandle<Read>, usize)>,
    /// The state the newest read left after the first chunk, the next
    /// read resumed from it (#2261): dropped on any setting changed, and
    /// used by a read only for the same first chunk read the same way.
    resume: Mutex<Option<Arc<Resume>>>,
    /// The newest finished read, set as live is.
    view: Option<Box<LiveAnalyzer>>,
    /// Fed by [`LiveAnalyzer::push`] (a test, a headless read): no read
    /// runs in the background, and the figures are read when they are
    /// asked for, of everything heard ([`Self::fresh`]).
    on_ask: bool,
    /// Everything heard when the figures were last asked for, read then,
    /// for a feed read [`Self::on_ask`]: kept while no more than
    /// [`READ_LAG_CAP_S`] more is heard and no setting changes.
    fresh: std::sync::OnceLock<Box<LiveAnalyzer>>,
    /// Reads of everything heard made on the caller's thread, for the
    /// tests that no read is made twice.
    #[cfg(test)]
    reads: std::sync::atomic::AtomicUsize,
}

impl AsFile {
    /// The state the next read resumes from, if any.
    fn resume_from(&self) -> Option<Arc<Resume>> {
        self.resume.lock().ok().and_then(|r| r.clone())
    }

    /// `resume` kept for the next read.
    fn keep_resume(&self, resume: Option<Arc<Resume>>) {
        if let Ok(mut r) = self.resume.lock() {
            *r = resume;
        }
    }
}

/// A copy keeps the newest read and what was heard; a read in flight
/// stays with the original.
impl Clone for AsFile {
    fn clone(&self) -> Self {
        AsFile {
            on: self.on,
            heard: self.heard.clone(),
            job: None,
            resume: Mutex::new(self.resume_from()),
            view: self.view.clone(),
            on_ask: self.on_ask,
            fresh: self.fresh.clone(),
            #[cfg(test)]
            reads: Default::default(),
        }
    }
}

impl std::fmt::Debug for AsFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsFile")
            .field("on", &self.on)
            .field("heard", &self.heard.len())
            .field("reading", &self.job.as_ref().map(|j| j.1))
            .field("view_s", &self.view.as_ref().map(|v| v.settled_to))
            .finish()
    }
}

impl LiveAnalyzer {
    /// This analyzer with its figures read as a file until the template
    /// is built again, as a screen showing a watch heard live needs them
    /// (#1987). Without it every figure is live's own, as a reading that
    /// must not hang on a background thread's timing (a test, a
    /// headless print) needs.
    pub fn with_figures_as_file(mut self) -> Self {
        self.as_file = AsFile {
            on: true,
            ..Default::default()
        };
        self
    }

    /// The figures' analyzer: the newest read as a file while those give
    /// them, else this one.
    pub(super) fn figures(&self) -> Option<&LiveAnalyzer> {
        self.as_file.on.then(|| self.newest_read()).flatten()
    }

    /// The newest read. For a feed read when asked ([`AsFile::on_ask`]),
    /// everything heard, read now when the newest read is more than
    /// [`READ_LAG_CAP_S`] behind it.
    fn newest_read(&self) -> Option<&LiveAnalyzer> {
        if !self.as_file.on_ask {
            return self.as_file.view.as_deref();
        }
        if let Some(f) = self.as_file.fresh.get() {
            return Some(f);
        }
        let behind = self.as_file.heard.len() as f64 - self.view_len() as f64;
        if behind <= READ_LAG_CAP_S * self.fs {
            return self.as_file.view.as_deref();
        }
        Some((self.as_file.fresh).get_or_init(|| Box::new(self.view_of(self.read_heard()))))
    }

    /// Everything heard, read as a file now, on this thread, resumed
    /// after the first chunk where the last read left it.
    fn read_heard(&self) -> BeatLog {
        #[cfg(test)]
        (self.as_file.reads).fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (log, resume) = crate::stream::analyze_samples_resumed(
            &self.as_file.heard,
            self.fs as u32,
            &self.stream_config(),
            self.as_file.resume_from(),
        );
        self.as_file.keep_resume(resume);
        log
    }

    /// Whether the figures still come from reads as a file: until the
    /// template is built again from the first chunk, or that is given up,
    /// and within [`AS_FILE_MAX_S`].
    pub fn reads_as_file(&self) -> bool {
        self.as_file.on
    }

    /// The end of the sound the newest read as a file covered, seconds,
    /// while the figures come from those reads.
    pub fn read_as_file_to(&self) -> Option<f64> {
        self.figures().map(|v| v.settled_to)
    }

    /// The reading over the `average_s` seconds before `end_s`, while the
    /// figures come from reads as a file: live's listening state (the
    /// watch heard or not, the beat being found, the sound's level and
    /// clipping) at `end_s`, with the newest read's figures to `read_end_s`
    /// or the end of the sound it covered if sooner, where live would show
    /// figures at all; finding the beat until a read has them.
    pub(super) fn reading_as_file(
        &self,
        end_s: f64,
        read_end_s: f64,
        average_s: f64,
    ) -> Option<LiveReading> {
        if !self.as_file.on {
            return None;
        }
        let own = self.own_reading_at(end_s, average_s);
        // Live shows no figures yet: none from the read either.
        if !own.watch_heard || own.finding_beat {
            return Some(own);
        }
        let read =
            (self.newest_read()).map(|v| v.reading_at(read_end_s.min(v.settled_to), average_s));
        let waits = own.listening.is_some();
        // Until a read has figures of this watch as live hears it (one has
        // landed, past its own first seconds, at live's beat rate), none:
        // the beat is still being found, never live's own figures nor no
        // beats (Norm on #2133). Whether a verdict waits for 10 s heard is
        // the read's too, so its end, beats, span and caveats are the saved
        // take's (#2333, #2149); live's own until a read lands.
        Some(match read {
            Some(r) if r.bph == own.bph && (r.beats_used > 0 || r.listening.is_some()) => {
                with_read_figures(own, r)
            }
            _ if waits => own,
            _ => without_figures(own),
        })
    }

    /// The trend while the figures come from reads as a file: the newest
    /// read's.
    pub(super) fn trend_as_file(&self, average_s: f64) -> Option<Vec<TrendReading>> {
        if !self.as_file.on {
            return None;
        }
        Some(match self.newest_read() {
            Some(v) => v.trend(average_s),
            None => Vec::new(),
        })
    }

    /// No more reads as a file: the figures are this analyzer's own.
    pub(super) fn stop_reading_as_file(&mut self) {
        self.as_file = AsFile::default();
    }

    /// Whether a read as a file is in flight, and whether one has
    /// landed, for the tests.
    #[cfg(test)]
    pub(super) fn read_state(&self) -> (bool, bool) {
        (self.as_file.job.is_some(), self.as_file.view.is_some())
    }

    /// Whether the read in flight has finished, for the tests.
    #[cfg(test)]
    pub(super) fn read_finished(&self) -> bool {
        self.as_file.job.as_ref().is_none_or(|j| j.0.is_finished())
    }

    /// A setting changed: the newest read as a file given it too.
    pub(super) fn in_view(&mut self, f: impl FnOnce(&mut LiveAnalyzer)) {
        self.as_file.fresh = std::sync::OnceLock::new();
        self.as_file.keep_resume(None);
        if let Some(v) = &mut self.as_file.view {
            f(v);
        }
    }

    /// After a push of `samples`: keeps them, takes a finished read's
    /// figures and starts the next read on everything heard, in the
    /// background. A feed that `wait`s ([`LiveAnalyzer::push`]) starts
    /// none: its figures are read when they are asked for
    /// ([`Self::newest_read`]), so it never waits on a read it doesn't use,
    /// until a push that doesn't wait comes.
    /// True when the figures changed.
    pub(super) fn as_file_after_push(&mut self, samples: &[f32], wait: bool) -> bool {
        if !self.as_file.on {
            return false;
        }
        if self.template_settled() || self.duration_s() > AS_FILE_MAX_S {
            self.stop_reading_as_file();
            return true;
        }
        self.as_file.heard.extend_from_slice(samples);
        if wait {
            self.as_file.on_ask = true;
            let heard = self.as_file.heard.len() as f64;
            if (self.as_file.fresh.get())
                .is_some_and(|f| heard - f.total as f64 > READ_LAG_CAP_S * self.fs)
            {
                self.as_file.fresh = std::sync::OnceLock::new();
            }
            return false;
        }
        // The newest push's feed decides: one that doesn't wait reads in
        // the background from here on.
        self.as_file.on_ask = false;
        self.as_file.fresh = std::sync::OnceLock::new();
        let mut changed = false;
        if self.as_file.job.as_ref().is_some_and(|j| j.0.is_finished()) {
            changed |= self.take_read();
        }
        if self.as_file.job.is_none() && self.as_file.heard.len() > self.view_len() {
            self.start_read();
        }
        changed
    }

    /// Samples the newest read covered.
    fn view_len(&self) -> usize {
        self.as_file.view.as_ref().map_or(0, |v| v.total as usize)
    }

    fn start_read(&mut self) {
        let sound = self.as_file.heard.clone();
        let n = sound.len();
        let fs = self.fs as u32;
        let cfg = self.stream_config();
        let from = self.as_file.resume_from();
        let job = std::thread::spawn(move || {
            crate::stream::analyze_samples_resumed(&sound, fs, &cfg, from)
        });
        self.as_file.job = Some((job, n));
    }

    /// How a saved take of this sound is read.
    fn stream_config(&self) -> crate::stream::StreamConfig {
        crate::stream::StreamConfig {
            analysis: self.cfg.analysis.clone(),
            ..Default::default()
        }
    }

    /// The read in flight, waited for, as the figures. True when it gave
    /// a read.
    fn take_read(&mut self) -> bool {
        let Some((job, _)) = self.as_file.job.take() else {
            return false;
        };
        match job.join() {
            Ok(read) => {
                self.set_read(read);
                true
            }
            Err(_) => false,
        }
    }

    /// A read from the background as the figures, and its state to resume
    /// the next from.
    fn set_read(&mut self, (log, resume): Read) {
        self.as_file.keep_resume(resume);
        self.set_view(log);
    }

    /// `log` as the figures' analyzer, set as live is: its clock, masks,
    /// known flags, profile span and the caller's breaks.
    fn set_view(&mut self, log: BeatLog) {
        self.as_file.view = Some(Box::new(self.view_of(log)));
        self.as_file.fresh = std::sync::OnceLock::new();
    }

    /// `log` as the figures' analyzer, set as live is.
    fn view_of(&self, mut log: BeatLog) -> LiveAnalyzer {
        // On Auto, a watch heard: the read takes the nearest standard rate
        // as live's lock does ([`LiveAnalyzer::take_nearest_standard`]).
        if self.found_bph && self.cfg.analysis.bph.is_none() {
            log.take_nearest_standard();
        }
        let mut v = LiveAnalyzer::from_log(log, self.cfg.clone());
        v.rate_clock = self.rate_clock.clone();
        v.clock_known = self.clock_known;
        v.masked = self.masked.clone();
        let caller: Vec<f64> = (self.breaks.iter().copied())
            .filter(|b| !self.intrinsic_breaks.contains(b))
            .collect();
        if !caller.is_empty() {
            v.set_breaks(caller);
        }
        v
    }

    /// Reads everything heard as a file now, waiting for it, so the
    /// figures are the saved take's of all of it: for a stop, before the
    /// stopped figures show. Nothing once the figures are live's own.
    pub fn read_as_file_now(&mut self) {
        if !self.as_file.on {
            return;
        }
        if self.view_len() == self.as_file.heard.len() {
            return;
        }
        // The figures last asked for were read of everything heard, with
        // every setting since (a change drops them): that read is the one
        // wanted, not read twice.
        if let Some(f) = self.as_file.fresh.take() {
            if f.total as usize == self.as_file.heard.len() {
                self.as_file.view = Some(f);
                return;
            }
        }
        // A read in flight of everything heard is the one wanted: waited
        // for, not read twice. One of less is left to finish on its own.
        if let Some((job, n)) = self.as_file.job.take() {
            if n == self.as_file.heard.len() {
                if let Ok(read) = job.join() {
                    self.set_read(read);
                    return;
                }
            }
        }
        let log = self.read_heard();
        self.set_view(log);
    }
}

/// Live's listening state, `own`, with the read's figures, `read`: its
/// rate, beat error and amplitude with their ±, scatter, the beats and span
/// behind them, its time, whether it waits for 10 s heard (#2333), and its
/// caveats on those figures. The figures
/// that name a side are the read's too, its sides as it tells them, as an
/// opened take's are: never held back on live's sides (Norm on #2133).
fn with_read_figures(own: LiveReading, read: LiveReading) -> LiveReading {
    let mut caveats: Vec<Caveat> = (read.caveats.into_iter())
        .filter(|c| !listening_caveat(c))
        .collect();
    for c in own.caveats.into_iter().filter(listening_caveat) {
        measure::add(&mut caveats, c);
    }
    LiveReading {
        bph: own.bph,
        bph_set: own.bph_set,
        snr: own.snr,
        clipped_beats: own.clipped_beats,
        clipped_fraction: own.clipped_fraction,
        watch_heard: own.watch_heard,
        finding_beat: own.finding_beat,
        caveats,
        ..read
    }
}

/// Live's listening state with no figures, while no read has figures of
/// the watch live hears: the beat is still being found, as when live
/// itself first hears a regular run, its beats heard still counted
/// (Norm on #2133).
pub(super) fn without_figures(own: LiveReading) -> LiveReading {
    LiveReading {
        rate_s_per_day: None,
        beat_error_ms: None,
        beat_error_unlock_ms: None,
        beat_error_source: None,
        amplitude_deg: None,
        amplitude_error_deg: None,
        amplitude_no_pm: None,
        amplitude_even_deg: None,
        amplitude_odd_deg: None,
        jitter_us: None,
        timing_scatter_fraction: None,
        whole: false,
        rate_error_s_per_day: None,
        rate_no_pm: None,
        rate_noise_values: 0,
        rate_own_values: 0,
        amplitude_values: 0,
        amplitude_noise_values: 0,
        beat_error_unlock_error_ms: None,
        caveats: own.caveats.into_iter().filter(listening_caveat).collect(),
        whole_take: false,
        watch_heard: false,
        finding_beat: true,
        ..own
    }
}

/// A caveat on what is heard, not on the figures: no watch, a set rate
/// the sound disagrees with, clipping.
fn listening_caveat(c: &Caveat) -> bool {
    matches!(
        c,
        Caveat::NoWatch { .. } | Caveat::SetRateDisagrees { .. } | Caveat::Clipped { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{generate, SynthConfig};
    use std::sync::Arc;

    fn watch(bph: u32) -> Vec<f32> {
        let cfg = SynthConfig {
            duration_s: 20.0,
            bph,
            ..Default::default()
        };
        generate(&cfg, |_| 280.0, |_| 0.0).samples
    }

    /// The read's beat error and Tick and Tock amplitudes stay on show
    /// where live has lost its sides, as after a jump it hasn't yet
    /// checked by sound: they are the read's, the saved take's, as an
    /// opened take shows them (Norm on Walter 17's hold of #2133).
    #[test]
    fn the_reads_sides_stand_where_live_lost_its_own() {
        let mut live = LiveAnalyzer::new(48_000, LiveConfig::default()).with_figures_as_file();
        for c in watch(28_800).chunks(960) {
            live.push(c);
        }
        live.read_as_file_now();
        let before = live.reading(10.0);
        assert!(before.beat_error_ms.is_some(), "{before:?}");
        let end = live.settled_to;
        live.jump_at(end - 1.0, true);
        assert!(!live.sides_known_at(end), "live lost its sides");
        let r = live.reading(10.0);
        assert!(r.rate_s_per_day.is_some(), "{r:?}");
        for (name, v) in [
            ("beat error", r.beat_error_ms),
            ("from the unlock", r.beat_error_unlock_ms),
            ("Tick amplitude", r.amplitude_even_deg),
            ("Tock amplitude", r.amplitude_odd_deg),
        ] {
            assert!(v.is_some(), "{name}: {r:?}");
        }
    }

    /// `sound` fed by `push` (read when asked) and by
    /// `push_without_waiting` then read whole once more as a stop does
    /// (the background's read, [`LiveAnalyzer::read_as_file_now`]).
    fn fed_both(sound: &[f32]) -> (LiveAnalyzer, LiveAnalyzer) {
        let mut asked = LiveAnalyzer::new(48_000, LiveConfig::default()).with_figures_as_file();
        let mut whole = asked.clone();
        for c in sound.chunks(960) {
            asked.push(c);
            whole.push_without_waiting(c);
        }
        whole.read_as_file_now();
        (asked, whole)
    }

    /// A push fed figures asked for past the cap reads everything heard
    /// there and then, with the background's own read: the figures are a
    /// full read of all heard, to the bit (Norm on #2133).
    #[test]
    fn an_ask_past_the_cap_reads_everything_heard() {
        let sound = watch(28_800);
        let (asked, whole) = fed_both(&sound);
        let got = asked.reading(10.0);
        assert!(got.rate_s_per_day.is_some(), "{got:?}");
        assert_eq!(format!("{got:?}"), format!("{:?}", whole.reading(10.0)));
        assert_eq!(asked.read_as_file_to(), Some(asked.duration_s()));
        // As a full read of all heard, set as live is.
        let log = crate::stream::analyze_samples(&sound, 48_000, &asked.stream_config());
        let full = asked.view_of(log);
        let r = full.reading_at(full.settled_to, 10.0);
        assert_eq!(
            got.rate_s_per_day.map(f64::to_bits),
            r.rate_s_per_day.map(f64::to_bits)
        );
        assert_eq!(
            got.amplitude_deg.map(f64::to_bits),
            r.amplitude_deg.map(f64::to_bits)
        );
    }

    /// A stop after the figures were asked for of everything heard keeps
    /// that read, the one a stop would make: not read twice, and the
    /// figures the same to the bit.
    #[test]
    fn a_stop_after_an_ask_of_all_heard_reads_it_once() {
        let (mut asked, whole) = fed_both(&watch(28_800));
        let got = asked.reading(10.0);
        assert!(got.rate_s_per_day.is_some(), "{got:?}");
        assert_eq!(asked.read_as_file_to(), Some(asked.duration_s()));
        let reads = |l: &LiveAnalyzer| l.as_file.reads.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(reads(&asked), 1);
        asked.read_as_file_now();
        assert_eq!(reads(&asked), 1, "the stop read nothing again");
        // As the stop of a feed read in the background, which reads it all.
        assert_eq!(format!("{:?}", asked.reading(10.0)), format!("{got:?}"));
        assert_eq!(reads(&asked), 1, "the ask after the stop read nothing");
        assert_eq!(format!("{got:?}"), format!("{:?}", whole.reading(10.0)));
    }

    /// A stop after an ask that left out sound heard since (less than the
    /// cap) reads everything heard: the figures reach the end of the
    /// sound, as a background-fed take's after its stop, to the bit.
    #[test]
    fn a_stop_after_an_ask_short_of_all_heard_reads_it_all() {
        let sound = watch(28_800);
        let (mut asked, mut whole) = fed_both(&sound);
        let _ = asked.reading(10.0);
        let more = &watch(28_800)[..4_800];
        asked.push(more);
        whole.push_without_waiting(more);
        assert!(asked.read_as_file_to() < Some(asked.duration_s()), "kept");
        asked.read_as_file_now();
        whole.read_as_file_now();
        assert_eq!(asked.read_as_file_to(), Some(asked.duration_s()));
        let got = asked.reading(10.0);
        assert!(got.rate_s_per_day.is_some(), "{got:?}");
        assert_eq!(format!("{got:?}"), format!("{:?}", whole.reading(10.0)));
    }

    /// Asks past the first chunk resume the reads from where the last
    /// read left it (#2261), and the figures are still a full read's of
    /// everything heard, to the bit; a setting changed drops the resume.
    #[test]
    fn asks_past_the_first_chunk_resume_the_read() {
        let cfg = SynthConfig {
            duration_s: 60.0,
            bph: 28_800,
            ..Default::default()
        };
        let sound = generate(&cfg, |_| 280.0, |_| 0.0).samples;
        let mut asked = LiveAnalyzer::new(48_000, LiveConfig::default()).with_figures_as_file();
        let (mut fed, mut firsts) = (0, Vec::new());
        for at_s in [49.0, 55.0, 60.0] {
            let to = (at_s as usize * 48_000).min(sound.len());
            for c in sound[fed..to].chunks(960) {
                asked.push(c);
            }
            fed = to;
            if at_s == 55.0 {
                asked.set_lift(50.0);
                assert!(asked.as_file.resume_from().is_none(), "dropped");
            }
            let got = asked.reading(10.0);
            assert!(
                asked.reads_as_file() && got.rate_s_per_day.is_some(),
                "{got:?}"
            );
            let log = crate::stream::analyze_samples(&sound[..to], 48_000, &asked.stream_config());
            let full = asked.view_of(log);
            assert_eq!(asked.read_as_file_to(), Some(full.settled_to));
            let r = full.reading_at(full.settled_to, 10.0);
            for (name, a, b) in [
                ("rate", got.rate_s_per_day, r.rate_s_per_day),
                ("amplitude", got.amplitude_deg, r.amplitude_deg),
                ("beat error", got.beat_error_ms, r.beat_error_ms),
            ] {
                assert_eq!(a.map(f64::to_bits), b.map(f64::to_bits), "{at_s} s: {name}");
            }
            firsts.push(asked.as_file.resume_from().expect("kept past 48 s"));
        }
        assert!(
            !Arc::ptr_eq(&firsts[0], &firsts[1]),
            "read again on the new lift"
        );
        assert!(Arc::ptr_eq(&firsts[1], &firsts[2]), "resumed");
    }

    /// A take fed in the background past the first chunk: each read in
    /// the background keeps its resume, the next resumes from it, and the
    /// figures are a full read's of everything heard, to the bit (Vernon
    /// on #2265).
    #[test]
    fn background_reads_past_the_first_chunk_resume() {
        let cfg = SynthConfig {
            duration_s: 56.0,
            bph: 28_800,
            ..Default::default()
        };
        let sound = generate(&cfg, |_| 280.0, |_| 0.0).samples;
        let mut live = LiveAnalyzer::new(48_000, LiveConfig::default()).with_figures_as_file();
        // A read in the background of everything heard to `to`, waited for.
        let read_to = |live: &mut LiveAnalyzer, fed: &mut usize, to: usize| {
            for c in sound[*fed..to].chunks(960) {
                live.push_without_waiting(c);
            }
            *fed = to;
            if let Some((job, _)) = live.as_file.job.take() {
                let _ = job.join();
            }
            live.start_read();
            assert!(live.take_read(), "the background read landed");
        };
        let mut fed = 0;
        read_to(&mut live, &mut fed, 50 * 48_000);
        let first = live.as_file.resume_from().expect("kept past 48 s");
        read_to(&mut live, &mut fed, sound.len());
        let next = live.as_file.resume_from().expect("kept");
        assert!(Arc::ptr_eq(&first, &next), "resumed");
        assert!(live.reads_as_file());
        let got = live.reading(10.0);
        assert!(got.rate_s_per_day.is_some(), "{got:?}");
        let log = crate::stream::analyze_samples(&sound, 48_000, &live.stream_config());
        let full = live.view_of(log);
        assert_eq!(live.read_as_file_to(), Some(full.settled_to));
        let r = full.reading_at(full.settled_to, 10.0);
        for (name, a, b) in [
            ("rate", got.rate_s_per_day, r.rate_s_per_day),
            ("amplitude", got.amplitude_deg, r.amplitude_deg),
            ("beat error", got.beat_error_ms, r.beat_error_ms),
        ] {
            assert_eq!(a.map(f64::to_bits), b.map(f64::to_bits), "{name}");
        }
    }

    /// A setting changed while the newest read is within the cap of all
    /// heard drops it: the next ask reads everything heard again, with the
    /// change, to the bit (Norm on #2133).
    #[test]
    fn a_setting_changed_inside_the_cap_reads_again() {
        let mut sound = watch(28_800);
        let mut asked = LiveAnalyzer::new(48_000, LiveConfig::default()).with_figures_as_file();
        for c in sound.chunks(960) {
            asked.push(c);
        }
        let before = asked.reading(10.0);
        // Within the cap: the read kept, and still the figures.
        let more = watch(28_800)[..4_800].to_vec();
        asked.push(&more);
        sound.extend_from_slice(&more);
        assert_eq!(asked.read_as_file_to(), Some(before.time_s));
        asked.set_lift(44.0);
        let after = asked.reading(10.0);
        assert_ne!(after.amplitude_deg, before.amplitude_deg, "the lift took");
        let log = crate::stream::analyze_samples(&sound, 48_000, &asked.stream_config());
        let full = asked.view_of(log);
        assert_eq!(asked.read_as_file_to(), Some(full.settled_to));
        let r = full.reading_at(full.settled_to, 10.0);
        assert_eq!(
            after.amplitude_deg.map(f64::to_bits),
            r.amplitude_deg.map(f64::to_bits)
        );
        assert_eq!(
            after.rate_s_per_day.map(f64::to_bits),
            r.rate_s_per_day.map(f64::to_bits)
        );
    }

    /// A read heard at another beat rate than live's gives no figures, nor
    /// live's own: the beat is still being found (#1987, Norm on #2133).
    #[test]
    fn a_read_at_another_beat_rate_is_still_finding_the_beat() {
        let sound = watch(28_800);
        let mut live = LiveAnalyzer::new(48_000, LiveConfig::default()).with_figures_as_file();
        for c in sound.chunks(960) {
            live.push(c);
        }
        live.read_as_file_now();
        assert!(
            live.reading(10.0).rate_s_per_day.is_some(),
            "read at live's rate"
        );
        let other = crate::stream::analyze_samples(&watch(21_600), 48_000, &live.stream_config());
        assert_ne!(other.bph, 28_800);
        live.set_view(other);
        let r = live.reading(10.0);
        assert!(r.finding_beat && !r.watch_heard, "{r:?}");
        assert_eq!((r.rate_s_per_day, r.amplitude_deg), (None, None));
        assert!(r.beats_used > 0, "its beats are heard");
    }
}
