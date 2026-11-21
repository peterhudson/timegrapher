//! The readings of a whole take that rest on its masks, clock and
//! settings: the whole take beside the cards, the Positions Test table
//! and the charts' run of readings. Each is read at once where a caller
//! needs it now, and in the background for the window, so that a mask
//! on a take of many hours never holds the window while they are read
//! again (#978).

use super::*;

#[path = "stop_read.rs"]
mod stop_read;

/// What [`TimegrapherApp::whole_take`] was read from: the beats heard,
/// the stretches, the masks, what the readings rest on, and whether it is
/// the stopped take read as its saved take opens (#2151).
pub(super) type WholeKey = (usize, Vec<(f64, f64)>, Vec<(f64, f64)>, Revision, bool);

/// What [`TimegrapherApp::free_table`] was read from: the beats heard,
/// each position's settled stretches, the masks and the True Clock.
pub(super) type FreeKey = (
    usize,
    Vec<(core_session::Position, f64, f64)>,
    Vec<(f64, f64)>,
    Revision,
);

/// What [`TimegrapherApp::trend`] was read from: what the readings rest
/// on and Average Over.
pub(super) type TrendKey = (Revision, u64);

/// How often the Positions Test table is read again while listening.
const FREE_TABLE_STEP_S: f64 = 1.0;

/// A reading under way in the background, and what it is read from.
pub(super) struct Job<K, T> {
    pub(super) key: K,
    rx: std::sync::mpsc::Receiver<T>,
    /// Held while the reading is wanted: one let go before its turn is
    /// never read.
    _wanted: std::sync::Arc<()>,
}

/// A reading for the background thread.
type Task = Box<dyn FnOnce() + Send>;

/// The one thread the take's readings are read on, one after another,
/// in the order asked for: the charts first. One thread, not one each,
/// leaves the other cores to the window's drawing (#978).
fn reader() -> Option<&'static std::sync::mpsc::Sender<Task>> {
    static READER: std::sync::OnceLock<Option<std::sync::mpsc::Sender<Task>>> =
        std::sync::OnceLock::new();
    READER
        .get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Task>();
            let spawned = std::thread::Builder::new()
                .name("take readings".into())
                .spawn(move || {
                    for task in rx {
                        // A reading that panics loses its answer, not the
                        // thread.
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
                    }
                });
            spawned.ok().map(|_| tx)
        })
        .as_ref()
}

impl<K, T: Send + 'static> Job<K, T> {
    /// Read `read` on the background thread, after those asked for
    /// before; the window is drawn again when it is done.
    pub(super) fn start(
        key: K,
        ctx: &egui::Context,
        read: impl FnOnce() -> T + Send + 'static,
    ) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let wanted = std::sync::Arc::new(());
        let still = std::sync::Arc::downgrade(&wanted);
        let task: Task = Box::new(move || {
            if still.strong_count() > 0 {
                let _ = tx.send(read());
                ctx.request_repaint();
            }
        });
        // With no thread to read on, it is read on one of its own.
        let unsent = match reader() {
            Some(r) => r.send(task).err().map(|e| e.0),
            None => Some(task),
        };
        if let Some(task) = unsent {
            std::thread::spawn(task);
        }
        Job {
            key,
            rx,
            _wanted: wanted,
        }
    }

    /// What it read, once it is done; `Err(true)` while it runs, and
    /// `Err(false)` when it ended with nothing.
    fn done(&self) -> Result<T, bool> {
        match self.rx.try_recv() {
            Ok(t) => Ok(t),
            Err(TryRecvError::Empty) => Err(true),
            Err(TryRecvError::Disconnected) => Err(false),
        }
    }
}

