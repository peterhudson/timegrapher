//! What a user adds to the calibre table: turn periods of wheels the table
//! leaves out, tooth and leaf counts ([`crate::train`]), and calibres it
//! does not list. They are kept in one JSON
//! file whose place the caller chooses (the app keeps it in its settings
//! folder), read over the shipped table so that the app, the command line
//! and the MCP server see the same thing. Nothing here leaves the
//! computer: [`Catalog::contribution_url`] only builds the address of a
//! pre-filled GitHub issue for the person to open and send themselves.

use crate::calibres::{self, key, rank, Calibre};
use crate::periodicity::Wheel;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Layout of the file; bumped when a field changes meaning.
pub const SCHEMA: &str = "timegrapher.user-calibres/1";
/// Where a contribution is sent: the project's issue form for calibre data.
pub const NEW_ISSUE: &str = "https://github.com/peterhudson/timegrapher/issues/new";
/// The issue form a contribution fills in.
pub const ISSUE_TEMPLATE: &str = "calibre.yml";
/// Layout of the JSON a contribution carries.
pub const CONTRIBUTION_SCHEMA: &str = "timegrapher.calibre-contribution/1";

/// The wheels every lever movement has, in the order they are listed.
pub const WHEEL_SLOTS: [&str; 6] = [
    "balance",
    "escape wheel",
    "fourth wheel",
    "third wheel",
    "centre wheel",
    "barrel",
];

/// Everything the user has added.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserCalibres {
    pub schema: String,
    /// Turn periods for calibres in the shipped table.
    #[serde(default)]
    pub wheels: Vec<UserWheel>,
    /// Calibres the table does not list.
    #[serde(default)]
    pub custom: Vec<CustomCalibre>,
    /// Tooth and leaf counts, for calibres in the table or the user's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub counts: Vec<UserCount>,
}

/// The teeth and pinion leaves a user counted on one arbor of a calibre.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserCount {
    /// The calibre's name as the table or the user writes it.
    pub calibre: String,
    /// Lower case, one of [`crate::train::ARBORS`]: "fourth wheel".
    pub arbor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub teeth: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaves: Option<u32>,
}

/// A wheel's turn period the user entered for a calibre in the table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserWheel {
    /// The calibre's name as the table writes it ("Rolex 4130").
    pub calibre: String,
    /// Lower case: "third wheel".
    pub name: String,
    /// One full turn, seconds.
    pub period_s: f64,
    /// Where the figure came from, in the user's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A calibre the user added.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomCalibre {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maker: Option<String>,
    /// Other names it should be found by.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bph: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lift_angle_deg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escape_teeth: Option<u32>,
    #[serde(default)]
    pub wheels: Vec<CustomWheel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// One wheel of a custom calibre.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomWheel {
    pub name: String,
    /// One full turn, seconds.
    pub period_s: f64,
}

/// Where the app keeps the store: `calibres.json` in the settings folder
/// ([`crate::clockstore::config_dir`]), so the command line and the MCP
/// server read what the app saved.
pub fn default_path() -> Option<std::path::PathBuf> {
    crate::clockstore::config_dir().map(|d| d.join("calibres.json"))
}

impl Default for UserCalibres {
    fn default() -> Self {
        UserCalibres {
            schema: SCHEMA.into(),
            wheels: Vec::new(),
            custom: Vec::new(),
            counts: Vec::new(),
        }
    }
}

