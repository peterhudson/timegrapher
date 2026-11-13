//! A going train worked out from its tooth and leaf counts (#399).
//!
//! A watchmaker counts the teeth of each wheel and the leaves of each
//! pinion on the bench or under a microscope; the turn period of every
//! wheel then follows from the beat rate by arithmetic anyone can check:
//!
//! - the balance swings once and back every two beats, `7200 / bph` s;
//! - a lever escape wheel moves one tooth per swing, so it turns in
//!   `teeth × 7200 / bph` s;
//! - a wheel turns more slowly than the pinion it drives by its teeth over
//!   that pinion's leaves, so each arbor outward turns in the next arbor
//!   in's period × its wheel's teeth / that arbor's pinion leaves.
//!
//! These are the rules `data/README.md` sets out for the calibre table,
//! and its unit tests check the table against them through this module.
//! The same counts give the tooth-mesh period, a wheel's turn over its
//! teeth (#1472).
//!
//! What a count can't give is left out, never guessed.

use serde::Serialize;

/// One full swing of the balance and back, two beats, in seconds.
pub fn balance_period_s(bph: u32) -> f64 {
    2.0 * crate::timing::beat_period_s(None, bph)
}

/// A lever escape wheel's turn: one tooth per swing of the balance.
pub fn escape_period_s(bph: u32, teeth: u32) -> f64 {
    f64::from(teeth) * balance_period_s(bph)
}

/// A wheel's period as it is shown and typed: to the thousandth of a
/// second, trailing zeros dropped ("3553.846", "60", "0.25"). A typed
/// period and the counts' period are the same figure when they read
/// alike here.
pub fn period_text(p: f64) -> String {
    let s = format!("{p:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// One arbor of the going train: its wheel and the pinion on the same
/// arbor, as counted, and its turn period where one was typed.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize, Default)]
pub struct Arbor {
    /// Lower case, as the calibre table words it: "escape wheel".
    pub name: String,
    /// Teeth on the wheel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub teeth: Option<u32>,
    /// Leaves of the pinion on the same arbor; none on a barrel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaves: Option<u32>,
    /// One full turn, seconds, where it was typed or the table gives it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_s: Option<f64>,
}

/// Where a period worked out from counts starts.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "arbor")]
pub enum From {
    /// The beat rate and the escape wheel's teeth.
    BeatRate,
    /// The typed period of the arbor named, carried through the counts.
    Period(String),
}

/// The last step of the arithmetic that gave an arbor its period.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Step {
    /// The escape wheel: its teeth × two beats.
    Escape { bph: u32, teeth: u32 },
    /// From the next arbor in (`outward`) or out: its period, the teeth
    /// of the wheel in the mesh between them and the leaves of the pinion.
    Mesh {
        arbor: String,
        period_s: f64,
        teeth: u32,
        leaves: u32,
        /// The wheel is this arbor's, so the period grows by teeth ÷
        /// leaves; else it shrinks by leaves ÷ teeth.
        outward: bool,
    },
}

/// Why the counts don't reach an arbor: the nearer count missing on each
/// side, wheel in and wheel out.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Missing {
    /// The arbor whose count is missing.
    pub arbor: String,
    /// "teeth" or "leaves"; "period" when the counts are whole and there
    /// is neither a beat rate nor a period to start them.
    pub count: &'static str,
}

/// One arbor worked out.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Worked {
    pub name: String,
    pub teeth: Option<u32>,
    pub leaves: Option<u32>,
    /// The period typed for it, if any.
    pub typed_s: Option<f64>,
    /// Its period from the counts, if they reach it.
    pub counted_s: Option<f64>,
    /// Where `counted_s` starts.
    pub from: Option<From>,
    /// The last step to `counted_s`.
    pub step: Option<Step>,
    /// With no period at all, typed or counted: the count that would
    /// carry one here from the nearest arbor with one, or the escape
    /// wheel's teeth.
    pub needs: Option<Missing>,
}

impl Worked {
    /// The period to show: the typed one, else the counted one.
    pub fn period_s(&self) -> Option<f64> {
        self.typed_s.or(self.counted_s)
    }

