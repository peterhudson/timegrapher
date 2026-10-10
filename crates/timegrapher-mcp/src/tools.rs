//! The tools: each one runs a `timegrapher` command with `--json` and
//! hands back its document unchanged (docs/agent-interface.md is the
//! contract). A tool is a table of parameters, each mapped to the
//! command's argument, so the schema the agent sees and the command line
//! built from its call cannot drift apart.

use serde_json::{json, Map, Value};

/// How one parameter reaches the command line.
#[derive(Clone, Copy)]
pub enum Arg {
    /// A path (or several, for an array), in order, before the options.
    Positional,
    /// `--flag VALUE`.
    Flag(&'static str),
}

#[derive(Clone, Copy)]
pub enum Kind {
    Path,
    Paths,
    Integer,
    Number,
    Text,
}

pub struct Param {
    pub name: &'static str,
    pub kind: Kind,
    pub arg: Arg,
    pub required: bool,
    pub description: &'static str,
}

pub struct ToolSpec {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// The command and any fixed arguments, before the parameters.
    pub command: &'static [&'static str],
    pub params: &'static [Param],
    /// Changes nothing on the computer.
    pub read_only: bool,
    /// Flags added when the call leaves them out, as (flag, value); a value
    /// of `~` is the home folder.
    pub defaults: &'static [(&'static str, &'static str)],
}

const fn p(
    name: &'static str,
    kind: Kind,
    arg: Arg,
    required: bool,
    description: &'static str,
) -> Param {
    Param {
        name,
        kind,
        arg,
        required,
        description,
    }
}

const RECORDING: Param = p(
    "recording",
    Kind::Path,
    Arg::Positional,
    true,
    "Full path of the recording (WAV or FLAC) on this computer. Ask the person for it; \
     `~` stands for their home folder.",
);
const RECORDINGS: Param = p(
    "recordings",
    Kind::Paths,
    Arg::Positional,
    true,
    "Full paths of the recording: one file, a folder of segments, or several files read \
     as one continuous recording in the order given.",
);
const BPH: Param = p(
    "bph",
    Kind::Integer,
    Arg::Flag("--bph"),
    false,
    "Beat rate in beats per hour, e.g. 28800 (guessed from the sound if left out).",
);
const LIFT: Param = p(
    "lift_deg",
    Kind::Number,
    Arg::Flag("--lift"),
    false,
    "Lift angle of the calibre in degrees (52 if left out). Amplitude depends on it: \
     find the calibre's angle before quoting amplitude.",
);
const CALIBRE: Param = p(
    "calibre",
    Kind::Text,
    Arg::Flag("--calibre"),
    false,
    "The movement, e.g. \"ETA 2824-2\" or \"Rolex 3235\", to name periodic changes after \
     its wheels. An unknown name gives an error listing the known ones.",
);

pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "analyze_recording",
        title: "Analyse a Recording",
        description: "Measure one recording of a watch in one position: rate (seconds per \
            day, positive gaining), amplitude (degrees), beat error (ms), how steady the \
            beats are, and whether a watch was heard at all. When `watch_heard` is false \
            the readings describe noise: do not show them; check the microphone instead.",
        command: &["analyze"],
        params: &[RECORDING, BPH, LIFT],
        read_only: true,
        defaults: &[],
    },
    ToolSpec {
        name: "run_session",
        title: "Read a Multi-Position Test",
        description: "Read a watch measured in several positions (and states of wind) as \
            one Witschi-style report: per-position readings, the characteristic values \
            (mean rate, largest difference, isochronism) and findings with advice. Give a \
            folder holding session.toml, the session.toml itself, or recordings with the \
            position in each file name (DU, DD, CU, CD, CL, CR). Also writes report.html \
            next to the recordings.",
        command: &["session"],
        params: &[
            p(
                "paths",
                Kind::Paths,
                Arg::Positional,
                true,
                "Full paths: a folder with session.toml, the session.toml, or the \
                 recordings themselves.",
            ),
            BPH,
            p(
                "lift_deg",
                Kind::Number,
                Arg::Flag("--lift"),
                false,
                "Lift angle in degrees (overrides session.toml; 52 if neither gives one).",
            ),
            p(
                "tolerance",
                Kind::Text,
                Arg::Flag("--tolerance"),
                false,
                "Tolerance class: ladies, mens, cosc-small, cosc or metas (mens if left out).",
            ),
            p(
                "settle_s",
                Kind::Number,
                Arg::Flag("--settle"),
                false,
                "Seconds skipped at the start of each recording while the watch settles \
                 (20 if left out).",
            ),
        ],
        read_only: false,
        defaults: &[],
    },
    ToolSpec {
        name: "check_steadiness",
        title: "Check Steadiness",
        description: "Is the watch steady over a long recording (an hour or more)? Gives a \
            verdict per series (rate, amplitude, beat error): steady, periodic, two \
            states, measurement, shifting mean, drifting, wandering or too short, with \
            the evidence, the wheel a periodic change points at, and findings with advice.",
        command: &["series"],
        params: &[
            RECORDINGS,
            BPH,
            LIFT,
            CALIBRE,
            p(
                "reading_s",
                Kind::Number,
                Arg::Flag("--reading"),
                false,
                "Length of each rate reading in seconds (10 if left out).",
            ),
        ],
        read_only: true,
        defaults: &[],
    },
    ToolSpec {
        name: "long_run",
        title: "Analyse a Long Run",
        description: "Rate and amplitude over a long recording (hours to days) and the \
            periodic changes that point at a wheel of the train, named after the calibre's \
            wheels. Also writes report.html and summary.json in a folder next to the \
            recording.",
        command: &["long"],
        params: &[RECORDINGS, BPH, LIFT, CALIBRE],
        read_only: false,
        defaults: &[],
    },
    ToolSpec {
        name: "tick_tock_profile",
        title: "Tick and Tock Profiles",
        description: "The sound of the Tick and the Tock over a stretch of a recording, with \
            the unlock, drop and sound marks the engine measured on them: the place to look \
            when amplitude or beat error seem wrong. In the JSON, `a` is the Tick (even \
            beats) and `b` the Tock (odd beats).",
        command: &["profile"],
        params: &[
            RECORDING,
            p(
                "at_s",
                Kind::Number,
                Arg::Flag("--at"),
                false,
                "Start of the stretch, seconds from the start (0 if left out).",
            ),
            p(
                "span_s",
                Kind::Number,
                Arg::Flag("--span"),
                false,
                "Length of the stretch, seconds (10 if left out).",
            ),
            BPH,
            LIFT,
        ],
        read_only: true,
        defaults: &[],
    },
    ToolSpec {
        name: "beat_shape",
        title: "Beat Shape",
        description: "The three sounds of each side's beat (unlock, impulse, drop): their \
            timing and level for the Tick (even beats) and the Tock (odd beats), the \
            evidence for escapement faults.",
        command: &["shape"],
        params: &[RECORDING, BPH, LIFT],
        read_only: true,
        defaults: &[],
    },
    ToolSpec {
        name: "check_recording",
        title: "Check a Recording",
        description: "Is a recording fit to measure with? Level, clipping, automatic gain, \
            noise and whether ticks are heard, with the settings to change. Reads the file \
            only: it never opens the microphone and changes nothing.",
        command: &["doctor"],
        params: &[
            p(
                "recording",
                Kind::Path,
                Arg::Flag("--file"),
                true,
                "Full path of the recording to check.",
            ),
            BPH,
        ],
        read_only: true,
        defaults: &[],
    },
    ToolSpec {
        name: "list_sound_inputs",
        title: "List Sound Inputs",
        description: "The computer's sound inputs, which is the default, and the sample \
            rates and formats each supports. Lists them only: nothing is recorded.",
        command: &["devices"],
        params: &[],
        read_only: true,
        defaults: &[],
    },
    ToolSpec {
        name: "make_report_bundle",
        title: "Make a Report Bundle",
        description: "Make a zip the person can attach to a GitHub issue when a reading \
            looks wrong or something failed: an excerpt of the recording, what was \
            measured, the system and logs, with home folder, user and host names removed. \
            Nothing is uploaded; the person attaches it themselves. Only the person can \
            say they listened to the excerpt or offer it to the public test corpus, so \
            this tool never does either.",
        command: &["report"],
        params: &[
            p(
                "recording",
                Kind::Path,
                Arg::Positional,
                false,
                "Full path of the recording the report is about, if any.",
            ),
            p(
                "at_s",
                Kind::Number,
                Arg::Flag("--at"),
                false,
                "Start of the excerpt, seconds into the recording (0 if left out).",
            ),
            p(
                "span_s",
                Kind::Number,
                Arg::Flag("--span"),
                false,
                "Length of the excerpt, seconds (300 if left out).",
            ),
            p(
                "kind",
                Kind::Text,
                Arg::Flag("--kind"),
                false,
                "What the report is about: reading (a reading looks wrong), bug, setup or idea.",
            ),
            p(
                "note",
                Kind::Text,
                Arg::Flag("--note"),
                false,
                "What happened, in the person's words.",
            ),
            BPH,
            p(
                "lift_deg",
                Kind::Number,
                Arg::Flag("--lift"),
                false,
                "Lift angle in degrees, as set when the problem showed.",
            ),
            p(
                "out",
                Kind::Path,
                Arg::Flag("--out"),
                false,
                "Folder for the zip (the home folder if left out).",
            ),
        ],
        read_only: false,
        // The CLI's default is the current folder, which for a server the
        // chat app started is nowhere the person would look.
        defaults: &[("--out", "~")],
    },
];

