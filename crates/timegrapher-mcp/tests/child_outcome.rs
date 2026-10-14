//! Subprocess failure semantics over MCP stdio, without building the CLI or
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
