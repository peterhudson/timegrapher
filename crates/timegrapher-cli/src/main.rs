mod long;
mod report;
mod shape_cmd;

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
    version,
    about = "Analyse mechanical watch recordings beat by beat"
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
        /// Beat rate in beats per hour (guessed if omitted).
        #[arg(long)]
        bph: Option<u32>,
        /// Lift angle in degrees.
        #[arg(long, default_value_t = 52.0)]
        lift: f64,
        /// Comma-separated steady tones to notch out, Hz (e.g. 5000,6000,7000).
        #[arg(long, value_delimiter = ',')]
        notch: Vec<f64>,
        /// High-pass corner, Hz.
        #[arg(long, default_value_t = 1500.0)]
        highpass: f64,
        /// Escape wheel teeth, used to name periodic components.
        #[arg(long, default_value_t = 15)]
        escape_teeth: u32,
        /// Write one row per beat to this CSV file.
        #[arg(long)]
        beats: Option<PathBuf>,
        /// Write rate and amplitude per window to this CSV file.
        #[arg(long)]
        windows: Option<PathBuf>,
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
        /// Escape wheel teeth, used to name periodic components.
        #[arg(long, default_value_t = 15)]
        escape_teeth: u32,
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

fn main() -> ExitCode {
    let cli = Cli::parse();
    let res = match cli.command {
        Command::Analyze {
            file,
            bph,
            lift,
            notch,
            highpass,
            escape_teeth,
            beats,
            windows,
            json,
        } => {
            let mut cfg = AnalysisConfig {
                bph,
                escape_teeth,
                ..Default::default()
            };
            cfg.amplitude.lift_deg = lift;
            cfg.envelope.notch_hz = notch;
            cfg.envelope.highpass_hz = highpass;
            run_analyze(&file, &cfg, beats, windows, json)
        }
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
        Command::Long {
            files,
            clock,
            bph,
            lift,
            notch,
            highpass,
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
                clock.as_deref(),
                &cfg,
                escape_teeth,
                &wheel,
                out,
                json,
            )
        }
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
    beats: Option<PathBuf>,
    windows: Option<PathBuf>,
    json: bool,
) -> Result<(), String> {
    let audio = load(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let a = analyze(&audio, cfg);
    if let Some(p) = beats {
        write_beats(&p, &a).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    if let Some(p) = windows {
        write_windows(&p, &a).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&a.summary).map_err(|e| e.to_string())?
        );
    } else {
        print_summary(&a);
    }
    Ok(())
}

fn opt(v: Option<f64>, digits: usize) -> String {
    v.map_or("-".into(), |x| format!("{x:.digits$}"))
}

fn print_summary(a: &Analysis) {
    let s = &a.summary;
    println!("Duration     {:.1} s at {} Hz", s.duration_s, s.sample_rate);
    println!("Beat rate    {} bph, {} beats found", s.bph, s.beats_found);
    if let Some(f) = &s.overall {
        println!(
            "Rate         {:+.1} s/d (uncalibrated sound-card clock)",
            f.rate_s_per_day
        );
        println!("Beat error   {:.2} ms", f.beat_error_ms.abs());
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
        "Amplitude    {} deg (even beats {}, odd beats {}; lift angle {} deg)",
        opt(s.amplitude_deg, 0),
        opt(s.amplitude_even_deg, 0),
        opt(s.amplitude_odd_deg, 0),
        s.lift_deg
    );
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
    let mut w = BufWriter::new(File::create(p)?);
    writeln!(
        w,
        "kind,start_s,end_s,rate_s_per_day,beat_error_ms,amplitude_even_deg,amplitude_odd_deg"
    )?;
    for r in &a.rate_windows {
        writeln!(
            w,
            "rate,{:.2},{:.2},{:.2},{:.3},,",
            r.start_s, r.end_s, r.fit.rate_s_per_day, r.fit.beat_error_ms
        )?;
    }
    for m in &a.amplitude_windows {
        writeln!(
            w,
            "amplitude,{:.2},{:.2},,,{},{}",
            m.start_s,
            m.end_s,
            opt(m.even_deg, 1).replace('-', ""),
            opt(m.odd_deg, 1).replace('-', "")
        )?;
    }
    w.flush()
}
