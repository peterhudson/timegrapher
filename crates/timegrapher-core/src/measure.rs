//! What was measured over a stretch of beats beyond the readings
//! themselves: the ± of each reading and the conditions detected, decided
//! in one place for every reading of a recording (`analyze`, `session`,
//! the live screen and `long`'s slices).
//!
//! The readings themselves come from the one beat pipeline
//! ([`crate::stream`]); this module only states what they rest on. Each
//! caveat names a condition detected, with its figure; none of them
//! changes a reading or its ±, except `no_watch`: when no regular beat is
//! found, rate, amplitude and beat error are not given, since they would
//! describe noise.
//!
//! Whether the beats are regular is assessed on 10 s windows, so with
//! less than [`SHORT_SPAN_S`] of sound their timing is not assessed. What
//! is looked at instead, and on a longer stretch whose scatter is not far
//! below noise's, is the beats' averaged sound: when neither the Tick's
//! nor the Tock's shows an unlock or a drop, no regular beat is found
//! (`no_watch`, reason `no_beat_sound`). Otherwise the readings are given,
//! `watch_heard` stays true, and `short_span` says how short a stretch
//! under 10 s is (see [`no_watch_reason`]).
//!
//! The beat error's sign: positive means Tick comes late. Each take starts
//! positive: Tick is the beat that comes late over the take's first stretch
//! with a clear sign, live and from the file alike ([`crate::beats::SideCheck`]);
//! near no beat error it is the take's first beat. Text shows the size.

use crate::amplitude::AmplitudeWindow;
use crate::confidence::{self, Confidence, Doubts};
use crate::timing::WindowFit;
use serde::{Deserialize, Serialize};

/// Seconds of sound needed to assess whether the beats are regular, and
/// below which a stretch of beats is named `short_span` and a reading
/// waits ([`awaiting_verdict`]). The shortest piece a reading is fitted
/// over ([`crate::timing::MIN_PIECE_S`]), for the same reason: a few
/// seconds of beats give a rate several s/d out (Norm on #2333).
pub const SHORT_SPAN_S: f64 = crate::timing::MIN_PIECE_S;

/// Whether a stretch's beats are judged for regularity: it holds
/// `listened_s` seconds of sound, at least [`SHORT_SPAN_S`], and
/// `holds_window`, at least one whole scatter window on the take's grid
/// lies inside it, clear of its cuts ([`crate::session::holds_scatter_window`],
/// decided from where the windows fall, not from whether they fit). A
/// stretch that can hold no window gives no scatter to judge, so it is
/// read as one under [`SHORT_SPAN_S`]: the beat sound decides (Norm on
/// #1688). One that holds a window whose beats give no fit, such as
/// noise, is still judged, and still no watch.
pub fn regularity_assessed(listened_s: f64, holds_window: bool) -> bool {
    // A tolerance, so a 10 s average reached by subtraction counts.
    holds_window && listened_s >= SHORT_SPAN_S - 1e-6
}

/// The fact for a recording read from its start too short for
/// [`regularity_assessed`]: "Beat regularity not assessed: 5.0 s of sound,
/// 10 s needed". The wording for the doctor (Norm on #2363), `analyze`'s
/// text (#2367) and `shape`'s (#2371).
pub fn not_assessed_text(listened_s: f64) -> String {
    format!(
        "Beat regularity not assessed: {} s of sound, {SHORT_SPAN_S} s needed",
        crate::library::fixed_text(listened_s, 1)
    )
}

/// False when a regular beat was looked for and not found: where
/// regularity is `assessed` ([`regularity_assessed`]), the beat timing
/// scatter is at or above [`crate::beats::NO_WATCH_JITTER_FRACTION`] of a
/// beat. Where it is not, this is true. [`no_watch_reason`] also looks at
/// the beats' sound.
pub fn watch_heard(jitter_us: Option<f64>, bph: u32, assessed: bool) -> bool {
    !assessed || crate::beats::watch_heard(jitter_us, bph)
}

/// Whether the beats' averaged sound has a say in [`no_watch_reason`]:
/// where regularity is not `assessed` ([`regularity_assessed`]: under
/// [`SHORT_SPAN_S`], or no whole scatter window), or where the timing
/// scatter is at or above [`crate::beats::NOISE_DOUBT_FRACTION`], where
/// noise can still scatter as little as this (#675). Every reader asks
/// this before working out the sound ([`stretch_beat_sound`]), so all of
/// them judge a stretch alike.
pub fn beat_sound_needed(jitter_us: Option<f64>, bph: u32, assessed: bool) -> bool {
    !assessed || in_doubt(jitter_us, bph)
}

/// A timing scatter at or above [`crate::beats::NOISE_DOUBT_FRACTION`].
fn in_doubt(jitter_us: Option<f64>, bph: u32) -> bool {
    crate::beats::timing_scatter_fraction(jitter_us, bph)
        .is_some_and(|f| f >= crate::beats::NOISE_DOUBT_FRACTION)
}

/// Whether a reading waits for its verdict: under [`SHORT_SPAN_S`] of
/// sound has been heard since the start or the latest break, its masked
/// seconds left out (`heard_s`; a mask does not restart the count, Norm
/// on #2017). Then there is no verdict either way and no figures, however
/// the beats sound: a few seconds of beats is not a rate, and one stretch
/// reads one way live, stopped and opened (Norm on #2333, revising #2014,
/// where the beats' sound could speak for a watch sooner; live had no
/// beats in its first seconds while a saved read had them, so a stopped
/// take read a rate from about ten beats). An empty stand's first seconds
/// show no figures either (#2014). A short average over longer sound is
/// not waiting: what is heard does not depend on how it is averaged.
/// Every reader asks this beside [`no_watch_reason`], so they withhold
/// alike.
pub fn awaiting_verdict(heard_s: f64) -> bool {
    heard_s < SHORT_SPAN_S
}

/// A reader's verdict once `heard_s` of sound is heard
/// ([`awaiting_verdict`]): why no watch is heard, `no_watch`, and whether
/// it waits. While it waits there is no verdict either way, so no "No
/// Regular Beat" from the beats' sound alone in the first seconds (Norm
/// on #2333). Every reader takes its verdict through this.
pub fn verdict_heard(
    heard_s: f64,
    no_watch: Option<NoWatchReason>,
) -> (Option<NoWatchReason>, bool) {
    let awaiting = awaiting_verdict(heard_s);
    (no_watch.filter(|_| !awaiting), awaiting)
}

/// Why no regular beat was found, or `None` when one was, from the timing
/// scatter and the beats' averaged sound (`beat_sound`, from
/// [`stretch_beat_sound`]: whether either side shows an unlock or a drop):
/// - where regularity is not `assessed` ([`regularity_assessed`]: under
///   [`SHORT_SPAN_S`], or no whole scatter window), the timing is not
///   judged, and the sound decides when it can be judged;
/// - where it is, a scatter at or above
///   [`crate::beats::NO_WATCH_JITTER_FRACTION`] is no watch;
/// - a scatter from [`crate::beats::NOISE_DOUBT_FRACTION`] up to it, where
///   noise can still fall (#675) and so can a weak watch about to stop, is
///   a watch unless the sound shows none.
pub fn no_watch_reason(
    jitter_us: Option<f64>,
    bph: u32,
    assessed: bool,
    beat_sound: Option<bool>,
) -> Option<NoWatchReason> {
    if !crate::beats::watch_heard(jitter_us, bph) && assessed {
        Some(NoWatchReason::Scatter)
    } else if beat_sound_needed(jitter_us, bph, assessed) && beat_sound == Some(false) {
        Some(NoWatchReason::NoBeatSound)
    } else {
        None
    }
}

/// [`beat_sound_found`] for a stretch whose beats run from `first_s` to
/// `last_s`, from `snippets`: its clean beats whose envelope windows are
/// kept, each at its time with whether it is a Tick: the take's one
/// sample of beat sound ([`crate::sound_sample`]), spread evenly over the
/// whole take. `None` when the kept ones in the stretch span less than
/// half of it, so a stretch is not judged on a small part of it, or when
/// neither side has the [`crate::profile::PROFILE_MIN_BEATS`] an
/// averaged profile needs.
pub fn stretch_beat_sound(
    snippets: &[(f64, bool, &[f32])],
    first_s: f64,
    last_s: f64,
    fs: f64,
    bph: u32,
    cfg: &crate::amplitude::AmplitudeConfig,
) -> Option<bool> {
    let kept = snippets.last()?.0 - snippets.first()?.0;
    if kept < 0.5 * (last_s - first_s) {
        return None;
    }
    beat_sound_found(snippets.iter().map(|s| (s.1, s.2)), fs, bph, cfg)
}

/// Whether the beats' averaged Tick and Tock sounds show a watch's sound:
/// an unlock or a drop on either side, as [`crate::profile`] finds them on
/// the same template amplitude uses. `snippets` are each beat's envelope
/// window ([`crate::profile::snippet`]) with whether it is a Tick (an even
/// beat). `None` when neither side has enough beats for a profile.
///
/// No period moves the verdict: the edges are found on the sound alone,
/// and the period, at `bph`'s nominal one, only turns them into the
/// profile's amplitude, which the verdict does not read (Trevor on #2252).
pub fn beat_sound_found<'a>(
    snippets: impl IntoIterator<Item = (bool, &'a [f32])>,
    fs: f64,
    bph: u32,
    cfg: &crate::amplitude::AmplitudeConfig,
) -> Option<bool> {
    let osc_period_s = 2.0 * crate::timing::beat_period_s(None, bph.max(1));
    let (tick, tock): (Vec<_>, Vec<_>) = snippets.into_iter().partition(|s| s.0);
    // Whether there is a sound at all, not where its unlock lies: the
    // windows are kept on the short template ([`crate::sound_sample`]).
    let short = crate::amplitude::UnlockWindow::SHORT;
    let shows = |side: crate::profile::Side, w: Vec<(bool, &[f32])>| {
        let w: Vec<&[f32]> = w.into_iter().map(|s| s.1).collect();
        crate::profile::from_snippets(&w, fs, side, osc_period_s, cfg, None, short)
            .map(|p| p.unlock_ms.is_some() || p.drop_ms.is_some())
    };
    match (
        shows(crate::profile::Side::A, tick),
        shows(crate::profile::Side::B, tock),
    ) {
        (None, None) => None,
        (a, b) => Some(a == Some(true) || b == Some(true)),
    }
}

/// The beats of a watch a reading counts, and the beat rate it gives
/// (Norm on #2378): a beat is a peak in a beat train heard as a watch, so
/// where none is heard (`watch_heard` false, [`no_watch_reason`]) no
/// beats of a watch were found, 0, and the beat rate is the one set
/// (`bph_set`), or none, since a rate guessed from noise is no rate
/// heard. Where one is heard, the peaks `found` and the rate `bph` read
/// at. The one rule for every document that counts beats.
pub fn watch_beats(
    watch_heard: bool,
    found: usize,
    bph: u32,
    bph_set: Option<u32>,
) -> (usize, Option<u32>) {
    if watch_heard {
        (found, Some(bph))
    } else {
        (0, bph_set.map(|_| bph))
    }
}

/// Whether too few of the beats expected were found: under
/// [`confidence::MIN_BEAT_FRACTION`] of them, as `found` gives the share.
pub fn beats_missing(found: f64) -> bool {
    found < confidence::MIN_BEAT_FRACTION
}

/// The reason on the `no_watch` caveat among `caveats`, if there is one.
pub fn no_watch_reason_in(caveats: &[Caveat]) -> Option<NoWatchReason> {
    caveats.iter().find_map(|c| match c {
        Caveat::NoWatch { reason } => Some(*reason),
        _ => None,
    })
}

/// The heading every surface gives a reading with no regular beat, over
/// [`no_watch_text`]'s sentence or in the figures' place (#510).
pub const NO_WATCH_TITLE: &str = "No Regular Beat";

/// What the sound behind a reading is heard since: where the
/// [`SHORT_SPAN_S`] a reading's figures wait for is counted from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeardSince {
    /// Listen started (or the take's start, opened).
    Start,
    /// The latest break, such as a move to a new position.
    Break,
}

/// The heading every surface gives a reading still waiting for
/// [`SHORT_SPAN_S`] of sound, in the figures' place (#2014, Hugh).
pub const LISTENING_TITLE: &str = "Listening";

/// The sentence under [`LISTENING_TITLE`]: what the figures wait for, and
/// since when it is counted.
pub fn listening_text(since: HeardSince) -> String {
    let from = match since {
        HeardSince::Start => "Listen started",
        HeardSince::Break => "the break",
    };
    format!(
        "The figures need {s:.0} s of sound: they appear once {s:.0} s have been heard \
         since {from}.",
        s = SHORT_SPAN_S
    )
}

/// What a reading says in place of [`NO_WATCH_TITLE`] while a regular
/// beat just heard is still being found (`live::LiveReading::finding_beat`,
/// #2008): a wait, not a verdict.
pub const FINDING_TITLE: &str = "Finding the Beat";

/// Why a reading at Average Over `average_s` waits while the beat is
/// being found, for a hover: the wait is [`crate::live::finding_s`] long.
pub fn finding_text(average_s: f64) -> String {
    format!(
        "A regular beat is heard. The figures wait until the last {} is mostly \
         the watch's, so the sound before it isn't counted against it.",
        crate::figure_text::duration_text(crate::live::finding_s(average_s))
    )
}

