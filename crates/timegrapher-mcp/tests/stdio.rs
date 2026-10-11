//! The server end to end: a chat app's conversation over stdio, with the
//! real `timegrapher` program doing the work on synthetic recordings.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use serde_json::{json, Value};

/// The `timegrapher` program: TIMEGRAPHER_CLI when set, else built for this
/// test into its own target folder (a nested cargo on the outer test run's
/// folder could wait on its lock), without sound input so no ALSA is needed.
/// Built once for all the tests here: a second cargo run, even with nothing
/// to build, replaces the program, and a test starting it in that instant
/// would not find it.
fn cli() -> PathBuf {
    static CLI: OnceLock<PathBuf> = OnceLock::new();
    CLI.get_or_init(build_cli).clone()
}

fn build_cli() -> PathBuf {
    if let Some(p) = std::env::var_os("TIMEGRAPHER_CLI") {
        return PathBuf::from(p);
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = root.join("target/mcp-test-cli");
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "--quiet",
            "-p",
            "timegrapher-cli",
            "--no-default-features",
            "--release",
        ])
        .env("CARGO_TARGET_DIR", &target)
        .current_dir(&root)
        .status()
        .expect("cargo");
    assert!(status.success(), "building timegrapher-cli failed");
    target
        .join("release")
        .join(format!("timegrapher{}", std::env::consts::EXE_SUFFIX))
}

