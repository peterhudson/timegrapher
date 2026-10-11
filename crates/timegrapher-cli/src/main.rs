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
mod report;
mod series_cmd;
mod session_cmd;
mod session_report;
mod shape_cmd;
mod update_cmd;

use clap::{Parser, Subcommand};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use timegrapher_core::stream::StreamConfig;
use timegrapher_core::synth::{self, SynthConfig};
use timegrapher_core::{analyze, load, Analysis, AnalysisConfig};

#[derive(Parser)]
#[command(
    name = "timegrapher",
    version = env!("TIMEGRAPHER_LONG_VERSION"),
    about = "Analyse mechanical watch recordings beat by beat",
    after_help = "Every command with --json prints one JSON document with a `schema` field \
(e.g. timegrapher.analyze/1), the software version and the input it read. \
Start with `timegrapher doctor` to check the microphone. Exit codes: 0 success, \
1 error, 3 doctor found problems or a take was not fit to keep."
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
        #[arg(long)]
        bph: Option<u32>,
        /// Lift angle in degrees (default: the calibre's, or 52, graded as
        /// unknown).
        #[arg(long)]
        lift: Option<f64>,
        /// Comma-separated steady tones to notch out, Hz (e.g. 5000,6000,7000).
        #[arg(long, value_delimiter = ',')]
        notch: Vec<f64>,
        /// High-pass corner, Hz.
        #[arg(long, default_value_t = 1500.0)]
        highpass: f64,
        /// Escape wheel teeth, used to name periodic components (default:
        /// the calibre's, or 15).
        #[arg(long)]
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
        #[arg(long, allow_hyphen_values = true)]
        card_ppm: Option<f64>,
        /// Print the summary as JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Measure the three sounds of each side's beat.
    Shape {
        file: PathBuf,
        /// Beat rate in beats per hour (guessed if omitted).
        #[arg(long)]
        bph: Option<u32>,
        /// Lift angle in degrees.
        #[arg(long, default_value_t = 52.0)]
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
        /// Write the even and odd whole-recording templates to this CSV file.
        #[arg(long)]
        templates: Option<PathBuf>,
    },
    /// The sound of the tick and the tock (even and odd beats) over a stretch of
    /// a recording, with the unlock, drop and sound marks the engine
    /// measured on them: the place to look when amplitude or beat error
    /// seem wrong.
    Profile {
        file: PathBuf,
        /// Start of the stretch, seconds from the start of the recording.
        #[arg(long, default_value_t = 0.0)]
        at: f64,
        /// Length of the stretch, seconds.
        #[arg(long, default_value_t = 10.0)]
        span: f64,
        /// Beat rate in beats per hour (guessed if omitted).
        #[arg(long)]
        bph: Option<u32>,
        /// Lift angle in degrees.
        #[arg(long, default_value_t = 52.0)]
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
        #[arg(long, allow_hyphen_values = true)]
        card_ppm: Option<f64>,
        /// Beat rate in beats per hour (guessed if omitted).
        #[arg(long)]
        bph: Option<u32>,
        /// Lift angle in degrees.
        #[arg(long, default_value_t = 52.0)]
        lift: f64,
        /// Comma-separated steady tones to notch out, Hz.
        #[arg(long, value_delimiter = ',')]
        notch: Vec<f64>,
        /// High-pass corner, Hz.
        #[arg(long, default_value_t = 1500.0)]
        highpass: f64,
        /// The movement, to name periodic components after its wheels
        /// (e.g. "ETA 2824-2", "Rolex 3235"; an unknown name lists the
        /// known ones).
        #[arg(long)]
        calibre: Option<String>,
        /// Escape wheel teeth, used to name periodic components (default:
        /// the calibre's, or both 15 and 20 when no calibre is given).
        #[arg(long)]
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
    /// Is each series of readings steady? Tests the rate, amplitude and
    /// beat error of a take in time order for independence
    /// (autocorrelation, Allan deviation, CUSUM, change points), alongside
    /// the period search and the two-state finder, and gives one verdict
    /// per series: steady, periodic, two states, measurement, shifting
    /// mean, drifting or wandering.
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
        #[arg(long)]
        bph: Option<u32>,
        /// Lift angle in degrees.
        #[arg(long, default_value_t = 52.0)]
        lift: f64,
        /// Comma-separated steady tones to notch out, Hz.
        #[arg(long, value_delimiter = ',')]
        notch: Vec<f64>,
        /// High-pass corner, Hz.
        #[arg(long, default_value_t = 1500.0)]
        highpass: f64,
        /// The movement, to name periodic components after its wheels
        /// (e.g. "ETA 2824-2", "Rolex 3235"; an unknown name lists the
        /// known ones).
        #[arg(long)]
        calibre: Option<String>,
        /// Escape wheel teeth, used to name periodic components (default:
        /// the calibre's, or both 15 and 20 when no calibre is given).
        #[arg(long)]
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
    /// List the sound inputs, with the sample rates and formats each supports.
    /// The built-in table of calibres: beat rate, lift angle, escape wheel
    /// teeth and the turn of each train wheel, for `--calibre`.
    Calibres {
        /// Show only this calibre, with its wheels, notes and sources.
        name: Option<String>,
        /// Print the table as JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    Devices {
        /// Print the list as JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Check the microphone: record a few seconds and report level,
    /// clipping, automatic gain, noise and whether ticks are heard, with
    /// the exact settings to change. Changes nothing without --apply.
    /// Exits 0 when the input is fit to measure with, 3 when it is not.
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
        /// Make the proposed mixer changes (Linux), then check again.
        /// Ask the watch's owner before using this: it changes their
        /// computer's settings.
        #[arg(long)]
        apply: bool,
        /// Beat rate in beats per hour (guessed if omitted).
        #[arg(long)]
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
    /// as the desktop app saves it, checked at once and, when a watch is
    /// heard cleanly, added to the folder's session.toml for `session`.
    /// Listens only while it runs, for --seconds. Exits 3 when the take is
    /// not fit to keep (no watch heard, or clipped).
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
        /// they are not given (an unknown name lists the known ones).
        #[arg(long)]
        calibre: Option<String>,
        /// Beat rate in beats per hour (guessed if omitted).
        #[arg(long)]
        bph: Option<u32>,
        /// Lift angle in degrees (default: the calibre's, else 52).
        #[arg(long)]
        lift: Option<f64>,
        /// Hours since the watch was fully wound (0 for the fully wound
        /// half of a test, 24 for the run-down half).
        #[arg(long, default_value_t = 0.0)]
        wind_h: f64,
        /// Print the result as JSON instead of text.
        #[arg(long)]
        json: bool,
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
        #[arg(long)]
        bph: Option<u32>,
        /// Lift angle in degrees (overrides the session file; default 52).
        #[arg(long)]
        lift: Option<f64>,
        /// The movement (e.g. "ETA 2824-2", "Rolex 3235"; an unknown name
        /// lists the known ones): its beat rate, lift angle and wheels
        /// where --bph, --lift and the session file give none.
        #[arg(long)]
        calibre: Option<String>,
        /// Tolerance class: ladies, mens, cosc-small, cosc, metas (default mens).
        #[arg(long)]
        tolerance: Option<String>,
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
    /// tg by more than its margin, or a reading lost beats or a value.
    Regress {
        /// A folder of takes, each with a session.toml whose readings carry
        /// tg's numbers (e.g. a clone of the recordings repo).
        root: PathBuf,
        /// Baseline file (default: regress-baseline.json in ROOT).
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// Write the baseline from this run (accepting it) instead of failing.
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
        #[arg(long)]
        bph: Option<u32>,
        /// Lift angle in degrees, as set.
        #[arg(long)]
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
        out: PathBuf,
        #[arg(long, default_value_t = 60.0)]
        duration: f64,
        #[arg(long, default_value_t = 28800)]
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
        #[arg(allow_hyphen_values = true)]
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
            let train = long::Train {
                calibre: calibre.as_deref(),
                escape_teeth,
            };
            clock_cmd::correction(&file, device.as_deref(), card_ppm)
                .and_then(|c| run_analyze(&file, &cfg, &train, beats, windows, c, json))
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
            cfg.analysis.amplitude.lift_deg = lift;
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
            cfg.analysis.amplitude.lift_deg = lift;
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
        Command::Calibres { name, json } => calibres_cmd::run(name.as_deref(), json),
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
            match doctor::run(&o) {
                Ok(true) => Ok(()),
                Ok(false) => return ExitCode::from(3),
                Err(e) => Err(e),
            }
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
            };
            match record_cmd::run(&o) {
                Ok(true) => Ok(()),
                Ok(false) => return ExitCode::from(3),
                Err(e) => Err(e),
            }
        }
        Command::Session {
            paths,
            init,
            bph,
            lift,
            calibre,
            tolerance,
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
                tolerance,
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
        } => {
            let o = regress_cmd::Options {
                fetch,
                smoke,
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
        } => {
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
    train: &long::Train,
    beats: Option<PathBuf>,
    windows: Option<PathBuf>,
    clock: Option<(f64, String)>,
    json: bool,
) -> Result<(), String> {
    let audio = load(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut a = analyze(&audio, cfg);
    let calibre = train.calibre()?;
    // A calibre's own wheels name the periodic components.
    if calibre.is_some() {
        let wheels = calibres_cmd::wheels(train, a.summary.bph)?;
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
        None => a.summary.usb_clock.map(|c| {
            (
                c.ppm,
                format!(
                    "from {} USB lines in the recording, {:.1} ppm apart",
                    c.lines, c.spread_ppm
                ),
            )
        }),
    };
    if let Some(p) = beats {
        write_beats(&p, &a).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    if let Some(p) = windows {
        write_windows(&p, &a).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    if json {
        let settings = serde_json::json!({
            "calibre": calibre.map(|c| &c.calibre),
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
                "Calibre      {}: {} bph, lift {}°, as set unless a flag said otherwise",
                c.calibre, c.bph, cfg.amplitude.lift_deg
            );
        }
        print_summary(&a, clock.as_ref());
    }
    Ok(())
}

/// The clock correction applied to a rate read on the sound card.
pub fn clock_json(ppm: f64, from: &str, card_rate: Option<f64>) -> serde_json::Value {
    use timegrapher_core::clockstore::correct_rate;
    serde_json::json!({
        "ppm": ppm,
        "from": from,
        "rate_error_s_per_day": ppm * 1e-6 * 86400.0,
        "rate_s_per_day": card_rate.map(|r| correct_rate(r, ppm)),
    })
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

fn print_summary(a: &Analysis, clock: Option<&(f64, String)>) {
    let s = &a.summary;
    println!("Duration     {:.1} s at {} Hz", s.duration_s, s.sample_rate);
    println!("Beat rate    {} bph, {} beats found", s.bph, s.beats_found);
    if let Some(heard) = s.bph_heard {
        println!(
            "Warning      the beat rate was set to {} bph, but the recording sounds like {heard} bph: every reading below is measured against the wrong beat and graded poor. Check the calibre or --bph.",
            s.bph
        );
    }
    if !s.watch_heard {
        println!(
            "No watch heard: the beats found are no steadier than noise (jitter {} us), so no readings are shown. Check that the watch is on the pickup and running.",
            opt(s.jitter_us, 0)
        );
        return;
    }
    if let Some(c) = &s.confidence {
        let g = |r: &timegrapher_core::confidence::Reading, unit: &str, d: usize| {
            format!(
                "{:?}{}",
                r.grade,
                r.se.map_or(String::new(), |e| format!(" (± {e:.d$} {unit})"))
            )
            .to_lowercase()
        };
        let mut why = Vec::new();
        if s.doubts.clipped {
            why.push("clipping");
        }
        if s.doubts.beats_missing {
            why.push("beats missing");
        }
        if s.doubts.lift_unknown {
            why.push("lift angle not given");
        }
        if s.doubts.clock_unknown {
            why.push("sound card clock unknown");
        }
        println!(
            "Confidence   rate {}, amplitude {}, beat error {}{}",
            g(&c.rate, "s/d", 1),
            g(&c.amplitude, "deg", 1),
            g(&c.beat_error, "ms", 2),
            if why.is_empty() {
                String::new()
            } else {
                format!("; lowered for {}", why.join(", "))
            }
        );
    }
    if let Some(f) = &s.overall {
        match clock {
            Some((ppm, from)) => println!(
                "Rate         {:+.1} s/d, corrected for the sound card's clock ({:.1} ppm {}, {from}); {:+.1} s/d on the card's clock",
                timegrapher_core::clockstore::correct_rate(f.rate_s_per_day, *ppm),
                ppm.abs(),
                if *ppm >= 0.0 { "slow" } else { "fast" },
                f.rate_s_per_day
            ),
            None => println!(
                "Rate         {:+.1} s/d (uncalibrated sound-card clock)",
                f.rate_s_per_day
            ),
        }
        println!(
            "Beat error   {}",
            beat_error_text(s.beat_error_unlock_ms, f.beat_error_ms)
        );
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
        "Amplitude    {}{} deg (even beats {}, odd beats {}; lift angle {} deg)",
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
    let c = &s.clipping;
    if c.warns() {
        println!(
            "Clipping     in {:.0}% of beats ({} samples): amplitude and beat error may be off; turn the input down",
            c.beat_fraction * 100.0,
            c.samples
        );
    } else if c.samples > 0 {
        println!(
            "Clipping     {} samples, in {} of {} beats: too few to matter",
            c.samples, c.beats, s.beats_found
        );
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
    if !s.faults.is_empty() {
        println!("Possible faults, from the shape of the Tick and Tock (worth checking, not a diagnosis):");
        for f in &s.faults {
            let evidence: Vec<String> = f
                .evidence
                .iter()
                .map(|(k, v)| format!("{} {v:.2}", k.replace('_', " ")))
                .collect();
            println!(
                "  {:<13} {} ({}; {})",
                f.side.word(),
                f.label,
                evidence.join(", "),
                f.see
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
    let mut w = BufWriter::new(File::create(p)?);
    writeln!(
        w,
        "kind,start_s,end_s,rate_s_per_day,beat_error_ms,beat_error_unlock_ms,amplitude_even_deg,amplitude_odd_deg"
    )?;
    for r in &a.rate_windows {
        writeln!(
            w,
            "rate,{:.2},{:.2},{:.2},{:.3},,,",
            r.start_s, r.end_s, r.fit.rate_s_per_day, r.fit.beat_error_ms
        )?;
    }
    for m in &a.amplitude_windows {
        writeln!(
            w,
            "amplitude,{:.2},{:.2},,{},{},{},{}",
            m.start_s,
            m.end_s,
            m.beat_error_ms.map_or(String::new(), |v| format!("{v:.3}")),
            m.beat_error_unlock_ms
                .map_or(String::new(), |v| format!("{v:.3}")),
            opt(m.even_deg, 1).replace('-', ""),
            opt(m.odd_deg, 1).replace('-', "")
        )?;
    }
    w.flush()
}
