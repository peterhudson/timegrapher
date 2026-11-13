//! The `calibres` command, and `--calibre` on `analyze`: the built-in
//! table of calibres (`timegrapher_core::calibres`) for people and agents,
//! so a beat rate and lift angle come from the table rather than being
//! copied by hand.

use crate::long::Train;
use crate::output;
use serde_json::json;
use timegrapher_core::calibre_store::{self, Catalog, Entry, Origin, UserCalibres};
use timegrapher_core::calibres::{self, Calibre};
use timegrapher_core::library::fixed_text;
use timegrapher_core::periodicity::{Component, Wheel};
use timegrapher_core::steadiness;

/// Lift angle when neither `--lift` nor the calibre gives one, degrees.
pub use timegrapher_core::take::DEFAULT_LIFT_DEG;
/// Escape wheel teeth when neither `--escape-teeth` nor the calibre gives
/// a count.
pub use timegrapher_core::take::ESCAPE_TEETH as DEFAULT_ESCAPE_TEETH;

/// The settings `analyze`, `profile` and `shape` run with: an explicit
/// flag wins, then the take's session file ([`resolve_for`]), then the
/// calibre's figure, then the default.
pub struct Resolved {
    pub bph: Option<u32>,
    pub lift_deg: f64,
    /// The lift angle came from the flag or the calibre, not the default.
    pub lift_known: bool,
    pub escape_teeth: u32,
    pub(crate) train: crate::long::ResolvedTrain,
}

pub fn resolve(
    calibre: Option<&str>,
    bph: Option<u32>,
    lift_deg: Option<f64>,
    escape_teeth: Option<u32>,
) -> Result<Resolved, String> {
    let train = Train {
        calibre,
        escape_teeth,
    }
    .resolve()?;
    Ok(resolve_train(train, bph, lift_deg, escape_teeth))
}

/// The settings a recording `file` is read with: for a sound file its
/// folder's session file lists ([`timegrapher_core::take_path::take_of`]),
/// the take's, as `series`, `long` and the app read it
/// ([`timegrapher_core::take::stream_settings`]: a flag, then the session
/// file, then its calibre, then the default); for any other, or one
/// beside a session file that can't be read, [`resolve`]'s (#1573).
pub fn resolve_for(
    file: &std::path::Path,
    calibre: Option<&str>,
    bph: Option<u32>,
    lift_deg: Option<f64>,
    escape_teeth: Option<u32>,
) -> Result<Resolved, String> {
    // A session file that can't be read leaves the file's own reading
    // standing, as `analyze` says beside it (`session_cmd::take_of`).
    let listed = if file.is_dir() {
        None
    } else {
        timegrapher_core::take_path::take_of(file).ok().flatten()
    };
    let Some((m, _)) = listed else {
        return resolve(calibre, bph, lift_deg, escape_teeth);
    };
    let train = Train {
        calibre,
        escape_teeth,
    }
    .resolve_for_take(&m)?;
    let given = timegrapher_core::take::Given { bph, lift_deg };
    let set = timegrapher_core::take::stream_settings(&m, train.calibre.as_ref(), given).analysis;
    Ok(Resolved {
        bph: set.bph,
        lift_deg: set.amplitude.lift_deg,
        lift_known: set.lift_known,
        escape_teeth: escape_teeth.unwrap_or(set.escape_teeth),
        train,
    })
}

fn resolve_train(
    train: crate::long::ResolvedTrain,
    bph: Option<u32>,
    lift_deg: Option<f64>,
    escape_teeth: Option<u32>,
) -> Resolved {
    let c = train.calibre.as_ref();
    let lift = lift_deg.or(c.and_then(|c| c.lift_angle_deg));
    Resolved {
        bph: bph.or(c.and_then(|c| c.bph)),
        lift_deg: lift.unwrap_or(DEFAULT_LIFT_DEG),
        lift_known: lift.is_some(),
        escape_teeth: escape_teeth
            .or(c.and_then(|c| c.escape_teeth))
            .unwrap_or(DEFAULT_ESCAPE_TEETH),
        train,
    }
}

/// The wheels to name components after: the calibre's train, with the
/// escape wheel `--escape-teeth` gives in place of the table's.
#[cfg(test)]
fn wheels(train: &Train, bph: u32) -> Result<Vec<Wheel>, String> {
    Ok(train.resolve()?.analysis_wheels(bph))
}

