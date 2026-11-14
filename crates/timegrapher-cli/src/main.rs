mod archive;
mod bundle_cmd;
mod calibres_cmd;
mod clock_cmd;
mod corpus;
mod doctor;
mod excerpt_cmd;
mod long;
mod output;
mod profile_cmd;
mod record_cmd;
mod regress_cmd;
mod regress_provenance;
mod report;
mod series_cmd;
mod session_cmd;
mod session_report;
mod shape_cmd;
mod synth_cmd;
mod train_cmd;
mod update_cmd;
mod watches_cmd;
mod watches_edit;

use clap::{Parser, Subcommand};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use timegrapher_core::figure_text::{figure, figure_with_pm};
use timegrapher_core::library::{base_digits, fixed_text, signed_text};
use timegrapher_core::stream::StreamConfig;
use timegrapher_core::synth::{self, SynthConfig};
use timegrapher_core::{analyze, load, Analysis, AnalysisConfig};

fn parse_bph(text: &str) -> Result<u32, String> {
    let value = text
        .parse::<u64>()
        .map_err(|_| "BPH must be an integer from 1 through 4294967295".to_string())?;
    timegrapher_core::input_domain::bph(value)
}
fn parse_lift(text: &str) -> Result<f64, String> {
    timegrapher_core::input_domain::lift_deg(
        text.parse()
            .map_err(|_| timegrapher_core::input_domain::LIFT_RANGE_TEXT.to_string())?,
    )
}
fn parse_escape_teeth(text: &str) -> Result<u32, String> {
    timegrapher_core::input_domain::escape_teeth(
        text.parse()
            .map_err(|_| "escape teeth must be a positive u32 integer".to_string())?,
    )
}
fn parse_trend_average(text: &str) -> Result<f64, String> {
    timegrapher_core::input_domain::average_s(
        text.parse()
            .map_err(|_| "the averaging time must be a number of seconds".to_string())?,
    )
}
fn parse_card_ppm(text: &str) -> Result<f64, String> {
    timegrapher_core::input_domain::card_ppm(
        text.parse()
            .map_err(|_| "card ppm must be finite with a positive clock scale".to_string())?,
    )
}

