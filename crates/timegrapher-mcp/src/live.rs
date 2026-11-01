//! The Live view's feed. While `record_position` records, `timegrapher
//! record --live FILE` keeps its latest readings in a temporary file
//! (`timegrapher.live/1`), and the app-only tool `recording_status` hands
//! that file to the view, which asks for it about once a second. The tool
//! reads the file and nothing else: it runs no command and never opens the
//! microphone.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use rmcp::service::RequestContext;
use rmcp::RoleServer;
use serde_json::{json, Value};

pub const TOOL: &str = "recording_status";
const SCHEMA: &str = "timegrapher.live/1";

/// The file of the recording running now, if any.
static CURRENT: Mutex<Option<PathBuf>> = Mutex::new(None);

/// One recording's file: named when the recording starts, deleted when the
/// call ends however it ends (done, failed or cancelled).
pub struct Feed {
    path: PathBuf,
}

impl Feed {
    pub fn start() -> Feed {
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "timegrapher-live-{}-{}.json",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(path.clone());
        Feed { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        let mut current = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
        if current.as_deref() == Some(self.path.as_path()) {
            *current = None;
        }
        let _ = std::fs::remove_file(&self.path);
        // The command writes each update beside the file, then renames it.
        let _ = std::fs::remove_file(self.path.with_extension("tmp"));
    }
}

/// The latest readings in a feed file, if it has any yet.
pub fn read(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&text)
        .ok()
        .filter(|d| d["schema"] == SCHEMA)
}

/// Whether a call can show live readings: the chat app draws views (the
/// MCP Apps extension) or asked for progress messages. Otherwise the
/// recording is not slowed by a feed nobody reads.
pub fn wanted(context: &RequestContext<RoleServer>) -> bool {
    if context.meta.get_progress_token().is_some() {
        return true;
    }
    context
        .client_capabilities()
        .and_then(|c| serde_json::to_value(c.extensions).ok())
        .is_some_and(|e| e.get("io.modelcontextprotocol/ui").is_some())
}

/// The tool as `tools/list` shows it: for the view only, so the model
/// never sees or calls it.
pub fn definition() -> Value {
    json!({
        "name": TOOL,
        "title": "Recording Status",
        "description": "The latest readings of the recording running now, for the Live view. \
            Reads them from a file the recording keeps; never opens the microphone.",
        "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
        "annotations": {
            "title": "Recording Status",
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": false,
        },
        "_meta": { "ui": { "resourceUri": crate::views::URI, "visibility": ["app"] } },
    })
}

/// The tool's result: the latest `timegrapher.live/1` document with
/// `recording: true`, `{schema, recording: true}` while the recording is
/// starting, or `{schema, recording: false}` when none is running.
pub fn status() -> Value {
    let path = CURRENT.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let doc = match path {
        None => json!({ "schema": SCHEMA, "recording": false }),
        Some(p) => {
            let mut d = read(&p).unwrap_or_else(|| json!({ "schema": SCHEMA }));
            d["recording"] = json!(true);
            d
        }
    };
    let text = summary(&doc);
    let mut result = json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": doc,
    });
    // The amplitude's ±, in core's words, for the Live view.
    if let Some(pm) = crate::views::pm_texts(&result["structuredContent"]) {
        result["_meta"] = json!({ crate::views::PM_KEY: pm });
    }
    result
}

/// One line for the progress messages and the status text, e.g.
/// "Recording CD: 72 of 120 s · +4.1 s/day · 268° · 0.3 ms".
pub fn summary(doc: &Value) -> String {
    if doc["recording"] == false {
        return "No recording is running.".into();
    }
    let position = doc["position"].as_str().unwrap_or("");
    let (Some(at), Some(total)) = (doc["recorded_s"].as_f64(), doc["seconds"].as_f64()) else {
        return format!("Recording {position}").trim_end().to_string();
    };
    let mut s = if doc["done"] == true {
        format!("Recorded {position}; checking the take")
    } else {
        format!("Recording {position}: {at:.0} of {total:.0} s")
    };
    let r = &doc["reading"];
    if r["beats_used"].as_u64().unwrap_or(0) > 0 {
        if let Some(v) = r["rate_s_per_day"].as_f64() {
            s += &format!(" · {v:+.1} s/day");
        }
        if let Some(v) = r["amplitude_deg"].as_f64() {
            s += &format!(" · {v:.0}°");
        }
        if let Some(v) = timegrapher_core::session::beat_error_size(
            r["beat_error_unlock_ms"].as_f64(),
            r["beat_error_ms"].as_f64(),
        ) {
            s += &format!(" · {v:.1} ms");
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feed_goes_when_the_call_ends() {
        let feed = Feed::start();
        let path = feed.path().to_path_buf();
        assert_eq!(status()["structuredContent"]["recording"], true);
        std::fs::write(
            &path,
            json!({ "schema": SCHEMA, "position": "CD", "seconds": 120.0, "recorded_s": 72.0,
                    "done": false, "reading": { "beats_used": 80, "rate_s_per_day": 4.12,
                    "amplitude_deg": 268.4, "beat_error_unlock_ms": -0.31 } })
            .to_string(),
        )
        .unwrap();
        let s = status();
        assert_eq!(s["structuredContent"]["recorded_s"], 72.0);
        assert_eq!(
            s["content"][0]["text"],
            "Recording CD: 72 of 120 s · +4.1 s/day · 268° · 0.3 ms"
        );
        drop(feed);
        assert!(!path.exists());
        assert_eq!(status()["structuredContent"]["recording"], false);
    }

    /// The view's tool is the view's alone, and only reads.
    #[test]
    fn the_status_tool_is_for_the_view_and_only_reads() {
        let d = definition();
        assert_eq!(d["_meta"]["ui"]["visibility"], json!(["app"]));
        assert_eq!(d["annotations"]["readOnlyHint"], true);
        assert!(!d["description"]
            .as_str()
            .unwrap()
            .contains("uses the microphone"));
        assert!(crate::tools::find(TOOL).is_none(), "it runs no command");
    }
}