pub fn find(name: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().find(|t| t.name == name)
}

/// The tool's definition as MCP lists it.
pub fn definition(t: &ToolSpec) -> Value {
    let mut props = Map::new();
    let mut required = Vec::new();
    for p in t.params {
        let schema = match p.kind {
            Kind::Path | Kind::Text => json!({"type": "string"}),
            Kind::Paths => json!({"type": "array", "items": {"type": "string"}, "minItems": 1}),
            Kind::Integer => json!({"type": "integer", "minimum": 1}),
            Kind::Number => json!({"type": "number"}),
        };
        let mut schema = schema.as_object().unwrap().clone();
        schema.insert("description".into(), p.description.into());
        props.insert(p.name.into(), Value::Object(schema));
        if p.required {
            required.push(p.name);
        }
    }
    json!({
        "name": t.name,
        "title": t.title,
        "description": t.description,
        "inputSchema": {
            "type": "object",
            "properties": props,
            "required": required,
            "additionalProperties": false,
        },
        "annotations": {
            "title": t.title,
            "readOnlyHint": t.read_only,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": false,
        },
    })
}

/// The command line for a call, after the `timegrapher` program name.
pub fn command_line(t: &ToolSpec, args: &Map<String, Value>) -> Result<Vec<String>, String> {
    if let Some(unknown) = args
        .keys()
        .find(|k| !t.params.iter().any(|p| p.name == k.as_str()))
    {
        return Err(format!("{}: no parameter named {unknown}", t.name));
    }
    let mut positional = Vec::new();
    let mut options = Vec::new();
    for p in t.params {
        let Some(v) = args.get(p.name).filter(|v| !v.is_null()) else {
            if p.required {
                return Err(format!("{}: {} is required", t.name, p.name));
            }
            continue;
        };
        let values: Vec<String> = match (p.kind, v) {
            (Kind::Path, Value::String(s)) => vec![expand_home(s)?],
            (Kind::Paths, Value::Array(a)) if !a.is_empty() => a
                .iter()
                .map(|x| match x {
                    Value::String(s) => expand_home(s),
                    _ => Err(format!("{}: {} must be strings", t.name, p.name)),
                })
                .collect::<Result<_, _>>()?,
            (Kind::Paths, Value::String(s)) => vec![expand_home(s)?],
            (Kind::Text, Value::String(s)) => vec![s.clone()],
            (Kind::Integer, Value::Number(n)) if n.as_u64().is_some_and(|n| n > 0) => {
                vec![n.to_string()]
            }
            (Kind::Number, Value::Number(n)) if n.as_f64().is_some_and(f64::is_finite) => {
                vec![n.to_string()]
            }
            _ => return Err(format!("{}: {} has the wrong type", t.name, p.name)),
        };
        match p.arg {
            Arg::Positional => positional.extend(values),
            Arg::Flag(flag) => {
                for v in values {
                    // `=` keeps a value starting with `-` from reading as a flag.
                    options.push(format!("{flag}={v}"));
                }
            }
        }
    }
    for (flag, value) in t.defaults {
        let prefix = format!("{flag}=");
        if !options.iter().any(|o| o.starts_with(&prefix)) {
            options.push(format!("{prefix}{}", expand_home(value)?));
        }
    }
    let mut line: Vec<String> = t.command.iter().map(|s| s.to_string()).collect();
    line.extend(options);
    line.push("--json".into());
    if !positional.is_empty() {
        line.push("--".into());
        line.extend(positional);
    }
    Ok(line)
}