impl UserCalibres {
    /// Read the file at `path`; a missing file is an empty store.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let mut u: UserCalibres =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if u.schema != SCHEMA {
            return Err(format!(
                "{}: layout {} (this version reads {SCHEMA})",
                path.display(),
                u.schema
            ));
        }
        u.follow_renames();
        Ok(u)
    }

    /// Move periods entered under a name the table has since changed onto
    /// the entry's current name, so they stay with their calibre.
    fn follow_renames(&mut self) {
        for w in &mut self.wheels {
            if let Some(now) = calibres::renamed(&w.calibre) {
                w.calibre = now.into();
            }
        }
        for c in &mut self.counts {
            if let Some(now) = calibres::renamed(&c.calibre) {
                c.calibre = now.into();
            }
        }
    }

    /// Write the file at `path`, creating its folder; a half-written file
    /// never replaces a good one.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let err = |e: std::io::Error| format!("{}: {e}", path.display());
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(err)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        crate::library::write_atomic(path, (text + "\n").as_bytes())
    }

    /// Set the turn of `wheel` on a calibre, table or custom; it replaces
    /// any period the user gave that wheel before.
    pub fn set_wheel(&mut self, calibre: &str, wheel: &str, period_s: f64, note: Option<String>) {
        let wheel = wheel.trim().to_lowercase();
        if let Some(c) = self.custom.iter_mut().find(|c| c.name == calibre) {
            c.wheels.retain(|w| w.name != wheel);
            c.wheels.push(CustomWheel {
                name: wheel,
                period_s,
            });
            return;
        }
        self.wheels
            .retain(|w| !(w.calibre == calibre && w.name == wheel));
        self.wheels.push(UserWheel {
            calibre: calibre.into(),
            name: wheel,
            period_s,
            note,
        });
    }

    /// Forget the period the user gave `wheel` on a calibre.
    pub fn clear_wheel(&mut self, calibre: &str, wheel: &str) {
        if let Some(c) = self.custom.iter_mut().find(|c| c.name == calibre) {
            c.wheels.retain(|w| w.name != wheel);
        }
        self.wheels
            .retain(|w| !(w.calibre == calibre && w.name == wheel));
    }

    /// Set the teeth and pinion leaves counted on `arbor` of a calibre,
    /// table or custom; `None` forgets that count, and with both `None`
    /// the arbor's counts are forgotten. A count of 0 is not kept.
    pub fn set_count(
        &mut self,
        calibre: &str,
        arbor: &str,
        teeth: Option<u32>,
        leaves: Option<u32>,
    ) {
        let arbor = arbor.trim().to_lowercase();
        self.counts
            .retain(|c| !(c.calibre == calibre && c.arbor == arbor));
        let (teeth, leaves) = (teeth.filter(|&n| n > 0), leaves.filter(|&n| n > 0));
        if teeth.is_some() || leaves.is_some() {
            self.counts.push(UserCount {
                calibre: calibre.into(),
                arbor,
                teeth,
                leaves,
            });
        }
    }

    /// Add a custom calibre, or replace the one of the same name.
    pub fn put_custom(&mut self, c: CustomCalibre) {
        match self.custom.iter_mut().find(|x| x.name == c.name) {
            Some(x) => *x = c,
            None => self.custom.push(c),
        }
    }

    pub fn remove_custom(&mut self, name: &str) {
        self.custom.retain(|c| c.name != name);
        self.counts.retain(|c| c.calibre != name);
    }
}

/// A calibre from the table or from the user.
#[derive(Debug, Clone, Copy)]
pub enum Entry<'a> {
    Table(&'static Calibre),
    Custom(&'a CustomCalibre),
}

impl<'a> Entry<'a> {
    pub fn name(&self) -> &'a str {
        match self {
            Entry::Table(c) => &c.calibre,
            Entry::Custom(c) => &c.name,
        }
    }

    pub fn maker(&self) -> Option<&'a str> {
        match self {
            Entry::Table(c) => Some(&c.maker),
            Entry::Custom(c) => c.maker.as_deref(),
        }
    }

    pub fn bph(&self) -> Option<u32> {
        match self {
            Entry::Table(c) => c.bph,
            Entry::Custom(c) => c.bph,
        }
    }

    /// The lift angle it gives, when inside
    /// [`crate::input_domain::LIFT_RANGE_DEG`]: one outside it (a custom
    /// calibre saved before the range, or edited by hand) is set aside,
    /// so a reading takes the default and says the lift is unknown.
    pub fn lift_angle_deg(&self) -> Option<f64> {
        self.stored_lift_deg()
            .filter(|l| crate::input_domain::lift_deg(*l).is_ok())
    }

    /// The lift angle [`Entry::lift_angle_deg`] set aside, if any.
    pub fn lift_set_aside_deg(&self) -> Option<f64> {
        self.stored_lift_deg()
            .filter(|l| crate::input_domain::lift_deg(*l).is_err())
    }

    fn stored_lift_deg(&self) -> Option<f64> {
        match self {
            Entry::Table(c) => c.lift_angle_deg,
            Entry::Custom(c) => c.lift_angle_deg,
        }
    }

    pub fn escape_teeth(&self) -> Option<u32> {
        match self {
            Entry::Table(c) => c.escape_teeth,
            Entry::Custom(c) => c.escape_teeth,
        }
    }

    pub fn is_custom(&self) -> bool {
        matches!(self, Entry::Custom(_))
    }
}