/// Why a reading card has no figure yet, as every card says it in the
/// figure's place: one waiting language for the Rate, Amplitude and Beat
/// Error cards, saying what the figures wait for and roughly how long
/// (#2007). [`LISTENING_TITLE`] and [`FINDING_TITLE`] are the waits the
/// reading itself gives; these are the rest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wait {
    /// The watch settles after a move, `left_s` seconds still to come:
    /// nothing is read while it does.
    Settling { left_s: f64 },
    /// No beat heard at all yet: no watch on the stand, or none the
    /// microphone hears.
    NoBeats,
    /// Beats are heard, but this card's figure needs more of them than
    /// the stretch holds yet.
    MoreBeats,
    /// Beats heard, but under [`SHORT_SPAN_S`] of sound since `since`, and
    /// the sound is over (stopped, or an opened take at its newest
    /// moment), so the figures are still held: a fact, in the
    /// [`Wait::NoBeatsHeard`] family (Hugh on #2066).
    ShortHeard { since: HeardSince },
    /// A test is set up and waits for its Start.
    TestStart,
    /// A recording is being read.
    Analysing,
    /// Nothing is listened to and no recording is open (Hugh on #2036).
    NotListening,
    /// A stopped take or an opened recording with no beat in the stretch
    /// read: a fact, not a wait (Hugh on #2036).
    NoBeatsHeard,
    /// Beat error, where the reading has figures but hasn't told the Tick
    /// from the Tock yet (Hugh 5 on #2133).
    TickAndTock,
}

/// The Beat Error card's own wait: a reading with a rate but neither beat
/// error (from the unlock or the drop) hasn't told the sides apart yet,
/// so it says so rather than showing a bare dash (Norm and Hugh 5 on
/// #2133).
pub fn beat_error_wait(rate: Option<f64>, drop: Option<f64>, unlock: Option<f64>) -> Option<Wait> {
    (rate.is_some() && drop.is_none() && unlock.is_none()).then_some(Wait::TickAndTock)
}

/// The heading of [`Wait::NoBeats`].
pub const WAITING_FOR_BEATS: &str = "Waiting for Beats";

/// The heading of [`Wait::MoreBeats`]: beats are heard, so not
/// [`WAITING_FOR_BEATS`] (Hugh on #2036).
pub const WAITING_FOR_MORE_BEATS: &str = "Waiting for More Beats";

/// A wait's shorter words, where its title is too wide for the figure's
/// place at its own size: "More Beats" for [`WAITING_FOR_MORE_BEATS`]
/// (Hugh on #2036), "Under 10 s" for [`Wait::ShortHeard`] (Hugh on
/// #2066). None for a title with no shorter form.
pub fn wait_short_title(title: &str) -> Option<String> {
    if title == WAITING_FOR_MORE_BEATS {
        return Some("More Beats".into());
    }
    let under = format!("Under {SHORT_SPAN_S:.0} s");
    (title == format!("{under} Heard")).then_some(under)
}

/// The whole seconds of a settle still to come, as every countdown says
/// them: rounded up, so it reads 1 s until the settle is over, never 0 s
/// (#2044: the cards and the Positions pane said the same settle 1 s
/// apart).
pub fn settle_seconds(left_s: f64) -> f64 {
    left_s.ceil()
}

impl Wait {
    /// The short words in the figure's place.
    pub fn title(self) -> String {
        match self {
            Wait::Settling { left_s } => format!("Settling, {:.0} s", settle_seconds(left_s)),
            Wait::NoBeats => WAITING_FOR_BEATS.into(),
            Wait::MoreBeats => WAITING_FOR_MORE_BEATS.into(),
            Wait::ShortHeard { .. } => format!("Under {SHORT_SPAN_S:.0} s Heard"),
            Wait::TestStart => "Waiting for Start".into(),
            Wait::Analysing => "Analysing".into(),
            Wait::NotListening => "Not Listening".into(),
            Wait::NoBeatsHeard => "No Beats Heard".into(),
            Wait::TickAndTock => "Waiting for Tick and Tock".into(),
        }
    }

    /// The sentence for the figure's hover, in the style of
    /// [`listening_text`]: what the figures need, and when they appear.
    pub fn text(self) -> String {
        match self {
            Wait::Settling { left_s } => format!(
                "The figures need the watch settled in its new position: reading starts in \
                 {:.0} s.",
                settle_seconds(left_s)
            ),
            Wait::NoBeats => "The figures need a watch's beats: none are heard yet.".into(),
            Wait::ShortHeard { since } => {
                let from = match since {
                    HeardSince::Start => "the start",
                    HeardSince::Break => "the break",
                };
                format!(
                    "The figures need {s:.0} s of sound: less than {s:.0} s was heard since \
                     {from}.",
                    s = SHORT_SPAN_S
                )
            }
            Wait::MoreBeats => "This figure needs more beats than the stretch holds yet: it \
                                appears once they have been heard."
                .into(),
            Wait::TestStart => {
                "The figures need the test started: Start Test on the Listen card starts it.".into()
            }
            Wait::Analysing => "The figures need the recording read: they appear once \
                                Analysing is done."
                .into(),
            Wait::NotListening => "The figures need sound: nothing is being listened to.".into(),
            Wait::NoBeatsHeard => {
                "The figures need a watch's beats: none were heard in this stretch.".into()
            }
            Wait::TickAndTock => "Beat error needs the Tick and the Tock told apart: reading \
                                  starts once they are."
                .into(),
        }
    }
}

/// The sentence for no regular beat found, as every surface states it:
/// with the timing scatter, as [`crate::beats::no_regular_beat_text`]
/// gives it, or that the beats' sound shows no unlock and no drop.
pub fn no_watch_text(
    reason: NoWatchReason,
    jitter_us: Option<f64>,
    bph_set: Option<u32>,
) -> String {
    match reason {
        NoWatchReason::Scatter => crate::session::no_regular_beat(jitter_us, bph_set),
        NoWatchReason::NoBeatSound => {
            "No regular beat found (neither the Tick's nor the Tock's sound shows an unlock \
             or a drop)"
                .into()
        }
        // Hugh's words (#2257).
        NoWatchReason::TooShort => format!(
            "No regular beat found (the beats were regular, but no run of them lasted the {} \
             a reading needs)",
            crate::figure_text::duration_text(crate::timing::MIN_PIECE_S)
        ),
    }
}

/// What a reading too short to read needs, said over the strip that draws
/// its beats: the [`NoWatchReason::TooShort`] rule's length, from
/// [`crate::timing::MIN_PIECE_S`] (Hugh on #2311). No time until figures:
/// live shows them some seconds after a run reaches it.
pub fn too_short_need_text() -> String {
    format!(
        "A reading needs a run of regular beats lasting {} or more.",
        crate::figure_text::duration_text(crate::timing::MIN_PIECE_S)
    )
}

/// What the `no_watch` caveat ([`Caveat::NoWatch`]) was found on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoWatchReason {
    /// The beat timing scatter, with at least [`SHORT_SPAN_S`] of sound.
    #[default]
    Scatter,
    /// With less sound, the beats' averaged sound: neither the Tick's nor
    /// the Tock's shows an unlock or a drop ([`beat_sound_found`]).
    NoBeatSound,
    /// No 10 s scatter window fitted, but the beats came in a regular
    /// run ([`regular_run`]), each run between pauses too short for one
    /// ([`too_short`], Norm on #2257).
    TooShort,
}

impl NoWatchReason {
    /// Whether the reason says the beats were regular: only
    /// [`NoWatchReason::TooShort`]. Words about beats that don't come at
    /// regular intervals go beside every other reason, never beside this
    /// one (Hugh, Vernon on #2311).
    pub fn beats_were_regular(self) -> bool {
        self == NoWatchReason::TooShort
    }
}

/// Whether a run of beats, fitted as `fit`, is a watch's by its timing:
/// its own scatter under [`crate::beats::NO_WATCH_JITTER_FRACTION`] of a
/// beat at `bph`. The one test live's Finding the Beat and the
/// [`NoWatchReason::TooShort`] reason share (Norm on #2257).
pub fn regular_run(fit: Option<&crate::timing::TimingFit>, bph: u32) -> bool {
    fit.is_some_and(|f| crate::beats::watch_heard(Some(f.jitter_us), bph))
}

/// Whether beats with no 10 s scatter window fitted (`jitter_us` None)
/// came in regular runs too short to read: the longest run of `beats`'
/// good ones between pauses ([`crate::clean::beat_pauses`]) that began at
/// or after `from_s` is a [`regular_run`], fitted as a reading fits its
/// stretch.
///
/// `beats` are all those since the take's start or its latest cut, and
/// `from_s` is where the reader's own beats start (its look back or its
/// span). A run that began before `from_s` was cut by the reader, not by
/// a pause, so it is not a short run and does not count; only a run that
/// began after a pause, a cut or the take's start does (Norm on #2311).
pub fn too_short(
    jitter_us: Option<f64>,
    beats: &[crate::beats::Beat],
    from_s: f64,
    bph: u32,
) -> bool {
    if jitter_us.is_some() {
        return false;
    }
    let good: Vec<crate::beats::Beat> = beats.iter().filter(|b| b.is_good()).copied().collect();
    let mut runs = Vec::new();
    let mut lo = 0;
    for c in crate::clean::beat_pauses(&good) {
        let hi = lo + good[lo..].partition_point(|b| b.time < c);
        runs.push(&good[lo..hi]);
        lo = hi;
    }
    runs.push(&good[lo..]);
    let span = |r: &[crate::beats::Beat]| match (r.first(), r.last()) {
        (Some(a), Some(b)) => b.time - a.time,
        _ => 0.0,
    };
    runs.into_iter()
        .filter(|r| r.first().is_some_and(|b| b.time >= from_s))
        .max_by(|a, b| span(a).total_cmp(&span(b)))
        .is_some_and(|run| regular_run(crate::timing::fit(run, bph).as_ref(), bph))
}

/// [`no_watch_reason`], with a scatter that no window could read, of
/// beats that came in short regular runs, said as
/// [`NoWatchReason::TooShort`] ([`too_short`] of `beats` since the take's
/// start or latest cut, counting runs begun from `from_s`): the one
/// reason every reader gives (Norm on #2257).
pub fn no_watch_reason_of(
    jitter_us: Option<f64>,
    bph: u32,
    (assessed, beat_sound): (bool, Option<bool>),
    (beats, from_s): (&[crate::beats::Beat], f64),
) -> Option<NoWatchReason> {
    match no_watch_reason(jitter_us, bph, assessed, beat_sound)? {
        NoWatchReason::Scatter if too_short(jitter_us, beats, from_s, bph) => {
            Some(NoWatchReason::TooShort)
        }
        r => Some(r),
    }
}

/// What a [`Caveat::BeatErrorSourceReadings`] counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Counted {
    /// Readings, such as a session's position readings.
    #[default]
    Readings,
    /// The amplitude's windows, [`crate::distribution::STRETCH_S`] long by default
    /// (`AnalysisConfig::amplitude_window_s`).
    Windows,
}

impl Counted {
    /// The plural name of what was counted, with its length where it has one.
    pub fn plural(self) -> String {
        match self {
            Counted::Readings => "readings".to_string(),
            Counted::Windows => format!("{:.0} s windows", crate::distribution::STRETCH_S),
        }
    }
}

/// Why the unlock gives no beat error: so a Swiss lever's comes from the
/// drop ([`Caveat::BeatErrorFromDrop`]), and a co-axial has none
/// ([`Caveat::CoaxialBeatError`], #2313).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DropWhy {
    /// The unlock was not found (or in too few windows).
    #[default]
    UnlockNotFound,
    /// The unlock's beat errors disagree in sign more than their spread
    /// allows ([`crate::confidence::unlock_inconsistent`], #1439).
    UnlockInconsistent,
}

impl DropWhy {
    /// The reason saved readings from before #1439 carry: none written.
    pub fn is_not_found(&self) -> bool {
        *self == Self::UnlockNotFound
    }

