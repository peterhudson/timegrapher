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
/// new lift: the whole take, the Tick Tock profiles and the Positions
/// Test table scale with it.
#[test]
fn a_new_lift_rescales_every_amplitude() {
    let mut app = synthetic(60.0, 12.0);
    let ctx = egui::Context::default();
    let table = |app: &TimegrapherApp| app.free_table(&ctx).expect("a table").mean[1].unwrap();
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
    let (w0, p0, t0) = (whole(&app), profiles(&app), table(&app));
    assert!(!p0.is_empty());
    let k = 55.0 / app.lift_deg;
    app.set_lift(55.0);
    let w1 = whole(&app);
    assert!((w1 - w0 * k).abs() < 0.05, "{w0} then {w1}");
    for (a, b) in p0.iter().zip(profiles(&app)) {
        assert!((b - a * k).abs() < 1e-9, "{a} then {b}");
    }
    let t1 = table(&app);
    assert!((t1 - t0 * k).abs() < 0.05, "{t0} then {t1}");
}

#[test]
fn persisted_lift_context_roundtrips_without_guessing_legacy_history() {
    for (text, expected) in [
        ("lift = 52\nlift_known = false", false),
        ("lift = 52\nlift_known = true", true),
        ("lift = 49\nlift_known = false", false),
        ("lift = 52", false), // Assumed, as core reads it (#685).
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
        app.apply_manifest(&m, &[], None);
        assert_eq!(app.lift_known(), expected, "{text}");
        assert_eq!(app.live_config().analysis.lift_known, expected);
        let value = app.lift_deg;
        let setup = app.watch_setup();
        let saved = toml::to_string(&app.take_manifest()).unwrap();
        let again: Manifest = toml::from_str(&saved).unwrap();
        assert_eq!(again.lift, Some(value));
        assert_eq!(again.lift_known, Some(expected));
        let mut reopened = TimegrapherApp::with_devices(Vec::new(), None);
        reopened.apply_manifest(&again, &[], None);
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

/// An older session file's lift is read by core's one rule, from the
/// take's own calibre: a calibre picked before the take was opened does
/// not make its default 52 known (#685).
#[test]
fn an_older_takes_lift_is_read_by_core_rule_not_the_calibre_on_show() {
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.set_calibre("Unitas 6497-2".into());
    assert!(app.lift_known());
    let m: Manifest = toml::from_str("lift = 52").unwrap();
    app.apply_manifest(&m, &[], None);
    assert_eq!(app.calibre, "Unitas 6497-2");
    assert!(!app.lift_known());
    let cal = timegrapher_core::take::calibre_named(&app.calibre, &app.cal.user);
    assert!(cal.as_ref().is_some_and(|c| c.lift_angle_deg.is_some()));
    assert_eq!(
        timegrapher_core::take::file_lift_known(&m, None),
        Some(false)
    );
}

/// Keep Analysis Only reads whether a take's lift was supplied as core
/// does: an older file's default 52 is assumed unless its calibre gives
/// a lift, any other value was supplied, and a marker wins (#685).
#[test]
fn keep_analysis_only_reads_the_lift_by_core_rule() {
    let app = TimegrapherApp::with_devices(Vec::new(), None);
    for (text, expected) in [
        ("lift = 52", false),
        ("lift = 49", true),
        ("lift = 52\nlift_known = true", true),
        ("lift = 49\nlift_known = false", false),
        ("calibre = \"Unitas 6497-2\"\nlift = 52", true),
        ("calibre = \"Seagull ST36\"\nlift = 52", false),
        ("lift_known = true", false), // No accompanying value.
    ] {
        let m: Manifest = toml::from_str(text).unwrap();
        let cfg = app.keep_config(&m);
        let cal = m
            .calibre
            .as_deref()
            .and_then(|n| timegrapher_core::take::calibre_named(n, &app.cal.user));
        let core = timegrapher_core::take::file_lift_known(&m, cal.as_ref());
        assert_eq!(cfg.analysis.lift_known, expected, "{text}");
        assert_eq!(core.unwrap_or(false), expected, "{text}");
    }
}

/// Lift changed after a guided test has measured reads its stretches
/// again at the new lift, as every other view is: the Positions,
/// Complication and Dynamic Poise tests' amplitudes scale with it, so
/// the table, its Copy and the take's lift agree (#1645, from an
/// external audit).
#[test]
fn a_new_lift_rereads_every_guided_tests_amplitudes() {
    let each = 20.0;
    for kind in [
        guided::Kind::Positions,
        guided::Kind::Complication,
        guided::Kind::Poise,
    ] {
        let mut app = synthetic(8.0 * each + 10.0, 0.0);
        let mut test = match kind {
            guided::Kind::Complication => guided::Test::complication(app.position),
            guided::Kind::Poise => guided::Test::poise(),
            _ => guided::Test::new(),
        }
        .timed(2.0, each - 2.0);
        let mut t = 1.0;
        test.tick(t);
        for _ in 0..8 {
            t += each;
            test.tick(t);
            if test.advance(t).is_none() {
                break;
            }
        }
        let log = app.live.as_ref().and_then(steady::log_of).unwrap();
        test.finish(&log, None, &app.masked);
        app.guided = Some(test);
        // A stretch whose amplitude core withholds stays withheld.
        let amps = |app: &TimegrapherApp| -> Vec<Option<f64>> {
            let readings = app.guided.as_ref().unwrap().readings.as_ref().unwrap();
            readings
                .iter()
                .map(|r| r.measurement.amplitude_deg)
                .collect()
        };
        let before = amps(&app);
        let read = before.iter().flatten().count();
        assert!(read >= 2, "{kind:?}: {before:?}");
        let k = 60.0 / app.lift_deg;
        app.set_lift(60.0);
        let after = amps(&app);
        assert_eq!(after.len(), before.len(), "{kind:?}");
        for (a, b) in before.iter().zip(&after) {
            match (a, b) {
                (Some(a), Some(b)) => assert!((b - a * k).abs() < 0.05, "{kind:?}: {a} then {b}"),
                _ => assert_eq!(a.is_some(), b.is_some(), "{kind:?}: {before:?} {after:?}"),
            }
        }
    }
}
