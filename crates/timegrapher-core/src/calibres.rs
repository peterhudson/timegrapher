//! Calibres: beat rate, lift angle and the train-wheel turn periods.
//!
//! A fault on a wheel repeats once per turn of that wheel, so a periodic
//! change in rate or amplitude can be named once the turn periods of the
//! movement's train are known. The table is `data/train-wheels.json` in
//! this crate, and `data/makers.json` lists the other names makers go by
//! ("DD" for Dandong, "AP" for Audemars Piguet); `data/README.md` says how
//! they were built and how far each figure can be trusted. A wheel is left
//! out where its period is not known, rather than guessed, so many entries
//! carry a beat rate and lift angle and no wheels at all.
//!
//! What a user adds on top of the table (wheel periods, calibres of their
//! own) lives in [`crate::calibre_store`].

use crate::periodicity::Wheel;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const DATA: &str = include_str!("../data/train-wheels.json");
const MAKERS: &str = include_str!("../data/makers.json");

/// One calibre and its train.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calibre {
    /// The name shown to the user, e.g. "ETA 2824-2".
    pub calibre: String,
    pub maker: String,
    /// Other names that match this entry: sister calibres with the same
    /// train, clones, and the names the session files use.
    #[serde(default)]
    pub family: Vec<String>,
    /// Names the entry went by in earlier versions of the table, so a
    /// saved pick or a take that names one still finds it, with or without
    /// the maker. Listed in `calibres --json`; not searched.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub former_names: Vec<String>,
    /// Common and modern: in the short list the app offers first. The
    /// rest (uncommon and vintage calibres) appear when the list is
    /// expanded or searched.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub common: bool,
    /// Beats per hour, where a source gives it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bph: Option<u32>,
    /// Escape wheel teeth, when known.
    #[serde(default)]
    pub escape_teeth: Option<u32>,
    /// Lift angle in degrees, where one is published or widely used.
    #[serde(default)]
    pub lift_angle_deg: Option<f64>,
    /// Where the seconds hand sits: "centre", "indirect centre", "small
    /// seconds at 6" and so on.
    #[serde(default)]
    pub seconds: Option<String>,
    /// The wheels whose turn is known; often none.
    #[serde(default)]
    pub wheels: Vec<TrainWheel>,
    pub sources: Vec<String>,
    /// The best basis in the entry: "published", "derived from tooth
    /// counts", "derived from the beat rate", "measured", "published list"
    /// (imported from a public lift-angle list) or "maker's page" (from the
    /// maker's own specification page).
    pub confidence: String,
    #[serde(default)]
    pub notes: Option<String>,
    /// When the calibre was made, from a cited source; never inferred from
    /// serial numbers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub years: Option<Years>,
    /// A short factual description, one sourced sentence per entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary: Vec<Sourced>,
    /// Faults a cited service document, maker bulletin or named
    /// watchmaker's write-up reports for this calibre.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<Sourced>,
    /// Other calibres this one is related to, stored on the row described
    /// ("this row … that calibre"). Never carries figures across.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<Related>,
    /// Makers' files for this calibre kept in the timegrapher-calibres
    /// repository, by path there (`<maker>/<calibre>/<file>`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<Asset>,
    /// An escapement Timegrapher does not measure yet, from a cited
    /// source; absent for a Swiss lever (#1872).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escapement: Option<Escapement>,
}

/// A calibre's escapement when it is not a Swiss lever (#1872).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Escapement {
    /// One of [`ESCAPEMENTS`].
    pub kind: String,
    pub source: String,
}

/// The escapements [`Escapement::kind`] may name.
pub const ESCAPEMENTS: [&str; 3] = ["co-axial", "pin lever", "detent"];

impl Escapement {
    /// The short tag beside the calibre's name where it is listed (#1872).
    pub const TAG: &'static str = "Not supported yet";

    /// What the app says of it, in place of the lift angle (#1872).
    pub fn sentence(&self) -> String {
        let mut kind = self.kind.chars();
        let kind = match kind.next() {
            Some(first) => first.to_uppercase().chain(kind).collect::<String>(),
            None => String::new(),
        };
        format!(
            "{kind} escapement: not supported yet. Timegrapher measures Swiss lever \
             escapements only."
        )
    }
}

/// When a calibre was made.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Years {
    #[serde(default)]
    pub from: Option<u16>,
    #[serde(default)]
    pub to: Option<u16>,
    /// The source gives the years as approximate ("about 1970").
    #[serde(default)]
    pub approx: bool,
    pub source: String,
}

/// One sentence and the source it rests on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sourced {
    pub text: String,
    pub source: String,
}

/// How this calibre relates to another: "clone of", "based on",
/// "successor of" or "same base as" (the only symmetric one; the app
/// shows each relation's inverse on the other row, which is not stored).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Related {
    /// The other calibre's exact `calibre` name in the table.
    pub calibre: String,
    pub relation: String,
    pub source: String,
}

/// The relations `Related::relation` may take.
pub const RELATIONS: [&str; 4] = ["clone of", "based on", "successor of", "same base as"];

/// A maker's file kept in the timegrapher-calibres repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    /// Path in that repository, `<maker>/<calibre>/<file>`.
    pub path: String,
    pub kind: String,
}

/// The kinds `Asset::kind` may take.
pub const ASSET_KINDS: [&str; 6] = [
    "spec sheet",
    "drawing",
    "manual",
    "photo (dial side)",
    "photo (balance side)",
    "photo",
];

/// The raw address of an asset in the timegrapher-calibres repository.
pub fn asset_url(a: &Asset) -> String {
    format!(
        "https://raw.githubusercontent.com/peterhudson/timegrapher-calibres/main/{}",
        a.path
            .split('/')
            .map(|p| p.replace(' ', "%20"))
            .collect::<Vec<_>>()
            .join("/")
    )
}

/// One wheel of the train.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainWheel {
    /// Lower case, as the app words it: "escape wheel", "fourth wheel".
    pub name: String,
    /// One full turn, seconds.
    pub period_s: f64,
    /// Teeth on the wheel, where known.
    #[serde(default)]
    pub teeth: Option<u32>,
    /// Leaves of the pinion on the same arbor, where known.
    #[serde(default)]
    pub pinion: Option<u32>,
    /// How this period is known, when it differs from the calibre's.
    #[serde(default)]
    pub basis: Option<String>,
}

/// A maker and the other names it goes by.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Maker {
    /// As the table's `maker` field writes it.
    pub maker: String,
    /// One of the big makers listed first; the rest appear when the list
    /// of makers is expanded.
    #[serde(default)]
    pub common: bool,
    /// Abbreviations, older names and the names clone factories and
    /// sellers use: "DD" for Dandong, "AP" for Audemars Piguet.
    pub aliases: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
}

/// Every calibre in the table.
pub fn all() -> &'static [Calibre] {
    static TABLE: OnceLock<Vec<Calibre>> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(DATA).expect("data/train-wheels.json parses"))
}

/// The calibres in the short list, common and modern ones.
pub fn common() -> impl Iterator<Item = &'static Calibre> {
    all().iter().filter(|c| c.common)
}

/// Every maker with other names.
pub fn makers() -> &'static [Maker] {
    static TABLE: OnceLock<Vec<Maker>> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(MAKERS).expect("data/makers.json parses"))
}

/// The other names of `maker`, the maker's own name first.
pub fn maker_names(maker: &str) -> Vec<&str> {
    let mut v = vec![maker];
    if let Some(m) = makers().iter().find(|m| m.maker == maker) {
        v.extend(m.aliases.iter().map(String::as_str));
    }
    v
}

