use super::*;

/// A take's listed beat error keeps the Bench card's two decimals, so a
/// good watch's 0.04 ms is never "0.0" and 0.06 never "0.1" (#1267);
/// rate and amplitude at theirs, a rate that rounds to zero unsigned.
#[test]
fn a_takes_figures_keep_the_cards_decimals() {
    assert_eq!(
        figure_texts(Some(-0.04), Some(271.6), Some(-0.04)),
        [Some("0.0".into()), Some("272".into()), Some("0.04".into())]
    );
    assert_eq!(
        figure_texts(Some(2.0), None, Some(0.06)),
        [Some("+2.0".into()), None, Some("0.06".into())]
    );
}

/// A take's reading is kept until what it reads changes: a report
/// written into the take's folder keeps it; a changed session file,
/// kept analysis or sound in a folder of its own doesn't.
#[test]
fn a_takes_reading_stamp_is_what_the_reading_reads() {
    let dir = std::env::temp_dir().join(format!("tg-reading-stamp-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("parts")).unwrap();
    fs::write(dir.join("session.toml"), "bph = 28800\n").unwrap();
    fs::write(dir.join("parts").join("audio-001.wav"), [0u8; 64]).unwrap();
    let first = reading_stamp(&dir);
    let report = dir.join(crate::take::REPORT_DIR);
    fs::create_dir_all(&report).unwrap();
    fs::write(report.join("summary.json"), "{}").unwrap();
    assert_eq!(reading_stamp(&dir), first);
    assert_ne!(stamp(&dir), first, "the take's own stamp counts the report");
    fs::write(dir.join("parts").join("audio-001.wav"), [0u8; 96]).unwrap();
    let sound = reading_stamp(&dir);
    assert_ne!(sound, first, "sound in a folder of its own");
    fs::create_dir_all(dir.join(crate::analysis_take::DIR)).unwrap();
    fs::write(
        dir.join(crate::analysis_take::DIR).join("beats.bin"),
        [1u8; 8],
    )
    .unwrap();
    let analysis = reading_stamp(&dir);
    assert_ne!(analysis, sound);
    fs::write(dir.join("session.toml"), "bph = 21600\nlift = 52\n").unwrap();
    assert_ne!(reading_stamp(&dir), analysis);
    let _ = fs::remove_dir_all(&dir);
}

/// A take whose unlock is never found reads its beat error from the
/// drop, as `session` reads it, and so does the reading the app kept
/// for it at its end.
#[test]
fn a_take_whose_unlock_is_never_found_reads_the_drops_beat_error() {
    let dir = std::env::temp_dir().join(format!("tg-unlock-unheard-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    crate::audio::write_wav(
        &dir.join("audio-001.wav"),
        &crate::synth::unlock_unheard(60.0, 0.8),
    )
    .unwrap();
    fs::write(
        dir.join("session.toml"),
        "bph = 28800\n\n[[recording]]\nfile = '.'\nposition = 'DU'\n",
    )
    .unwrap();
    let (r, _) = measure_take(&dir).unwrap();
    let be = r.beat_error_ms.expect("the drop's beat error");
    assert!((be - 0.8).abs() < 0.05, "{be}");
    fs::write(
        dir.join("session.json"),
        r#"{"result":{"rate_s_per_day":3.5,"amplitude_deg":null,"beat_error_ms":-0.4,"beat_error_unlock_ms":null}}"#,
    )
    .unwrap();
    let a = read_take(&dir, 0).unwrap().at_end.unwrap();
    assert_eq!(a.beat_error_ms, Some(-0.4));
    let _ = fs::remove_dir_all(&dir);
}

/// A write that another writer's file beside it can't reach: one
/// still holding the shared `.tmp` name open (as each save once did)
/// writes into that file, never into the one just put in place, and
/// a write leaves no file of its own behind (#872).
#[test]
fn an_atomic_write_has_a_file_of_its_own() {
    let dir = std::env::temp_dir().join(format!("tg-write-atomic-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let p = dir.join("clocks.json");
    let mut other = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(p.with_extension("tmp"))
        .unwrap();
    write_atomic(&p, b"{\"whole\": true}\n").unwrap();
    std::io::Write::write_all(&mut other, b"{\"half").unwrap();
    drop(other);
    let kept = fs::read(&p).unwrap();
    let left: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(kept, b"{\"whole\": true}\n");
    assert!(
        left.iter().all(|n| n == "clocks.json" || n == "clocks.tmp"),
        "{left:?}"
    );
}

/// A kept reading holds the very correction it was read with: one
/// that changes while the take is read (here the store gives another
/// from the one the reading used) is never kept beside it, so the next
/// look sees the change and reads the take again (audit F1 review).
#[test]
fn a_kept_reading_holds_the_correction_it_was_read_with() {
    let dir = std::env::temp_dir().join(format!("tg-reading-used-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    crate::audio::write_wav(
        &dir.join("audio-001.wav"),
        &crate::synth::generate(
            &crate::synth::SynthConfig {
                duration_s: 40.0,
                rate_s_per_day: 5.0,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        ),
    )
    .unwrap();
    fs::write(
        dir.join("session.toml"),
        "bph = 28800\n\n[[recording]]\nfile = '.'\nposition = 'DU'\n",
    )
    .unwrap();
    let used = (50.0, "used by the reading".to_string());
    let (r, kept) = measure_take_with(&dir, |_, _| Ok(Some(used.clone()))).unwrap();
    let now = outside(&dir, kept.first.as_deref()).unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert!(r.calibrated, "{r:?}");
    assert!(kept.first.is_some(), "the correction was asked for");
    assert_eq!(kept.correction, Some(used));
    assert_ne!(now, kept, "a different correction now: read it again");
}

/// A take's facts give the readings it ended on, but not for a test:
/// a Positions Test's or a Power Reserve Test's last window doesn't
/// stand for the test.
#[test]
fn a_tests_facts_leave_out_its_last_window() {
    let t = Take {
        name: "t".into(),
        bytes: 0,
        audio: true,
        incomplete: false,
        started_utc: None,
        named_start: None,
        duration_s: Some(120.0),
        positions: vec!["DU".into()],
        at_end: Some(AtEnd {
            rate_s_per_day: Some(3.5),
            amplitude_deg: Some(281.0),
            beat_error_ms: Some(-0.4),
            rate_corrected_s_per_day: None,
        }),
        power_reserve: None,
    };
    assert_eq!(
        t.facts(false),
        ["Dial Up", "2:00", "+3.5 s/d", "281°", "0.40 ms"]
    );
    assert_eq!(t.facts(true), ["Positions Test", "Dial Up", "2:00"]);
    let r = Take {
        power_reserve: Some(crate::power_reserve::Summary {
            run_s: 5400.0,
            ending: crate::power_reserve::Ending::Stopped,
            position: "DU".into(),
        }),
        ..t
    };
    assert_eq!(r.facts(false), ["Power Reserve 1 h 30 min", "Dial Up"]);
}

#[test]
fn a_recording_without_a_session_file_is_added_as_a_new_take() {
    let root = scratch("add-recording");
    let mut lib = Library::create(&root).unwrap();
    lib.add(&Watch::new("Pepsi")).unwrap();
    let id = lib.find("Pepsi").unwrap().watch.id.clone();
    let outside = scratch("add-recording-outside");
    fs::create_dir_all(&outside).unwrap();
    let file = outside.join("pepsi_DU.flac");
    fs::write(&file, vec![0u8; 100]).unwrap();
    let written = [
        (
            "session.toml",
            "bph = 28800\n\n[[recording]]\nfile = \".\"\n".to_string(),
        ),
        (
            "session.json",
            "{\"started_utc\": \"2026-10-22T17:00:00Z\"}".to_string(),
        ),
    ];
    let to = lib
        .add_recording(&file, &id, "2026-10-22 10.00 Dial Up", &written)
        .unwrap();
    assert_eq!(to.file_name().unwrap(), "2026-10-22 10.00 Dial Up");
    assert!(to.join("pepsi_DU.flac").is_file() && file.is_file());
    let takes = &lib.find("Pepsi").unwrap().takes;
    assert_eq!(takes.len(), 1);
    assert_eq!(
        takes[0].started_utc.as_deref(),
        Some("2026-10-22T17:00:00Z")
    );
    assert!(!takes[0].incomplete);
    // A folder is copied whole, under a name of its own.
    let again = lib.add_recording(&outside, &id, "2026-10-22 10.00 Dial Up", &written);
    assert!(again.unwrap().join("pepsi_DU.flac").is_file());
    assert_eq!(lib.find("Pepsi").unwrap().takes.len(), 2);
    assert!(lib
        .add_recording(&to, &id, "x", &written)
        .unwrap_err()
        .contains("already in"));
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&outside);
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tg-library-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    d
}

/// A standard take folder: a manifest naming `files`, the ones in
/// `present` written, and a session.json.
fn take(dir: &Path, files: &[(&str, &str)], present: usize, started: &str) {
    fs::create_dir_all(dir).unwrap();
    let mut toml = String::from("watch = \"YM42\"\nbph = 28800\n");
    for (f, p) in files {
        toml += &format!("\n[[recording]]\nfile = \"{f}\"\nposition = \"{p}\"\n");
    }
    fs::write(dir.join("session.toml"), toml).unwrap();
    for (f, _) in &files[..present] {
        fs::write(dir.join(f), vec![0u8; 1000]).unwrap();
    }
    fs::write(
        dir.join("session.json"),
        format!(
            r#"{{"started_utc":"{started}","duration_s":120.0,
                "result":{{"rate_s_per_day":3.5,"amplitude_deg":281.0,"beat_error_unlock_ms":-0.4}}}}"#
        ),
    )
    .unwrap();
}

/// Only a watch with nothing in it is taken back (#1275).
#[test]
fn only_an_empty_watch_is_taken_back() {
    let base = scratch("remove-empty");
    let root = base.join("Timegrapher Data");
    let mut lib = Library::create(&root).unwrap();
    lib.add(&Watch::new("Empty")).unwrap();
    lib.add(&Watch::new("Kept")).unwrap();
    let id = |lib: &Library, n: &str| lib.find(n).unwrap().watch.id.clone();
    let src = base.join("outside");
    take(&src, &[("a.wav", "DU")], 1, "2026-10-22T14:30:00Z");
    let kept = id(&lib, "Kept");
    lib.add_take(&src, &kept, true).unwrap();
    let err = lib.remove_empty(&kept).unwrap_err();
    assert!(err.contains("has takes or photos"), "{err}");
    let empty = id(&lib, "Empty");
    lib.remove_empty(&empty).unwrap();
    assert!(lib.find("Empty").is_none() && lib.find("Kept").is_some());
    assert!(lib.remove_empty(&empty).is_err(), "gone already");
    // An unreadable takes folder (a file in its place) is not empty.
    lib.add(&Watch::new("Unread")).unwrap();
    let (unread, dir) = (id(&lib, "Unread"), lib.find("Unread").unwrap().dir.clone());
    fs::remove_dir_all(dir.join(TAKES)).unwrap();
    fs::write(dir.join(TAKES), "not a folder").unwrap();
    let err = lib.remove_empty(&unread).unwrap_err();
    assert!(err.contains("has takes or photos"), "{err}");
    assert!(dir.join(TAKES).is_file(), "the folder is kept");
    fs::remove_dir_all(base).unwrap();
}

/// A take folder's name gives its start when it begins as the app and
/// `timegrapher record` name a take, on a real minute (#1272).
#[test]
fn a_takes_folder_name_gives_its_start() {
    assert_eq!(
        named_start("2026-11-06 18.30 Dial Up").as_deref(),
        Some("2026-11-06T18:30")
    );
    assert_eq!(
        named_start("2028-02-29 00.00").as_deref(),
        Some("2028-02-29T00:00")
    );
    // Every 400th year is a leap year, though a century (#1300).
    assert_eq!(
        named_start("2000-02-29 10.00").as_deref(),
        Some("2000-02-29T10:00")
    );
    for name in [
        "ym42 copied",
        "2026-11-0",
        "2026-13-06 18.30",
        "2026-02-29 18.30",
        "2100-02-29 10.00",
        "2026-11-00 10.00",
        "2026-04-31 18.30",
        "2026-11-06 24.00",
        "2026-11-06 18.60",
        "2026-11-06 18:30",
        "2026/11/06 18.30",
        "2026-11-06 1a.30",
    ] {
        assert_eq!(named_start(name), None, "{name}");
    }
}

/// A take copied in by hand, with no `session.json`, still has a
/// length: its sound's, from the files' headers, over every file the
/// manifest lists (#1272). When it started stays unknown, never
/// guessed; a `session.json` that says how long wins.
#[test]
fn a_take_copied_in_without_its_record_is_as_long_as_its_sound() {
    let dir = scratch("no-record");
    fs::create_dir_all(dir.join("hours")).unwrap();
    let audio = |s: f64| crate::audio::Audio {
        samples: vec![0.0; (8000.0 * s) as usize],
        sample_rate: 8000,
    };
    crate::audio::write_wav(&dir.join("take.wav"), &audio(90.0)).unwrap();
    crate::audio::write_wav(&dir.join("hours/audio-001.wav"), &audio(30.0)).unwrap();
    crate::audio::write_wav(&dir.join("hours/audio-002.wav"), &audio(15.25)).unwrap();
    fs::write(
        dir.join("session.toml"),
        "bph = 28800\n\n[[recording]]\nfile = 'take.wav'\nposition = 'DU'\n\
         \n[[recording]]\nfile = 'hours'\nposition = 'DD'\n",
    )
    .unwrap();
    let t = read_take(&dir, 0).unwrap();
    assert_eq!(t.started_utc, None);
    // Its folder isn't named as a take: no start from it either.
    assert_eq!(t.named_start, None);
    assert_eq!(t.duration_s, Some(135.25));
    assert_eq!(t.facts(false), ["Dial Up, Dial Down", "2:15"]);
    // One file split into two entries, each a stretch of it, is one
    // sound: its length counts once.
    fs::write(
        dir.join("session.toml"),
        "bph = 28800\n\n[[recording]]\nfile = 'take.wav'\nposition = 'DU'\nend_s = 40.0\n\
         \n[[recording]]\nfile = 'take.wav'\nposition = 'DD'\nstart_s = 40.0\n\
         \n[[recording]]\nfile = 'hours'\nposition = 'CU'\n",
    )
    .unwrap();
    assert_eq!(read_take(&dir, 0).unwrap().duration_s, Some(135.25));
    fs::write(dir.join("session.json"), r#"{"duration_s":140.0}"#).unwrap();
    assert_eq!(read_take(&dir, 0).unwrap().duration_s, Some(140.0));
    // Sound whose headers hold no samples gives no length, not 0:00
    // (#1300).
    fs::remove_file(dir.join("session.json")).unwrap();
    let empty = dir.join("empty");
    fs::create_dir_all(&empty).unwrap();
    crate::audio::write_wav(&empty.join("take.wav"), &audio(0.0)).unwrap();
    fs::write(
        empty.join("session.toml"),
        "bph = 28800\n\n[[recording]]\nfile = 'take.wav'\nposition = 'DU'\n",
    )
    .unwrap();
    assert_eq!(read_take(&empty, 0).unwrap().duration_s, None);
    fs::write(dir.join("session.json"), r#"{"duration_s":140.0}"#).unwrap();
    // One whose sound isn't all there has no length to give.
    fs::remove_file(dir.join("session.json")).unwrap();
    fs::remove_file(dir.join("take.wav")).unwrap();
    let t = read_take(&dir, 0).unwrap();
    assert!(t.incomplete && t.duration_s.is_none(), "{t:?}");
    // A folder named as the app names a take gives its start, until a
    // session.json records one.
    let named = dir.join("2026-11-06 18.30 Dial Up");
    fs::create_dir_all(&named).unwrap();
    crate::audio::write_wav(&named.join("take.wav"), &audio(60.0)).unwrap();
    fs::write(
        named.join("session.toml"),
        "bph = 28800\n\n[[recording]]\nfile = 'take.wav'\nposition = 'DU'\n",
    )
    .unwrap();
    let t = read_take(&named, 0).unwrap();
    assert_eq!(t.named_start.as_deref(), Some("2026-11-06T18:30"));
    assert_eq!(t.duration_s, Some(60.0));
    fs::write(
        named.join("session.json"),
        r#"{"started_utc":"2026-11-07T01:30:00Z"}"#,
    )
    .unwrap();
    let t = read_take(&named, 0).unwrap();
    assert_eq!(t.started_utc.as_deref(), Some("2026-11-07T01:30:00Z"));
    assert_eq!(t.named_start, None);
    let _ = fs::remove_dir_all(&dir);
}

/// The rate a take keeps is as heard; its clock log corrects it, as
/// reading the take does, and without a log it stays as heard.
#[test]
fn a_takes_rate_at_its_end_is_corrected_from_its_clock_log() {
    let dir = scratch("clock-log");
    fs::create_dir_all(&dir).unwrap();
    let audio = crate::audio::Audio {
        samples: vec![0.0; 8000 * 120],
        sample_rate: 8000,
    };
    crate::audio::write_wav(&dir.join("audio-001.wav"), &audio).unwrap();
    // The card's clock 50 ppm off the computer's.
    let mut log = String::from("unix_s,frames\n");
    for k in 0..13 {
        let audio_s = 10.0 * k as f64;
        log += &format!(
            "{:.6},{}\n",
            1_700_000_000.0 + audio_s * (1.0 + 50e-6),
            (audio_s * 8000.0) as u64
        );
    }
    fs::write(dir.join("clock.csv"), log).unwrap();
    fs::write(
        dir.join("session.toml"),
        "bph = 28800\n\n[[recording]]\nfile = '.'\nposition = 'DU'\nclock = 'clock.csv'\n",
    )
    .unwrap();
    fs::write(
        dir.join("session.json"),
        r#"{"result":{"rate_s_per_day":3.5,"amplitude_deg":281.0,"beat_error_unlock_ms":-0.4}}"#,
    )
    .unwrap();
    let a = read_take(&dir, 0).unwrap().at_end.unwrap();
    assert_eq!(a.rate_s_per_day, Some(3.5));
    let fit = crate::take::clock_of_files(
        &dir,
        Some("clock.csv"),
        &[dir.join("audio-001.wav")],
        None,
        None,
    )
    .unwrap()
    .unwrap();
    assert!((fit.ppm.abs() - 50.0).abs() < 0.5, "{}", fit.ppm);
    let want = crate::clockstore::correct_rate(3.5, fit.ppm);
    assert_eq!(a.rate_corrected_s_per_day, Some(want));
    assert!((want - 3.5).abs() > 4.0, "{want}");
    assert_eq!(a.rate(), Some(want));
    // A log that can't be read leaves the rate as heard.
    fs::write(dir.join("clock.csv"), "unix_s,frames\n").unwrap();
    let a = read_take(&dir, 0).unwrap().at_end.unwrap();
    assert_eq!(a.rate_corrected_s_per_day, None);
    assert_eq!(a.rate(), Some(3.5));
}

/// A take whose recording carries USB lines keeps its rate corrected
/// by them, not by its clock log, which then only places breaks: as
/// reading the take and `timegrapher series` do.
#[test]
fn a_takes_usb_lines_set_its_rate_over_its_clock_log() {
    let dir = scratch("usb-lines");
    fs::create_dir_all(&dir).unwrap();
    let fs_hz = 48_000u32;
    let n = fs_hz as usize * 60;
    // Quiet noise, and USB lines at 5, 6 and 7 kHz from a card 20 ppm
    // slow.
    let mut s = 7u64;
    let mut samples: Vec<f32> = (0..n)
        .map(|_| {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (((s >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * 0.02) as f32
        })
        .collect();
    for (f, a) in [(5000.0, 0.002), (6000.0, 0.003), (7000.0, 0.002)] {
        let f = f * (1.0 + 20e-6);
        for (i, v) in samples.iter_mut().enumerate() {
            *v += (a * (std::f64::consts::TAU * f * i as f64 / fs_hz as f64).sin()) as f32;
        }
    }
    let audio = crate::audio::Audio {
        samples,
        sample_rate: fs_hz,
    };
    crate::audio::write_wav(&dir.join("audio-001.wav"), &audio).unwrap();
    // A log that would correct on its own, at 50 ppm.
    let mut log = String::from("unix_s,frames\n");
    for k in 0..13 {
        let audio_s = 5.0 * k as f64;
        log += &format!(
            "{:.6},{}\n",
            1_700_000_000.0 + audio_s * (1.0 + 50e-6),
            (audio_s * fs_hz as f64) as u64
        );
    }
    fs::write(dir.join("clock.csv"), log).unwrap();
    fs::write(
        dir.join("session.toml"),
        "bph = 28800\n\n[[recording]]\nfile = '.'\nposition = 'DU'\nclock = 'clock.csv'\n",
    )
    .unwrap();
    fs::write(
        dir.join("session.json"),
        r#"{"result":{"rate_s_per_day":3.5,"amplitude_deg":281.0,"beat_error_unlock_ms":-0.4}}"#,
    )
    .unwrap();
    let a = read_take(&dir, 0).unwrap().at_end.unwrap();
    let usb = crate::clock::usb_lines(&[dir.join("audio-001.wav")])
        .unwrap()
        .expect("lines found");
    assert!((usb.ppm + 20.0).abs() < 0.5, "{}", usb.ppm);
    // Core's reading of them, for every surface, is the same.
    let files = [dir.join("audio-001.wav")];
    assert_eq!(crate::take::usb(&files).unwrap(), Some(usb.correction()));
    let want = crate::clockstore::correct_rate(3.5, usb.ppm);
    assert_eq!(a.rate_corrected_s_per_day, Some(want));
    // Without a clock log the USB lines still set it, as reading the
    // take does.
    fs::write(
        dir.join("session.toml"),
        "bph = 28800\n\n[[recording]]\nfile = '.'\nposition = 'DU'\n",
    )
    .unwrap();
    let a = read_take(&dir, 0).unwrap().at_end.unwrap();
    assert_eq!(a.rate_corrected_s_per_day, Some(want));
}

#[test]
fn a_library_lists_its_watches_and_their_takes() {
    let root = scratch("lists");
    let mut lib = Library::create(&root).unwrap();
    assert!(lib.watches.is_empty());
    let mut w = Watch::new("Yacht-Master 42 replica");
    w.calibre = Some("Dandong 3235".into());
    w.bph = Some(28800);
    w.fields.push(Field {
        label: "Serial Number".into(),
        kind: FieldKind::Text,
        value: "X123".into(),
    });
    let dir = lib.add(&w).unwrap();
    assert_eq!(file_name(&dir), "Yacht-Master 42 replica");
    let takes = dir.join(TAKES);
    take(
        &takes.join("2026-10-20 14.32 Dial Up"),
        &[("take_part01-001.flac", "DU")],
        1,
        "2026-10-20T21:32:00Z",
    );
    take(
        &takes.join("2026-10-21 09.10"),
        &[
            ("take_part01-001.flac", "DU"),
            ("take_part02-001.flac", "DD"),
        ],
        1,
        "2026-10-21T16:10:00Z",
    );
    fs::write(dir.join(PHOTOS).join("dial.jpg"), b"jpeg").unwrap();
    fs::write(dir.join(PHOTOS).join("notes.txt"), b"not a photo").unwrap();
    lib.scan();
    let e = &lib.watches[0];
    assert_eq!(e.watch, w);
    assert_eq!(e.photos, [dir.join(PHOTOS).join("dial.jpg")]);
    let names: Vec<&str> = e.takes.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["2026-10-21 09.10", "2026-10-20 14.32 Dial Up"]);
    // The six-position take is missing its second file.
    assert!(e.takes[0].incomplete);
    assert!(!e.takes[1].incomplete);
    assert_eq!(e.takes[0].positions, ["DU", "DD"]);
    let one = &e.takes[1];
    assert!(one.audio);
    assert_eq!(one.duration_s, Some(120.0));
    assert_eq!(
        one.at_end,
        Some(AtEnd {
            rate_s_per_day: Some(3.5),
            amplitude_deg: Some(281.0),
            beat_error_ms: Some(-0.4),
            rate_corrected_s_per_day: None,
        })
    );
    let file_bytes = |t: &str| -> u64 {
        fs::read_dir(takes.join(t))
            .unwrap()
            .map(|e| e.unwrap().metadata().unwrap().len())
            .sum()
    };
    assert_eq!(one.bytes, file_bytes("2026-10-20 14.32 Dial Up"));
    assert_eq!(
        lib.bytes(),
        file_bytes("2026-10-20 14.32 Dial Up") + file_bytes("2026-10-21 09.10")
    );
    // Found again from the folders alone, after the library is moved.
    let moved = scratch("lists-moved");
    let takes = lib.watches[0].takes.clone();
    lib.unlock();
    move_library(&root, &moved).unwrap();
    assert!(!root.exists());
    let again = Library::open(&moved).unwrap();
    assert_eq!(again.watches[0].watch, w);
    assert_eq!(again.watches[0].takes, takes);
    let _ = fs::remove_dir_all(&moved);
}

#[test]
fn a_take_kept_as_analysis_only_reads_as_such() {
    let root = scratch("analysis-only");
    let mut lib = Library::create(&root).unwrap();
    let dir = lib.add(&Watch::new("Pepsi")).unwrap();
    let t = dir.join(TAKES).join("2026-10-22 22.00 Dial Up");
    take(
        &t,
        &[("take_part01-001.flac", "DU")],
        0,
        "2026-10-23T05:00:00Z",
    );
    let toml = fs::read_to_string(t.join("session.toml")).unwrap();
    fs::write(t.join("session.toml"), format!("audio = false\n{toml}")).unwrap();
    lib.scan();
    let k = &lib.watches[0].takes[0];
    assert!(!k.audio);
    // Without its analysis/ folder it is incomplete; with it, whole,
    // though its sound files are gone.
    assert!(k.incomplete);
    fs::create_dir(t.join("analysis")).unwrap();
    lib.scan();
    assert!(!lib.watches[0].takes[0].incomplete);
    // Its length, with no session.json to give it, is the analysis's.
    // A header that doesn't read as the analysis gives none.
    let _ = fs::remove_file(t.join("session.json"));
    let header = t.join("analysis").join("analysis.json");
    fs::write(&header, r#"{"duration_s": 3600.0}"#).unwrap();
    lib.scan();
    assert_eq!(lib.watches[0].takes[0].duration_s, None);
    let log = crate::stream::BeatLog {
        sample_rate: 48_000,
        duration_s: 3600.0,
        bph: 28_800,
        lift_deg: 52.0,
        beats: Vec::new(),
        amplitude_windows: Vec::new(),
        clipped_beats: Vec::new(),
        clipped_samples: 0,
        jumps: Vec::new(),
        sided: Vec::new(),
        by_count: Vec::new(),
        snippets: Vec::new(),
    };
    let stored = crate::analysis_take::from_log(
        log,
        &crate::stream::StreamConfig::default(),
        2,
        serde_json::Value::Null,
    );
    crate::analysis_take::write(&t, &stored).unwrap();
    lib.scan();
    assert_eq!(lib.watches[0].takes[0].duration_s, Some(3600.0));
    // A layout from a newer version gives none either. Its name is
    // longer, so the take's fingerprint changes even when the rewrite
    // lands within the same modified-time tick.
    let text = fs::read_to_string(&header).unwrap();
    fs::write(
        &header,
        text.replace(crate::analysis_take::FORMAT, "timegrapher.analysis/99"),
    )
    .unwrap();
    lib.scan();
    assert_eq!(lib.watches[0].takes[0].duration_s, None);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_cache_is_used_until_a_take_changes() {
    let root = scratch("cache");
    let mut lib = Library::create(&root).unwrap();
    let dir = lib.add(&Watch::new("Daytona")).unwrap();
    let t = dir.join(TAKES).join("2026-10-20 10.00 Dial Up");
    take(
        &t,
        &[("take_part01-001.flac", "DU")],
        1,
        "2026-10-20T17:00:00Z",
    );
    lib.scan();
    assert!(root.join(CACHE_FILE).is_file());
    // A cache entry is trusted while the take's files are unchanged...
    let cache = fs::read_to_string(root.join(CACHE_FILE)).unwrap();
    fs::write(root.join(CACHE_FILE), cache.replace("3.5", "9.5")).unwrap();
    lib.scan();
    let rate = |l: &Library| {
        l.watches[0].takes[0]
            .at_end
            .as_ref()
            .unwrap()
            .rate_s_per_day
    };
    assert_eq!(rate(&lib), Some(9.5));
    // ...and read afresh when they change.
    fs::write(t.join("clock.csv"), "x\n").unwrap();
    lib.scan();
    assert_eq!(rate(&lib), Some(3.5));
    // A cache that is gone or unreadable is made again.
    fs::write(root.join(CACHE_FILE), "not json").unwrap();
    lib.scan();
    assert_eq!(rate(&lib), Some(3.5));
    // A cache an earlier build wrote, in an older format, is not used.
    lib.scan();
    let cache = fs::read_to_string(root.join(CACHE_FILE)).unwrap();
    let want = format!("\"format\":{CACHE_FORMAT}");
    assert!(cache.contains(&want), "{cache}");
    let old = cache.replace("3.5", "9.5").replace(&want, "\"format\":1");
    fs::write(root.join(CACHE_FILE), old).unwrap();
    lib.scan();
    assert_eq!(rate(&lib), Some(3.5));
    // Nor one from the build before USB lines corrected a take's rate.
    let cache = fs::read_to_string(root.join(CACHE_FILE)).unwrap();
    let old = cache.replace("3.5", "9.5").replace(&want, "\"format\":2");
    fs::write(root.join(CACHE_FILE), old).unwrap();
    lib.scan();
    assert_eq!(rate(&lib), Some(3.5));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn names_are_unique_and_folders_safe_everywhere() {
    let root = scratch("names");
    let mut lib = Library::create(&root).unwrap();
    lib.add(&Watch::new("Seiko 5")).unwrap();
    assert!(lib.add(&Watch::new("  seiko 5 ")).is_err());
    assert!(lib.add(&Watch::new("   ")).is_err());
    // Another name that cleans to the same folder gets a number.
    let a = lib.add(&Watch::new("Omega: 321")).unwrap();
    let b = lib.add(&Watch::new("Omega? 321")).unwrap();
    assert_eq!(file_name(&a), "Omega- 321");
    assert_eq!(file_name(&b), "Omega- 321 2");
    assert_eq!(lib.find("omega? 321").unwrap().watch.name, "Omega? 321");
    // Renaming a watch keeps its folder; taking another's name is refused.
    let mut w = lib.find("Omega: 321").unwrap().watch.clone();
    w.name = "Speedmaster".into();
    lib.update(&w).unwrap();
    assert_eq!(lib.find("Speedmaster").unwrap().dir, a);
    w.name = "Seiko 5".into();
    assert!(lib.update(&w).is_err());
    assert_eq!(folder_name("CON"), "CON_");
    assert_eq!(folder_name("lpt1.txt"), "lpt1_.txt");
    assert_eq!(folder_name("Com2"), "Com2_");
    assert_eq!(folder_name("COM¹"), "COM¹_");
    assert_eq!(folder_name("CONSOLE"), "CONSOLE");
    assert_eq!(folder_name("COM10"), "COM10");
    assert_eq!(folder_name("Tudor..."), "Tudor");
    assert_eq!(folder_name("a/b\\c\u{7}"), "a-b-c-");
    assert_eq!(folder_name(""), "Watch");
    assert_eq!(folder_name(&"x".repeat(300)).len(), 100);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_library_is_made_only_in_an_empty_folder_and_read_by_its_version() {
    let root = scratch("create");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("something.txt"), "x").unwrap();
    assert!(Library::create(&root).is_err());
    assert!(Library::open(&root)
        .unwrap_err()
        .contains("not a Timegrapher Watches folder"));
    assert!(Library::open_or_create(&root).is_err());
    fs::remove_file(root.join("something.txt")).unwrap();
    Library::open_or_create(&root).unwrap();
    fs::write(root.join(LIBRARY_FILE), "format = 99\n").unwrap();
    assert!(Library::open(&root).unwrap_err().contains("newer version"));
    // A move never lands on files already there.
    let other = scratch("create-other");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("keep.txt"), "x").unwrap();
    fs::write(root.join(LIBRARY_FILE), "format = 1\n").unwrap();
    assert!(move_library(&root, &other).is_err());
    assert!(move_library(&root, &root.join("inside")).is_err());
    // Nor inside itself by way of a link to it.
    #[cfg(unix)]
    {
        let link = scratch("create-link");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        // Lexically outside the library, so only the links followed
        // find it inside.
        let refused = move_library(&root, &link.join("inside")).unwrap_err();
        assert!(refused.contains("inside itself"), "{refused}");
        let _ = fs::remove_file(&link);
    }
    assert!(Library::is_library(&root) && other.join("keep.txt").is_file());
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&other);
}

/// Onto another disk the library is copied, checked, and only then
/// removed, its library.toml first.
#[test]
fn a_move_onto_another_disk_copies_then_removes() {
    let root = scratch("across");
    let mut lib = Library::create(&root).unwrap();
    let dir = lib.add(&Watch::new("Pepsi")).unwrap();
    take(
        &dir.join(TAKES).join("2026-10-20 10.00 Dial Up"),
        &[("take_part01-001.flac", "DU")],
        1,
        "2026-10-20T17:00:00Z",
    );
    // A read-only file is copied, and stays read-only.
    let locked = dir.join(PHOTOS).join("dial.jpg");
    fs::write(&locked, b"jpeg").unwrap();
    let mut ro = fs::metadata(&locked).unwrap().permissions();
    ro.set_readonly(true);
    fs::set_permissions(&locked, ro).unwrap();
    // An old file keeps its modification time.
    let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
    let notes = dir.join("notes.txt");
    fs::write(&notes, b"serviced").unwrap();
    fs::File::options()
        .write(true)
        .open(&notes)
        .unwrap()
        .set_modified(old)
        .unwrap();
    lib.scan();
    let takes = lib.watches[0].takes.clone();
    drop(lib);
    let to = scratch("across-to");
    // Removing a read-only file fails on Windows; the whole library is
    // at `to` either way.
    let moved = copy_across(&root, &to, None).unwrap();
    let notes = to.join(file_name(&dir)).join("notes.txt");
    assert_eq!(fs::metadata(&notes).unwrap().modified().unwrap(), old);
    // The lock is not copied.
    assert!(!to.join(LOCK_FILE).exists());
    let copy = to.join(file_name(&dir)).join(PHOTOS).join("dial.jpg");
    assert_eq!(fs::read(&copy).unwrap(), b"jpeg");
    assert!(fs::metadata(&copy).unwrap().permissions().readonly());
    if cfg!(windows) {
        if moved.is_some() {
            let mut rw = fs::metadata(&locked).unwrap().permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            rw.set_readonly(false);
            fs::set_permissions(&locked, rw).unwrap();
            let _ = fs::remove_dir_all(&root);
        }
    } else {
        assert_eq!(moved, None);
    }
    assert!(!root.exists());
    let moved = Library::open(&to).unwrap();
    assert_eq!(moved.watches[0].takes, takes);
    let _ = fs::remove_dir_all(&to);
}

/// A link, or a special file such as a named pipe (which would never
/// finish reading) or a socket, stops a copy onto another disk; the copy is removed and the library left.
#[cfg(unix)]
#[test]
fn a_move_onto_another_disk_refuses_links_and_special_files() {
    let root = scratch("special");
    let mut lib = Library::create(&root).unwrap();
    let dir = lib.add(&Watch::new("Pepsi")).unwrap();
    drop(lib);
    let outside = scratch("special-outside");
    fs::write(&outside, b"not the library's").unwrap();
    let link = dir.join(PHOTOS).join("dial.jpg");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let to = scratch("special-to");
    let refused = copy_across(&root, &to, None).unwrap_err();
    assert!(refused.contains("link or a special file"), "{refused}");
    assert!(!to.exists());
    assert!(Library::is_library(&root) && link.is_symlink());
    fs::remove_file(&link).unwrap();
    // A socket, made without starting a program (a program started
    // while another test drops a lock would hold it a moment).
    let socket = std::os::unix::net::UnixListener::bind(dir.join("socket")).unwrap();
    let refused = copy_across(&root, &to, None).unwrap_err();
    assert!(refused.contains("link or a special file"), "{refused}");
    assert!(!to.exists());
    drop(socket);
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&outside);
}

/// A take whose session file names its whole folder, as the app's own
/// takes do, is complete while the folder holds its sound.
#[test]
fn a_take_naming_its_folder_is_complete_with_its_sound() {
    let root = scratch("folder-entry");
    let mut lib = Library::create(&root).unwrap();
    let dir = lib.add(&Watch::new("Pepsi")).unwrap();
    let t = dir.join(TAKES).join("2026-10-22 09.15");
    take(&t, &[(".", "DU")], 0, "2026-10-22T16:15:00Z");
    lib.scan();
    assert!(lib.watches[0].takes[0].incomplete, "no sound yet");
    fs::write(t.join("take_part01-001.flac"), vec![0u8; 1000]).unwrap();
    lib.scan();
    assert!(!lib.watches[0].takes[0].incomplete);
    let _ = fs::remove_dir_all(&root);
}

/// Reading a library to list it (the command line) takes no lock and
/// writes nothing, so it neither shuts out the app nor is shut out.
#[test]
fn a_library_read_to_list_it_takes_no_lock_and_writes_nothing() {
    let root = scratch("read");
    let mut app = Library::create(&root).unwrap();
    let dir = app.add(&Watch::new("Pepsi")).unwrap();
    take(
        &dir.join(TAKES).join("2026-10-22 09.15"),
        &[("take_part01-001.flac", "DU")],
        1,
        "2026-10-22T16:15:00Z",
    );
    let _ = fs::remove_file(root.join(CACHE_FILE));
    let mut listed = Library::read(&root).unwrap();
    assert_eq!(listed.watches[0].takes.len(), 1);
    assert!(listed.read_only.is_some());
    assert!(!root.join(CACHE_FILE).exists(), "the cache is left alone");
    assert!(listed.add(&Watch::new("Daytona")).is_err());
    // The app, which has it open, still changes it; and a copy opened
    // while the command line reads it is not made read-only.
    app.add(&Watch::new("Daytona")).unwrap();
    drop(app);
    let reopened = Library::open(&root).unwrap();
    assert_eq!(reopened.read_only, None);
    listed.scan();
    assert_eq!(listed.watches.len(), 2);
    drop(reopened);
    let _ = fs::remove_dir_all(&root);
}

/// A take made with Quick Listen is listed in Unsorted, counted in
/// the library's size, can be renamed or deleted there, and is filed
/// under a watch by a move. No watch can take Unsorted's name, and a
/// folder called Unsorted that holds a watch is left to it.
#[test]
fn photos_are_copied_in_under_free_names_and_deleted_one_at_a_time() {
    let root = scratch("photos");
    let mut lib = Library::create(&root).unwrap();
    let dir = lib.add(&Watch::new("Pepsi")).unwrap();
    let id = lib.watches[0].watch.id.clone();
    let from = root.join("from phone");
    fs::create_dir_all(&from).unwrap();
    let a = from.join("IMG_0001.HEIC");
    let b = from.join("dial.jpg");
    fs::write(&a, b"heic").unwrap();
    fs::write(&b, b"jpeg").unwrap();
    let added = lib.add_photos(&id, &[a.clone(), b.clone()]).unwrap();
    let photos = dir.join(PHOTOS);
    assert_eq!(
        added,
        [photos.join("IMG_0001.HEIC"), photos.join("dial.jpg")]
    );
    // The same again: numbered, the first copies untouched.
    let again = lib.add_photos(&id, std::slice::from_ref(&b)).unwrap();
    assert_eq!(again, [photos.join("dial 2.jpg")]);
    assert_eq!(fs::read(&again[0]).unwrap(), b"jpeg");
    assert_eq!(
        lib.watches[0].photos,
        [
            photos.join("IMG_0001.HEIC"),
            photos.join("dial 2.jpg"),
            photos.join("dial.jpg")
        ]
    );
    // Not a photo, or a watch that's gone: nothing is copied.
    let txt = from.join("notes.txt");
    fs::write(&txt, b"x").unwrap();
    assert!(lib.add_photos(&id, &[b.clone(), txt]).is_err());
    assert!(lib.add_photos("gone", std::slice::from_ref(&b)).is_err());
    assert_eq!(fs::read_dir(&photos).unwrap().count(), 3);
    // Deleting: only the library's own photos.
    assert!(lib.delete_photo(&b).is_err());
    assert!(b.exists());
    lib.delete_photo(&photos.join("dial.jpg")).unwrap();
    assert_eq!(lib.watches[0].photos.len(), 2);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn quick_listen_takes_wait_in_unsorted_until_filed() {
    let root = scratch("unsorted");
    let mut lib = Library::create(&root).unwrap();
    lib.add(&Watch::new("Pepsi")).unwrap();
    let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
    let unsorted = lib.unsorted_dir().unwrap();
    assert_eq!(unsorted, root.join(UNSORTED).join(TAKES));
    let t = unsorted.join("2026-10-22 09.15 Dial Up");
    take(
        &t,
        &[("take_part01-001.flac", "DU")],
        1,
        "2026-10-22T16:15:00Z",
    );
    let u = unsorted.join("2026-10-22 09.40");
    take(
        &u,
        &[("take_part01-001.flac", "DD")],
        1,
        "2026-10-22T16:40:00Z",
    );
    lib.scan();
    let names: Vec<&str> = lib.unsorted.takes.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["2026-10-22 09.40", "2026-10-22 09.15 Dial Up"]);
    assert_eq!(lib.unsorted.take_dir(&lib.unsorted.takes[1]), t);
    assert_eq!(lib.watches.len(), 1);
    assert!(lib.bytes() >= lib.unsorted.bytes() && lib.unsorted.bytes() > 0);
    assert!(lib.is_unsorted(&t) && lib.watch_of(&t).is_none());
    // The cache keeps Unsorted's takes too.
    let cache = fs::read_to_string(root.join(CACHE_FILE)).unwrap();
    assert!(cache.contains("Unsorted/takes/2026-10-22 09.15 Dial Up"));

    let t = lib.rename_take(&t, "Handed Over").unwrap();
    assert!(lib.is_unsorted(&t));
    let filed = lib.move_take(&t, &pepsi).unwrap();
    assert_eq!(lib.watch_of(&filed).unwrap().watch.name, "Pepsi");
    assert_eq!(lib.find("Pepsi").unwrap().takes.len(), 1);
    lib.delete_take(&u).unwrap();
    assert!(lib.unsorted.takes.is_empty());
    // Nothing can be moved into Unsorted by its empty id.
    assert!(lib.move_take(&filed, "").is_err());

    for name in ["Unsorted", " unsorted ", "UNSORTED.", "Unsorted?"] {
        let refused = lib.add(&Watch::new(name));
        if name == "Unsorted?" {
            // A different folder: "Unsorted-".
            assert!(refused.is_ok(), "{name}");
            continue;
        }
        assert!(refused.unwrap_err().contains("Quick Listen"), "{name}");
    }
    let mut w = lib.find("Pepsi").unwrap().watch.clone();
    w.name = "unsorted".into();
    assert!(lib.update(&w).unwrap_err().contains("Quick Listen"));

    // A watch someone made by hand in a folder called Unsorted.
    Watch::new("By Hand").write(&root.join(UNSORTED)).unwrap();
    lib.scan();
    assert!(lib.find("By Hand").is_some());
    assert!(lib.unsorted.takes.is_empty());
    assert!(lib.unsorted_dir().unwrap_err().contains("holds a watch"));
    let _ = fs::remove_dir_all(&root);
}

/// A take can be renamed, moved to another watch and deleted; a take
/// from outside is added by a copy or a move; a kept take replaces
/// the take it was kept from. None of it in a library open elsewhere.
#[test]
fn takes_are_renamed_moved_deleted_added_and_replaced() {
    let root = scratch("take-ops");
    let mut lib = Library::create(&root).unwrap();
    let a = lib.add(&Watch::new("Pepsi")).unwrap();
    let b = lib.add(&Watch::new("Daytona")).unwrap();
    let b_id = lib.find("Daytona").unwrap().watch.id.clone();
    let t = a.join(TAKES).join("2026-10-20 10.00 Dial Up");
    take(
        &t,
        &[("take_part01-001.flac", "DU")],
        1,
        "2026-10-20T17:00:00Z",
    );
    lib.scan();
    assert_eq!(lib.watch_of(&t).unwrap().dir, a);

    // Rename: cleaned, never over another take.
    let t = lib.rename_take(&t, "Before Service: DU").unwrap();
    assert_eq!(file_name(&t), "Before Service- DU");
    take(
        &a.join(TAKES).join("Other"),
        &[("take_part01-001.flac", "DU")],
        1,
        "2026-10-21T17:00:00Z",
    );
    lib.scan();
    assert!(lib
        .rename_take(&t, "Other")
        .unwrap_err()
        .contains("already"));
    assert!(lib.rename_take(&t, "  ").is_err());

    // Move into Daytona, then delete there.
    let moved = lib.move_take(&t, &b_id).unwrap();
    assert_eq!(moved, b.join(TAKES).join("Before Service- DU"));
    assert!(!t.exists());
    assert_eq!(lib.find("Daytona").unwrap().takes.len(), 1);
    lib.delete_take(&moved).unwrap();
    assert!(!moved.exists() && lib.find("Daytona").unwrap().takes.is_empty());
    assert!(lib.delete_take(&root.join("elsewhere")).is_err());

    // Added from outside: a copy keeps the original, a move doesn't.
    let outside = scratch("take-ops-outside");
    take(
        &outside,
        &[("take_part01-001.flac", "CL")],
        1,
        "2026-10-22T17:00:00Z",
    );
    let (copy, left) = lib.add_take(&outside, &b_id, true).unwrap();
    assert_eq!(left, None);
    assert!(outside.join("session.toml").is_file() && copy.join("session.toml").is_file());
    let (second, _) = lib.add_take(&outside, &b_id, false).unwrap();
    assert!(!outside.exists());
    assert_ne!(copy, second);
    assert_eq!(lib.find("Daytona").unwrap().takes.len(), 2);
    assert!(lib
        .add_take(&copy, &b_id, true)
        .unwrap_err()
        .contains("already in"));
    assert!(lib
        .add_take(&root, &b_id, true)
        .unwrap_err()
        .contains("not a take"));

    // A kept take replaces the one it was kept from, under its name.
    let kept = second.with_file_name(".kept");
    fs::create_dir_all(kept.join(crate::analysis_take::DIR)).unwrap();
    fs::write(
        kept.join(crate::analysis_take::DIR)
            .join(crate::analysis_take::HEADER),
        "{}",
    )
    .unwrap();
    fs::write(kept.join("session.toml"), "audio = false\n").unwrap();
    assert!(lib.replace_take(&second, &kept).unwrap().is_none());
    assert!(crate::analysis_take::is_analysis_only(&second));
    assert!(!kept.exists());
    assert_eq!(fs::read_dir(b.join(TAKES)).unwrap().count(), 2);

    // A second copy of the app changes none of it.
    let mut other = Library::open(&root).unwrap();
    assert!(other.rename_take(&copy, "x").is_err());
    assert!(other.delete_take(&copy).is_err() && copy.exists());
    drop(lib);
    let _ = fs::remove_dir_all(&root);
}

/// The second copy of the app to open a library can only read it, and
/// a library open elsewhere is not moved.
#[test]
fn a_second_copy_of_the_app_opens_the_library_read_only() {
    let root = scratch("second");
    let mut first = Library::create(&root).unwrap();
    assert_eq!(first.read_only, None);
    first.add(&Watch::new("Pepsi")).unwrap();
    let mut second = Library::open(&root).unwrap();
    let why = second.read_only.clone().unwrap();
    assert!(why.contains("Another copy"), "{why}");
    assert_eq!(second.watches.len(), 1);
    assert_eq!(second.add(&Watch::new("Daytona")).unwrap_err(), why);
    let mut w = second.watches[0].watch.clone();
    w.notes = Some("changed".into());
    assert!(second.update(&w).is_err());
    // Not even its cache.
    fs::remove_file(root.join(CACHE_FILE)).unwrap();
    second.scan();
    assert!(!root.join(CACHE_FILE).exists());
    let to = scratch("second-to");
    let refused = move_library(&root, &to).unwrap_err();
    assert!(refused.contains("Another copy"), "{refused}");
    assert!(Library::is_library(&root) && !to.exists());
    // Once the first lets go, the next to open it may change it.
    drop(first);
    drop(second);
    let mut third = Library::open(&root).unwrap();
    assert_eq!(third.read_only, None);
    third.add(&Watch::new("Daytona")).unwrap();
    third.unlock();
    assert!(third.read_only.is_some());
    move_library(&root, &to).unwrap_or_else(|e| panic!("{e}"));
    assert!(Library::is_library(&to));
    let _ = fs::remove_dir_all(&to);
}

#[test]
fn a_take_is_named_by_its_start_and_its_one_position() {
    assert_eq!(
        take_name("2026-10-20 14.32", &[Position::CH]),
        "2026-10-20 14.32 Dial Up"
    );
    assert_eq!(
        take_name("2026-10-20 18.05", &[Position::H6, Position::H6]),
        "2026-10-20 18.05 Crown Left"
    );
    assert_eq!(
        take_name("2026-10-21 09.10", &[Position::CH, Position::CB]),
        "2026-10-21 09.10"
    );
    assert_eq!(take_name("2026-10-21 09.10", &[]), "2026-10-21 09.10");
}

#[test]
fn watch_toml_reads_as_a_person_wrote_it() {
    let w: Watch = toml::from_str(
        r#"
id = "0123456789abcdef"
name = "Pepsi"
calibre = "Rolex 3285"
bph = 28800
archived = true

[[field]]
label = "Purchase Price"
kind = "money"
value = "£9,000"
"#,
    )
    .unwrap();
    assert!(w.archived && w.lift.is_none());
    assert_eq!(w.fields[0].kind, FieldKind::Money);
    let back: Watch = toml::from_str(&toml::to_string_pretty(&w).unwrap()).unwrap();
    assert_eq!(back, w);
    assert_ne!(Watch::new("a").id, Watch::new("a").id);
}

#[test]
fn sizes_and_lengths_read_as_the_app_shows_them() {
    assert_eq!(size_text(40), "40 bytes");
    assert_eq!(size_text(912_400), "912 kB");
    assert_eq!(size_text(58_300_000), "58.3 MB");
    assert_eq!(size_text(7_700_000_000), "7.7 GB");
    assert_eq!(length_text(245.0), "4:05");
    assert_eq!(length_text(5400.0), "1:30:00");
}

/// A signed figure reads as the app's pane writes it: a figure that
/// rounds to zero takes no sign, either way.
#[test]
fn a_signed_figure_takes_no_sign_at_zero() {
    assert_eq!(signed_text(1.24, 1), "+1.2");
    assert_eq!(signed_text(-1.24, 1), "-1.2");
    assert_eq!(signed_text(-0.04, 1), "0.0");
    assert_eq!(signed_text(0.04, 1), "0.0");
    assert_eq!(signed_text(-0.4, 0), "0");
    assert_eq!(signed_text(0.0, 2), "0.00");
    assert_eq!(signed_text(-0.006, 2), "-0.01");
}

/// A figure keeps its own sign, but one that rounds to zero has no
/// minus, whatever its decimals (#815).
#[test]
fn a_fixed_figure_has_no_minus_at_zero() {
    assert_eq!(fixed_text(1.24, 1), "1.2");
    assert_eq!(fixed_text(-1.24, 1), "-1.2");
    assert_eq!(fixed_text(-0.04, 1), "0.0");
    assert_eq!(fixed_text(-0.0, 1), "0.0");
    assert_eq!(fixed_text(-0.4, 0), "0");
    assert_eq!(fixed_text(-0.004, 2), "0.00");
    assert_eq!(fixed_text(-0.006, 2), "-0.01");
    assert_eq!(fixed_text(-10.0, 0), "-10");
}

/// A take listed with its own reading shows that reading's figures, as
/// the card would; with none, or for a test, it shows no figures. A
/// position the take names in its own words keeps them, and how a take
/// was kept or cut short is said last.
#[test]
fn a_takes_facts_read_show_its_own_reading() {
    let t = Take {
        name: "t".into(),
        bytes: 0,
        audio: false,
        incomplete: true,
        started_utc: None,
        named_start: None,
        duration_s: Some(3725.0),
        positions: vec!["DU".into(), "Crown Left at 45°".into()],
        at_end: None,
        power_reserve: None,
    };
    let reading = Reading {
        position: Some("DU".into()),
        rate_s_per_day: Some(-2.04),
        calibrated: true,
        amplitude_deg: Some(265.4),
        beat_error_ms: Some(0.06),
        beat_error_source: Some(crate::session::BeatErrorSource::Unlock),
        caveats: Vec::new(),
        watch_heard: true,
    };
    let named = ["Dial Up, Crown Left at 45°", "1:02:05"];
    let kept = ["Analysis Only", "Incomplete"];
    assert_eq!(
        t.facts_read(false, Some(&reading)),
        [&named[..], &["-2.0 s/d", "265°", "0.06 ms"], &kept[..]].concat()
    );
    assert_eq!(t.facts_read(false, None), [&named[..], &kept[..]].concat());
    assert_eq!(
        t.facts_read(true, Some(&reading)),
        [&["Positions Test"], &named[..], &kept[..]].concat()
    );
    // A Power Reserve run the watch didn't end says how it ended.
    let run = Take {
        power_reserve: Some(crate::power_reserve::Summary {
            run_s: 5400.0,
            ending: crate::power_reserve::Ending::ByHand,
            position: "DU".into(),
        }),
        audio: true,
        incomplete: false,
        positions: vec!["DU".into()],
        ..t
    };
    assert_eq!(
        run.facts_read(false, Some(&reading)),
        [
            "Power Reserve 1 h 30 min",
            "Stopped by hand while the watch still ran",
            "Dial Up"
        ]
    );
}

/// A watch or take the library can't read is listed as unreadable, with
/// the file and what is wrong with it, not left out without a word; the
/// rest are listed as before.
#[test]
fn what_cant_be_read_is_listed_as_unreadable() {
    let root = scratch("unreadable");
    let mut lib = Library::create(&root).unwrap();
    let good = lib.add(&Watch::new("Pepsi")).unwrap();
    let nameless = lib.add(&Watch::new("Daytona")).unwrap();
    let broken = lib.add(&Watch::new("Explorer II")).unwrap();
    fs::write(nameless.join(WATCH_FILE), "id = \"x\"\nname = \"  \"\n").unwrap();
    fs::write(broken.join(WATCH_FILE), "name = [").unwrap();
    take(
        &good.join(TAKES).join("2026-10-20 10.00 Dial Up"),
        &[("take_part01-001.flac", "DU")],
        1,
        "2026-10-20T17:00:00Z",
    );
    let bad = good.join(TAKES).join("2026-10-21 10.00 Dial Down");
    fs::create_dir_all(&bad).unwrap();
    fs::write(bad.join(take::MANIFEST), "[[recording]\n").unwrap();
    lib.scan();
    let names: Vec<_> = lib.watches.iter().map(|e| e.watch.name.as_str()).collect();
    assert_eq!(names, ["Pepsi"]);
    assert_eq!(lib.watches[0].takes.len(), 1);
    let mut said = lib.unreadable.clone();
    said.sort();
    assert_eq!(said.len(), 3, "{said:?}");
    assert!(said.iter().any(|e| e.contains("the watch has no name")
        && e.contains(&nameless.join(WATCH_FILE).display().to_string())));
    assert!(said
        .iter()
        .any(|e| e.contains(&broken.join(WATCH_FILE).display().to_string())));
    assert!(said
        .iter()
        .any(|e| e.contains(&bad.join(take::MANIFEST).display().to_string())));
    // Put right, each is listed again, and nothing is left said.
    fs::write(bad.join(take::MANIFEST), "bph = 28800\n").unwrap();
    fs::remove_dir_all(&nameless).unwrap();
    fs::remove_dir_all(&broken).unwrap();
    lib.scan();
    assert!(lib.unreadable.is_empty(), "{:?}", lib.unreadable);
    assert_eq!(lib.watches[0].takes.len(), 2);
    let _ = fs::remove_dir_all(&root);
}

/// Only a kept take beside the take, in this library, replaces it; the
/// take is left as it was otherwise. A Positions Test's take is known by
/// the results it keeps.
#[test]
fn only_a_kept_take_beside_it_replaces_a_take() {
    let root = scratch("replace-refused");
    let mut lib = Library::create(&root).unwrap();
    let dir = lib.add(&Watch::new("Pepsi")).unwrap();
    let t = dir.join(TAKES).join("2026-10-20 10.00 Dial Up");
    take(
        &t,
        &[("take_part01-001.flac", "DU")],
        1,
        "2026-10-20T17:00:00Z",
    );
    lib.scan();
    let kept_at = |p: &Path| {
        fs::create_dir_all(p.join(crate::analysis_take::DIR)).unwrap();
        fs::write(
            p.join(crate::analysis_take::DIR)
                .join(crate::analysis_take::HEADER),
            "{}",
        )
        .unwrap();
        fs::write(p.join("session.toml"), "audio = false\n").unwrap();
    };
    let elsewhere = scratch("replace-refused-elsewhere");
    kept_at(&elsewhere);
    let not_kept = t.with_file_name(".not-kept");
    fs::create_dir_all(&not_kept).unwrap();
    for kept in [&elsewhere, &not_kept] {
        let e = lib.replace_take(&t, kept).unwrap_err();
        assert!(e.contains("is not a kept take beside it"), "{e}");
        assert!(t.join("take_part01-001.flac").is_file() && kept.exists());
    }
    // A take outside the library is refused though its kept take is
    // beside it, and neither is touched.
    let outside = elsewhere.join("2026-10-20 10.00 Dial Up");
    take(
        &outside,
        &[("take_part01-001.flac", "DU")],
        1,
        "2026-10-20T17:00:00Z",
    );
    let beside = elsewhere.join(".kept");
    kept_at(&beside);
    let e = lib.replace_take(&outside, &beside).unwrap_err();
    assert!(e.contains("is not a take of these watches"), "{e}");
    assert!(outside.join("take_part01-001.flac").is_file() && beside.exists());
    assert!(!is_positions_test(&t));
    fs::write(t.join(POSITIONS_TEST_FILE), "{}").unwrap();
    assert!(is_positions_test(&t));
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&elsewhere);
}

/// Two programs adding a take of one name to one watch at once (the app,
/// and the command line adding while the app is open) each put theirs
/// together in a folder of its own, and the second in gets the next free
/// name, so neither clears or overwrites the other's (Vernon on #1487).
#[test]
fn two_adds_of_one_name_never_meet() {
    let takes = std::env::temp_dir().join(format!("tg-two-adds-{}", std::process::id()));
    let _ = fs::remove_dir_all(&takes);
    fs::create_dir_all(&takes).unwrap();
    let to = unused_dir(&takes, "2026-11-12 10.00 Dial Up");
    let (a, b) = (staging(&takes, &to), staging(&takes, &to));
    assert_ne!(a, b);
    for (s, who) in [(&a, "app"), (&b, "cli")] {
        fs::create_dir_all(s).unwrap();
        fs::write(s.join("session.toml"), who).unwrap();
    }
    let first = move_in(&a, &takes, "2026-11-12 10.00 Dial Up", to.clone()).unwrap();
    let second = move_in(&b, &takes, "2026-11-12 10.00 Dial Up", to.clone()).unwrap();
    assert_eq!(first, to);
    assert_eq!(second, takes.join("2026-11-12 10.00 Dial Up 2"));
    assert_eq!(
        fs::read_to_string(first.join("session.toml")).unwrap(),
        "app"
    );
    assert_eq!(
        fs::read_to_string(second.join("session.toml")).unwrap(),
        "cli"
    );
    let _ = fs::remove_dir_all(&takes);
}

/// A picked calibre's figures are the watch's, and only a figure it
/// doesn't give is set by hand (#1555: the app's form and `watches edit`
/// both take this rule).
#[test]
fn a_calibre_gives_its_lift_and_beat_rate_and_only_the_rest_is_set_by_hand() {
    let mut w = Watch::new("Pepsi");
    w.lift = Some(50.0);
    w.bph = Some(21_600);
    let both = FromCalibre {
        lift: Some(52.0),
        bph: Some(28_800),
    };
    w.pick_calibre(" ETA 2824-2 ", both);
    assert_eq!(w.calibre.as_deref(), Some("ETA 2824-2"));
    assert_eq!((w.lift, w.bph), (Some(52.0), Some(28_800)));
    let e = w.set_lift(49.0, both).unwrap_err();
    assert!(e.contains("calibre gives the lift angle (52°)"), "{e}");
    let e = w.set_bph(Some(21_600), both).unwrap_err();
    assert!(e.contains("calibre gives the beat rate (28800 bph)"), "{e}");
    assert_eq!((w.lift, w.bph), (Some(52.0), Some(28_800)));
    // A calibre giving only its beat rate leaves the lift angle the
    // watch's own, to set by hand, in range.
    let bph_only = FromCalibre {
        lift: None,
        bph: Some(18_000),
    };
    w.pick_calibre("Unitas 6497", bph_only);
    assert_eq!((w.lift, w.bph), (Some(52.0), Some(18_000)));
    w.set_lift(44.0, bph_only).unwrap();
    assert_eq!(w.lift, Some(44.0));
    assert!(w.set_lift(120.0, bph_only).is_err());
    assert_eq!(w.lift, Some(44.0));
    // No calibre: both are the watch's, and the beat rate can go back
    // to being found from the sound.
    w.pick_calibre("", FromCalibre::default());
    assert!(w.calibre.is_none());
    w.set_bph(None, FromCalibre::default()).unwrap();
    assert_eq!((w.lift, w.bph), (Some(44.0), None));
    // Figures set by hand give way to a calibre picked after them.
    w.hold_calibre(both);
    assert_eq!((w.lift, w.bph), (Some(52.0), Some(28_800)));
}

/// The engine a kept reading names is this build's version as it is and
/// the first 8 hex digits of core's source hash (#1521).
#[test]
fn the_engine_is_this_versions_and_a_short_source_hash() {
    let (version, source) = super::engine();
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
    assert_eq!(source.len(), 8, "{source}");
    assert!(source.chars().all(|c| c.is_ascii_hexdigit()), "{source}");
    assert!(super::ENGINE.starts_with(&format!("{version}+{source}")));
}