#[derive(Parser)]
#[command(
    name = "timegrapher",
    version = env!("TIMEGRAPHER_LONG_VERSION"),
    about = "Analyse mechanical watch recordings beat by beat",
    after_help = "Every command with --json prints one JSON document with a `schema` field \
(e.g. timegrapher.analyze/2), the software version and the input it read. \
`timegrapher doctor` measures the microphone input. Exit codes: 0 success, \
1 error, 3 a take moved beyond its margins (regress)."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Analyse a WAV or FLAC recording.
    Analyze {
        file: PathBuf,
        /// The movement (e.g. "ETA 2824-2", "Rolex 3235"; see `calibres`):
        /// sets the beat rate, the lift angle and the escape wheel teeth
        /// from the table and names periodic components after its wheels.
        /// --bph, --lift and --escape-teeth still win.
        #[arg(long)]
        calibre: Option<String>,
        /// Beat rate in beats per hour (default: the take's, for a sound
        /// file its folder's session.toml lists; else the calibre's, or
        /// guessed).
        #[arg(long, value_parser = parse_bph)]
        bph: Option<u32>,
        /// Lift angle in degrees (default: the take's, for a sound file its
        /// folder's session.toml lists; else the calibre's, or 52, named as
        /// the default).
        #[arg(long, value_parser = parse_lift)]
        lift: Option<f64>,
        /// Comma-separated steady tones to notch out, Hz (e.g. 5000,6000,7000).
        #[arg(long, value_delimiter = ',')]
        notch: Vec<f64>,
        /// High-pass corner, Hz.
        #[arg(long, default_value_t = timegrapher_core::dsp::DEFAULT_HIGHPASS_HZ)]
        highpass: f64,
        /// Escape wheel teeth, used to name periodic components (default:
        /// the calibre's, or 15).
        #[arg(long, value_parser = parse_escape_teeth)]
        escape_teeth: Option<u32>,
        /// Write one row per beat to this CSV file.
        #[arg(long)]
        beats: Option<PathBuf>,
        /// Write rate and amplitude per window to this CSV file.
        #[arg(long)]
        windows: Option<PathBuf>,
        /// Write the readings over time, each averaged over
        /// `--trend-average` seconds as the app's charts read them, to this
        /// CSV file.
        #[arg(long)]
        trend: Option<PathBuf>,
        /// Seconds each `--trend` reading is averaged over (the app's
        /// Average Over).
        #[arg(long, default_value_t = timegrapher_core::live::DEFAULT_AVERAGE_S, value_parser = parse_trend_average)]
        trend_average: f64,
        /// Correct the rate with the clock error stored for this input
        /// (see `clock`). Without it, the input named in the recording's
        /// sidecar is used when one is stored.
        #[arg(long)]
        device: Option<String>,
        /// Correct the rate for a sound card this many ppm fast (negative
        /// if slow), instead of a stored correction.
        #[arg(long, allow_hyphen_values = true, value_parser = parse_card_ppm)]
        card_ppm: Option<f64>,
        /// Print the summary as JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Measure the three sounds of each side's beat.
    Shape {
        file: PathBuf,
        /// Beat rate in beats per hour (default: the take's, for a sound
        /// file its folder's session.toml lists; else guessed).
        #[arg(long, value_parser = parse_bph)]
        bph: Option<u32>,
        /// Lift angle in degrees (default: the take's, for a sound file its
        /// folder's session.toml lists; else 52).
        #[arg(long, value_parser = parse_lift)]
        lift: Option<f64>,
        /// Comma-separated steady tones to notch out, Hz.
        #[arg(long, value_delimiter = ',')]
        notch: Vec<f64>,
        /// High-pass corner, Hz.
        #[arg(long, default_value_t = timegrapher_core::dsp::DEFAULT_HIGHPASS_HZ)]
        highpass: f64,
        /// Print the measurements as JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Write the Tick and Tock whole-recording templates to this CSV file
        /// (columns `even` and `odd`).
        #[arg(long)]
        templates: Option<PathBuf>,
    },
    /// The sound of the Tick and the Tock over a stretch of a recording,
    /// with the unlock, drop and sound marks the engine measured on them:
    /// the place to look when amplitude or beat error seem wrong.
    Profile {
        file: PathBuf,
        /// Start of the stretch, seconds from the start of the recording.
        #[arg(long, default_value_t = 0.0)]
        at: f64,
        /// Length of the stretch, seconds.
        #[arg(long, default_value_t = 10.0)]
        span: f64,
        /// Beat rate in beats per hour (default: the take's, for a sound
        /// file its folder's session.toml lists; else guessed).
        #[arg(long, value_parser = parse_bph)]
        bph: Option<u32>,
        /// Lift angle in degrees (default: the take's, for a sound file its
        /// folder's session.toml lists; else 52).
        #[arg(long, value_parser = parse_lift)]
        lift: Option<f64>,
        /// Comma-separated steady tones to notch out, Hz.
        #[arg(long, value_delimiter = ',')]
        notch: Vec<f64>,
        /// High-pass corner, Hz.
        #[arg(long, default_value_t = timegrapher_core::dsp::DEFAULT_HIGHPASS_HZ)]
        highpass: f64,
        /// Print the profiles (envelopes and marks) as JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Also draw them to this SVG file.
        #[arg(long)]
        svg: Option<PathBuf>,
    },
    /// Analyse a long recording (hours to days): rate and amplitude over
    /// time and the periodic changes that point at a wheel of the train.
    Long {
        /// The recording; several files are read as one continuous
        /// recording, in the order given (a capture split into segments).
        /// A folder stands for its WAV and FLAC files, sorted by name; a
        /// take's folder is read with its session.toml.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Clock log for calibrating the sound card: audio position against
        /// NTP-synced system time (see `docs/reference/long-runs.md`).
        #[arg(long)]
        clock: Option<PathBuf>,
        /// Without a clock log: correct with the clock error stored for
        /// this input (see `clock`).
        #[arg(long)]
        device: Option<String>,
        /// Without a clock log: correct for a sound card this many ppm fast
        /// (negative if slow).
        #[arg(long, allow_hyphen_values = true, value_parser = parse_card_ppm)]
        card_ppm: Option<f64>,
        /// Beat rate in beats per hour (guessed if omitted).
        #[arg(long, value_parser = parse_bph)]
        bph: Option<u32>,
        /// Lift angle in degrees (default: the calibre's; 52 without a
        /// calibre or when the table has no lift angle for it).
        #[arg(long, value_parser = parse_lift)]
        lift: Option<f64>,
        /// Comma-separated steady tones to notch out, Hz.
        #[arg(long, value_delimiter = ',')]
        notch: Vec<f64>,
        /// High-pass corner, Hz.
        #[arg(long, default_value_t = timegrapher_core::dsp::DEFAULT_HIGHPASS_HZ)]
        highpass: f64,
        /// The movement, to name periodic components after its wheels and
        /// take its lift angle from the table (e.g. "ETA 2824-2", "Rolex
        /// 3235"; a name not in the catalogue is an error that points to
        /// `timegrapher calibres --search`).
        #[arg(long)]
        calibre: Option<String>,
        /// Escape wheel teeth, used to name periodic components (default:
        /// the calibre's, or both 15 and 20 when no calibre is given).
        #[arg(long, value_parser = parse_escape_teeth)]
        escape_teeth: Option<u32>,
        /// Another wheel to name, as NAME=SECONDS per turn (repeatable),
        /// e.g. --wheel "third wheel=450".
        #[arg(long)]
        wheel: Vec<String>,
        /// Folder for the report and data files (default: next to the
        /// recording, named after it).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Print the summary as JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// The structure of each series of readings in time order. Tests the
    /// rate, amplitude and beat error of a take for correlation between
    /// readings (autocorrelation, Ljung–Box, Allan deviation, CUSUM),
    /// level changes, a trend, a cycle (period search) and two levels
    /// (two-state finder), and states what each finds with its statistics.
    Series {
        /// The recording; several files are read as one continuous
        /// recording, in the order given. A folder stands for its WAV and
        /// FLAC files, sorted by name; a take's folder is read with its
        /// session.toml.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Clock log for calibrating the sound card (as for `long`).
        #[arg(long)]
        clock: Option<PathBuf>,
        /// Beat rate in beats per hour (guessed if omitted).
        #[arg(long, value_parser = parse_bph)]
        bph: Option<u32>,
        /// Lift angle in degrees (default: the calibre's; 52 without a
        /// calibre or when the table has no lift angle for it).
        #[arg(long, value_parser = parse_lift)]
        lift: Option<f64>,
        /// Comma-separated steady tones to notch out, Hz.
        #[arg(long, value_delimiter = ',')]
        notch: Vec<f64>,
        /// High-pass corner, Hz.
        #[arg(long, default_value_t = timegrapher_core::dsp::DEFAULT_HIGHPASS_HZ)]
        highpass: f64,
        /// The movement, to name periodic components after its wheels and
        /// take its lift angle from the table (e.g. "ETA 2824-2", "Rolex
        /// 3235"; a name not in the catalogue is an error that points to
        /// `timegrapher calibres --search`).
        #[arg(long)]
        calibre: Option<String>,
        /// Escape wheel teeth, used to name periodic components (default:
        /// the calibre's, or both 15 and 20 when no calibre is given).
        #[arg(long, value_parser = parse_escape_teeth)]
        escape_teeth: Option<u32>,
        /// Another wheel to name, as NAME=SECONDS per turn (repeatable).
        #[arg(long)]
        wheel: Vec<String>,
        /// Length of each rate reading, seconds.
        #[arg(long, default_value_t = 10.0)]
        reading: f64,
        /// Hours since the watch was last fully wound when the take
        /// started (for the 0 h / 24 h comparison).
        #[arg(long, default_value_t = 0.0)]
        wind_h: f64,
        /// Count the distributions over stretches of this many minutes as
        /// well as over the whole take (0: the whole take only).
        #[arg(long, default_value_t = 60.0)]
        stretch_min: f64,
        /// Print the result as JSON instead of text.
        #[arg(long)]
        json: bool,
        /// With --json, add the chart data behind
        /// each headline (the readings, the period search and the fold):
        /// a few hundred kB on a long take.
        #[arg(long, requires = "json")]
        views: bool,
        /// With --json, add the time-frequency maps of each series (an
        /// STFT and a synchrosqueezed wavelet transform) and the wavelet
        /// coherence of the rate with the amplitude and of Tick's
        /// amplitude with Tock's, the Lomb-Scargle periodogram of each
        /// series' readings and each series' kurtogram: about 4.1 MB on a
        /// 2 h take (parts 6 and 7 of the Pepsi replica's take, Dandong
        /// 3285).
        #[arg(long, requires = "json")]
        maps: bool,
    },
    /// Check GitHub for a newer published release (one request; nothing
    /// is downloaded or installed).
    Update {
        /// Print the result as JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// The table of calibres: beat rate, lift angle, escape wheel teeth and
    /// the turn of each train wheel, for `--calibre`, with the wheel periods
    /// and calibres added in the app (calibres.json in the settings folder).
    Calibres {
        /// Show only this calibre, with every wheel: its period from the
        /// table or the user, or unknown; and its notes and sources.
        name: Option<String>,
        /// List the calibres that answer this, best first: a calibre's
        /// name, other names or maker under any of its names ("dd 3235",
        /// "AP 3120").
        #[arg(long, conflicts_with = "name")]
        search: Option<String>,
        /// Print the table as JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// A going train worked out from its tooth and leaf counts: each
    /// wheel's turn, the beat rate where only periods are given, and each
    /// mesh's period. Counts are TEETH/LEAVES (the wheel's teeth, the
    /// leaves of the pinion on the same arbor), e.g. `--fourth 108/9`.
    Train {
        /// Start from this calibre's counts and periods (the table's and
        /// those added in the app); the flags below replace them.
        #[arg(long)]
        calibre: Option<String>,
        /// Beats per hour; the calibre's when left out.
        #[arg(long, value_parser = parse_bph)]
        bph: Option<u32>,
        /// The escape wheel's teeth and the escape pinion's leaves.
        #[arg(long)]
        escape: Option<String>,
        /// The fourth wheel's teeth and the fourth pinion's leaves.
        #[arg(long)]
        fourth: Option<String>,
        /// The third wheel's teeth and the third pinion's leaves.
        #[arg(long)]
        third: Option<String>,
        /// The centre wheel's teeth and the centre pinion's leaves.
        #[arg(long)]
        centre: Option<String>,
        /// The barrel's teeth.
        #[arg(long)]
        barrel: Option<String>,
        /// Periods known, ARBOR=SECONDS, several split by commas or the
        /// flag repeated, e.g. `--period fourth=60,centre=3600`.
        #[arg(long)]
        period: Vec<String>,
        /// Print the result as JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// List the sound inputs, with the sample rates and formats each supports.
    Devices {
        /// Print the list as JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Read each USB microphone's automatic gain switch (macOS: over
        /// USB; Windows: the system's control), instead of listing the inputs.
        #[arg(long, conflicts_with_all = ["agc_off", "agc_on"])]
        agc: bool,
        /// Turn each USB microphone's automatic gain off (macOS, Windows),
        /// and read it back. The microphone turns it on again when it is
        /// plugged in.
        #[arg(long, conflicts_with = "agc_on")]
        agc_off: bool,
        /// Turn it back on (macOS, Windows).
        #[arg(long)]
        agc_on: bool,
    },
    /// Measure the microphone input: record a few seconds and report the
    /// level, clipping, whether the background rises between ticks, the
    /// signal above noise and whether a regular beat is found, with the
    /// mixer changes --apply makes. Changes nothing without --apply.
    Doctor {
        /// Sound input to use: part of its name or id from `devices`
        /// (default: the system's default input).
        #[arg(long)]
        device: Option<String>,
        /// ALSA card (number or name) whose mixer to read on Linux
        /// (default: from the device, or the only USB sound card).
        #[arg(long)]
        card: Option<String>,
        /// Seconds to record.
        #[arg(long, default_value_t = 5.0)]
        seconds: f64,
        /// Check a recording instead of listening live.
        #[arg(long)]
        file: Option<PathBuf>,
        /// Keep the recording as a WAV file, with its device and mixer
        /// settings in FILE.json beside it.
        #[arg(long)]
        save: Option<PathBuf>,
        /// Make the listed mixer changes (Linux): sets the peak to about
        /// −10 dBFS and turns automatic gain off, then checks again.
        /// Ask the watch's owner before using this: it changes their
        /// computer's settings.
        #[arg(long)]
        apply: bool,
        /// Beat rate in beats per hour (guessed if omitted).
        #[arg(long, value_parser = parse_bph)]
        bph: Option<u32>,
        /// Print the report as JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Each sound input's clock error: a sound card's crystal is off by a
    /// steady 10-50 ppm (1-4 s/d on every rate). Measure it once and
    /// `analyze`, `long` and the app correct rates read on that input.
    Clock {
        #[command(subcommand)]
        action: ClockAction,
        /// Print the result as JSON instead of text.
        #[arg(long, global = true)]
        json: bool,
    },
    /// Record one position of a test into a session folder: a take folder
    /// as the desktop app saves it, checked at once and added to the
    /// folder's session.toml for `session`, with what was measured: no
    /// regular beat, or clipping, is stated, not refused. Listens only while
    /// it runs, for --seconds.
    Record {
        /// The session folder (created if missing); one per watch and test.
        /// Left out, --watch names a watch in the app's library, and the
        /// folder is made among its takes.
        folder: Option<PathBuf>,
        /// The app's library, for --watch without a FOLDER (default:
        /// Timegrapher Data in the home folder).
        #[arg(long)]
        library: Option<PathBuf>,
        /// Position (core `Position::accepted_names`).
        #[arg(long, help = format!("Position: {}", timegrapher_core::session::Position::accepted_names()))]
        position: String,
        /// Seconds to record.
        #[arg(long, default_value_t = 120.0)]
        seconds: f64,
        /// Sound input: part of its name or id from `devices` (default:
        /// the system's default input).
        #[arg(long)]
        device: Option<String>,
        /// Read this WAV or FLAC instead of the microphone (to try the
        /// command, and for tests).
        #[arg(long)]
        file: Option<PathBuf>,
        /// The watch, in the owner's words (for a new session file).
        /// Without a FOLDER, a watch in the app's library: its calibre,
        /// beat rate and lift angle fill in those not given.
        #[arg(long)]
        watch: Option<String>,
        /// The movement, from the calibre table: gives bph and lift when
        /// they are not given (a name not in the catalogue is an error
        /// that points to `timegrapher calibres --search`).
        #[arg(long)]
        calibre: Option<String>,
        /// Beat rate in beats per hour (guessed if omitted).
        #[arg(long, value_parser = parse_bph)]
        bph: Option<u32>,
        /// Lift angle in degrees (default: the calibre's, else 52).
        #[arg(long, value_parser = parse_lift)]
        lift: Option<f64>,
        /// Hours since the watch was fully wound (0 for the fully wound
        /// half of a test, 24 for the run-down half); left out of the
        /// session file when not given.
        #[arg(long)]
        wind_h: Option<f64>,
        /// A Power Reserve Test: record until the watch stops (the last
        /// beat heard before 10 minutes with none) or 80 hours pass, in
        /// place of --seconds, and keep the run, hour by hour, in the
        /// session folder's power-reserve.json.
        #[arg(long, conflicts_with = "seconds")]
        power_reserve: bool,
        /// Print the result as JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Keep the latest readings in this file while recording, replaced
        /// about once a second (`timegrapher.live/1`), for a live view.
        /// With --file the file is played back in real time.
        #[arg(long)]
        live: Option<PathBuf>,
        /// End the recording early once this file exists, as the app's
        /// Stop does: a way for another program to stop it. The take is
        /// kept and checked; a Power Reserve Test ends "by hand" with its
        /// hours. Refused if the file is already there.
        #[arg(long)]
        stop_file: Option<PathBuf>,
    },
    /// Read a watch measured in several positions (and states of wind)
    /// into one multi-position report, Witschi style.
    Session {
        /// A session file, a folder holding one, or recordings named by
        /// position (core `Position::accepted_names`).
        #[arg(required = true, help = format!(
            "A session file (session.toml), a folder holding one, or the recordings \
             themselves with the position in each file name: {}",
            timegrapher_core::session::Position::accepted_names()
        ))]
        paths: Vec<PathBuf>,
        /// Write a session.toml for this folder of recordings to edit, and stop.
        #[arg(long)]
        init: bool,
        /// Beat rate in beats per hour (overrides the session file).
        #[arg(long, value_parser = parse_bph)]
        bph: Option<u32>,
        /// Lift angle in degrees (overrides the session file; default 52).
        #[arg(long, value_parser = parse_lift)]
        lift: Option<f64>,
        /// The movement (e.g. "ETA 2824-2", "Rolex 3235"; an unknown name
        /// lists the known ones): its beat rate, lift angle and wheels
        /// where --bph, --lift and the session file give none.
        #[arg(long)]
        calibre: Option<String>,
        /// Seconds skipped at the start of each recording while the watch
        /// settles (overrides the session file; default 20).
        #[arg(long)]
        settle: Option<f64>,
        /// Skip the beat-shape measurement.
        #[arg(long)]
        no_shape: bool,
        /// Skip the periodic-change search.
        #[arg(long)]
        no_cycles: bool,
        /// Folder for the report (default: session_report next to the session file).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Print the summary as JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Check the engine against tg on stored takes: every reading of every
    /// session file under ROOT is measured again and compared with its tg
    /// reference and a baseline. Exit code 3 when a value moved away from
    /// tg by more than its margin, a reading lost beats or a value, required
    /// evidence is missing/unreadable, or an existing recording label changed.
    Regress {
        /// A folder of takes, each with a session.toml whose readings carry
        /// tg's numbers (e.g. a clone of the recordings repo).
        root: PathBuf,
        /// Baseline file (default: regress-baseline.json in ROOT).
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// Accept successful measurements and label changes into the baseline.
        /// Required missing or unreadable evidence still prevents writing.
        #[arg(long)]
        write_baseline: bool,
        /// Only the takes whose folder name contains this (repeatable).
        #[arg(long = "take")]
        takes: Vec<String>,
        /// Readings measured at once (default: one per processor).
        #[arg(long)]
        jobs: Option<usize>,
        /// Margin for rate, s/d.
        #[arg(long, default_value_t = 0.3)]
        rate_margin: f64,
        /// Margin for amplitude, degrees.
        #[arg(long, default_value_t = 1.5)]
        amplitude_margin: f64,
        /// Margin for beat error, ms.
        #[arg(long, default_value_t = 0.03)]
        beat_error_margin: f64,
        /// List every value, not only those that moved.
        #[arg(long)]
        all: bool,
        /// Print the outcome as JSON instead of text.
        #[arg(long)]
        json: bool,
        /// No progress lines or recording file names (for public CI logs).
        #[arg(long)]
        quiet: bool,
        /// ROOT is a checkout of the public corpus: download the audio it
        /// needs from the corpus's releases first (with curl), checked by
        /// SHA-256.
        #[arg(long)]
        fetch: bool,
        /// Also run the corpus's smoke takes (no tg reference) through the
        /// engine; they fail only if it panics.
        #[arg(long)]
        smoke: bool,
        /// Read only the corpus's holdout takes: judged against tg,
        /// printed as counts only, never failing.
        #[arg(long)]
        holdout: bool,
        /// With --holdout, print every reading's numbers too (milestone
        /// runs only, never in CI).
        #[arg(long)]
        reveal: bool,
    },
    /// Make a zip to attach to an issue: an excerpt of the recording, what
    /// was measured, the system, clock corrections and logs, with the home
    /// folder, user and host names removed. Never opens the microphone.
    /// With --replay, run a bundle's commands again and show what moved.
    Report {
        /// The recording (a file, or a folder of segments), if the report
        /// is about one.
        recording: Vec<PathBuf>,
        /// Start of the excerpt, seconds into the recording.
        #[arg(long, default_value_t = 0.0)]
        at: f64,
        /// Length of the excerpt, seconds (5 minutes of FLAC is about 18 MB;
        /// GitHub takes attachments up to 25 MB).
        #[arg(long, default_value_t = 300.0)]
        span: f64,
        /// Leave the audio out.
        #[arg(long)]
        no_audio: bool,
        /// Leave the take's analysis (its beats and amplitude windows) out,
        /// which saves reading the whole recording once more.
        #[arg(long)]
        no_analysis: bool,
        /// What the report is about (default: reading with a recording, bug
        /// without).
        #[arg(long, value_enum)]
        kind: Option<bundle_cmd::Kind>,
        /// What happened, in your words.
        #[arg(long)]
        note: Option<String>,
        /// Beat rate in beats per hour, as set when the problem showed.
        #[arg(long, value_parser = parse_bph)]
        bph: Option<u32>,
        /// Lift angle in degrees, as set.
        #[arg(long, value_parser = parse_lift)]
        lift: Option<f64>,
        /// Folder for the zip (default: the current folder).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Say you listened to the excerpt and it holds nothing private.
        #[arg(long)]
        listened_back: bool,
        /// Offer the excerpt to the public test corpus under CC0. Only the
        /// person who made the recording should set this.
        #[arg(long)]
        cc0: bool,
        /// Run the commands of this bundle (zip or unpacked folder) with
        /// this build and show what moved.
        #[arg(long, conflicts_with_all = ["recording", "no_audio", "no_analysis", "note", "out", "cc0", "listened_back"])]
        replay: Option<PathBuf>,
        /// Print the manifest (or the replay) as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Write a stretch of a recording or take as one FLAC file, its
    /// samples untouched: for a program that keeps the sound beside a
    /// reading. Never opens the microphone; never writes over a file.
    Excerpt {
        /// The recording (a file, or a take folder of segments).
        #[arg(required = true)]
        recording: Vec<PathBuf>,
        /// The FLAC file to write. It must not exist yet.
        #[arg(long)]
        out: PathBuf,
        /// Start of the excerpt, seconds into the recording.
        #[arg(long, default_value_t = 0.0)]
        from: f64,
        /// End of the excerpt, seconds into the recording (default: its end).
        #[arg(long)]
        to: Option<f64>,
        /// Print what was written as JSON.
        #[arg(long)]
        json: bool,
    },
    /// The watches in the library the app keeps, with their details and
    /// takes, and the takes Quick Listen left in Unsorted; and the changes
    /// the app's Watches view makes (new, edit, add-take, move-take,
    /// keep-analysis-only, delete-take, add-photos, delete-photo).
    Watches {
        #[command(subcommand)]
        action: WatchesAction,
        /// The library's folder (default: Timegrapher Data in the home
        /// folder). A library chosen elsewhere in the app's Settings has to
        /// be named here.
        #[arg(long, global = true)]
        library: Option<PathBuf>,
        /// Print JSON instead of text.
        #[arg(long, global = true)]
        json: bool,
    },
    /// Write a synthetic watch recording (for testing).
    Synth {
        /// The WAV file to write, or with --scenario the take folder.
        out: Option<PathBuf>,
        /// Write a take with a known hidden variation instead, as a take
        /// folder (FLAC files, session.json, session.toml, a README saying
        /// what is hidden); `list` names them.
        #[arg(long, conflicts_with_all = [
            "duration", "bph", "amplitude", "rate", "beat_error", "snr",
            "fault_period", "fault_length", "fault_rate", "fault_amplitude",
        ])]
        scenario: Option<String>,
        /// The noise and beat-to-beat variation of a scenario take.
        #[arg(long, default_value_t = 1, requires = "scenario")]
        seed: u64,
        #[arg(long, default_value_t = 60.0)]
        duration: f64,
        #[arg(long, default_value_t = 28800, value_parser = parse_bph)]
        bph: u32,
        #[arg(long, default_value_t = 270.0)]
        amplitude: f64,
        #[arg(long, default_value_t = 5.0)]
        rate: f64,
        #[arg(long, default_value_t = 0.4)]
        beat_error: f64,
        #[arg(long, default_value_t = 30.0)]
        snr: f64,
        /// Add a fault that repeats every this many seconds (like a bad
        /// tooth on a wheel), lasting --fault-length seconds each time.
        #[arg(long)]
        fault_period: Option<f64>,
        #[arg(long, default_value_t = 8.0)]
        fault_length: f64,
        /// Rate change during the fault, s/d.
        #[arg(long, default_value_t = -30.0)]
        fault_rate: f64,
        /// Amplitude change during the fault, degrees.
        #[arg(long, default_value_t = -15.0)]
        fault_amplitude: f64,
    },
}

#[derive(Subcommand)]
enum WatchesAction {
    /// Each watch with its calibre, number of takes, newest take and size,
    /// and how many takes wait in Unsorted.
    List {
        /// Accepted and ignored: every watch is listed (watches are no
        /// longer archived).
        #[arg(long)]
        all: bool,
    },
    /// One watch's details and every take, newest first, each with its
    /// folder to pass to `session`, `long` or `series`. `Unsorted` shows
    /// the takes Quick Listen made.
    Show { name: String },
    #[command(flatten)]
    Change(watches_edit::Change),
}

#[derive(Subcommand)]
enum ClockAction {
    /// The stored corrections and where they are kept.
    List,
    /// Time an input against the system clock (which must be kept by
    /// NTP). Nothing needs to be on the microphone. Twenty minutes gives
    /// the error to a fraction of a ppm.
    Measure {
        /// Sound input: part of its name or id from `devices` (default:
        /// the system's default input).
        #[arg(long)]
        device: Option<String>,
        #[arg(long, default_value_t = timegrapher_core::clock::MEASURE_S / 60.0)]
        minutes: f64,
        /// Keep the result for this input.
        #[arg(long)]
        save: bool,
    },
    /// Read the error from a recording's clock log (audio position
    /// against NTP time; see `long --clock`).
    FromLog {
        log: PathBuf,
        /// The recording the log belongs to (for its sample rate).
        recording: PathBuf,
        /// The input it was recorded on, as `devices` names it.
        #[arg(long)]
        device: String,
        /// Keep the result for this input.
        #[arg(long)]
        save: bool,
    },
    /// Store a known error by hand: ppm fast, negative if slow.
    Set {
        device: String,
        #[arg(allow_hyphen_values = true, value_parser = parse_card_ppm)]
        ppm: f64,
    },
    /// Remove an input's stored correction.
    Forget { device: String },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let res = match cli.command {
        Command::Regress {
            root,
            baseline,
            write_baseline,
            takes,
            jobs,
            rate_margin,
            amplitude_margin,
            beat_error_margin,
            all,
            json,
            quiet,
            fetch,
            smoke,
            holdout,
            reveal,
        } => {
            let o = regress_cmd::Options {
                fetch,
                smoke,
                holdout,
                reveal,
                root,
                baseline,
                write_baseline,
                takes,
                jobs,
                margins: regress_cmd::Margins {
                    rate_s_per_day: rate_margin,
                    amplitude_deg: amplitude_margin,
                    beat_error_ms: beat_error_margin,
                    ..Default::default()
                },
                all,
                json,
                quiet,
            };
            match regress_cmd::run(&o) {
                Ok(true) => Ok(()),
                Ok(false) => return ExitCode::from(3),
                Err(e) => Err(e),
            }
        }
        c => run(c),
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Every command but `regress`, which says how it ended in its exit code.
fn run(c: Command) -> Result<(), String> {
    match c {
        c @ (Command::Analyze { .. } | Command::Shape { .. } | Command::Profile { .. }) => {
            measure_command(c)
        }
        c @ (Command::Long { .. } | Command::Series { .. }) => long_command(c),
        c @ (Command::Session { .. } | Command::Report { .. }) => session_command(c),
        Command::Excerpt {
            recording,
            out,
            from,
            to,
            json,
        } => excerpt_cmd::run(&excerpt_cmd::Options {
            recording,
            out,
            from,
            to,
            json,
        }),
        Command::Train {
            calibre,
            bph,
            escape,
            fourth,
            third,
            centre,
            barrel,
            period,
            json,
        } => train_cmd::run(&train_cmd::Options {
            calibre,
            bph,
            counts: [escape, fourth, third, centre, barrel],
            periods: period,
            json,
        }),
        c @ (Command::Synth { .. }
        | Command::Calibres { .. }
        | Command::Watches { .. }
        | Command::Devices { .. }
        | Command::Update { .. }) => table_command(c),
        c @ (Command::Clock { .. } | Command::Doctor { .. } | Command::Record { .. }) => {
            tool_command(c)
        }
        // Listed so a new command can't go unrouted.
        Command::Regress { .. } => unreachable!("main runs regress itself"),
    }
}

/// Analyze, shape and profile: the commands that read one recording.
fn measure_command(c: Command) -> Result<(), String> {
    match c {
        Command::Analyze {
            file,
            calibre,
            bph,
            lift,
            notch,
            highpass,
            escape_teeth,
            beats,
            windows,
            trend,
            trend_average,
            device,
            card_ppm,
            json,
        } => calibres_cmd::resolve_for(&file, calibre.as_deref(), bph, lift, escape_teeth)
            .and_then(|r| {
                let mut cfg = AnalysisConfig {
                    bph: r.bph,
                    escape_teeth: r.escape_teeth,
                    lift_known: r.lift_known,
                    ..Default::default()
                };
                cfg.amplitude.lift_deg = r.lift_deg;
                cfg.envelope.notch_hz = notch;
                cfg.envelope.highpass_hz = highpass;
                let take = session_cmd::take_of(&file, bph, lift, calibre.clone());
                clock_cmd::correction(&file, device.as_deref(), card_ppm).and_then(|c| {
                    run_analyze(
                        &file,
                        &cfg,
                        &r.train,
                        (
                            beats,
                            windows,
                            trend.map(|path| TrendOut {
                                path,
                                average_s: trend_average,
                                card_ppm,
                                device: device.clone(),
                            }),
                        ),
                        c,
                        take,
                        json,
                    )
                })
            }),
        Command::Shape {
            file,
            bph,
            lift,
            notch,
            highpass,
            json,
            templates,
        } => {
            let r = calibres_cmd::resolve_for(&file, None, bph, lift, None)?;
            let envelope = timegrapher_core::dsp::EnvelopeConfig {
                notch_hz: notch,
                highpass_hz: highpass,
                ..Default::default()
            };
            let o = shape_cmd::Options {
                bph: r.bph,
                envelope,
                lift_deg: r.lift_deg,
                json,
                templates: templates.as_deref(),
            };
            shape_cmd::run(&file, &o)
        }
        c @ Command::Profile { .. } => profile_command(c),
        _ => unreachable!("run passes measure_command only Analyze, Shape, Profile"),
    }
}

/// Profile: the sound of the Tick and of the Tock over a stretch.
fn profile_command(c: Command) -> Result<(), String> {
    let Command::Profile {
        file,
        at,
        span,
        bph,
        lift,
        notch,
        highpass,
        json,
        svg,
    } = c
    else {
        unreachable!("measure_command passes profile_command only Profile")
    };
    let r = calibres_cmd::resolve_for(&file, None, bph, lift, None)?;
    let mut cfg = AnalysisConfig {
        bph: r.bph,
        ..Default::default()
    };
    cfg.amplitude.lift_deg = r.lift_deg;
    cfg.envelope.notch_hz = notch;
    cfg.envelope.highpass_hz = highpass;
    let o = profile_cmd::Options {
        cfg,
        at_s: at,
        span_s: span,
        json,
        svg: svg.as_deref(),
    };
    profile_cmd::run(&file, &o)
}

/// Long and series: the commands that read a long run.
fn long_command(c: Command) -> Result<(), String> {
    match c {
        Command::Long {
            files,
            clock,
            device,
            card_ppm,
            bph,
            lift,
            notch,
            highpass,
            calibre,
            escape_teeth,
            wheel,
            out,
            json,
        } => {
            let mut cfg = StreamConfig::default();
            cfg.analysis.bph = bph;
            cfg.analysis.envelope.notch_hz = notch;
            cfg.analysis.envelope.highpass_hz = highpass;
            long::run(
                &files,
                long::ClockArgs {
                    log: clock.as_deref(),
                    device: device.as_deref(),
                    card_ppm,
                },
                &cfg,
                lift,
                long::Train {
                    calibre: calibre.as_deref(),
                    escape_teeth,
                },
                &wheel,
                out,
                json,
            )
        }
        Command::Series {
            files,
            clock,
            bph,
            lift,
            notch,
            highpass,
            calibre,
            escape_teeth,
            wheel,
            reading,
            wind_h,
            stretch_min,
            json,
            views,
            maps,
        } => {
            let mut cfg = StreamConfig::default();
            cfg.analysis.bph = bph;
            cfg.analysis.envelope.notch_hz = notch;
            cfg.analysis.envelope.highpass_hz = highpass;
            if reading.is_nan() || reading <= 0.0 {
                Err("--reading must be a positive number of seconds".to_string())
            } else if stretch_min.is_nan() || stretch_min < 0.0 {
                Err("--stretch-min must be 0 or a positive number of minutes".to_string())
            } else {
                series_cmd::run(series_cmd::Options {
                    files: &files,
                    clock_log: clock.as_deref(),
                    stream: cfg,
                    lift,
                    train: long::Train {
                        calibre: calibre.as_deref(),
                        escape_teeth,
                    },
                    wheels: &wheel,
                    check: timegrapher_core::steadiness::Config {
                        rate_reading_s: reading,
                        wind_h,
                        stretch_s: stretch_min * 60.0,
                        ..Default::default()
                    },
                    json,
                    views,
                    maps,
                })
            }
        }
        _ => unreachable!("run passes long_command only Long, Series"),
    }
}

/// Session and report.
fn session_command(c: Command) -> Result<(), String> {
    match c {
        Command::Session {
            paths,
            init,
            bph,
            lift,
            calibre,
            settle,
            no_shape,
            no_cycles,
            out,
            json,
        } => session_cmd::run(
            &paths,
            &session_cmd::Options {
                bph,
                lift,
                calibre,
                settle,
                out,
                json,
                init,
                no_shape,
                no_cycles,
            },
        ),
        Command::Report {
            recording,
            at,
            span,
            no_audio,
            no_analysis,
            kind,
            note,
            bph,
            lift,
            out,
            listened_back,
            cc0,
            replay,
            json,
        } => match replay {
            Some(b) => bundle_cmd::replay(&b, json),
            None => {
                let kind = kind.unwrap_or(if recording.is_empty() {
                    bundle_cmd::Kind::Bug
                } else {
                    bundle_cmd::Kind::Reading
                });
                bundle_cmd::run(&bundle_cmd::Options {
                    recording,
                    at,
                    span,
                    no_audio,
                    no_analysis,
                    kind,
                    note,
                    bph,
                    lift,
                    out,
                    json,
                    listened_back,
                    cc0,
                })
            }
        },
        _ => unreachable!("run passes session_command only Session, Report"),
    }
}

/// Synth, the calibre table, watches, devices and updates.
fn table_command(c: Command) -> Result<(), String> {
    match c {
        Command::Synth {
            out,
            scenario,
            seed,
            duration,
            bph,
            amplitude,
            rate,
            beat_error,
            snr,
            fault_period,
            fault_length,
            fault_rate,
            fault_amplitude,
        } => match (scenario.as_deref(), out) {
            (Some("list"), _) => {
                print!("{}", synth_cmd::list());
                Ok(())
            }
            (Some(name), Some(out)) => synth_cmd::write(&out, name, seed),
            (_, None) => Err("synth needs OUT: a WAV file, or with --scenario a folder".into()),
            (None, Some(out)) => {
                let cfg = SynthConfig {
                    duration_s: duration,
                    bph,
                    rate_s_per_day: rate,
                    beat_error_ms: beat_error,
                    snr_db: snr,
                    ..Default::default()
                };
                let bad = |t: f64| fault_period.is_some_and(|p| t % p < fault_length);
                let audio = synth::generate(
                    &cfg,
                    |t| amplitude + if bad(t) { fault_amplitude } else { 0.0 },
                    |t| if bad(t) { fault_rate } else { 0.0 },
                );
                timegrapher_core::audio::write_wav(&out, &audio).map_err(|e| e.to_string())
            }
        },
        Command::Calibres { name, search, json } => {
            calibres_cmd::run(name.as_deref(), search.as_deref(), json)
        }
        Command::Watches {
            action,
            library,
            json,
        } => match action {
            WatchesAction::List { all } => watches_cmd::list(library.as_deref(), all, json),
            WatchesAction::Show { name } => watches_cmd::show(&name, library.as_deref(), json),
            WatchesAction::Change(c) => watches_edit::run(&c, library.as_deref(), json),
        },
        Command::Devices {
            json,
            agc,
            agc_off,
            agc_on,
        } if agc || agc_off || agc_on => {
            run_usb_agc(json, agc_off.then_some(false).or(agc_on.then_some(true)))
        }
        Command::Devices { json, .. } => run_devices(json),
        Command::Update { json } => update_cmd::run(json),
        _ => {
            unreachable!("run passes table_command only Synth, Calibres, Watches, Devices, Update")
        }
    }
}

/// The clock, the doctor and recording.
fn tool_command(c: Command) -> Result<(), String> {
    match c {
        Command::Clock { action, json } => {
            let a = match action {
                ClockAction::List => clock_cmd::Action::List,
                ClockAction::Measure {
                    device,
                    minutes,
                    save,
                } => clock_cmd::Action::Measure {
                    device,
                    minutes,
                    save,
                },
                ClockAction::FromLog {
                    log,
                    recording,
                    device,
                    save,
                } => clock_cmd::Action::FromLog {
                    log,
                    recording,
                    device,
                    save,
                },
                ClockAction::Set { device, ppm } => clock_cmd::Action::Set { device, ppm },
                ClockAction::Forget { device } => clock_cmd::Action::Forget { device },
            };
            clock_cmd::run(a, json)
        }
        Command::Doctor {
            device,
            card,
            seconds,
            file,
            save,
            apply,
            bph,
            json,
        } => {
            let o = doctor::Options {
                device,
                card,
                seconds,
                file,
                save,
                apply,
                json,
                bph,
            };
            doctor::run(&o)
        }
        Command::Record {
            folder,
            library,
            position,
            seconds,
            device,
            file,
            watch,
            calibre,
            bph,
            lift,
            wind_h,
            json,
            live,
            power_reserve,
            stop_file,
        } => {
            let o = record_cmd::Options {
                folder,
                library,
                position,
                seconds,
                device,
                file,
                watch,
                calibre,
                bph,
                lift,
                wind_h,
                json,
                live,
                power_reserve,
                stop_file,
            };
            record_cmd::run(&o)
        }
        _ => unreachable!("run passes tool_command only Clock, Doctor, Record"),
    }
}

fn run_analyze(
    file: &Path,
    cfg: &AnalysisConfig,
    train: &long::ResolvedTrain,
    (beats, windows, trend): (Option<PathBuf>, Option<PathBuf>, Option<TrendOut>),
    clock: Option<(f64, String)>,
    take: Option<serde_json::Value>,
    json: bool,
) -> Result<(), String> {
    let audio = load(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut a = analyze(&audio, cfg);
    let calibre = train.calibre.as_ref();
    // A calibre's own wheels name the periodic components; with none,
    // only an escape wheel `--escape-teeth` gives does.
    let wheels = train.analysis_wheels(a.summary.bph);
    calibres_cmd::rename(&mut a.summary.timing_periods, &wheels);
    calibres_cmd::rename(&mut a.summary.amplitude_periods, &wheels);
    // The clock is core's pick for a take with no clock log
    // (`take::clock_from`): the recording's USB lines when it has them,
    // else a stored or given correction, said when the two disagree (#635).
    if clock.is_some() {
        a.summary.clock_known();
    }
    let usb = timegrapher_core::analyze_doc::usb_clock(&a.summary);
    let ppm = |c: &Option<(f64, String)>| c.as_ref().map(|(p, _)| *p);
    use timegrapher_core::take::{clock as take_clock, clock_from, ClockFrom};
    let fit = take_clock(None, 0, 0, ppm(&usb), ppm(&clock), a.summary.duration_s)?;
    if let Some(d) = fit.as_ref().and_then(|f| f.sources_disagree()) {
        timegrapher_core::measure::add(&mut a.summary.caveats, d.into());
    }
    let clock = match clock_from(false, ppm(&usb), ppm(&clock)) {
        Some(ClockFrom::Usb) => usb,
        Some(_) => clock,
        None => None,
    };
    if let Some(p) = beats {
        write_beats(&p, &a).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    if let Some(p) = windows {
        write_windows(&p, &a).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    if let Some(t) = trend {
        write_trend(file, cfg, &t)?;
    }
    if json {
        use timegrapher_core::analyze_doc;
        let settings = analyze_doc::settings(cfg, calibre.map(|c| c.name.as_str()));
        let mut body = analyze_doc::body(
            &a.summary,
            clock.as_ref().map(|(ppm, from)| (*ppm, from.as_str())),
        )?;
        // The file is a take's: what `session` reads of it, beside the
        // whole file read alone.
        if let Some(t) = take {
            body["take"] = t;
        }
        output::print(
            "analyze",
            output::input(&[file.to_path_buf()], settings),
            &body,
        )?;
    } else {
        if let Some(c) = calibre {
            println!(
                "Calibre      {}: {}lift {}°, as set unless a flag said otherwise",
                c.name,
                c.bph.map_or(String::new(), |b| format!("{b} bph, ")),
                cfg.amplitude.lift_deg
            );
        }
        // A listed file leads with what `session` reads of it, as the MCP
        // lead does (#695, #826); the whole file read alone follows.
        if let Some(t) = &take {
            print_take(t);
        }
        if take.as_ref().is_some_and(|t| t["readings"].is_array()) {
            println!("{WHOLE_FILE}");
        }
        print_summary(&a, clock.as_ref(), calibre);
    }
    Ok(())
}

/// The line between a listed file's take readings and the whole file's.
const WHOLE_FILE: &str =
    "Whole file   read alone, with no settling and none of the take's settings:";

/// What `session` reads of a take's file ([`session_cmd::take_of`]), one
/// line for each entry naming it ([`take_lines`]).
fn print_take(t: &serde_json::Value) {
    if let Some(error) = t["error"].as_str() {
        eprintln!(
            "note: {} could not be read for this file's take: {error}",
            t["session_file"].as_str().unwrap_or("")
        );
        return;
    }
    for line in take_lines(t) {
        println!("{line}");
    }
}

/// The take block's lines: who lists the file, then each entry's rate,
/// amplitude and beat error with their ± (core's `pm_text`), as the MCP
/// lead gives them, and each entry left out.
fn take_lines(t: &serde_json::Value) -> Vec<String> {
    let mut lines = vec![format!(
        "Take         {} lists this file; `timegrapher session` reads it:",
        t["session_file"].as_str().unwrap_or("")
    )];
    for r in t["readings"].as_array().into_iter().flatten() {
        let m = &r["measurement"];
        let at = |k: &str| m[k].as_f64().map_or("-".into(), |x| fixed_text(x, 0));
        let said = timegrapher_core::reading_text::measurement(m);
        lines.push(format!(
            "             {} from {} to {} s: {said}",
            position_label(&r["position"]),
            at("start_s"),
            at("end_s"),
        ));
    }
    for l in t["left_out"].as_array().into_iter().flatten() {
        lines.push(format!(
            "             {} is left out: {}",
            position_label(&l["position"]),
            l["why"].as_str().unwrap_or("")
        ));
    }
    lines
}

/// A position as core names it ("Crown Up (CU, 3H)"); a code core does
/// not know stays as given.
fn position_label(p: &serde_json::Value) -> String {
    let p = p.as_str().unwrap_or("-");
    timegrapher_core::reading_text::position_name(p)
}

/// The clock correction applied to a rate read on the sound card.
pub fn clock_json(ppm: f64, from: &str, card_rate: Option<f64>) -> serde_json::Value {
    timegrapher_core::clockstore::correction_json(ppm, from, card_rate)
}

#[cfg(feature = "live")]
fn run_devices(json: bool) -> Result<(), String> {
    let devs = timegrapher_core::capture::list()?;
    if json {
        return output::print(
            "devices",
            serde_json::json!({ "host": timegrapher_core::capture::host() }),
            &serde_json::json!({ "devices": devs }),
        );
    }
    if devs.is_empty() {
        println!("No sound inputs found.");
    }
    for d in &devs {
        println!(
            "{}{}",
            d.name,
            if d.is_default { "  (default)" } else { "" }
        );
        println!("    id {}", d.id);
        for c in &d.supported {
            let rate = if c.min_sample_rate == c.max_sample_rate {
                format!("{} Hz", c.min_sample_rate)
            } else {
                format!("{}-{} Hz", c.min_sample_rate, c.max_sample_rate)
            };
            let ch = if c.min_channels == c.max_channels {
                c.min_channels.to_string()
            } else {
                format!("{}-{}", c.min_channels, c.max_channels)
            };
            println!("    {ch} channel(s), {rate}, {}", c.sample_format);
        }
    }
    Ok(())
}

#[cfg(not(feature = "live"))]
fn run_devices(_json: bool) -> Result<(), String> {
    Err("this build has no sound input support; build with --features live".into())
}

/// `devices --agc`, `--agc-off`, `--agc-on`: each USB microphone's
/// automatic gain switch, read and set over USB.
fn run_usb_agc(json: bool, set: Option<bool>) -> Result<(), String> {
    // On Windows the system's own control, input by input.
    #[cfg(windows)]
    {
        let found = timegrapher_core::mixer::endpoint_agc(None, set)?;
        if json {
            return output::print(
                "usb_agc",
                serde_json::json!({ "set": set }),
                &serde_json::json!({ "microphones": found }),
            );
        }
        if found.is_empty() {
            println!("No input with an automatic gain switch found.");
        }
        let state = |v: Option<bool>| match v {
            Some(true) => "on",
            Some(false) => "off",
            None => "not read",
        };
        for f in &found {
            println!("{}", f.name);
            println!("    automatic gain {}", state(f.before));
            if set.is_some() {
                println!("    after the change {}", state(f.after));
            }
            if let Some(e) = &f.error {
                println!("    {e}");
            }
        }
        return Ok(());
    }
    #[allow(unreachable_code)]
    let found = timegrapher_core::mixer::usb_agc(set)?;
    if json {
        return output::print(
            "usb_agc",
            serde_json::json!({ "set": set }),
            &serde_json::json!({ "microphones": found }),
        );
    }
    if found.is_empty() {
        println!("No USB microphone with an automatic gain switch found.");
    }
    let state = |v: Option<bool>| match v {
        Some(true) => "on",
        Some(false) => "off",
        None => "not read",
    };
    for f in &found {
        println!(
            "{} ({:04x}:{:04x}), unit {} channel {}",
            f.name, f.vendor_id, f.product_id, f.control.unit, f.control.channel
        );
        println!("    automatic gain {}", state(f.before));
        if set.is_some() {
            println!("    after the change {}", state(f.after));
        }
        if let Some((cur, lo, hi)) = f.volume_db {
            println!(
                "    capture volume {} dB (from {} to {} dB)",
                signed_text(cur, 1),
                signed_text(lo, 1),
                signed_text(hi, 1)
            );
        }
        if let Some(e) = &f.error {
            println!("    {e}");
        }
    }
    Ok(())
}

fn opt(v: Option<f64>, digits: usize) -> String {
    v.map_or("-".into(), |x| fixed_text(x, digits))
}

/// Beat error from the unlock as the headline, with the drop's alongside;
/// from the drop, why (`why`, core's reason: the unlock not found, or its
/// signs disagreeing, #1439).
pub fn beat_error_text(
    unlock_ms: Option<f64>,
    drop_ms: f64,
    why: timegrapher_core::measure::DropWhy,
) -> String {
    match unlock_ms {
        Some(u) => format!(
            "{} ms (from the unlock; {} ms from the drop)",
            fixed_text(u.abs(), base_digits::BEAT_ERROR),
            fixed_text(drop_ms.abs(), base_digits::BEAT_ERROR)
        ),
        None => format!(
            "{} ms from the drop ({})",
            fixed_text(drop_ms.abs(), base_digits::BEAT_ERROR),
            why.words()
        ),
    }
}

fn print_summary(
    a: &Analysis,
    clock: Option<&(f64, String)>,
    calibre: Option<&timegrapher_core::take::Calibre>,
) {
    use timegrapher_core::measure;
    let s = &a.summary;
    println!(
        "Duration     {} s at {} Hz",
        fixed_text(s.duration_s, 1),
        s.sample_rate
    );
    if !s.watch_heard {
        let reason = measure::no_watch_reason_in(&s.caveats).unwrap_or_default();
        println!("{}", measure::no_watch_text(reason, s.jitter_us, s.bph_set));
        return;
    }
    if !measure::regularity_assessed(s.duration_s) {
        println!(
            "Beat regularity not assessed: {} s of sound, {} s needed",
            fixed_text(s.duration_s, 1),
            measure::SHORT_SPAN_S
        );
    }
    println!(
        "Beat rate    {} bph, {} beats found ({}% of those expected)",
        s.bph,
        s.beats_found,
        fixed_text(s.beats_found_fraction * 100.0, 1)
    );
    if let Some(heard) = s.bph_heard {
        println!(
            "Set rate     set {} bph; the beats are {heard} bph apart",
            s.bph
        );
    }
    print_rate(s, clock);
    println!(
        "Jitter       {} us per beat (within 10 s windows)",
        opt(s.jitter_us, 0)
    );
    println!(
        "Rate spread  {} to {} s/d (5th-95th percentile of 10 s windows)",
        opt(s.rate_p05, 1),
        opt(s.rate_p95, 1)
    );
    // Tick and Tock have no ± of their own; they are at the amplitude's
    // place, as the figure beside them (#1470).
    let side = |x: Option<f64>| {
        x.map_or("-".into(), |x| {
            figure(x, s.amplitude_se_deg, base_digits::AMPLITUDE, false)
        })
    };
    println!(
        "Amplitude    {} deg (Tick {}, Tock {}; lift angle {} deg)",
        s.amplitude_deg.map_or("-".into(), |a| figure_with_pm(
            a,
            s.amplitude_se_deg,
            base_digits::AMPLITUDE,
            false
        )),
        side(s.amplitude_even_deg),
        side(s.amplitude_odd_deg),
        s.lift_deg
    );
    print_conditions(s, calibre);
    print_shape_and_periods(s);
}

/// The rate, corrected for the card's clock where it can be, and the beat
/// error, each with its ± at the figure's place (core's `figure_with_pm`,
/// #1470). The rate on the card's clock and the beat error from the drop
/// take the same place as the figure beside them.
fn print_rate(s: &timegrapher_core::analysis::Summary, clock: Option<&(f64, String)>) {
    let c = s.confidence.as_ref();
    if let Some(f) = &s.overall {
        let rate_se = c.and_then(|c| c.rate.se);
        match clock {
            Some((ppm, from)) => println!(
                "Rate         {} s/d, corrected for the sound card's clock ({} ppm {}, {from}); {} s/d on the card's clock",
                figure_with_pm(
                    timegrapher_core::clockstore::correct_rate(f.rate_s_per_day, *ppm),
                    rate_se,
                    base_digits::RATE,
                    true
                ),
                fixed_text(ppm.abs(), base_digits::CLOCK),
                if *ppm >= 0.0 { "fast" } else { "slow" },
                figure(f.rate_s_per_day, rate_se, base_digits::RATE, true)
            ),
            None => println!(
                "Rate         {} s/d",
                figure_with_pm(f.rate_s_per_day, rate_se, base_digits::RATE, true)
            ),
        }
        // The ± is the unlock figure's.
        let be_se = c
            .and_then(|c| c.beat_error.se)
            .filter(|e| e.is_finite() && *e > 0.0);
        match (s.beat_error_unlock_ms, be_se) {
            (Some(u), Some(_)) => println!(
                "Beat error   {} ms (from the unlock; {} ms from the drop)",
                figure_with_pm(u.abs(), be_se, base_digits::BEAT_ERROR, false),
                figure(f.beat_error_ms.abs(), be_se, base_digits::BEAT_ERROR, false)
            ),
            _ => println!(
                "Beat error   {}",
                beat_error_text(
                    s.beat_error_unlock_ms,
                    f.beat_error_ms,
                    timegrapher_core::measure::DropWhy::of(&s.caveats)
                )
            ),
        }
    } else {
        println!("Rate         not enough clean beats to fit");
    }
}

/// The conditions the figures were read under, and clipping.
fn print_conditions(
    s: &timegrapher_core::analysis::Summary,
    calibre: Option<&timegrapher_core::take::Calibre>,
) {
    use timegrapher_core::measure::Caveat;
    let conditions: Vec<String> = s
        .caveats
        .iter()
        .filter_map(|c| match c {
            // Said on their own lines.
            Caveat::NoWatch { .. }
            | Caveat::SetRateDisagrees { .. }
            | Caveat::Clipped { .. }
            | Caveat::BeatsMissing { .. } => None,
            Caveat::LiftUnknown => c.short(s.lift_deg, s.beats_found).map(|w| {
                match timegrapher_core::take::lift_default_why(calibre) {
                    Some(why) => format!("{w}; {why}"),
                    None => w,
                }
            }),
            _ => c.short(s.lift_deg, s.beats_found),
        })
        .collect();
    for (i, line) in conditions.iter().enumerate() {
        println!("{:<13}{line}", if i == 0 { "Conditions" } else { "" });
    }
    let c = &s.clipping;
    if c.samples > 0 {
        println!(
            "Clipping     on {}% of beats ({} of {}, {} samples), peak {} dBFS",
            fixed_text(c.beat_fraction * 100.0, 1),
            c.beats,
            s.beats_found,
            c.samples,
            fixed_text(c.peak_dbfs, 1)
        );
    }
}

/// The beat's shape on each side, and the strongest periodic components.
fn print_shape_and_periods(s: &timegrapher_core::analysis::Summary) {
    if let Some(m) = &s.shape {
        let pct = |v: Option<f64>| {
            v.map_or("-".to_string(), |v| {
                format!("{}%", fixed_text(v * 100.0, 0))
            })
        };
        for (i, (side, d)) in [("Tick", &m.even), ("Tock", &m.odd)].iter().enumerate() {
            println!(
                "{:<13}{side}: sound 1 in {}, sound 2 in {} of {} windows; 1-2 {} ms, 1-3 {} ms, level 1/3 {}, tail {}",
                if i == 0 { "Shape" } else { "" },
                pct(d.share_with_sound_1),
                pct(d.share_with_sound_2),
                d.windows_measured,
                opt(d.i12_ms, 2),
                opt(d.i13_ms, 2),
                opt(d.ratio13, 2),
                opt(d.tail_ratio, 2),
            );
        }
        if let (Some(a), Some(b)) = (m.beat_error_first_quarter_ms, m.beat_error_last_quarter_ms) {
            println!(
                "{:<13}beat error from the unlock {} ms in the first quarter, {} ms in the last ({} window{} each)",
                "",
                signed_text(a, base_digits::BEAT_ERROR),
                signed_text(b, base_digits::BEAT_ERROR),
                m.beat_error_quarter_windows,
                timegrapher_core::measure::plural(m.beat_error_quarter_windows)
            );
        }
    }
    for (title, list) in [
        ("timing", &s.timing_periods),
        ("amplitude", &s.amplitude_periods),
    ] {
        if list.is_empty() {
            continue;
        }
        println!("Periodic components in {title}:");
        for c in list.iter().take(3) {
            println!(
                "  {:>8} s  explains {:>4}% of variance  {}",
                fixed_text(c.period_s, 1),
                fixed_text(c.power * 100.0, 0),
                c.wheel
                    .as_deref()
                    .map(|w| format!("({w}?)"))
                    .unwrap_or_default()
            );
        }
    }
}

fn write_beats(p: &Path, a: &Analysis) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(p)?);
    writeln!(w, "index,time_s,quality,residual_us")?;
    for (b, r) in a.beats.iter().zip(&a.residuals) {
        writeln!(
            w,
            "{},{:.6},{:.3},{:.1}",
            b.index,
            b.time,
            b.quality,
            r * 1e6
        )?;
    }
    w.flush()
}

/// Where `analyze --trend` writes, and how it reads the take.
struct TrendOut {
    path: PathBuf,
    average_s: f64,
    /// The correction the reader gives, as for the whole file.
    card_ppm: Option<f64>,
    device: Option<String>,
}

/// The readings over time as the app's charts read them: the file opened
/// whole as its take's context (core's `TakeContext` on `take::whole_entry`:
/// its beats, clock, breaks, the masks of the session file's entries
/// naming it and each entry's window),
/// cut where the app cuts a take's readings (its breaks, moves and
/// settling ends, `take::reading_cuts`), on the take's clock where it
/// corrects, then core's trend, each reading averaged over `average_s`
/// (`LiveAnalyzer::trend`), over every beat of the file (the app keeps
/// the latest `RUN_BEATS_KEPT`). A reading with no figures (not a whole
/// averaging time) is a row of blanks. `clock_ppm` is the clock fit's
/// steady ppm, blank where the rates are on the sound card's clock; a
/// fit that drifts corrects each reading by its ppm over that reading.
fn write_trend(file: &Path, cfg: &AnalysisConfig, t: &TrendOut) -> Result<(), String> {
    use timegrapher_core::live::{LiveAnalyzer, LiveConfig};
    use timegrapher_core::take;
    use timegrapher_core::take_context::{Masks, TakeContext};
    let p = &t.path;
    let average_s = t.average_s;
    // A session file that can't be read leaves the file read alone, as
    // the whole file's own reading is; `take` says why.
    let listing = session_cmd::listing_of(file).ok().flatten();
    let (base, mut m, entry) = take::whole_entry(file, listing.as_ref().map(|(m, _)| m))?;
    // A file no session file lists is a take of one entry naming it.
    if m.recordings.is_empty() {
        m.recordings.push(entry);
    }
    let settings = take::EntrySettings {
        stream: StreamConfig {
            analysis: cfg.clone(),
            ..Default::default()
        },
        card_clock: false,
        settle_s: None,
    };
    let (ctx, log) = TakeContext::open(
        &base,
        &m,
        settings,
        Masks::Cut,
        take::usb,
        |first| take::correction_for(&m, first, t.card_ppm, t.device.as_deref()),
        |_, _| {},
    )?;
    let (stretches, settles) = ctx.positions(&m);
    let changes: Vec<f64> = stretches.iter().map(|s| s.0).collect();
    // Every beat of the file, however long, not only the day a live
    // listen keeps.
    let live_cfg = LiveConfig {
        analysis: cfg.clone(),
        max_beats: usize::MAX,
        ..Default::default()
    };
    let mut live = LiveAnalyzer::from_log(log, live_cfg);
    live.set_breaks(take::reading_cuts(&ctx.breaks, &changes, &settles));
    // The settlings are left out as a mask is, so no reading falls where
    // the watch was just handled (#1233).
    live.set_masked(&timegrapher_core::session::with_holes(
        &ctx.masked,
        &settles,
    ));
    let clock = ctx.clock.as_ref().filter(|c| c.corrects());
    live.set_rate_clock(clock);
    let trend = live.trend(average_s);
    // The rates' clock: the correction's ppm, blank on the card's.
    let clock_ppm = clock.map_or(String::new(), |c| format!("{:.3}", c.ppm));
    let write = || -> std::io::Result<()> {
        let mut w = BufWriter::new(File::create(p)?);
        writeln!(
            w,
            "t_s,average_s,rate_s_per_day,amplitude_deg,amplitude_even_deg,amplitude_odd_deg,beat_error_unlock_ms,beat_error_ms,heard,clock_ppm"
        )?;
        let v = |x: Option<f64>, d: usize| x.map_or(String::new(), |x| format!("{x:.d$}"));
        for r in &trend {
            writeln!(
                w,
                "{:.2},{average_s},{},{},{},{},{},{},{},{clock_ppm}",
                r.t,
                v(r.rate_s_per_day, 3),
                v(r.amplitude_deg, 2),
                v(r.amplitude_even_deg, 2),
                v(r.amplitude_odd_deg, 2),
                v(r.beat_error_unlock_ms, 4),
                v(r.beat_error_ms, 4),
                r.heard
            )?;
        }
        w.flush()
    };
    write().map_err(|e| format!("{}: {e}", p.display()))
}

fn write_windows(p: &Path, a: &Analysis) -> std::io::Result<()> {
    // Where each side's unlock and drop sat, ms from the beat time; the
    // sign matters here, so these are not run through `opt`.
    let edge = |v: Option<f64>| v.map_or(String::new(), |v| format!("{v:.4}"));
    let mut w = BufWriter::new(File::create(p)?);
    writeln!(
        w,
        "kind,start_s,end_s,rate_s_per_day,beat_error_ms,beat_error_unlock_ms,amplitude_even_deg,amplitude_odd_deg,unlock_even_ms,unlock_odd_ms,drop_even_ms,drop_odd_ms"
    )?;
    for r in &a.rate_windows {
        writeln!(
            w,
            "rate,{:.2},{:.2},{:.2},{:.3},,,,,,,",
            r.start_s, r.end_s, r.fit.rate_s_per_day, r.fit.beat_error_ms
        )?;
    }
    for m in &a.amplitude_windows {
        writeln!(
            w,
            "amplitude,{:.2},{:.2},,{},{},{},{},{},{},{},{}",
            m.start_s,
            m.end_s,
            m.beat_error_ms.map_or(String::new(), |v| format!("{v:.3}")),
            m.beat_error_unlock_ms
                .map_or(String::new(), |v| format!("{v:.3}")),
            opt(m.even_deg, 1).replace('-', ""),
            opt(m.odd_deg, 1).replace('-', ""),
            edge(m.even_unlock_ms),
            edge(m.odd_unlock_ms),
            edge(m.even_drop_ms),
            edge(m.odd_drop_ms)
        )?;
    }
    w.flush()
}

#[cfg(test)]
mod take_lines_tests {
    use super::*;

    #[test]
    fn take_lines_use_the_shared_reading_words() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../timegrapher-core/tests/fixtures/reading_text.json"
        ))
        .unwrap();
        for c in cases.as_array().unwrap() {
            let lines = take_lines(&serde_json::json!({"readings": [c]}));
            let expected = format!(
                "             {} from - to - s: {}",
                c["position_text"].as_str().unwrap(),
                c["text"].as_str().unwrap()
            );
            assert_eq!(lines[1], expected);
        }
    }

    /// A take entry with no regular beat says so, with no figures, and
    /// one left out says why (#826).
    #[test]
    fn take_lines_say_no_regular_beat_and_left_out() {
        let t = serde_json::json!({
            "session_file": "session.toml",
            "readings": [{
                "position": "DU",
                "measurement": { "watch_heard": false, "start_s": 10.0, "end_s": 30.0 }
            }],
            "left_out": [{ "position": "CD", "why": "nothing to read" }]
        });
        let lines = take_lines(&t);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[0].starts_with("Take         session.toml lists this file"));
        assert!(
            lines[1].ends_with("from 10 to 30 s: no regular beat found"),
            "{}",
            lines[1]
        );
        assert!(lines[1].contains("Dial Up (DU"), "{}", lines[1]);
        assert!(
            lines[2].contains("is left out: nothing to read"),
            "{}",
            lines[2]
        );
    }
}

