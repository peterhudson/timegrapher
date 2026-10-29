//! Subprocess contracts over MCP stdio, without building the CLI or
//! opening sound input. The deterministic CLI doubles use the Unix shell.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{json, Value};

struct Client {
    child: Child,
    messages: Receiver<String>,
    next_id: u64,
}

impl Client {
    fn start(cli: &Path, config: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_timegrapher-mcp"))
            .env("TIMEGRAPHER_CLI", cli)
            .env("TIMEGRAPHER_CONFIG_DIR", config)
            .env_remove("TIMEGRAPHER_MCP_TEST_INPUT")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start MCP server");
        let stdout = child.stdout.take().unwrap();
        let (sender, messages) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            messages,
            next_id: 0,
        };
        client.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "child-outcome-test", "version": "1" },
            }),
        );
        client.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        client
    }

    fn send(&mut self, message: Value) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let line = self
                .messages
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("bounded MCP response");
            let message: Value = serde_json::from_str(&line).expect("JSON protocol response");
            if message["id"] == id {
                assert!(message.get("error").is_none(), "{message}");
                return message["result"].clone();
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn analysis_double(dir: &Path, doc: &Value) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("analyze.json"), doc.to_string()).unwrap();
    std::fs::write(
        dir.join("profile.json"),
        r#"{"schema":"timegrapher.profile/1","profile_seen":true}"#,
    )
    .unwrap();
    let cli = dir.join("cli");
    std::fs::write(
        &cli,
        r#"#!/bin/sh
folder=$(dirname "$0")
case "$1" in
analyze) cat "$folder/analyze.json" ;;
profile)
    printf '%s\n' "$@" > "$folder/profile.args"
    cat "$folder/profile.json"
    ;;
*) exit 1 ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
    cli
}

