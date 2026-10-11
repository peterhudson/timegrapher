//! The `calibres` command, and `--calibre` on `analyze`: the built-in
//! table of calibres (`timegrapher_core::calibres`) for people and agents,
//! so a beat rate and lift angle come from the table rather than being
//! copied by hand.

use crate::long::Train;
use crate::output;
use serde_json::json;
use timegrapher_core::calibres::{self, Calibre};
use timegrapher_core::periodicity::{standard_wheels, Component, Wheel};

/// Lift angle when neither `--lift` nor the calibre gives one, degrees.
pub const DEFAULT_LIFT_DEG: f64 = 52.0;
/// Escape wheel teeth when neither `--escape-teeth` nor the calibre gives
/// a count.
pub const DEFAULT_ESCAPE_TEETH: u32 = 15;

/// The settings `analyze` runs with: an explicit flag wins, then the
/// calibre's figure, then the default.
pub struct Resolved {
    pub bph: Option<u32>,
    pub lift_deg: f64,
    /// The lift angle came from the flag or the calibre, not the default.
    pub lift_known: bool,
    pub escape_teeth: u32,
}

pub fn resolve(
    calibre: Option<&str>,
    bph: Option<u32>,
    lift_deg: Option<f64>,
    escape_teeth: Option<u32>,
) -> Result<Resolved, String> {
    let c = Train {
        calibre,
        escape_teeth,
    }
    .calibre()?;
    let lift = lift_deg.or(c.and_then(|c| c.lift_angle_deg));
    Ok(Resolved {
        bph: bph.or(c.map(|c| c.bph)),
        lift_deg: lift.unwrap_or(DEFAULT_LIFT_DEG),
        lift_known: lift.is_some(),
        escape_teeth: escape_teeth
            .or(c.and_then(|c| c.escape_teeth))
            .unwrap_or(DEFAULT_ESCAPE_TEETH),
    })
}

/// The wheels to name components after: the calibre's train, with the
/// escape wheel `--escape-teeth` gives in place of the table's.
pub fn wheels(train: &Train, bph: u32) -> Result<Vec<Wheel>, String> {
    let mut w = train.wheels(bph)?;
    if let Some(t) = train.escape_teeth {
        w.retain(|w| w.name != "escape wheel");
        w.extend(
            standard_wheels(bph, t)
                .into_iter()
                .filter(|w| w.name == "escape wheel"),
        );
    }
    Ok(w)
}

/// Name each component after the first wheel whose turn is within 6% of
/// its period, as the period search does, or after none.
pub fn rename(components: &mut [Component], wheels: &[Wheel]) {
    for c in components {
        c.wheel = wheels
            .iter()
            .find(|w| (c.period_s / w.period_s - 1.0).abs() < 0.06)
            .map(|w| w.name.clone());
    }
}

/// `timegrapher calibres [NAME]`: every calibre, or the one called NAME.
pub fn run(name: Option<&str>, json: bool) -> Result<(), String> {
    let list: Vec<&Calibre> = match name {
        Some(n) => vec![calibres::find(n).ok_or_else(|| {
            format!("'{n}' is not in the table; `timegrapher calibres` lists the known calibres")
        })?],
        None => calibres::all().iter().collect(),
    };
    if json {
        return output::print(
            "calibres",
            output::input(&[], json!({ "name": name })),
            &json!({ "calibres": list }),
        );
    }
    if name.is_some() {
        for c in list {
            print_one(c);
        }
        return Ok(());
    }
    println!(
        "{:<22} {:>7}  {:>5}  {:>6}  {:>9}  Also matches",
        "Calibre", "vph", "Lift", "Escape", "Wheels"
    );
    for c in list {
        println!(
            "{:<22} {:>7}  {:>5}  {:>6}  {:>9}  {}",
            c.calibre,
            c.bph,
            c.lift_angle_deg
                .map(|l| format!("{l:.0}°"))
                .unwrap_or_else(|| "-".into()),
            c.escape_teeth
                .map(|t| t.to_string())
                .unwrap_or_else(|| "-".into()),
            c.wheels.len(),
            c.family.join(", ")
        );
    }
    println!(
        "\nLift: the lift angle; Escape: escape wheel teeth; '-' where no source gives one. \
`timegrapher calibres NAME` shows one calibre's wheels and sources."
    );
    Ok(())
}

fn print_one(c: &Calibre) {
    println!("{} ({})", c.calibre, c.maker);
    if !c.family.is_empty() {
        println!("Also matches  {}", c.family.join(", "));
    }
    println!("Beat rate     {} vph", c.bph);
    match c.lift_angle_deg {
        Some(l) => println!("Lift angle    {l}°"),
        None => println!("Lift angle    not published (analyze uses {DEFAULT_LIFT_DEG}°)"),
    }
    if let Some(t) = c.escape_teeth {
        println!("Escape wheel  {t} teeth");
    }
    if let Some(s) = &c.seconds {
        println!("Seconds       {s}");
    }
    println!("Wheels (one full turn):");
    for w in &c.wheels {
        println!("  {:<16} {:>10.2} s", w.name, w.period_s);
    }
    println!("Confidence    {}", c.confidence);
    if let Some(n) = &c.notes {
        println!("Notes         {n}");
    }
    println!("Sources:");
    for s in &c.sources {
        println!("  {s}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_win_over_the_calibre_and_the_calibre_over_defaults() {
        let r = resolve(Some("ETA 2824-2"), None, None, None).unwrap();
        let c = calibres::find("ETA 2824-2").unwrap();
        assert_eq!(r.bph, Some(c.bph));
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
