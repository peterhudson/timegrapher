//! `timegrapher profile`: the sound of the tick and of the tock (even and odd
//! beats; the JSON keeps `a` for the tick and `b` for the tock), with the
//! unlock, drop and sound marks the engine measured on them.

use std::fmt::Write as _;
use std::path::Path;
use timegrapher_core::analysis::{analyze, AnalysisConfig};
use timegrapher_core::load;
use timegrapher_core::profile::{self, Side, TickProfile};

pub struct Options<'a> {
    pub cfg: AnalysisConfig,
    /// Start of the stretch, seconds from the start of the recording.
    pub at_s: f64,
    pub span_s: f64,
    pub json: bool,
    pub svg: Option<&'a Path>,
}

pub fn run(file: &Path, o: &Options) -> Result<(), String> {
    let audio = load(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let fs = audio.sample_rate as f64;
    let dur = audio.samples.len() as f64 / fs;
    let a = analyze(&audio, &o.cfg);
    let bph = a.summary.bph;
    let beat = a
        .summary
        .overall
        .as_ref()
        .map_or(3600.0 / bph as f64, |f| f.period_s);
    let from = o.at_s.clamp(0.0, dur);
    let to = (from + o.span_s).min(dur);
    if to - from < 1.0 {
        return Err(format!("the stretch {from:.1}–{to:.1} s is too short"));
    }
    let env = timegrapher_core::dsp::envelope(&audio.samples, fs, &o.cfg.envelope);
    let profiles = profile::profiles(&env, fs, &a.beats, 2.0 * beat, from, to, &o.cfg.amplitude);
    if let Some(p) = o.svg {
        std::fs::write(p, svg(&profiles, file, from, to, o.cfg.amplitude.lift_deg))
            .map_err(|e| format!("{}: {e}", p.display()))?;
    }
    if o.json {
        let settings = serde_json::json!({
            "bph": bph,
            "lift_deg": o.cfg.amplitude.lift_deg,
            "notch_hz": o.cfg.envelope.notch_hz,
            "highpass_hz": o.cfg.envelope.highpass_hz,
            "from_s": from,
            "to_s": to,
        });
        let body = serde_json::json!({ "a": profiles[0], "b": profiles[1] });
        crate::output::print(
            "profile",
            crate::output::input(&[file.to_path_buf()], settings),
            &body,
        )?;
    } else {
        println!(
            "Stretch      {from:.1}–{to:.1} s at {bph} bph, lift angle {} deg; times in ms from the beat",
            o.cfg.amplitude.lift_deg
        );
        for p in profiles.iter().flatten() {
            print_side(p);
        }
        if profiles.iter().all(Option::is_none) {
            println!("No beats clean enough for a profile in this stretch.");
        }
    }
    Ok(())
}

fn ms(v: Option<f64>) -> String {
    v.map_or("-".into(), |x| format!("{x:+.2}"))
}

fn print_side(p: &TickProfile) {
    let name = match p.side {
        Side::A => "Tick",
        Side::B => "Tock",
    };
    println!(
        "{name:<12} {} beats; unlock {}, drop {}, peak {}; sounds {} / {} / {}; amplitude {}",
        p.beats,
        ms(p.unlock_ms),
        ms(p.drop_ms),
        ms(p.peak_ms),
        ms(p.sound1_ms),
        ms(p.sound2_ms),
        ms(p.sound3_ms),
        p.amplitude_deg
            .map_or("- (out of range)".into(), |a| format!("{a:.0} deg")),
    );
}

/// Two stacked plots (tick above tock) on a log level scale, with the median, the
/// 10–90% band and the marks.
fn svg(profiles: &[Option<TickProfile>; 2], file: &Path, from: f64, to: f64, lift: f64) -> String {
    let (w, h, ml, mr, mt, gap) = (900.0, 260.0, 60.0, 20.0, 40.0, 50.0);
    let total = mt + 2.0 * h + gap + 40.0;
    let mut s = String::new();
    let _ = write!(
        s,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{total}" font-family="sans-serif" font-size="12">
<rect width="100%" height="100%" fill="#fff"/>
<text x="{ml}" y="20" font-size="14">{} — {from:.1}–{to:.1} s, lift {lift}°: median sound (line), 10–90% of beats (band), engine marks</text>
"##,
        file.file_name().map_or("".into(), |n| n.to_string_lossy())
    );
    // A shared level scale so the two sides can be compared.
    let all = profiles.iter().flatten();
    let lo = all
        .clone()
        .map(|p| p.floor.max(1e-6))
        .fold(f32::INFINITY, f32::min)
        * 0.7;
    let hi = all
        .flat_map(|p| p.p90.iter().copied())
        .fold(lo * 2.0, f32::max)
        * 1.3;
    for (i, p) in profiles.iter().enumerate() {
        let y0 = mt + i as f64 * (h + gap);
        let label = if i == 0 {
            "Tick (even beats)"
        } else {
            "Tock (odd beats)"
        };
        let _ = writeln!(
            s,
            r##"<rect x="{ml}" y="{y0}" width="{}" height="{h}" fill="none" stroke="#999"/><text x="{ml}" y="{}">{label}</text>"##,
            w - ml - mr,
            y0 - 6.0
        );
        let Some(p) = p else {
            let _ = writeln!(
                s,
                r##"<text x="{}" y="{}">no profile</text>"##,
                ml + 10.0,
                y0 + 20.0
            );
            continue;
        };
        let n = p.median.len();
        let t1 = p.t0_ms + (n - 1) as f64 * p.step_ms;
        let xs = |t: f64| ml + (t - p.t0_ms) / (t1 - p.t0_ms) * (w - ml - mr);
        let ys = |v: f32| {
            let v = v.max(lo) as f64;
            y0 + h - (v.ln() - (lo as f64).ln()) / ((hi as f64).ln() - (lo as f64).ln()) * h
        };
        let t = |k: usize| p.t0_ms + k as f64 * p.step_ms;
        let mut band = String::new();
        for k in 0..n {
            let _ = write!(band, "{:.1},{:.1} ", xs(t(k)), ys(p.p90[k]));
        }
        for k in (0..n).rev() {
            let _ = write!(band, "{:.1},{:.1} ", xs(t(k)), ys(p.p10[k]));
        }
        let _ = writeln!(
            s,
            r##"<polygon points="{band}" fill="#cfe0f5" stroke="none"/>"##
        );
        let line: String = (0..n)
            .map(|k| format!("{:.1},{:.1} ", xs(t(k)), ys(p.median[k])))
            .collect();
        let _ = writeln!(
            s,
            r##"<polyline points="{line}" fill="none" stroke="#123" stroke-width="1.2"/>"##
        );
        // Time grid every 2 ms.
        let mut g = (p.t0_ms / 2.0).ceil() * 2.0;
        while g <= t1 {
            let x = xs(g);
            let _ = writeln!(
                s,
                r##"<line x1="{x:.1}" y1="{}" x2="{x:.1}" y2="{}" stroke="#eee"/><text x="{x:.1}" y="{}" text-anchor="middle" fill="#666">{g:.0}</text>"##,
                y0 + h,
                y0 + h + 4.0,
                y0 + h + 16.0
            );
            g += 2.0;
        }
        let marks = [
            (p.unlock_ms, "#d33", "unlock"),
            (p.drop_ms, "#d33", "drop"),
            (p.sound1_ms, "#2a2", "1"),
            (p.sound2_ms, "#2a2", "2"),
            (p.sound3_ms, "#2a2", "3"),
        ];
        for (k, (m, c, name)) in marks.iter().enumerate() {
            if let Some(m) = m {
                let x = xs(*m);
                let ty = y0 + 14.0 + (k % 3) as f64 * 13.0;
                let _ = writeln!(
                    s,
                    r##"<line x1="{x:.1}" y1="{y0}" x2="{x:.1}" y2="{}" stroke="{c}" stroke-dasharray="4 3"/><text x="{:.1}" y="{ty:.1}" fill="{c}">{name}</text>"##,
                    y0 + h,
                    x + 3.0
                );
            }
        }
        let amp = p
            .amplitude_deg
            .map_or("out of range".into(), |a| format!("{a:.0}°"));
        let _ = writeln!(
            s,
            r##"<text x="{}" y="{}" text-anchor="end">{} beats, amplitude {amp}</text>"##,
            w - mr - 4.0,
            y0 + 16.0,
            p.beats
        );
    }
    let _ = writeln!(
        s,
        r##"<text x="{}" y="{}" text-anchor="middle" fill="#666">ms from the beat (drop); level on a log scale</text></svg>"##,
        w / 2.0,
        total - 6.0
    );
    s
}
