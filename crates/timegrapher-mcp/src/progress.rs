//! Progress notifications while a recording runs, for chat apps that asked
//! for them (a `progressToken` on the call).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rmcp::model::ProgressNotificationParam;
use rmcp::service::RequestContext;
use rmcp::RoleServer;
use serde_json::{json, Map, Value};

/// Seconds between notifications.
const EVERY_S: u64 = 2;

/// Abort even when the request future is dropped during service shutdown.
pub struct Report(tokio::task::JoinHandle<()>);
impl Drop for Report {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Starts reporting the seconds recorded so far, with the latest readings
/// from the live file when there is one, if the chat app asked; abort the
/// handle when the recording ends.
pub fn report(
    context: &RequestContext<RoleServer>,
    args: &Map<String, Value>,
    live: Option<PathBuf>,
) -> Option<Report> {
    let token = context.meta.get_progress_token()?;
    let peer = context.peer.clone();
    let total = args.get("seconds").and_then(Value::as_f64).unwrap_or(120.0);
    let position = args
        .get("position")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Some(Report(tokio::spawn(async move {
        let start = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_secs(EVERY_S)).await;
            let doc = live.as_deref().and_then(crate::live::read);
            let done = doc
                .as_ref()
                .and_then(|d| d["recorded_s"].as_f64())
                .unwrap_or_else(|| start.elapsed().as_secs_f64())
                .min(total);
            let message = match &doc {
                Some(d) => crate::live::summary(d),
                None if done < total => format!("Recording {position}: {done:.0} of {total:.0} s"),
                None => format!("Recorded {position}; checking the take"),
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
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn dropping_report_aborts_its_detached_task() {
        let task = tokio::spawn(std::future::pending::<()>());
        let aborted = task.abort_handle();
        let report = Report(task);
        drop(report);
        tokio::task::yield_now().await;
        assert!(aborted.is_finished());
    }
}
