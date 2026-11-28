//! A take's lift angle: whether its session file's was supplied, the
//! one rule every reader reads it by (#685, #2226), and a kept log read
//! at a lift ([`kept_at_lift`]). Split from `take.rs` by concern.

use super::*;

/// Whether the lift a session file `m` gives was supplied, or `None`
/// when it gives none: its `lift_known` where it says, else (an older
/// file, which did not record it) supplied unless it is the default
/// [`DEFAULT_LIFT_DEG`] and the file's own calibre gives no lift (#685).
/// Only the file's own `calibre =` line counts (#2234): the recorder knew
/// that calibre's lift and would have written it, so a 52 beside it was
/// typed; a calibre named only on the call (`--calibre`, MCP `calibre`)
/// was never known to the recorder. `cal` is the calibre this reading
/// resolved; it stands for the file's own where it is the one the file
/// names, else the file's line is looked up in the table. Core, the CLI
/// and the desktop take it from here.
pub fn file_lift_known(m: &Manifest, cal: Option<&Calibre>) -> Option<bool> {
    let lift = m.lift?;
    Some(
        m.lift_known
            .unwrap_or(lift != DEFAULT_LIFT_DEG || own_calibre_gives_lift(m, cal)),
    )
}

/// Whether the calibre the session file `m` names gives a lift (#2234):
/// `cal`'s where `cal` is that calibre ([`is_files_own`]), else the
/// shipped table's row for the name; false where the file names none.
fn own_calibre_gives_lift(m: &Manifest, cal: Option<&Calibre>) -> bool {
    use crate::calibre_store::Entry;
    let Some(named) = m
        .calibre
        .as_deref()
        .filter(|n| !crate::calibres::key(n).is_empty())
    else {
        return false;
    };
    let row = crate::calibres::find(named);
    match cal.filter(|c| is_files_own(named, row, c)) {
        Some(c) => c.lift_angle_deg.is_some(),
        None => row.is_some_and(|r| Entry::Table(r).lift_angle_deg().is_some()),
    }
}

/// Whether `cal` is the calibre a file's line `named` (whose table row is
/// `row`) names: it has that name; or the table resolves the name to it;
/// or, where the table does not know the name, `cal` is no table
/// calibre, so it can only be the user's calibre the line names by one
/// of its aliases (core has no user catalogue here).
fn is_files_own(named: &str, row: Option<&crate::calibres::Calibre>, cal: &Calibre) -> bool {
    use crate::calibres::{find, key};
    let k = key(&cal.name);
    if k == key(named) {
        return true;
    }
    match row {
        Some(r) => key(&r.calibre) == k,
        None => find(&cal.name).is_none_or(|t| key(&t.calibre) != k),
    }
}

/// A take's lift angle, whether it was supplied, and where it came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiftSetting {
    pub lift_deg: f64,
    pub known: bool,
    pub from: SettingFrom,
}

/// The one rule for a take's lift angle (#2226), which core, the CLI and
/// the desktop's take opening ask (#2232): the lift `given` for this command, else the session
/// file `m`'s where [`file_lift_known`] says it was supplied, else its
/// calibre `cal`'s, else the lift the session file assumed, else
/// [`DEFAULT_LIFT_DEG`]. Only the last two are not known: an assumed lift
/// never outranks a calibre's.
pub fn lift_setting(
    given: Option<f64>,
    m: Option<&Manifest>,
    cal: Option<&Calibre>,
) -> LiftSetting {
    let set = |lift_deg, known, from| LiftSetting {
        lift_deg,
        known,
        from,
    };
    let kept = m.and_then(|m| m.lift);
    let supplied = m.and_then(|m| file_lift_known(m, cal)) == Some(true);
    match (given, kept, cal.and_then(|c| c.lift_angle_deg)) {
        (Some(l), ..) => set(l, true, SettingFrom::Given),
        (None, Some(l), _) if supplied => set(l, true, SettingFrom::Take),
        (None, _, Some(l)) => set(l, true, SettingFrom::Calibre),
        (None, Some(l), None) => set(l, false, SettingFrom::Take),
        (None, None, None) => set(DEFAULT_LIFT_DEG, false, SettingFrom::Default),
    }
}