    /// The typed period and the counts' period read differently
    /// ([`period_text`]): `Some((typed, counted))`.
    pub fn differs(&self) -> Option<(f64, f64)> {
        match (self.typed_s, self.counted_s) {
            (Some(t), Some(c)) if period_text(t) != period_text(c) => Some((t, c)),
            _ => None,
        }
    }

    /// The caption under a wheel's period where the teeth have a say:
    /// "From Teeth" when they gave it, "Teeth give 37.5 s" when a given
    /// period reads otherwise; `None` otherwise.
    pub fn caption(&self) -> Option<String> {
        match (self.typed_s, self.counted_s) {
            (None, Some(_)) => Some("From Teeth".into()),
            _ => self
                .differs()
                .map(|(_, c)| format!("Teeth give {} s", period_text(c))),
        }
    }

    /// The arithmetic behind `counted_s`, in words, each count named by
    /// its owner: "450 s: the fourth wheel's 60 s × the third wheel's 60
    /// teeth ÷ the fourth pinion's 8 leaves".
    pub fn how(&self) -> Option<String> {
        let p = period_text(self.counted_s?);
        Some(match self.step.as_ref()? {
            Step::Escape { bph, teeth } => {
                format!(
                    "{p} s: the {}'s {teeth} teeth × two beats at {bph} bph",
                    self.name
                )
            }
            Step::Mesh {
                arbor,
                period_s,
                teeth,
                leaves,
                outward,
            } => {
                let q = period_text(*period_s);
                // Outward, this arbor's wheel drives the pinion of the one
                // in; inward, the wheel is the other arbor's.
                let (wheel, pinion) = if *outward {
                    (self.name.as_str(), arbor.as_str())
                } else {
                    (arbor.as_str(), self.name.as_str())
                };
                let teeth = format!("the {wheel}'s {teeth} teeth");
                let leaves = format!("the {}'s {leaves} leaves", pinion_name(pinion));
                if *outward {
                    format!("{p} s: the {arbor}'s {q} s × {teeth} ÷ {leaves}")
                } else {
                    format!("{p} s: the {arbor}'s {q} s × {leaves} ÷ {teeth}")
                }
            }
        })
    }

    /// Why the counts don't reach this arbor, in words: "Needs the fourth
    /// wheel's teeth"; `None` where they do.
    pub fn needs_text(&self) -> Option<String> {
        let m = self.needs.as_ref()?;
        Some(match m.count {
            "period" => "Needs the beat rate or a wheel's period".into(),
            "leaves" => format!("Needs the leaves of the {}", pinion_name(&m.arbor)),
            _ => format!("Needs the {}'s teeth", m.arbor),
        })
    }
}

/// The pinion on an arbor, as a watchmaker names it: "escape pinion",
/// "fourth pinion", "centre pinion".
pub fn pinion_name(arbor: &str) -> String {
    let stem = arbor.strip_suffix(" wheel").unwrap_or(arbor);
    format!("{stem} pinion")
}

/// Where a wheel's teeth drive the next pinion in: one tooth engages,
/// drives and leaves every `period_s`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Mesh {
    /// The driving wheel's arbor.
    pub wheel: String,
    /// The driven pinion's arbor.
    pub pinion: String,
    pub teeth: u32,
    pub leaves: u32,
    /// The wheel's turn over its teeth, which is the pinion's turn over
    /// its leaves; `None` where neither turn is known.
    pub period_s: Option<f64>,
}

/// A train worked out from its counts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Solved {
    /// The beat rate given.
    pub bph: Option<u32>,
    /// With no beat rate given: the beat rate the escape wheel's teeth
    /// and its turn (typed, or carried from another arbor's) give.
    pub bph_from_counts: Option<f64>,
    /// The arbors, escape wheel first, as given.
    pub arbors: Vec<Worked>,
    /// Each meshing pair whose teeth and leaves are both counted, escape
    /// pinion first.
    pub meshes: Vec<Mesh>,
}

