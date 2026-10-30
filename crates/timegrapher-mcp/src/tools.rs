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
    /// Read by the server itself; never on the command line.
    Server,
}

#[derive(Clone, Copy)]
pub enum Kind {
    /// One of a fixed list of words.
    Choice(&'static [&'static str]),
    Path,
    Paths,
    /// Positive BPH representable by the CLI's u32.
    BeatRate,
    /// Finite positive lift angle; no empirical upper limit.
    LiftAngle,
    Number,
    /// Finite, positive acquisition seconds, checked before child spawn.
    CaptureSeconds,
    Text,
    /// `true` adds the bare flag; `false` or left out adds nothing.
    Switch,
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
    /// Calling it again with the same arguments leaves things as one call
    /// would: false for the tools that add a new take or a new file each time.
    pub idempotent: bool,
    /// The chat app shows the charts view beside the result.
    pub charts: bool,
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
const LIBRARY: Param = p(
    "library",
    Kind::Path,
    Arg::Flag("--library"),
    false,
    "Full path of the watch library the app keeps (the app's own if left out).",
);
const BPH: Param = p(
    "bph",
    Kind::BeatRate,
    Arg::Flag("--bph"),
    false,
    "Beat rate in beats per hour, e.g. 28800 (guessed from the sound if left out).",
);
const LIFT: Param = p(
    "lift_deg",
    Kind::LiftAngle,
    Arg::Flag("--lift"),
    false,
    "Lift angle of the calibre in degrees (52 if left out). Amplitude depends on it.",
);
/// For tools that also take a calibre, whose lift angle fills in.
const LIFT_OR_CALIBRE: Param = p(
    "lift_deg",
    Kind::LiftAngle,
    Arg::Flag("--lift"),
    false,
    "Lift angle of the calibre in degrees (the calibre's if one is given and the table \
     has its lift angle, else 52). Amplitude depends on it.",
);
/// The charts over time in the view, averaged as the app's Average Over.
const AVERAGE: Param = p(
    "average_s",
    Kind::Number,
    Arg::Flag("--trend-average"),
    false,
    "Seconds each point of the charts over time is averaged over, as the app's Average \
     Over: 2, 4, 10, 20, 30 or 60 (10 if left out). Changes only the charts.",
);
const CALIBRE: Param = p(
    "calibre",
    Kind::Text,
    Arg::Flag("--calibre"),
    false,
    "The movement, e.g. \"ETA 2824-2\" or \"Rolex 3235\", to name periodic changes after \
     its wheels; its lift angle is used where lift_deg is not given. An unknown name is \
     an error that suggests a wider search; list_calibres has the known ones.",
);

/// For tools that read a watch: the calibre fills in what is not given.
const CALIBRE_READING: Param = p(
    "calibre",
    Kind::Text,
    Arg::Flag("--calibre"),
    false,
    "The movement, e.g. \"ETA 2824-2\" or \"Rolex 3235\": its beat rate and lift angle \
     are used where bph and lift_deg are not given. An unknown name is an error that \
     suggests a wider search; list_calibres has the known ones.",
);
const DEVICE: Param = p(
    "device",
    Kind::Text,
    Arg::Flag("--device"),
    false,
    "Sound input: part of its name or id from list_sound_inputs (the computer's default \
     input if left out).",
);

pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "analyze_recording",
        title: "Analyse a Recording",
        description: "Measure one recording of a watch in one position: rate (seconds per \
            day, positive gaining), amplitude (degrees), beat error (ms), how steady the \
            beats are, and whether a regular beat was found. When `watch_heard` is false \
            no regular beat was found, and the readings are null.",
        command: &["analyze"],
        params: &[RECORDING, CALIBRE_READING, BPH, LIFT_OR_CALIBRE, AVERAGE],
        charts: true,
        read_only: true,
        idempotent: true,
        defaults: &[],
    },
    ToolSpec {
        name: "run_session",
        title: "Read a Multi-Position Test",
        description: "Read a watch measured in several positions (and states of wind) as \
            one Witschi-style report: per-position readings, the characteristic values \
            (mean rate, largest difference, vertical against horizontal). Give a \
            folder holding session.toml, the session.toml itself, or recordings with the \
            position in each file name (DU, DD, CU, CD, CL, CR). Also writes report.html \
            next to the recordings. The report may name the vertical positions by the \
            hour at the top, with the crown at 3 o'clock: crown up is 3H, crown down 9H, \
            crown left 6H and crown right 12H. A Power Reserve take's folder (the app's \
            Power Reserve Test) also gives its run in `power_reserve`: how long the watch \
            ran, how the run ended, the stops on the way and its readings hour by hour.",
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
            CALIBRE_READING,
            BPH,
            p(
                "lift_deg",
                Kind::LiftAngle,
                Arg::Flag("--lift"),
                false,
                "Lift angle in degrees (overrides session.toml; else the calibre's if the \
                 table has it; else 52).",
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
        charts: true,
        read_only: false,
        idempotent: true,
        defaults: &[],
    },
    ToolSpec {
        name: "check_steadiness",
        title: "Check Steadiness",
        description: "How the rate, amplitude and beat error move over a long recording \
            (an hour or more). Gives each series' headline (what was detected: a cycle, \
            two levels, level changes, a trend, or none), the same in a few words as \
            `summary`, and its statistics: \
            autocorrelation, Allan deviation, the period and its size, the wheel whose \
            turn matches a cycle, level changes and an amplitude spectrum, and findings \
            with their evidence. With `maps`, also each series' time-frequency maps (an \
            STFT and a synchrosqueezed wavelet map, drawn in the chart view; the result \
            gives each map's outline in `time_frequency_maps`), its Lomb-Scargle \
            periodogram, gaps and all, its kurtogram in `kurtograms` (the spectral \
            kurtosis at each period over windows of several lengths: about 0 is noise, \
            -1 a line of steady size, above 0 a line that comes in bursts), the \
            coherences without their grids, and each named wheel's averaged turn; it \
            takes longer. Facts only: the reading of them is the \
            caller's.",
        command: &["series", "--views"],
        params: &[
            RECORDINGS,
            BPH,
            LIFT_OR_CALIBRE,
            CALIBRE,
            p(
                "reading_s",
                Kind::Number,
                Arg::Flag("--reading"),
                false,
                "Length of each rate reading in seconds (10 if left out).",
            ),
            p(
                "maps",
                Kind::Switch,
                Arg::Flag("--maps"),
                false,
                "Add the time-frequency maps and the analyses that come with them.",
            ),
        ],
        charts: true,
        read_only: true,
        idempotent: true,
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
        params: &[RECORDINGS, BPH, LIFT_OR_CALIBRE, CALIBRE],
        charts: false,
        read_only: false,
        idempotent: true,
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
        charts: true,
        read_only: true,
        idempotent: true,
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
        charts: false,
        read_only: true,
        idempotent: true,
        defaults: &[],
    },
    ToolSpec {
        name: "check_recording",
        title: "Check a Recording",
        description: "A recording's sound: level, clipping, automatic gain, noise and \
            whether ticks are heard. Reads the file \
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
        charts: false,
        read_only: true,
        idempotent: true,
        defaults: &[],
    },
    ToolSpec {
        name: "list_calibres",
        title: "Look Up a Calibre",
        description: "The calibre table: each movement's beat rate, lift angle, escape \
            wheel and train, how sure the figures are and their sources. Give a name for \
            one calibre (an unknown name is an error), or `search` for the calibres that \
            answer it, best first; with neither, the common calibres and the user's own are \
            listed. Amplitude depends on the lift angle.",
        command: &["calibres"],
        params: &[
            p(
                "name",
                Kind::Text,
                Arg::Positional,
                false,
                "A calibre, e.g. \"ETA 2824-2\" (the common ones and the user's own if left \
                 out).",
            ),
            p(
                "search",
                Kind::Text,
                Arg::Flag("--search"),
                false,
                "Words to look for in calibres' names, other names and makers, e.g. \
                 \"dd 3235\" or \"AP 3120\"; not with `name`.",
            ),
        ],
        charts: false,
        read_only: true,
        idempotent: true,
        defaults: &[],
    },
    ToolSpec {
        name: "list_watches",
        title: "List My Watches",
        description: "The watches in the library the app keeps: each one's name, calibre, \
            beat rate and lift angle as the person entered them, how many takes it has and \
            when the newest started, and how many Quick Listen takes wait in Unsorted. \
            Every watch is listed. It only reads, so the app can be open.",
        command: &["watches", "list"],
        params: &[
            LIBRARY,
            p(
                "all",
                Kind::Switch,
                Arg::Flag("--all"),
                false,
                "Ignored: every watch is listed. Watches are kept or deleted, no longer \
                 archived; kept so older calls still work.",
            ),
        ],
        charts: false,
        read_only: true,
        idempotent: true,
        defaults: &[],
    },
    ToolSpec {
        name: "show_watch",
        title: "Show a Watch",
        description: "One watch from the library, by its name (any case, spaces at either \
            end ignored), or \"Unsorted\" \
            for the Quick Listen takes: its details, the fields the person added, and every \
            take, newest first, with its full folder `path`, start, length, positions, \
            whether it kept its sound (`audio`), its own `reading` (its last position as \
            run_session reads it: rate, whether the rate is `calibrated` on the take's \
            clock, amplitude, signed beat error; the first show of a take reads its sound, which \
            takes a while, about a minute per 9 h of sound, and later shows are immediate), and the readings the app showed when it saved the \
            take (`at_end`, to be retired). Pass a take's `path` to run_session, long_run or \
            check_steadiness to analyse it; a take with `audio` false holds only its \
            analysis. It changes nothing in the library.",
        command: &["watches", "show"],
        params: &[
            p(
                "name",
                Kind::Text,
                Arg::Positional,
                true,
                "The watch's name, as list_watches gives it, or \"Unsorted\".",
            ),
            LIBRARY,
        ],
        charts: false,
        read_only: true,
        idempotent: true,
        defaults: &[],
    },
    ToolSpec {
        name: "record_position",
        title: "Record a Position",
        description: "Record the watch on the microphone in one position into a test \
            folder, with a quick check of the take. Without `folder`, `watch` names a watch in \
            the app's library and a new test folder is made among its takes; give that \
            folder (the result's `session_file` is in it) as `folder` for the next position. \
            Recording into the library is refused while the app has it open. Every take is \
            added to the folder's session.toml, ready for run_session; the check states the facts about its \
            sound (`watch_heard` and the timing scatter, and the beats clipped). This \
            uses the microphone: before every call, tell the person which position to put the \
            watch in (DU dial up, DD dial down, CU crown up, CD crown down, CL crown left, \
            CR crown right), that it should be wound and settled, and to keep the room \
            quiet; call only when they say it is ready. It takes as long as `seconds`.",
        command: &["record"],
        params: &[
            p(
                "folder",
                Kind::Path,
                Arg::Positional,
                false,
                "Full path of the test folder, one per watch and test (created if missing), \
                 e.g. ~/Timegrapher/my-watch-2026-10-11. Leave it out to start a new test \
                 under a library watch named by `watch`.",
            ),
            p(
                "position",
                Kind::Choice(&["DU", "DD", "CU", "CD", "CL", "CR"]),
                Arg::Flag("--position"),
                true,
                "The position the watch is in.",
            ),
            p(
                "seconds",
                Kind::CaptureSeconds,
                Arg::Flag("--seconds"),
                false,
                "Seconds to record (120 if left out; 60 to 120 is usual).",
            ),
            p(
                "wind_h",
                Kind::Number,
                Arg::Flag("--wind-h"),
                false,
                "Hours since the watch was fully wound: 0 for the fully wound half of a \
                 test, 24 for the run-down half (0 if left out).",
            ),
            p(
                "watch",
                Kind::Text,
                Arg::Flag("--watch"),
                false,
                "With `folder`: the watch, in the owner's words (written to a new session \
                 file). Without: a watch in the app's library, by its name as list_watches \
                 gives it; its calibre, beat rate and lift angle fill in those not given.",
            ),
            LIBRARY,
            CALIBRE_READING,
            BPH,
            p(
                "lift_deg",
                Kind::LiftAngle,
                Arg::Flag("--lift"),
                false,
                "Lift angle in degrees (the calibre's if left out and the table has it, \
                 else 52). In a folder whose session.toml has a lift angle, that angle is \
                 used: a different one, given here or through the calibre, is an error, and \
                 if the file marks its angle as assumed, any lift given here or through the \
                 calibre is refused, even the same one.",
            ),
            DEVICE,
            p(
                TAKE_ID,
                Kind::Text,
                Arg::Server,
                false,
                "A name for this take, new for each take, e.g. \"DU-1\". If the call is \
                 made again with the same take_id, folder and position (say the answer \
                 was lost), the take already recorded comes back instead of a second \
                 recording, for as long as the server runs.",
            ),
        ],
        charts: true,
        read_only: false,
        idempotent: false,
        defaults: &[],
    },
    ToolSpec {
        name: "check_microphone",
        title: "Check the Microphone",
        description: "Listen for a few seconds and report the level, clipping, automatic \
            gain, the noise and whether ticks are heard: `issues[]` names each condition \
            detected with its figure, and on Linux `mixer` holds the sound card's controls \
            with the `amixer` command for each change in `fixes[]`. It changes nothing; \
            any change is the person's to make. \
            Its rate and beat error come from a few seconds of sound (5 s unless set), \
            not from a measurement of the watch. \
            This uses the microphone: put the watch on it first.",
        command: &["doctor"],
        params: &[
            DEVICE,
            p(
                "seconds",
                Kind::CaptureSeconds,
                Arg::Flag("--seconds"),
                false,
                "Seconds to listen (5 if left out).",
            ),
            BPH,
        ],
        charts: false,
        read_only: true,
        idempotent: true,
        defaults: &[],
    },
    ToolSpec {
        name: "list_sound_inputs",
        title: "List Sound Inputs",
        description: "The computer's sound inputs, which is the default, and the sample \
            rates and formats each supports. Lists them only: nothing is recorded.",
        command: &["devices"],
        params: &[],
        charts: false,
        read_only: true,
        idempotent: true,
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
                Kind::LiftAngle,
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
        charts: false,
        read_only: false,
        idempotent: false,
        // The CLI's default is the current folder, which for a server the
        // chat app started is nowhere the person would look.
        defaults: &[("--out", "~")],
    },
];