#[cfg(test)]
mod numeric_input_tests {
    use super::*;

    fn args(command: &str) -> Vec<String> {
        let mut args = vec!["timegrapher".into(), command.into()];
        match command {
            "doctor" => {}
            "record" => args.extend(["unused-folder", "--position", "DU"].map(String::from)),
            _ => args.push("unused.wav".into()),
        }
        args
    }

    #[test]
    fn numeric_inputs_reject_invalid_explicit_scalars_before_dispatch() {
        for command in [
            "analyze", "shape", "profile", "long", "series", "doctor", "record", "session",
            "synth", "report",
        ] {
            let mut argv = args(command);
            argv.push("--bph=0".into());
            assert!(Cli::try_parse_from(argv).is_err(), "{command}: zero BPH");
        }
        for command in [
            "analyze", "shape", "profile", "long", "series", "record", "session", "report",
        ] {
            for value in ["0", "-1", "NaN", "inf", "-inf"] {
                let mut argv = args(command);
                argv.push(format!("--lift={value}"));
                assert!(
                    Cli::try_parse_from(argv).is_err(),
                    "{command}: lift {value}"
                );
            }
        }
        for command in ["analyze", "long", "series"] {
            let mut argv = args(command);
            argv.push("--escape-teeth=0".into());
            assert!(Cli::try_parse_from(argv).is_err(), "{command}: zero teeth");
        }
        for value in ["NaN", "inf", "-inf", "1000000", "1000001"] {
            for command in ["analyze", "long"] {
                let mut argv = args(command);
                argv.push(format!("--card-ppm={value}"));
                assert!(Cli::try_parse_from(argv).is_err(), "{command}: ppm {value}");
            }
            assert!(
                Cli::try_parse_from(["timegrapher", "clock", "set", "unused-device", value])
                    .is_err()
            );
        }
    }

