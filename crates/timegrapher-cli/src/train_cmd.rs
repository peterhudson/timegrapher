//! The `train` command: a going train worked out from its tooth and leaf
//! counts (`timegrapher_core::train`), from a calibre's counts or from
//! counts typed on the command line, or both.

use crate::output;
use serde_json::json;
use timegrapher_core::calibre_store::{self, Catalog, UserCalibres};
use timegrapher_core::train::{self, period_text, Solved, ARBORS};

pub struct Options {
    pub calibre: Option<String>,
    pub bph: Option<u32>,
    /// TEETH/LEAVES for each of [`ARBORS`], in its order.
    pub counts: [Option<String>; 5],
    /// ARBOR=SECONDS, several split by commas.
    pub periods: Vec<String>,
    pub json: bool,
}

pub fn run(o: &Options) -> Result<(), String> {
    let user = match calibre_store::default_path() {
        Some(p) => UserCalibres::load(&p)?,
        None => UserCalibres::default(),
    };
    let cat = Catalog::new(&user);
    let entry = match &o.calibre {
        Some(n) => Some(cat.find(n).ok_or_else(|| {
            format!("'{n}' is not in the table; `timegrapher calibres --search {n}` looks wider")
        })?),
        None => None,
    };
    let mut arbors = entry.map(|e| cat.train_input(e)).unwrap_or_default();
    let mut counts = Vec::new();
    for (name, typed) in ARBORS.iter().zip(&o.counts) {
        if let Some(t) = typed {
            counts.push((*name, train::parse_count(t)?));
        }
    }
    let mut periods = Vec::new();
    for p in &o.periods {
        periods.extend(train::parse_periods(p)?);
    }
    train::overlay(&mut arbors, &counts, &periods);
    let bph = o.bph.or(entry.and_then(|e| e.bph()));
    let solved = train::solve(bph, &arbors);
    if o.json {
        return output::print(
            "train",
            output::input(
                &[],
                json!({
                    "calibre": entry.map(|e| e.name()),
                    "bph": o.bph,
                    "counts": counts.iter().map(|(a, (t, l))| json!({"arbor": a, "teeth": t, "leaves": l})).collect::<Vec<_>>(),
                    "periods": periods.iter().map(|(a, p)| json!({"arbor": a, "period_s": p})).collect::<Vec<_>>(),
                }),
            ),
            &solved,
        );
    }
    print_text(entry.map(|e| e.name()), &solved);
    Ok(())
}

fn print_text(calibre: Option<&str>, s: &Solved) {
    if let Some(c) = calibre {
        println!("Calibre       {c}");
    }
    match (s.bph, s.bph_from_counts) {
        (Some(b), _) => println!("Beat rate     {b} bph"),
        (None, Some(b)) => println!("Beat rate     {} bph, from the counts", period_text(b)),
        (None, None) => println!("Beat rate     not given"),
    }
    println!(
        "{:<14} {:>5} {:>6}  {:>10}  {:>10}  How the teeth give it",
        "Arbor", "Teeth", "Leaves", "Given (s)", "Teeth (s)"
    );
    let n = |v: Option<u32>| v.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
    let p = |v: Option<f64>| v.map(period_text).unwrap_or_else(|| "-".into());
    for a in &s.arbors {
        let leaves = if a.name == "barrel" {
            String::new()
        } else {
            n(a.leaves)
        };
        let how = a.how().or_else(|| a.needs_text()).unwrap_or_default();
        println!(
            "{:<14} {:>5} {:>6}  {:>10}  {:>10}  {how}",
            a.name,
            n(a.teeth),
            leaves,
            p(a.typed_s),
            p(a.counted_s)
        );
    }
    if !s.meshes.is_empty() {
        println!("Meshes (one tooth every):");
        for m in &s.meshes {
            let p = m
                .period_s
                .map(|p| format!("{} s", period_text(p)))
                .unwrap_or_else(|| "not known".into());
            println!(
                "  {:<14} {p}  (the {}'s {} teeth on {} leaves)",
                train::pinion_name(&m.pinion),
                m.wheel,
                m.teeth,
                m.leaves
            );
        }
    }
}
