//! `timegrapher shape`: the three sounds of each side's beat.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use timegrapher_core::beats;
use timegrapher_core::dsp::{envelope, EnvelopeConfig};
use timegrapher_core::load;
use timegrapher_core::shape::{self, Shape, ShapeConfig, ShapeReport, SideReport};
use timegrapher_core::timing;

pub struct Options<'a> {
    pub bph: Option<u32>,
    pub envelope: EnvelopeConfig,
    pub lift_deg: f64,
    pub json: bool,
    pub templates: Option<&'a Path>,
}

pub fn run(file: &Path, o: &Options) -> Result<(), String> {
    let audio = load(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let fs = audio.sample_rate as f64;
    let env = envelope(&audio.samples, fs, &o.envelope);
    let bph = o.bph.unwrap_or_else(|| beats::guess_bph(&env, fs));
    let (found, _) = beats::detect(&env, fs, bph);
    let beat_s = timing::fit(&found, bph).map_or(3600.0 / bph as f64, |f| f.period_s);
    let r = shape::analyze(&env, fs, &found, beat_s, &ShapeConfig::default());
    if let Some(p) = o.templates {
        write_templates(p, &r, fs).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    if o.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&r).map_err(|e| e.to_string())?
        );
    } else {
        print_report(&r, bph, beat_s, o.lift_deg);
    }
    Ok(())
}

fn write_templates(p: &Path, r: &ShapeReport, fs: f64) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(p)?);
    writeln!(w, "time_ms,even,odd")?;
    for (k, (e, o)) in r.template_even.iter().zip(&r.template_odd).enumerate() {
        let t = r.template_start_ms + k as f64 / fs * 1000.0;
        writeln!(w, "{t:.4},{e:.6e},{o:.6e}")?;
    }
    w.flush()
}

fn opt(v: Option<f64>, digits: usize) -> String {
    v.filter(|x| x.is_finite())
        .map_or("-".into(), |x| format!("{x:.digits$}"))
}

fn print_report(r: &ShapeReport, bph: u32, beat_s: f64, lift_deg: f64) {
    println!(
        "Beat rate    {bph} bph; {} windows of 2 s; times from the unlock edge",
        r.windows.len()
    );
    let cols: [(&str, Option<&Shape>); 4] = [
        ("even", Some(&r.even.windows)),
        ("odd", Some(&r.odd.windows)),
        ("even all", r.even.whole.as_ref()),
        ("odd all", r.odd.whole.as_ref()),
    ];
    print!("{:<24}", "");
    for (name, _) in &cols {
        print!("{name:>10}");
    }
    println!();
    type Row<'a> = (&'a str, &'a dyn Fn(&Shape) -> Option<f64>, usize);
    let amp = |s: &Shape| {
        let osc = 2.0 * beat_s;
        Some(timegrapher_core::amplitude::amplitude_deg(
            s.unlock_to_drop_ms / 1000.0,
            osc,
            lift_deg,
        ))
    };
    let rows: [Row; 22] = [
        ("unlock to drop, ms", &|s| Some(s.unlock_to_drop_ms), 2),
        ("  amplitude, deg", &amp, 0),
        ("sound 1 rise, ms", &|s| s.t1_ms, 2),
        ("sound 2 rise, ms", &|s| s.t2_ms, 2),
        ("sound 3 rise, ms", &|s| Some(s.t3_ms), 2),
        ("sound 1 peak, ms", &|s| s.peak1_ms, 2),
        ("sound 2 peak, ms", &|s| s.peak2_ms, 2),
        ("sound 3 peak, ms", &|s| Some(s.peak3_ms), 2),
        ("interval 1-2, ms", &|s| s.i12_ms, 2),
        ("interval 2-3, ms", &|s| s.i23_ms, 2),
        ("interval 1-3, ms", &|s| s.i13_ms, 2),
        ("level 1:3", &|s| s.ratio13, 3),
        ("level 2:3", &|s| s.ratio23, 3),
        ("valley 1-2", &|s| s.valley12, 3),
        ("valley 2-3", &|s| s.valley23, 3),
        ("fill 1-2", &|s| s.fill12, 3),
        ("fill 2-3", &|s| s.fill23, 3),
        ("tail 3-15 ms after 3", &|s| Some(s.tail_ratio), 3),
        ("noise between beats", &|s| s.noise_ratio, 2),
        ("rises from 1 to 3", &|s| Some(s.rises as f64), 0),
        (
            "extra events before 1",
            &|s| Some(s.extra_pre.len() as f64),
            0,
        ),
        (
            "extra events after 3",
            &|s| Some(s.extra_post.len() as f64),
            0,
        ),
    ];
    for (name, f, digits) in rows {
        print!("{name:<24}");
        for (k, (_, s)) in cols.iter().enumerate() {
            // Extra events are counted on the whole-recording templates only.
            let v = if k < 2 && name.starts_with("extra") {
                None
            } else {
                s.and_then(f)
            };
            print!("{:>10}", opt(v, digits));
        }
        println!();
    }
    for (name, s) in [("even", &r.even), ("odd", &r.odd)] {
        print_side(name, s);
    }
}

fn print_side(name: &str, s: &SideReport) {
    println!(
        "{name:<5} sound 1 found in {}/{} windows, sound 2 in {}, extra events before 1 in {}, after 3 in {}",
        s.windows_with_1,
        s.windows_measured,
        s.windows_with_2,
        s.windows_with_extra_pre,
        s.windows_with_extra_post
    );
    if let Some(p) = &s.spread {
        println!(
            "      single beats: interval 1-3 {:.2} ms, spread {:.0} us (robust SD, {} beats)",
            p.i13_ms, p.i13_sd_us, p.beats
        );
    }
    if let Some(w) = &s.whole {
        for (when, list) in [("before 1", &w.extra_pre), ("after 3", &w.extra_post)] {
            for e in list {
                println!(
                    "      extra event {when} at {:+.2} ms, level {:.3} of the drop",
                    e.t_ms, e.level
                );
            }
        }
    }
}
