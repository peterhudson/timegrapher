//! Live builds its beat template again as a file's reading does (#1327):
//! from the sound of the first chunk, which it then reads again with it.

use super::*;

/// Where live is in building its template again ([`LiveAnalyzer::rebuild_template`]).
#[derive(Debug, Clone, Default)]
pub(super) struct Rebuild {
    /// The sound from the window the watch was first heard in, and the
    /// sample it starts at, gathered until it holds a file's first chunk.
    /// None before the watch is heard and once built.
    gather: Option<(u64, Vec<f32>)>,
    /// Whether the template has been built again since the watch was
    /// first heard (or that was given up).
    done: bool,
    /// When the sound read again took over, seconds.
    switch_s: Option<f64>,
    /// How far the beats moved then: seconds, the median over the beats
    /// both reads found (the new template's reference point against the
    /// old one's), and how their count moved.
    shift: Option<(f64, i64)>,
    /// Why the template was not built again, when it was not.
    given_up: Option<&'static str>,
    /// The gathered sound being read again with the template built from
    /// it, a little faster than it comes in, until it catches up.
    shadow: Option<Box<Shadow>>,
}

/// The sound gathered read again: a fresh analyzer and the sound it has
/// still to read.
#[derive(Debug, Clone)]
struct Shadow {
    live: LiveAnalyzer,
    pending: Vec<f32>,
    at: usize,
}

/// How much faster than it comes in the gathered sound is read again: 47 s
/// is caught up about 12 s after the template is built, at most this many
/// times a push's work in any one push, so the screen never waits on all
/// of it at once.
const CATCH_UP: usize = 4;

/// Why the template was not built again when the first chunk's template
/// read both sides in fewer windows
/// ([`crate::amplitude::reads_fewer_sides`]).
const KEPT_LOCK: &str = "the first chunk's template read both sides in fewer windows";

impl LiveAnalyzer {
    /// When the template built again from a file's first chunk of sound,
    /// and that sound read again with it, took over (#1327), seconds; None
    /// until then.
    pub fn template_switch_s(&self) -> Option<f64> {
        self.rebuild.switch_s
    }

    /// How far the beats moved when the first chunk read again took over:
    /// seconds (the new template's reference point against the old one's)
    /// and beats in the count, so a view anchored on the old beats can stay
    /// where it was.
    pub fn template_shift(&self) -> Option<(f64, i64)> {
        self.rebuild.shift
    }

