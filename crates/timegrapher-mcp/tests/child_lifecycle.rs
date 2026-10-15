//! Deterministic direct-child lifecycle probes; no CLI, audio or devices.
#![cfg(unix)]
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const PROCESSING: &str = "TIMEGRAPHER_MCP_PROCESSING_TIMEOUT_SECONDS";
const GRACE: &str = "TIMEGRAPHER_MCP_ACQUISITION_GRACE_SECONDS";
const PROFILE: &str = "TIMEGRAPHER_MCP_PROFILE_TIMEOUT_SECONDS";

struct Fixture(PathBuf);
impl Fixture {
    fn new(mode: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("tg-mcp-lifecycle-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("mode"), mode).unwrap();
        std::fs::write(root.join("cli"), r#"#!/bin/sh
folder=$(dirname "$0")
mode=$(cat "$folder/mode")
phase=$1
if [ "$phase" = analyze ] && [ "$mode" != principal ]; then
  printf '%s\n' '{"schema":"timegrapher.analyze/2","duration_s":1,"bph":28800,"watch_heard":true,"primary_preserved":true}'
  exit 0
fi
if [ "$phase" = profile ] && [ "$mode" = nonzero ]; then
  printf '%s\n' '{"schema":"timegrapher.profile/1","failed_profile":true}'
  printf '%s\n' 'profile diagnostic' >&2
  exit 1
fi
if [ "$phase" = record ] && [ -f "$folder/record.pid" ]; then
  printf '%s\n' '{"schema":"timegrapher.test/1","retried":true}'
  exit 0
fi
printf '%s\n' "$$" > "$folder/$phase.pid"
# exec keeps the blocked process the direct child whose PID was recorded.
exec sleep 30
"#).unwrap();
        std::fs::set_permissions(root.join("cli"), std::fs::Permissions::from_mode(0o700)).unwrap();
        Self(root)
    }
    fn pid(&self, phase: &str) -> u32 {
        let path = self.0.join(format!("{phase}.pid"));
        wait_until(|| path.exists(), "fake child start");
        std::fs::read_to_string(path)
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Test assertion failures must not leave a direct fake child running.
        if let Ok(files) = std::fs::read_dir(&self.0) {
            for file in files
                .flatten()
                .filter(|f| f.path().extension().is_some_and(|e| e == "pid"))
            {
                if let Ok(pid) = std::fs::read_to_string(file.path()) {
                    let _ = Command::new("/bin/kill")
                        .args(["-KILL", pid.trim()])
                        .stderr(Stdio::null())
                        .status();
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn alive(pid: u32) -> bool {
    Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}
fn wait_until(mut condition: impl FnMut() -> bool, description: &str) {
    let end = Instant::now() + Duration::from_secs(4);
    while !condition() {
        assert!(Instant::now() < end, "bounded wait: {description}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
struct Client {
    child: Child,
    messages: Receiver<String>,
    id: u64,
}
impl Client {
    fn new(fixture: &Fixture, env: &[(&str, &str)]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_timegrapher-mcp"));
        command
            .env("TIMEGRAPHER_CLI", fixture.0.join("cli"))
            .env("TIMEGRAPHER_CONFIG_DIR", fixture.0.join("config"))
            .env_remove("TIMEGRAPHER_MCP_TEST_INPUT");
        for name in [PROCESSING, GRACE, PROFILE] {
            command.env_remove(name);
        }
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
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
            id: 0,
        };
        let id = client.begin(
            "initialize",
            json!({"protocolVersion":"2025-06-18", "capabilities":{},
            "clientInfo":{"name":"lifecycle-test","version":"1"}}),
        );
        client.result(id);
        client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        client
    }
    fn send(&mut self, message: Value) {
        let input = self.child.stdin.as_mut().unwrap();
        writeln!(input, "{message}").unwrap();
        input.flush().unwrap();
    }
    fn begin(&mut self, method: &str, params: Value) -> u64 {
        self.id += 1;
        self.send(json!({"jsonrpc":"2.0", "id":self.id, "method":method, "params":params}));
        self.id
    }
    fn analyze(&mut self, fixture: &Fixture) -> u64 {
        self.begin("tools/call", json!({"name":"analyze_recording", "arguments":{"recording":fixture.0.join("unused.wav")}}))
    }
    fn result(&self, id: u64) -> Value {
        let end = Instant::now() + Duration::from_secs(4);
        loop {
            let line = self
                .messages
                .recv_timeout(end.saturating_duration_since(Instant::now()))
                .expect("bounded MCP response");
            let message: Value = serde_json::from_str(&line).unwrap();
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
fn text(result: &Value) -> String {
    result["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn principal_deadline_kills_and_reports_a_tool_error() {
    let fixture = Fixture::new("principal");
    let mut client = Client::new(&fixture, &[(PROCESSING, "1")]);
    let id = client.analyze(&fixture);
    let pid = fixture.pid("analyze");
    let result = client.result(id);
    assert_eq!(result["isError"], true, "{result}");
    assert!(text(&result).contains("deadline"), "{result}");
    assert!(!alive(pid), "deadline response preceded child cleanup");
}
#[test]
fn profile_deadlines_preserve_primary_and_never_exceed_remaining_call_budget() {
    for env in [
        [(PROCESSING, "3"), (PROFILE, "1")],
        [(PROCESSING, "1"), (PROFILE, "30")],
    ] {
        let fixture = Fixture::new("profile");
        let mut client = Client::new(&fixture, &env);
        let start = Instant::now();
        let id = client.analyze(&fixture);
        let pid = fixture.pid("profile");
        let result = client.result(id);
        assert_ne!(result["isError"], true, "{result}");
        assert_eq!(result["structuredContent"]["primary_preserved"], true);
        assert!(result["_meta"]["timegrapher/charts"]
            .get("profile")
            .is_none());
        assert!(
            text(&result).contains("Auxiliary Tick/Tock profile unavailable"),
            "{result}"
        );
        assert!(text(&result).contains("deadline"), "{result}");
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(!alive(pid));
    }
}
#[test]
fn nonzero_profile_json_is_diagnostic_and_does_not_replace_primary_success() {
    let fixture = Fixture::new("nonzero");
    let mut client = Client::new(&fixture, &[]);
    let id = client.analyze(&fixture);
    let result = client.result(id);
    assert_ne!(result["isError"], true, "{result}");
    assert_eq!(result["structuredContent"]["primary_preserved"], true);
    assert!(result["_meta"]["timegrapher/charts"]
        .get("profile")
        .is_none());
    assert!(text(&result).contains("profile diagnostic"), "{result}");
}
#[test]
fn cancellation_stops_principal_and_auxiliary_children() {
    for phase in ["analyze", "profile"] {
        let fixture = Fixture::new(if phase == "analyze" {
            "principal"
        } else {
            "profile"
        });
        let mut client = Client::new(&fixture, &[]);
        let id = client.analyze(&fixture);
        let pid = fixture.pid(phase);
        client.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id,"reason":"test cancellation"}}));
        wait_until(|| !alive(pid), "cancelled child reaped");
        // A cancelled request may have no response, but the server stays usable.
        let list = client.begin("tools/list", json!({}));
        assert!(client.result(list)["tools"].is_array());
    }
}
#[test]
fn stdin_disconnect_stops_principal_and_auxiliary_children() {
    for phase in ["analyze", "profile"] {
        let fixture = Fixture::new(if phase == "analyze" {
            "principal"
        } else {
            "profile"
        });
        let mut client = Client::new(&fixture, &[]);
        client.analyze(&fixture);
        let pid = fixture.pid(phase);
        drop(client.child.stdin.take());
        wait_until(|| !alive(pid), "disconnected child reaped");
        wait_until(
            || client.child.try_wait().unwrap().is_some(),
            "server EOF exit",
        );
    }
}
#[test]
fn acquisition_deadline_releases_mic_and_does_not_cache_failed_take() {
    let fixture = Fixture::new("principal");
    let mut client = Client::new(&fixture, &[(GRACE, "1")]);
    let params = json!({"name":"record_position", "arguments":{"folder":fixture.0.join("unused"),"position":"DU","take_id":"same-take","seconds":0.01}});
    let id = client.begin("tools/call", params.clone());
    let pid = fixture.pid("record");
    let failed = client.result(id);
    assert_eq!(failed["isError"], true, "{failed}");
    assert!(!alive(pid));
    let id = client.begin("tools/call", params);
    let retried = client.result(id);
    assert_ne!(retried["isError"], true, "{retried}");
    assert_eq!(retried["structuredContent"]["retried"], true, "{retried}");
}
#[test]
fn invalid_startup_configuration_fails_before_starting_any_cli() {
    for name in [PROCESSING, GRACE, PROFILE] {
        let fixture = Fixture::new("principal");
        let out = Command::new(env!("CARGO_BIN_EXE_timegrapher-mcp"))
            .env(name, "NaN")
            .env("TIMEGRAPHER_CLI", fixture.0.join("cli"))
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains(name));
        assert!(!fixture.0.join("analyze.pid").exists());
    }
}