    /// The reason in a few words, as the command line gives it beside a
    /// beat error from the drop, or beside a co-axial's "none".
    pub fn words(self) -> &'static str {
        match self {
            Self::UnlockNotFound => "unlock not found",
            Self::UnlockInconsistent => "the unlock's signs disagree beyond their spread",
        }
    }

    /// The reason `caveats` give for a beat error from the drop; not found
    /// where they give none.
    pub fn of(caveats: &[Caveat]) -> Self {
        caveats
            .iter()
            .find_map(|c| match c {
                Caveat::BeatErrorFromDrop { why } => Some(*why),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// Why a co-axial reading in `caveats` has no beat error
    /// ([`Caveat::CoaxialBeatError`]); None where it has one, or is not
    /// read for a co-axial.
    pub fn coaxial_of(caveats: &[Caveat]) -> Option<Self> {
        caveats.iter().find_map(|c| match c {
            Caveat::CoaxialBeatError { why } => *why,
            _ => None,
        })
    }

    /// The caveat's sentence for this reason, as a reading's hover gives it.
    pub fn sentence(self) -> &'static str {
        match self {
            Self::UnlockNotFound => {
                "The unlock was not found; the beat error is measured from the drop."
            }
            Self::UnlockInconsistent => {
                "The unlock's beat errors disagree in sign more than their spread allows; \
                 the beat error is measured from the drop."
            }
        }
    }
}

/// [`Caveat::FaintSidesWithheld`] as a sentence, for `sides` sides in
/// `windows` windows (Hugh's words on #1800).
fn faint_sides_text(sides: usize, windows: usize) -> String {
    let share = crate::amplitude::FAINT_SOUND_MAX_SHARE;
    let they = if sides == 1 { "It is" } else { "They are" };
    let whose = if windows == 1 {
        "that window's"
    } else {
        "those windows'"
    };
    format!(
        "{sides} side{} in {windows} window{} had an unlock sound too faint to tell from \
         noise: under {share:.2} of that side's usual level nearby. {they} left out of \
         {whose} amplitude and beat error.",
        plural(sides),
        plural(windows),
    )
}

/// The beat rate heard of [`Caveat::OffStandardRate`] at its place, as
/// the figure gives it: "17571 bph".
fn heard_bph(o: &crate::timing::OffStandard) -> String {
    let digits = crate::figure_text::base_digits::BEAT_RATE_HEARD;
    format!(
        "{} bph",
        crate::figure_text::fixed_text(o.bph_measured, digits)
    )
}

/// [`Caveat::OffStandardRate`] as a sentence: that no rate in s/d is
/// given because the lock was judged off every standard rate, then this
/// reading's beat rate heard and how far it is from the standard rate on
/// each side (Norm on #2223, point 5; Hugh on #2316).
fn off_standard_text(o: &crate::timing::OffStandard) -> String {
    let rate = crate::figure_text::base_digits::RATE;
    let side = |s: &crate::timing::StandardSide| {
        let r = crate::figure_text::signed_text(s.rate_s_per_day, rate);
        format!("{r} s/d against {} bph", s.bph)
    };
    let sides: Vec<String> = o.below.iter().chain(o.above.iter()).map(side).collect();
    // The verdict is the lock's; the figures are this reading's, so the
    // sentence holds when a reading's own rate is back in reach (Hugh on
    // #2316).
    format!(
        "No rate in s/d: over the first {:.0} s after Auto locked on to the beat, the watch \
         was more than {:.0} s/d from every standard rate. This reading: {} heard, {}.",
        crate::beats::GUESS_SPAN_S,
        crate::beats::STANDARD_REACH_S_PER_DAY,
        heard_bph(o),
        sides.join(", "),
    )
}

/// The beat rate heard where a reading is off every standard rate
/// ([`Caveat::OffStandardRate`]), for a reading whose rate is then none.
pub fn off_standard(caveats: &[Caveat]) -> Option<crate::timing::OffStandard> {
    caveats.iter().find_map(|c| match c {
        Caveat::OffStandardRate(o) => Some(*o),
        _ => None,
    })
}

/// What every reading off every standard rate keeps beside its blanked
/// rate figures: no rate ± in `confidence`, and the caveat `o` (#2223).
pub fn off_standard_withheld(
    o: crate::timing::OffStandard,
    confidence: Option<&mut Confidence>,
    caveats: &mut Vec<Caveat>,
) {
    if let Some(c) = confidence {
        c.rate = confidence::Reading {
            se: None,
            no_pm: None,
        };
    }
    add(caveats, Caveat::OffStandardRate(o));
}

/// [`Caveat::LockTemplateKept`] as a sentence, with the first chunk's
/// length from [`crate::stream::StreamConfig`] and the lock-on window's
/// from [`crate::live::LiveConfig`].
fn lock_template_kept_text() -> String {
    let chunk_s = crate::stream::StreamConfig::default().chunk_s;
    let lock_s = crate::live::LiveConfig::default().window_s;
    format!(
        "This reading used the beat shape from the first {lock_s:.0} s of sound, where Live \
         locks on. The shape from the first {chunk_s:.0} s found amplitude on both sides in \
         fewer windows. Live and saved takes choose the same way."
    )
}

/// Something that limits how far a measurement can be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum Caveat {
    /// No regular beat found: the beat timing scatter is at or above
    /// [`crate::beats::NO_WATCH_JITTER_FRACTION`] of a beat (`reason`
    /// `scatter`), or, with under [`SHORT_SPAN_S`] of sound, neither the
    /// Tick's nor the Tock's averaged sound shows an unlock or a drop
    /// (`no_beat_sound`), or `too_short`, the beats came in regular runs,
    /// but none that began after a pause, a cut or the recording's start
    /// lasted the 10 s a reading needs ([`NoWatchReason::TooShort`]). No
    /// readings are given.
    NoWatch {
        #[serde(default)]
        reason: NoWatchReason,
    },
    /// A set beat rate (`--bph`, `--calibre`) differs from the one heard.
    SetRateDisagrees { heard: u32 },
    /// On Auto, the rate read against the standard rate guessed is
    /// further than [`crate::beats::STANDARD_REACH_S_PER_DAY`] from it:
    /// no rate in s/d is given, and this gives the beat rate heard and
    /// the standard rate on each side ([`crate::timing::OffStandard`],
    /// Norm on #2223).
    OffStandardRate(crate::timing::OffStandard),
    /// More than [`crate::analysis::Clipping::NOTED_FRACTION`] of the beats
    /// have a clipped sample; `fraction` is their share.
    Clipped { fraction: f64 },
    /// Fewer than [`confidence::MIN_BEAT_FRACTION`] of the beats expected
    /// were found; `fraction` is the share found.
    BeatsMissing { fraction: f64 },
    /// The lift angle is a default, not the calibre's.
    LiftUnknown,
    /// The take is read for a co-axial escapement (#2313,
    /// [`crate::escapement`]): its amplitude is an estimate (beta), read
    /// with the Swiss lever's formula and not yet checked against a
    /// reference.
    CoaxialEstimate,
    /// The sound card's clock is not corrected.
    ClockUnknown,
    /// The recording's USB lines read the card `ppm` fast, past
    /// [`crate::usbclock::MAX_CARD_PPM`], so they correct nothing (#2283).
    UsbLinesNotUsed { ppm: f64 },
    /// The recording's USB lines correct the rates, and its clock log, which
    /// would correct them on its own, measures the card differently by
    /// more than its own scatter ([`crate::clock::ClockFit::sources_disagree`]):
    /// ppm the card runs
    /// slow by each, and the log's standard error. With `against`
    /// `correction`, on a take with no clock log, the other is the
    /// correction known for the input, and `log_ppm` is its ppm.
    ClockSourcesDisagree {
        usb_ppm: f64,
        log_ppm: f64,
        log_ppm_sd: f64,
        #[serde(default, skip_serializing_if = "crate::clock::Against::is_clock_log")]
        against: crate::clock::Against,
    },
    /// The beats span less than [`SHORT_SPAN_S`].
    ShortSpan { span_s: f64 },
    /// The take was paused or resumed inside the stretch.
    Breaks { count: usize },
    /// The take's breaks are not all from the list its `session.json`
    /// keeps, but from its clock log: `why` says what stood in the way
    /// ([`crate::breaks::Unlisted`]). A pause the clock log does not show
    /// is then not a break, and the rate may be fitted across it.
    BreaksUnlisted { why: crate::breaks::Unlisted },
    /// `count` pauses the take's `session.json` lists show no gap in its
    /// clock log ([`crate::clock::ClockFit::unseen`]). The log is the
    /// better witness, so each is a break of no width: the time after it
    /// is not moved on by the gap listed (#646).
    PausesUnseen { count: usize },
    /// At `count` of those breaks Tick and Tock were not told apart by
    /// their sound or by the beat error's sign (too alike, too few good
    /// beats, or no jump in the beats for the side check to run at), so the
    /// sides after each are named by the beat count across it alone.
    SidesByCount { count: usize },
    /// The beat error's sign differs either side of a cut in the stretch
    /// ([`crate::timing::sign_turns`]), so it is given as a size, positive,
    /// rather than signed.
    BeatErrorAsSize,
    /// The selected beat error comes from the drop, for the reason `why`:
    /// the unlock not found, or found inconsistent (#1439).
    BeatErrorFromDrop {
        #[serde(default, skip_serializing_if = "DropWhy::is_not_found")]
        why: DropWhy,
    },
    /// The take is read for a co-axial escapement (#2313,
    /// [`crate::escapement`]), so its beat error is the unlock's alone,
    /// never the drop's; `why` says why there is none, where there is
    /// none: the unlock not found, or its signs disagreeing (#1439).
    /// Given with every co-axial reading that gives readings.
    CoaxialBeatError {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        why: Option<DropWhy>,
    },
    /// Saved readings of unknown event were excluded from a pooled figure.
    BeatErrorUnknownSource { readings: usize },
    /// Only readings from the selected event entered the beat-error statistic.
    BeatErrorSourceReadings {
        source: crate::session::BeatErrorSource,
        used: usize,
        readings: usize,
        /// What `used` and `readings` count; older reports counted readings.
        #[serde(default)]
        counted: Counted,
    },
    /// A figure was read from fewer than the required share of its kept windows.
    ReadingWindows {
        reading: confidence::Figure,
        windows_used: usize,
        windows_in_span: usize,
    },
    /// `sides` window sides, in `windows` windows, were withheld because
    /// the side's sound 1 was under [`crate::amplitude::FAINT_SOUND_MAX_SHARE`]
    /// of the same side's in its chunk: too faint to tell its unlock from
    /// noise (#1738). They give no amplitude and no unlock beat error.
    FaintSidesWithheld { sides: usize, windows: usize },
    /// Stretches of the take were masked out of the readings by hand;
    /// `seconds` is their total within the stretch.
    Masked { seconds: f64 },
    /// The take's entries name `count` positions, so what goes with the
    /// amplitude across it may go with the position instead (#651).
    Positions { count: usize },
    /// The first `seconds` of the stretch were noise before the watch was
    /// heard, and are left out of the readings
    /// ([`crate::session::watch_lead_s`], #844).
    LeadLeftOut { seconds: f64 },
    /// The template built from the first 47 s read both sides in fewer
    /// amplitude windows than the one a short window gave, as live locks
    /// on with, so that one was kept: by live
    /// ([`crate::live::LiveAnalyzer::kept_lock_template`], #1500) and by a
    /// file's reading ([`crate::stream::BeatLog::lock_template_kept`],
    /// #1503) alike.
    LockTemplateKept,
    /// The amplitude's or beat error's period search named a cycle on a
    /// stretch under [`crate::longterm::NOISE_CAVEAT_SPAN_S`] whose 2 s
    /// windows are correlated at `rho` (lag one, the ρ the search whitens
    /// by, not the 10 s readings' lag-1 r) of [`crate::longterm::NOISE_CAVEAT_RHO`] or more, over `span_s`:
    /// there the search names a cycle in noise alone about 1 time in 24
    /// (#916).
    PeriodMayBeNoise { rho: f64, span_s: f64 },
    /// The period search named a cycle on a take its simulated null
    /// ([`crate::longterm::null`]) does not cover: its length, grid step
    /// or noise `mix` (ρ for readings, the crossover times the detrend's
    /// window for the timing) is outside the table, so the cycle is held
    /// to the plain trials count, which ran 2 to 10 times over its stated
    /// rate where it was measured (#1195).
    PeriodSearchUncalibrated {
        model: crate::longterm::NullModel,
        mix: f64,
        span_s: f64,
    },
    /// The rate's period search named a cycle on beat timing that is not
    /// the random walk plus white jitter its simulated null draws: its
    /// band powers sit `misfit` (rms of log10) from the fitted model,
    /// past `limit`, the 99th percentile over the null's own takes
    /// (#1218, #1219).
    TimingModelMisfit { misfit: f64, limit: f64 },
}

/// The amplitude's hover where Average Over, `average_s`, is under
/// [`SHORT_SPAN_S`], which it is read over then (Norm, #1951; Hugh's
/// words): "Over the last 10 s, not 5 s: amplitude needs at least 10 s of
/// beats." None at [`SHORT_SPAN_S`] and over.
pub fn amplitude_span_note(average_s: f64) -> Option<String> {
    span_note("amplitude", average_s)
}

/// [`amplitude_span_note`] for the beat error from the unlock, read from
/// the same windows.
pub fn unlock_span_note(average_s: f64) -> Option<String> {
    span_note("the unlock", average_s)
}

fn span_note(what: &str, average_s: f64) -> Option<String> {
    amplitude_span_s(average_s).map(|span| {
        format!(
            "Over the last {span} s, not {average_s} s: {what} needs at least \
             {span} s of beats."
        )
    })
}

/// The span the amplitude and the unlock beat error are read over where
/// Average Over, `average_s`, is shorter than they need: [`SHORT_SPAN_S`]
/// (Norm, #1951). None where they are read over the average itself. The
/// one place that floor is decided; live readings and every hover that
/// names it read it here.
pub fn amplitude_span_s(average_s: f64) -> Option<f64> {
    (average_s < SHORT_SPAN_S).then_some(SHORT_SPAN_S)
}

/// What a caveat bears on: one reading, or the take as a whole (#1975).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Rate,
    Amplitude,
    BeatError,
    Take,
}