/// Work out a train. `train` runs outward from the escape wheel: arbor
/// `i`'s pinion is driven by arbor `i + 1`'s wheel. The beat rate (with
/// the escape wheel's teeth) and each typed period start the counts; each
/// arbor's `counted_s` comes from the start nearest it in meshes, the one
/// further in on a tie, and never from its own typed period. So a wrong
/// count shows only on the wheels either side of it. A count or period of
/// 0 is taken as not known.
pub fn solve(bph: Option<u32>, train: &[Arbor]) -> Solved {
    let bph = bph.filter(|&b| b > 0);
    let clean = |v: Option<u32>| v.filter(|&n| n > 0);
    let mut arbors: Vec<Worked> = train
        .iter()
        .map(|a| Worked {
            name: a.name.clone(),
            teeth: clean(a.teeth),
            leaves: clean(a.leaves),
            typed_s: a.period_s.filter(|p| p.is_finite() && *p > 0.0),
            counted_s: None,
            from: None,
            step: None,
            needs: None,
        })
        .collect();
    // How many meshes each counted period was carried over: a period
    // comes from the nearest start, so a wrong count shows only on the
    // wheels either side of it.
    let mut far = vec![usize::MAX; arbors.len()];
    if let (Some(b), Some(first)) = (bph, arbors.first()) {
        if let Some(t) = first.teeth {
            let p = escape_period_s(b, t);
            arbors[0].counted_s = Some(p);
            arbors[0].from = Some(From::BeatRate);
            arbors[0].step = Some(Step::Escape { bph: b, teeth: t });
            far[0] = 0;
            carry(&mut arbors, &mut far, 0, p, &From::BeatRate);
        }
    }
    for i in 0..arbors.len() {
        if let Some(p) = arbors[i].typed_s {
            let from = From::Period(arbors[i].name.clone());
            carry(&mut arbors, &mut far, i, p, &from);
        }
    }
    for i in 0..arbors.len() {
        if arbors[i].period_s().is_none() {
            arbors[i].needs = needs(&arbors, i, bph.is_some());
        }
    }
    // The escape wheel's period falls as 1 / bph, so the beat rate that
    // turns it in `p` is its period at 1 bph over `p`.
    let bph_from_counts = match (bph, arbors.first()) {
        (None, Some(e)) => e
            .teeth
            .zip(e.period_s())
            .map(|(t, p)| escape_period_s(1, t) / p),
        _ => None,
    };
    let meshes = meshes(&arbors);
    Solved {
        bph,
        bph_from_counts,
        arbors,
        meshes,
    }
}

/// Carry period `p` of arbor `at` through the counts both ways, setting
/// each arbor's `counted_s` where this start is nearer, in meshes, than
/// any before it (`far`); a tie keeps the earlier start, the one further
/// in. `at`'s own is left as it is.
fn carry(arbors: &mut [Worked], far: &mut [usize], at: usize, p: f64, from: &From) {
    let mut put = |arbors: &mut [Worked], to: usize, via: usize, out: f64, outward: bool| {
        let (wheel, pinion) = if outward { (to, via) } else { (via, to) };
        let (Some(teeth), Some(leaves)) = (arbors[wheel].teeth, arbors[pinion].leaves) else {
            return None;
        };
        let ratio = f64::from(teeth) / f64::from(leaves);
        let next = if outward { out * ratio } else { out / ratio };
        let meshes = to.abs_diff(at);
        if meshes < far[to] {
            far[to] = meshes;
            let via_name = arbors[via].name.clone();
            let w = &mut arbors[to];
            w.counted_s = Some(next);
            w.from = Some(from.clone());
            w.step = Some(Step::Mesh {
                arbor: via_name,
                period_s: out,
                teeth,
                leaves,
                outward,
            });
        }
        Some(next)
    };
    let mut out = p;
    for i in at + 1..arbors.len() {
        match put(arbors, i, i - 1, out, true) {
            Some(next) => out = next,
            None => break,
        }
    }
    let mut back = p;
    for i in (0..at).rev() {
        match put(arbors, i, i + 1, back, false) {
            Some(next) => back = next,
            None => break,
        }
    }
}

