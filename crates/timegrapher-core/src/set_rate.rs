//! Where a set beat rate came from (#2224): one rule for every document
//! that gives `bph_set`.

use crate::analysis::AnalysisConfig;
use crate::take::{Calibre, SettingFrom};

/// Where a set beat rate came from (`bph_set_from`, Max on #2224): given
/// for this reading as one of [`crate::beats::STANDARD_BPH`] or typed as
/// any other, the calibre's table row, or the take's session file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BphFrom {
    Standard,
    Typed,
    Calibre,
    Session,
}

impl BphFrom {
    /// Where the set rate came from, by [`SettingFrom::of`]: `given` for
    /// this reading, else what a session file `kept`, else a calibre's
    /// `table` rate. `None` when none sets it, so the rate is heard (Auto).
    pub fn of(given: Option<u32>, kept: Option<u32>, table: Option<u32>) -> Option<Self> {
        match SettingFrom::of(given, kept, table) {
            SettingFrom::Given => Some(match given {
                Some(b) if crate::beats::STANDARD_BPH.contains(&b) => Self::Standard,
                _ => Self::Typed,
            }),
            SettingFrom::Take => Some(Self::Session),
            SettingFrom::Calibre => Some(Self::Calibre),
            SettingFrom::Default => None,
        }
    }
}

/// Whether `bph` lies in [`crate::input_domain::BPH_RANGE`].
pub fn in_range(bph: u32) -> bool {
    crate::input_domain::bph(u64::from(bph)).is_ok()
}

/// The beat rate `calibre` gives a reading: its row's, when in range. One
/// outside it (the table's IWC 9520 at 8,000 bph, or a custom calibre) is
/// set aside, so the reading hears the rate (Norm on #2321).
pub fn calibre_rate(calibre: &Calibre) -> Option<u32> {
    calibre.bph.filter(|b| in_range(*b))
}

/// Why `calibre`'s beat rate is not read at, when it is set aside.
pub fn set_aside_why(calibre: Option<&Calibre>) -> Option<String> {
    let c = calibre?;
    let b = c.bph.filter(|b| !in_range(*b))?;
    Some(crate::input_domain::bph_set_aside_text(
        &format!("the {}'s", c.name),
        b,
    ))
}

/// How a take is read, said after a rate set aside: on Auto with no
/// `bph`, else at the rate and where it came from (`from`).
pub fn read_at_text(bph: Option<u32>, from: Option<BphFrom>) -> String {
    match (bph, from) {
        (None, _) => "so this take is read on Auto".into(),
        (Some(b), Some(BphFrom::Calibre)) => {
            format!("so this take is read at the calibre's {b} bph")
        }
        (Some(b), Some(BphFrom::Session)) => {
            format!("so this take is read at the session file's {b} bph")
        }
        (Some(b), _) => format!("so this take is read at {b} bph"),
    }
}

/// The beat rate a session's header gives for reading `r` (#2432): the
/// rate set, "17280 bph"; on Auto, the standard rate the lock was read at,
/// "Auto, 28800 bph found", or, where the lock was judged off every
/// standard rate ([`crate::measure::Caveat::OffStandardRate`]), the beat
/// rate heard, "Auto, 17571 bph heard"; "Auto" alone with no watch heard.
pub fn header_text(r: &crate::session::Reading) -> String {
    if let Some(b) = r.bph_set {
        return format!("{b} bph");
    }
    if let Some(o) = crate::measure::off_standard(&r.measurement.caveats) {
        let heard = crate::figure_text::beat_rate_heard_text(o.bph_measured);
        return format!("Auto, {heard} heard");
    }
    if r.measurement.watch_heard {
        format!("Auto, {} bph found", r.bph)
    } else {
        "Auto".into()
    }
}

/// What a session file whose calibre's rate is set aside says
/// ([`set_aside_why`]): its takes are read on Auto, unless a rate is set.
pub fn session_set_aside_text(calibre: Option<&Calibre>, bph: Option<u32>) -> Option<String> {
    let why = set_aside_why(calibre).filter(|_| bph.is_none())?;
    Some(format!("{why}, so the session's takes are read on Auto"))
}

/// The top-level key in a take's `session.json` that says a watch the
/// take was moved into set its beat rate in `session.toml` (#1283), so
/// that rate stands although the take was recorded on Auto (Peter,
/// 26 Nov 2026, on #2357). Older builds ignore it.
pub const WATCH_RATE_KEY: &str = "bph_from_watch";

