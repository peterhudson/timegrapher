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

/// The calibre called `name`: its full name, its name without the
/// maker, one of its family names or a name it had before.
pub fn find(name: &str) -> Option<&'static Calibre> {
    let k = key(name);
    if k.is_empty() {
        return None;
    }
    all().iter().find(|c| {
        key(&c.calibre) == k
            || key(c.calibre.strip_prefix(c.maker.as_str()).unwrap_or("")) == k
            || c.family.iter().any(|f| key(f) == k)
            || had_name(c, &k)
    })
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
            let beat = 3600.0 / bph as f64;
            let by_name = |n: &str| c.wheels.iter().find(|w| w.name == n);
            for w in &c.wheels {
                assert_eq!(w.name, w.name.to_lowercase(), "{}", c.calibre);
                assert!(w.period_s > 0.0, "{} {}", c.calibre, w.name);
            }
            if let Some(b) = by_name("balance") {
                assert!((b.period_s - 2.0 * beat).abs() < 1e-6, "{}", c.calibre);
            }
            // A lever escape wheel moves one tooth per swing, two beats.
            if let (Some(n), Some(e)) = (c.escape_teeth, by_name("escape wheel")) {
                let want = n as f64 * 2.0 * beat;
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
