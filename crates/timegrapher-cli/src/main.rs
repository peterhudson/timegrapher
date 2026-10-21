mod archive;
mod bundle_cmd;
mod calibres_cmd;
mod clock_cmd;
mod corpus;
mod doctor;
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
mod update_cmd;

use clap::{Parser, Subcommand};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
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
            .map_err(|_| "lift angle must be finite and positive".to_string())?,
    )
}
fn parse_escape_teeth(text: &str) -> Result<u32, String> {
    timegrapher_core::input_domain::escape_teeth(
        text.parse()
            .map_err(|_| "escape teeth must be a positive u32 integer".to_string())?,
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
        /// Beat rate in beats per hour (default: the calibre's, or guessed).
        #[arg(long, value_parser = parse_bph)]
        bph: Option<u32>,
        /// Lift angle in degrees (default: the calibre's, or 52, named as
        /// the default).
        #[arg(long, value_parser = parse_lift)]
        lift: Option<f64>,
        /// Comma-separated steady tones to notch out, Hz (e.g. 5000,6000,7000).
        #[arg(long, value_delimiter = ',')]
        notch: Vec<f64>,
        /// High-pass corner, Hz.
        #[arg(long, default_value_t = 1500.0)]
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
        /// Correct the rate with the clock error stored for this input
        /// (see `clock`). Without it, the input named in the recording's
        /// sidecar is used when one is stored.
        #[arg(long)]
        device: Option<String>,
        /// Correct the rate for a sound card this many ppm slow (negative
        /// if fast), instead of a stored correction.
        #[arg(long, allow_hyphen_values = true, value_parser = parse_card_ppm)]
        card_ppm: Option<f64>,
        /// Print the summary as JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Measure the three sounds of each side's beat.
    Shape {
        file: PathBuf,
        /// Beat rate in beats per hour (guessed if omitted).
        #[arg(long, value_parser = parse_bph)]
        bph: Option<u32>,
        /// Lift angle in degrees.
        #[arg(long, default_value_t = 52.0, value_parser = parse_lift)]
        lift: f64,
        /// Comma-separated steady tones to notch out, Hz.
        #[arg(long, value_delimiter = ',')]
        notch: Vec<f64>,
        /// High-pass corner, Hz.
        #[arg(long, default_value_t = 1500.0)]
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
        /// Beat rate in beats per hour (guessed if omitted).
        #[arg(long, value_parser = parse_bph)]
        bph: Option<u32>,
        /// Lift angle in degrees.
        #[arg(long, default_value_t = 52.0, value_parser = parse_lift)]
        lift: f64,
        /// Comma-separated steady tones to notch out, Hz.
        #[arg(long, value_delimiter = ',')]
        notch: Vec<f64>,
        /// High-pass corner, Hz.
        #[arg(long, default_value_t = 1500.0)]
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
        /// A folder stands for its WAV and FLAC files, sorted by name.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Clock log for calibrating the sound card: audio position against
        /// NTP-synced system time (see `docs/long-runs.md`).
        #[arg(long)]
        clock: Option<PathBuf>,
        /// Without a clock log: correct with the clock error stored for
        /// this input (see `clock`).
        #[arg(long)]
        device: Option<String>,
        /// Without a clock log: correct for a sound card this many ppm slow
        /// (negative if fast).
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
        #[arg(long, default_value_t = 1500.0)]
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
        /// FLAC files, sorted by name.
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
        #[arg(long, default_value_t = 1500.0)]
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
        /// With --json, add what the app's Steadiness pane draws behind
        /// each verdict (the readings, the period search and the fold):
        /// a few hundred kB on a long take.
        #[arg(long, requires = "json")]
        views: bool,
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
    /// List the sound inputs, with the sample rates and formats each supports.
    Devices {
        /// Print the list as JSON instead of text.
        #[arg(long)]
        json: bool,
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
        folder: PathBuf,
        /// Position: DU, DD, CD, CL, CU or CR (or CH, CB, 9H, 6H, 3H, 12H).
        #[arg(long)]
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
        /// Print the result as JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Keep the latest readings in this file while recording, replaced
        /// about once a second (`timegrapher.live/1`), for a live view.
        /// With --file the file is played back in real time.
        #[arg(long)]
        live: Option<PathBuf>,
    },
    /// Read a watch measured in several positions (and states of wind)
    /// into one multi-position report, Witschi style.
    Session {
        /// A session file (session.toml), a folder holding one, or the
        /// recordings themselves with the position in each file name
        /// (DU, DD, CU, CD, CL, CR or CH, CB, 3H, 6H, 9H, 12H).
        #[arg(required = true)]
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
        #[arg(long, conflicts_with_all = ["recording", "no_audio", "note", "out", "cc0", "listened_back"])]
        replay: Option<PathBuf>,
        /// Print the manifest (or the replay) as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Write a synthetic watch recording (for testing).
    Synth {
        /// The WAV file to write, or with --scenario the take folder.
        out: Option<PathBuf>,
        /// Write a take with a known hidden variation instead, as a take
        /// folder (FLAC files, session.json, a README saying what is
        /// hidden); `list` names them.
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
        #[arg(long, default_value_t = 20.0)]
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
    /// Store a known error by hand: ppm slow, negative if fast.
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
            device,
            card_ppm,
            json,
        } => calibres_cmd::resolve(calibre.as_deref(), bph, lift, escape_teeth).and_then(|r| {
            let mut cfg = AnalysisConfig {
                bph: r.bph,
                escape_teeth: r.escape_teeth,
                lift_known: r.lift_known,
                ..Default::default()
            };
            cfg.amplitude.lift_deg = r.lift_deg;
            cfg.envelope.notch_hz = notch;
            cfg.envelope.highpass_hz = highpass;
            clock_cmd::correction(&file, device.as_deref(), card_ppm)
                .and_then(|c| run_analyze(&file, &cfg, &r.train, beats, windows, c, json))
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
            let envelope = timegrapher_core::dsp::EnvelopeConfig {
                notch_hz: notch,
                highpass_hz: highpass,
                ..Default::default()
            };
            let o = shape_cmd::Options {
                bph,
                envelope,
                lift_deg: lift,
                json,
                templates: templates.as_deref(),
            };
            shape_cmd::run(&file, &o)
        }
        Command::Profile {
            file,
            at,
            span,
            bph,
            lift,
            notch,
            highpass,
            json,
            svg,
        } => {
            let mut cfg = AnalysisConfig {
                bph,
                ..Default::default()
            };
            cfg.amplitude.lift_deg = lift;
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
                })
            }
        }
        Command::Calibres { name, search, json } => {
            calibres_cmd::run(name.as_deref(), search.as_deref(), json)
        }
        Command::Devices { json } => run_devices(json),
        Command::Update { json } => update_cmd::run(json),
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
        } => {
            let o = record_cmd::Options {
                folder,
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
            };
            record_cmd::run(&o)
        }
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
        Command::Report {
            recording,
            at,
            span,
            no_audio,
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
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_analyze(
    file: &Path,
    cfg: &AnalysisConfig,
    train: &long::ResolvedTrain,
    beats: Option<PathBuf>,
    windows: Option<PathBuf>,
    clock: Option<(f64, String)>,
    json: bool,
) -> Result<(), String> {
    let audio = load(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut a = analyze(&audio, cfg);
    let calibre = train.calibre.as_ref();
    // A calibre's own wheels name the periodic components.
    if calibre.is_some() {
        let wheels = train.analysis_wheels(a.summary.bph);
        calibres_cmd::rename(&mut a.summary.timing_periods, &wheels);
        calibres_cmd::rename(&mut a.summary.amplitude_periods, &wheels);
    }
    // A stored or given correction wins; otherwise the USB lines in the
    // recording, when it has them.
    let clock = match clock {
        Some(c) => {
            a.summary.clock_known();
            Some(c)
        }
        None => a.summary.usb_clock.map(|c| c.correction()),
    };
    if let Some(p) = beats {
        write_beats(&p, &a).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    if let Some(p) = windows {
        write_windows(&p, &a).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    if json {
        let settings = serde_json::json!({
            "calibre": calibre.map(|c| &c.name),
            "bph": cfg.bph,
            "lift_deg": cfg.amplitude.lift_deg,
            "notch_hz": cfg.envelope.notch_hz,
            "highpass_hz": cfg.envelope.highpass_hz,
            "escape_teeth": cfg.escape_teeth,
        });
        // The summary's rates stay on the sound card's clock (what tg reads
        // and the regression check compares); the correction goes beside
        // them.
        let mut body = serde_json::to_value(&a.summary).map_err(|e| e.to_string())?;
        if let Some((ppm, from)) = &clock {
            body["clock"] = clock_json(
                *ppm,
                from,
                a.summary.overall.as_ref().map(|f| f.rate_s_per_day),
            );
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
        print_summary(&a, clock.as_ref(), calibre.is_some());
    }
    Ok(())
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

fn opt(v: Option<f64>, digits: usize) -> String {
    v.map_or("-".into(), |x| format!("{x:.digits$}"))
}

/// Beat error from the unlock as the headline, with the drop's alongside.
pub fn beat_error_text(unlock_ms: Option<f64>, drop_ms: f64) -> String {
    match unlock_ms {
        Some(u) => format!(
            "{:.2} ms (from the unlock; {:.2} ms from the drop)",
            u.abs(),
            drop_ms.abs()
        ),
        None => format!("{:.2} ms from the drop (unlock not found)", drop_ms.abs()),
    }
}

fn print_summary(a: &Analysis, clock: Option<&(f64, String)>, calibre_set: bool) {
    use timegrapher_core::measure::{self, Caveat};
    let s = &a.summary;
    println!("Duration     {:.1} s at {} Hz", s.duration_s, s.sample_rate);
    if !s.watch_heard {
        println!(
            "{}",
            timegrapher_core::beats::no_regular_beat_text(s.jitter_us, s.timing_scatter_fraction)
        );
        return;
    }
    if !measure::regularity_assessed(s.duration_s) {
        println!(
            "Beat regularity not assessed: {:.1} s of sound, {} s needed",
            s.duration_s,
            measure::SHORT_SPAN_S
        );
    }
    println!(
        "Beat rate    {} bph, {} beats found ({:.1}% of those expected)",
        s.bph,
        s.beats_found,
        s.beats_found_fraction * 100.0
    );
    if let Some(heard) = s.bph_heard {
        println!(
            "Set rate     set {} bph; the beats are {heard} bph apart",
            s.bph
        );
    }
    let se = |r: Option<&timegrapher_core::confidence::Reading>, unit: &str, d: usize| {
        r.and_then(|r| r.se)
            .map_or(String::new(), |e| format!(" ± {e:.d$}{unit}"))
    };
    let c = s.confidence.as_ref();
    if let Some(f) = &s.overall {
        let pm = se(c.map(|c| &c.rate), "", 1);
        match clock {
            Some((ppm, from)) => println!(
                "Rate         {:+.1}{pm} s/d, corrected for the sound card's clock ({:.1} ppm {}, {from}); {:+.1} s/d on the card's clock",
                timegrapher_core::clockstore::correct_rate(f.rate_s_per_day, *ppm),
                ppm.abs(),
                if *ppm >= 0.0 { "slow" } else { "fast" },
                f.rate_s_per_day
            ),
            None => println!("Rate         {:+.1}{pm} s/d", f.rate_s_per_day),
        }
        // The ± is the unlock figure's.
        let be_se = c.and_then(|c| c.beat_error.se);
        match (s.beat_error_unlock_ms, be_se) {
            (Some(u), Some(e)) => println!(
                "Beat error   {:.2} ± {e:.2} ms (from the unlock; {:.2} ms from the drop)",
                u.abs(),
                f.beat_error_ms.abs()
            ),
            _ => println!(
                "Beat error   {}",
                beat_error_text(s.beat_error_unlock_ms, f.beat_error_ms)
            ),
        }
    } else {
        println!("Rate         not enough clean beats to fit");
    }
    println!(
        "Jitter       {} us per beat (within 10 s windows)",
        opt(s.jitter_us, 0)
    );
    println!(
        "Rate spread  {} to {} s/d (5th-95th percentile of 10 s windows)",
        opt(s.rate_p05, 1),
        opt(s.rate_p95, 1)
    );
    println!(
        "Amplitude    {}{} deg (Tick {}, Tock {}; lift angle {} deg)",
        opt(s.amplitude_deg, 0),
        s.amplitude_se_deg.map_or(String::new(), |e| if e < 1.0 {
            format!(" ± {e:.1}")
        } else {
            format!(" ± {e:.0}")
        }),
        opt(s.amplitude_even_deg, 0),
        opt(s.amplitude_odd_deg, 0),
        s.lift_deg
    );
    let conditions: Vec<String> = s
        .caveats
        .iter()
        .filter_map(|c| match c {
            Caveat::ShortSpan { span_s } => Some(format!(
                "the beats span {span_s:.1} s (under {} s)",
                measure::SHORT_SPAN_S
            )),
            Caveat::LiftUnknown => Some(format!(
                "lift {}°, the default; {}",
                s.lift_deg,
                if calibre_set {
                    "the calibre's lift angle is not in the table"
                } else {
                    "calibre not set"
                }
            )),
            Caveat::ClockUnknown => Some("clock: card clock, not corrected".to_string()),
            Caveat::Breaks { count } => Some(format!("{count} breaks in the take")),
            Caveat::SidesByCount { count } => Some(format!(
                "Tick and Tock named by the beat count across {count} break{}, \
                 not told apart by their sound",
                if *count == 1 { "" } else { "s" }
            )),
            Caveat::Masked { seconds } if *seconds < 0.05 => None,
            Caveat::Masked { seconds } if *seconds < 9.95 => {
                Some(format!("{seconds:.1} s masked out"))
            }
            Caveat::Masked { seconds } => Some(format!("{seconds:.0} s masked out")),
            // Said on their own lines.
            Caveat::NoWatch
            | Caveat::SetRateDisagrees { .. }
            | Caveat::Clipped { .. }
            | Caveat::BeatsMissing { .. } => None,
        })
        .collect();
    for (i, line) in conditions.iter().enumerate() {
        println!("{:<13}{line}", if i == 0 { "Conditions" } else { "" });
    }
    let c = &s.clipping;
    if c.samples > 0 {
        println!(
            "Clipping     on {:.1}% of beats ({} of {}, {} samples), peak {:.1} dBFS",
            c.beat_fraction * 100.0,
            c.beats,
            s.beats_found,
            c.samples,
            c.peak_dbfs
        );
    }
    if let Some(m) = &s.shape {
        let pct = |v: Option<f64>| v.map_or("-".to_string(), |v| format!("{:.0}%", v * 100.0));
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
                "{:<13}beat error from the unlock {a:+.2} ms in the first quarter, {b:+.2} ms in the last ({} window{} each)",
                "",
                m.beat_error_quarter_windows,
                if m.beat_error_quarter_windows == 1 { "" } else { "s" }
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
                "  {:>8.1} s  explains {:>4.0}% of variance  {}",
                c.period_s,
                c.power * 100.0,
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
        for value in ["NaN", "inf", "-inf", "-1000000", "-1000001"] {
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
        assert!(matches!(parsed.command, Command::Shape { lift: 52.0, .. }));
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
            "--lift=1e300",
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