/// The count that would reach arbor `i`: the gap on the way in from the
/// escape wheel (whose teeth start the counts when there is a beat rate),
/// or, failing that, on the way from the nearest arbor out with a period.
fn needs(arbors: &[Worked], i: usize, bph: bool) -> Option<Missing> {
    let gap = |j: usize| {
        // The mesh between arbor j (pinion) and j + 1 (wheel).
        if arbors[j].leaves.is_none() {
            Some(Missing {
                arbor: arbors[j].name.clone(),
                count: "leaves",
            })
        } else if arbors[j + 1].teeth.is_none() {
            Some(Missing {
                arbor: arbors[j + 1].name.clone(),
                count: "teeth",
            })
        } else {
            None
        }
    };
    if bph && arbors[0].teeth.is_none() {
        return Some(Missing {
            arbor: arbors[0].name.clone(),
            count: "teeth",
        });
    }
    // The nearest gap on the way in, then on the way out; with none, the
    // counts are whole and lack only a start.
    (0..i)
        .rev()
        .find_map(gap)
        .or_else(|| (i..arbors.len().saturating_sub(1)).find_map(gap))
        .or(Some(Missing {
            arbor: arbors[0].name.clone(),
            count: "period",
        }))
}

fn meshes(arbors: &[Worked]) -> Vec<Mesh> {
    arbors
        .windows(2)
        .filter_map(|w| {
            let (pinion, wheel) = (&w[0], &w[1]);
            let (leaves, teeth) = (pinion.leaves?, wheel.teeth?);
            let period_s = wheel
                .period_s()
                .map(|p| p / f64::from(teeth))
                .or_else(|| pinion.period_s().map(|p| p / f64::from(leaves)));
            Some(Mesh {
                wheel: wheel.name.clone(),
                pinion: pinion.name.clone(),
                teeth,
                leaves,
                period_s,
            })
        })
        .collect()
}

/// The going train's arbors as the calibre table names them, escape wheel
/// outward: the five the calculator asks for.
pub const ARBORS: [&str; 5] = [
    "escape wheel",
    "fourth wheel",
    "third wheel",
    "centre wheel",
    "barrel",
];

/// A calibre's going train as [`ARBORS`] names it, with the table's
/// counts and periods; an arbor the table leaves out is blank. The
/// escape wheel's teeth come from the calibre's `escape_teeth` where its
/// wheel carries none.
pub fn of_calibre(c: &crate::calibres::Calibre) -> Vec<Arbor> {
    ARBORS
        .iter()
        .map(|&name| {
            let w = c.wheels.iter().find(|w| w.name == name);
            let teeth = w.and_then(|w| w.teeth);
            Arbor {
                name: name.into(),
                teeth: if name == "escape wheel" {
                    teeth.or(c.escape_teeth)
                } else {
                    teeth
                },
                leaves: w.and_then(|w| w.pinion),
                period_s: w.map(|w| w.period_s),
            }
        })
        .collect()
}

/// A count as typed: the wheel's teeth and the pinion's leaves, either
/// left out.
pub type Count = (Option<u32>, Option<u32>);

/// Lay counts and periods typed for one solve over a train: a count or
/// period given replaces the train's, one left out keeps it. Every arbor
/// of [`ARBORS`] the train lacks is added blank, in its place, so that two
/// arbors are only ever taken to mesh when they are neighbours there.
pub fn overlay(train: &mut Vec<Arbor>, counts: &[(&str, Count)], periods: &[(&str, f64)]) {
    for name in ARBORS {
        if !train.iter().any(|a| a.name == name) {
            train.push(Arbor {
                name: name.into(),
                ..Default::default()
            });
        }
    }
    let rank = |n: &str| ARBORS.iter().position(|&a| a == n).unwrap_or(ARBORS.len());
    train.sort_by_key(|a| rank(&a.name));
    for a in train.iter_mut() {
        for (_, (t, l)) in counts.iter().filter(|c| c.0 == a.name) {
            a.teeth = t.or(a.teeth);
            a.leaves = l.or(a.leaves);
        }
        for (_, p) in periods.iter().filter(|p| p.0 == a.name) {
            a.period_s = Some(*p);
        }
    }
}

/// The arbor `word` names, as [`ARBORS`] writes it: "fourth", "Fourth
/// Wheel" and "4th" all name "fourth wheel"; "center" names the centre.
pub fn arbor_named(word: &str) -> Option<&'static str> {
    let k = crate::calibres::key(word);
    let k = k.strip_suffix("wheel").unwrap_or(&k);
    match k {
        "escape" => Some(ARBORS[0]),
        "fourth" | "4th" => Some(ARBORS[1]),
        "third" | "3rd" => Some(ARBORS[2]),
        "centre" | "center" => Some(ARBORS[3]),
        "barrel" => Some(ARBORS[4]),
        _ => None,
    }
}