/// `~` and `~/...` to the home folder. Other paths must be absolute: the
/// server's working folder is whatever the chat app started it in, so a
/// relative path would point somewhere the person never meant.
fn expand_home(s: &str) -> Result<String, String> {
    let home = || {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(std::path::PathBuf::from)
            .ok_or_else(|| "no home folder to expand ~ in".to_string())
    };
    let path = if s == "~" {
        home()?
    } else if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        home()?.join(rest)
    } else {
        std::path::PathBuf::from(s)
    };
    if !path.is_absolute() {
        return Err(format!(
            "{s} is not a full path: ask the person for the whole path (starting with / on \
             a Mac or Linux, a drive letter on Windows, or ~ for their home folder)"
        ));
    }
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn analyze_line() {
        let t = find("analyze_recording").unwrap();
        let line = command_line(
            t,
            &args(json!({"recording": "/w/take.flac", "bph": 28800, "lift_deg": 52.5})),
        )
        .unwrap();
        assert_eq!(
            line,
            [
                "analyze",
                "--bph=28800",
                "--lift=52.5",
                "--json",
                "--",
                "/w/take.flac"
            ]
        );
    }

    #[test]
    fn recording_is_a_flag_for_doctor() {
        let t = find("check_recording").unwrap();
        let line = command_line(t, &args(json!({"recording": "/w/t.wav"}))).unwrap();
        assert_eq!(line, ["doctor", "--file=/w/t.wav", "--json"]);
    }

    #[test]
    fn refuses_bad_calls() {
        let t = find("analyze_recording").unwrap();
        assert!(command_line(t, &args(json!({}))).is_err());
        assert!(command_line(t, &args(json!({"recording": "take.flac"}))).is_err());
        assert!(command_line(t, &args(json!({"recording": "/t", "apply": true}))).is_err());
        assert!(command_line(t, &args(json!({"recording": "/t", "bph": -1}))).is_err());
        assert!(command_line(t, &args(json!({"recording": "/t", "bph": "x"}))).is_err());
    }

    #[test]
    fn no_tool_sets_consent_or_touches_the_mic() {
        for t in TOOLS {
            assert_ne!(t.command[0], "record");
            let names: Vec<_> = t.params.iter().map(|p| p.name).collect();
            for forbidden in ["cc0", "listened_back", "apply", "save", "device", "seconds"] {
                assert!(!names.contains(&forbidden), "{} has {forbidden}", t.name);
            }
            // `doctor` listens live unless it is given a file.
            if t.command[0] == "doctor" {
                assert!(t
                    .params
                    .iter()
                    .any(|p| p.required && matches!(p.arg, Arg::Flag("--file"))));
            }
        }
    }

    #[test]
    fn definitions_are_well_formed() {
        for t in TOOLS {
            let d = definition(t);
            assert_eq!(d["inputSchema"]["type"], "object");
            assert!(d["description"].as_str().unwrap().len() > 40);
        }
    }
}
