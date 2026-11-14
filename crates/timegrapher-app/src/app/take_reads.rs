//! The readings of a whole take that rest on its masks, clock and
//! settings: the whole take beside the cards, the Positions Test table
//! and the charts' run of readings. Each is read at once where a caller
//! needs it now, and in the background for the window, so that a mask
//! on a take of many hours never holds the window while they are read
//! again (#978).

use super::*;

/// What [`TimegrapherApp::whole_take`] was read from: the beats heard,
/// the stretches, the masks and what the readings rest on.
pub(super) type WholeKey = (usize, Vec<(f64, f64)>, Vec<(f64, f64)>, Revision);

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
    fn start(key: K, ctx: &egui::Context, read: impl FnOnce() -> T + Send + 'static) -> Self {
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
    pub(super) trend: Option<Job<TrendKey, Vec<TrendPoint>>>,
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
}

/// A Positions Test table being read in the background, and what from.
pub(super) type FreeJob = Job<FreeKey, Option<guided::FreeTable>>;

/// The whole take in one position, and the seconds it covers.
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
        let opened = self.take_breaks.is_some();
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

    /// The whole take as kept (`Ok`), or what it is to be read from
    /// (`Err`); none while listening or with nothing to read.
    fn whole_read(&self) -> Option<Result<Option<WholeTake>, WholeKey>> {
        if self.running() {
            return None;
        }
        let live = self.live.as_ref()?;
        // To the take's end, as session reads it, not its last beat.
        let opened = self.take_breaks.is_some();
        let end = if opened {
            live.duration_s()
        } else {
            self.last_heard_s().min(live.duration_s())
        };
        let spans: Vec<(f64, f64)> = self
            .current_spans()
            .into_iter()
            .map(|(a, b)| (a.max(0.0), b.min(end)))
            .filter(|(a, b)| a < b)
            .collect();
        let key: WholeKey = (
            live.beats().len(),
            spans.clone(),
            self.masked.clone(),
            self.rev,
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
        let opened = self.take_breaks.is_some();
        let log = self.live.as_ref().and_then(steady::log_of);
        let masks = self.masked.clone();
        let (take_clock, take_breaks, ppm) = (
            self.take_clock.clone(),
            self.take_breaks.clone(),
            self.clock.as_ref().map(|c| c.ppm),
        );
        let (lift_known, unlisted) = (self.lift_known(), self.take_unlisted);
        let read = move || {
            let span: f64 = spans.iter().map(|(a, b)| b - a).sum();
            let (&(from, _), &(_, to)) = (spans.first()?, spans.last()?);
            let mut log = log?;
            let clock = reading_clock(take_clock.as_ref(), opened, ppm, log.duration_s);
            // The cuts the app adds (a settling's end, a move, a mask's ends)
            // at the reading's or a mask's edges change nothing it reads, and
            // session has none there (#682).
            let edge = |j: f64| j <= from || j >= to || masks.iter().any(|m| j == m.0 || j == m.1);
            log.jumps.retain(|&j| !edge(j));
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
        // The six, then the upright stops between them, for the Dynamic
        // Poise pane (#1619).
        let between = guided::POISE_ORDER
            .iter()
            .filter(|p| !guided::ORDER.contains(p));
        guided::ORDER
            .iter()
            .chain(between)
            .filter_map(|&p| Some((p, core_session::Position::parse(p.code())?)))
            .flat_map(|(p, sp)| {
                self.session
                    .settled_spans(self.position, p)
                    .into_iter()
                    .map(move |(a, b)| (sp, a, b))
            })
            .collect()
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
        let stretches: Vec<(core_session::Position, f64, f64)> = stretches
            .into_iter()
            .map(|(p, a, b)| (p, a.max(0.0), b.min(end)))
            .filter(|s| s.1 < s.2)
            .collect();
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
            let t = core_session::positions_table(
                &log,
                clock.as_ref(),
                &breaks,
                &masked,
                &stretches,
                core_session::POSITION_MIN_S,
            );
            let upright = core_session::position_rows(
                &core_session::Position::UPRIGHT,
                (&log, clock.as_ref(), &breaks, &masked),
                &stretches,
                core_session::POSITION_MIN_S,
            );
            guided::free_table(&t, guided::upright(&upright, log.bph, bph_set))
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
    /// it rests on changed. While listening, here and now
    /// ([`Self::refresh_trend`]); for a take on show, in the background,
    /// the run on show kept until the new one comes.
    pub(super) fn refresh_trend_shown(&mut self, ctx: &egui::Context) {
        let key: TrendKey = (self.rev, self.average_s.to_bits());
        if self.running() || self.live.is_none() {
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
                live.map_or_else(Vec::new, |l| l.trend(average_s))
            }));
            return;
        };
        match job.done() {
            Ok(points) => {
                self.bg.trend = None;
                self.put_trend(points);
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