/// Whether the take in `dir` is read on Auto as it was recorded: the
/// recorder's `session.json` names no beat rate set by hand (its `bph`,
/// from [`crate::recorder::SessionInfo`], is null), and no watch it was
/// moved into set one since ([`WATCH_RATE_KEY`]). A take with no
/// `session.json`, one that can't be read, or one the recorder did not
/// write (no lift angle) is not, nor is one with no `bph` key: only a
/// `bph` written null says Auto. Read by
/// [`crate::recorder::recorded_setup`], the file's one reader.
pub fn recorded_on_auto(dir: &std::path::Path) -> bool {
    crate::recorder::recorded_setup(dir).is_some_and(|r| r.on_auto)
}

/// Mark the take in `dir` as one whose beat rate a watch set
/// ([`WATCH_RATE_KEY`]): written into its `session.json` as the first
/// key, the rest of the file as it was. A take with no `session.json`, or
/// one already marked, is left as it is; one that isn't a JSON object is
/// refused. Whether it was marked now.
pub fn mark_watch_rate(dir: &std::path::Path) -> Result<bool, String> {
    use serde_json::Value;
    let file = dir.join(crate::take::RECORD);
    let text = match crate::recorder::read_settled(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("{}: {e}", file.display())),
    };
    let bad = || {
        format!(
            "{} is not a session file this build can read",
            file.display()
        )
    };
    let v: Value = serde_json::from_str(&text).map_err(|_| bad())?;
    let o = v.as_object().ok_or_else(bad)?;
    if o.get(WATCH_RATE_KEY) == Some(&Value::Bool(true)) {
        return Ok(false);
    }
    let key = format!("\"{WATCH_RATE_KEY}\": true");
    let open = text.find('{').ok_or_else(bad)?;
    let (head, rest) = text.split_at(open + 1);
    let marked = if o.contains_key(WATCH_RATE_KEY) {
        let mut o = o.clone();
        o.insert(WATCH_RATE_KEY.into(), Value::Bool(true));
        serde_json::to_string_pretty(&o).map_err(|e| e.to_string())?
    } else if o.is_empty() {
        format!("{{{key}}}\n")
    } else {
        format!("{head}\n  {key},{rest}")
    };
    crate::library::write_atomic(&file, marked.as_bytes())?;
    Ok(true)
}

/// The take in `dir` read as it was recorded (Peter, 26 Nov 2026, on
/// #2357): where `a`'s beat rate is the session file's or the calibre's,
/// not given for this reading, and the take was recorded on Auto
/// ([`recorded_on_auto`]), on Auto. Its session file's `bph` was then the
/// rate Auto guessed, never one set, and the calibre's rate was not the
/// one chosen. A rate given for the reading stands. The one rule for
/// every reader of a take's entries ([`crate::take::open_entry`]) and
/// for what a reading says it was read at.
pub fn read_as_recorded(dir: &std::path::Path, a: &mut AnalysisConfig) {
    let kept = matches!(a.bph_from, Some(BphFrom::Session | BphFrom::Calibre));
    if kept && recorded_on_auto(dir) {
        (a.bph, a.bph_from) = (None, None);
    }
}

/// [`read_as_recorded`] for an entry's settings `s`.
pub fn as_recorded(
    dir: &std::path::Path,
    s: &crate::take::EntrySettings,
) -> crate::take::EntrySettings {
    let mut stream = s.stream.clone();
    read_as_recorded(dir, &mut stream.analysis);
    crate::take::EntrySettings { stream, ..*s }
}

