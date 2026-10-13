//! What a user adds to the calibre table: turn periods of wheels the table
//! leaves out, and calibres it does not list. They are kept in one JSON
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
        let u: UserCalibres =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if u.schema != SCHEMA {
            return Err(format!(
                "{}: layout {} (this version reads {SCHEMA})",
                path.display(),
                u.schema
            ));
        }
        Ok(u)
    }

    /// Write the file at `path`, creating its folder; a half-written file
    /// never replaces a good one.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let err = |e: std::io::Error| format!("{}: {e}", path.display());
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(err)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text + "\n").map_err(err)?;
        std::fs::rename(&tmp, path).map_err(err)
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

    /// Add a custom calibre, or replace the one of the same name.
    pub fn put_custom(&mut self, c: CustomCalibre) {
        match self.custom.iter_mut().find(|x| x.name == c.name) {
            Some(x) => *x = c,
            None => self.custom.push(c),
        }
    }

    pub fn remove_custom(&mut self, name: &str) {
        self.custom.retain(|c| c.name != name);
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
            Entry::Table(c) => Some(c.bph),
            Entry::Custom(c) => c.bph,
        }
    }

    pub fn lift_angle_deg(&self) -> Option<f64> {
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

    /// The JSON a contribution carries: the calibre and what the user
    /// added to it, nothing else. `None` when there is nothing to send.
    pub fn contribution(&self, e: Entry<'_>, version: &str) -> Option<serde_json::Value> {
        let slots = self.slots(e);
        let added: Vec<&Slot> = slots.iter().filter(|s| s.origin == Origin::User).collect();
        if added.is_empty() && !e.is_custom() {
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
    fn user_periods_fill_blanks_and_replace_shipped_ones() {
        let u = store();
        let cat = Catalog::new(&u);
        let rolex = cat.find("4130").unwrap();
        let slots = cat.slots(rolex);
        let third = slots.iter().find(|s| s.name == "third wheel").unwrap();
        assert_eq!((third.period_s, third.origin), (Some(240.0), Origin::User));
        assert!(slots
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
