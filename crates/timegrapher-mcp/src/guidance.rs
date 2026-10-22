//! The watchmaker skill and the docs an agent reads, served as MCP
//! resources, and the instructions and prompt that point at them. The
//! files are compiled in unchanged, so a chat app gets the same guidance
//! as a shell agent with the skill installed.

use serde_json::{json, Value};

pub struct Doc {
    pub uri: &'static str,
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub text: &'static str,
}

macro_rules! doc {
    ($uri:literal, $name:literal, $title:literal, $description:literal, $path:literal) => {
        Doc {
            uri: $uri,
            name: $name,
            title: $title,
            description: $description,
            text: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../", $path)),
        }
    };
}

pub const DOCS: &[Doc] = &[
    doc!(
        "timegrapher://skill/watchmaker",
        "watchmaker",
        "Watchmaker",
        "How to help someone test a mechanical watch: ground rules, the test, reading the \
         results and when a watch needs a watchmaker. Read this first.",
        "skills/watchmaker/SKILL.md"
    ),
    doc!(
        "timegrapher://skill/guided-test",
        "guided-test",
        "Guided Test",
        "Walking a first-timer through the whole test: what to ask, what to say, what to \
         run, step by step.",
        "skills/watchmaker/references/guided-test.md"
    ),
    doc!(
        "timegrapher://skill/findings",
        "findings",
        "Findings",
        "The finding codes and the patterns in the Tick and Tock, as the skill reads them.",
        "skills/watchmaker/references/findings.md"
    ),
    doc!(
        "timegrapher://skill/setup-and-microphone",
        "setup-and-microphone",
        "Setup and Microphone",
        "Getting the microphone right, positions, winding and settling.",
        "skills/watchmaker/references/setup-and-microphone.md"
    ),
    doc!(
        "timegrapher://skill/reading-the-numbers",
        "reading-the-numbers",
        "Reading the Numbers",
        "Rate, amplitude, beat error, lift angles by calibre, positional variation.",
        "skills/watchmaker/references/reading-the-numbers.md"
    ),
    doc!(
        "timegrapher://skill/diagnosis",
        "diagnosis",
        "Diagnosis",
        "From symptoms to likely causes.",
        "skills/watchmaker/references/diagnosis.md"
    ),
    doc!(
        "timegrapher://skill/tick-shape",
        "tick-shape",
        "Tick Shape",
        "What the unlock, impulse and drop sounds say about the escapement.",
        "skills/watchmaker/references/tick-shape.md"
    ),
    doc!(
        "timegrapher://skill/long-runs",
        "long-runs",
        "Long Runs",
        "Hours-long recordings, the steadiness of each series and wheel-named periodic changes.",
        "skills/watchmaker/references/long-runs.md"
    ),
    doc!(
        "timegrapher://skill/when-to-see-a-watchmaker",
        "when-to-see-a-watchmaker",
        "When to See a Watchmaker",
        "Fine, keep an eye on it, service soon or stop wearing it.",
        "skills/watchmaker/references/when-to-see-a-watchmaker.md"
    ),
    doc!(
        "timegrapher://skill/qc-videos",
        "qc-videos",
        "Other Timegraphers' Screens",
        "Reading a photo or video of another timegrapher's screen.",
        "skills/watchmaker/references/qc-videos.md"
    ),
    doc!(
        "timegrapher://skill/cli-reference",
        "cli-reference",
        "Command Reference",
        "The timegrapher commands behind this server's tools, and their options.",
        "skills/watchmaker/references/cli-reference.md"
    ),
    doc!(
        "timegrapher://docs/agent-interface",
        "agent-interface",
        "JSON Contract",
        "Every field of the JSON documents the tools return.",
        "docs/agent-interface.md"
    ),
    doc!(
        "timegrapher://docs/sessions",
        "sessions",
        "Multi-Position Sessions",
        "session.toml, positions, characteristic values and session finding codes.",
        "docs/sessions.md"
    ),
    doc!(
        "timegrapher://docs/series",
        "series",
        "Steadiness",
        "What each steadiness result describes and the evidence behind it.",
        "docs/series.md"
    ),
    doc!(
        "timegrapher://docs/fault-signatures",
        "fault-signatures",
        "Fault Signatures",
        "Escapement faults and how they show in the beat's shape.",
        "docs/fault-signatures.md"
    ),
];

pub fn find(uri: &str) -> Option<&'static Doc> {
    DOCS.iter().find(|d| d.uri == uri)
}

/// The tool that hands the model the skill and its pages: a model can call
/// a tool, while many chat apps leave resources for the person to attach.
pub const GUIDE_TOOL: &str = "read_guide";

pub fn guide_definition() -> Value {
    let pages: Vec<&str> = DOCS.iter().map(|d| d.name).collect();
    let list: Vec<String> = DOCS
        .iter()
        .map(|d| format!("{}: {}", d.name, d.description))
        .collect();
    json!({
        "name": GUIDE_TOOL,
        "title": "Read the Guide",
        "description": format!(
            "Read the watchmaker skill or one of its pages, as Markdown. Read \"watchmaker\" \
             before guiding anyone; it says when to read the others. Pages: {}",
            list.join(" ")
        ),
        "inputSchema": {
            "type": "object",
            "properties": { "page": {
                "type": "string",
                "enum": pages,
                "description": "The page (watchmaker if left out).",
            }},
            "additionalProperties": false,
        },
        "annotations": {
            "title": "Read the Guide",
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": false,
        },
    })
}