/// The readings of a take on show being read in the background.
#[derive(Default)]
pub(super) struct Background {
    /// The charts' run of readings.
    /// The charts' run, and how much sound it was read on.
    pub(super) trend: Option<Job<TrendKey, (Vec<TrendPoint>, f64)>>,
    /// A long take's run for the Power reserve pane, once it is wanted:
    /// `Some(None)` until its reading starts (#1014).
    pub(super) reserve: Option<Option<Job<Revision, Option<reserve::Test>>>>,
    /// While listening, a long take's run so far being read, keyed by
    /// the take's second it was asked for at.
    pub(super) reserve_live: Option<Job<f64, Option<reserve::Test>>>,
    /// The take's second the run so far was last asked for at.
    pub(super) reserve_live_s: f64,
    /// What the run on show from a long take was read from, so it is
    /// read again when the take, its masks or its settings change after
    /// it came (#1644).
    pub(super) reserve_of: Option<Revision>,
    /// The window's last pass that drew a Steadiness pane, and an STFT or
    /// Wavelet one: the tests, and their maps, are read only while one is
    /// shown; an Isochronism pane alone reads the isochronism alone (#1830).
    pub(super) shown_pass: [Option<u64>; 2],
    /// The isochronism read alone, and what the readings rested on then.
    pub(super) isochronism: Option<(Revision, steady::IsochronismAlone)>,
    /// A finished guided test being read again from its saved take (#1958).
    pub(super) test_read: Option<super::test_read::TestRead>,
    /// The test marks a take opened with, as they went on show: the marks
    /// on show are the take's while they are these (#1957).
    pub(super) opened_marks: Option<Vec<(f64, f64, &'static str)>>,
    /// The take just stopped, read as opening its saved take reads it
    /// (#2151).
    pub(super) stop_read: Option<stop_read::StopRead>,
}

/// A Positions Test table being read in the background, and what from.
pub(super) type FreeJob = Job<FreeKey, Option<guided::FreeTable>>;

/// The whole take in one position, and the seconds it covers: the
/// stretch its reading read, less what that left out (core's
/// `Measurement::measured_s`), so a lead of stand noise the reading
/// leaves out is not counted in "over {span}" (#2177).
#[derive(Clone)]
pub(super) struct WholeTake {
    pub(super) m: timegrapher_core::session::Measurement,
    pub(super) span: f64,
}

/// A reading of the whole take, ready to run here or on a thread of its
/// own.
type WholeRead = Box<dyn FnOnce() -> Option<WholeTake> + Send>;

impl TimegrapherApp {
    /// The take in the position the watch is in now, once it is stopped:
    /// its settled stretches, at whatever length session reads, read as
    /// `timegrapher session` reads a recording (core
    /// `session::measure_masked`), the time between them masked, and the
    /// seconds they cover. A take opened from disk is read to its end, as
    /// session reads it; one just listened to, to its last reading with a
    /// watch heard. Read here and now when it is not kept, as the tests
    /// read it; the window reads it with [`Self::whole_take_shown`].
    #[cfg(test)]
    pub(super) fn whole_take(&self) -> Option<WholeTake> {
        match self.whole_read()? {
            Ok(w) => w,
            Err(key) => {
                let w = self.whole_reader(key.1.clone())();
                *self.whole_take.borrow_mut() = Some((key, w.clone()));
                w
            }
        }
    }

    /// [`Self::whole_take`] for the window: when it is not kept, read in
    /// the background and none until it comes.
    pub(super) fn whole_take_shown(&self, ctx: &egui::Context) -> Option<WholeTake> {
        // A take just listened to is read to its last reading with a
        // watch heard: once the charts are read on what is masked now.
        let opened = self.take_breaks.is_some() || self.stopped_take().is_some();
        if !opened && self.trend_key != (self.rev, self.average_s.to_bits()) {
            return None;
        }
        let key = match self.whole_read()? {
            Ok(w) => return w,
            Err(key) => key,
        };
        let mut job = self.whole_job.borrow_mut();
        if job.as_ref().is_none_or(|j| j.key != key) {
            let read = self.whole_reader(key.1.clone());
            *job = Some(Job::start(key, ctx, read));
            return None;
        }
        let done = job.as_ref().map(Job::done)?;
        match done {
            Ok(w) => {
                let key = job.take().map(|j| j.key)?;
                *self.whole_take.borrow_mut() = Some((key, w.clone()));
                w
            }
            Err(true) => None,
            Err(false) => {
                *job = None;
                None
            }
        }
    }

    /// Where an opened take's readings end once stopped: at the take's
    /// end, as `session` and the take's Watches row read it, not at its
    /// last beat (#2073). The one rule for the whole take and the cards;
    /// none for a take not opened.
    pub(super) fn opened_end_s(&self) -> Option<f64> {
        if let Some(s) = self.stopped_take() {
            return Some(s.live.duration_s());
        }
        let live = self.live.as_ref()?;
        self.take_breaks.is_some().then(|| live.duration_s())
    }

