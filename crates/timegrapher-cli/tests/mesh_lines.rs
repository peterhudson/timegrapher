//! `long` and `series` list the calibre's meshes, as core's
//! `Catalog::meshes` gives them, in `input.settings.named_meshes`
//! (#1472), and none with no calibre; and the lines to mark for its
//! wheels and meshes as core picks them, in `input.settings.wheel_lines`
//! (#1694).
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Output};
use timegrapher_core::calibre_store::{Catalog, UserCalibres};

struct Fixture(PathBuf);
impl Fixture {
    /// A fixture of its own for each test, `tag`, since tests run at once.
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("tg-mesh-lines-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fixture = Self(dir);
        let out = fixture.run(&["synth", fixture.wav().to_str().unwrap(), "--duration", "40"]);
        assert!(out.status.success(), "{}", stderr(&out));
        fixture
    }
    fn wav(&self) -> PathBuf {
        self.0.join("watch.wav")
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_timegrapher"))
            .env("TIMEGRAPHER_CONFIG_DIR", &self.0)
            .args(args)
            .output()
            .unwrap()
    }
    /// `command`'s `named_meshes`, with `extra` flags.
    fn meshes(&self, command: &str, extra: &[&str]) -> Value {
        self.settings(command, extra)["named_meshes"].clone()
    }
    /// `command`'s `input.settings`, with `extra` flags.
    fn settings(&self, command: &str, extra: &[&str]) -> Value {
        let out_dir = self.0.join(format!("out-{command}-{}", extra.len()));
        let out_dir = out_dir.to_str().unwrap().to_string();
        let wav = self.wav();
        let mut args = vec![command, wav.to_str().unwrap(), "--json"];
        if command == "long" {
            args.extend(["--out", &out_dir]);
        }
        args.extend(extra);
        let out = self.run(&args);
        assert!(out.status.success(), "{command}: {}", stderr(&out));
        let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
        doc["input"]["settings"].clone()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn long_and_series_list_the_calibres_meshes_from_core() {
    let f = Fixture::new("meshes");
    let user = UserCalibres::default();
    let cat = Catalog::new(&user);
    let want = serde_json::to_value(cat.meshes(cat.find("ETA 2824-2").unwrap())).unwrap();
    assert_eq!(want.as_array().map(Vec::len), Some(4), "{want}");
    for command in ["long", "series"] {
        let got = f.meshes(command, &["--calibre", "ETA 2824-2"]);
        assert_eq!(got, want, "{command}");
        assert_eq!(
            got[1]["about"],
            "6.67 s, the time one tooth of the third wheel takes to pass through the fourth pinion's leaves",
            "{command}"
        );
        assert_eq!(
            f.meshes(command, &[]),
            Value::Array(Vec::new()),
            "{command}"
        );
    }
}

/// The lines to mark are core's, for the wheels and meshes listed beside
/// them: the view draws them as given (#1694).
#[test]
fn long_and_series_give_cores_wheel_lines() {
    let f = Fixture::new("lines");
    for command in ["long", "series"] {
        let s = f.settings(command, &["--calibre", "ETA 2824-2"]);
        let list = |k: &str| s[k].as_array().unwrap().clone();
        let text = |v: &Value, k: &str| v[k].as_str().unwrap().to_string();
        let wheels: Vec<timegrapher_core::periodicity::Wheel> = list("named_wheels")
            .iter()
            .map(|w| timegrapher_core::periodicity::Wheel {
                name: text(w, "name"),
                period_s: w["period_s"].as_f64().unwrap(),
            })
            .collect();
        let meshes: Vec<timegrapher_core::train::MeshLine> = list("named_meshes")
            .iter()
            .map(|m| timegrapher_core::train::MeshLine {
                name: text(m, "name"),
                wheel: text(m, "wheel"),
                period_s: m["period_s"].as_f64().unwrap(),
                about: text(m, "about"),
            })
            .collect();
        let want = serde_json::to_value(timegrapher_core::steady_views::wheel_lines(
            &wheels, &meshes,
        ))
        .unwrap();
        assert!(
            want.as_array().is_some_and(|a| a.len() > wheels.len()),
            "{want}"
        );
        assert_eq!(s["wheel_lines"], want, "{command}");
        let turns: Vec<&str> = s["wheel_lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["turn"].as_str().unwrap())
            .collect();
        assert!(
            turns.contains(&"wheel") && turns.contains(&"mesh"),
            "{turns:?}"
        );
    }
}
