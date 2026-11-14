//! The server end to end: a chat app's conversation over stdio, with the
//! real `timegrapher` program doing the work on synthetic recordings.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use serde_json::{json, Value};

/// The `timegrapher` program: TIMEGRAPHER_CLI when set, else
/// TIMEGRAPHER_MCP_TEST_CLI (CI's test job names the program its own build
/// made, as nextest starts each test in a process of its own and each would
/// otherwise run the cargo build below, #804), else built for this test
/// into its own target folder (a nested cargo on the outer test run's
/// folder could wait on its lock), without sound input so no ALSA is needed.
/// Built once for all the tests here: a second cargo run, even with nothing
/// to build, replaces the program, and a test starting it in that instant
/// would not find it.
fn cli() -> PathBuf {
    static CLI: OnceLock<PathBuf> = OnceLock::new();
    CLI.get_or_init(build_cli).clone()
}

fn build_cli() -> PathBuf {
    if let Some(p) =
        std::env::var_os("TIMEGRAPHER_CLI").or_else(|| std::env::var_os("TIMEGRAPHER_MCP_TEST_CLI"))
    {
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
    assert_eq!(r["structuredContent"]["check"]["watch_heard"], true, "{r}");
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
            .any(|m| m.starts_with("Recording CD") && m.contains(" s/d ·")),
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

    analyze_gives_the_clis_document(&mut c, &take);
    sessions_checks_and_mistakes(&mut c, &dir, &take);
    recorded_position_by_position(&mut c, &dir);
    the_compact_series_says_how_it_differs(&mut c, &cli, &take);
    the_view_skill_and_prompt(&mut c, &tools);

    drop(c);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `check_steadiness` gives a compact series document. Its `projection`
/// names it and lists every path it took out, replaced or added, checked
/// here against the CLI's own document for the same take (#1651).
fn the_compact_series_says_how_it_differs(c: &mut Client, cli: &Path, take: &Path) {
    let r = c.call("check_steadiness", json!({ "recordings": [path(take)] }));
    assert!(r.get("isError").is_none(), "{r}");
    let compact = &r["structuredContent"];
    let out = Command::new(cli)
        .arg("series")
        .arg(take)
        .args(["--views", "--json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let full: Value = serde_json::from_slice(&out.stdout).unwrap();
    let p = &compact["projection"];
    assert_eq!(p["schema"], "timegrapher.mcp.series_compact/1");
    assert_eq!(p["of"], full["schema"]);
    assert_eq!(
        compact["schema"], full["schema"],
        "readers keep the CLI's schema"
    );
    let listed = |key: &str| -> Vec<String> {
        p[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };
    let (removed, replaced, added) = (listed("removed"), listed("replaced"), listed("added"));
    let under = |path: &str, roots: &[&Vec<String>]| {
        roots.iter().any(|list| {
            list.iter().any(|r| {
                path == r
                    || path.starts_with(&format!("{r}."))
                    || path.starts_with(&format!("{r}[]"))
            })
        })
    };
    let (full_paths, compact_paths) = (key_paths(&full), key_paths(compact));
    let gone: Vec<&String> = full_paths
        .iter()
        .filter(|k| !compact_paths.contains(*k) && !under(k, &[&removed, &replaced]))
        .collect();
    assert!(gone.is_empty(), "taken out but not listed: {gone:?}");
    let new: Vec<&String> = compact_paths
        .iter()
        .filter(|k| !full_paths.contains(*k) && !under(k, &[&added, &replaced]))
        .collect();
    assert!(new.is_empty(), "added but not listed: {new:?}");
    // What is listed as taken out is out; this take has some of each.
    for r in &removed {
        assert!(!compact_paths.contains(r), "{r} is still there");
    }
    assert!(
        removed.iter().any(|r| full_paths.contains(r)),
        "{full_paths:?}"
    );
}

/// Every object key in a document, as a path: `a.b`, with `[]` for every
/// item of an array.
fn key_paths(v: &Value) -> std::collections::BTreeSet<String> {
    fn walk(v: &Value, at: &str, out: &mut std::collections::BTreeSet<String>) {
        match v {
            Value::Object(o) => {
                for (k, v) in o {
                    let p = if at.is_empty() {
                        k.clone()
                    } else {
                        format!("{at}.{k}")
                    };
                    out.insert(p.clone());
                    walk(v, &p, out);
                }
            }
            Value::Array(a) => {
                for v in a {
                    walk(v, &format!("{at}[]"), out);
                }
            }
            _ => {}
        }
    }
    let mut out = std::collections::BTreeSet::new();
    walk(v, "", &mut out);
    out
}

/// A path as the tools take it.
fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// `analyze_recording` on one take.
fn analyze_gives_the_clis_document(c: &mut Client, take: &Path) {
    // The CLI's own document comes back unchanged, as structured content
    // and as text.
    let r = c.call(
        "analyze_recording",
        json!({ "recording": path(take), "bph": 28800 }),
    );
    assert!(r.get("isError").is_none(), "{r}");
    let doc = &r["structuredContent"];
    assert_eq!(doc["schema"], "timegrapher.analyze/2");
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
    // A beat was found, so the view gets no no-regular-beat sentence.
    assert!(r["_meta"].get("timegrapher/no_beat").is_none());
    assert!(limits
        .iter()
        .any(|l| l["text"].as_str().unwrap().contains("the default 52°")));
    let said = r["content"][0]["text"].as_str().unwrap();
    assert!(said.starts_with("Readings: rate +"), "{said}");
    assert!(!said.contains("confidence"), "{said}");
    assert!(
        said.contains(
            "- The lift angle is the default 52°, not one set for the calibre. Every amplitude \
             is worked out from it."
        ),
        "{said}"
    );
}

/// A session, a check of the recording, and mistakes.
fn sessions_checks_and_mistakes(c: &mut Client, dir: &Path, take: &Path) {
    let r = c.call(
        "run_session",
        json!({ "paths": [path(&dir.join("session"))] }),
    );
    assert!(r.get("isError").is_none(), "{r}");
    assert_eq!(r["structuredContent"]["schema"], "timegrapher.session/2");
    assert_eq!(
        r["structuredContent"]["readings"].as_array().unwrap().len(),
        3
    );
    let said = r["content"][0]["text"].as_str().unwrap();
    assert!(said.starts_with("Readings by position:"), "{said}");
    assert!(said.contains("(dvh_amplitude)"), "{said}");

    let r = c.call("check_recording", json!({ "recording": path(take) }));
    assert_eq!(r["structuredContent"]["schema"], "timegrapher.doctor/2");

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
}

/// Takes recorded one position at a time.
fn recorded_position_by_position(c: &mut Client, dir: &Path) {
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
        assert_eq!(r["structuredContent"]["schema"], "timegrapher.record/2");
        // The facts in words first: the first take's lift angle is the
        // calibre's, and the next take's the one the session file now
        // keeps (#1648).
        let said = r["content"][0]["text"].as_str().unwrap();
        assert!(said.starts_with("Recorded "), "{said}");
        let from = if pos == "DU" {
            "Lift angle 50°: the ETA 2824-2's."
        } else {
            "Lift angle 50°: the take's, from its session file."
        };
        assert!(said.contains(from), "{said}");
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
}

/// The charts view, the skill, the guide and the prompt.
fn the_view_skill_and_prompt(c: &mut Client, tools: &Value) {
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
}

#[test]
fn a_chat_app_finds_a_watchs_takes_in_the_library() {
    use timegrapher_core::library::{Library, Watch, TAKES};
    let cli = cli();
    let dir = std::env::temp_dir().join(format!("timegrapher-mcp-watches-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("Timegrapher Data");
    let mut lib = Library::create(&root).unwrap();
    let mut w = Watch::new("Pepsi");
    w.calibre = Some("Rolex 3135".into());
    w.bph = Some(28800);
    let takes = lib.add(&w).unwrap().join(TAKES);
    let take = takes.join("2026-10-20 14.32 Dial Up");
    std::fs::create_dir_all(&take).unwrap();
    std::fs::write(
        take.join("session.toml"),
        "bph = 28800\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
    )
    .unwrap();
    std::fs::write(take.join("take_part01-001.flac"), vec![0u8; 2000]).unwrap();
    std::fs::write(
        take.join("session.json"),
        r#"{"started_utc":"2026-10-20T21:32:00Z","duration_s":600.0}"#,
    )
    .unwrap();
    let mut old = Watch::new("Sold Seiko");
    old.archived = true;
    lib.add(&old).unwrap();
    drop(lib);

    let sound = dir.join("take.wav");
    synth(&cli, &sound, "6");
    let mut c = Client::start(&cli, &sound);
    let library = root.to_str().unwrap();
    the_library_lists_and_shows_its_watches(&mut c, library, &take);
    a_watchs_take_goes_among_its_takes(&mut c, library, &takes);
    calibres_are_searched_and_a_missing_library_said(&mut c, &dir);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `list_watches` and `show_watch` on the library at `library`, whose
/// Pepsi has the one take `take`.
fn the_library_lists_and_shows_its_watches(c: &mut Client, library: &str, take: &Path) {
    let r = c.call("list_watches", json!({ "library": library }));
    assert_ne!(r["isError"], true, "{r}");
    let doc = &r["structuredContent"];
    assert_eq!(doc["schema"], "timegrapher.watches/1");
    // A watch an older app marked archived is listed like any other.
    assert_eq!(doc["watches"].as_array().unwrap().len(), 2, "{doc}");
    assert!(r["content"][0]["text"]
        .as_str()
        .unwrap()
        .starts_with("2 watches, 1 take."));
    let r = c.call("list_watches", json!({ "library": library, "all": true }));
    assert_eq!(
        r["structuredContent"]["watches"].as_array().unwrap().len(),
        2
    );

    // A take's path from show_watch is the folder itself, ready for
    // run_session and the other tools.
    let r = c.call("show_watch", json!({ "name": "pepsi", "library": library }));
    assert_ne!(r["isError"], true, "{r}");
    let doc = &r["structuredContent"];
    assert_eq!(doc["schema"], "timegrapher.watch/1");
    assert_eq!(doc["takes"][0]["positions"], json!(["DU"]));
    let path = std::path::PathBuf::from(doc["takes"][0]["path"].as_str().unwrap());
    assert_eq!(path.canonicalize().unwrap(), take.canonicalize().unwrap());
    assert!(r["content"][0]["text"]
        .as_str()
        .unwrap()
        .starts_with("Pepsi: 1 take; the newest started 2026-10-20T21:32:00Z."));

    let r = c.call("show_watch", json!({ "name": "Omega", "library": library }));
    assert_eq!(r["isError"], true, "{r}");
}

/// `record_position` by a watch's name, into its `takes` folder.
fn a_watchs_take_goes_among_its_takes(c: &mut Client, library: &str, takes: &Path) {
    // With no folder, a take goes into a new test among the watch's takes,
    // and a retry finds it by the watch's name in any case.
    let r = c.call(
        "record_position",
        json!({ "watch": " pepsi ", "library": library, "position": "DU",
                "seconds": 30, "take_id": "lib-1" }),
    );
    assert_ne!(r["isError"], true, "{r}");
    let doc = &r["structuredContent"];
    assert_eq!(doc["library_watch"], "Pepsi");
    let folder = Path::new(doc["session_file"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    assert_eq!(
        folder.parent().unwrap().canonicalize().unwrap(),
        takes.canonicalize().unwrap()
    );
    let said = r["content"][0]["text"].as_str().unwrap();
    assert!(
        said.ends_with(&format!(
            "The test's folder, made among Pepsi's takes, is {}.",
            folder.display()
        )),
        "{said}"
    );
    let again = c.call(
        "record_position",
        json!({ "watch": "PEPSI", "library": library, "position": "DU",
                "seconds": 30, "take_id": "lib-1" }),
    );
    assert!(again["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("nothing was recorded again"));
    assert_eq!(std::fs::read_dir(takes).unwrap().count(), 2, "one new test");
    let r = c.call("record_position", json!({ "position": "DU" }));
    assert_eq!(r["isError"], true, "{r}");
    assert!(r["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("give folder, or watch"));
}

/// A calibre search, and a library that isn't there.
fn calibres_are_searched_and_a_missing_library_said(c: &mut Client, dir: &Path) {
    // A search finds calibres by any of their names.
    let r = c.call("list_calibres", json!({ "search": "2824" }));
    assert_ne!(r["isError"], true, "{r}");
    assert_eq!(r["structuredContent"]["listed"], "matching", "{r}");
    assert!(r["structuredContent"]["calibres"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["calibre"] == "ETA 2824-2"));
    let r = c.call(
        "list_watches",
        json!({ "library": dir.join("none").to_str().unwrap() }),
    );
    assert_eq!(r["isError"], true, "{r}");
}