    /// A stopped take's reading over `average` seconds (Norm on #2081):
    /// an opened take's at its end ([`Self::opened_end_s`]) as its whole
    /// reading gives it, where Average Over holds that whole stretch (core's
    /// `LiveReading::at_whole_end`, #2075); else, opened or just stopped, at
    /// the watch's last beat (core's `beats::last_watch_beat_s`), never at
    /// a later point whose look-back still hears the watch over a quiet
    /// stand's hiss; with no run of beats long enough for that, at the
    /// last beat with a watch heard, as before.
    pub(super) fn stopped_reading(&self, live: &LiveAnalyzer, average: f64) -> LiveReading {
        // Just stopped: the take as its saved take opens (#2151).
        let stopped = self.stopped_take();
        let live = stopped.map_or(live, |s| &s.live);
        if let (Some(end), Some(Ok(Some(w)))) = (self.opened_end_s(), self.whole_read()) {
            let r = live.reading_at(end, average).at_whole_end(&w.m, average);
            if r.whole_take {
                return r;
            }
        }
        let at =
            timegrapher_core::beats::last_watch_beat_s(live.beats()).unwrap_or_else(
                || match stopped {
                    Some(_) => live.duration_s(),
                    None => self.latest_s().min(self.last_heard_s()),
                },
            );
        live.reading_at(at, average)
    }

    /// What the whole reading on show is of, in words, as a take's row
    /// names it (`watches::whole_named`): the position on show where the
    /// take reads more than one, as its row counts them (a stretch that is
    /// the watch being handled is not one, #1980).
    pub(super) fn whole_named(&self) -> String {
        let read = self.session.positions_read(self.position).len();
        crate::watches::whole_named(Some(self.position.core()), read)
    }

    /// The whole take as kept (`Ok`), or what it is to be read from
    /// (`Err`); none while listening or with nothing to read.
    fn whole_read(&self) -> Option<Result<Option<WholeTake>, WholeKey>> {
        if self.running() {
            return None;
        }
        let stopped = self.stopped_take();
        let live = stopped.map(|s| &s.live).or(self.live.as_ref())?;
        let end = self
            .opened_end_s()
            .unwrap_or_else(|| self.last_heard_s().min(live.duration_s()));
        // The stopped take read as saved is in the saved sound's time.
        let in_take = |t: f64| match stopped {
            Some(_) => self.session.audio_of(t).unwrap_or(t),
            None => t,
        };
        let spans: Vec<(f64, f64)> = self
            .current_spans()
            .into_iter()
            .map(|(a, b)| (in_take(a), in_take(b)))
            .map(|(a, b)| (a.max(0.0), b.min(end)))
            .filter(|(a, b)| a < b)
            .collect();
        let key: WholeKey = (
            live.beats().len(),
            spans.clone(),
            stopped.map_or_else(|| self.masked.clone(), |s| s.masked().to_vec()),
            self.rev,
            stopped.is_some(),
        );
        if let Some((k, w)) = self.whole_take.borrow().as_ref() {
            if *k == key {
                return Some(Ok(w.clone()));
            }
        }
        Some(Err(key))
    }

    /// The reading of the whole take over its stretches `spans` (see
    /// [`Self::whole_read`]), with what it reads copied in, ready to run
    /// here or in the background.
    fn whole_reader(&self, spans: Vec<(f64, f64)>) -> WholeRead {
        let ppm = self.clock.as_ref().map(|c| c.ppm);
        let lift_known = self.lift_known();
        // Just stopped: the take as its saved take opens (#2151).
        let (opened, log, masks, take_clock, take_breaks, unlisted) = match self.stopped_take() {
            Some(s) => (
                true,
                steady::log_of(&s.live),
                s.masked().to_vec(),
                s.clock.clone(),
                Some(s.breaks.clone()),
                s.unlisted,
            ),
            None => (
                self.take_breaks.is_some(),
                self.live.as_ref().and_then(steady::log_of),
                self.masked.clone(),
                self.take_clock.clone(),
                self.take_breaks.clone(),
                self.take_unlisted,
            ),
        };
        let read = move || {
            spans.first()?;
            let mut log = log?;
            let clock = reading_clock(take_clock.as_ref(), opened, ppm, log.duration_s);
            // The cuts the app adds (a settling's end, a move, a mask's ends)
            // at the reading's or a mask's edges change nothing it reads, and
            // session has none there (#682).
            log.jumps
                .retain(|&j| !timegrapher_core::take_context::cut_at_edge(j, &spans, &masks));
            let breaks = steady::reading_breaks(take_breaks.unwrap_or_default(), &log);
            // The time between the stretches, another position or its
            // settling, is left out as a mask is (core's rule).
            let mut m = timegrapher_core::session::measure_together(
                &log,
                clock.as_ref(),
                &breaks,
                &masks,
                (&spans, &spans),
            )?;
            m.take_doubts(lift_known, unlisted);
            let span = m.measured_s();
            Some(WholeTake { m, span })
        };
        Box::new(read)
    }