#[test]
fn analyze_result_and_lead_keep_the_clis_authoritative_clock() {
    let dir = Scratch(std::env::temp_dir().join(format!(
        "timegrapher-mcp-clock-contract-{}",
        std::process::id()
    )));
    let cases: Value = serde_json::from_str(include_str!("fixtures/analysis_rates.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let mut doc = case["doc"].clone();
        doc["duration_s"] = json!(60);
        doc["bph"] = json!(21600);
        doc["input"] = json!({ "settings": { "lift_deg": 52 } });
        let cli = analysis_double(&dir.0.join(name), &doc);
        let mut client = Client::start(&cli, &dir.0.join("config"));
        let result = client.request(
            "tools/call",
            json!({ "name": "analyze_recording", "arguments": {
                "recording": dir.0.join("unused.wav").to_string_lossy() } }),
        );
        assert_ne!(result["isError"], true, "{name}: {result}");
        assert_eq!(
            result["structuredContent"], doc,
            "{name}: raw/USB data changed"
        );
        let lead = result["content"][0]["text"].as_str().unwrap();
        assert!(
            lead.starts_with(case["expected_lead"].as_str().unwrap()),
            "{name}: {lead}"
        );
        assert!(
            lead.contains(case["expected_basis"].as_str().unwrap()),
            "{name}: {lead}"
        );
    }
}

#[test]
fn auxiliary_profile_uses_effective_analysis_settings() {
    let dir = Scratch(std::env::temp_dir().join(format!(
        "timegrapher-mcp-profile-contract-{}",
        std::process::id()
    )));
    for (name, args, lift, bph) in [
        ("calibre", json!({ "calibre": "ETA 6497-2" }), 44, 21600),
        (
            "override",
            json!({ "calibre": "ETA 6497-2", "lift_deg": 50, "bph": 18000 }),
            50,
            18000,
        ),
        ("default", json!({}), 52, 28800),
        ("top-level-lift", json!({}), 53, 21600),
    ] {
        let mut doc = json!({
            "schema": "timegrapher.analyze/2", "watch_heard": true,
            "duration_s": 60, "bph": bph, "lift_deg": lift,
            "overall": { "rate_s_per_day": 6 },
            "input": { "settings": { "lift_deg": lift } },
        });
        if name == "top-level-lift" {
            doc["input"]["settings"]
                .as_object_mut()
                .unwrap()
                .remove("lift_deg");
        }
        let cli = analysis_double(&dir.0.join(name), &doc);
        let mut client = Client::start(&cli, &dir.0.join("config"));
        let mut args = args;
        args["recording"] = json!(dir.0.join("unused.wav").to_string_lossy());
        let result = client.request(
            "tools/call",
            json!({ "name": "analyze_recording", "arguments": args }),
        );
        assert_ne!(result["isError"], true, "{name}: {result}");
        assert_eq!(result["structuredContent"], doc);
        assert_eq!(
            result["_meta"]["timegrapher/charts"]["profile"]["profile_seen"],
            true
        );
        let child_args = std::fs::read_to_string(dir.0.join(name).join("profile.args")).unwrap();
        let child_args: Vec<_> = child_args.lines().collect();
        assert!(
            child_args.contains(&format!("--lift={lift}").as_str()),
            "{name}: {child_args:?}"
        );
        assert!(
            child_args.contains(&format!("--bph={bph}").as_str()),
            "{name}: {child_args:?}"
        );
        // Core's beat-shape stretch: settled after 20 s, then the 40 s left
        // of this 60 s recording.
        assert!(
            child_args.contains(&"--at=20"),
            "profile reads core's beat-shape stretch: {child_args:?}"
        );
        assert!(child_args.contains(&"--span=40"), "{child_args:?}");
    }
}

#[test]
fn no_regular_beat_reaches_the_view_in_cores_words() {
    let dir = Scratch(
        std::env::temp_dir().join(format!("timegrapher-mcp-no-beat-{}", std::process::id())),
    );
    let doc = json!({
        "schema": "timegrapher.analyze/2", "watch_heard": false,
        "duration_s": 60, "bph": 28800, "lift_deg": 52, "jitter_us": 8600.0,
    });
    let cli = analysis_double(&dir.0.join("cli"), &doc);
    let mut client = Client::start(&cli, &dir.0.join("config"));
    let result = client.request(
        "tools/call",
        json!({ "name": "analyze_recording",
            "arguments": { "recording": dir.0.join("unused.wav").to_string_lossy() } }),
    );
    assert_ne!(result["isError"], true, "{result}");
    assert_eq!(
        result["_meta"]["timegrapher/no_beat"],
        "No regular beat found (timing scatter 8.6 ms, 6.9% of a beat). There are no rate, \
         amplitude or beat error readings."
    );
}

#[test]
fn invalid_capture_seconds_never_spawn_a_child() {
    let dir = Scratch(std::env::temp_dir().join(format!(
        "timegrapher-mcp-duration-contract-{}",
        std::process::id()
    )));
    std::fs::create_dir_all(&dir.0).unwrap();
    let cli = dir.0.join("cli");
    std::fs::write(
        &cli,
        r#"#!/bin/sh
folder=$(dirname "$0")
printf '%s\n' "$@" > "$folder/started"
printf '%s\n' '{"schema":"timegrapher.test/1"}'
"#,
    )
    .unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut client = Client::start(&cli, &dir.0.join("config"));
    for tool in ["record_position", "check_microphone"] {
        for seconds in [
            json!(0),
            json!(-1),
            json!(1e300),
            json!(1e-300),
            json!("NaN"),
        ] {
            let mut args = json!({ "seconds": seconds });
            if tool == "record_position" {
                args["folder"] = json!(dir.0.join("not-created").to_string_lossy());
                args["position"] = json!("DU");
            }
            let result = client.request("tools/call", json!({ "name": tool, "arguments": args }));
            assert_eq!(result["isError"], true, "{tool}: {result}");
            assert!(
                !dir.0.join("started").exists(),
                "invalid {tool} spawned the CLI"
            );
        }
    }
    // Omitting the acquisition length still delegates to the CLI's defaults.
    let result = client.request(
        "tools/call",
        json!({ "name": "check_microphone", "arguments": {} }),
    );
    assert_ne!(result["isError"], true, "{result}");
    let args = std::fs::read_to_string(dir.0.join("started")).unwrap();
    assert!(!args.contains("--seconds"), "{args}");
}

#[test]
fn child_exit_status_controls_success_even_with_json() {
    let dir = Scratch(std::env::temp_dir().join(format!(
        "timegrapher-mcp-child-outcome-{}",
        std::process::id()
    )));
    std::fs::create_dir_all(&dir.0).unwrap();
    let diagnostics = json!({
        "schema": "timegrapher.calibres/1",
        "calibres": [],
        "diagnostics": { "partial": true, "reason": "test fixture" },
    });
    for (case, stdout, finish, expected_code, expected_json) in [
        ("success", diagnostics.to_string(), "exit 0", "", true),
        (
            "failed-json",
            diagnostics.to_string(),
            "exit 1",
            "exit code 1",
            true,
        ),
        (
            "other-failed-json",
            diagnostics.to_string(),
            "exit 3",
            "exit code 3",
            true,
        ),
        (
            "failed-scalar-json",
            "42".into(),
            "exit 1",
            "exit code 1",
            true,
        ),
        (
            "malformed-success",
            "not JSON".into(),
            "exit 0",
            "exit code 0",
            false,
        ),
        (
            "malformed-failure",
            "not JSON".into(),
            "exit 1",
            "exit code 1",
            false,
        ),
        (
            "signal-json",
            diagnostics.to_string(),
            "kill -TERM \"$$\"",
            "no exit code",
            true,
        ),
    ] {
        let cli = dir.0.join(case);
        let invocations = dir.0.join(format!("{case}.calls"));
        // Fixtures contain no apostrophes; all arguments to the double are ignored.
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' call >> '{}'\nprintf '%s\\n' '{stdout}'\nprintf '%s\\n' 'child diagnostic' >&2\n{finish}\n",
            invocations.display()
        );
        std::fs::write(&cli, script).unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut client = Client::start(&cli, &dir.0.join("config"));
        let result = client.request(
            "tools/call",
            json!({ "name": "list_calibres", "arguments": {} }),
        );
        let text = result["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("child diagnostic"), "{case}: {result}");
        if case == "success" {
            assert_ne!(result["isError"], true, "{result}");
        } else {
            assert_eq!(result["isError"], true, "{case}: {result}");
            assert!(text.contains(expected_code), "{case}: {result}");
            assert!(text.contains("timegrapher calibres failed"), "{result}");
        }
        if expected_json {
            assert_eq!(
                result["structuredContent"],
                serde_json::from_str::<Value>(&stdout).unwrap(),
                "{case}: {result}"
            );
            assert!(text.contains(&stdout), "{case}: {result}");
        } else {
            assert!(result.get("structuredContent").is_none(), "{result}");
        }
        if case == "failed-json" {
            let params = json!({
                "name": "record_position",
                "arguments": { "folder": dir.0.to_string_lossy(), "position": "DU",
                    "take_id": "failed-take" },
            });
            // A failed take with diagnostics is never remembered as a successful
            // recording, even when retried using the same take identifier.
            for _ in 0..2 {
                assert_eq!(
                    client.request("tools/call", params.clone())["isError"],
                    true
                );
            }
            assert_eq!(
                std::fs::read_to_string(&invocations)
                    .unwrap()
                    .lines()
                    .count(),
                3,
                "one calibre call and both failed recording attempts ran"
            );
        }
        // An ordinary tool failure leaves the server usable for the next request.
        assert!(client.request("tools/list", json!({}))["tools"].is_array());
    }
}

#[test]
fn numeric_inputs_are_rejected_before_any_cli_child_is_spawned() {
    let dir = Scratch(std::env::temp_dir().join(format!("tg-mcp-numeric-{}", std::process::id())));
    std::fs::create_dir_all(&dir.0).unwrap();
    let cli = dir.0.join("cli");
    std::fs::write(
        &cli,
        r#"#!/bin/sh
folder=$(dirname "$0")
printf '%s\n' "$@" > "$folder/spawned"
printf '%s\n' '{"schema":"timegrapher.analyze/2","watch_heard":false}'
"#,
    )
    .unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
    let marker = dir.0.join("spawned");
    let mut client = Client::start(&cli, &dir.0.join("config"));
    for (field, value) in [
        ("lift_deg", json!(0)),
        ("lift_deg", json!(-1)),
        ("bph", json!(u32::MAX as u64 + 1)),
        ("bph", json!(0)),
    ] {
        let mut args = json!({"recording":dir.0.join("not-read.wav").to_string_lossy()});
        args[field] = value;
        let result = client.request(
            "tools/call",
            json!({"name":"analyze_recording","arguments":args}),
        );
        assert!(
            !marker.exists(),
            "invalid {field} reached the actual CLI child: {result}"
        );
        assert_eq!(result["isError"], true, "{result}");
    }
    // Positive control proves the double is reachable and marker detection works.
    let result = client.request(
        "tools/call",
        json!({"name":"analyze_recording","arguments":{
        "recording":dir.0.join("not-read.wav").to_string_lossy(),"lift_deg":52,"bph":28800}}),
    );
    assert_ne!(result["isError"], true, "{result}");
    assert!(marker.is_file());
    let args = std::fs::read_to_string(marker).unwrap();
    assert!(args.contains("--lift=52"));
    assert!(args.contains("--bph=28800"));
}