/// Letters and digits only, lower case, so "ETA 2824-2", "eta2824-2" and
/// "2824 2" compare alike.
pub fn key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The calibre called `name`: the best of [`named`].
pub fn find(name: &str) -> Option<&'static Calibre> {
    named(name).into_iter().next()
}

/// Every calibre `name` names, best first (#1878): its full name, then
/// its name without the maker or one of its family names, then a name it
/// had before; within each, table order, as the table's first match
/// was always taken.
/// So "ETA 2732" is the ETA 2732, not the Cyma ETA 2732 whose name
/// without its maker is the same.
pub fn named(name: &str) -> Vec<&'static Calibre> {
    let k = key(name);
    if k.is_empty() {
        return Vec::new();
    }
    let tier = |c: &Calibre| {
        if key(&c.calibre) == k {
            Some(0)
        } else if key(c.calibre.strip_prefix(c.maker.as_str()).unwrap_or("")) == k
            || c.family.iter().any(|f| key(f) == k)
        {
            Some(1)
        } else if had_name(c, &k) {
            Some(2)
        } else {
            None
        }
    };
    let mut hits: Vec<(u8, &'static Calibre)> = all()
        .iter()
        .filter_map(|c| tier(c).map(|t| (t, c)))
        .collect();
    // A stable sort keeps table order within a tier.
    hits.sort_by_key(|h| h.0);
    hits.into_iter().map(|h| h.1).collect()
}

/// What to say when `name` is not one calibre's full name and fits
/// others than the one [`find`] picks: their names, so the pick is never
/// silent (#1878). None for a full name or a name only one calibre has.
pub fn also_named_note(name: &str) -> Option<String> {
    let hits = named(name);
    let (first, rest) = hits.split_first()?;
    if rest.is_empty() || key(&first.calibre) == key(name) {
        return None;
    }
    let shown: Vec<&str> = rest.iter().take(3).map(|c| c.calibre.as_str()).collect();
    let others = match (shown.as_slice(), rest.len() - shown.len()) {
        ([one], 0) => (*one).to_string(),
        (names, 0) => {
            let (last, before) = names.split_last()?;
            format!("{} and {last}", before.join(", "))
        }
        (names, more) => format!("{} and {more} more", names.join(", ")),
    };
    let pick = if rest.len() == 1 {
        "pick that one"
    } else {
        "pick one of those"
    };
    Some(format!(
        "'{name}' is read as {}. It also names {others}. Give the full name to {pick}.",
        first.calibre
    ))
}

/// Whether `c` once went by the name whose [`key`] is `k`, with or without
/// the maker.
fn had_name(c: &Calibre, k: &str) -> bool {
    c.former_names
        .iter()
        .any(|f| key(f) == k || key(f.strip_prefix(c.maker.as_str()).unwrap_or("")) == k)
}

/// The current name of a table entry once called `name`, matched as
/// [`find`] matches a former name; `None` for a name still in use or not
/// in the table.
pub fn renamed(name: &str) -> Option<&'static str> {
    let k = key(name);
    if k.is_empty() {
        return None;
    }
    all()
        .iter()
        .find(|c| had_name(c, &k))
        .map(|c| c.calibre.as_str())
}

/// Calibres in the table that answer `query`, best first: see [`rank`].
pub fn search(query: &str) -> Vec<&'static Calibre> {
    let mut hits: Vec<(u32, &'static Calibre)> = all()
        .iter()
        .filter_map(|c| rank(query, &c.calibre, &c.maker, &c.family, c.common).map(|s| (s, c)))
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.calibre.cmp(&b.1.calibre)));
    hits.into_iter().map(|h| h.1).collect()
}

/// How well `query` names a calibre called `name` by `maker`, with other
/// names `family`; `None` when it does not. Every word of the query must
/// match the calibre's name, one of its other names, or its maker under
/// any of the maker's names ("dd 3235", "audemars 3120", "AP3120"); an
/// exact name scores above a name that only starts or contains the word,
/// and a common calibre wins a tie. An empty query matches everything.
pub fn rank(query: &str, name: &str, maker: &str, family: &[String], common: bool) -> Option<u32> {
    let bonus = u32::from(common);
    let q = key(query);
    if q.is_empty() {
        return Some(bonus);
    }
    let bare = name.strip_prefix(maker).unwrap_or(name);
    let mut names: Vec<String> = std::iter::once(name)
        .chain(std::iter::once(bare))
        .chain(family.iter().map(String::as_str))
        .map(key)
        .filter(|k| !k.is_empty())
        .collect();
    let makers: Vec<String> = maker_names(maker).into_iter().map(key).collect();
    // The maker's other names joined to the calibre: "dd3235", "ap3120".
    let joined: Vec<String> = makers
        .iter()
        .flat_map(|m| names.iter().map(move |n| format!("{m}{n}")))
        .collect();
    names.extend(joined);
    if names.contains(&q) {
        return Some(100 + bonus);
    }
    let mut score = 0;
    for word in query.split_whitespace().map(key).filter(|w| !w.is_empty()) {
        score += if names.contains(&word) {
            8
        } else if makers.contains(&word) {
            6
        } else if names.iter().any(|n| n.starts_with(&word)) {
            4
        } else if makers.iter().any(|m| m.starts_with(&word)) {
            3
        } else if names.iter().any(|n| n.contains(&word)) {
            2
        } else {
            return None;
        };
    }
    Some(score * 2 + bonus)
}