/// Set `a`'s beat rate and where it came from: `given` for this reading,
/// else what the session file `kept`, else the calibre's
/// ([`calibre_rate`]); none means it is heard.
pub fn set(a: &mut AnalysisConfig, given: Option<u32>, kept: Option<u32>, cal: Option<&Calibre>) {
    let table = cal.and_then(calibre_rate);
    a.bph = given.or(kept).or(table);
    a.bph_from = BphFrom::of(given, kept, table);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Manifest;
    use crate::take::{stream_settings, Given};

    /// Where a set rate came from: given (a standard rate or a typed
    /// one), the session file, the calibre, or none on Auto (#2224).
    #[test]
    fn a_set_rate_says_where_it_came_from() {
        use BphFrom::{Session, Standard, Typed};
        assert_eq!(BphFrom::of(Some(28_800), None, None), Some(Standard));
        assert_eq!(BphFrom::of(Some(17_570), None, None), Some(Typed));
        assert_eq!(
            BphFrom::of(Some(17_570), Some(18_000), Some(21_600)),
            Some(Typed)
        );
        assert_eq!(
            BphFrom::of(Some(18_000), Some(18_000), None),
            Some(Standard)
        );
        assert_eq!(BphFrom::of(None, Some(17_570), Some(21_600)), Some(Session));
        assert_eq!(
            BphFrom::of(None, None, Some(21_600)),
            Some(BphFrom::Calibre)
        );
        assert_eq!(BphFrom::of(None, None, None), None);
        for b in crate::beats::STANDARD_BPH {
            assert_eq!(BphFrom::of(Some(b), None, None), Some(Standard), "{b}");
        }
        let m: Manifest = toml::from_str("bph = 17570").unwrap();
        let cal = |bph| Calibre {
            name: "X".into(),
            bph,
            lift_angle_deg: None,
            lift_set_aside_deg: None,
            escape_teeth: None,
            escapement: None,
            wheels: Vec::new(),
            meshes: Vec::new(),
        };
        let set = |m: &Manifest, c: Option<&Calibre>, bph| {
            let a = stream_settings(
                m,
                c,
                Given {
                    bph,
                    lift_deg: None,
                },
            )
            .analysis;
            (a.bph, a.bph_from)
        };
        assert_eq!(set(&m, None, None), (Some(17_570), Some(Session)));
        assert_eq!(set(&m, None, Some(28_800)), (Some(28_800), Some(Standard)));
        let none = Manifest::default();
        assert_eq!(
            set(&none, Some(&cal(Some(21_600))), None),
            (Some(21_600), Some(BphFrom::Calibre))
        );
        assert_eq!(set(&none, Some(&cal(None)), None), (None, None));
    }

    /// A calibre whose rate is outside the range, the table's IWC 9520 at
    /// 8,000 bph or a custom one at 10,800, gives no rate: the reading
    /// hears it, and says why (Norm on #2321).
    #[test]
    fn a_calibre_rate_outside_the_range_is_set_aside() {
        use crate::calibre_store::{CustomCalibre, UserCalibres};
        let mut user = UserCalibres::default();
        user.put_custom(CustomCalibre {
            name: "Old clock".into(),
            maker: None,
            aliases: Vec::new(),
            bph: Some(10_800),
            lift_angle_deg: None,
            escape_teeth: None,
            escapement: None,
            wheels: Vec::new(),
            notes: None,
        });
        for (name, b) in [("IWC 9520", 8_000), ("Old clock", 10_800)] {
            let c = crate::take::calibre_named(name, &user).unwrap();
            assert_eq!(c.bph, Some(b), "{name}: the row keeps its rate");
            assert_eq!(calibre_rate(&c), None, "{name}");
            assert_eq!(
                set_aside_why(Some(&c)).unwrap(),
                format!(
                    "the {name}'s beat rate of {b} bph is set aside, outside 12000 to 72000 bph"
                )
            );
            let a = stream_settings(&Manifest::default(), Some(&c), Given::default()).analysis;
            assert_eq!((a.bph, a.bph_from), (None, None), "{name}");
            assert_eq!(
                session_set_aside_text(Some(&c), None).unwrap(),
                format!(
                    "the {name}'s beat rate of {b} bph is set aside, outside 12000 to 72000 bph, \
                     so the session's takes are read on Auto"
                )
            );
            assert_eq!(session_set_aside_text(Some(&c), Some(18_000)), None);
        }
        let c = crate::take::calibre_named("IWC 9520 (18,000 bph, 40°)", &user).unwrap();
        assert_eq!(
            (calibre_rate(&c), set_aside_why(Some(&c))),
            (Some(18_000), None)
        );
        assert_eq!(read_at_text(None, None), "so this take is read on Auto");
        assert_eq!(
            read_at_text(Some(21_600), Some(BphFrom::Calibre)),
            "so this take is read at the calibre's 21600 bph"
        );
        assert_eq!(
            read_at_text(Some(18_000), Some(BphFrom::Session)),
            "so this take is read at the session file's 18000 bph"
        );
    }
}
