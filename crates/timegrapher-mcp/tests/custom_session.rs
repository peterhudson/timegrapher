//! Real CLI/MCP file-surrogate integration. CI runs this; no native capture is tested.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

fn cli() -> PathBuf {
    if let Some(path) = std::env::var_os("TIMEGRAPHER_CLI") {
        return path.into();
    }
    // Follow the existing stdio suite's separate nested target (avoid outer lock).
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = root.join("target/mcp-test-cli");
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "--locked",
            "--quiet",
            "-p",
            "timegrapher-cli",
            "--no-default-features",
            "--release",
        ])
        .env("CARGO_TARGET_DIR", &target)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success());
    target
        .join("release")
        .join(format!("timegrapher{}", std::env::consts::EXE_SUFFIX))
}
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Client {
    child: Child,
    messages: Receiver<String>,
    id: u64,
}
impl Client {
    fn start(cli: &Path, input: &Path, config: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_timegrapher-mcp"))
            .env("TIMEGRAPHER_CLI", cli)
            .env("TIMEGRAPHER_MCP_TEST_INPUT", input)
            .env("TIMEGRAPHER_CONFIG_DIR", config)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, messages) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            messages,
            id: 0,
        };
        client.request("initialize",json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"custom-session-test","version":"1"}}));
        client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        client
    }
    fn send(&mut self, value: Value) {
        let input = self.child.stdin.as_mut().unwrap();
        writeln!(input, "{value}").unwrap();
        input.flush().unwrap();
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        let deadline = std::time::Instant::now() + Duration::from_secs(180);
        loop {
            let line = self
                .messages
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("bounded MCP reply");
            let message: Value = serde_json::from_str(&line).unwrap();
            if message["id"] == id {
                assert!(message.get("error").is_none(), "{message}");
                return message["result"].clone();
            }
        }
    }
    fn call(&mut self, name: &str, args: Value) -> Value {
        let r = self.request("tools/call", json!({"name":name,"arguments":args}));
        assert_ne!(r["isError"], true, "{r}");
        r
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn run(cli: &Path, config: &Path, args: &[&str]) -> Value {
    let out = Command::new(cli)
        .env("TIMEGRAPHER_CONFIG_DIR", config)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn custom_alias_file_recording_roundtrips_through_cli_and_mcp_session() {
    let cli = cli();
    let root = std::env::temp_dir().join(format!("tg-custom-session-e2e-{}", std::process::id()));
    std::fs::create_dir_all(root.join("config")).unwrap();
    let root = Scratch(root);
    let config = root.0.join("config");
    std::fs::write(config.join("calibres.json"),json!({"schema":"timegrapher.user-calibres/1","wheels":[],"custom":[{
        "name":"Bench 44","aliases":["Workshop-A"],"bph":21600,"lift_angle_deg":44.0,"escape_teeth":20,
        "wheels":[{"name":"third wheel","period_s":123.0}]}]}).to_string()).unwrap();
    let wav = root.0.join("watch.wav");
    let folder = root.0.join("session");
    let status = Command::new(&cli)
        .env("TIMEGRAPHER_CONFIG_DIR", &config)
        .arg("synth")
        .arg(&wav)
        .args(["--duration", "40", "--bph", "21600"])
        .status()
        .unwrap();
    assert!(status.success());
    let path = |p: &Path| p.to_string_lossy().into_owned();
    let wav_text = path(&wav);
    let folder_text = path(&folder);
    let first = run(
        &cli,
        &config,
        &[
            "record",
            &folder_text,
            "--file",
            &wav_text,
            "--seconds",
            "35",
            "--position",
            "DU",
            "--calibre",
            "Workshop-A",
            "--json",
        ],
    );
    assert_eq!(first["input"]["settings"]["calibre"], "Bench 44");
    assert_eq!(first["input"]["settings"]["bph"], 21600);
    assert_eq!(first["input"]["settings"]["lift_deg"], 44.0);
    let mut client = Client::start(&cli, &wav, &config);
    let discovery = client.call("list_calibres", json!({"name":"Workshop-A"}));
    assert_eq!(
        discovery["structuredContent"]["custom"][0]["name"],
        "Bench 44"
    );
    let second=client.call("record_position",json!({"folder":folder_text,"position":"DD","seconds":35,"calibre":"Workshop-A","take_id":"custom-dd"}));
    assert_eq!(
        second["structuredContent"]["input"]["settings"]["lift_deg"],
        44.0
    );
    let manifest = std::fs::read_to_string(folder.join("session.toml")).unwrap();
    assert!(manifest
        .lines()
        .any(|line| line == "calibre = \"Bench 44\""));
    assert!(manifest.lines().any(|line| line == "lift = 44.0"));
    assert!(manifest.lines().any(|line| line == "lift_known = true"));
    assert_eq!(manifest.matches("[[recording]]").count(), 2);
    let expected = run(&cli, &config, &["session", &folder_text, "--json"]);
    let actual = client.call("run_session", json!({"paths":[folder_text]}));
    let actual = &actual["structuredContent"];
    assert_eq!(actual["input"]["settings"], expected["input"]["settings"]);
    assert_eq!(actual["input"]["settings"]["calibre"], "Bench 44");
    assert_eq!(actual["input"]["settings"]["bph"], 21600);
    assert_eq!(actual["input"]["settings"]["lift_deg"], 44.0);
    assert_eq!(actual["readings"], expected["readings"]);
    assert_eq!(actual["readings"].as_array().unwrap().len(), 2);
    for reading in actual["readings"].as_array().unwrap() {
        assert_eq!(reading["bph"], 21600);
        assert_eq!(reading["measurement"]["watch_heard"], true);
        assert!(reading["measurement"]["caveats"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["code"] != "lift_unknown"));
    }
}
