//! Train-wheel turn periods of common calibres.
//!
//! A fault on a wheel repeats once per turn of that wheel, so a periodic
//! change in rate or amplitude can be named once the turn periods of the
//! movement's train are known. The table is `data/train-wheels.json` in
//! this crate; `data/README.md` says how it was built and how far each
//! figure can be trusted. A wheel is left out where its period is not
//! known, rather than guessed.

use crate::periodicity::Wheel;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const DATA: &str = include_str!("../data/train-wheels.json");

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
    pub bph: u32,
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
    pub wheels: Vec<TrainWheel>,
    pub sources: Vec<String>,
    /// How the train periods are known: "published", "derived from
    /// tooth counts", "derived from the beat rate" or "measured".
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

/// Every calibre in the table.
pub fn all() -> &'static [Calibre] {
    static TABLE: OnceLock<Vec<Calibre>> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(DATA).expect("data/train-wheels.json parses"))
}

/// Letters and digits only, lower case, so "ETA 2824-2", "eta2824-2" and
/// "2824 2" compare alike.
fn key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The calibre called `name`: its full name, its name without the
/// maker, or one of its family names.
pub fn find(name: &str) -> Option<&'static Calibre> {
    let k = key(name);
    if k.is_empty() {
        return None;
    }
    all().iter().find(|c| {
        key(&c.calibre) == k
            || key(c.calibre.strip_prefix(c.maker.as_str()).unwrap_or("")) == k
            || c.family.iter().any(|f| key(f) == k)
    })
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
            for n in std::iter::once(&c.calibre).chain(&c.family) {
                assert!(seen.insert(key(n)), "{n} is in the table twice");
            }
            assert!(!c.wheels.is_empty(), "{} has no wheels", c.calibre);
            assert!(!c.sources.is_empty(), "{} has no source", c.calibre);
        }
    }

    #[test]
    fn periods_agree_with_the_beat_rate_and_the_teeth() {
        for c in all() {
            let beat = 3600.0 / c.bph as f64;
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
        assert_eq!(c.bph, 28800);
        assert_eq!(find("eta2824-2").map(|c| &c.calibre), Some(&c.calibre));
        assert_eq!(find("2824-2").map(|c| &c.calibre), Some(&c.calibre));
        assert!(find("3235").is_some());
        assert!(find("").is_none());
        assert!(find("no such calibre").is_none());
    }
}