impl Caveat {
    /// The readings it bears on (Hugh and Norm, #1975). One reading's
    /// caveat is said beside that reading; one that bears on more than
    /// one, or on the take, is said once for the whole take. Clipping
    /// and a default lift angle move the amplitude and the beat error
    /// (its angle), not the rate, and sides too faint to read give neither
    /// an amplitude nor an unlock beat error; a set beat rate unlike the
    /// one heard moves every reading.
    pub fn scopes(&self) -> &'static [Scope] {
        use Scope::{Amplitude, BeatError, Rate, Take};
        match self {
            Caveat::ClockUnknown
            | Caveat::ClockSourcesDisagree { .. }
            | Caveat::UsbLinesNotUsed { .. }
            | Caveat::OffStandardRate(_) => &[Rate],
            Caveat::LiftUnknown | Caveat::Clipped { .. } | Caveat::FaintSidesWithheld { .. } => {
                &[Amplitude, BeatError]
            }
            Caveat::CoaxialEstimate => &[Amplitude],
            Caveat::SidesByCount { .. }
            | Caveat::BeatErrorAsSize
            | Caveat::BeatErrorFromDrop { .. }
            | Caveat::CoaxialBeatError { .. }
            | Caveat::BeatErrorUnknownSource { .. }
            | Caveat::BeatErrorSourceReadings { .. } => &[BeatError],
            Caveat::ReadingWindows { reading, .. } => match reading {
                confidence::Figure::Rate => &[Rate],
                confidence::Figure::Amplitude => &[Amplitude],
                confidence::Figure::BeatError => &[BeatError],
            },
            Caveat::NoWatch { .. }
            | Caveat::SetRateDisagrees { .. }
            | Caveat::BeatsMissing { .. }
            | Caveat::ShortSpan { .. }
            | Caveat::Breaks { .. }
            | Caveat::BreaksUnlisted { .. }
            | Caveat::PausesUnseen { .. }
            | Caveat::Masked { .. }
            | Caveat::Positions { .. }
            | Caveat::LeadLeftOut { .. }
            | Caveat::LockTemplateKept
            | Caveat::PeriodMayBeNoise { .. }
            | Caveat::PeriodSearchUncalibrated { .. }
            | Caveat::TimingModelMisfit { .. } => &[Take],
        }
    }

    /// The one reading it bears on, where there is one: the reading it is
    /// said beside. None for one said for the whole take.
    pub fn reading_scope(&self) -> Option<Scope> {
        match self.scopes() {
            [Scope::Take] => None,
            [one] => Some(*one),
            _ => None,
        }
    }

    /// The beat error from the drop because the unlock was not found.
    pub const FROM_DROP: Caveat = Caveat::BeatErrorFromDrop {
        why: DropWhy::UnlockNotFound,
    };

    /// The caveat in a few words, for a line of conditions or a mark beside
    /// a reading, as the app words it: "2 breaks", "input clock not
    /// corrected". `lift_deg` is the lift angle the amplitude used, and
    /// `beats` the beats found. `None` for no regular beat (said by
    /// [`no_watch_text`]) and for a masked sliver under 0.05 s.
    pub fn short(&self, lift_deg: f64, beats: usize) -> Option<String> {
        Some(match self {
            Caveat::NoWatch { .. } => return None,
            Caveat::ReadingWindows {
                reading,
                windows_used,
                windows_in_span,
            } => format!("{reading} read from {windows_used} of {windows_in_span} windows"),
            Caveat::BeatErrorSourceReadings {
                source,
                used,
                readings,
                counted,
            } => format!(
                "beat error from the {source} in {used} of {readings} {}",
                counted.plural()
            ),
            Caveat::BeatErrorUnknownSource { readings } => {
                format!("{readings} beat-error readings with no recorded source left out")
            }
            Caveat::BeatErrorFromDrop { .. } => "beat error from the drop".to_string(),
            Caveat::CoaxialBeatError { why } => coaxial_beat_error_short(*why),
            Caveat::FaintSidesWithheld { sides, .. } => {
                format!("{sides} faint side{} left out", plural(*sides))
            }
            Caveat::SetRateDisagrees { heard } => format!("{heard} bph heard"),
            Caveat::OffStandardRate(_) => "not a standard rate".to_string(),
            Caveat::ShortSpan { span_s } => format!("{span_s:.0} s of beats"),
            Caveat::Clipped { fraction } => format!("{:.0}% clipped", fraction * 100.0),
            Caveat::BeatsMissing { fraction } => {
                let expected = (beats as f64 / fraction.max(1e-9)).round() as usize;
                if beats > 0 {
                    format!("{beats} of {expected} beats")
                } else {
                    format!("{:.0}% of beats found", fraction * 100.0)
                }
            }
            Caveat::LiftUnknown => lift_words(lift_deg, false),
            Caveat::CoaxialEstimate => COAXIAL_ESTIMATE_SHORT.to_string(),
            Caveat::ClockUnknown => "input clock not corrected".to_string(),
            Caveat::ClockSourcesDisagree { .. } => self.sources_disagree()?.short(),
            Caveat::UsbLinesNotUsed { ppm } => usb_not_used(*ppm).not_used_short()?,
            Caveat::Breaks { count } => format!("{count} break{}", plural(*count)),
            Caveat::BreaksUnlisted { .. } => "breaks from the clock log".to_string(),
            Caveat::PausesUnseen { count } => {
                format!(
                    "{count} listed pause{} not in the clock log",
                    plural(*count)
                )
            }
            Caveat::SidesByCount { .. } => "sides named by count".to_string(),
            Caveat::BeatErrorAsSize => {
                "beat error given as a size: its sign differs either side of a cut".to_string()
            }
            Caveat::Masked { seconds } => format!("{} s masked", masked_seconds(*seconds)?),
            Caveat::Positions { count } => format!(
                "{count} positions: a rate change with position can read as one with amplitude"
            ),
            Caveat::LeadLeftOut { seconds } => format!("first {seconds:.0} s left out"),
            Caveat::LockTemplateKept => "beat shape kept from lock-on".to_string(),
            Caveat::PeriodMayBeNoise { .. } => "cycle may be noise".to_string(),
            Caveat::PeriodSearchUncalibrated { .. } => {
                "cycle search not calibrated here".to_string()
            }
            Caveat::TimingModelMisfit { .. } => "timing noise not the simulated kind".to_string(),
        })
    }

    /// The caveat as a plain fact, a sentence, as the app words it in a
    /// reading's hover: what limits the reading, with no advice. `lift_deg`
    /// is the lift angle the amplitude used, when known. `None` as for
    /// [`Caveat::short`].
    pub fn long(&self, lift_deg: Option<f64>) -> Option<String> {
        Some(match self {
            Caveat::NoWatch { .. } => return None,
            Caveat::ReadingWindows { reading, windows_used, windows_in_span } =>
                format!("The {reading} was read from {windows_used} of {windows_in_span} windows in the kept time."),
            Caveat::BeatErrorSourceReadings { source, used, readings, counted } =>
                format!("The beat error uses the {source} in {used} of {readings} {c}; {c} from the other event were left out.", c = counted.plural()),
            Caveat::BeatErrorUnknownSource { readings } =>
                format!("The beat-error aggregate leaves out {readings} reading{} whose source was not recorded.", plural(*readings)),
            Caveat::BeatErrorFromDrop { why } => why.sentence().to_string(),
            Caveat::CoaxialBeatError { why } => coaxial_beat_error_long(*why),
            Caveat::FaintSidesWithheld { sides, windows } => faint_sides_text(*sides, *windows),
            Caveat::SetRateDisagrees { heard } => format!("The beat rate set differs from the {heard} bph heard."),
            Caveat::OffStandardRate(o) => off_standard_text(o),
            Caveat::ShortSpan { span_s } => format!(
                "The beats span {span_s:.0} s, under the {SHORT_SPAN_S:.0} s needed to judge \
                 whether they are regular."
            ),
            Caveat::Clipped { fraction } => format!(
                "{:.0}% of the beats have a clipped sample, which moves the timing of the \
                 sounds inside a beat.",
                fraction * 100.0
            ),
            Caveat::BeatsMissing { fraction } => format!(
                "{:.0}% of the beats the beat rate expects in this stretch were found.",
                fraction * 100.0
            ),
            Caveat::LiftUnknown => match lift_deg {
                Some(l) => format!(
                    "The lift angle is the default {l}°, not one set for the calibre. Every \
                     amplitude is worked out from it."
                ),
                None => "The lift angle is the default, not one set for the calibre. Every \
                         amplitude is worked out from it."
                    .to_string(),
            },
            Caveat::CoaxialEstimate => COAXIAL_ESTIMATE_LONG.to_string(),
            Caveat::ClockUnknown => CLOCK_UNKNOWN_LONG.to_string(),
            Caveat::ClockSourcesDisagree { .. } => {
                let note = self.sources_disagree()?.note();
                format!("{}{}.", note[..1].to_uppercase(), &note[1..])
            }
            Caveat::UsbLinesNotUsed { ppm } => usb_not_used(*ppm).not_used_text()?,
            Caveat::Breaks { count } => format!(
                "{count} break{} in this stretch: a hole in the sound, such as when the \
                 computer slept, or a stumble where the beats stepped off together by more \
                 than {:.0} ms.",
                plural(*count),
                crate::beats::JUMP_S * 1000.0
            ),
            Caveat::BreaksUnlisted { why } => {
                let what = match why {
                    crate::breaks::Unlisted::Unreadable => {
                        "This take's session.json could not be read"
                    }
                    crate::breaks::Unlisted::EntriesSkipped => {
                        "Some of the breaks this take's session.json lists give no place or gap"
                    }
                    crate::breaks::Unlisted::NotTheTake => {
                        "These files are not the whole of the take its session.json describes"
                    }
                };
                format!(
                    "{what}, so its breaks were read from the clock log instead, which shows \
                     a pause only where the time it lost is {:.1} s or more.",
                    crate::breaks::MIN_GAP_S
                )
            }
            Caveat::PausesUnseen { count } => format!(
                "{count} pause{} this take's session.json lists show{} no gap in its clock \
                 log, so no time is counted for {}.",
                plural(*count),
                if *count == 1 { "s" } else { "" },
                if *count == 1 { "it" } else { "them" }
            ),
            Caveat::SidesByCount { count } => format!(
                "Tick and Tock are named by the beat count across {count} break{} in this \
                 stretch, not told apart by their sound.",
                plural(*count)
            ),
            Caveat::BeatErrorAsSize => "The beat error's sign differs either side of a cut \
                                        in the stretch, so it is given as a size."
                .to_string(),
            Caveat::Masked { seconds } => format!(
                "{} s of this stretch were masked out and left out of the readings.",
                masked_seconds(*seconds)?
            ),
            Caveat::Positions { count } => format!(
                "The take holds {count} positions, so a rate that changes with the \
                 position can show as one that goes with the amplitude."
            ),
            Caveat::LeadLeftOut { seconds } => format!(
                "The first {seconds:.0} s held no regular beat before the watch was heard, \
                 and are left out of the readings."
            ),
            Caveat::LockTemplateKept => lock_template_kept_text(),
            Caveat::PeriodMayBeNoise { .. }
            | Caveat::PeriodSearchUncalibrated { .. }
            | Caveat::TimingModelMisfit { .. } => period_search_text(self),
        })
    }

    /// The two clocks of [`Caveat::ClockSourcesDisagree`]; `None` for any
    /// other caveat.
    fn sources_disagree(&self) -> Option<crate::clock::SourcesDisagree> {
        match *self {
            Caveat::ClockSourcesDisagree {
                usb_ppm,
                log_ppm,
                log_ppm_sd,
                against,
            } => Some(crate::clock::SourcesDisagree {
                usb_ppm,
                log_ppm,
                log_ppm_sd,
                against,
            }),
            _ => None,
        }
    }

    /// Whether the caveat is about the take's breaks: how many there are,
    /// where they were read from, or a listed pause the clock log did not
    /// see.
    pub fn is_about_breaks(&self) -> bool {
        matches!(
            self,
            Caveat::Breaks { .. } | Caveat::BreaksUnlisted { .. } | Caveat::PausesUnseen { .. }
        )
    }

    /// Where the caveat sits in a list.
    fn rank(&self) -> u8 {
        match self {
            Caveat::NoWatch { .. } => 0,
            Caveat::SetRateDisagrees { .. } => 1,
            Caveat::OffStandardRate(_) => 1,
            Caveat::ShortSpan { .. } => 2,
            Caveat::Clipped { .. } => 3,
            Caveat::BeatsMissing { .. } => 4,
            Caveat::LiftUnknown => 5,
            Caveat::CoaxialEstimate => 5,
            Caveat::ClockUnknown => 6,
            Caveat::ClockSourcesDisagree { .. } => 7,
            Caveat::UsbLinesNotUsed { .. } => 7,
            Caveat::Breaks { .. } => 8,
            Caveat::BreaksUnlisted { .. } => 9,
            Caveat::PausesUnseen { .. } => 10,
            Caveat::SidesByCount { .. } => 11,
            Caveat::Masked { .. } => 12,
            Caveat::BeatErrorAsSize => 13,
            Caveat::Positions { .. } => 14,
            Caveat::LeadLeftOut { .. } => 15,
            Caveat::LockTemplateKept => 15,
            Caveat::BeatErrorFromDrop { .. } => 16,
            Caveat::CoaxialBeatError { .. } => 16,
            Caveat::BeatErrorSourceReadings { .. } => 18,
            Caveat::BeatErrorUnknownSource { .. } => 19,
            Caveat::ReadingWindows { .. } => 17,
            Caveat::FaintSidesWithheld { .. } => 17,
            Caveat::PeriodMayBeNoise { .. } => 19,
            Caveat::PeriodSearchUncalibrated { .. } => 19,
            Caveat::TimingModelMisfit { .. } => 19,
        }
    }
}

