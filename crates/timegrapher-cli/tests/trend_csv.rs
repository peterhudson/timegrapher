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
             amplitude_odd_deg,beat_error_unlock_ms,beat_error_ms,heard,clock_ppm"
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
    // A file no session file lists is on the sound card's clock.
    assert_eq!(last[9], "");
}

/// The trend file's rows, each split at its commas.
fn rows_of(csv: &std::path::Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(csv)
        .unwrap()
        .lines()
        .skip(1)
        .map(|l| l.split(',').map(String::from).collect())
        .collect()
}

#[test]
fn a_take_is_cut_and_corrected_as_the_app_reads_it() {
    let dir = Dir(std::env::temp_dir().join(format!("tg-trend-take-{}", std::process::id())));
    let (bare, take) = (dir.0.join("bare"), dir.0.join("take"));
    std::fs::create_dir_all(&bare).unwrap();
    std::fs::create_dir_all(&take).unwrap();
    run(&[
        "synth".as_ref(),
        bare.join("w.wav").as_os_str(),
        "--duration=60".as_ref(),
        "--rate=6".as_ref(),
    ]);
    std::fs::copy(bare.join("w.wav"), take.join("w.wav")).unwrap();
    // Dial Up to 30 s with a mask from 40 to 44 s, then Dial Down,
    // settling 5 s, on a card 20 ppm slow.
    std::fs::write(
        take.join("session.toml"),
        "bph = 28800\ncard_ppm = 20\n\n\
         [[recording]]\nfile = \"w.wav\"\nposition = \"DU\"\nstart_s = 0\nend_s = 30\n\
         settle_s = 0\nmasked = [[40.0, 44.0]]\n\n\
         [[recording]]\nfile = \"w.wav\"\nposition = \"DD\"\nstart_s = 30\nsettle_s = 5\n",
    )
    .unwrap();
    let trend = |folder: &std::path::Path| {
        let csv = folder.join("trend.csv");
        let flag = format!("--trend={}", csv.display());
        run(&[
            "analyze".as_ref(),
            folder.join("w.wav").as_os_str(),
            flag.as_ref(),
            "--trend-average=4".as_ref(),
        ]);
        rows_of(&csv)
    };
    let (alone, read) = (trend(&bare), trend(&take));
    assert_eq!(alone.len(), read.len());
    let rate = |r: &[String]| r[2].parse::<f64>().ok();
    // 20 ppm slow, in seconds per day.
    let correction = 20e-6 * 86_400.0;
    let mut cut = 0;
    for (a, b) in alone.iter().zip(&read) {
        let t: f64 = a[0].parse().unwrap();
        // No 4 s reading reaches across the move at 30 s, the settling's
        // end at 35 s or the mask from 40 to 44 s.
        // A reading ending on a cut takes no beats past it either.
        let across =
            (30.0..34.0).contains(&t) || (35.0..39.0).contains(&t) || (40.0..48.0).contains(&t);
        if t < 10.0 {
            continue;
        }
        assert!(rate(a).is_some(), "the file alone reads at {t}");
        if across {
            assert_eq!(rate(b), None, "nothing read across a cut at {t}");
            cut += 1;
        } else {
            let (a, b) = (rate(a).unwrap(), rate(b).unwrap());
            assert!((a - correction - b).abs() < 0.005, "{t}: {a} {b}");
        }
        assert_eq!(b[9], "20.000");
    }
    assert!(cut >= 20, "{cut}");
}