    #[test]
    fn numeric_inputs_preserve_defaults_valid_extremes_and_report_analysis_settings() {
        let parsed = Cli::try_parse_from(args("analyze")).unwrap();
        assert!(matches!(
            parsed.command,
            Command::Analyze {
                bph: None,
                lift: None,
                escape_teeth: None,
                card_ppm: None,
                ..
            }
        ));
        let parsed = Cli::try_parse_from(args("shape")).unwrap();
        assert!(matches!(parsed.command, Command::Shape { lift: None, .. }));
        for ppm in ["0", "-12.5", "-999999.9999999999", "12.5"] {
            assert!(Cli::try_parse_from([
                "timegrapher",
                "analyze",
                "unused.wav",
                "--bph=1",
                "--lift=52",
                "--escape-teeth=1",
                &format!("--card-ppm={ppm}")
            ])
            .is_ok());
        }
        assert!(Cli::try_parse_from([
            "timegrapher",
            "analyze",
            "unused.wav",
            "--bph=4294967295",
            "--lift=90",
            "--escape-teeth=4294967295"
        ])
        .is_ok());
        // Report settings are forwarded to analysis subprocesses, even with --no-audio.
        assert!(Cli::try_parse_from([
            "timegrapher",
            "report",
            "--bph=28800",
            "--lift=52",
            "--no-audio"
        ])
        .is_ok());
    }
}