    /// Why the template was not built again from the first chunk, when it
    /// was not (#1327), for the log: live and a file may then differ.
    pub fn template_rebuild_given_up(&self) -> Option<&'static str> {
        self.rebuild.given_up
    }

    /// Whether live kept the template it locked on with because the first
    /// chunk's read both sides in fewer windows (#1500): its readings may
    /// then differ from a file's reading of the same sound, which uses the
    /// first chunk's.
    pub fn kept_lock_template(&self) -> bool {
        self.rebuild.given_up == Some(KEPT_LOCK)
    }

    /// [`Caveat::LockTemplateKept`] on a reading when live kept the
    /// template it locked on with.
    pub(super) fn add_rebuild_caveat(&self, caveats: &mut Vec<Caveat>) {
        if self.kept_lock_template() {
            measure::add(caveats, Caveat::LockTemplateKept);
        }
    }

    /// Whether sound is held to build the template again or being read
    /// again with it.
    pub fn gathering(&self) -> bool {
        self.rebuild.gather.is_some() || self.rebuild.shadow.is_some()
    }

    /// Start again: the next watch heard has its template built again.
    pub(super) fn forget_rebuild(&mut self) {
        self.rebuild = Rebuild::default();
    }

    /// Stop building the template again: what the caller changed (a new
    /// stretch, a pause, its own breaks) has no place in the sound
    /// gathered. The template stays as first built.
    pub(super) fn give_up_rebuild(&mut self, why: &'static str) {
        // Before the watch is heard nothing is gathered: a pause or breaks
        // then come before the first chunk, which the copy keeps.
        if self.template.is_none() {
            return;
        }
        if !self.rebuild.done {
            self.rebuild.given_up = Some(why);
        }
        self.rebuild.gather = None;
        self.rebuild.shadow = None;
        self.rebuild.done = true;
    }

    /// The same change made to the analyzer reading the sound again.
    pub(super) fn in_shadow(&mut self, f: impl FnOnce(&mut LiveAnalyzer)) {
        if let Some(sh) = &mut self.rebuild.shadow {
            f(&mut sh.live);
        }
    }

    /// After a push of `samples`, `passed` when a pass ran: gathers them,
    /// builds the template again once a chunk is gathered, and reads the
    /// sound again a little further. True when the readings may have
    /// changed.
    pub(super) fn rebuild_after_push(&mut self, samples: &[f32], passed: bool) -> bool {
        if let Some((_, g)) = &mut self.rebuild.gather {
            g.extend_from_slice(samples);
        }
        if let Some(sh) = &mut self.rebuild.shadow {
            sh.pending.extend_from_slice(samples);
            self.catch_up(samples.len());
            return true;
        }
        if passed {
            self.rebuild_template();
        }
        false
    }

    /// Gathers the sound from the window the watch was first heard in
    /// until it holds a file's first chunk, then builds the template again
    /// from it as a file's reading does ([`crate::stream::first_beats_in`])
    /// and reads that sound again with it ([`Self::reread`]), so live and
    /// a file read the beats with the same template from the start, with
    /// no cut between two (#1327). The sound gathered is let go. Only a
    /// watch first heard with no beats yet: one heard again after a long
    /// silence keeps its template, as a file's reading keeps its first.
    fn rebuild_template(&mut self) {
        let Some(bph) = self
            .bph
            .filter(|_| self.template.is_some() && !self.rebuild.done)
        else {
            return;
        };
        if self.rebuild.gather.is_none() {
            let win = ((self.cfg.window_s * self.fs).ceil() as usize).min(self.buf.len());
            let from = self.buf.len() - win;
            let g0 = self.buf0 + from as u64;
            if self
                .beats
                .first()
                .is_some_and(|b| b.time < g0 as f64 / self.fs)
            {
                self.give_up_rebuild("the watch was heard again after a silence");
                return;
            }
            self.rebuild.gather = Some((g0, self.buf[from..].to_vec()));
        }
        let chunk = (crate::stream::StreamConfig::default().chunk_s * self.fs).round() as usize;
        if self
            .rebuild
            .gather
            .as_ref()
            .is_some_and(|(_, g)| g.len() < chunk)
        {
            return;
        }
        let Some((g0, g)) = self.rebuild.gather.take() else {
            return;
        };
        self.rebuild.done = true;
        let a = &self.cfg.analysis;
        let env = envelope(&g[..chunk], self.fs, &a.envelope);
        let Some(t) = crate::stream::first_beats_in(&env, self.fs, Some(bph), a.amplitude_window_s)
            .and_then(|f| f.template)
        else {
            self.rebuild.given_up = Some("no watch heard in the first chunk");
            return;
        };
        drop(env);
        self.reread(g0, g, t);
    }

    /// Starts reading the sound `g`, from sample `g0` to now, again with
    /// `template` from a fresh start, as a file reads its first chunk: a
    /// copy of this analyzer as it was at `g0`, with the masks kept, finds
    /// the beats, amplitude windows, profiles and cuts since `g0` again
    /// through the same passes ([`Self::catch_up`]).
    fn reread(&mut self, g0: u64, g: Vec<f32>, template: Vec<f32>) {
        let mut l = self.clone();
        let since = g0 as f64 / self.fs;
        let before = |v: &[f64]| -> Vec<f64> { v.iter().copied().filter(|&t| t < since).collect() };
        // The caller's breaks all stand (set before the watch was heard,
        // since any later gives the rebuild up); the jumps live found are
        // found again from `g0`.
        let jumps = before(&self.intrinsic_breaks);
        let breaks: Vec<f64> = (self.breaks.iter().copied())
            .filter(|t| *t < since || !self.intrinsic_breaks.contains(t))
            .collect();
        let (breaks, jumps, by_count) = (breaks, jumps, before(&self.by_count));
        l.restart();
        (l.breaks, l.intrinsic_breaks, l.by_count) = (breaks, jumps, by_count);
        (l.bph, l.found_bph, l.logged_bph) = (self.bph, self.found_bph, self.logged_bph);
        l.template = Some(template);
        l.rebuild.done = true;
        l.buf.clear();
        (l.buf0, l.total, l.next_pass) = (g0, g0, g0);
        l.settled_to = since;
        self.rebuild.shadow = Some(Box::new(Shadow {
            live: l,
            pending: g,
            at: 0,
        }));
    }

    /// Reads up to [`CATCH_UP`] times `n` samples more of the sound being
    /// read again, and once it has caught up, takes its reading over: the
    /// first chunk's readings change once, then.
    fn catch_up(&mut self, n: usize) {
        let step = (self.cfg.update_s * self.fs).round().max(1.0) as usize;
        let Some(sh) = self.rebuild.shadow.as_mut() else {
            return;
        };
        let to = (sh.at + CATCH_UP * n).min(sh.pending.len());
        while sh.at < to {
            let end = (sh.at + step).min(to);
            sh.live.push(&sh.pending[sh.at..end]);
            sh.at = end;
        }
        if sh.at < sh.pending.len() {
            // Let go of what has been read, now and then.
            if sh.at > sh.pending.len() / 2 {
                sh.pending.drain(..sh.at);
                sh.at = 0;
            }
            return;
        }
        let Some(sh) = self.rebuild.shadow.take() else {
            return;
        };
        let mut l = sh.live;
        if crate::amplitude::reads_fewer_sides(&self.amp, &l.amp) {
            // The shadow is gone and the template stays as first built.
            self.rebuild.given_up = Some(KEPT_LOCK);
            return;
        }
        l.rebuild.switch_s = Some(self.duration_s());
        l.rebuild.shift = moved(&self.beats, &l.beats);
        *self = l;
    }
}