    /// The Positions Test table of the take on show or under way, when no
    /// guided test is: each position's settled stretches read by core
    /// `session::positions_table`. While listening it is read afresh every
    /// second, in the background, the last table showing until the next is
    /// ready; a take on show is read at once, as the tests read it. The
    /// window reads it with [`Self::free_table_shown`].
    #[cfg(test)]
    pub(super) fn free_table(&self, ctx: &egui::Context) -> Option<guided::FreeTable> {
        self.table_of(ctx, self.free_stretches(), false)
    }

    /// [`Self::free_table`] for the window: a take on show's is read in
    /// the background when it is not kept, and none until it comes.
    pub(super) fn free_table_shown(&self, ctx: &egui::Context) -> Option<guided::FreeTable> {
        self.table_of(ctx, self.free_stretches(), true)
    }

    /// Whether a Positions Test table of the take on show is being read
    /// in the background.
    pub(super) fn free_table_reading(&self) -> bool {
        !self.running() && self.free_job.borrow().is_some()
    }

    /// Each position's settled stretches, in the table's order.
    fn free_stretches(&self) -> Vec<(core_session::Position, f64, f64)> {
        stretches_in(&self.session, self.position)
    }

    /// The guided test's figures for the position being measured, so far:
    /// its measuring up to now read as [`Self::free_table`] reads a
    /// position, every second. None until it has 10 s.
    pub(super) fn guided_live(&self, ctx: &egui::Context) -> Option<guided::FreeCell> {
        let t = self.guided.as_ref()?;
        let now = self.live.as_ref()?.duration_s();
        let &(from, to) = t.measured(now).get(t.stretches().len())?;
        let p = core_session::Position::parse(t.position().code())?;
        let table = self.table_of(ctx, vec![(p, from, to)], false)?;
        let label = guided::position_label(p);
        let row = table.rows.into_iter().find(|r| r.0 == label)?;
        matches!(row.2, guided::FreeCell::Read(..)).then_some(row.2)
    }

    /// A Positions Test table of `stretches` (position, start, end), each
    /// cut at the take's end so far: read afresh every second, in the
    /// background, while listening; for a take on show at once, or in the
    /// background with none until it comes when `later`.
    fn table_of(
        &self,
        ctx: &egui::Context,
        stretches: Vec<(core_session::Position, f64, f64)>,
        later: bool,
    ) -> Option<guided::FreeTable> {
        let live = self.live.as_ref()?;
        let latest = self.latest_s();
        let running = self.running();
        // A take on show reads to the end of its sound, as `timegrapher
        // session` reads its last position.
        let end = if running {
            (latest / FREE_TABLE_STEP_S).floor() * FREE_TABLE_STEP_S
        } else {
            latest.max(live.duration_s())
        };
        let stretches = clipped(stretches, end);
        let masked = core_session::masked_spans(
            &self.masked.iter().map(|&(a, b)| [a, b]).collect::<Vec<_>>(),
        );
        let beats = if running {
            (end / FREE_TABLE_STEP_S) as usize
        } else {
            live.beats().len()
        };
        let key: FreeKey = (beats, stretches.clone(), masked.clone(), self.rev);
        let mut job = self.free_job.borrow_mut();
        // A table being read on what the readings rested on before is let
        // go; so is one of a take on show read from anything else.
        if job
            .as_ref()
            .is_some_and(|j| j.key.3 != self.rev || !running && j.key != key)
        {
            *job = None;
        }
        if let Some(done) = job.as_ref().map(Job::done) {
            match done {
                Ok(t) => {
                    let key = job.take().map(|j| j.key)?;
                    *self.free_table.borrow_mut() = Some((key, t));
                }
                Err(running) => {
                    if !running {
                        *job = None;
                    }
                }
            }
        }
        if let Some((k, t)) = self.free_table.borrow().as_ref() {
            if *k == key || running && job.is_some() {
                return t.clone();
            }
        }
        if !running && job.is_some() {
            return None;
        }
        let (take_clock, opened, ppm) = (
            self.take_clock.clone(),
            self.take_breaks.clone(),
            self.clock.as_ref().map(|c| c.ppm),
        );
        let bph_set = live.config().analysis.bph;
        let read = move |log: BeatLog| {
            let clock = reading_clock(take_clock.as_ref(), opened.is_some(), ppm, log.duration_s);
            let breaks = steady::reading_breaks(opened.unwrap_or_default(), &log);
            read_table(&log, clock.as_ref(), &breaks, &masked, &stretches, bph_set)
        };
        let log = steady::log_of(live);
        let shown = self.free_table.borrow().as_ref().map(|(_, t)| t.clone());
        match (running, shown, log) {
            // Listening, with a table on show: read the next one in the
            // background and keep this one until it comes.
            (true, Some(shown), Some(log)) => {
                *job = Some(Job::start(key, ctx, move || Some(read(log))));
                shown
            }
            // A take on show, for the window: none until it comes.
            (false, _, Some(log)) if later => {
                *job = Some(Job::start(key, ctx, move || Some(read(log))));
                None
            }
            (_, _, log) => {
                let t = log.map(read);
                *self.free_table.borrow_mut() = Some((key, t.clone()));
                t
            }
        }
    }