/// Whether the tool opens the microphone: recording a position, and the
/// microphone check, which has no recording to read instead.
pub fn listens(t: &ToolSpec) -> bool {
    t.command[0] == "record"
        || (t.command[0] == "doctor"
            && !t
                .params
                .iter()
                .any(|p| matches!(p.arg, Arg::Flag("--file"))))
}

/// Plain notes before a result's document, for what the numbers alone do
/// not say: a microphone check's readings come from a few seconds. What
/// limits a recording's or a test's readings is in its lead (summary.rs).
pub fn notes(t: &ToolSpec) -> Vec<String> {
    let mut notes = Vec::new();
    if t.name == "check_microphone" {
        notes.push(
            "The rate and beat error here come from a few seconds of sound (5 s unless \
             set), not from a measurement of the watch: record_position or \
             analyze_recording measures the watch."
                .to_string(),
        );
    }
    notes
}

/// `record_position`'s retry identifier, read by the server.
pub const TAKE_ID: &str = "take_id";

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
            Kind::Choice(words) => json!({"type": "string", "enum": words}),
            Kind::Paths => json!({"type": "array", "items": {"type": "string"}, "minItems": 1}),
            Kind::BeatRate => json!({"type":"integer", "minimum":1, "maximum":u32::MAX}),
            Kind::LiftAngle => json!({
                "type": "number",
                "minimum": timegrapher_core::input_domain::LIFT_RANGE_DEG.start(),
                "maximum": timegrapher_core::input_domain::LIFT_RANGE_DEG.end(),
            }),
            Kind::Number => json!({"type": "number"}),
            Kind::CaptureSeconds => json!({"type": "number", "exclusiveMinimum": 0}),
            Kind::Switch => json!({"type": "boolean"}),
        };
        let mut schema = schema.as_object().unwrap().clone();
        schema.insert("description".into(), p.description.into());
        props.insert(p.name.into(), Value::Object(schema));
        if p.required {
            required.push(p.name);
        }
    }
    let mut d = json!({
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
            "idempotentHint": t.idempotent,
            "openWorldHint": false,
        },
    });
    if t.charts {
        d["_meta"] = crate::views::tool_meta();
    }
    d
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
        if let Kind::Switch = p.kind {
            match (v, p.arg) {
                (Value::Bool(true), Arg::Flag(flag)) => options.push(flag.to_string()),
                (Value::Bool(_), _) => {}
                _ => return Err(format!("{}: {} must be true or false", t.name, p.name)),
            }
            continue;
        }
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
            (Kind::Choice(words), Value::String(s)) if words.contains(&s.as_str()) => {
                vec![s.clone()]
            }
            (Kind::BeatRate, Value::Number(n)) => {
                let value = n.as_u64().ok_or_else(|| {
                    format!("{}: {} must be a positive u32 integer", t.name, p.name)
                })?;
                timegrapher_core::input_domain::bph(value)
                    .map_err(|e| format!("{}: {}: {e}", t.name, p.name))?;
                vec![n.to_string()]
            }
            (Kind::LiftAngle, Value::Number(n)) => {
                let value = n.as_f64().ok_or_else(|| {
                    format!(
                        "{}: {}: {}",
                        t.name,
                        p.name,
                        timegrapher_core::input_domain::LIFT_RANGE_TEXT
                    )
                })?;
                timegrapher_core::input_domain::lift_deg(value)
                    .map_err(|e| format!("{}: {}: {e}", t.name, p.name))?;
                vec![n.to_string()]
            }
            (Kind::Number, Value::Number(n)) if n.as_f64().is_some_and(f64::is_finite) => {
                vec![n.to_string()]
            }
            (Kind::CaptureSeconds, Value::Number(n)) => {
                let seconds = n
                    .as_f64()
                    .ok_or_else(|| format!("{}: {} must be a number of seconds", t.name, p.name))?;
                let checked = if t.command[0] == "doctor" {
                    timegrapher_core::capture::checked_recording_deadline(seconds).map(|_| ())
                } else {
                    timegrapher_core::capture::checked_duration(seconds).map(|_| ())
                };
                checked.map_err(|e| format!("{}: {} {e}", t.name, p.name))?;
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
            Arg::Server => {}
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
    Ok(expand_home_path(s)?.to_string_lossy().into_owned())
}

