//! The lift angle, headless: where it comes from, what changing it
//! does to the amplitudes, and how a take keeps it.

use super::tests::synthetic;
use super::*;

/// The lift angle is typed only with no calibre setting it, in Quick
/// Listen too.
#[test]
fn a_calibre_picked_sets_the_lift_angle() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    assert!(app.open_watch().is_none());
    assert!(app.lift_settable());
    app.set_calibre("ETA 2824-2".into());
    assert!(app.cal.lift("ETA 2824-2").is_some());
    assert!(!app.lift_settable());
    app.set_calibre(String::new());
    assert!(app.lift_settable());
}

/// Lift changed on a stopped take reads every amplitude again at the
/// new lift: the whole take and the Tick Tock profiles scale with it,
/// and the Positions Test table is read afresh.
#[test]
fn a_new_lift_rescales_every_amplitude() {
    let mut app = synthetic(60.0, 12.0);
    let whole = |app: &TimegrapherApp| app.whole_take().unwrap().m.amplitude_deg.unwrap();
    let profiles = |app: &TimegrapherApp| -> Vec<f64> {
        app.live
            .as_ref()
            .unwrap()
            .tick_profiles()
            .iter()
            .flatten()
            .filter_map(|p| p.amplitude_deg)
            .collect()
    };
    let (w0, p0) = (whole(&app), profiles(&app));
    assert!(!p0.is_empty());
    *app.free_table.borrow_mut() = Some(((0, Vec::new(), Vec::new(), None), None));
    let k = 55.0 / app.lift_deg;
    app.set_lift(55.0);
    let w1 = whole(&app);
    assert!((w1 - w0 * k).abs() < 0.05, "{w0} then {w1}");
    for (a, b) in p0.iter().zip(profiles(&app)) {
        assert!((b - a * k).abs() < 1e-9, "{a} then {b}");
    }
    assert!(app.free_table.borrow().is_none());
}

#[test]
fn persisted_lift_context_roundtrips_without_guessing_legacy_history() {
    for (text, expected) in [
        ("lift = 52\nlift_known = false", false),
        ("lift = 52\nlift_known = true", true),
        ("lift = 49\nlift_known = false", false),
        ("lift = 52", false), // Legacy app inference, unlike legacy CLI.
        ("lift = 49", true),
        ("lift_known = true", false), // No accompanying value.
        ("calibre = \"Seagull ST36\"", false),
        ("calibre = \"Unitas 6497-2\"", true),
        (
            "calibre = \"Unitas 6497-2\"\nlift = 52\nlift_known = false",
            false,
        ),
    ] {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let m: Manifest = toml::from_str(text).unwrap();
        app.apply_manifest(&m, &[]);
        assert_eq!(app.lift_known(), expected, "{text}");
        assert_eq!(app.live_config().analysis.lift_known, expected);
        let value = app.lift_deg;
        let setup = app.watch_setup();
        let saved = toml::to_string(&app.take_manifest()).unwrap();
        let again: Manifest = toml::from_str(&saved).unwrap();
        assert_eq!(again.lift, Some(value));
        assert_eq!(again.lift_known, Some(expected));
        let mut reopened = TimegrapherApp::with_devices(Vec::new(), None);
        reopened.apply_manifest(&again, &[]);
        assert_eq!(
            (reopened.lift_deg, reopened.lift_known()),
            (value, expected)
        );
        reopened.set_watch_setup(WatchSetup::default());
        reopened.set_watch_setup(setup);
        assert_eq!(
            (reopened.lift_deg, reopened.lift_known()),
            (value, expected)
        );
        reopened.set_lift(value + 1.0);
        reopened.set_lift(value);
        assert_eq!(
            reopened.lift_known(),
            expected,
            "numeric revert retains context"
        );
    }
}