/// The page's text, or what pages there are.
pub fn guide(page: Option<&str>) -> Value {
    let name = page.unwrap_or("watchmaker");
    match DOCS.iter().find(|d| d.name == name) {
        Some(d) => json!({ "content": [{ "type": "text", "text": d.text }] }),
        None => {
            let pages: Vec<&str> = DOCS.iter().map(|d| d.name).collect();
            json!({
                "content": [{ "type": "text", "text": format!(
                    "No page {name:?}; the pages are {}.", pages.join(", ")) }],
                "isError": true,
            })
        }
    }
}

/// Sent to the chat app when it connects.
pub const INSTRUCTIONS: &str = "\
The timegrapher measures a mechanical watch from the sound of its ticking: rate \
(seconds per day), amplitude (degrees) and beat error (ms), in several positions and \
over long runs. You are helping someone test their watch at home, as an experienced \
watchmaker would. Before guiding anyone, call read_guide with page \"watchmaker\" to read \
the watchmaker skill (also the resource timegrapher://skill/watchmaker), and follow its \
ground rules: ask before changing anything on their \
computer, never tell anyone to open a watch, rule out the set-up before blaming the watch, \
and say how sure you are.

The skill names command-line commands; here each is a tool that runs it for you and \
returns the same JSON: analyze is analyze_recording, session is run_session, series is \
check_steadiness, long is long_run, profile is tick_tock_profile, shape is beat_shape, \
doctor --file is check_recording, doctor is check_microphone, devices is \
list_sound_inputs, calibres is list_calibres, watches list is list_watches, watches show \
is show_watch, record is record_position and report is make_report_bundle. Two tools use the microphone, record_position and check_microphone: \
before each, tell the person how to place the watch and wait until they say it is ready. \
For a multi-position test, record each position into one folder with record_position, \
then read the folder with run_session. Recordings made elsewhere work too: ask for the \
file's full path. Say Tick and Tock to people; in the JSON, even/a is the Tick and odd/b the Tock.";

pub const PROMPT_NAME: &str = "test_my_watch";
pub const PROMPT_TITLE: &str = "Test My Watch";
pub const PROMPT_DESCRIPTION: &str =
    "Walk me through a multi-position test of my watch at home and explain the readings.";

/// The skill's own prompt, unchanged, after a note on where its files and
/// commands are here.
const PROMPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../skills/watchmaker/references/prompt-test-my-watch.md"
));

pub fn prompt_text(watch: Option<&str>) -> String {
    let watch = watch
        .filter(|w| !w.trim().is_empty())
        .map(|w| format!("\n\nThe watch: {}.", w.trim()))
        .unwrap_or_default();
    format!(
        "First call the read_guide tool with page \"watchmaker\" and read the skill. \
         The files named below are pages of the same tool: the file's name without .md \
         (guided-test, findings, when-to-see-a-watchmaker), also resources at \
         timegrapher://skill/. The commands are this server's tools: \
         calibres is list_calibres, doctor is check_microphone, record is \
         record_position, session is run_session, analyze is analyze_recording and \
         series is check_steadiness.\n\n{PROMPT}{watch}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_hands_over_every_page() {
        for d in DOCS {
            assert_eq!(guide(Some(d.name))["content"][0]["text"], d.text);
        }
        assert_eq!(guide(None)["content"][0]["text"], DOCS[0].text);
        assert_eq!(guide(Some("nope"))["isError"], true);
        assert_eq!(guide_definition()["annotations"]["readOnlyHint"], true);
    }

    #[test]
    fn docs_are_present_and_unique() {
        for (i, d) in DOCS.iter().enumerate() {
            assert!(d.text.len() > 500, "{} is nearly empty", d.uri);
            assert!(DOCS[..i].iter().all(|e| e.uri != d.uri && e.name != d.name));
        }
        assert!(INSTRUCTIONS.contains(DOCS[0].uri));
        assert!(INSTRUCTIONS.contains(GUIDE_TOOL));
    }

    /// Every reference the skill links to is served, so the prompt and the
    /// skill never point an agent at a file it cannot read.
    #[test]
    fn every_skill_reference_is_served() {
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../skills/watchmaker/references"
        );
        for entry in std::fs::read_dir(dir).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            let Some(stem) = name.strip_suffix(".md") else {
                continue;
            };
            if stem == "prompt-test-my-watch" {
                continue;
            }
            let uri = format!("timegrapher://skill/{stem}");
            assert!(find(&uri).is_some(), "{name} is not served as {uri}");
        }
    }

    #[test]
    fn the_prompt_is_the_skills_own() {
        let text = prompt_text(Some("ETA 2824-2"));
        assert!(text.contains(PROMPT));
        assert!(text.ends_with("The watch: ETA 2824-2."));
        assert!(!prompt_text(Some("  ")).contains("The watch:"));
    }
}