    /// For the window: the charts' run of readings read again when what
    /// it rests on changed, in the background, the run on show kept until
    /// the new one comes. While listening too: read here, a long take's
    /// 4,000 readings held the window and the listening (1.6 s at 4 h,
    /// #1825). The readings heard after the sound it was read on are on
    /// the new footing already and are kept after it.
    pub(super) fn refresh_trend_shown(&mut self, ctx: &egui::Context) {
        let key: TrendKey = (self.rev, self.average_s.to_bits());
        if self.live.is_none() {
            self.bg.trend = None;
            self.refresh_trend();
            return;
        }
        if self.trend_key == key {
            self.bg.trend = None;
            return;
        }
        if self.bg.trend.as_ref().is_some_and(|j| j.key != key) {
            self.bg.trend = None;
        }
        let Some(job) = &self.bg.trend else {
            let (live, average_s) = (self.live.clone(), self.average_s);
            self.bg.trend = Some(Job::start(key, ctx, move || {
                live.map_or_else(
                    || (Vec::new(), 0.0),
                    |l| (l.trend(average_s), l.duration_s()),
                )
            }));
            return;
        };
        match job.done() {
            Ok((mut points, read_to)) => {
                self.bg.trend = None;
                let first = self.trend.is_empty();
                points.extend(self.trend.iter().filter(|p| p.t > read_to).copied());
                self.put_trend(points);
                if first {
                    self.refit_opened();
                }
            }
            Err(true) => {}
            Err(false) => self.bg.trend = None,
        }
    }

    /// The readings a frame of the window reads in the background: the
    /// charts' run, then a long take's run for the Power reserve pane.
    pub(super) fn take_reads_shown(&mut self, ctx: &egui::Context) {
        self.refresh_trend_shown(ctx);
        self.long_reserve_shown(ctx);
    }

    /// [`Self::reserve_from_long_take`] for the window, read in the
    /// background once a take has opened, so a take of many hours doesn't
    /// hold the window as it lands (#1014). It is read again if what it
    /// rests on changes before it comes, and again if it changes after
    /// (#1644), and let go when the pane gets a test of its own. While listening, the run so far is read once a
    /// minute ([`Self::long_reserve_listening`]), and once more where
    /// listening stops.
    fn long_reserve_shown(&mut self, ctx: &egui::Context) {
        if self.reserve_is_test() {
            self.bg.reserve = None;
            self.bg.reserve_live = None;
            self.bg.reserve_of = None;
            return;
        }
        if self.running() {
            self.long_reserve_listening(ctx);
            return;
        }
        self.bg.reserve_live = None;
        let key = self.rev;
        if self.bg.reserve.is_none() && self.bg.reserve_of.is_some_and(|of| of != key) {
            self.bg.reserve = Some(None);
        }
        let Some(wanted) = &self.bg.reserve else {
            return;
        };
        match wanted.as_ref().filter(|j| j.key == key).map(Job::done) {
            Some(Ok(t)) => {
                self.reserve = t;
                self.bg.reserve = None;
                self.bg.reserve_of = Some(key);
            }
            Some(Err(true)) => {}
            Some(Err(false)) => self.bg.reserve = None,
            None => {
                self.bg.reserve = self
                    .long_take_reader()
                    .map(|read| Some(Job::start(key, ctx, read)));
            }
        }
    }