impl From<crate::clock::SourcesDisagree> for Caveat {
    fn from(d: crate::clock::SourcesDisagree) -> Caveat {
        Caveat::ClockSourcesDisagree {
            usb_ppm: d.usb_ppm,
            log_ppm: d.log_ppm,
            log_ppm_sd: d.log_ppm_sd,
            against: d.against,
        }
    }
}

/// [`Caveat::UsbLinesNotUsed`]'s reading, to word it as the USB lines'
/// own ([`crate::usbclock::UsbClock::not_used_text`]).
fn usb_not_used(ppm: f64) -> crate::usbclock::UsbClock {
    crate::usbclock::UsbClock {
        ppm,
        lines: 0,
        spread_ppm: 0.0,
    }
}

/// A period-search caveat as a sentence.
fn period_search_text(c: &Caveat) -> String {
    match *c {
        Caveat::PeriodMayBeNoise { rho, span_s } => may_be_noise_text(rho, span_s),
        Caveat::PeriodSearchUncalibrated { model, mix, span_s } => {
            uncalibrated_text(model, mix, span_s)
        }
        Caveat::TimingModelMisfit { misfit, limit } => misfit_text(misfit, limit),
        _ => unreachable!("not a period-search caveat"),
    }
}

/// [`Caveat::PeriodMayBeNoise`] as a sentence.
fn may_be_noise_text(rho: f64, span_s: f64) -> String {
    use crate::longterm::{MEASURED_CROSSING_IN, SHORT_CORRELATED_CROSSING_IN};
    format!(
        "On a take this short with its 2 s windows this correlated (ρ {rho:.2} from one to \
         the next, over {:.0} min), the period search names a cycle in noise alone about 1 \
         time in {SHORT_CORRELATED_CROSSING_IN}, not 1 in {MEASURED_CROSSING_IN}: this cycle may be noise.",
        span_s / 60.0
    )
}

/// [`Caveat::TimingModelMisfit`] as a sentence.
fn misfit_text(misfit: f64, limit: f64) -> String {
    format!(
        "The beat timing's noise is not the wander plus jitter the period search was \
         checked against: its spectrum sits {misfit:.2} decades from that shape, where \
         99 in 100 simulated takes sit within {limit:.2}. How often it names a cycle in \
         this noise alone is not known: this cycle may be noise."
    )
}

/// [`Caveat::PeriodSearchUncalibrated`] as a sentence.
fn uncalibrated_text(model: crate::longterm::NullModel, mix: f64, span_s: f64) -> String {
    use crate::longterm::NullModel;
    let what = match model {
        NullModel::Readings => format!("readings correlated at ρ {mix:.2}"),
        NullModel::Timing if mix < crate::longterm::null::WALK_MIX => {
            format!("beat timing whose wander meets its jitter at {mix:.0} cycles a detrend window")
        }
        NullModel::Timing => "beat timing with no jitter floor to measure".to_string(),
    };
    format!(
        "The period search has no simulated noise for {:.1} h of {what}, so how often it \
         names a cycle in noise alone is not known here: this cycle may be noise.",
        span_s / 3600.0
    )
}

/// [`Caveat::ClockUnknown`] as a sentence.
const CLOCK_UNKNOWN_LONG: &str = "This input's clock hasn't been measured, so the rate includes \
     the sound card's own clock error.";

/// [`Caveat::CoaxialEstimate`] in a few words (the owner's, #2313).
pub const COAXIAL_ESTIMATE_SHORT: &str = "co-axial escapement estimate (beta)";

/// [`COAXIAL_ESTIMATE_SHORT`] shorter still, for a mark that has no room
/// for it (Hugh on #2319): the app's chip tries it next.
pub const COAXIAL_ESTIMATE_SHORTER: &str = "co-axial estimate (beta)";

/// A caveat's few words ([`Caveat::short`]) shorter still, where it has a
/// shorter form, for a mark too narrow for them: the one place each
/// limit's forms are kept.
pub fn shorter_words(short: &str) -> Option<&'static str> {
    (short == COAXIAL_ESTIMATE_SHORT).then_some(COAXIAL_ESTIMATE_SHORTER)
}

/// [`Caveat::CoaxialEstimate`] as a sentence.
pub const COAXIAL_ESTIMATE_LONG: &str = "The amplitude is a co-axial escapement estimate \
     (beta): it uses the Swiss lever's formula and has not been checked against a reference.";

/// [`Caveat::CoaxialBeatError`] in a few words, short enough for a card's
/// chip at 1920 wide (Walter on #2317): that the take is read for a
/// co-axial, or why it has no beat error.
fn coaxial_beat_error_short(why: Option<DropWhy>) -> String {
    match why {
        None => "co-axial (beta)".to_string(),
        Some(w) => format!("co-axial: {}", w.words()),
    }
}

/// [`Caveat::CoaxialBeatError`] as a sentence.
fn coaxial_beat_error_long(why: Option<DropWhy>) -> String {
    let lead = "Read for a co-axial escapement (beta): the beat error is the unlock's alone";
    let lead_none =
        "Read for a co-axial escapement (beta): the beat error is taken from the unlock only";
    match why {
        None => format!(
            "{lead}. The drop's is not used: on a co-axial the two sides end on different \
             events."
        ),
        Some(DropWhy::UnlockNotFound) => {
            format!("{lead_none}, and the unlock was not found, so none is given.")
        }
        Some(DropWhy::UnlockInconsistent) => format!(
            "{lead_none}, and the unlock's beat errors disagree in sign more than their \
             spread allows, so none is given."
        ),
    }
}

/// The lift angle an amplitude was worked out from, as every reading
/// beside an amplitude says it: "lift angle 52°", or "lift angle 52°, the
/// default" when neither the calibre nor the user set it (#1881).
pub fn lift_words(lift_deg: f64, known: bool) -> String {
    if known {
        format!("lift angle {lift_deg}°")
    } else {
        format!("lift angle {lift_deg}°, the default")
    }
}

/// The "s" an English noun takes after a count of `n`: none after 1.
/// The one rule for every count the app, CLI and MCP word (#1366).
pub fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// Masked seconds as the words give them: a tenth under 10 s, so a mask
/// reaching just into a stretch doesn't read 0, and nothing under 0.05 s,
/// where that would read 0.0.
fn masked_seconds(s: f64) -> Option<String> {
    if s < 0.05 {
        None
    } else if s < 9.95 {
        Some(format!("{s:.1}"))
    } else {
        Some(format!("{s:.0}"))
    }
}

/// Add a caveat in its place, once.
pub fn add(caveats: &mut Vec<Caveat>, c: Caveat) {
    if !caveats.contains(&c) {
        caveats.push(c);
        caveats.sort_by_key(Caveat::rank);
    }
}

/// What a stretch of beats was measured on, beyond its readings.
#[derive(Debug, Clone, Copy, Default)]
pub struct Context {
    /// See [`watch_heard`].
    pub watch_heard: bool,
    /// What no regular beat was found on, when `watch_heard` is false.
    pub no_watch_reason: NoWatchReason,
    /// The beat rate heard, when a set one differs from it.
    pub bph_heard: Option<u32>,
    pub lift_known: bool,
    pub clock_known: bool,
    /// Beats found, and the beats the stretch should hold.
    pub beats: usize,
    pub expected: f64,
    /// Share of the beats with a clipped sample.
    pub clipped_fraction: f64,
    /// Seconds from the first beat to the last.
    pub span_s: f64,
    /// Seconds of sound the beats were sought in: the recording, the
    /// stretch, or the live screen's averaging time so far.
    pub listened_s: f64,
    pub breaks: usize,
    /// Of `breaks`, those where the sides were kept by the beat count.
    pub sides_by_count: usize,
    /// The beat error is given as a size ([`Caveat::BeatErrorAsSize`]).
    pub beat_error_as_size: bool,
    /// The seconds a shown rate is averaged over, when shorter than the
    /// stretch (the live screen's averaging time). `None`: the rate is the
    /// fit over the whole stretch.
    pub average_s: Option<f64>,
}

/// The length of a rate window, s: rate and timing-scatter fits use ten
/// seconds to collect enough beats. The one length every reading uses
/// (#512).
pub const RATE_WINDOW_S: f64 = 10.0;
/// The spacing of the rate windows, s: half a window, so they half
/// overlap.
pub const RATE_WINDOW_STEP_S: f64 = RATE_WINDOW_S / 2.0;

/// The beats the rate's ± needs in the take, s: the windows half overlap
/// and [`confidence::rate_se`] takes every other one, so its
/// [`confidence::MIN_NOISE_VALUES`] need that many window lengths
/// (#1470). A shorter reading borrows the scatter of those nearest it.
pub const RATE_SE_MIN_S: f64 = confidence::MIN_NOISE_VALUES as f64 * RATE_WINDOW_S;

/// The rate's ± for a figure averaged over `average_s` within a run whose
/// rate windows are given: the scatter of the 10 s rates over the number
/// of them averaged ([`confidence::rate_se_of_average_or_why`]), with no
/// allowance for a drift over the run, which isn't uncertainty about the
/// last `average_s` (#1950). With no lag-one correlation it is the
/// standard error [`confidence::rate_se`] gives over the whole run. Its
/// noise is borrowed from `take` as [`confidence::rate_se`]'s.
pub fn rate_se_over(rate_windows: &[WindowFit], take: &[WindowFit], average_s: f64) -> Option<f64> {
    rate_se_over_or_why(rate_windows, take, average_s).ok()
}

/// [`rate_se_over`], or why there is none (#1908).
pub fn rate_se_over_or_why(
    rate_windows: &[WindowFit],
    take: &[WindowFit],
    average_s: f64,
) -> Result<f64, confidence::NoPm> {
    let independent = confidence::rate_value_count(rate_windows.len()) as f64;
    let averaged = (average_s / RATE_WINDOW_S).clamp(1.0, independent.max(1.0));
    confidence::rate_se_of_average_or_why(rate_windows, take, averaged)
}

/// What one measurement rests on.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Assessment {
    /// Each reading's ±; `None` when no regular beat is found.
    pub confidence: Option<Confidence>,
    pub caveats: Vec<Caveat>,
    pub doubts: Doubts,
}

/// The take's windows a reading's ± borrows its noise from where it holds
/// fewer than [`confidence::MIN_NOISE_VALUES`] values (#1470): its 10 s
/// rate windows (half overlapping) and its 2 s amplitude windows, each in
/// time order, outside the masks.
#[derive(Debug, Clone, Copy)]
pub struct TakeWindows<'a> {
    pub rate: &'a [WindowFit],
    pub amplitude: &'a [&'a AmplitudeWindow],
}

/// State what a measurement rests on, from its 10 s rate windows (half
/// overlapping) and its 2 s amplitude windows, when they are the whole
/// take: [`assess_in`] borrowing from no more than them.
pub fn assess(
    rate_windows: &[WindowFit],
    amplitude_windows: &[&AmplitudeWindow],
    c: &Context,
) -> Assessment {
    let take = TakeWindows {
        rate: rate_windows,
        amplitude: amplitude_windows,
    };
    assess_in(rate_windows, amplitude_windows, take, c)
}

/// State what a measurement rests on, from its 10 s rate windows (half
/// overlapping) and its 2 s amplitude windows, its ± borrowing noise from
/// `take` where they hold too few values (#1470).
pub fn assess_in(
    rate_windows: &[WindowFit],
    amplitude_windows: &[&AmplitudeWindow],
    take: TakeWindows,
    c: &Context,
) -> Assessment {
    let found = if c.expected > 0.0 {
        c.beats as f64 / c.expected
    } else {
        1.0
    };
    let doubts = Doubts {
        clipped: crate::analysis::Clipping::noted_fraction(c.clipped_fraction),
        beats_missing: beats_missing(found),
        lift_unknown: !c.lift_known,
        clock_unknown: !c.clock_known,
    };
    let mut caveats = Vec::new();
    if !c.watch_heard {
        caveats.push(Caveat::NoWatch {
            reason: c.no_watch_reason,
        });
    }
    if let Some(heard) = c.bph_heard {
        caveats.push(Caveat::SetRateDisagrees { heard });
    }
    if c.span_s < SHORT_SPAN_S {
        caveats.push(Caveat::ShortSpan { span_s: c.span_s });
    }
    if doubts.clipped {
        caveats.push(Caveat::Clipped {
            fraction: c.clipped_fraction,
        });
    }
    if doubts.beats_missing {
        caveats.push(Caveat::BeatsMissing { fraction: found });
    }
    if doubts.lift_unknown {
        caveats.push(Caveat::LiftUnknown);
    }
    if doubts.clock_unknown {
        caveats.push(Caveat::ClockUnknown);
    }
    if c.breaks > 0 {
        caveats.push(Caveat::Breaks { count: c.breaks });
    }
    if c.sides_by_count > 0 {
        caveats.push(Caveat::SidesByCount {
            count: c.sides_by_count,
        });
    }
    if c.beat_error_as_size && c.watch_heard {
        caveats.push(Caveat::BeatErrorAsSize);
    }
    // Sides withheld as too faint to read, among the measurement's own
    // windows (#1738).
    let faint = amplitude_windows.iter().map(|w| w.faint_withheld());
    let sides = faint.clone().flatten().filter(|&f| f).count();
    let windows = faint.filter(|f| f.contains(&true)).count();
    if sides > 0 && c.watch_heard {
        caveats.push(Caveat::FaintSidesWithheld { sides, windows });
    }
    let confidence = c.watch_heard.then(|| Confidence {
        rate_pool_values: confidence::rate_pool_values(take.rate),
        ..Confidence::of(
            match c.average_s {
                Some(t) => rate_se_over_or_why(rate_windows, take.rate, t),
                None => confidence::rate_se_or_why(rate_windows, take.rate),
            },
            confidence::amplitude_windows_se_or_why(
                amplitude_windows.iter().copied(),
                take.amplitude,
                |w| w.mean(),
            ),
            confidence::beat_error_se_or_why(
                amplitude_windows.iter().copied(),
                take.amplitude,
                c.beat_error_as_size,
            ),
        )
    });
    Assessment {
        confidence,
        caveats,
        doubts,
    }
}