impl Calibre {
    /// The train as the period search names it.
    pub fn wheels(&self) -> Vec<Wheel> {
        self.wheels
            .iter()
            .map(|w| Wheel {
                name: w.name.clone(),
                period_s: w.period_s,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A full name wins over a name without the maker (#1878), and a
    /// short name several calibres share says which it picked and the rest.
    #[test]
    fn an_exact_name_wins_and_a_shared_one_says_so() {
        let c = find("ETA 2732").unwrap();
        assert_eq!(c.calibre, "ETA 2732");
        assert!(also_named_note("ETA 2732").is_none());
        let all = named("321");
        assert!(
            all.len() > 1,
            "{:?}",
            all.iter().map(|c| &c.calibre).collect::<Vec<_>>()
        );
        assert_eq!(
            find("321").map(|c| &c.calibre),
            all.first().map(|c| &c.calibre)
        );
        let note = also_named_note("321").unwrap();
        // Hugh's words on #1899: no "the", a full stop to each sentence.
        assert_eq!(
            note,
            "'321' is read as Chezard 321. It also names Valjoux 321 and Omega 321. Give the full name to pick one of those."
        );
        assert_eq!(
            also_named_note("9015").unwrap(),
            "'9015' is read as Miyota 9015. It also names Ronda 9015. Give the full name to pick that one."
        );
        // More than three others: the first three, then how many more.
        let rest = named("520").len() - 1;
        assert!(rest > 3);
        let many = also_named_note("520").unwrap();
        assert!(
            many.contains(&format!(
                " and {} more. Give the full name to pick one of those.",
                rest - 3
            )),
            "{many}"
        );
        assert!(
            note.contains(&all[0].calibre) && note.contains(&all[1].calibre),
            "{note}"
        );
        assert!(also_named_note(&all[0].calibre).is_none());
        assert_eq!(find("2824-2").unwrap().calibre, "ETA 2824-2");
    }

    #[test]
    fn seiko_miyota_research_resolves_variants_without_broad_family_transfers() {
        for name in [
            "NH34A", "NH37", "NH39A", "NH70", "NH71A", "NH72", "4R35A", "4R35B", "4R36", "4R37A",
            "4R38A", "4R39A", "7S26A", "7S26B", "7S26C", "7S36A", "7S36B", "7S36C", "7S25C",
            "7S35C", "6R15", "6R15D", "6R35A", "6R31", "6R38", "6R54", "6R55", "8215", "821A",
            "820A", "8217", "82S0", "82S5", "82S7", "8N24", "8N40", "9015", "9039", "90S5", "9075",
            "9100", "9110", "9120", "9122", "9130", "9132",
        ] {
            let c = find(name).unwrap_or_else(|| panic!("missing {name}"));
            let escape = c.wheels.iter().find(|w| w.name == "escape wheel").unwrap();
            assert_eq!(escape.period_s, 5.0, "{name}");
            assert_eq!(escape.teeth, c.escape_teeth, "{name}");
            assert_eq!(
                c.escape_teeth,
                Some(if c.bph == Some(28800) { 20 } else { 15 })
            );
            assert!(escape
                .basis
                .as_deref()
                .unwrap()
                .contains("manufacturer illustration"));
            assert!(escape.pinion.is_none(), "unmeasured escape pinion: {name}");
        }
        assert!(find("6R").is_none());
        assert!(find("7S").is_none());
        for name in [
            "Miyota 8200",
            "Seiko NH05",
            "Seiko 6R20A",
            "Seiko 6R21A",
            "Seiko 6R24A",
        ] {
            let c = find(name).unwrap();
            assert!(
                c.escape_teeth.is_none(),
                "unverified family transfer: {name}"
            );
        }
        assert_eq!(find("4R35").unwrap().calibre, "Seiko 4R35B");
        assert_eq!(find("4R35A").unwrap().lift_angle_deg, Some(52.0));
        assert_eq!(find("4R35B").unwrap().lift_angle_deg, Some(53.0));
        assert_ne!(find("6R15").unwrap().calibre, find("6R35").unwrap().calibre);
    }

    #[test]
    fn miyota_hand_periods_do_not_invent_unknown_train_periods() {
        for name in ["8215", "82S0", "8N24"] {
            let c = find(name).unwrap();
            assert!(c
                .wheels
                .iter()
                .any(|w| w.name == "seconds pinion" && w.period_s == 60.0));
            assert!(!c.wheels.iter().any(|w| w.name == "fourth wheel"));
        }
        for name in [
            "9015", "9039", "90S5", "9075", "9100", "9110", "9120", "9122", "9130", "9132",
        ] {
            let c = find(name).unwrap();
            assert!(c
                .wheels
                .iter()
                .any(|w| w.name == "cannon pinion" && w.period_s == 3600.0));
            assert!(!c
                .wheels
                .iter()
                .any(|w| matches!(w.name.as_str(), "centre wheel" | "third wheel" | "barrel")));
        }
    }

    #[test]
    fn table_parses_and_names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for c in all() {
            for n in std::iter::once(&c.calibre)
                .chain(&c.family)
                .chain(&c.former_names)
            {
                assert!(seen.insert(key(n)), "{n} is in the table twice");
            }
            assert!(!c.sources.is_empty(), "{} has no source", c.calibre);
        }
    }

    #[test]
    fn periods_agree_with_the_beat_rate_and_the_teeth() {
        for c in all() {
            let Some(bph) = c.bph else {
                assert!(
                    c.wheels.is_empty(),
                    "{} has wheels and no beat rate",
                    c.calibre
                );
                continue;
            };
            let by_name = |n: &str| c.wheels.iter().find(|w| w.name == n);
            for w in &c.wheels {
                assert_eq!(w.name, w.name.to_lowercase(), "{}", c.calibre);
                assert!(w.period_s > 0.0, "{} {}", c.calibre, w.name);
            }
            if let Some(b) = by_name("balance") {
                let want = crate::train::balance_period_s(bph);
                assert!((b.period_s - want).abs() < 1e-6, "{}", c.calibre);
            }
            // A lever escape wheel moves one tooth per swing, two beats.
            if let (Some(n), Some(e)) = (c.escape_teeth, by_name("escape wheel")) {
                let want = crate::train::escape_period_s(bph, n);
                assert!(
                    (e.period_s - want).abs() < 1e-6,
                    "{} escape wheel",
                    c.calibre
                );
            }
            // Where a wheel and the next pinion are both counted, their
            // ratio is the ratio of the periods.
            let train = [
                "barrel",
                "centre wheel",
                "third wheel",
                "fourth wheel",
                "escape wheel",
            ];
            for pair in train.windows(2) {
                if let (Some(a), Some(b)) = (by_name(pair[0]), by_name(pair[1])) {
                    if let (Some(t), Some(p)) = (a.teeth, b.pinion) {
                        let want = b.period_s * t as f64 / p as f64;
                        assert!(
                            (a.period_s - want).abs() < 1e-6 * want,
                            "{}: {} {} s, its teeth say {} s",
                            c.calibre,
                            pair[0],
                            a.period_s,
                            want
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn dd_jh_32xx_empirical_lift_setting_covers_aliases_only() {
        // Match the catalogue source label; this test does not read the document.
        let source = "dd32xx-optical-amplitude-validation";
        let covered: Vec<_> = all()
            .iter()
            .filter(|c| c.sources.iter().any(|s| s.contains(source)))
            .map(|c| c.calibre.as_str())
            .collect();
        let dd_jh = [
            "Dandong 3230",
            "Dandong 3235",
            "Dandong 3255",
            "Dandong 3285",
            "JH3230",
            "JH3235",
            "JH3255",
            "JH3285",
        ];
        for name in dd_jh {
            let c = find(name).unwrap();
            assert_eq!(c.lift_angle_deg, Some(59.0), "{name}");
            assert!(c.sources.iter().any(|s| s.contains(source)), "{name}");
            for alias in &c.family {
                assert_eq!(find(alias).unwrap().lift_angle_deg, Some(59.0), "{alias}");
                assert_eq!(search(alias)[0].calibre, c.calibre, "{alias}");
            }
        }
        assert_eq!(covered, dd_jh);
        for (name, lift) in [
            ("Rolex 3235", Some(55.0)),
            ("Rolex 3235 (53°)", Some(53.0)),
            ("Rolex 3255 (53°)", Some(53.0)),
            ("VR3235", Some(52.0)),
            ("VR3285", None),
            ("DD3135", None),
            ("DD324", Some(52.0)),
        ] {
            let c = find(name).unwrap();
            assert_eq!(c.lift_angle_deg, lift, "{name}");
            assert!(c.sources.iter().all(|s| !s.contains(source)), "{name}");
        }
        for name in ["SH3235", "SH3285", "SH3285-2"] {
            assert!(find(name)
                .unwrap()
                .sources
                .iter()
                .all(|s| !s.contains(source)));
        }
    }

    #[test]
    fn counted_32xx_train_resolves_for_covered_makers_and_lift_variants() {
        let mut names = vec![
            "Rolex 32xx".to_string(),
            "DD32xx".to_string(),
            "Dandong 32xx".to_string(),
            "JH32xx".to_string(),
            "Jinghe 32xx".to_string(),
            "VS3235".to_string(),
            "Rolex 3235 (53°)".to_string(),
            "Rolex 3255 (53°)".to_string(),
        ];
        for number in [3230, 3235, 3255, 3285] {
            for prefix in ["", "Rolex ", "DD", "Dandong ", "JH", "Jinghe "] {
                names.push(format!("{prefix}{number}"));
            }
        }
        for name in names {
            let c = find(&name).unwrap_or_else(|| panic!("missing {name}"));
            assert_eq!(c.bph, Some(28800), "{name}");
            assert_eq!(c.escape_teeth, Some(20), "{name}");
            for (wheel, period, teeth, pinion) in [
                ("barrel", 25200.0, 98, None),
                ("centre wheel", 3600.0, 80, Some(14)),
                ("third wheel", 540.0, 99, Some(12)),
                ("fourth wheel", 60.0, 120, Some(11)),
                ("escape wheel", 5.0, 20, Some(10)),
            ] {
                let w = c.wheels.iter().find(|w| w.name == wheel).unwrap();
                assert_eq!(
                    (w.period_s, w.teeth, w.pinion),
                    (period, Some(teeth), pinion),
                    "{name}: {wheel}"
                );
            }
            let expected_lift = match c.maker.as_str() {
                "Peacock (Dandong)" | "Shanghai Jinghe (clone)" => 59.0,
                _ if name.contains("53°") => 53.0,
                _ => 55.0,
            };
            assert_eq!(c.lift_angle_deg, Some(expected_lift), "{name}");
            assert!(c
                .sources
                .iter()
                .any(|s| s.contains("32xx-train-wheel-counts.md")));
        }
    }

    #[test]
    fn each_32xx_movement_has_its_own_row_and_shares_its_makers_train() {
        // 3230 time only, 3235 date, 3255 day-date, 3285 GMT: separate
        // movements on one going train (#1820).
        for (maker, name_of) in [
            ("Rolex", "Rolex "),
            ("Peacock (Dandong)", "Dandong "),
            ("Shanghai Jinghe (clone)", "JH"),
        ] {
            let base = find(&format!("{name_of}3235")).unwrap();
            for number in ["3230", "3235", "3255", "3285"] {
                let name = format!("{name_of}{number}");
                let c = find(&name).unwrap_or_else(|| panic!("missing {name}"));
                assert_eq!(c.calibre, name);
                assert_eq!(c.maker, maker, "{name}");
                assert!(c.common, "{name}");
                assert_eq!(search(&name)[0].calibre, name);
                assert_eq!(c.lift_angle_deg, base.lift_angle_deg, "{name}");
                assert_eq!(c.bph, base.bph, "{name}");
                let train = |c: &Calibre| {
                    c.wheels
                        .iter()
                        .map(|w| (w.name.clone(), w.period_s, w.teeth, w.pinion))
                        .collect::<Vec<_>>()
                };
                assert_eq!(train(c), train(base), "{name}");
            }
        }
        for (alias, row) in [
            ("DD3255", "Dandong 3255"),
            ("DD3285", "Dandong 3285"),
            ("DD3230", "Dandong 3230"),
            ("Jinghe 3255", "JH3255"),
            ("3255", "Rolex 3255"),
            ("3285", "Rolex 3285"),
            ("3230", "Rolex 3230"),
            ("DD32xx", "Dandong 3235"),
            ("VS3235", "Dandong 3235"),
            ("Rolex 32xx", "Rolex 3235"),
            ("JH32xx", "JH3235"),
        ] {
            assert_eq!(find(alias).unwrap().calibre, row, "{alias}");
        }
        // A search for the bare number offers every maker's row of it.
        let hits: Vec<_> = search("3255").iter().map(|c| c.calibre.as_str()).collect();
        for row in ["Rolex 3255", "Dandong 3255", "JH3255"] {
            assert!(hits.contains(&row), "{row}: {hits:?}");
        }
    }

    #[test]
    fn sister_movements_have_their_own_rows_on_one_train() {
        // Each sister movement is its own row (#1822), with the train,
        // beat rate and lift of the row it was an alias of.
        let train = |c: &Calibre| {
            c.wheels
                .iter()
                .map(|w| (w.name.clone(), w.period_s, w.teeth, w.pinion))
                .collect::<Vec<_>>()
        };
        for (base, rows) in [
            (
                "Rolex 3135",
                &[
                    "Rolex 3130",
                    "Rolex 3155",
                    "Rolex 3175",
                    "Rolex 3185",
                    "Rolex 3186",
                    "Rolex 3187",
                ][..],
            ),
            (
                "Dandong 3135",
                &["Dandong 3130", "Dandong 3186", "Dandong 3187"][..],
            ),
            ("VR3135", &["VR3130", "VR3186", "VR3187"][..]),
            ("ETA 2824-2", &["ETA 2834-2", "ETA 2836-2"][..]),
            (
                "Sellita SW200-1",
                &["Sellita SW220-1", "Sellita SW240-1"][..],
            ),
            ("ETA 2892-A2", &["Sellita SW300-1"][..]),
        ] {
            let b = find(base).unwrap();
            for name in rows {
                let c = find(name).unwrap_or_else(|| panic!("missing {name}"));
                assert_eq!(c.calibre, *name);
                assert_eq!(search(name)[0].calibre, *name);
                assert_eq!(train(c), train(b), "{name}");
                assert_eq!(
                    (c.bph, c.lift_angle_deg),
                    (b.bph, b.lift_angle_deg),
                    "{name}"
                );
                assert!(
                    c.sources.iter().any(|s| s.contains("/issues/1822")),
                    "{name}"
                );
            }
        }
        for (alias, row) in [
            ("3186", "Rolex 3186"),
            ("3155", "Rolex 3155"),
            ("DD3187", "Dandong 3187"),
            ("VS3130", "Dandong 3130"),
            ("VR3186", "VR3186"),
            ("2836-2", "ETA 2836-2"),
            ("SW220", "Sellita SW220-1"),
            ("SW300-1", "Sellita SW300-1"),
            ("Rolex 31xx", "Rolex 3135"),
            ("DD31xx", "Dandong 3135"),
            ("VR31xx", "VR3135"),
            ("2892-2", "ETA 2892-A2"),
            ("SW200-1 a", "Sellita SW200-1"),
        ] {
            assert_eq!(find(alias).unwrap().calibre, row, "{alias}");
        }
        assert_eq!(find("Sellita SW300-1").unwrap().maker, "Sellita");
    }

    #[test]
    fn counted_32xx_train_does_not_leak_into_vr_sh_or_ambiguous_clones() {
        // Bare clone names cannot safely choose a train manufacturer.
        assert!(find("3235 (clone)").is_none());
        assert!(find("32xx").is_none());
        for name in ["SH3235", "SH3285", "SH3285-2"] {
            let c = find(name).unwrap();
            assert_eq!(c.escape_teeth, None, "{name}");
            assert!(c
                .wheels
                .iter()
                .all(|w| w.teeth.is_none() && w.pinion.is_none()));
            for wheel in ["third wheel", "barrel", "escape wheel"] {
                assert!(c.wheels.iter().all(|w| w.name != wheel), "{name}: {wheel}");
            }
            assert_eq!(search(name)[0].calibre, c.calibre, "{name}");
        }
        // Other VR/SH names have no measured entry yet, not a DD/JH fallback.
        for name in ["VR3230", "VR3255", "SH3230", "SH3255", "SH32xx"] {
            assert!(find(name).is_none(), "{name}");
        }
    }

    #[test]
    fn vr32xx_has_its_own_train_and_accepted_escape_pair() {
        for name in ["VR3235", "VR32xx", "VR3285"] {
            let c = find(name).unwrap();
            let wheel = |n: &str| c.wheels.iter().find(|w| w.name == n).unwrap();
            for (n, t, p, period) in [
                ("barrel", 100, None, 18000.0),
                ("centre wheel", 80, Some(20), 3600.0),
                ("third wheel", 99, Some(12), 540.0),
                ("fourth wheel", 120, Some(11), 60.0),
                ("escape wheel", 20, Some(10), 5.0),
            ] {
                let w = wheel(n);
                assert_eq!(
                    (w.teeth, w.pinion, w.period_s),
                    (Some(t), p, period),
                    "{name}: {n}"
                );
            }
            assert_eq!(c.escape_teeth, Some(20));
            assert!(wheel("escape wheel")
                .basis
                .as_deref()
                .unwrap()
                .contains("Accepted 20/10 escape pair"));
            assert!(wheel("escape wheel")
                .basis
                .as_deref()
                .unwrap()
                .contains("no VR escape-wheel photograph"));
            // The measured second-to-fourth ratio is exactly 60.
            assert_eq!((80.0 / 12.0) * (99.0 / 11.0), 60.0);
            assert_eq!(search(name)[0].calibre, c.calibre);
        }
        assert_ne!(
            find("VR3235").unwrap().calibre,
            find("SH3235").unwrap().calibre
        );
        assert_ne!(
            find("VR3285").unwrap().calibre,
            find("SH3285").unwrap().calibre
        );
        for name in [
            "SH3235",
            "SH3285",
            "SH3285-2",
            "Rolex 3235",
            "DD3235",
            "JH3235",
        ] {
            assert!(
                find(name)
                    .unwrap()
                    .sources
                    .iter()
                    .all(|s| !s.contains("vr32xx-train-wheel-counts.md")),
                "{name}"
            );
        }
        assert_eq!(
            find("DD3235")
                .unwrap()
                .wheels
                .iter()
                .find(|w| w.name == "barrel")
                .unwrap()
                .period_s,
            25200.0
        );
        assert_eq!(find("VR3235").unwrap().lift_angle_deg, Some(52.0));
        assert_eq!(find("VR3285").unwrap().lift_angle_deg, None);
    }

    #[test]
    fn counted_2824_train_reconciles_cannon_mesh_and_scopes_variants() {
        let covered: Vec<_> = all()
            .iter()
            .filter(|c| {
                c.sources
                    .iter()
                    .any(|s| s.contains("2824-train-wheel-counts.md"))
            })
            .collect();
        // 39 rows, plus the 2834-2, 2836-2, SW220-1 and SW240-1 split from
        // their sisters' aliases (#1822).
        assert_eq!(covered.len(), 43);
        for c in covered {
            assert_eq!(c.bph, Some(28800), "{}", c.calibre);
            let wheel = |n: &str| c.wheels.iter().find(|w| w.name == n).unwrap();
            let third = wheel("third wheel");
            let cannon = wheel("cannon pinion");
            assert_eq!(third.period_s, 600.0);
            assert_eq!(
                cannon.period_s,
                third.period_s * cannon.teeth.unwrap() as f64 / third.pinion.unwrap() as f64
            );
            assert!((wheel("centre wheel").period_s - 46200.0 / 13.0).abs() < 1e-9);
            assert!((wheel("barrel").period_s - 4666200.0 / 221.0).abs() < 1e-9);
            assert_eq!(wheel("escape wheel").pinion, Some(9));
            if c.calibre != "ETA 2824-2" {
                assert!(third
                    .basis
                    .as_deref()
                    .unwrap()
                    .contains("INFERRED family transfer"));
            }
        }
        for name in [
            "ETA 2801-2",
            "ETA 2804-2",
            "ETA 2826-2",
            "2834-2",
            "2836-2",
            "SW240-1",
            "SW215-1",
            "SW288-1 M b",
            "SW400-1 b",
            "TY2130",
            "ST2100",
            "ST2108",
            "PT5000",
            "STP1-11",
            "P024",
            "HZ6300",
            "VAL24",
            "SH3135 (ETA 2824 base)",
        ] {
            let c = find(name).unwrap_or_else(|| panic!("missing {name}"));
            assert!(
                c.sources
                    .iter()
                    .any(|s| s.contains("2824-train-wheel-counts.md")),
                "{name}"
            );
        }
        for c in all().iter().filter(|c| c.bph != Some(28800)) {
            assert!(
                c.sources
                    .iter()
                    .all(|s| !s.contains("2824-train-wheel-counts.md")),
                "{}",
                c.calibre
            );
        }
        for name in [
            "ETA 2824 (21,600 bph, 53°)",
            "ETA 2826",
            "ETA 2892-A2",
            "SH3135",
            "VR3235",
        ] {
            assert!(
                find(name)
                    .unwrap()
                    .sources
                    .iter()
                    .all(|s| !s.contains("2824-train-wheel-counts.md")),
                "{name}"
            );
        }
        for name in ["SW200-2", "SW200-2 Power+", "SW200-1 S", "SW400-1 AJ"] {
            assert!(find(name).is_none(), "{name}");
        }
        assert_eq!(find("STP1-11").unwrap().lift_angle_deg, Some(52.0));
        assert_eq!(find("ETA 2824-2").unwrap().lift_angle_deg, Some(50.0));
    }

    #[test]
    fn counted_4130_train_keeps_maker_scope_and_inference_provenance() {
        for name in [
            "4130",
            "Rolex 4130",
            "DD4130",
            "Dandong 4130",
            "SH4130",
            "Shanghai 4130",
        ] {
            let c = find(name).unwrap_or_else(|| panic!("missing {name}"));
            let wheel = |n: &str| c.wheels.iter().find(|w| w.name == n).unwrap();
            assert_eq!(wheel("third wheel").period_s, 480.0, "{name}");
            assert_eq!(wheel("barrel").period_s, 25200.0, "{name}");
            // The extra 30-tooth clutch drive must not be stored as the
            // receiving pinion, which would make the going-train ratio wrong.
            let fourth = wheel("fourth wheel");
            assert_eq!(fourth.pinion, Some(12), "{name}");
            assert!(fourth.basis.as_deref().unwrap().contains("INFERRED"));
            assert!(c
                .sources
                .iter()
                .any(|s| s.contains("4130-train-wheel-counts.md")));
            // Stopped chronograph output has no unconditional turn period.
            assert!(c
                .wheels
                .iter()
                .all(|w| !w.name.contains("clutch") && !w.name.contains("chronograph")));
        }
        // Lift applicability was separately confirmed by the contributor.
        assert_eq!(find("SH4130").unwrap().lift_angle_deg, Some(52.0));
        assert_eq!(find("Rolex 4130").unwrap().lift_angle_deg, Some(52.0));
        assert_eq!(find("DD4130").unwrap().lift_angle_deg, Some(52.0));
        assert!(find("4130 (clone)").is_none());
        for name in ["Rolex 4131", "DD4131", "SH4131", "Valjoux 7750"] {
            let c = find(name).unwrap_or_else(|| panic!("missing {name}"));
            assert!(
                c.sources
                    .iter()
                    .all(|s| !s.contains("4130-train-wheel-counts.md")),
                "{name}"
            );
        }
    }

    #[test]
    fn counted_31xx_train_distinguishes_second_wheel_from_cannon_pinion() {
        let mut names = vec![
            "Rolex 31xx".to_string(),
            "Rolex 3131".to_string(),
            "DD31xx".to_string(),
            "VS31xx".to_string(),
            "Dandong 31xx".to_string(),
            "VR31xx".to_string(),
        ];
        for number in [3130, 3135, 3186, 3187] {
            for prefix in ["", "Rolex ", "DD", "VS", "Dandong ", "VR"] {
                names.push(format!("{prefix}{number}"));
            }
        }
        for name in names {
            let c = find(&name).unwrap_or_else(|| panic!("missing {name}"));
            assert_eq!(c.bph, Some(28800), "{name}");
            assert_eq!(c.escape_teeth, Some(20), "{name}");
            for (wheel, period, teeth, pinion) in [
                ("barrel", 18000.0, Some(100), None),
                ("centre wheel", 2160.0, Some(81), Some(12)),
                ("third wheel", 320.0, Some(96), Some(12)),
                ("fourth wheel", 60.0, Some(120), Some(18)),
                ("escape wheel", 5.0, Some(20), Some(10)),
                ("cannon pinion", 3600.0, None, None),
            ] {
                let w = c.wheels.iter().find(|w| w.name == wheel).unwrap();
                assert_eq!(
                    (w.period_s, w.teeth, w.pinion),
                    (period, teeth, pinion),
                    "{name}: {wheel}"
                );
            }
            assert!(c
                .sources
                .iter()
                .any(|s| s.contains("31xx-train-wheel-counts.md")));
        }
        assert_eq!(find("DD3135").unwrap().lift_angle_deg, None);
        assert_eq!(find("Rolex 3135").unwrap().lift_angle_deg, Some(52.0));
        assert_eq!(find("VR3135").unwrap().lift_angle_deg, Some(52.0));
        // A shared number/family must not assign these measurements to SH,
        // 32xx/4130 variants, or the initially mistyped AP 3120.
        for name in ["SH3135", "VR3235", "SH3235", "DD3235", "DD4130", "AP 3120"] {
            let c = find(name).unwrap_or_else(|| panic!("missing {name}"));
            assert!(
                c.sources
                    .iter()
                    .all(|s| !s.contains("31xx-train-wheel-counts.md")),
                "{name}"
            );
        }
        assert!(find("SH3135")
            .unwrap()
            .wheels
            .iter()
            .all(|w| w.teeth.is_none() && w.pinion.is_none()));
        for name in ["31xx", "SH31xx", "DD3120", "VR3120"] {
            assert!(find(name).is_none(), "{name}");
        }
    }

    #[test]
    fn ap3120_clone_simple_idler_closes_both_one_hour_branches() {
        for name in [
            "SA3120",
            "AP3120 Clone",
            "Audemars Piguet 3120 Clone",
            "APS 3120",
            "ZF 3120",
        ] {
            let c = find(name).unwrap();
            assert_eq!(c.calibre, "SA3120");
            let wheel = |n: &str| c.wheels.iter().find(|w| w.name == n).unwrap();
            let idler = wheel("intermediate wheel");
            let barrel = wheel("barrel");
            assert_eq!(idler.teeth, Some(40));
            assert_eq!(idler.pinion, None);
            // This rim-to-rim mesh and the branching minute drive are outside
            // the generic wheel-to-pinion train check.
            let expected_barrel =
                idler.period_s * barrel.teeth.unwrap() as f64 / idler.teeth.unwrap() as f64;
            assert!((expected_barrel - barrel.period_s).abs() < 1e-6);
            assert_eq!(barrel.period_s, 28800.0);
            for role in ["centre wheel", "cannon pinion"] {
                let receiver = wheel(role);
                assert_eq!(receiver.pinion, Some(13));
                assert_eq!(receiver.period_s, 3600.0);
                let expected =
                    idler.period_s * receiver.pinion.unwrap() as f64 / idler.teeth.unwrap() as f64;
                assert!((expected - receiver.period_s).abs() < 1e-6);
            }
            assert_eq!(wheel("fourth wheel").period_s, 60.0);
            assert!(c.wheels.iter().all(|w| w.name != "seconds wheel"));
        }
        let covered: Vec<_> = all()
            .iter()
            .filter(|c| {
                c.sources
                    .iter()
                    .any(|s| s.contains("ap3120-clone-train-wheel-counts.md"))
            })
            .map(|c| c.calibre.as_str())
            .collect();
        assert_eq!(covered, ["SA3120"]);
    }

    #[test]
    fn dd324_indirect_hand_drives_close_and_do_not_leak_to_genuine_patek() {
        for name in ["DD324", "Dandong 324 S C", "SL1258"] {
            let c = find(name).unwrap();
            let wheel = |n: &str| c.wheels.iter().find(|w| w.name == n).unwrap();
            let third = wheel("third wheel");
            let seconds = wheel("seconds pinion");
            let cannon = wheel("cannon pinion");
            // These branches are outside the generic adjacent-train check.
            // Both independently known hand speeds must close through the third.
            assert_eq!(seconds.period_s, 60.0);
            assert_eq!(cannon.period_s, 3600.0);
            assert_eq!(
                third.period_s * seconds.pinion.unwrap() as f64 / third.teeth.unwrap() as f64,
                seconds.period_s
            );
            assert_eq!(
                third.period_s * cannon.teeth.unwrap() as f64 / third.pinion.unwrap() as f64,
                cannon.period_s
            );
            assert_eq!(wheel("fourth wheel").period_s, seconds.period_s);
            assert_eq!(wheel("fourth wheel").teeth, Some(84));
            assert_eq!(wheel("escape wheel").pinion, Some(7));
            assert_eq!(wheel("barrel").period_s, 18600.0);
            assert!(c.wheels.iter().all(|w| w.name != "seconds wheel"));
        }
        assert!(find("SL1258")
            .unwrap()
            .wheels
            .iter()
            .find(|w| w.name == "third wheel")
            .unwrap()
            .basis
            .as_deref()
            .unwrap()
            .contains("INFERRED family transfer"));
        assert_eq!(find("DD324").unwrap().lift_angle_deg, Some(52.0));
        // Peacock's own SL1258 drawings print 53°; the DD324 keeps its 52° setting.
        assert_eq!(find("SL1258").unwrap().lift_angle_deg, Some(53.0));
        let covered: Vec<_> = all()
            .iter()
            .filter(|c| {
                c.sources
                    .iter()
                    .any(|s| s.contains("dd324-train-wheel-counts.md"))
            })
            .map(|c| c.calibre.as_str())
            .collect();
        assert_eq!(covered, ["Dandong 324", "Peacock SL1258"]);
        for name in [
            "324",
            "Patek Philippe 324 S C",
            "324 S QA LU",
            "324 S C FUS",
        ] {
            let c = find(name).unwrap();
            assert_eq!(c.maker, "Patek Philippe");
            assert!(c
                .wheels
                .iter()
                .all(|w| w.teeth.is_none() && w.pinion.is_none()));
        }
    }

    #[test]
    fn finds_by_name_family_and_session_names() {
        let c = find("ETA 2824-2").expect("ETA 2824-2");
        assert_eq!(c.bph, Some(28800));
        assert_eq!(find("eta2824-2").map(|c| &c.calibre), Some(&c.calibre));
        assert_eq!(find("2824-2").map(|c| &c.calibre), Some(&c.calibre));
        assert!(find("3235").is_some());
        assert!(find("").is_none());
        assert!(find("no such calibre").is_none());
    }

    #[test]
    fn variant_rows_say_bph_and_their_old_names_still_resolve() {
        assert!(all().iter().all(|c| !c.calibre.contains("vph")));
        let old = "ETA 2824 (21,600 vph, 53°)";
        let c = find(old).expect("the old name finds the row");
        assert_eq!(c.calibre, "ETA 2824 (21,600 bph, 53°)");
        assert_eq!(renamed(old), Some("ETA 2824 (21,600 bph, 53°)"));
        assert_eq!(renamed("ETA 2824-2"), None);
        assert_eq!(
            renamed("eta 2824 (21600 vph 53°)"),
            Some("ETA 2824 (21,600 bph, 53°)")
        );
        assert_eq!(
            renamed("2824 (21,600 vph, 53°)"),
            Some("ETA 2824 (21,600 bph, 53°)")
        );
        // Without the maker, as a bare name also finds the current row.
        assert_eq!(
            find("2824 (21,600 vph, 53°)").map(|c| c.calibre.as_str()),
            Some("ETA 2824 (21,600 bph, 53°)")
        );
        // Only renamed rows carry former names, and none matches a search.
        assert!(search("vph").is_empty());
    }

    #[test]
    fn contributed_puw_360_train_covers_its_series_only_and_leaves_lift_out() {
        let c = find("PUW 1361").expect("PUW 360 series");
        assert_eq!(c.calibre, "PUW 360");
        assert_eq!(c.bph, Some(18000));
        assert_eq!(c.escape_teeth, Some(15));
        let period = |n: &str| c.wheels.iter().find(|w| w.name == n).map(|w| w.period_s);
        assert_eq!(period("escape wheel"), Some(6.0));
        assert_eq!(period("third wheel"), Some(450.0));
        assert_eq!(period("barrel"), Some(25800.0));
        // No published lift angle: the contributor's estimate stays in the notes.
        assert_eq!(c.lift_angle_deg, None);
        assert!(c.sources[0].contains("/issues/1611"));
        // The contributor's 260 and 460 claims are not transferred.
        assert_eq!(find("PUW 260").map(|c| c.calibre.as_str()), None);
        assert_eq!(find("PUW 461").map(|c| c.calibre.as_str()), None);
        assert_eq!(search("Pforzheimer 360")[0].calibre, "PUW 360");
    }

    #[test]
    fn hangzhou_drawings_give_each_calibre_its_lift_and_beat_rate() {
        let hz: Vec<_> = all()
            .iter()
            .filter(|c| c.maker == "Hangzhou" && c.confidence == "maker's page")
            .collect();
        assert!(hz.len() > 390, "{} Hangzhou rows", hz.len());
        for c in &hz {
            assert!(c.lift_angle_deg.is_some(), "{} has no lift", c.calibre);
            assert!(c.bph.is_some(), "{} has no beat rate", c.calibre);
            assert!(c.sources[0].contains("全升角"), "{}", c.calibre);
        }
        // Read off the drawings: three lift angles in use.
        let lift_bph = |n: &str| find(n).map(|c| (c.lift_angle_deg, c.bph));
        assert_eq!(lift_bph("HZ1A01D"), Some((Some(53.0), Some(28800))));
        assert_eq!(lift_bph("HZ2B00"), Some((Some(53.2), Some(21600))));
        assert_eq!(lift_bph("HZ9011"), Some((Some(47.5), Some(21600))));
        // One row per calibre: the date positions of a 2B00A share it.
        assert_eq!(find("HZ2B00").unwrap().calibre, "Hangzhou HZ2B00A");
        // The 2824-clone 6300 keeps its counted train and takes the
        // drawing's 53°, not the 50° 2824 family figure (Peter, 2026-11-16).
        let c = find("HZ6300A").unwrap();
        assert_eq!(c.calibre, "Hangzhou 6300");
        assert_eq!(c.lift_angle_deg, Some(53.0));
        assert_eq!(c.escape_teeth, Some(20));
        assert_eq!(lift_bph("HZ6T15"), Some((Some(53.0), Some(28800))));
    }

    #[test]
    fn peacock_drawings_give_each_drawn_calibre_its_lift() {
        let drawn: Vec<_> = all()
            .iter()
            .filter(|c| c.sources.iter().any(|s| s.contains("机芯外廓图")))
            .collect();
        assert_eq!(drawn.len(), 67);
        for c in &drawn {
            assert!(c.calibre.starts_with("Peacock SL"), "{}", c.calibre);
            assert!(c.lift_angle_deg.is_some(), "{} has no lift", c.calibre);
        }
        let lift_bph = |n: &str| find(n).map(|c| (c.lift_angle_deg, c.bph));
        assert_eq!(lift_bph("SL1701"), Some((Some(51.0), Some(28800))));
        assert_eq!(lift_bph("SL3032"), Some((Some(51.5), Some(28800))));
        assert_eq!(lift_bph("SL3511"), Some((Some(52.5), Some(28800))));
        assert_eq!(lift_bph("SL5003AT"), Some((Some(53.1), Some(21600))));
        assert_eq!(lift_bph("SL5200"), Some((Some(54.0), Some(21600))));
        assert_eq!(lift_bph("SL5231P"), Some((Some(54.0), Some(28800))));
        // The roman-numeral spelling on the drawing finds the same row.
        assert_eq!(find("SL5720Ⅱ").unwrap().calibre, "Peacock SL5720II");
        // Either spelling finds the SL4617 II and III, whichever the row uses.
        assert_eq!(find("SL4617Ⅱ").unwrap().calibre, "Peacock SL4617II");
        assert_eq!(find("SL4617II").unwrap().calibre, "Peacock SL4617II");
        assert_eq!(find("SL4617III").unwrap().calibre, "Peacock SL4617Ⅲ");
        assert_eq!(find("SL4617Ⅲ").unwrap().calibre, "Peacock SL4617Ⅲ");
        // SL1258 keeps its counted DD324 train and takes the drawing's 53°.
        let c = find("SL1258").unwrap();
        assert_eq!(c.lift_angle_deg, Some(53.0));
        assert_eq!(c.escape_teeth, Some(20));
        // A calibre with no drawing on the site keeps no lift angle.
        assert_eq!(find("SL4609").unwrap().lift_angle_deg, None);
    }

    #[test]
    fn peters_jinghe_list_names_movements_without_moving_figures() {
        let post = "post-12474350";
        let quoted: Vec<_> = all()
            .iter()
            .filter(|c| c.notes.as_deref().is_some_and(|n| n.contains(post)))
            .map(|c| c.calibre.as_str())
            .collect();
        assert_eq!(quoted.len(), 11, "{quoted:?}");
        // The quote identifies; each row keeps its own figures.
        assert_eq!(find("SH4130").unwrap().lift_angle_deg, Some(52.0));
        let jh = find("JH6203").unwrap();
        assert_eq!(jh.calibre, "Jinghe JH6203");
        assert_eq!(jh.lift_angle_deg, None);
        assert_eq!(jh.bph, Some(28800));
        assert!(jh.notes.as_deref().unwrap().contains("46 jewels"));
        assert!(find("JH6501").is_none());
    }

    /// The app links a source's address only when the source starts with
    /// it and a space follows, so an address in the middle of a source, or
    /// one with a colon stuck to its end, shows as plain text.
    #[test]
    fn every_web_source_starts_with_its_address() {
        for c in all() {
            for s in &c.sources {
                if !s.contains("http://") && !s.contains("https://") {
                    continue;
                }
                assert!(s.starts_with("http"), "{}: {s}", c.calibre);
                let url = s.split(' ').next().unwrap_or_default();
                assert!(!url.ends_with([':', ';', ',', '.']), "{}: {s}", c.calibre);
                assert_eq!(url.matches("://").count(), 1, "{}: {s}", c.calibre);
            }
        }
    }

    /// Each Ranfft source's figures agree with its row, unless the row's
    /// notes say Ranfft disagrees.
    #[test]
    fn ranfft_sources_fill_gaps_and_disagreements_are_noted() {
        let mut n = 0;
        for c in all() {
            for s in c
                .sources
                .iter()
                .filter(|s| s.contains("ranfft.org/caliber/") && s.contains("read 2026-11-17"))
            {
                n += 1;
                let says = s
                    .rsplit_once(") (")
                    .map(|(_, r)| r.trim_end_matches(')'))
                    .unwrap_or_default();
                let noted = c.notes.as_deref().is_some_and(|t| t.contains("Ranfft"));
                for part in says.split(", ") {
                    if let Some(b) = part.strip_suffix(" bph") {
                        assert!(noted || c.bph == b.parse().ok(), "{}: {s}", c.calibre);
                    } else if let Some(l) = part.strip_suffix('°') {
                        assert!(
                            noted || c.lift_angle_deg == l.parse().ok(),
                            "{}: {s}",
                            c.calibre
                        );
                    }
                }
            }
        }
        assert_eq!(n, 106);
        let bph = |n: &str| find(n).and_then(|c| c.bph);
        assert_eq!(bph("Arogno 151"), Some(18000));
        assert_eq!(bph("Bifora 49"), Some(18000));
        // Where Ranfft and the lift-angle sheet disagree, no beat rate
        // (Norm, #1869), and no balance period derived from one.
        assert_eq!(bph("Elgin 905"), None);
        assert!(find("Elgin 905").unwrap().wheels.is_empty());
        assert_eq!(bph("Gruen 106"), None);
        // Ranfft's "0°" is a placeholder, not a lift angle.
        assert_eq!(find("Doxa 41").unwrap().lift_angle_deg, None);
    }

    /// Every figure in the table is in a plausible range for a Swiss
    /// lever: a placeholder such as Ranfft's "0°" never lands as a figure.
    #[test]
    fn every_beat_rate_and_lift_angle_is_plausible() {
        for c in all() {
            if let Some(b) = c.bph {
                assert!((3600..=72000).contains(&b), "{}: {b} bph", c.calibre);
            }
            if let Some(l) = c.lift_angle_deg {
                assert!((30.0..=70.0).contains(&l), "{}: {l}°", c.calibre);
            }
        }
    }

    /// A calibre with an escapement Timegrapher does not measure yet says
    /// which one, from the fixed list, with a source that starts with its
    /// address (#1872); the Omega 2500 family is co-axial.
    #[test]
    fn non_lever_escapements_are_named_and_sourced() {
        let mut flagged = 0;
        for c in all() {
            let Some(e) = &c.escapement else { continue };
            flagged += 1;
            assert!(
                ESCAPEMENTS.contains(&e.kind.as_str()),
                "{}: {}",
                c.calibre,
                e.kind
            );
            assert!(e.source.starts_with("https://"), "{}", c.calibre);
        }
        assert_eq!(flagged, 4);
        for n in ["Omega 2500", "Omega 2500A", "Omega 2500B", "Omega 2500C"] {
            let e = find(n).unwrap().escapement.as_ref().unwrap();
            assert_eq!(e.kind, "co-axial", "{n}");
        }
        assert!(find("ETA 2824-2").unwrap().escapement.is_none());
        let e = find("Omega 2500").unwrap().escapement.clone().unwrap();
        assert_eq!(
            e.sentence(),
            "Co-axial escapement: not supported yet. Timegrapher measures Swiss lever \
             escapements only."
        );
    }

    /// The metadata fields (#1861) keep their rules: every sentence,
    /// issue, relation and span of years has a source; relations use the
    /// fixed list and name a real row; assets use the fixed kinds and sit
    /// in a maker/calibre folder of the asset repository.
    #[test]
    fn calibre_metadata_is_sourced_and_well_formed() {
        let names: std::collections::HashSet<_> =
            all().iter().map(|c| c.calibre.as_str()).collect();
        let mut assets = 0;
        for c in all() {
            for s in c.summary.iter().chain(&c.issues) {
                assert!(!s.text.is_empty() && !s.source.is_empty(), "{}", c.calibre);
            }
            if let Some(y) = &c.years {
                assert!(!y.source.is_empty(), "{}", c.calibre);
                assert!(y.from.is_some() || y.to.is_some(), "{}", c.calibre);
            }
            for r in &c.related {
                assert!(
                    RELATIONS.contains(&r.relation.as_str()),
                    "{}: {}",
                    c.calibre,
                    r.relation
                );
                assert!(
                    names.contains(r.calibre.as_str()),
                    "{}: {}",
                    c.calibre,
                    r.calibre
                );
                assert_ne!(r.calibre, c.calibre);
                assert!(!r.source.is_empty(), "{}", c.calibre);
            }
            for a in &c.assets {
                assets += 1;
                assert!(
                    ASSET_KINDS.contains(&a.kind.as_str()),
                    "{}: {}",
                    c.calibre,
                    a.kind
                );
                assert_eq!(a.path.split('/').count(), 3, "{}: {}", c.calibre, a.path);
                assert!(
                    !a.path.starts_with('/') && !a.path.contains(".."),
                    "{}",
                    a.path
                );
            }
        }
        // The makers' files mirrored on 2026-11-17 (timegrapher-calibres#1).
        assert_eq!(assets, 1402);
        let hz = find("HZ36K0E").unwrap();
        assert!(hz.assets.iter().any(|a| a.kind == "spec sheet"));
        assert!(asset_url(&hz.assets[0]).starts_with(
            "https://raw.githubusercontent.com/peterhudson/timegrapher-calibres/main/hangzhou/"
        ));
    }

    #[test]
    fn seagull_manuals_tongji_and_shanghai_rows_resolve() {
        let manual: Vec<_> = all()
            .iter()
            .filter(|c| {
                c.sources
                    .iter()
                    .any(|s| s.contains("Seagull's manual for watch"))
            })
            .collect();
        // 85 rows from the manuals plus five older rows they confirm.
        assert_eq!(manual.len(), 90);
        for c in &manual {
            assert!(c.calibre.starts_with("Seagull ST"), "{}", c.calibre);
            assert_eq!(c.lift_angle_deg, None, "{}", c.calibre);
        }
        let bph = |n: &str| find(n).and_then(|c| c.bph);
        assert_eq!(bph("ST8083"), Some(21600));
        assert_eq!(bph("ST9200"), Some(18000));
        assert_eq!(bph("ST8090"), Some(28800));
        assert_eq!(bph("ST8080-2"), Some(21600));
        // Seagull's own manuals disagree, or print an unusual figure.
        assert_eq!(bph("ST9250"), None);
        assert_eq!(bph("ST8250"), None);
        assert_eq!(find("ST2555").unwrap().confidence, "dealer's page");
        let t = find("Tongji").unwrap();
        assert_eq!(t.bph, Some(21600));
        assert_eq!(find("ZB-1").map(|c| &c.calibre), Some(&t.calibre));
        // The bare 2650 stays with the Swiss rows.
        assert_ne!(find("2650").map(|c| &c.calibre), Some(&t.calibre));
        assert_eq!(bph("SS1A"), Some(18000));
        assert_eq!(bph("SS1K"), Some(21600));
        assert!(find("SS1").is_none());
    }

    #[test]
    fn makers_are_in_the_table_and_their_names_unique() {
        let mut seen = std::collections::HashSet::new();
        for m in makers() {
            assert!(
                all().iter().any(|c| c.maker == m.maker),
                "{} makes nothing in the table",
                m.maker
            );
            for n in std::iter::once(&m.maker).chain(&m.aliases) {
                assert!(seen.insert(key(n)), "maker name {n} is listed twice");
            }
        }
    }

    #[test]
    fn search_takes_maker_abbreviations_and_ranks_exact_names_first() {
        let first = |q: &str| search(q).first().map(|c| c.calibre.clone());
        assert_eq!(first("ETA 2824-2").as_deref(), Some("ETA 2824-2"));
        assert_eq!(first("dd 3235").as_deref(), Some("Dandong 3235"));
        assert_eq!(first("DD3235").as_deref(), Some("Dandong 3235"));
        assert_eq!(first("AP 3120").as_deref(), Some("Audemars Piguet 3120"));
        // The maker's own calibres come before names that only start "AP".
        let ap = all()
            .iter()
            .filter(|c| c.maker == "Audemars Piguet")
            .count();
        assert!(search("AP")[..ap]
            .iter()
            .all(|c| c.maker == "Audemars Piguet"));
        assert!(search("rolex 32").iter().all(|c| c.maker == "Rolex"));
        assert_eq!(search("").len(), all().len());
        assert!(search("no such calibre").is_empty());
    }
}
