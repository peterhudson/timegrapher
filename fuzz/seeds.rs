//! Writes a few well-formed inputs for each target to `corpus/<target>/`,
//! for libFuzzer to start from: `cargo run --release --bin seeds` in this
//! folder, before the first `cargo fuzz run`.
use flate2::read::GzDecoder;
use std::io::Read;
use std::path::Path;
use timegrapher_core::analysis_take;
use timegrapher_core::session::Manifest;
use timegrapher_core::stream::StreamConfig;
use timegrapher_core::synth::{generate, SynthConfig};

#[path = "fuzz_targets/common.rs"]
mod common;

fn seed(target: &str, name: &str, data: &[u8]) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("corpus")
        .join(target);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), data).unwrap();
}

fn main() {
    let manifests = [
        "bph = 28800\nlift = 52\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\nclock = \"clock.csv\"\n",
        "audio = false\nbph = 21600\nlift_known = false\ncard_ppm = -3.5\nsettle_s = 20\nmeasure_s = 60\n\
         wheels = { \"third wheel\" = 450.0 }\n\n\
         [[recording]]\nfile = \"du.flac\"\nposition = \"DU\"\nwind_h = 0\nstart_s = 5\nend_s = 50\n\
         masked = [[10.0, 12.5], [30.0, 31.0]]\n\n\
         [[recording]]\nfile = \"cu.flac\"\nposition = \"CU\"\nsettle_s = 0\n\n\
         [[recorded]]\nposition = \"DU\"\nstart_s = 0\nend_s = 50\n",
    ];
    for (i, text) in manifests.iter().enumerate() {
        let m: Manifest = toml::from_str(text).unwrap();
        m.validate_analysis_inputs().unwrap();
        seed("session_toml", &format!("seed-{i}"), text.as_bytes());
    }

    // A minute and a half with a clock log of its own (one line a second,
    // the way the recorder writes one) and the same log without a header.
    let mut log = String::from("audio_frames,unix_ns\n");
    let mut bare = String::new();
    for s in 0..90u64 {
        let ns = 1_790_000_000_000_000_000 + s * 1_000_000_013;
        log.push_str(&format!("{},{ns}\n", s * 48_000));
        bare.push_str(&format!("{}.0,{}\n", s, ns as f64 / 1e9));
    }
    // Each seed is checked to read, so that the fuzzing starts past the
    // readers' first checks.
    for text in [&log, &bare] {
        let pairs = timegrapher_core::clock::parse_log(text, 48_000, 2).unwrap();
        timegrapher_core::clock::ClockFit::new(&pairs).unwrap();
    }
    seed(
        "clock_log",
        "seed-header",
        &[[1u8].as_slice(), log.as_bytes()].concat(),
    );
    seed(
        "clock_log",
        "seed-bare",
        &[[1u8].as_slice(), bare.as_bytes()].concat(),
    );

    let zip = common::scratch("seeds").join("bundle.zip");
    timegrapher_core::archive::write_zip(
        &zip,
        &[
            (
                "bundle.json".into(),
                b"{\"format\":\"timegrapher.bundle/1\"}".to_vec(),
            ),
            ("report/session.json".into(), b"{\"readings\":[]}".to_vec()),
            (
                "report/session.toml".into(),
                manifests[0].as_bytes().to_vec(),
            ),
        ],
    )
    .unwrap();
    timegrapher_core::archive::read_zip(&zip).unwrap();
    seed("report_zip", "seed-0", &std::fs::read(&zip).unwrap());

    // A take kept as its analysis: a short synthesized watch, measured as
    // the app measures one it keeps without its sound.
    let wav = common::scratch("seeds").join("watch.wav");
    let audio = generate(
        &SynthConfig {
            duration_s: 30.0,
            rate_s_per_day: 6.0,
            beat_error_ms: 0.4,
            ..Default::default()
        },
        |_| 275.0,
        |_| 0.0,
    );
    timegrapher_core::audio::write_wav(&wav, &audio).unwrap();
    let m: Manifest = toml::from_str(manifests[0]).unwrap();
    let mut cfg = StreamConfig::default();
    cfg.analysis.bph = Some(28800);
    cfg.analysis.amplitude.lift_deg = 52.0;
    let stored = analysis_take::measure(
        &[wav.as_path()],
        &cfg,
        &analysis_take::shape_stretches(&m, 30.0),
        serde_json::json!({"name": "timegrapher", "version": "fuzz", "git_hash": "seed"}),
        |_| {},
    )
    .unwrap();
    let files = analysis_take::files(&stored).unwrap();
    let get = |name: &str| files.iter().find(|f| f.0 == name).unwrap().1.clone();
    let gunzip = |b: Vec<u8>| {
        let mut out = Vec::new();
        GzDecoder::new(&b[..]).read_to_end(&mut out).unwrap();
        out
    };
    let parts = [
        get(analysis_take::HEADER),
        gunzip(get(analysis_take::BEATS)),
        gunzip(get(analysis_take::WINDOWS)),
        gunzip(get(analysis_take::PROFILES)),
    ];
    let take = common::scratch("seeds").join("take");
    analysis_take::write(&take, &stored).unwrap();
    analysis_take::read(&take).unwrap();
    seed("analysis_take", "seed-0", &parts.join(common::SPLIT));
    let raw = [
        get(analysis_take::HEADER),
        get(analysis_take::BEATS),
        get(analysis_take::WINDOWS),
        get(analysis_take::PROFILES),
    ];
    seed(
        "analysis_take",
        "seed-raw",
        &[b"!".as_slice(), &raw.join(common::SPLIT)].concat(),
    );
}