/// Where a wheel's period comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The shipped table.
    Shipped,
    /// The user entered it.
    User,
    /// Worked out from tooth and leaf counts ([`crate::train`]), where
    /// nobody gave the period.
    Counts,
    /// Nobody has given it yet.
    Unknown,
}

/// One wheel of a calibre: its period if anyone knows it, and who said so.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Slot {
    pub name: String,
    pub period_s: Option<f64>,
    pub origin: Origin,
    /// The table's period when the user's replaces it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shipped_period_s: Option<f64>,
    /// How the table's period is known, or the user's note.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basis: Option<String>,
}

/// The shipped table with the user's additions over it.
pub struct Catalog<'a> {
    pub user: &'a UserCalibres,
}

impl<'a> Catalog<'a> {
    pub fn new(user: &'a UserCalibres) -> Self {
        Catalog { user }
    }

    /// The calibre called `name`: a custom one first, then the table.
    pub fn find(&self, name: &str) -> Option<Entry<'a>> {
        let k = key(name);
        if k.is_empty() {
            return None;
        }
        self.user
            .custom
            .iter()
            .find(|c| key(&c.name) == k || c.aliases.iter().any(|a| key(a) == k))
            .map(Entry::Custom)
            .or_else(|| calibres::find(name).map(Entry::Table))
    }

    /// Calibres answering `query`, best first, the user's own among them.
    pub fn search(&self, query: &str) -> Vec<Entry<'a>> {
        let mut hits: Vec<(u32, Entry<'a>)> = self
            .user
            .custom
            .iter()
            .filter_map(|c| {
                rank(
                    query,
                    &c.name,
                    c.maker.as_deref().unwrap_or(""),
                    &c.aliases,
                    true,
                )
                .map(|s| (s, Entry::Custom(c)))
            })
            .chain(calibres::all().iter().filter_map(|c| {
                rank(query, &c.calibre, &c.maker, &c.family, c.common).map(|s| (s, Entry::Table(c)))
            }))
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name().cmp(b.1.name())));
        hits.into_iter().map(|h| h.1).collect()
    }

    /// Every wheel of a calibre: the six every lever movement has, then
    /// any other the table or the user names, each with its period and
    /// where it comes from, or marked unknown.
    pub fn slots(&self, e: Entry<'_>) -> Vec<Slot> {
        let mut slots = self.given_slots(e);
        for w in self.train(e).arbors {
            if let (Some(s), Some(p)) = (slots.iter_mut().find(|s| s.name == w.name), w.counted_s) {
                if s.period_s.is_none() {
                    s.period_s = Some(p);
                    s.origin = Origin::Counts;
                    s.basis = w.how();
                }
            }
        }
        slots
    }

    /// The going train of a calibre worked out from its counts: the
    /// table's, with the user's over them, and the periods the table or
    /// the user gave ([`crate::train::solve`]).
    pub fn train(&self, e: Entry<'_>) -> crate::train::Solved {
        crate::train::solve(e.bph(), &self.train_input(e))
    }

    /// The arbors [`Catalog::train`] works from: the counts and periods
    /// the table and the user gave.
    pub fn train_input(&self, e: Entry<'_>) -> Vec<crate::train::Arbor> {
        let given = self.given_slots(e);
        let mut arbors = match e {
            Entry::Table(c) => crate::train::of_calibre(c),
            Entry::Custom(c) => crate::train::ARBORS
                .iter()
                .map(|&name| crate::train::Arbor {
                    name: name.into(),
                    teeth: (name == "escape wheel").then_some(c.escape_teeth).flatten(),
                    ..Default::default()
                })
                .collect(),
        };
        for a in &mut arbors {
            a.period_s = given
                .iter()
                .find(|s| s.name == a.name)
                .and_then(|s| s.period_s);
            if let Some(u) = self.count(e.name(), &a.name) {
                a.teeth = u.teeth.or(a.teeth);
                a.leaves = u.leaves.or(a.leaves);
            }
        }
        arbors
    }

    /// The counts the user gave `arbor` of `calibre`.
    pub fn count(&self, calibre: &str, arbor: &str) -> Option<&'a UserCount> {
        self.user
            .counts
            .iter()
            .find(|c| c.calibre == calibre && c.arbor == arbor)
    }

    /// The wheels with the periods the table or the user gave.
    fn given_slots(&self, e: Entry<'_>) -> Vec<Slot> {
        let mut slots: Vec<Slot> = WHEEL_SLOTS
            .iter()
            .map(|n| Slot {
                name: (*n).into(),
                period_s: None,
                origin: Origin::Unknown,
                shipped_period_s: None,
                basis: None,
            })
            .collect();
        let mut put = |name: &str, period_s: f64, origin: Origin, basis: Option<String>| {
            let i = match slots.iter().position(|s| s.name == name) {
                Some(i) => i,
                None => {
                    slots.push(Slot {
                        name: name.into(),
                        period_s: None,
                        origin: Origin::Unknown,
                        shipped_period_s: None,
                        basis: None,
                    });
                    slots.len() - 1
                }
            };
            let s = &mut slots[i];
            if origin == Origin::User && s.origin == Origin::Shipped {
                s.shipped_period_s = s.period_s;
            }
            s.period_s = Some(period_s);
            s.origin = origin;
            s.basis = basis;
        };
        match e {
            Entry::Table(c) => {
                for w in &c.wheels {
                    let basis = w.basis.clone().or_else(|| Some(c.confidence.clone()));
                    put(&w.name, w.period_s, Origin::Shipped, basis);
                }
                for w in self.user.wheels.iter().filter(|w| w.calibre == c.calibre) {
                    put(&w.name, w.period_s, Origin::User, w.note.clone());
                }
            }
            Entry::Custom(c) => {
                for w in &c.wheels {
                    put(&w.name, w.period_s, Origin::User, None);
                }
            }
        }
        slots
    }

    /// The wheels known for a calibre, for naming periodic components.
    pub fn wheels(&self, e: Entry<'_>) -> Vec<Wheel> {
        self.slots(e)
            .into_iter()
            .filter_map(|s| {
                s.period_s.map(|p| Wheel {
                    name: s.name,
                    period_s: p,
                })
            })
            .collect()
    }

    /// The meshes known for a calibre, for marking beside its wheels
    /// (#1472): each named by the pinion a wheel's teeth drive, with the
    /// time one tooth takes to pass through its leaves
    /// ([`crate::train::Mesh::line`]), where the counts and a turn give it.
    pub fn meshes(&self, e: Entry<'_>) -> Vec<crate::train::MeshLine> {
        self.train(e)
            .meshes
            .iter()
            .filter_map(crate::train::Mesh::line)
            .collect()
    }

    /// The JSON a contribution carries: the calibre and what the user
    /// added to it, nothing else. `None` when there is nothing to send.
    pub fn contribution(&self, e: Entry<'_>, version: &str) -> Option<serde_json::Value> {
        let slots = self.slots(e);
        let added: Vec<&Slot> = slots.iter().filter(|s| s.origin == Origin::User).collect();
        let counts: Vec<&UserCount> = self
            .user
            .counts
            .iter()
            .filter(|c| c.calibre == e.name())
            .collect();
        if added.is_empty() && counts.is_empty() && !e.is_custom() {
            return None;
        }
        let wheels: Vec<serde_json::Value> = added
            .iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "period_s": s.period_s,
                    "replaces_period_s": s.shipped_period_s,
                    "note": s.basis,
                })
            })
            .collect();
        let mut v = serde_json::json!({
            "schema": CONTRIBUTION_SCHEMA,
            "software": version,
            "calibre": e.name(),
            "in_table": !e.is_custom(),
            "wheels": wheels,
        });
        if !counts.is_empty() {
            v["counts"] = serde_json::to_value(
                counts
                    .iter()
                    .map(|c| serde_json::json!({"arbor": c.arbor, "teeth": c.teeth, "leaves": c.leaves}))
                    .collect::<Vec<_>>(),
            )
            .ok()?;
        }
        if let Entry::Custom(c) = e {
            v["custom"] = serde_json::to_value(c).ok()?;
        }
        Some(v)
    }

    /// The address of a GitHub issue pre-filled with a contribution, for
    /// the person to look over and send; `None` when there is nothing to
    /// send. `version` names the software that made it.
    pub fn contribution_url(&self, e: Entry<'_>, version: &str) -> Option<String> {
        let data = self.contribution(e, version)?;
        let json = serde_json::to_string_pretty(&data).ok()?;
        let title = format!("Calibre data: {}", e.name());
        Some(format!(
            "{NEW_ISSUE}?template={ISSUE_TEMPLATE}&title={}&calibre={}&data={}",
            encode(&title),
            encode(e.name()),
            encode(&json)
        ))
    }
}