/// Name each component after the first wheel whose turn is within 6% of
/// its period, as the period search does, or after none.
pub fn rename(components: &mut [Component], wheels: &[Wheel]) {
    for c in components {
        c.wheel = steadiness::wheel_for(c.period_s, wheels);
    }
}

/// `timegrapher calibres [NAME] [--search QUERY]`: every calibre, the
/// one called NAME with each wheel's period and where it comes from, or
/// those answering QUERY, best first. What the user added in the app
/// (`calibre_store::default_path`) is read over the table.
pub fn run(name: Option<&str>, search: Option<&str>, json: bool) -> Result<(), String> {
    let path = calibre_store::default_path();
    let user = match &path {
        Some(p) => UserCalibres::load(p)?,
        None => UserCalibres::default(),
    };
    let cat = Catalog::new(&user);
    let list: Vec<Entry> = match (name, search) {
        (Some(n), _) => vec![cat.find(n).ok_or_else(|| {
            format!("'{n}' is not in the table; `timegrapher calibres --search {n}` looks wider")
        })?],
        (None, Some(q)) => cat.search(q),
        // With neither, the common calibres and the user's own.
        (None, None) => cat
            .search("")
            .into_iter()
            .filter(|e| match e {
                Entry::Table(c) => c.common,
                Entry::Custom(_) => true,
            })
            .collect(),
    };
    let everything = calibres::all().len();
    if json {
        return print_json(&cat, &list, name, search, path.as_deref(), everything);
    }
    if name.is_some() {
        print_one(&cat, list[0]);
        return Ok(());
    }
    if list.is_empty() {
        println!("No calibre answers that; try fewer words, or the maker alone.");
        return Ok(());
    }
    print_table(
        &cat,
        &list,
        user.custom.len(),
        everything,
        name.is_none() && search.is_none(),
    );
    Ok(())
}

/// The calibres listed, as JSON: the table's and the user's apart, and a
/// named one's wheel slots.
fn print_json(
    cat: &Catalog,
    list: &[Entry],
    name: Option<&str>,
    search: Option<&str>,
    path: Option<&std::path::Path>,
    everything: usize,
) -> Result<(), String> {
    let table: Vec<&Calibre> = list
        .iter()
        .filter_map(|e| match e {
            Entry::Table(c) => Some(*c),
            Entry::Custom(_) => None,
        })
        .collect();
    let custom: Vec<_> = list
        .iter()
        .filter_map(|e| match e {
            Entry::Custom(c) => Some(*c),
            Entry::Table(_) => None,
        })
        .collect();
    let mut body = json!({
        "calibres": table,
        "custom": custom,
        "listed": if name.is_none() && search.is_none() { "common" } else { "matching" },
        "table_size": everything,
    });
    if name.is_some() {
        body["slots"] = json!(cat.slots(list[0]));
        body["train"] = json!(cat.train(list[0]));
    }
    output::print(
        "calibres",
        output::input(
            &[],
            json!({
                "name": name,
                "search": search,
                "user_calibres": path.as_ref().map(|p| p.display().to_string()),
            }),
        ),
        &body,
    )
}

/// The calibres listed as a table, with what its columns mean.
fn print_table(cat: &Catalog, list: &[Entry], yours: usize, everything: usize, common: bool) {
    println!(
        "{:<26} {:>7}  {:>5}  {:>6}  {:>6}  Also matches",
        "Calibre", "bph", "Lift", "Escape", "Wheels"
    );
    let dash = || "-".to_string();
    for e in list {
        let known = cat
            .slots(*e)
            .iter()
            .filter(|s| s.origin != Origin::Unknown)
            .count();
        let also = match e {
            Entry::Table(c) => c.family.join(", "),
            Entry::Custom(c) => {
                let mut v = vec!["(yours)".to_string()];
                v.extend(c.aliases.iter().cloned());
                v.join(", ")
            }
        };
        println!(
            "{:<26} {:>7}  {:>5}  {:>6}  {:>6}  {}",
            e.name(),
            e.bph().map(|b| b.to_string()).unwrap_or_else(dash),
            e.lift_angle_deg()
                .map(|l| format!("{}°", fixed_text(l, 0)))
                .unwrap_or_else(dash),
            e.escape_teeth().map(|t| t.to_string()).unwrap_or_else(dash),
            known,
            also
        );
    }
    if common {
        println!(
            "\nThe {} common calibres of the {everything} in the table, and yours. \
`timegrapher calibres --search WORDS` finds any of them, by number, maker or other name.",
            list.len() - yours
        );
    }
    println!(
        "\nLift: the lift angle; Escape: escape wheel teeth; Wheels: how many wheel periods \
are known; '-' where no source gives one. `timegrapher calibres NAME` shows one \
calibre's wheels and sources."
    );
}