#[cfg(test)]
mod tests {

    /// [`too_short`] tests the longest run begun from `from_s`: a regular
    /// 9 s run, then after a pause a 2 s burst at no steady rate, is too
    /// short by the 9 s run; from inside the 9 s run, only the burst
    /// counts, and it is not regular (Vernon on #2311).
    #[test]
    fn too_short_tests_the_longest_run_begun_from_its_start() {
        let beat = |index: i64, time: f64| crate::beats::Beat {
            index,
            time,
            quality: 1.0,
        };
        let mut beats: Vec<_> = (0..72)
            .map(|i| beat(i, f64::from(i as i32) * 0.125))
            .collect();
        // Offsets of up to 40% of a beat, as no watch beats.
        let jumble = [0.0, 0.05, -0.04, 0.03, -0.05, 0.045, -0.02, 0.04];
        beats.extend((0..16).map(|i| {
            let k = 112 + i;
            beat(k, f64::from(k as i32) * 0.125 + jumble[i as usize % 8])
        }));
        assert!(too_short(None, &beats, f64::NEG_INFINITY, 28_800));
        assert!(!too_short(None, &beats, 1.0, 28_800));
        assert!(!too_short(Some(100.0), &beats, f64::NEG_INFINITY, 28_800));
    }
    /// The Finding the Beat hover names the wait for the Average Over in
    /// use, in the Measure field's words (Hugh on #2187, #2178).
    #[test]
    fn finding_text_names_the_wait_for_the_average() {
        assert_eq!(
            super::finding_text(10.0),
            "A regular beat is heard. The figures wait until the last 20 s is mostly \
             the watch's, so the sound before it isn't counted against it."
        );
        assert_eq!(
            super::finding_text(60.0),
            "A regular beat is heard. The figures wait until the last 1 min is mostly \
             the watch's, so the sound before it isn't counted against it."
        );
    }

    /// Beat error with the sides not told apart says what it waits for,
    /// in Hugh 5's words, and only where the reading has figures but no
    /// beat error from either edge (#2133).
    #[test]
    fn beat_error_waits_for_the_tick_and_the_tock() {
        use super::{beat_error_wait, Wait};
        let w = Wait::TickAndTock;
        assert_eq!(w.title(), "Waiting for Tick and Tock");
        assert_eq!(
            w.text(),
            "Beat error needs the Tick and the Tock told apart: reading starts once they are."
        );
        assert_eq!(beat_error_wait(Some(3.0), None, None), Some(w));
        assert_eq!(beat_error_wait(None, None, None), None, "no figures at all");
        assert_eq!(
            beat_error_wait(Some(3.0), Some(0.2), None),
            None,
            "from the drop"
        );
        assert_eq!(beat_error_wait(Some(3.0), None, Some(0.2)), None);
    }

    /// Each wait has its short words and a sentence saying what the
    /// figures need; a settle counts whole seconds up, so "0 s" never
    /// shows while something is still left (#2007).
    #[test]
    fn every_wait_says_what_the_figures_need() {
        use super::Wait;
        let says = |w: Wait| (w.title(), w.text());
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(
            says(Wait::Settling { left_s: 15.2 }),
            pair(
                "Settling, 16 s",
                "The figures need the watch settled in its new position: reading starts in 16 s."
            )
        );
        assert_eq!(Wait::Settling { left_s: 0.3 }.title(), "Settling, 1 s");
        assert_eq!(
            says(Wait::NoBeats),
            pair(
                "Waiting for Beats",
                "The figures need a watch's beats: none are heard yet."
            )
        );
        assert_eq!(
            says(Wait::MoreBeats),
            pair(
                "Waiting for More Beats",
                "This figure needs more beats than the stretch holds yet: it appears once they \
                 have been heard."
            )
        );
        assert_eq!(
            says(Wait::ShortHeard {
                since: HeardSince::Start
            }),
            pair(
                "Under 10 s Heard",
                "The figures need 10 s of sound: less than 10 s was heard since the start."
            )
        );
        assert_eq!(
            says(Wait::ShortHeard {
                since: HeardSince::Break
            })
            .1,
            "The figures need 10 s of sound: less than 10 s was heard since the break."
        );
        assert_eq!(
            wait_short_title("Under 10 s Heard").as_deref(),
            Some("Under 10 s")
        );
        assert_eq!(wait_short_title("No Beats Heard"), None);
        assert_eq!(
            says(Wait::TestStart),
            pair(
                "Waiting for Start",
                "The figures need the test started: Start Test on the Listen card starts it."
            )
        );
        assert_eq!(
            says(Wait::Analysing),
            pair(
                "Analysing",
                "The figures need the recording read: they appear once Analysing is done."
            )
        );
        assert_eq!(
            says(Wait::NoBeatsHeard),
            pair(
                "No Beats Heard",
                "The figures need a watch's beats: none were heard in this stretch."
            )
        );
        assert_eq!(
            says(Wait::NotListening),
            pair(
                "Not Listening",
                "The figures need sound: nothing is being listened to."
            )
        );
    }

    #[test]
    fn a_short_average_says_its_amplitude_is_read_over_ten_seconds() {
        assert_eq!(
            amplitude_span_note(5.0).as_deref(),
            Some("Over the last 10 s, not 5 s: amplitude needs at least 10 s of beats.")
        );
        assert_eq!(
            unlock_span_note(2.0).as_deref(),
            Some("Over the last 10 s, not 2 s: the unlock needs at least 10 s of beats.")
        );
        assert_eq!(amplitude_span_note(SHORT_SPAN_S), None);
        assert_eq!(unlock_span_note(30.0), None);
    }

    /// The table of which cards each caveat limits (Norm and Hugh, #1975):
    /// a caveat with one card is said there, any other for the whole take.
    #[test]
    fn each_caveat_limits_the_cards_ruled() {
        use confidence::Figure;
        use Scope::{Amplitude, BeatError, Rate, Take};
        let no_watch = Caveat::NoWatch {
            reason: NoWatchReason::Scatter,
        };
        let set_rate = Caveat::SetRateDisagrees { heard: 28800 };
        for take in [&no_watch, &set_rate] {
            assert_eq!(take.scopes(), &[Take], "{take:?}");
        }
        let lift_and_sides = [
            Caveat::LiftUnknown,
            Caveat::Clipped { fraction: 0.02 },
            Caveat::FaintSidesWithheld {
                sides: 1,
                windows: 1,
            },
        ];
        for c in &lift_and_sides {
            assert_eq!(c.scopes(), &[Amplitude, BeatError], "{c:?}");
        }
        assert_eq!(Caveat::ClockUnknown.scopes(), &[Rate]);
        assert_eq!(Caveat::FROM_DROP.scopes(), &[BeatError]);
        for (figure, scope) in [
            (Figure::Rate, Rate),
            (Figure::Amplitude, Amplitude),
            (Figure::BeatError, BeatError),
        ] {
            let c = Caveat::ReadingWindows {
                reading: figure,
                windows_used: 1,
                windows_in_span: 4,
            };
            assert_eq!(c.scopes(), &[scope]);
            assert_eq!(c.reading_scope(), Some(scope));
        }
        for c in lift_and_sides.iter().chain([&no_watch, &set_rate]) {
            assert_eq!(c.reading_scope(), None, "{c:?}");
        }
        assert_eq!(Caveat::ClockUnknown.reading_scope(), Some(Rate));
        assert_eq!(Caveat::FROM_DROP.reading_scope(), Some(BeatError));
    }

    use super::*;

    /// The not-assessed fact names the sound heard and the span needed.
    #[test]
    fn the_not_assessed_fact_names_the_sound_and_the_span() {
        assert_eq!(
            not_assessed_text(5.0),
            "Beat regularity not assessed: 5.0 s of sound, 10 s needed"
        );
    }

    /// The kept-template caveat names the first chunk's length from
    /// `stream` and the lock-on window's from `live`, not numbers of its
    /// own (#1500, #1503).
    #[test]
    fn faint_sides_withheld_is_worded_with_its_count() {
        let one = Caveat::FaintSidesWithheld {
            sides: 1,
            windows: 1,
        };
        // Both sides of one window, and one side of another.
        let three = Caveat::FaintSidesWithheld {
            sides: 3,
            windows: 2,
        };
        assert_eq!(one.short(52.0, 0).as_deref(), Some("1 faint side left out"));
        assert_eq!(
            three.short(52.0, 0).as_deref(),
            Some("3 faint sides left out")
        );
        assert_eq!(
            one.long(None).as_deref(),
            Some(
                "1 side in 1 window had an unlock sound too faint to tell from noise: under \
                 0.28 of that side's usual level nearby. It is left out of that window's \
                 amplitude and beat error."
            )
        );
        assert_eq!(
            three.long(None).as_deref(),
            Some(
                "3 sides in 2 windows had an unlock sound too faint to tell from noise: under \
                 0.28 of that side's usual level nearby. They are left out of those windows' \
                 amplitude and beat error."
            )
        );
    }

    #[test]
    fn the_kept_template_caveat_names_the_first_chunk() {
        let c = Caveat::LockTemplateKept;
        assert_eq!(
            c.short(52.0, 0).as_deref(),
            Some("beat shape kept from lock-on")
        );
        let chunk_s = crate::stream::StreamConfig::default().chunk_s;
        let lock_s = crate::live::LiveConfig::default().window_s;
        let long = c.long(None).unwrap();
        assert!(
            long.contains(&format!("first {chunk_s:.0} s found")),
            "{long}"
        );
        assert!(
            long.contains(&format!("first {lock_s:.0} s of sound")),
            "{long}"
        );
    }