/// Percent-encode everything but unreserved characters.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> UserCalibres {
        let mut u = UserCalibres::default();
        u.set_wheel("Rolex 4130", "Third Wheel", 240.0, Some("bench".into()));
        u.set_wheel("Rolex 3235", "escape wheel", 6.0, None);
        u.put_custom(CustomCalibre {
            name: "My Movement".into(),
            maker: Some("Workshop".into()),
            aliases: vec!["MM1".into()],
            bph: Some(21600),
            lift_angle_deg: Some(50.0),
            escape_teeth: None,
            wheels: vec![CustomWheel {
                name: "fourth wheel".into(),
                period_s: 60.0,
            }],
            notes: None,
        });
        u
    }

    #[test]
    fn meshes_are_named_by_their_pinion_from_the_one_train() {
        // ETA 2824-2's counts (#1472): the meshes come from
        // `train::solve`, named as a watchmaker names the pinion.
        let u = UserCalibres::default();
        let cat = Catalog::new(&u);
        let e = cat.find("ETA 2824-2").unwrap();
        let named: Vec<(String, f64)> = cat
            .meshes(e)
            .into_iter()
            .map(|m| (m.name, m.period_s))
            .collect();
        let solved: Vec<(String, f64)> = cat
            .train(e)
            .meshes
            .iter()
            .map(|m| (crate::train::pinion_name(&m.pinion), m.period_s.unwrap()))
            .collect();
        assert_eq!(named, solved);
        let names: Vec<&str> = named.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "escape pinion",
                "fourth pinion",
                "third pinion",
                "centre pinion"
            ]
        );
        // A calibre with a wheel's period but no counts has none.
        let u = store();
        let cat = Catalog::new(&u);
        assert!(cat.meshes(cat.find("My Movement").unwrap()).is_empty());
    }

    #[test]
    fn user_periods_fill_blanks_and_replace_shipped_ones() {
        let mut u = store();
        u.set_wheel("ETA 2892-A2", "third wheel", 240.0, None);
        let cat = Catalog::new(&u);
        let rolex = cat.find("4130").unwrap();
        let slots = cat.slots(rolex);
        let third = slots.iter().find(|s| s.name == "third wheel").unwrap();
        assert_eq!((third.period_s, third.origin), (Some(240.0), Origin::User));
        assert_eq!(third.shipped_period_s, Some(480.0));
        // 4130 now has a complete train. Exercise filling an unknown
        // period separately, on a catalogue entry still missing it.
        let eta = cat.slots(cat.find("ETA 2892-A2").unwrap());
        let eta_third = eta.iter().find(|s| s.name == "third wheel").unwrap();
        assert_eq!(
            (eta_third.period_s, eta_third.origin),
            (Some(240.0), Origin::User)
        );
        assert_eq!(eta_third.shipped_period_s, None);
        assert!(eta
            .iter()
            .any(|s| s.origin == Origin::Unknown && s.period_s.is_none()));
        let esc = cat.slots(cat.find("3235").unwrap());
        let esc = esc.iter().find(|s| s.name == "escape wheel").unwrap();
        assert_eq!(esc.origin, Origin::User);
        assert_eq!((esc.period_s, esc.shipped_period_s), (Some(6.0), Some(5.0)));
        assert!(cat
            .wheels(rolex)
            .iter()
            .any(|w| w.name == "third wheel" && w.period_s == 240.0));
    }

    /// A custom calibre's lift outside the one range is set aside, so a
    /// reading takes the default and says the lift is unknown.
    #[test]
    fn a_custom_lift_outside_the_range_is_set_aside() {
        for (lift, kept) in [
            (5.0, None),
            (95.0, None),
            (10.0, Some(10.0)),
            (44.0, Some(44.0)),
        ] {
            let mut u = store();
            u.custom[0].lift_angle_deg = Some(lift);
            let name = u.custom[0].name.clone();
            let cat = Catalog::new(&u);
            assert_eq!(cat.find(&name).unwrap().lift_angle_deg(), kept, "{lift}");
            let aside = kept.is_none().then_some(lift);
            assert_eq!(
                cat.find(&name).unwrap().lift_set_aside_deg(),
                aside,
                "{lift}"
            );
            let c = crate::take::calibre_named(&name, &u).unwrap();
            assert_eq!(c.lift_angle_deg, kept, "{lift}");
        }
    }

    #[test]
    fn custom_calibres_are_found_and_searched() {
        let u = store();
        let cat = Catalog::new(&u);
        let e = cat.find("mm1").unwrap();
        assert!(e.is_custom());
        assert_eq!((e.bph(), e.lift_angle_deg()), (Some(21600), Some(50.0)));
        assert_eq!(cat.search("my movement")[0].name(), "My Movement");
        assert!(cat.search("rolex").iter().all(|e| !e.is_custom()));
    }

    #[test]
    fn the_store_round_trips_and_a_missing_file_is_empty() {
        let dir = std::env::temp_dir().join(format!("tg-user-calibres-{}", std::process::id()));
        let path = dir.join("calibres.json");
        assert_eq!(UserCalibres::load(&path).unwrap(), UserCalibres::default());
        let u = store();
        u.save(&path).unwrap();
        assert_eq!(UserCalibres::load(&path).unwrap(), u);
        let mut v = u.clone();
        v.clear_wheel("Rolex 4130", "third wheel");
        v.remove_custom("My Movement");
        assert_eq!(v.wheels.len(), 1);
        assert!(v.custom.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_failed_save_keeps_what_was_there_and_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join(format!("tg-user-atomic-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let path = dir.join("calibres.json");
        // A leftover at the old fixed temp name no longer stops a save (#1589).
        std::fs::create_dir_all(dir.join("calibres.json.tmp/stuck")).unwrap();
        let u = store();
        u.save(&path).unwrap();
        assert_eq!(UserCalibres::load(&path).unwrap(), u);
        // A folder in the file's place makes the move fail; what was there
        // stays, and the half-done write is cleared away.
        std::fs::remove_file(&path).unwrap();
        let kept = path.join("kept");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(&kept, "before").unwrap();
        assert!(UserCalibres::default().save(&path).is_err());
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "before");
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["calibres.json", "calibres.json.tmp"]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn periods_saved_under_a_former_name_follow_the_rename() {
        let dir = std::env::temp_dir().join(format!("tg-user-renamed-{}", std::process::id()));
        let path = dir.join("calibres.json");
        let mut u = UserCalibres::default();
        u.set_wheel("ETA 2824 (21,600 vph, 53°)", "third wheel", 75.0, None);
        u.save(&path).unwrap();
        let u = UserCalibres::load(&path).unwrap();
        assert_eq!(u.wheels[0].calibre, "ETA 2824 (21,600 bph, 53°)");
        let cat = Catalog::new(&u);
        let e = cat.find("ETA 2824 (21,600 vph, 53°)").unwrap();
        assert!(cat
            .slots(e)
            .iter()
            .any(|s| s.name == "third wheel" && s.origin == Origin::User));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn counts_fill_blank_periods_and_travel_with_a_contribution() {
        let mut u = store();
        // My Movement: 21,600 bph and a 60 s fourth wheel typed; count the
        // escape and fourth wheels and the third's teeth.
        u.set_count("My Movement", "escape wheel", Some(15), Some(10));
        u.set_count("My Movement", "Fourth Wheel", Some(120), Some(8));
        u.set_count("My Movement", "third wheel", Some(60), None);
        let cat = Catalog::new(&u);
        let e = cat.find("My Movement").unwrap();
        let slots = cat.slots(e);
        let slot = |n: &str| slots.iter().find(|s| s.name == n).unwrap().clone();
        let esc = slot("escape wheel");
        assert_eq!((esc.period_s, esc.origin), (Some(5.0), Origin::Counts));
        assert_eq!(
            esc.basis.as_deref(),
            Some("5 s: the escape wheel's 15 teeth × two beats at 21600 bph")
        );
        assert_eq!(slot("fourth wheel").origin, Origin::User, "typed wins");
        assert_eq!(slot("third wheel").period_s, Some(450.0));
        assert_eq!(slot("centre wheel").origin, Origin::Unknown);
        assert!(cat
            .wheels(e)
            .iter()
            .any(|w| w.name == "third wheel" && w.period_s == 450.0));
        let v = cat.contribution(e, "test").unwrap();
        assert_eq!(v["counts"].as_array().unwrap().len(), 3);
        assert_eq!(v["counts"][1]["arbor"], "fourth wheel");
        assert_eq!(v["counts"][2]["leaves"], serde_json::Value::Null);
        // Counts alone make a table calibre worth sharing.
        let mut t = UserCalibres::default();
        assert!(Catalog::new(&t)
            .contribution(Catalog::new(&t).find("ETA 2892-A2").unwrap(), "test")
            .is_none());
        t.set_count("ETA 2892-A2", "escape wheel", Some(21), None);
        let cat = Catalog::new(&t);
        assert!(cat
            .contribution(cat.find("ETA 2892-A2").unwrap(), "test")
            .is_some());
        // Clearing both counts forgets the arbor; removing the calibre
        // forgets its counts.
        t.set_count("ETA 2892-A2", "escape wheel", None, Some(0));
        assert!(t.counts.is_empty());
        u.remove_custom("My Movement");
        assert!(u.counts.is_empty());
    }

    #[test]
    fn user_counts_replace_the_tables_only_where_given() {
        let mut u = UserCalibres::default();
        u.set_count("ETA 2824-2", "fourth wheel", Some(100), None);
        let cat = Catalog::new(&u);
        let t = cat.train(cat.find("ETA 2824-2").unwrap());
        let fourth = &t.arbors[1];
        assert_eq!((fourth.teeth, fourth.leaves), (Some(100), Some(9)));
        assert_eq!(fourth.typed_s, Some(60.0));
        assert_eq!(fourth.caption().unwrap(), "Teeth give 55.556 s");
        // Only the wheels either side of the changed count read otherwise:
        // the third wheel's period comes from the fourth's given 60 s.
        let differ: Vec<&str> = t
            .arbors
            .iter()
            .filter(|a| a.differs().is_some())
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(differ, ["fourth wheel"]);
    }

    #[test]
    fn a_contribution_carries_only_what_the_user_added() {
        let u = store();
        let cat = Catalog::new(&u);
        let rolex = cat.find("Rolex 4130").unwrap();
        let c = cat.contribution(rolex, "0.1.0").unwrap();
        assert_eq!(c["calibre"], "Rolex 4130");
        assert_eq!(c["wheels"].as_array().unwrap().len(), 1);
        let url = cat.contribution_url(rolex, "0.1.0").unwrap();
        assert!(url.starts_with(NEW_ISSUE));
        assert!(url.contains("template=calibre.yml"));
        assert!(!url.contains(' '));
        let empty = UserCalibres::default();
        let cat = Catalog::new(&empty);
        assert!(cat
            .contribution_url(cat.find("4130").unwrap(), "0.1.0")
            .is_none());
    }
}