/// Native expansion for record destinations; identity and process arguments
/// must not replace non-UTF8 native path components with display characters.
pub(crate) fn expand_home_path(s: &str) -> Result<std::path::PathBuf, String> {
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
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    /// A full path on whichever system the test runs on.
    fn full(name: &str) -> String {
        std::env::temp_dir()
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn analyze_line() {
        let t = find("analyze_recording").unwrap();
        let line = command_line(
            t,
            &args(json!({"recording": full("take.flac"), "bph": 28800, "lift_deg": 52.5})),
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
                &full("take.flac")
            ]
        );
    }

    #[test]
    fn a_switch_is_a_bare_flag_only_when_true() {
        let t = find("list_watches").unwrap();
        let line = |a: Value| command_line(t, &args(a));
        assert_eq!(
            line(json!({"all": true})).unwrap(),
            ["watches", "list", "--all", "--json"]
        );
        assert_eq!(
            line(json!({"all": false})).unwrap(),
            ["watches", "list", "--json"]
        );
        assert!(line(json!({"all": "yes"})).is_err());
        assert_eq!(
            definition(t)["inputSchema"]["properties"]["all"]["type"],
            "boolean"
        );
        let t = find("show_watch").unwrap();
        assert_eq!(
            command_line(t, &args(json!({"name": "-Pepsi", "library": full("lib")}))).unwrap(),
            [
                "watches",
                "show",
                &format!("--library={}", full("lib")),
                "--json",
                "--",
                "-Pepsi"
            ]
        );
    }

    #[test]
    fn recording_is_a_flag_for_doctor() {
        let t = find("check_recording").unwrap();
        let line = command_line(t, &args(json!({"recording": full("t.wav")}))).unwrap();
        assert_eq!(
            line,
            ["doctor", &format!("--file={}", full("t.wav")), "--json"]
        );
    }

    #[test]
    fn acquisition_seconds_reject_invalid_lengths_and_preserve_defaults() {
        for name in ["record_position", "check_microphone"] {
            let tool = find(name).unwrap();
            assert_eq!(
                definition(tool)["inputSchema"]["properties"]["seconds"]["exclusiveMinimum"],
                0
            );
            let mut values = json!({ "seconds": 120 });
            if name == "record_position" {
                values["folder"] = json!(full("test"));
                values["position"] = json!("DU");
                values["wind_h"] = json!(0);
            }
            let line = command_line(tool, values.as_object().unwrap()).unwrap();
            assert!(line.contains(&"--seconds=120".to_string()));
            for seconds in [
                json!(0),
                json!(-1),
                json!(1e300),
                json!(1e-300),
                json!("NaN"),
            ] {
                values["seconds"] = seconds;
                let error = command_line(tool, values.as_object().unwrap()).unwrap_err();
                assert!(error.contains("seconds"), "{name}: {error}");
            }
            values.as_object_mut().unwrap().remove("seconds");
            let line = command_line(tool, values.as_object().unwrap()).unwrap();
            assert!(!line.iter().any(|arg| arg.starts_with("--seconds=")));
        }
    }

    #[test]
    fn refuses_bad_calls() {
        let t = find("analyze_recording").unwrap();
        assert!(command_line(t, &args(json!({}))).is_err());
        assert!(command_line(t, &args(json!({"recording": "take.flac"}))).is_err());
        assert!(command_line(t, &args(json!({"recording": full("t"), "apply": true}))).is_err());
        assert!(command_line(t, &args(json!({"recording": full("t"), "bph": -1}))).is_err());
        assert!(command_line(t, &args(json!({"recording": full("t"), "bph": "x"}))).is_err());
    }

    #[test]
    fn no_tool_sets_consent_or_changes_settings() {
        for t in TOOLS {
            let names: Vec<_> = t.params.iter().map(|p| p.name).collect();
            for forbidden in ["cc0", "listened_back", "apply", "save"] {
                assert!(!names.contains(&forbidden), "{} has {forbidden}", t.name);
            }
        }
    }

    /// The microphone is used by exactly the tools that say so, as
    /// docs/mcp.md decides: recording a position and the live check.
    #[test]
    fn only_the_listening_tools_use_the_mic() {
        let mut listening: Vec<&str> = TOOLS
            .iter()
            .filter(|t| listens(t))
            .map(|t| t.name)
            .collect();
        listening.sort();
        assert_eq!(listening, ["check_microphone", "record_position"]);
        for t in TOOLS {
            assert_eq!(
                listens(t),
                t.description.contains("uses the microphone"),
                "{}",
                t.name
            );
        }
        // The file check never listens: its recording is required.
        let file_check = find("check_recording").unwrap();
        assert!(file_check
            .params
            .iter()
            .any(|p| p.required && matches!(p.arg, Arg::Flag("--file"))));
    }

    /// A chat app may repeat a call it thinks is harmless to repeat: only
    /// the tools that make something new each time say they are not.
    #[test]
    fn only_the_tools_that_add_files_are_not_idempotent() {
        let mut not: Vec<&str> = TOOLS
            .iter()
            .filter(|t| definition(t)["annotations"]["idempotentHint"] == false)
            .map(|t| t.name)
            .collect();
        not.sort();
        assert_eq!(not, ["make_report_bundle", "record_position"]);
        for t in TOOLS.iter().filter(|t| t.read_only) {
            assert!(t.idempotent, "{}", t.name);
        }
    }

    #[test]
    fn a_quick_check_is_called_out() {
        let mic = find("check_microphone").unwrap();
        assert!(notes(mic)[0].contains("a few seconds of sound"));
        assert!(notes(find("analyze_recording").unwrap()).is_empty());
    }

    #[test]
    fn the_take_id_stays_off_the_command_line() {
        let t = find("record_position").unwrap();
        let line = command_line(
            t,
            &args(json!({"folder": full("test"), "position": "DU", "take_id": "DU-1"})),
        )
        .unwrap();
        assert!(!line.iter().any(|a| a.contains("DU-1")), "{line:?}");
    }

    #[test]
    fn positions_are_checked() {
        let t = find("record_position").unwrap();
        let ok = args(json!({"folder": full("test"), "position": "DU"}));
        let line = command_line(t, &ok).unwrap();
        assert_eq!(
            line[..2],
            ["record".to_string(), "--position=DU".to_string()]
        );
        let bad = args(json!({"folder": full("test"), "position": "up"}));
        assert!(command_line(t, &bad).is_err());
    }

    /// The Claude Desktop bundle lists the tools the server has.
    #[test]
    fn the_bundle_lists_every_tool() {
        let manifest: Value =
            serde_json::from_str(include_str!("../packaging/manifest.json")).unwrap();
        let mut listed: Vec<&str> = manifest["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        let mut tools: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
        tools.push(crate::guidance::GUIDE_TOOL);
        listed.sort();
        tools.sort();
        assert_eq!(listed, tools);
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

#[cfg(test)]
mod numeric_input_tests {
    use super::*;
    fn args(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    // Match the existing tools tests: a native absolute path, without creating a file.
    fn full(name: &str) -> String {
        std::env::temp_dir()
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn numeric_inputs_match_domains_and_schema_before_child_construction() {
        let t = find("analyze_recording").unwrap();
        for value in [json!(0), json!(-1), json!(u32::MAX as u64 + 1)] {
            let error = command_line(
                t,
                &args(json!({"recording":full("unused.wav"),"bph":value})),
            )
            .unwrap_err();
            assert!(error.contains("bph"), "wrong rejection: {error}");
        }
        for value in [json!(0), json!(-0.0), json!(-1.0), json!(9.9), json!(95)] {
            let error = command_line(
                t,
                &args(json!({"recording":full("unused.wav"),"lift_deg":value})),
            )
            .unwrap_err();
            assert!(error.contains("lift_deg"), "wrong rejection: {error}");
        }
        let schema = definition(t);
        assert_eq!(
            schema["inputSchema"]["properties"]["bph"]["maximum"],
            u32::MAX
        );
        assert_eq!(
            schema["inputSchema"]["properties"]["lift_deg"]["minimum"],
            10.0
        );
        assert_eq!(
            schema["inputSchema"]["properties"]["lift_deg"]["maximum"],
            90.0
        );
        let line = command_line(
            t,
            &args(json!({"recording":full("unused.wav"),"bph":u32::MAX,"lift_deg":52.0})),
        )
        .unwrap_or_else(|error| panic!("valid native-path numeric control rejected: {error}"));
        assert!(line.contains(&format!("--bph={}", u32::MAX)), "{line:?}");
        assert!(line.contains(&"--lift=52.0".to_string()), "{line:?}");
        for (name, field, expected) in [
            ("run_session", "paths", json!([full("unused")])),
            ("record_position", "folder", json!(full("unused"))),
        ] {
            let t = find(name).unwrap();
            let mut a = args(json!({"lift_deg":0}));
            if name == "record_position" {
                a.insert("position".into(), json!("DU"));
            }
            a.insert(field.into(), expected);
            let error = command_line(t, &a).unwrap_err();
            assert!(
                error.contains("lift_deg"),
                "{name}: wrong rejection: {error}"
            );
        }
    }

    #[test]
    fn numeric_inputs_validate_report_analysis_settings() {
        let t = find("make_report_bundle").unwrap();
        let schema = definition(t);
        assert_eq!(
            schema["inputSchema"]["properties"]["bph"]["maximum"],
            u32::MAX
        );
        assert_eq!(
            schema["inputSchema"]["properties"]["lift_deg"]["minimum"],
            10.0
        );
        assert_eq!(
            schema["inputSchema"]["properties"]["lift_deg"]["maximum"],
            90.0
        );
        for a in [
            json!({"bph":0}),
            json!({"bph":u32::MAX as u64+1}),
            json!({"lift_deg":0}),
            json!({"lift_deg":-1}),
        ] {
            let field = if a.get("bph").is_some() {
                "bph"
            } else {
                "lift_deg"
            };
            let error = command_line(t, &args(a)).unwrap_err();
            assert!(error.contains(field), "wrong rejection: {error}");
        }
        assert!(command_line(t, &args(json!({"bph":u32::MAX,"lift_deg":52}))).is_ok());
    }
}
