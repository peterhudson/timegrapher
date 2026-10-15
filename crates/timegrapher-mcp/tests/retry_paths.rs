//! Retry identity and native argv over MCP stdio, using zero-audio shell doubles.
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
    fn start(cli: &Path, config: &Path, home: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_timegrapher-mcp"))
            .env("TIMEGRAPHER_CLI", cli)
            .env("TIMEGRAPHER_CONFIG_DIR", config)
            .env("HOME", home)
            .env("USERPROFILE", home)
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

fn fixture(label: &str) -> (Scratch, PathBuf) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root =
        Scratch(std::env::temp_dir().join(format!("tg-retry-{label}-{}-{n}", std::process::id())));
    std::fs::create_dir_all(&root.0).unwrap();
    let cli = root.0.join("cli");
    std::fs::write(
        &cli,
        r#"#!/bin/sh
set -eu
root=$(dirname "$0")
# Only record is expected. Its sole positional folder must still be last,
# with -- immediately before it even when flags were added before it.
[ "$1" = record ]
previous=''
for argument do
  folder=$argument
  penultimate=$previous
  previous=$argument
done
[ "$penultimate" = -- ]
printf '%s\n' "$folder" >> "$root/folders"
mkdir -p "$folder"
count=$(wc -l < "$root/folders" | tr -d ' ')
printf '{"schema":"timegrapher.test/1","invocation":%s}\n' "$count"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
    (root, cli)
}
fn record(client: &mut Client, folder: &str, id: &str, position: &str) -> Value {
    client.request(
        "tools/call",
        json!({"name":"record_position", "arguments":{
        "folder":folder, "take_id":id, "position":position }}),
    )
}
fn invocation(result: &Value) -> u64 {
    assert_ne!(result["isError"], true, "{result}");
    result["structuredContent"]["invocation"].as_u64().unwrap()
}

#[test]
fn aliases_retry_after_destination_creation_but_distinct_takes_still_record() {
    use std::os::unix::fs::symlink;
    let (root, cli) = fixture("aliases");
    std::fs::create_dir(root.0.join("real")).unwrap();
    symlink(root.0.join("real"), root.0.join("alias")).unwrap();
    let folder = root.0.join("real/watch");
    assert!(!folder.exists());
    let mut client = Client::start(&cli, &root.0.join("config"), &root.0);
    assert_eq!(
        invocation(&record(&mut client, folder.to_str().unwrap(), "one", "DU")),
        1
    );
    assert!(
        folder.is_dir(),
        "first fake child creates the real destination"
    );
    for alias in [
        root.0.join("alias/watch"),
        root.0.join("real/../real/watch/."),
    ] {
        let result = record(&mut client, alias.to_str().unwrap(), "one", "DU");
        assert_eq!(invocation(&result), 1);
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("nothing was recorded again"));
    }
    assert_eq!(
        invocation(&record(&mut client, "~/real/watch", "one", "DU")),
        1
    );
    let physical = std::fs::canonicalize(&folder).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.0.join("folders")).unwrap(),
        format!("{}\n", physical.display()),
        "actual CLI argv must equal the key's resolved folder"
    );
    assert_eq!(
        invocation(&record(&mut client, folder.to_str().unwrap(), "two", "DU")),
        2
    );
    assert_eq!(
        invocation(&record(&mut client, folder.to_str().unwrap(), "one", "DD")),
        3
    );
    assert_eq!(
        invocation(&record(
            &mut client,
            root.0.join("other").to_str().unwrap(),
            "one",
            "DU"
        )),
        4
    );
}

#[test]
fn invalid_destination_returns_tool_error_without_spawning_or_creating_output() {
    use std::os::unix::fs::symlink;
    let (root, cli) = fixture("errors");
    std::fs::write(root.0.join("file"), []).unwrap();
    symlink(root.0.join("missing"), root.0.join("dangling")).unwrap();
    let mut client = Client::start(&cli, &root.0.join("config"), &root.0);
    for folder in [
        "relative/path".to_string(),
        root.0.join("file/child").display().to_string(),
        root.0.join("dangling/child").display().to_string(),
    ] {
        let result = record(&mut client, &folder, "one", "DU");
        assert_eq!(result["isError"], true, "{result}");
        assert!(
            result["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("path")
                || result["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("folder")
        );
    }
    assert!(
        !root.0.join("folders").exists(),
        "invalid identity must not silently disable retry protection"
    );
    assert!(!root.0.join("missing").exists());
}

// The real raw-byte filesystem fixture runs on Linux. Native identity storage
// is covered without filename creation by portable Unix unit tests as well.
#[cfg(target_os = "linux")]
#[test]
fn native_home_and_canonical_destination_argv_are_not_lossy() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::os::unix::fs::symlink;
    let (root, cli) = fixture("native");
    let native = root
        .0
        .join(std::ffi::OsString::from_vec(b"native-\xff".to_vec()));
    let lossy = root.0.join("native-�");
    std::fs::create_dir(&native).unwrap();
    std::fs::create_dir(&lossy).unwrap();
    symlink(&native, root.0.join("alias")).unwrap();
    let mut client = Client::start(&cli, &root.0.join("config"), &native);
    assert_eq!(invocation(&record(&mut client, "~/watch", "one", "DU")), 1);
    assert_eq!(
        invocation(&record(
            &mut client,
            root.0.join("alias/watch").to_str().unwrap(),
            "one",
            "DU"
        )),
        1
    );
    let physical = std::fs::canonicalize(native.join("watch")).unwrap();
    let mut expected = physical.as_os_str().as_bytes().to_vec();
    expected.push(b'\n');
    assert_eq!(
        std::fs::read(root.0.join("folders")).unwrap(),
        expected,
        "resolved nonUTF8 path must be passed unchanged to the direct child"
    );
    assert_eq!(
        invocation(&record(
            &mut client,
            lossy.join("watch").to_str().unwrap(),
            "one",
            "DU"
        )),
        2
    );
}
