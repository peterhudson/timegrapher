//! Progress notifications while a recording runs, for chat apps that asked
//! for them (a `progressToken` on the call).

use std::time::{Duration, Instant};

use rmcp::model::ProgressNotificationParam;
use rmcp::service::RequestContext;
use rmcp::RoleServer;
use serde_json::{json, Map, Value};

/// Seconds between notifications.
const EVERY_S: u64 = 2;

/// Starts reporting the seconds recorded so far, if the chat app asked;
/// abort the handle when the recording ends.
pub fn report(
    context: &RequestContext<RoleServer>,
    args: &Map<String, Value>,
) -> Option<tokio::task::JoinHandle<()>> {
    let token = context.meta.get_progress_token()?;
    let peer = context.peer.clone();
    let total = args.get("seconds").and_then(Value::as_f64).unwrap_or(120.0);
    let position = args
        .get("position")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Some(tokio::spawn(async move {
        let start = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_secs(EVERY_S)).await;
            let done = start.elapsed().as_secs_f64().min(total);
            let message = if done < total {
                format!("Recording {position}: {done:.0} of {total:.0} s")
            } else {
                format!("Recorded {position}; checking the take")
            };
            let Ok(param) = serde_json::from_value::<ProgressNotificationParam>(json!({
                "progressToken": token,
                "progress": done,
                "total": total,
                "message": message,
            })) else {
                return;
            };
            if peer.notify_progress(param).await.is_err() {
                return;
            }
        }
    }))
}
