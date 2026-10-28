//! `synth --scenario`: a synthetic take with a known hidden variation,
//! written as a take folder the app, `series` and `long` open like a
//! recording.

use std::path::Path;
use timegrapher_core::excerpt::{self, Pcm};
use timegrapher_core::scenarios::{self, Scenario};

/// Minutes of sound in each file of a take.
const FILE_MIN: f64 = 10.0;

/// The scenarios, one per line, for `--scenario list`.
pub fn list() -> String {
    scenarios::all()
        .iter()
        .map(|s| format!("{:<17} {}\n", s.name, s.summary))
        .collect()
}

/// Write scenario `name` into the folder `out`: FLAC files of ten
/// minutes, `session.json` and a README with what is hidden.
pub fn write(out: &Path, name: &str, seed: u64) -> Result<(), String> {
    let s = scenarios::find(name).ok_or_else(|| {
        format!(
            "no scenario '{name}'; one of: {}",
            scenarios::all()
                .iter()
                .map(|s| s.name)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let cfg = scenarios::config(&s, seed);
    let mut files = Vec::new();
    scenarios::render(&s, seed, FILE_MIN * 60.0, |block| {
        let file = format!("take_part01-{:03}.flac", files.len() + 1);
        let pcm = Pcm {
            sample_rate: cfg.sample_rate,
            channels: 1,
            bits_per_sample: 16,
            samples: block
                .iter()
                .map(|&v| (v.clamp(-1.0, 1.0) * 32767.0).round() as i32)
                .collect(),
        };
        excerpt::write_flac(&out.join(&file), &pcm).map_err(|e| format!("{file}: {e}"))?;
        files.push(file);
        Ok(())
    })?;
    let json = serde_json::json!({
        "watch": format!("Synthetic: {} ({} layout)", s.name, scenarios::CALIBRE),
        "position": "DU",
        "lift_deg": scenarios::LIFT_DEG,
        "bph": scenarios::BPH,
        "sample_rate": cfg.sample_rate,
        "bits": 16,
        "software": format!("timegrapher {} synth --scenario {} --seed {seed}", env!("CARGO_PKG_VERSION"), s.name),
        "duration_s": s.duration_s,
        "segments": files,
        "notes": [s.summary, format!("Expected: {}", s.expect)],
        "result": null,
        "synthetic": { "scenario": s.name, "seed": seed, "summary": s.summary, "expect": s.expect },
    });
    let text = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())? + "\n";
    std::fs::write(out.join(timegrapher_core::take::RECORD), text).map_err(|e| e.to_string())?;
    std::fs::write(out.join("README.md"), readme(&s, seed)).map_err(|e| e.to_string())
}

fn readme(s: &Scenario, seed: u64) -> String {
    format!(
        "# Synthetic take: {name}\n\n\
         {summary}\n\n\
         **Expected:** {expect}\n\n\
         The watch is synthetic: {bph} bph laid out as an {calibre} (a 20-tooth escape wheel turning in 5 s, the fourth wheel in 60 s), \
         lift angle {lift}°, dial up. Choose the {calibre} as the calibre so the views name its wheels. \
         Apart from what is hidden, the rate is +5 s/d, the amplitude 270° and the beat error 0.4 ms, with a beat-to-beat spread of 0.25 ms in timing and 3° in amplitude.\n\n\
         Made with `timegrapher synth --scenario {name} --seed {seed} FOLDER`; the same command writes the same sound.\n",
        name = s.name,
        summary = s.summary,
        expect = s.expect,
        bph = scenarios::BPH,
        calibre = scenarios::CALIBRE,
        lift = scenarios::LIFT_DEG,
    )
}
