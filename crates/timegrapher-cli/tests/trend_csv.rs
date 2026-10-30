//! `analyze --trend` writes core's readings over time: the app's averaged
//! readings at core's times, one row each.
use std::{path::PathBuf, process::Command};

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&std::ffi::OsStr]) {
    let out = Command::new(env!("CARGO_BIN_EXE_timegrapher"))
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
}

#[test]
fn the_trend_file_is_the_app_reading_over_time() {
    let dir = Dir(std::env::temp_dir().join(format!("tg-trend-{}", std::process::id())));
    std::fs::create_dir_all(&dir.0).unwrap();
    let wav = dir.0.join("w.wav");
    let csv = dir.0.join("trend.csv");
    run(&[
        "synth".as_ref(),
        wav.as_os_str(),
        "--duration=40".as_ref(),
        "--rate=6".as_ref(),
    ]);
    let trend = format!("--trend={}", csv.display());
    run(&[
        "analyze".as_ref(),
        wav.as_os_str(),
        trend.as_ref(),
        "--trend-average=4".as_ref(),
    ]);
    let text = std::fs::read_to_string(&csv).unwrap();
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some(
            "t_s,average_s,rate_s_per_day,amplitude_deg,amplitude_even_deg,\
             amplitude_odd_deg,beat_error_unlock_ms,beat_error_ms,heard"
        )
    );
    let rows: Vec<Vec<&str>> = lines.map(|l| l.split(',').collect()).collect();
    // Core's times: every half second for a recording this short.
    let times = timegrapher_core::live::trend_times(40.0);
    assert_eq!(rows.len(), times.len());
    for (row, t) in rows.iter().zip(&times) {
        assert!((row[0].parse::<f64>().unwrap() - t).abs() < 0.01, "{row:?}");
        assert_eq!(row[1], "4");
    }
    let last = rows.last().unwrap();
    assert_eq!(last[8], "true");
    let rate: f64 = last[2].parse().unwrap();
    assert!((rate - 6.0).abs() < 0.5, "{rate}");
    // A reading over too few beats has no figures.
    assert_eq!(rows[0][2], "");
}