struct Client {
    child: std::process::Child,
    out: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl Client {
    fn start(cli: &Path, input: &Path) -> Client {
        let mut c = Client::spawn(cli, input);
        let init = c.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
            }),
        );
        assert_eq!(init["serverInfo"]["name"], "timegrapher");
        c.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        c
    }

    fn spawn(cli: &Path, input: &Path) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_timegrapher-mcp"))
            .env("TIMEGRAPHER_CLI", cli)
            .env("TIMEGRAPHER_MCP_TEST_INPUT", input)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start timegrapher-mcp");
        let out = BufReader::new(child.stdout.take().unwrap());
        Client {
            child,
            out,
            next_id: 1,
        }
    }

    fn send(&mut self, msg: Value) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let mut line = String::new();
            assert!(self.out.read_line(&mut line).unwrap() > 0, "server closed");
            let msg: Value = serde_json::from_str(&line).expect("one JSON message per line");
            if msg["id"] == id {
                assert!(msg.get("error").is_none(), "{method}: {msg}");
                return msg["result"].clone();
            }
        }
    }

    fn call(&mut self, tool: &str, arguments: Value) -> Value {
        self.request(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        )
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A client on the 2026-07-28 revision, as Claude Code 2.1.296 speaks it:
/// no `initialize`, a `server/discover` probe, and the version in each
/// request's `_meta`. Every result must say `resultType`, or the client
/// drops it and the chat app shows no tools.
#[test]
fn a_2026_07_28_client_gets_complete_results() {
    let cli = cli();
    let dir = std::env::temp_dir().join(format!("timegrapher-mcp-2026-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let take = dir.join("take.wav");
    let mut c = Client::spawn(&cli, &take);
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": { "name": "test", "version": "1" },
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    let mut ask = |method: &str, mut params: Value| {
        params["_meta"] = meta.clone();
        let r = c.request(method, params);
        assert_eq!(r["resultType"], "complete", "{method}: {r}");
        r
    };
    let found = ask("server/discover", json!({}));
    assert!(found["supportedVersions"]
        .as_array()
        .unwrap()
        .contains(&json!("2026-07-28")));
    assert!(!ask("tools/list", json!({}))["tools"]
        .as_array()
        .unwrap()
        .is_empty());
    ask("prompts/list", json!({}));
    ask("resources/list", json!({}));
    ask(
        "resources/read",
        json!({ "uri": "timegrapher://skill/watchmaker" }),
    );
    ask(
        "resources/read",
        json!({ "uri": "ui://timegrapher/charts" }),
    );
    ask("prompts/get", json!({ "name": "test_my_watch" }));
    let r = ask(
        "tools/call",
        json!({ "name": "list_calibres", "arguments": { "name": "ETA 2824-2" } }),
    );
    assert_eq!(r["structuredContent"]["calibres"][0]["bph"], 28800);
    let r = ask(
        "tools/call",
        json!({ "name": "analyze_recording", "arguments": { "recording": "take.wav" } }),
    );
    assert_eq!(r["isError"], true);
    drop(c);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A recording a chat app follows live: the server keeps the take's latest
/// readings in a file, `recording_status` hands them to the view, and the
/// progress messages carry them. The file is played back in real time, so
/// this takes about as long as the take, 12 s.
#[test]
fn a_recording_can_be_followed_live() {
    let cli = cli();
    let dir = std::env::temp_dir().join(format!("timegrapher-mcp-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let take = dir.join("take.wav");
    synth(&cli, &take, "6");
    let mut c = Client::start(&cli, &take);
    let status =
        |c: &mut Client| c.call("recording_status", json!({}))["structuredContent"].clone();
    assert_eq!(status(&mut c)["recording"], false);

    // The view's tool is listed for the view alone.
    let tools = c.request("tools/list", json!({}));
    let st = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "recording_status")
        .unwrap();
    assert_eq!(st["_meta"]["ui"]["visibility"], json!(["app"]));

    // Start the take without waiting for it, asking for progress.
    let folder = dir.join("test").to_string_lossy().into_owned();
    c.send(
        json!({ "jsonrpc": "2.0", "id": 900, "method": "tools/call", "params": {
            "name": "record_position",
            "arguments": { "folder": folder, "position": "CD", "seconds": 12 },
            "_meta": { "progressToken": "take" },
        }}),
    );
    let mut progress = Vec::new();
    let mut live = Vec::new();
    let mut result = None;
    let mut next_id = 1000;
    while result.is_none() {
        std::thread::sleep(std::time::Duration::from_millis(1000));
        c.send(
            json!({ "jsonrpc": "2.0", "id": next_id, "method": "tools/call",
                       "params": { "name": "recording_status", "arguments": {} } }),
        );
        loop {
            let mut line = String::new();
            assert!(c.out.read_line(&mut line).unwrap() > 0, "server closed");
            let msg: Value = serde_json::from_str(&line).unwrap();
            if msg["method"] == "notifications/progress" {
                progress.push(msg["params"]["message"].as_str().unwrap_or("").to_string());
            } else if msg["id"] == 900 {
                result = Some(msg["result"].clone());
            } else if msg["id"] == next_id {
                live.push(msg["result"]["structuredContent"].clone());
                break;
            }
        }
        next_id += 1;
    }
    let r = result.unwrap();
    assert_eq!(r["structuredContent"]["verdict"], "ok", "{r}");
    // The view saw the take as it went, readings and the strip included.
    let heard: Vec<&Value> = live
        .iter()
        .filter(|d| d["recording"] == true && d["reading"]["beats_used"].as_u64().unwrap_or(0) > 0)
        .collect();
    assert!(heard.len() >= 3, "{live:?}");
    let d = heard.last().unwrap();
    assert_eq!(d["schema"], "timegrapher.live/1");
    assert_eq!(d["position"], "CD");
    assert!(!d["strip"]["beats"].as_array().unwrap().is_empty());
    assert!(
        progress
            .iter()
            .any(|m| m.starts_with("Recording CD") && m.contains("s/day")),
        "{progress:?}"
    );
    // When the call ends the file goes, and nothing is recording.
    assert_eq!(status(&mut c)["recording"], false);
    let left = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with(&format!("timegrapher-live-{}-", c.child.id()))
        });
    assert!(!left, "the live file was left behind");
    drop(c);
    let _ = std::fs::remove_dir_all(&dir);
}

fn synth(cli: &Path, out: &Path, rate: &str) {
    let done = Command::new(cli)
        .arg("synth")
        .arg(out)
        .args(["--duration", "40", "--amplitude", "280"])
        .arg(format!("--rate={rate}"))
        .output()
        .unwrap();
    assert!(
        done.status.success(),
        "synth failed ({}): {}",
        done.status,
        String::from_utf8_lossy(&done.stderr)
    );
}

#[test]
fn a_chat_app_reads_a_watch() {
    let cli = cli();
    let dir = std::env::temp_dir().join(format!("timegrapher-mcp-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("session")).unwrap();
    let take = dir.join("take.wav");
    synth(&cli, &take, "6");
    for (pos, rate) in [("DU", "4"), ("DD", "6"), ("CU", "-2")] {
        synth(&cli, &dir.join(format!("session/watch_{pos}.wav")), rate);
    }
    let path = |p: &Path| p.to_string_lossy().into_owned();
    let mut c = Client::start(&cli, &take);

    let tools = c.request("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for want in [
        "analyze_recording",
        "run_session",
        "check_steadiness",
        "check_recording",
    ] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }

    // The CLI's own document comes back unchanged, as structured content
    // and as text.
    let r = c.call(
        "analyze_recording",
        json!({ "recording": path(&take), "bph": 28800 }),
    );
    assert!(r.get("isError").is_none(), "{r}");
    let doc = &r["structuredContent"];
    assert_eq!(doc["schema"], "timegrapher.analyze/1");
    assert_eq!(doc["watch_heard"], true);
    let rate = doc["overall"]["rate_s_per_day"].as_f64().unwrap();
    assert!((rate - 6.0).abs() < 1.0, "rate {rate}");
    // The lead in plain words comes first, then the same document as text.
    let text: Value = serde_json::from_str(
        r["content"][1]["text"]
            .as_str()
            .unwrap()
            .split("\n\n")
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(&text, doc);
    // The charts view gets the readings over time and the profiles beside it.
    let charts = &r["_meta"]["timegrapher/charts"];
    assert!(
        charts["strip"]["rate"].as_array().unwrap().len() >= 3,
        "{charts}"
    );
    assert!(!charts["strip"]["amplitude"].as_array().unwrap().is_empty());
    assert_eq!(charts["profile"]["schema"], "timegrapher.profile/1");
    // What limits the readings, in words for the model and for the view:
    // with no calibre given, the lift angle is assumed.
    let limits = r["_meta"]["timegrapher/limits"].as_array().unwrap();
    assert!(limits
        .iter()
        .any(|l| l["text"].as_str().unwrap().contains("lift angle of 52°")));
    let said = r["content"][0]["text"].as_str().unwrap();
    assert!(said.starts_with("Readings: rate +"), "{said}");
    assert!(!said.contains("confidence"), "{said}");
    assert!(
        said.contains(
            "- No calibre or lift angle was given, so the amplitude assumes a lift angle of 52°"
        ),
        "{said}"
    );

    let r = c.call(
        "run_session",
        json!({ "paths": [path(&dir.join("session"))] }),
    );
    assert!(r.get("isError").is_none(), "{r}");
    assert_eq!(r["structuredContent"]["schema"], "timegrapher.session/1");
    assert_eq!(
        r["structuredContent"]["readings"].as_array().unwrap().len(),
        3
    );
    let said = r["content"][0]["text"].as_str().unwrap();
    assert!(said.starts_with("Readings by position:"), "{said}");
    assert!(said.contains("(dvh_amplitude)"), "{said}");

    let r = c.call("check_recording", json!({ "recording": path(&take) }));
    assert_eq!(r["structuredContent"]["schema"], "timegrapher.doctor/1");

    // Mistakes come back as results the agent can read, not protocol errors.
    let r = c.call("analyze_recording", json!({ "recording": "take.wav" }));
    assert_eq!(r["isError"], true);
    assert!(r["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("full path"));
    let r = c.call(
        "analyze_recording",
        json!({ "recording": path(&dir.join("missing.wav")) }),
    );
    assert_eq!(r["isError"], true);

    // A test recorded position by position, then read as a session (the
    // server reads the take in place of the microphone here).
    let folder = dir.join("recorded");
    for pos in ["DU", "CU"] {
        let r = c.call(
            "record_position",
            json!({ "folder": path(&folder), "position": pos, "seconds": 30,
                    "calibre": "ETA 2824-2", "watch": "Test watch",
                    "take_id": format!("{pos}-1") }),
        );
        assert!(r.get("isError").is_none(), "{r}");
        assert_eq!(r["structuredContent"]["schema"], "timegrapher.record/1");
        assert_eq!(r["structuredContent"]["verdict"], "ok");
        // The facts in words first: the lift angle is the calibre's.
        let said = r["content"][0]["text"].as_str().unwrap();
        assert!(said.starts_with("Recorded "), "{said}");
        assert!(said.contains("the ETA 2824-2's"), "{said}");
        assert!(said.ends_with("The take was added to the test."), "{said}");
    }
    // A retried take comes back as it was, and is not recorded again.
    let first = c.call(
        "record_position",
        json!({ "folder": path(&folder), "position": "DU", "seconds": 30, "take_id": "DU-1" }),
    );
    assert!(first["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("nothing was recorded again"));
    assert_eq!(
        std::fs::read_dir(&folder).unwrap().count(),
        3,
        "two takes and session.toml"
    );
    let r = c.call("run_session", json!({ "paths": [path(&folder)] }));
    assert_eq!(
        r["structuredContent"]["readings"].as_array().unwrap().len(),
        2,
        "{r}"
    );
    let r = c.call("list_calibres", json!({ "name": "ETA 2824-2" }));
    assert_eq!(r["structuredContent"]["calibres"][0]["bph"], 28800);
    let r = c.call(
        "record_position",
        json!({ "folder": path(&folder), "position": "upside down" }),
    );
    assert_eq!(r["isError"], true);

    // The skill is there to read, and the prompt points at it.
    // The tools that draw charts name the view, and the view is there to read.
    let linked: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["_meta"]["ui"]["resourceUri"] == "ui://timegrapher/charts")
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        linked,
        [
            "analyze_recording",
            "run_session",
            "check_steadiness",
            "tick_tock_profile",
            "record_position",
            "recording_status"
        ]
    );
    let view = c.request(
        "resources/read",
        json!({ "uri": "ui://timegrapher/charts" }),
    );
    assert_eq!(view["contents"][0]["mimeType"], "text/html;profile=mcp-app");
    assert!(view["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("ui/initialize"));

    let skill = c.request(
        "resources/read",
        json!({ "uri": "timegrapher://skill/watchmaker" }),
    );
    assert!(skill["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Ground rules"));
    let guide = c.call("read_guide", json!({ "page": "watchmaker" }));
    assert!(guide["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Ground rules"));
    let prompt = c.request(
        "prompts/get",
        json!({ "name": "test_my_watch", "arguments": { "watch": "ETA 2824-2" } }),
    );
    let said = prompt["messages"][0]["content"]["text"].as_str().unwrap();
    assert!(said.contains("ETA 2824-2") && said.contains("read_guide"));

    drop(c);
    let _ = std::fs::remove_dir_all(&dir);
}
