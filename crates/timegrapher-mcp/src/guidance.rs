//! The watchmaker skill and the docs an agent reads, served as MCP
//! resources, and the instructions and prompt that point at them. The
//! files are compiled in unchanged, so a chat app gets the same guidance
//! as a shell agent with the skill installed.

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
        "Hours-long recordings, steadiness verdicts and wheel-named periodic changes.",
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
        "Steadiness Verdicts",
        "What each steadiness verdict means and the evidence behind it.",
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

/// Sent to the chat app when it connects.
pub const INSTRUCTIONS: &str = "\
The timegrapher measures a mechanical watch from the sound of its ticking: rate \
(seconds per day), amplitude (degrees) and beat error (ms), in several positions and \
over long runs. You are helping someone test their watch at home, as an experienced \
watchmaker would. Before guiding anyone, read the resource timegrapher://skill/watchmaker \
(the watchmaker skill) and follow its ground rules: ask before changing anything on their \
computer, never tell anyone to open a watch, rule out the set-up before blaming the watch, \
and say how sure you are.

The skill names command-line commands; here each is a tool that runs it for you and \
returns the same JSON: analyze is analyze_recording, session is run_session, series is \
check_steadiness, long is long_run, profile is tick_tock_profile, shape is beat_shape, \
doctor --file is check_recording, devices is list_sound_inputs and report is \
make_report_bundle. This server does not record or open the microphone: the person \
records with the timegrapher app (or another recorder) and tells you the file's full path. \
Say Tick and Tock to people; in the JSON, even/a is the Tick and odd/b the Tock.";

pub const PROMPT_NAME: &str = "test_my_watch";
pub const PROMPT_TITLE: &str = "Test My Watch";
pub const PROMPT_DESCRIPTION: &str =
    "Walk me through testing my watch at home and tell me whether it needs a service.";

pub fn prompt_text(watch: Option<&str>) -> String {
    let watch = watch
        .filter(|w| !w.trim().is_empty())
        .map(|w| format!(" The watch is: {}.", w.trim()))
        .unwrap_or_default();
    format!(
        "I want to test my mechanical watch at home with the timegrapher and find out \
         whether it needs a service.{watch} Read timegrapher://skill/watchmaker and \
         timegrapher://skill/setup-and-microphone first. Then guide me one step at a time: \
         identify the calibre (for its beat rate and lift angle), check my microphone \
         set-up from a short test recording, have me record the positions the skill \
         recommends with the watch wound and settled, read them with the tools, and tell \
         me plainly what the numbers say, how sure you are, and whether the watch needs a \
         watchmaker."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_are_present_and_unique() {
        for (i, d) in DOCS.iter().enumerate() {
            assert!(d.text.len() > 500, "{} is nearly empty", d.uri);
            assert!(DOCS[..i].iter().all(|e| e.uri != d.uri && e.name != d.name));
        }
        assert!(INSTRUCTIONS.contains(DOCS[0].uri));
    }
}