/// A kept take's beat log with its amplitudes at `lift_deg`, the lift this
/// reading uses, where the analysis was kept at another: each window's
/// sides are read again from their edges at the new lift and gated there
/// ([`crate::amplitude::at_lift`]), so a side the kept lift culled can
/// come back (#1333), then screened for lone wrong-sound sides once over
/// the log ([`crate::amplitude::reject_side_outliers`]), as a read at
/// that lift is. Read for `escapement` (#2313,
/// [`crate::amplitude::kept_for`]), so a log kept for either escapement
/// reads as one read for this one.
pub fn kept_at_lift(
    log: &BeatLog,
    lift_deg: f64,
    escapement: crate::escapement::Escapement,
) -> BeatLog {
    let mut log = log.clone();
    if lift_deg > 0.0 && log.lift_deg > 0.0 && lift_deg != log.lift_deg && log.bph > 0 {
        // The balance's own period, as the windows were read (#2228).
        let osc = 2.0 * crate::timing::own_beat_period_s(&log.beats, log.bph);
        crate::amplitude::relift(
            &mut log.amplitude_windows,
            osc,
            log.lift_deg,
            lift_deg,
            escapement,
        );
        log.lift_deg = lift_deg;
    } else {
        crate::amplitude::kept_for(&mut log.amplitude_windows, escapement);
    }
    log.escapement = escapement;
    log
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A take's assumed lift gives way to its calibre's, and one it was
    /// given doesn't (#2226).
    #[test]
    fn a_calibres_lift_outranks_a_takes_assumed_one() {
        let cal = Calibre {
            name: "Test".into(),
            bph: None,
            lift_angle_deg: Some(59.0),
            lift_set_aside_deg: None,
            escape_teeth: None,
            escapement: None,
            wheels: Vec::new(),
            meshes: Vec::new(),
        };
        let file = |text: &str| -> Manifest { toml::from_str(text).unwrap() };
        let read = |m: &Manifest| {
            let cfg = stream_settings(m, Some(&cal), Given::default());
            let lift = lift_setting(None, Some(m), Some(&cal));
            assert_eq!(
                (cfg.analysis.amplitude.lift_deg, cfg.analysis.lift_known),
                (lift.lift_deg, lift.known)
            );
            (lift.lift_deg, lift.known, lift.from)
        };
        let assumed = file("lift = 52\nlift_known = false");
        assert_eq!(read(&assumed), (59.0, true, SettingFrom::Calibre));
        let supplied = file("lift = 52\nlift_known = true");
        assert_eq!(read(&supplied), (52.0, true, SettingFrom::Take));
        // With no calibre lift, the assumed one stands, unknown.
        assert_eq!(
            lift_setting(None, Some(&assumed), None),
            LiftSetting {
                lift_deg: 52.0,
                known: false,
                from: SettingFrom::Take
            }
        );
        assert_eq!(
            lift_setting(Some(48.0), Some(&assumed), Some(&cal)).from,
            SettingFrom::Given
        );
        assert_eq!(
            lift_setting(None, None, None),
            LiftSetting {
                lift_deg: DEFAULT_LIFT_DEG,
                known: false,
                from: SettingFrom::Default
            }
        );
    }

    /// An older file's bare 52 is typed only beside its own calibre line
    /// that gives a lift; beside a calibre named only on the call it is
    /// the default it was recorded with, and that calibre's lift wins
    /// (#2234).
    #[test]
    fn an_older_files_bare_52_is_typed_only_beside_its_own_calibre() {
        let cal = |name: &str, lift| Calibre {
            name: name.into(),
            bph: None,
            lift_angle_deg: lift,
            lift_set_aside_deg: None,
            escape_teeth: None,
            escapement: None,
            wheels: Vec::new(),
            meshes: Vec::new(),
        };
        let file = |text: &str| -> Manifest { toml::from_str(text).unwrap() };
        let read = |m: &Manifest, c: &Calibre| {
            let l = lift_setting(None, Some(m), Some(c));
            (file_lift_known(m, Some(c)), l.lift_deg, l.known, l.from)
        };
        let x = cal("Test X", Some(59.0));
        let unitas = calibre_named("Unitas 6497-2", &Default::default()).unwrap();
        // No calibre line: the one on the call was never the recorder's.
        assert_eq!(
            read(&file("lift = 52"), &x),
            (Some(false), 59.0, true, SettingFrom::Calibre)
        );
        // Its own line, however it is spelt, names the calibre read.
        for own in ["calibre = \"Test X\"", "calibre = \"test-x\""] {
            assert_eq!(
                read(&file(&format!("{own}\nlift = 52")), &x),
                (Some(true), 52.0, true, SettingFrom::Take),
                "{own}"
            );
        }
        // Its own line names another calibre: that one's lift decides,
        // from the table, not the one on the call.
        let with_lift = crate::calibres::find("Unitas 6497-2").unwrap();
        assert!(with_lift.lift_angle_deg.is_some());
        let own_lift = file("calibre = \"Unitas 6497-2\"\nlift = 52");
        assert_eq!(
            read(&own_lift, &x),
            (Some(true), 52.0, true, SettingFrom::Take)
        );
        let no_lift = crate::calibres::find("Seagull ST36").unwrap();
        assert!(no_lift.lift_angle_deg.is_none());
        let own_none = file("calibre = \"Seagull ST36\"\nlift = 52");
        assert_eq!(
            read(&own_none, &x),
            (Some(false), 59.0, true, SettingFrom::Calibre)
        );
        // Its own line names the calibre read, which gives no lift.
        assert_eq!(
            read(
                &file("calibre = \"Test Y\"\nlift = 52"),
                &cal("Test Y", None)
            )
            .0,
            Some(false)
        );
        // Its own line is a name the table resolves: the calibre read is
        // that row, and its own lift (here none) decides, not the row's.
        let short = file("calibre = \"6497-2\"\nlift = 52");
        assert_eq!(
            crate::calibres::find("6497-2").map(|r| r.calibre.as_str()),
            Some("Unitas 6497-2")
        );
        assert_eq!(
            file_lift_known(&short, Some(&cal("Unitas 6497-2", None))),
            Some(false)
        );
        // A label the table does not know, read with a table calibre
        // named on the call: not the file's own.
        assert_eq!(
            read(&file("calibre = \"Unknown label\"\nlift = 52"), &unitas).0,
            Some(false)
        );
        // A marker still wins, and any other value was typed.
        assert_eq!(
            read(&file("lift = 52\nlift_known = true"), &x).0,
            Some(true)
        );
        assert_eq!(read(&file("lift = 50"), &x).0, Some(true));
    }

    /// A user's calibre the file's own line names by one of its aliases
    /// is the file's own: a bare 52 beside it was typed, as on main
    /// (#2234, Trevor 107's probe on #2540).
    #[test]
    fn a_users_calibre_named_by_its_alias_is_the_files_own() {
        use crate::calibre_store::{CustomCalibre, UserCalibres};
        let mut user = UserCalibres::default();
        user.put_custom(CustomCalibre {
            name: "Bench 44".into(),
            maker: None,
            aliases: vec!["Workshop-A".into()],
            bph: None,
            lift_angle_deg: Some(44.0),
            escape_teeth: None,
            escapement: None,
            wheels: Vec::new(),
            notes: None,
        });
        let m: Manifest = toml::from_str("calibre = \"Workshop-A\"\nlift = 52").unwrap();
        let cal = calibre_named(m.calibre.as_deref().unwrap(), &user).unwrap();
        assert_eq!(cal.name, "Bench 44");
        assert_eq!(file_lift_known(&m, Some(&cal)), Some(true));
        assert_eq!(
            lift_setting(None, Some(&m), Some(&cal)),
            LiftSetting {
                lift_deg: 52.0,
                known: true,
                from: SettingFrom::Take
            }
        );
    }
}