fn print_one(cat: &Catalog, e: Entry) {
    match e.maker() {
        Some(m) => println!("{} ({m})", e.name()),
        None => println!("{}", e.name()),
    }
    let also = match e {
        Entry::Table(c) => c.family.clone(),
        Entry::Custom(c) => c.aliases.clone(),
    };
    if !also.is_empty() {
        println!("Also matches  {}", also.join(", "));
    }
    if e.is_custom() {
        println!("Added by you  (not in the shipped table)");
    }
    match e.bph() {
        Some(b) => println!("Beat rate     {b} bph"),
        None => println!("Beat rate     not given"),
    }
    match e.lift_angle_deg() {
        Some(l) => println!("Lift angle    {l}°"),
        None => println!("Lift angle    not published (analyze uses {DEFAULT_LIFT_DEG}°)"),
    }
    if let Some(t) = e.escape_teeth() {
        println!("Escape wheel  {t} teeth");
    }
    if let Entry::Table(c) = e {
        if let Some(s) = &c.seconds {
            println!("Seconds       {s}");
        }
    }
    println!("Wheels (one full turn):");
    for s in cat.slots(e) {
        let period = s
            .period_s
            .map(|p| format!("{:>10} s", fixed_text(p, 2)))
            .unwrap_or_else(|| format!("{:>12}", "unknown"));
        let from = match s.origin {
            Origin::Shipped => "table",
            Origin::User => "you",
            Origin::Counts => "counts",
            Origin::Unknown => "",
        };
        let mut line = format!("  {:<16} {period}  {from}", s.name);
        if let Some(t) = s.shipped_period_s {
            line.push_str(&format!(" (the table says {} s)", fixed_text(t, 2)));
        }
        println!("{}", line.trim_end());
    }
    if let Entry::Table(c) = e {
        println!("Confidence    {}", c.confidence);
        if let Some(n) = &c.notes {
            println!("Notes         {n}");
        }
        println!("Sources:");
        for s in &c.sources {
            println!("  {s}");
        }
    } else if let Entry::Custom(c) = e {
        if let Some(n) = &c.notes {
            println!("Notes         {n}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sound file its folder's session file lists is read with the
    /// take's beat rate and lift angle, as `series` and the app read it; a
    /// flag still wins, and a file the session file doesn't list is read
    /// on its own (#1573).
    #[test]
    fn a_sound_file_in_a_take_is_resolved_with_its_session_file() {
        let dir = std::env::temp_dir().join(format!("tg-resolve-for-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["take.wav", "other.wav"] {
            std::fs::write(dir.join(name), []).unwrap();
        }
        std::fs::write(
            dir.join(timegrapher_core::take::MANIFEST),
            "bph = 28800\nlift = 55\n\n[[recording]]\nfile = \"take.wav\"\nposition = \"DU\"\n",
        )
        .unwrap();
        let take = dir.join("take.wav");
        let r = resolve_for(&take, None, None, None, None).unwrap();
        assert_eq!((r.bph, r.lift_deg, r.lift_known), (Some(28800), 55.0, true));
        let r = resolve_for(&take, None, Some(21600), Some(49.0), None).unwrap();
        assert_eq!((r.bph, r.lift_deg), (Some(21600), 49.0));
        let r = resolve_for(&dir.join("other.wav"), None, None, None, None).unwrap();
        assert_eq!(
            (r.bph, r.lift_deg, r.lift_known),
            (None, DEFAULT_LIFT_DEG, false)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn flags_win_over_the_calibre_and_the_calibre_over_defaults() {
        let r = resolve(Some("ETA 2824-2"), None, None, None).unwrap();
        let c = calibres::find("ETA 2824-2").unwrap();
        assert_eq!(r.bph, c.bph);
        assert_eq!(r.lift_deg, c.lift_angle_deg.unwrap_or(DEFAULT_LIFT_DEG));
        assert_eq!(
            r.escape_teeth,
            c.escape_teeth.unwrap_or(DEFAULT_ESCAPE_TEETH)
        );

        let r = resolve(Some("ETA 2824-2"), Some(21600), Some(49.0), Some(15)).unwrap();
        assert_eq!((r.bph, r.lift_deg, r.escape_teeth), (Some(21600), 49.0, 15));

        let r = resolve(None, None, None, None).unwrap();
        assert!(r.bph.is_none());
        assert_eq!(r.lift_deg, DEFAULT_LIFT_DEG);

        assert!(resolve(Some("no such calibre"), None, None, None).is_err());
    }

    #[test]
    fn catalogue_custom_settings_keep_explicit_flag_precedence() {
        let path =
            std::env::temp_dir().join(format!("tg-custom-settings-{}.json", std::process::id()));
        let user: UserCalibres = serde_json::from_value(json!({
            "schema": calibre_store::SCHEMA,
            "custom": [{"name":"Bench 44","aliases":["Workshop-A"],"bph":21600,"lift_angle_deg":44.0,"escape_teeth":20,"wheels":[]}],
            "wheels": [],
        })).unwrap();
        user.save(&path).unwrap();
        let resolve = |bph, lift, teeth| {
            let train = Train {
                calibre: Some("Workshop-A"),
                escape_teeth: teeth,
            }
            .resolve_at(Some(&path))
            .unwrap();
            resolve_train(train, bph, lift, teeth)
        };
        let r = resolve(None, None, None);
        assert_eq!(
            (r.bph, r.lift_deg, r.escape_teeth, r.lift_known),
            (Some(21600), 44.0, 20, true)
        );
        assert_eq!(r.train.calibre.unwrap().name, "Bench 44");
        let r = resolve(Some(28800), Some(52.0), Some(15));
        assert_eq!(
            (r.bph, r.lift_deg, r.escape_teeth, r.lift_known),
            (Some(28800), 52.0, 15, true)
        );
        std::fs::write(&path, "not JSON").unwrap();
        assert!(Train {
            calibre: Some("Workshop-A"),
            escape_teeth: None
        }
        .resolve_at(Some(&path))
        .is_err());
        let no_calibre = Train {
            calibre: None,
            escape_teeth: None,
        }
        .resolve_at(Some(&path))
        .unwrap();
        let r = resolve_train(no_calibre, None, None, None);
        assert_eq!(
            (r.bph, r.lift_deg, r.escape_teeth, r.lift_known),
            (None, 52.0, 15, false)
        );
        std::fs::remove_file(&path).unwrap();
        let shipped = Train {
            calibre: Some("ETA 2824-2"),
            escape_teeth: None,
        }
        .resolve_at(Some(&path))
        .unwrap();
        assert_eq!(shipped.calibre.unwrap().name, "ETA 2824-2");
    }

    #[test]
    fn components_take_the_calibres_wheel_names() {
        let mut c = vec![
            Component {
                period_s: 5.1,
                power: 0.1,
                prominence: 10.0,
                wheel: Some("old".into()),
            },
            Component {
                period_s: 33.0,
                power: 0.1,
                prominence: 10.0,
                wheel: Some("old".into()),
            },
        ];
        let w = vec![Wheel {
            name: "escape wheel".into(),
            period_s: 5.0,
        }];
        rename(&mut c, &w);
        assert_eq!(c[0].wheel.as_deref(), Some("escape wheel"));
        assert_eq!(c[1].wheel, None);
    }

    #[test]
    fn escape_teeth_wins_over_the_calibres_escape_wheel() {
        let escape = |teeth| {
            let train = Train {
                calibre: Some("Rolex 3235"),
                escape_teeth: teeth,
            };
            let w = wheels(&train, 28800).unwrap();
            let e: Vec<f64> = w
                .iter()
                .filter(|w| w.name == "escape wheel")
                .map(|w| w.period_s)
                .collect();
            assert!(w.iter().any(|w| w.name == "fourth wheel"));
            e
        };
        assert_eq!(escape(None), vec![5.0]);
        assert_eq!(escape(Some(15)), vec![3.75]);
    }
}