    /// A beat error from the drop because the unlock was not found writes
    /// no reason, so saved readings stay as they were and read back as not
    /// found; one whose unlock was inconsistent writes its reason (#1439).
    #[test]
    fn a_drop_caveat_writes_its_reason_only_when_inconsistent() {
        let json = |c: &Caveat| serde_json::to_string(c).unwrap();
        let inconsistent = Caveat::BeatErrorFromDrop {
            why: DropWhy::UnlockInconsistent,
        };
        assert_eq!(
            json(&Caveat::FROM_DROP),
            r#"{"code":"beat_error_from_drop"}"#
        );
        assert_eq!(
            json(&inconsistent),
            r#"{"code":"beat_error_from_drop","why":"unlock_inconsistent"}"#
        );
        let read = |s: &str| serde_json::from_str::<Caveat>(s).unwrap();
        assert_eq!(
            read(r#"{"code":"beat_error_from_drop"}"#),
            Caveat::FROM_DROP
        );
        assert_eq!(read(&json(&inconsistent)), inconsistent);
    }

    fn heard() -> Context {
        Context {
            watch_heard: true,
            lift_known: true,
            clock_known: true,
            beats: 100,
            expected: 100.0,
            span_s: 60.0,
            listened_s: 60.0,
            ..Default::default()
        }
    }

    /// The faint-sides caveat counts the sides and the windows they sit in
    /// from the reading's own windows (#1738, Trevor on #1800): one window
    /// with both sides withheld and one with one side give 3 sides in 2
    /// windows; a window with none adds nothing.
    #[test]
    fn faint_sides_are_counted_by_side_and_by_window() {
        let window = |i: u32, faint: [bool; 2]| {
            let mut w = AmplitudeWindow {
                start_s: 2.0 * f64::from(i),
                end_s: 2.0 * f64::from(i) + 2.0,
                even_deg: (!faint[0]).then_some(250.0),
                odd_deg: (!faint[1]).then_some(250.0),
                beat_error_ms: Some(0.3),
                beat_error_unlock_ms: None,
                even_unlock_ms: None,
                odd_unlock_ms: None,
                even_drop_ms: None,
                odd_drop_ms: None,
                extra: None,
                sound_1: [f32::NAN; 2],
            };
            w.edit_extra(|x| x.faint_withheld = faint);
            w
        };
        let wins = [
            window(0, [true, true]),
            window(1, [false, false]),
            window(2, [false, true]),
        ];
        let refs: Vec<&AmplitudeWindow> = wins.iter().collect();
        let a = assess(&[], &refs, &heard());
        let faint: Vec<&Caveat> = a
            .caveats
            .iter()
            .filter(|c| matches!(c, Caveat::FaintSidesWithheld { .. }))
            .collect();
        assert_eq!(
            faint,
            [&Caveat::FaintSidesWithheld {
                sides: 3,
                windows: 2
            }]
        );
        // None withheld: no caveat.
        let clean = [window(0, [false, false])];
        let a = assess(&[], &[&clean[0]], &heard());
        assert!(!a
            .caveats
            .iter()
            .any(|c| matches!(c, Caveat::FaintSidesWithheld { .. })));
    }

    #[test]
    fn a_clean_stretch_has_no_caveats() {
        let a = assess(&[], &[], &heard());
        assert!(a.caveats.is_empty());
        assert!(a.confidence.is_some());
    }

    #[test]
    fn the_no_watch_reason_is_read_from_its_caveat_only() {
        assert_eq!(no_watch_reason_in(&[]), None);
        assert_eq!(
            no_watch_reason_in(&[Caveat::SetRateDisagrees { heard: 28_800 }]),
            None
        );
        assert_eq!(
            no_watch_reason_in(&[
                Caveat::SetRateDisagrees { heard: 28_800 },
                Caveat::NoWatch {
                    reason: NoWatchReason::NoBeatSound
                },
            ]),
            Some(NoWatchReason::NoBeatSound)
        );
    }

    #[test]
    fn no_regular_beat_withholds_the_readings() {
        let a = assess(
            &[],
            &[],
            &Context {
                watch_heard: false,
                ..heard()
            },
        );
        assert_eq!(
            a.caveats,
            vec![Caveat::NoWatch {
                reason: NoWatchReason::Scatter
            }]
        );
        assert!(a.confidence.is_none());
    }

    #[test]
    fn no_regular_beat_says_nothing_of_a_beat_error_size() {
        // A cut take with no regular beat has no beat error to call a size.
        let a = assess(
            &[],
            &[],
            &Context {
                watch_heard: false,
                beat_error_as_size: true,
                ..heard()
            },
        );
        assert_eq!(
            a.caveats,
            vec![Caveat::NoWatch {
                reason: NoWatchReason::Scatter
            }]
        );
        let a = assess(
            &[],
            &[],
            &Context {
                beat_error_as_size: true,
                ..heard()
            },
        );
        assert_eq!(a.caveats, vec![Caveat::BeatErrorAsSize]);
    }

    #[test]
    fn a_short_stretch_is_named_and_keeps_its_readings() {
        let a = assess(
            &[],
            &[],
            &Context {
                span_s: 6.0,
                ..heard()
            },
        );
        assert_eq!(a.caveats, vec![Caveat::ShortSpan { span_s: 6.0 }]);
        assert!(a.confidence.is_some());
    }

    /// Rate windows 5 s apart, alternately reading `rates`' values.
    fn rate_windows(rates: &[f64]) -> Vec<WindowFit> {
        rates
            .iter()
            .flat_map(|&r| [r, r])
            .enumerate()
            .map(|(i, r)| WindowFit {
                start_s: i as f64 * 5.0,
                end_s: i as f64 * 5.0 + 10.0,
                fit: crate::timing::TimingFit {
                    rate_s_per_day: r,
                    beat_error_ms: 0.0,
                    period_s: 0.125,
                    jitter_us: 0.0,
                    beats_used: 80,
                },
            })
            .collect()
    }

    #[test]
    fn a_short_average_has_a_wider_rate_error() {
        let rates: Vec<f64> = (0..36).map(|i| [1.0, -1.0, 0.5, -0.5][i % 4]).collect();
        let w = rate_windows(&rates);
        let whole = confidence::rate_se(&w, &w).unwrap();
        // Averaged over the whole run, it is the standard error analyze uses.
        assert_eq!(rate_se_over(&w, &w, 360.0), Some(whole));
        assert_eq!(rate_se_over(&w, &w, 3600.0), Some(whole));
        // A 10 s average carries one window's scatter: six times wider
        // with 36 independent windows.
        let ten = rate_se_over(&w, &w, 10.0).unwrap();
        assert!((ten / whole - 6.0).abs() < 1e-9, "{ten} against {whole}");
        let with = |average_s| {
            assess(
                &w,
                &[],
                &Context {
                    average_s,
                    ..heard()
                },
            )
            .confidence
            .unwrap()
            .rate
            .se
        };
        assert_eq!(with(None), Some(whole));
        assert_eq!(with(Some(10.0)), Some(ten));
    }

    /// `n` rates for [`rate_windows`], with a scatter of about 2 s/d and,
    /// with `hump`, a drift over them like a watch settling after a move:
    /// down 15 s/d and back, as CD did on the Datejust, slowly enough that
    /// the differences between neighbours barely change (#1950).
    fn settling(n: usize, hump: bool) -> Vec<f64> {
        (0..n)
            .map(|k| {
                let noise = [2.0, -2.0, 1.0, -1.0, 0.0][k % 5];
                let x = k as f64 / n as f64;
                let drift = if hump {
                    -15.0 * (std::f64::consts::PI * x).sin()
                } else {
                    0.0
                };
                noise + drift
            })
            .collect()
    }

    /// A set average's rate ± is the 10 s rates' scatter over how many it
    /// averages, with no allowance for a drift over the run (#1950): a
    /// hump in the values it borrows its noise from leaves a 10 s
    /// reading's ± where a steady run puts it, where the run's own ± (a
    /// whole stop's) still widens for the drift, as it should for a mean.
    #[test]
    fn a_drift_in_the_pool_leaves_a_ten_second_average_its_scatter() {
        let flat = rate_windows(&settling(30, false));
        let humped = rate_windows(&settling(30, true));
        // A reading 5 values into its run, its noise from the 30 nearest.
        let own = |w: &[WindowFit]| w[w.len() - 10..].to_vec();
        let ten = |w: &[WindowFit]| rate_se_over(&own(w), w, 10.0).unwrap();
        let (a, b) = (ten(&flat), ten(&humped));
        // Within the 6% the hump adds to the neighbours' differences.
        assert!((b / a - 1.0).abs() < 0.07, "{b} against {a}");
        // Its scatter, not a multiple of it: 1.2533 s at one value, s
        // about 2.1.
        assert!(b < 3.0 * 2.2, "{b}");
        // The whole run's ± carries the drift's correlation.
        let whole = |w: &[WindowFit]| confidence::rate_se(w, w).unwrap();
        assert!(
            whole(&humped) > 3.0 * whole(&flat),
            "{} {}",
            whole(&humped),
            whole(&flat)
        );
    }

    /// A whole stop's rate ± (no Average Over, as `session` and Dynamic
    /// Poise read a stop) is unchanged by #1950: a wandering stop keeps
    /// the ± its correlation gives, pinned here as main gave it.
    #[test]
    fn a_wandering_stops_rate_error_is_its_own() {
        let w = rate_windows(&settling(30, true));
        let whole = confidence::rate_se(&w, &w).unwrap();
        assert!((whole - WANDERING_STOP_SE).abs() < 1e-9, "{whole}");
        let a = assess(
            &w,
            &[],
            &Context {
                average_s: None,
                ..heard()
            },
        );
        assert_eq!(a.confidence.unwrap().rate.se, Some(whole));
    }

    /// [`a_wandering_stops_rate_error_is_its_own`]'s ±, s/d, as main
    /// (f9d3626) read it before #1950.
    const WANDERING_STOP_SE: f64 = 9.223_227_759_400_327;

    #[test]
    fn under_three_windows_the_rate_error_is_unknown() {
        assert_eq!(
            rate_se_over(
                &rate_windows(&[1.0, -1.0]),
                &rate_windows(&[1.0, -1.0]),
                10.0
            ),
            None
        );
    }

    /// Under 10 s of sound, regularity is not assessed: however scattered
    /// the beats, a regular beat is not said to be missing, the readings
    /// stay, and the short span is named.
    #[test]
    fn a_stretch_too_short_to_assess_keeps_its_readings() {
        assert!(!regularity_assessed(8.0, true));
        assert!(regularity_assessed(10.0, true));
        // A stretch long enough that holds no whole scatter window is not
        // assessed either (#1688).
        assert!(!regularity_assessed(14.0, false));
        // Scatter of 8.6 ms at 21,600 bph: no regular beat over 10 s or
        // more, not assessed under it.
        let assessed = |s: f64| regularity_assessed(s, true);
        assert!(!watch_heard(Some(8600.0), 21_600, assessed(60.0)));
        assert!(watch_heard(Some(8600.0), 21_600, assessed(8.0)));
        assert!(watch_heard(None, 21_600, assessed(8.0)));
        let a = assess(
            &[],
            &[],
            &Context {
                watch_heard: watch_heard(Some(8600.0), 21_600, assessed(8.0)),
                span_s: 8.0,
                listened_s: 8.0,
                ..heard()
            },
        );
        assert_eq!(a.caveats, vec![Caveat::ShortSpan { span_s: 8.0 }]);
        assert!(a.confidence.is_some());
    }

    /// Where noise can still scatter as little (#675), and so can a weak
    /// watch about to stop, the timing says a watch unless the sound shows
    /// none, as under 10 s.
    #[test]
    fn in_the_doubt_zone_only_a_silent_beat_sound_is_no_watch() {
        // 1.5% of a beat at 28,800 bph.
        let doubt = Some(0.015 * 125_000.0);
        let reason = |s, sound| no_watch_reason(doubt, 28_800, regularity_assessed(s, true), sound);
        for s in [5.0, 30.0] {
            assert_eq!(reason(s, Some(true)), None);
            assert_eq!(reason(s, None), None);
            assert_eq!(reason(s, Some(false)), Some(NoWatchReason::NoBeatSound));
        }
        // Below the zone the timing alone says a watch, and at the limit
        // no watch, whatever the sound.
        let at = |f: f64, sound| no_watch_reason(Some(f * 125_000.0), 28_800, true, sound);
        assert_eq!(at(0.005, Some(false)), None);
        assert_eq!(at(0.02, Some(true)), Some(NoWatchReason::Scatter));
    }

    /// Beats too scattered to make 10 s of a run, in a minute of sound, as
    /// on an empty stand heard live: no watch, not too short to tell.
    #[test]
    fn scattered_beats_in_long_sound_are_no_watch() {
        let a = assess(
            &[],
            &[],
            &Context {
                watch_heard: false,
                span_s: 0.0,
                ..heard()
            },
        );
        assert_eq!(
            a.caveats,
            vec![
                Caveat::NoWatch {
                    reason: NoWatchReason::Scatter
                },
                Caveat::ShortSpan { span_s: 0.0 }
            ]
        );
    }

    #[test]
    fn each_doubt_is_named() {
        let a = assess(
            &[],
            &[],
            &Context {
                clipped_fraction: 0.05,
                beats: 90,
                lift_known: false,
                clock_known: false,
                bph_heard: Some(21_600),
                breaks: 1,
                ..heard()
            },
        );
        let codes: Vec<String> = a
            .caveats
            .iter()
            .map(|c| {
                serde_json::to_value(c).unwrap()["code"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            codes,
            [
                "set_rate_disagrees",
                "clipped",
                "beats_missing",
                "lift_unknown",
                "clock_unknown",
                "breaks"
            ]
        );
    }

    #[test]
    fn a_pause_the_clock_log_did_not_see_is_worded() {
        let unseen = |count| Caveat::PausesUnseen { count };
        assert_eq!(
            unseen(1).short(52.0, 0).unwrap(),
            "1 listed pause not in the clock log"
        );
        assert_eq!(
            unseen(1).long(None).unwrap(),
            "1 pause this take's session.json lists shows no gap in its clock log, so no \
             time is counted for it."
        );
        assert_eq!(
            unseen(2).long(None).unwrap(),
            "2 pauses this take's session.json lists show no gap in its clock log, so no \
             time is counted for them."
        );
    }

    #[test]
    fn caveats_are_worded_once_for_every_surface() {
        let unread = Caveat::BreaksUnlisted {
            why: crate::breaks::Unlisted::Unreadable,
        };
        assert_eq!(unread.short(52.0, 0).unwrap(), "breaks from the clock log");
        assert_eq!(
            unread.long(None).unwrap(),
            "This take's session.json could not be read, so its breaks were read from the \
             clock log instead, which shows a pause only where the time it lost is 0.5 s or \
             more."
        );
        assert_eq!(
            serde_json::to_value(unread).unwrap(),
            serde_json::json!({ "code": "breaks_unlisted", "why": "unreadable" })
        );
        assert_eq!(
            Caveat::Breaks { count: 1 }.short(52.0, 0).unwrap(),
            "1 break"
        );
        assert_eq!(
            Caveat::Breaks { count: 2 }.short(52.0, 0).unwrap(),
            "2 breaks"
        );
        assert_eq!(
            Caveat::BeatsMissing { fraction: 0.5 }
                .short(52.0, 400)
                .unwrap(),
            "400 of 800 beats"
        );
        assert_eq!(
            Caveat::LiftUnknown.short(52.0, 0).unwrap(),
            "lift angle 52°, the default"
        );
        assert_eq!(
            Caveat::Masked { seconds: 12.4 }.short(0.0, 0).unwrap(),
            "12 s masked"
        );
        // Says what the positions limit (#734).
        assert_eq!(
            Caveat::Positions { count: 2 }.short(0.0, 0).unwrap(),
            "2 positions: a rate change with position can read as one with amplitude"
        );
        assert_eq!(
            Caveat::Masked { seconds: 0.3 }.short(0.0, 0).unwrap(),
            "0.3 s masked"
        );
        assert_eq!(Caveat::Masked { seconds: 0.02 }.short(0.0, 0), None);
        assert_eq!(
            Caveat::NoWatch {
                reason: NoWatchReason::default()
            }
            .long(None),
            None
        );
        assert_eq!(
            Caveat::SidesByCount { count: 1 }.long(None).unwrap(),
            "Tick and Tock are named by the beat count across 1 break in this stretch, not \
             told apart by their sound."
        );
        assert!(Caveat::LiftUnknown
            .long(Some(52.0))
            .unwrap()
            .contains("default 52°"));
        // Both clocks' figures, and which one corrects (#498).
        let d = Caveat::ClockSourcesDisagree {
            usb_ppm: 120.0,
            log_ppm: 20.0,
            log_ppm_sd: 2.0,
            against: Default::default(),
        };
        assert_eq!(
            d.short(52.0, 0).unwrap(),
            "USB lines and clock log 100 ppm apart"
        );
        assert_eq!(
            d.long(None).unwrap(),
            "The USB lines measure the sound card at +120.0 ppm and the clock log at +20.0 ± 2.0 \
             ppm; the rates are corrected by the USB lines."
        );
        // Its JSON leaves the clock log unsaid, as before (#635).
        let json = serde_json::to_value(d).unwrap();
        assert!(json.get("against").is_none(), "{json}");
        assert_eq!(serde_json::from_value::<Caveat>(json).unwrap(), d);
        // On a take with no clock log, the USB lines over a known
        // correction, named as such (#635).
        let c = Caveat::ClockSourcesDisagree {
            usb_ppm: 12.0,
            log_ppm: 5.0,
            log_ppm_sd: 0.0,
            against: crate::clock::Against::Correction,
        };
        assert_eq!(
            c.short(52.0, 0).unwrap(),
            "USB lines and the known correction 7 ppm apart"
        );
        assert_eq!(
            c.long(None).unwrap(),
            "The USB lines measure the sound card at +12.0 ppm and the correction known for \
             its input is +5.0 ppm; the rates are corrected by the USB lines."
        );
        let json = serde_json::to_value(c).unwrap();
        assert_eq!(json["against"], "correction");
        assert_eq!(serde_json::from_value::<Caveat>(json).unwrap(), c);
        // The JSON a caveat is written as reads back into the same words.
        let c: Caveat = serde_json::from_str(r#"{"code":"clipped","fraction":0.04}"#).unwrap();
        assert_eq!(c.short(52.0, 0).unwrap(), "4% clipped");
    }

    use crate::breaks::Unlisted;
    use crate::longterm::{null::WALK_MIX, NullModel};

    /// Caveats beside their few words and their sentence, as the app, CLI
    /// and MCP show them.
    const WORDED: [(Caveat, &str, &str); 25] = [
        (
            Caveat::ReadingWindows {
                reading: confidence::Figure::Amplitude,
                windows_used: 3,
                windows_in_span: 9,
            },
            "amplitude read from 3 of 9 windows",
            "The amplitude was read from 3 of 9 windows in the kept time.",
        ),
        (
            Caveat::BeatErrorSourceReadings {
                source: crate::session::BeatErrorSource::Unlock,
                used: 5,
                readings: 7,
                counted: Counted::Readings,
            },
            "beat error from the unlock in 5 of 7 readings",
            "The beat error uses the unlock in 5 of 7 readings; readings from the other \
             event were left out.",
        ),
        (
            Caveat::BeatErrorSourceReadings {
                source: crate::session::BeatErrorSource::Unlock,
                used: 123,
                readings: 140,
                counted: Counted::Windows,
            },
            "beat error from the unlock in 123 of 140 2 s windows",
            "The beat error uses the unlock in 123 of 140 2 s windows; 2 s windows from the \
             other event were left out.",
        ),
        (
            Caveat::BeatErrorUnknownSource { readings: 1 },
            "1 beat-error readings with no recorded source left out",
            "The beat-error aggregate leaves out 1 reading whose source was not recorded.",
        ),
        (
            Caveat::FROM_DROP,
            "beat error from the drop",
            "The unlock was not found; the beat error is measured from the drop.",
        ),
        (
            Caveat::SetRateDisagrees { heard: 21600 },
            "21600 bph heard",
            "The beat rate set differs from the 21600 bph heard.",
        ),
        (
            Caveat::ShortSpan { span_s: 6.4 },
            "6 s of beats",
            "The beats span 6 s, under the 10 s needed to judge whether they are regular.",
        ),
        (
            Caveat::Clipped { fraction: 0.25 },
            "25% clipped",
            "25% of the beats have a clipped sample, which moves the timing of the sounds \
             inside a beat.",
        ),
        (
            Caveat::BeatsMissing { fraction: 0.5 },
            "50% of beats found",
            "50% of the beats the beat rate expects in this stretch were found.",
        ),
        (
            Caveat::LiftUnknown,
            "lift angle 52°, the default",
            "The lift angle is the default, not one set for the calibre. Every amplitude \
             is worked out from it.",
        ),
        (
            Caveat::ClockUnknown,
            "input clock not corrected",
            "This input's clock hasn't been measured, so the rate includes the sound \
             card's own clock error.",
        ),
        (
            Caveat::Breaks { count: 1 },
            "1 break",
            "1 break in this stretch: a hole in the sound, such as when the computer \
             slept, or a stumble where the beats stepped off together by more than 4 ms.",
        ),
        (
            Caveat::BreaksUnlisted {
                why: Unlisted::EntriesSkipped,
            },
            "breaks from the clock log",
            "Some of the breaks this take's session.json lists give no place or gap, so \
             its breaks were read from the clock log instead, which shows a pause only \
             where the time it lost is 0.5 s or more.",
        ),
        (
            Caveat::BreaksUnlisted {
                why: Unlisted::NotTheTake,
            },
            "breaks from the clock log",
            "These files are not the whole of the take its session.json describes, so its \
             breaks were read from the clock log instead, which shows a pause only where \
             the time it lost is 0.5 s or more.",
        ),
        (
            Caveat::PausesUnseen { count: 1 },
            "1 listed pause not in the clock log",
            "1 pause this take's session.json lists shows no gap in its clock log, so no \
             time is counted for it.",
        ),
        (
            Caveat::PausesUnseen { count: 3 },
            "3 listed pauses not in the clock log",
            "3 pauses this take's session.json lists show no gap in its clock log, so no \
             time is counted for them.",
        ),
        (
            Caveat::SidesByCount { count: 2 },
            "sides named by count",
            "Tick and Tock are named by the beat count across 2 breaks in this stretch, \
             not told apart by their sound.",
        ),
        (
            Caveat::BeatErrorAsSize,
            "beat error given as a size: its sign differs either side of a cut",
            "The beat error's sign differs either side of a cut in the stretch, so it is \
             given as a size.",
        ),
        (
            Caveat::Masked { seconds: 12.4 },
            "12 s masked",
            "12 s of this stretch were masked out and left out of the readings.",
        ),
        (
            Caveat::Positions { count: 6 },
            "6 positions: a rate change with position can read as one with amplitude",
            "The take holds 6 positions, so a rate that changes with the position can \
             show as one that goes with the amplitude.",
        ),
        (
            Caveat::LeadLeftOut { seconds: 31.6 },
            "first 32 s left out",
            "The first 32 s held no regular beat before the watch was heard, and are left \
             out of the readings.",
        ),
        (
            Caveat::PeriodMayBeNoise {
                rho: 0.6,
                span_s: 1800.0,
            },
            "cycle may be noise",
            "On a take this short with its 2 s windows this correlated (ρ 0.60 from one to \
             the next, over 30 min), the period search names a cycle in noise alone about \
             1 time in 27, not 1 in 100: this cycle may be noise.",
        ),
        (
            Caveat::PeriodSearchUncalibrated {
                model: NullModel::Timing,
                mix: 40.0,
                span_s: 9000.0,
            },
            "cycle search not calibrated here",
            "The period search has no simulated noise for 2.5 h of beat timing whose \
             wander meets its jitter at 40 cycles a detrend window, so how often it names \
             a cycle in noise alone is not known here: this cycle may be noise.",
        ),
        (
            Caveat::PeriodSearchUncalibrated {
                model: NullModel::Timing,
                mix: WALK_MIX,
                span_s: 9000.0,
            },
            "cycle search not calibrated here",
            "The period search has no simulated noise for 2.5 h of beat timing with no \
             jitter floor to measure, so how often it names a cycle in noise alone is not \
             known here: this cycle may be noise.",
        ),
        (
            Caveat::TimingModelMisfit {
                misfit: 0.41,
                limit: 0.18,
            },
            "timing noise not the simulated kind",
            "The beat timing's noise is not the wander plus jitter the period search was \
             checked against: its spectrum sits 0.41 decades from that shape, where 99 in \
             100 simulated takes sit within 0.18. How often it names a cycle in this \
             noise alone is not known: this cycle may be noise.",
        ),
    ];

    #[test]
    fn every_caveat_has_its_words() {
        for (c, short, long) in WORDED {
            let words = (c.short(52.0, 0).unwrap(), c.long(None).unwrap());
            assert_eq!(words, (short.to_string(), long.to_string()), "{c:?}");
        }
        // A sliver too small to say has no sentence either.
        assert_eq!(Caveat::Masked { seconds: 0.02 }.long(None), None);
        assert_eq!(
            no_watch_text(NoWatchReason::NoBeatSound, Some(900.0), Some(28800)),
            "No regular beat found (neither the Tick's nor the Tock's sound shows an unlock \
             or a drop)"
        );
        // Hugh on #2257: the whole sentence, its length from the window.
        assert_eq!(
            no_watch_text(NoWatchReason::TooShort, None, Some(28800)),
            "No regular beat found (the beats were regular, but no run of them lasted the \
             10 s a reading needs)"
        );
    }

    #[test]
    fn only_the_caveats_about_breaks_are_about_breaks() {
        // The three kinds about breaks, and no other kind.
        let about_breaks: Vec<_> = WORDED
            .iter()
            .map(|(c, ..)| *c)
            .filter(Caveat::is_about_breaks)
            .collect();
        assert_eq!(
            about_breaks,
            [
                Caveat::Breaks { count: 1 },
                Caveat::BreaksUnlisted {
                    why: Unlisted::EntriesSkipped
                },
                Caveat::BreaksUnlisted {
                    why: Unlisted::NotTheTake
                },
                Caveat::PausesUnseen { count: 1 },
                Caveat::PausesUnseen { count: 3 },
            ]
        );
    }

    /// The order a list gives caveats in.
    #[test]
    fn caveats_are_listed_in_their_place() {
        // Added in any order, a list reads no regular beat first, then the
        // rate set, the sound, the clock, the breaks, and the rest; a
        // caveat added twice is listed once.
        let in_place = [
            Caveat::NoWatch {
                reason: NoWatchReason::Scatter,
            },
            Caveat::SetRateDisagrees { heard: 21600 },
            Caveat::ShortSpan { span_s: 6.4 },
            Caveat::Clipped { fraction: 0.25 },
            Caveat::BeatsMissing { fraction: 0.5 },
            Caveat::LiftUnknown,
            Caveat::ClockUnknown,
            Caveat::ClockSourcesDisagree {
                usb_ppm: 12.0,
                log_ppm: 5.0,
                log_ppm_sd: 0.0,
                against: Default::default(),
            },
            Caveat::Breaks { count: 1 },
            Caveat::BreaksUnlisted {
                why: Unlisted::NotTheTake,
            },
            Caveat::PausesUnseen { count: 3 },
            Caveat::SidesByCount { count: 2 },
            Caveat::Masked { seconds: 12.4 },
            Caveat::BeatErrorAsSize,
            Caveat::Positions { count: 6 },
            Caveat::LeadLeftOut { seconds: 31.6 },
            Caveat::FROM_DROP,
            Caveat::ReadingWindows {
                reading: confidence::Figure::Rate,
                windows_used: 3,
                windows_in_span: 9,
            },
            Caveat::BeatErrorSourceReadings {
                source: crate::session::BeatErrorSource::Drop,
                used: 5,
                readings: 7,
                counted: Counted::Readings,
            },
        ];
        let mut list = Vec::new();
        for c in in_place.iter().rev().chain(in_place.iter()) {
            add(&mut list, *c);
        }
        assert_eq!(list, in_place);
        // The four that share the last place keep the order they came in.
        let last = [
            Caveat::TimingModelMisfit {
                misfit: 0.41,
                limit: 0.18,
            },
            Caveat::BeatErrorUnknownSource { readings: 1 },
            Caveat::PeriodSearchUncalibrated {
                model: NullModel::Readings,
                mix: 0.5,
                span_s: 9000.0,
            },
            Caveat::PeriodMayBeNoise {
                rho: 0.6,
                span_s: 1800.0,
            },
        ];
        let mut list = vec![Caveat::LiftUnknown];
        for c in last {
            add(&mut list, c);
        }
        add(
            &mut list,
            Caveat::NoWatch {
                reason: NoWatchReason::NoBeatSound,
            },
        );
        assert_eq!(
            list[..2],
            [
                Caveat::NoWatch {
                    reason: NoWatchReason::NoBeatSound
                },
                Caveat::LiftUnknown
            ]
        );
        assert_eq!(list[2..], last);
    }
}