    /// While listening, a take of [`power_reserve::LONG_TAKE_S`] or more
    /// that isn't a test fills the Power reserve pane as an opened one
    /// does, as the Positions Test pane fills while listening: its run so
    /// far, read in the background once a minute, the last reading kept
    /// until the next comes. A take that goes into a second position
    /// leaves the pane as before a test. Where listening stops, it is
    /// read once more ([`Self::long_reserve_shown`]).
    fn long_reserve_listening(&mut self, ctx: &egui::Context) {
        self.bg.reserve = Some(None);
        if let Some(job) = &self.bg.reserve_live {
            match job.done() {
                Ok(t) => {
                    self.reserve = t;
                    self.bg.reserve_live = None;
                }
                Err(true) => return,
                Err(false) => self.bg.reserve_live = None,
            }
        }
        let now = self.live.as_ref().map_or(0.0, |l| l.duration_s());
        if now < power_reserve::LONG_TAKE_S
            || (0.0..power_reserve::READ_AGAIN_S).contains(&(now - self.bg.reserve_live_s))
        {
            return;
        }
        self.bg.reserve_live_s = now;
        // Read as a run so far: the take hasn't ended.
        self.bg.reserve_live = self.long_take_reader().map(|read| {
            Job::start(now, ctx, move || {
                read().map(|mut t| {
                    t.results = t.results.map(power_reserve::Results::still_listening);
                    t.ending = t.results.as_ref().map(|r| r.ending);
                    t
                })
            })
        });
    }
}

/// Each position's settled stretches in `session`, the watch in `now`,
/// in the table's order: the six, then the upright stops between them,
/// for the Dynamic Poise pane (#1619). What the This Take table reads,
/// and Compare With reads of a take picked there (#1851).
pub(super) fn stretches_in(
    session: &crate::session::Session,
    now: Position,
) -> Vec<(core_session::Position, f64, f64)> {
    let between = guided::POISE_ORDER
        .iter()
        .filter(|p| !guided::ORDER.contains(p));
    guided::ORDER
        .iter()
        .chain(between)
        .filter_map(|&p| Some((p, core_session::Position::parse(p.code())?)))
        .flat_map(|(p, sp)| {
            session
                .settled_spans(now, p)
                .into_iter()
                .map(move |(a, b)| (sp, a, b))
        })
        .collect()
}

/// `stretches`, each cut at the take's start and at `end`; one with
/// nothing left is left out.
pub(super) fn clipped(
    stretches: Vec<(core_session::Position, f64, f64)>,
    end: f64,
) -> Vec<(core_session::Position, f64, f64)> {
    stretches
        .into_iter()
        .map(|(p, a, b)| (p, a.max(0.0), b.min(end)))
        .filter(|s| s.1 < s.2)
        .collect()
}

/// The Positions Test table of `log` over `stretches`, on `clock`, cut
/// at `breaks`, with `masked` left out: core's table of the six and of
/// the upright stops, at the beat rate set (`bph_set`). The one reading
/// of a take's positions, for its This Take table and for Compare With
/// reading another take (#1851), so the two agree to the bit.
pub(super) fn read_table(
    log: &BeatLog,
    clock: Option<&timegrapher_core::clock::ClockFit>,
    breaks: &[timegrapher_core::breaks::Break],
    masked: &[(f64, f64)],
    stretches: &[(core_session::Position, f64, f64)],
    bph_set: Option<u32>,
) -> guided::FreeTable {
    let t = core_session::positions_table(
        log,
        clock,
        breaks,
        masked,
        stretches,
        core_session::POSITION_MIN_S,
    );
    let upright = core_session::position_rows(
        &core_session::Position::UPRIGHT,
        (log, clock, breaks, masked),
        stretches,
        core_session::POSITION_MIN_S,
    );
    guided::free_table(
        &t,
        guided::take_readings(&guided::card_rows(&t), log.bph, bph_set),
        (guided::take_readings(&upright, log.bph, bph_set), &upright),
    )
}
