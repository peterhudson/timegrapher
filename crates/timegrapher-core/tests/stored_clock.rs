//! The correction stored for the input a recording was made on, read from
//! the user's store. One test, because it sets `TIMEGRAPHER_CONFIG_DIR`
//! for the whole process.

use std::fs;
use timegrapher_core::clockstore::{sidecar_path, ClockStore, DeviceClock};
use timegrapher_core::take;

#[test]
fn a_recording_takes_the_correction_stored_for_its_input() {
    let root = std::env::temp_dir().join(format!("tg-stored-clock-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let config = root.join("config");
    let takes = root.join("takes");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(&takes).unwrap();
    std::env::set_var("TIMEGRAPHER_CONFIG_DIR", &config);
    let mut store = ClockStore::default();
    store.set(DeviceClock {
        device: "USB Mic".into(),
        ppm: 12.5,
        ppm_sd: Some(0.1),
        span_s: 600.0,
        measured_utc: "2026-10-10T03:00:00Z".into(),
        source: "set".into(),
    });
    store.save_to(&config.join("clocks.json")).unwrap();

    let file = takes.join("take.wav");
    // A recording that names no input has no stored correction.
    assert_eq!(take::stored_correction(&file).unwrap(), None);
    assert_eq!(take::correction(&file, None, None).unwrap(), None);

    fs::write(
        sidecar_path(&file),
        r#"{"recording": {"source": {"device": {"name": "USB Mic"}}}}"#,
    )
    .unwrap();
    let stored = take::stored_correction(&file).unwrap().unwrap();
    assert_eq!((stored.device.as_str(), stored.ppm), ("USB Mic", 12.5));
    assert_eq!(
        take::correction(&file, None, None).unwrap(),
        Some((12.5, "stored for 'USB Mic', 2026-10-10T03:00:00Z".into()))
    );
    // A given ppm comes first, then a named input.
    assert_eq!(
        take::correction(&file, Some("USB Mic"), Some(3.0)).unwrap(),
        Some((3.0, "given with --card-ppm".into()))
    );
    assert!(take::correction(&file, Some("Other"), None).is_err());

    // An input with nothing stored for it has no correction.
    fs::write(
        sidecar_path(&file),
        r#"{"recording": {"source": {"device": {"name": "Built-in"}}}}"#,
    )
    .unwrap();
    assert_eq!(take::stored_correction(&file).unwrap(), None);
    let _ = fs::remove_dir_all(&root);
}