/// A count as typed, "TEETH/LEAVES": "108/9", "101" (teeth alone), "/9"
/// (leaves alone).
pub fn parse_count(text: &str) -> Result<Count, String> {
    let (t, l) = text.split_once('/').unwrap_or((text, ""));
    let n = |s: &str| -> Result<Option<u32>, String> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(None);
        }
        match s.parse::<u32>() {
            Ok(v) if v > 0 => Ok(Some(v)),
            _ => Err(format!(
                "'{text}': a count is whole teeth, then / and whole leaves, e.g. 108/9"
            )),
        }
    };
    Ok((n(t)?, n(l)?))
}

/// Typed periods as "ARBOR=SECONDS", several split by commas:
/// "fourth=60, centre=3600".
pub fn parse_periods(text: &str) -> Result<Vec<(&'static str, f64)>, String> {
    text.split(',')
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            let bad = || format!("'{}': a period is ARBOR=SECONDS, e.g. fourth=60", p.trim());
            let (a, v) = p.split_once('=').ok_or_else(bad)?;
            let arbor = arbor_named(a)
                .ok_or_else(|| format!("'{}' is not an arbor: {}", a.trim(), ARBORS.join(", ")))?;
            let v: f64 = v.trim().parse().map_err(|_| bad())?;
            if v.is_finite() && v > 0.0 {
                Ok((arbor, v))
            } else {
                Err(bad())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arbor(name: &str, teeth: u32, leaves: u32, period_s: Option<f64>) -> Arbor {
        Arbor {
            name: name.into(),
            teeth: Some(teeth).filter(|&t| t > 0),
            leaves: Some(leaves).filter(|&l| l > 0),
            period_s,
        }
    }

    /// Unitas 6497-2, the one train published whole (data/README.md):
    /// escape 15 on a 10-leaf pinion, fourth 120/8, third 60/10, centre 80,
    /// at 21,600 bph.
    fn unitas() -> Vec<Arbor> {
        vec![
            arbor("escape wheel", 15, 10, None),
            arbor("fourth wheel", 120, 8, None),
            arbor("third wheel", 60, 10, None),
            arbor("centre wheel", 80, 0, None),
        ]
    }

    #[test]
    fn the_unitas_train_turns_its_hands_on_time() {
        let s = solve(Some(21_600), &unitas());
        let p: Vec<f64> = s.arbors.iter().map(|a| a.counted_s.unwrap()).collect();
        // 15 × 1/3 s = 5 s; × 120/10 = 60 s; × 60/8 = 450 s; × 80/10.
        let want = [5.0, 60.0, 450.0, 3600.0];
        for (p, w) in p.iter().zip(want) {
            assert!((p - w).abs() < 1e-9 * w, "{p} {w}");
        }
        assert!(s.arbors.iter().all(|a| a.from == Some(From::BeatRate)));
        assert_eq!(s.bph_from_counts, None);
    }

    #[test]
    fn a_typed_period_fills_the_blanks_the_beat_rate_cannot_reach() {
        // No escape count: the beat rate reaches nothing. The 2824's
        // fourth wheel carries the seconds hand, 60 s.
        let train = vec![
            arbor("escape wheel", 0, 7, None),
            arbor("fourth wheel", 80, 9, Some(60.0)),
            arbor("third wheel", 75, 8, None),
        ];
        let s = solve(Some(28_800), &train);
        assert_eq!(s.arbors[0].counted_s, Some(60.0 * 7.0 / 80.0));
        assert_eq!(s.arbors[1].counted_s, None, "nothing else reaches it");
        assert!((s.arbors[2].counted_s.unwrap() - 500.0).abs() < 1e-9);
        let from = Some(From::Period("fourth wheel".into()));
        assert_eq!(s.arbors[2].from, from);
        assert_eq!(s.arbors[1].period_s(), Some(60.0));
    }

    #[test]
    fn counts_and_a_typed_period_give_the_beat_rate() {
        let mut train = unitas();
        train[3].period_s = Some(3600.0);
        let s = solve(None, &train);
        assert_eq!(s.bph_from_counts, Some(21_600.0));
        assert_eq!(s.arbors[0].counted_s, Some(5.0));
    }

    #[test]
    fn a_typed_period_the_counts_disagree_with_is_shown_beside_them() {
        let mut train = unitas();
        train[1].period_s = Some(72.0);
        let s = solve(Some(21_600), &train);
        assert_eq!(s.arbors[1].differs(), Some((72.0, 60.0)));
        assert_eq!(s.arbors[1].period_s(), Some(72.0));
        // A period that reads the same to the thousandth agrees.
        train[1].period_s = Some(60.0004);
        assert_eq!(solve(Some(21_600), &train).arbors[1].differs(), None);
        train[1].period_s = Some(60.001);
        let s = solve(Some(21_600), &train);
        assert!(s.arbors[1].differs().is_some());
        assert_eq!(s.arbors[1].caption().unwrap(), "Teeth give 60 s");
        assert_eq!(s.arbors[0].caption().unwrap(), "From Teeth");
        assert_eq!(s.arbors[2].caption().unwrap(), "From Teeth");
    }

    #[test]
    fn a_gap_in_the_counts_stops_the_carry() {
        let mut train = unitas();
        train[1].leaves = None;
        let s = solve(Some(21_600), &train);
        assert_eq!(s.arbors[1].counted_s, Some(60.0));
        assert_eq!(s.arbors[2].counted_s, None);
        assert_eq!(s.arbors[3].counted_s, None);
        assert_eq!(
            s.meshes.len(),
            2,
            "no fourth-pinion mesh without its leaves"
        );
    }

    #[test]
    fn meshes_are_a_wheels_turn_over_its_teeth() {
        // ETA 2824-2 from the table (#1472): the escape, fourth, third and
        // centre pinions mesh every 0.56 s, 6.7 s, 46 s and 209 s.
        let c = crate::calibres::find("ETA 2824-2").unwrap();
        let s = solve(c.bph, &of_calibre(c));
        let m: Vec<(&str, f64)> = s
            .meshes
            .iter()
            .map(|m| (m.pinion.as_str(), m.period_s.unwrap()))
            .collect();
        let want = [
            ("escape wheel", 5.0 / 9.0),
            ("fourth wheel", 60.0 / 9.0),
            ("third wheel", 600.0 / 13.0),
            ("centre wheel", 46200.0 / 13.0 / 17.0),
        ];
        assert_eq!(m.len(), want.len());
        for ((pin, p), (wpin, w)) in m.iter().zip(want) {
            assert_eq!(*pin, wpin);
            assert!((p - w).abs() < 1e-9 * w, "{pin}: {p} s, want {w} s");
        }
        assert_eq!(s.meshes[1].wheel, "third wheel");
    }

    #[test]
    fn the_arithmetic_is_given_in_words() {
        let train = vec![
            arbor("escape wheel", 15, 8, None),
            arbor("fourth wheel", 80, 0, None),
        ];
        let s = solve(Some(28_800), &train);
        assert_eq!(
            s.arbors[0].how().unwrap(),
            "3.75 s: the escape wheel's 15 teeth × two beats at 28800 bph"
        );
        assert_eq!(
            s.arbors[1].how().unwrap(),
            "37.5 s: the escape wheel's 3.75 s × the fourth wheel's 80 teeth ÷ the escape \
             pinion's 8 leaves"
        );
        let back = solve(
            None,
            &[
                arbor("escape wheel", 15, 8, None),
                arbor("fourth wheel", 80, 0, Some(37.5)),
            ],
        );
        assert_eq!(
            back.arbors[0].how().unwrap(),
            "3.75 s: the fourth wheel's 37.5 s × the escape pinion's 8 leaves ÷ the fourth \
             wheel's 80 teeth"
        );
        assert_eq!(back.bph_from_counts, Some(28_800.0));
    }

    #[test]
    fn a_blank_names_the_count_it_needs() {
        let mut train = unitas();
        train[2].teeth = None;
        let s = solve(Some(21_600), &train);
        let needs: Vec<Option<String>> = s.arbors.iter().map(Worked::needs_text).collect();
        assert_eq!(
            needs,
            vec![
                None,
                None,
                Some("Needs the third wheel's teeth".into()),
                Some("Needs the third wheel's teeth".into()),
            ]
        );
        train[2].teeth = Some(60);
        train[1].leaves = None;
        let s = solve(Some(21_600), &train);
        assert_eq!(
            s.arbors[3].needs_text().unwrap(),
            "Needs the leaves of the fourth pinion"
        );
        train[0].teeth = None;
        let s = solve(Some(21_600), &train);
        assert_eq!(
            s.arbors[1].needs_text().unwrap(),
            "Needs the escape wheel's teeth"
        );
        let s = solve(None, &unitas());
        assert_eq!(
            s.arbors[1].needs_text().unwrap(),
            "Needs the beat rate or a wheel's period"
        );
    }

    #[test]
    fn periods_read_to_the_thousandth() {
        assert_eq!(period_text(46200.0 / 13.0), "3553.846");
        assert_eq!(period_text(60.0), "60");
        assert_eq!(period_text(0.25), "0.25");
    }

    #[test]
    fn zero_counts_and_periods_are_not_known() {
        let train = vec![Arbor {
            name: "escape wheel".into(),
            teeth: Some(0),
            leaves: Some(0),
            period_s: Some(0.0),
        }];
        let s = solve(Some(0), &train);
        assert_eq!(s.bph, None);
        assert_eq!(s.arbors[0].teeth, None);
        assert_eq!(s.arbors[0].period_s(), None);
        assert!(s.meshes.is_empty());
    }

    #[test]
    fn a_calibres_train_comes_from_the_table() {
        let c = crate::calibres::find("Unitas 6497-2").unwrap();
        let t = of_calibre(c);
        assert_eq!(t.len(), ARBORS.len());
        assert_eq!(t[0].teeth, Some(15));
        assert_eq!(t[1].teeth, Some(120));
        assert_eq!(t[1].leaves, Some(8));
    }

    #[test]
    fn every_table_train_agrees_with_its_counts() {
        let mut checked = 0;
        for c in crate::calibres::all() {
            let s = solve(c.bph, &of_calibre(c));
            for a in &s.arbors {
                if let (Some(t), Some(n)) = (a.typed_s, a.counted_s) {
                    assert!(
                        (t - n).abs() < 1e-9 * t,
                        "{}: {} {t} s, counts give {n} s",
                        c.calibre,
                        a.name
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 100, "{checked}");
    }

    #[test]
    fn an_overlay_replaces_only_what_it_gives() {
        let mut t = unitas();
        overlay(
            &mut t,
            &[
                ("fourth wheel", (Some(108), None)),
                ("barrel", (Some(101), None)),
            ],
            &[("centre wheel", 3600.0)],
        );
        assert_eq!((t[1].teeth, t[1].leaves), (Some(108), Some(8)));
        assert_eq!(t[3].period_s, Some(3600.0));
        assert_eq!(t.last().unwrap().name, "barrel");
        assert_eq!(t.len(), ARBORS.len());
        let mut empty = Vec::new();
        overlay(
            &mut empty,
            &[("third wheel", (Some(90), Some(13)))],
            &[("escape wheel", 5.0)],
        );
        let names: Vec<&str> = empty.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ARBORS);
        assert_eq!(
            solve(None, &empty).arbors[2].counted_s,
            None,
            "no mesh skips the fourth"
        );
    }

    #[test]
    fn typed_counts_and_periods_parse() {
        assert_eq!(arbor_named("Fourth Wheel"), Some("fourth wheel"));
        assert_eq!(arbor_named("center"), Some("centre wheel"));
        assert_eq!(arbor_named("pallet"), None);
        assert_eq!(parse_count("108/9"), Ok((Some(108), Some(9))));
        assert_eq!(parse_count("101"), Ok((Some(101), None)));
        assert_eq!(parse_count(" /9"), Ok((None, Some(9))));
        assert!(parse_count("0/9").is_err());
        assert!(parse_count("ten").is_err());
        assert_eq!(
            parse_periods("fourth=60, centre = 3600"),
            Ok(vec![("fourth wheel", 60.0), ("centre wheel", 3600.0)])
        );
        assert!(parse_periods("fourth").is_err());
        assert!(parse_periods("pallet=1").is_err());
        assert!(parse_periods("fourth=-1").is_err());
    }

    #[test]
    fn periods_follow_the_beat_rate() {
        assert_eq!(balance_period_s(28_800), 0.25);
        assert_eq!(escape_period_s(28_800, 20), 5.0);
        assert_eq!(escape_period_s(21_600, 15), 5.0);
    }
}