#[cfg(test)]
mod beat_error_wording {
    use super::*;
    use timegrapher_core::measure::{Caveat, DropWhy};

    #[test]
    fn a_beat_error_from_the_drop_says_why() {
        let inconsistent = [Caveat::BeatErrorFromDrop {
            why: DropWhy::UnlockInconsistent,
        }];
        let why = DropWhy::of(&inconsistent);
        assert_eq!(why, DropWhy::UnlockInconsistent);
        let text = beat_error_text(None, 0.45, why);
        assert!(text.contains(DropWhy::UnlockInconsistent.words()), "{text}");
        assert!(!text.contains(DropWhy::UnlockNotFound.words()), "{text}");
        let text = beat_error_text(None, 0.45, DropWhy::of(&[Caveat::FROM_DROP]));
        assert!(text.contains(DropWhy::UnlockNotFound.words()), "{text}");
    }
}

#[cfg(test)]
mod acquisition_defaults {
    use super::*;

    #[test]
    fn public_acquisition_defaults_remain_unchanged() {
        let doctor = Cli::try_parse_from(["timegrapher", "doctor"]).unwrap();
        assert!(matches!(
            doctor.command,
            Command::Doctor { seconds: 5.0, .. }
        ));
        let record = Cli::try_parse_from([
            "timegrapher",
            "record",
            "unused-test-folder",
            "--position",
            "DU",
        ])
        .unwrap();
        assert!(matches!(
            record.command,
            Command::Record { seconds: 120.0, .. }
        ));
        let clock = Cli::try_parse_from(["timegrapher", "clock", "measure"]).unwrap();
        assert!(matches!(
            clock.command,
            Command::Clock {
                action: ClockAction::Measure { minutes: 20.0, .. },
                ..
            }
        ));
    }
}

#[cfg(test)]
mod report_clock_context {
    use super::*;

    #[test]
    fn explicit_zero_correction_keeps_precedence_and_document_shape() {
        // The explicit reader path must not consult a device or user's store.
        let (ppm, from) = clock_cmd::correction(
            std::path::Path::new("nonexistent-report-excerpt.flac"),
            Some("nonexistent-input"),
            Some(0.0),
        )
        .unwrap()
        .unwrap();
        assert_eq!(ppm, 0.0);
        assert_eq!(from, "given with --card-ppm");
        let doc = clock_json(ppm, &from, Some(12.0));
        assert_eq!(doc["ppm"], 0.0);
        assert_eq!(doc["from"], from);
        assert_eq!(doc["rate_error_s_per_day"], 0.0);
        assert!((doc["rate_s_per_day"].as_f64().unwrap() - 12.0).abs() < 1e-9);
        assert_eq!(
            clock_json(ppm, &from, None)["rate_s_per_day"],
            serde_json::Value::Null
        );
    }
}