/// The median move of the beats from `old` to `new`, seconds, over the
/// beats both found (each `new` beat paired with the nearest `old` one
/// within a millisecond), and the move in their count there.
fn moved(old: &[Beat], new: &[Beat]) -> Option<(f64, i64)> {
    let mut d: Vec<(f64, i64)> = new
        .iter()
        .filter_map(|b| {
            let i = old.partition_point(|o| o.time < b.time);
            [i.checked_sub(1), Some(i)]
                .into_iter()
                .flatten()
                .filter_map(|j| old.get(j))
                .map(|o| (b.time - o.time, b.index - o.index))
                .filter(|x| x.0.abs() <= 0.001)
                .min_by(|x, y| x.0.abs().total_cmp(&y.0.abs()))
        })
        .collect();
    if d.is_empty() {
        return None;
    }
    d.sort_by(|x, y| x.0.total_cmp(&y.0));
    let dt = d[d.len() / 2].0;
    let mut di: Vec<i64> = d.iter().map(|x| x.1).collect();
    di.sort_unstable();
    Some((dt, di[di.len() / 2]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{generate, SynthConfig};

    fn heard(seconds: f64, mut then: impl FnMut(&mut LiveAnalyzer, f64)) -> LiveAnalyzer {
        let cfg = SynthConfig {
            duration_s: seconds,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 280.0, |_| 0.0);
        let mut l = LiveAnalyzer::new(cfg.sample_rate, LiveConfig::default());
        for c in audio.samples.chunks(960) {
            l.push(c);
            let t = l.duration_s();
            then(&mut l, t);
        }
        l
    }

    #[test]
    fn the_first_chunk_read_again_takes_over_with_the_beats_moved_by_little() {
        let l = heard(80.0, |_, _| {});
        let at = l.template_switch_s().expect("taken over");
        // 47 s gathered, then read again four times faster than it comes.
        assert!((55.0..70.0).contains(&at), "{at}");
        assert!(!l.gathering());
        let (dt, di) = l.template_shift().expect("the beats paired");
        assert!(dt.abs() < 0.001 && di == 0, "{dt} {di}");
        assert!(l.breaks().is_empty() && l.template_rebuild_given_up().is_none());
    }

    /// What the caller changes while the first chunk is read again reaches
    /// the copy that takes over: lift, masks, known flags, clock, profile
    /// span and the beats kept.
    #[test]
    fn changes_made_while_reading_again_survive_the_takeover() {
        let mut set = false;
        let clock = crate::clock::ClockFit::fixed(12.0, 80.0);
        let l = heard(80.0, |l, _| {
            if !set && l.rebuild.shadow.is_some() {
                set = true;
                l.set_lift(49.0);
                l.set_masked(&[(20.0, 22.0)]);
                l.set_known(true, true);
                l.set_rate_clock(Some(&clock));
                l.set_profile_span(7.0);
                l.set_max_beats(12_345);
            }
        });
        assert!(set && l.template_switch_s().is_some());
        assert_eq!(l.cfg.analysis.amplitude.lift_deg, 49.0);
        assert!(!l.masked.is_empty());
        assert!(l.cfg.analysis.lift_known && l.clock_known);
        assert_eq!(l.rate_clock.as_ref().map(|c| c.ppm), Some(12.0));
        assert_eq!(l.cfg.profile_s, 7.0);
        assert_eq!(l.cfg.max_beats, 12_345);
    }

    fn kept_caveat(l: &LiveAnalyzer) -> bool {
        l.reading(10.0).caveats.contains(&Caveat::LockTemplateKept)
    }

    /// What the caller does at 30 s, before the takeover, and why the
    /// first template then stays. Only the kept lock template carries the
    /// caveat, not a rebuild given up for another reason.
    fn given_up_by(act: impl Fn(&mut LiveAnalyzer)) -> LiveAnalyzer {
        let l = heard(80.0, |l, t| {
            if (30.0..30.02).contains(&t) {
                act(l);
            }
        });
        assert_eq!(l.template_switch_s(), None);
        assert!(!l.gathering());
        assert!(!kept_caveat(&l));
        l
    }

    #[test]
    fn a_new_stretch_before_the_takeover_keeps_the_first_template_and_says_why() {
        let l = given_up_by(|l| l.new_stretch());
        assert_eq!(
            l.template_rebuild_given_up(),
            Some("a new stretch began first")
        );
    }

    #[test]
    fn a_pause_or_breaks_before_the_takeover_keep_the_first_template_too() {
        let l = given_up_by(|l| l.skip(2.0));
        assert_eq!(
            l.template_rebuild_given_up(),
            Some("listening paused first")
        );
        let l = given_up_by(|l| l.set_breaks(vec![29.0]));
        assert_eq!(l.template_rebuild_given_up(), Some("breaks were set first"));
    }

    /// Breaks set before the watch is heard give nothing up; the copy that
    /// reads the first chunk again keeps them.
    #[test]
    fn breaks_before_the_watch_is_heard_survive_the_takeover() {
        // A set rate: guessing one starts the beat log, and its breaks,
        // afresh when it locks on.
        let cfg = SynthConfig {
            duration_s: 80.0,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 280.0, |_| 0.0);
        let mut live = LiveConfig::default();
        live.analysis.bph = Some(cfg.bph);
        let mut l = LiveAnalyzer::new(cfg.sample_rate, live);
        for c in audio.samples.chunks(960) {
            l.push(c);
            if (1.0..1.02).contains(&l.duration_s()) {
                l.set_breaks(vec![0.5]);
            }
        }
        assert!(l.template_switch_s().is_some());
        assert_eq!(l.breaks(), &[0.5]);
    }

    /// A watch heard again after a long silence keeps its template, as a
    /// file's reading keeps its first.
    #[test]
    fn a_watch_heard_again_after_a_silence_keeps_its_template() {
        let cfg = |duration_s| SynthConfig {
            duration_s,
            ..Default::default()
        };
        let mut sound = generate(&cfg(10.0), |_| 280.0, |_| 0.0).samples;
        let fs = cfg(1.0).sample_rate;
        sound.extend(std::iter::repeat_n(0.0, 8 * fs as usize));
        sound.extend(generate(&cfg(60.0), |_| 280.0, |_| 0.0).samples);
        let mut l = LiveAnalyzer::new(fs, LiveConfig::default());
        for c in sound.chunks(960) {
            l.push(c);
        }
        assert_eq!(l.template_switch_s(), None);
        assert_eq!(
            l.template_rebuild_given_up(),
            Some("the watch was heard again after a silence")
        );
    }

    /// A copy that reads both sides in far fewer windows doesn't take
    /// over: live keeps its template, says why, and its readings carry the
    /// caveat.
    #[test]
    fn a_copy_that_reads_fewer_sides_is_refused_with_its_caveat() {
        let l = heard(80.0, |l, _| {
            if let Some(sh) = l.rebuild.shadow.as_mut() {
                for w in &mut sh.live.amp {
                    w.even_deg = None;
                }
            }
        });
        assert_eq!(l.template_switch_s(), None);
        assert!(!l.gathering() && l.kept_lock_template());
        assert_eq!(l.template_rebuild_given_up(), Some(KEPT_LOCK));
        assert!(kept_caveat(&l));
    }

    /// Setting the rate Auto found, while the copy reads, reaches the copy.
    #[test]
    fn a_rate_set_while_reading_again_survives_the_takeover() {
        let mut set = false;
        let l = heard(80.0, |l, _| {
            if !set && l.rebuild.shadow.is_some() {
                set = true;
                let bph = l.bph;
                assert!(l.set_bph(bph));
            }
        });
        assert!(set && l.template_switch_s().is_some());
        assert_eq!(l.cfg.analysis.bph, Some(28_800));
    }

    #[test]
    fn the_beats_move_by_their_median_and_their_count_by_its_median() {
        let at = |t: f64, index: i64| Beat {
            time: t,
            index,
            quality: 1.0,
        };
        let old: Vec<Beat> = (0..10).map(|i| at(i as f64 * 0.2, i)).collect();
        let new: Vec<Beat> = (0..10)
            .map(|i| at(i as f64 * 0.2 + 0.0003, i + 1))
            .collect();
        let (dt, di) = moved(&old, &new).unwrap();
        assert!((dt - 0.0003).abs() < 1e-12 && di == 1, "{dt} {di}");
        assert_eq!(moved(&old, &[at(5.0, 0)]), None);
    }

    /// The takeover is refused when the sound read again gives both sides
    /// in a share of the windows lower by more than 2 SE (#1500, Walter's
    /// run 11): the lock's template read 7 of 10 windows there, the first
    /// chunk's 1 of 11.
    #[test]
    fn a_template_that_reads_fewer_sides_does_not_take_over() {
        let win = |a: f64, both: bool| AmplitudeWindow {
            start_s: a,
            end_s: a + 2.0,
            even_deg: both.then_some(300.0),
            odd_deg: Some(330.0),
            beat_error_ms: None,
            beat_error_unlock_ms: None,
            even_unlock_ms: None,
            odd_unlock_ms: None,
            even_drop_ms: None,
            odd_drop_ms: None,
            culled_ms: None,
        };
        let run = |from: f64, n: usize, k: usize| -> Vec<AmplitudeWindow> {
            (0..n).map(|i| win(from + 2.0 * i as f64, i < k)).collect()
        };
        // Windows before the stretch read again don't count.
        let mut old = run(0.0, 20, 0);
        old.extend(run(40.0, 10, 7));
        assert!(crate::amplitude::reads_fewer_sides(&old, &run(41.0, 11, 1)));
        assert!(!crate::amplitude::reads_fewer_sides(
            &old,
            &run(41.0, 11, 5)
        ));
        assert!(!crate::amplitude::reads_fewer_sides(
            &old,
            &run(41.0, 10, 10)
        ));
        assert!(!crate::amplitude::reads_fewer_sides(&old, &[]));
    }
}
