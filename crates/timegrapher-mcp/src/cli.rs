//! Running the `timegrapher` command-line program, which does the work.

use std::path::PathBuf;
use std::process::Stdio;

use serde_json::Value;

/// Where the `timegrapher` program is: `TIMEGRAPHER_CLI` if set, else the
/// one installed beside this server (the install bundle ships both), else
/// whatever `timegrapher` is on PATH.
pub fn program() -> PathBuf {
    if let Some(p) = std::env::var_os("TIMEGRAPHER_CLI") {
        return PathBuf::from(p);
    }
    let name = format!("timegrapher{}", std::env::consts::EXE_SUFFIX);
    if let Some(beside) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(&name)))
        .filter(|p| p.is_file())
    {
        return beside;
    }
    PathBuf::from(name)
}

pub struct Outcome {
    /// The command's JSON document, when it printed one.
    pub json: Option<Value>,
    /// What it said on stderr (progress, warnings, the error).
    pub stderr: String,
    pub exit_code: Option<i32>,
}

/// Wall-clock execution limits, distinct from the length of recorded audio.
#[derive(Clone, Copy)]
pub struct Policy {
    processing: std::time::Duration,
    grace: std::time::Duration,
    pub profile: std::time::Duration,
}

const PROCESSING: &str = "TIMEGRAPHER_MCP_PROCESSING_TIMEOUT_SECONDS";
const GRACE: &str = "TIMEGRAPHER_MCP_ACQUISITION_GRACE_SECONDS";
const PROFILE: &str = "TIMEGRAPHER_MCP_PROFILE_TIMEOUT_SECONDS";

impl Policy {
    pub fn load() -> Result<Self, String> {
        Self::parse(|name| match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(e) => Err(format!("{name}: {e}")),
        })
    }

    fn parse(mut get: impl FnMut(&str) -> Result<Option<String>, String>) -> Result<Self, String> {
        let mut duration = |name, default| -> Result<std::time::Duration, String> {
            let seconds = get(name)?
                .map(|v| {
                    v.parse::<f64>()
                        .map_err(|_| format!("{name} must be a finite positive number of seconds"))
                })
                .transpose()?
                .unwrap_or(default);
            let duration = timegrapher_core::capture::checked_duration(seconds)
                .map_err(|e| format!("{name} {e}"))?;
            deadline(duration).map_err(|e| format!("{name}: {e}"))?;
            Ok(duration)
        };
        Ok(Self {
            processing: duration(PROCESSING, 21600.0)?,
            grace: duration(GRACE, 120.0)?,
            profile: duration(PROFILE, 60.0)?,
        })
    }

    pub fn budget(
        &self,
        tool: &str,
        args: &serde_json::Map<String, Value>,
    ) -> Result<std::time::Duration, String> {
        let default = match tool {
            "record_position" => 120.0,
            "check_microphone" => 5.0,
            _ => return Ok(self.processing),
        };
        let seconds = args
            .get("seconds")
            .and_then(Value::as_f64)
            .unwrap_or(default);
        timegrapher_core::capture::checked_duration(seconds)?
            .checked_add(self.grace)
            .ok_or_else(|| "acquisition plus processing grace exceeds the duration range".into())
    }
}

pub fn deadline(budget: std::time::Duration) -> Result<tokio::time::Instant, String> {
    tokio::time::Instant::now()
        .checked_add(budget)
        .ok_or_else(|| "execution budget exceeds the platform deadline range".into())
}

#[derive(Debug)]
pub enum RunError {
    Cancelled,
    Disconnected,
    Deadline,
    Failed(String),
    Cleanup {
        cause: Box<RunError>,
        detail: String,
    },
}
impl RunError {
    pub fn stopped(&self) -> bool {
        matches!(
            self,
            Self::Cancelled | Self::Disconnected | Self::Cleanup { .. }
        )
    }
}
impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "cancelled; CLI child stopped"),
            Self::Disconnected => write!(f, "client disconnected; CLI child stopped"),
            Self::Deadline => write!(f, "execution deadline exceeded; CLI child stopped"),
            Self::Failed(error) => f.write_str(error),
            Self::Cleanup { cause, detail } => write!(f, "{cause}; child cleanup failed: {detail}"),
        }
    }
}

pub struct Scope<'a> {
    pub context: &'a rmcp::service::RequestContext<rmcp::RoleServer>,
    pub closed: tokio::sync::watch::Receiver<bool>,
    pub deadline: tokio::time::Instant,
}

fn stop_reason(cancelled: bool, closed: bool, end: tokio::time::Instant) -> Option<RunError> {
    if cancelled {
        Some(RunError::Cancelled)
    } else if closed {
        Some(RunError::Disconnected)
    } else if tokio::time::Instant::now() >= end {
        Some(RunError::Deadline)
    } else {
        None
    }
}

impl Scope<'_> {
    pub async fn run(
        &self,
        args: &[String],
        limit: Option<std::time::Duration>,
    ) -> Result<Outcome, RunError> {
        use tokio::io::AsyncReadExt;
        if let Some(error) = stop_reason(
            self.context.ct.is_cancelled(),
            *self.closed.borrow(),
            self.deadline,
        ) {
            return Err(error);
        }
        let end = match limit {
            Some(limit) => self
                .deadline
                .min(deadline(limit).map_err(RunError::Failed)?),
            None => self.deadline,
        };
        let program = program();
        let mut child = tokio::process::Command::new(&program)
            .args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .kill_on_drop(true).spawn().map_err(|e| RunError::Failed(format!(
                "could not run {}: {e}. The timegrapher program must be installed beside timegrapher-mcp or on PATH (or named by TIMEGRAPHER_CLI).", program.display())))?;
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut closed = self.closed.clone();
        let result = tokio::select! {
            biased;
            () = self.context.ct.cancelled() => Err(RunError::Cancelled),
            _ = closed.wait_for(|closed| *closed) => Err(RunError::Disconnected),
            () = tokio::time::sleep_until(end) => Err(RunError::Deadline),
            result = async {
                let (status, output, errors) = tokio::join!(child.wait(), stdout.read_to_end(&mut out), stderr.read_to_end(&mut err));
                output?; errors?; status
            } => result.map_err(|e| RunError::Failed(format!("could not collect CLI output: {e}"))),
        };
        let status = match result {
            Ok(status) => status,
            Err(error) => {
                // Explicitly reap before releasing microphone/temp-file guards.
                // kill_on_drop remains the fallback if this future is dropped.
                let _ = child.start_kill();
                match tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await {
                    Ok(Ok(_)) => return Err(error),
                    cleanup => {
                        return Err(RunError::Cleanup {
                            cause: Box::new(error),
                            detail: format!("{cleanup:?}"),
                        })
                    }
                }
            }
        };
        Ok(Outcome {
            json: serde_json::from_slice(&out).ok(),
            stderr: String::from_utf8_lossy(&err).trim().to_string(),
            exit_code: status.code(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn defaults() -> Policy {
        Policy::parse(|_| Ok(None)).unwrap()
    }
    #[test]
    fn cancellation_disconnect_and_expiry_preflight_precede_spawn() {
        let future = deadline(std::time::Duration::from_secs(1)).unwrap();
        assert!(matches!(
            stop_reason(true, false, future),
            Some(RunError::Cancelled)
        ));
        assert!(matches!(
            stop_reason(false, true, future),
            Some(RunError::Disconnected)
        ));
        assert!(matches!(
            stop_reason(false, false, tokio::time::Instant::now()),
            Some(RunError::Deadline)
        ));
        assert!(stop_reason(false, false, future).is_none());
    }
    #[test]
    fn cleanup_failure_is_fatal_even_when_a_profile_is_optional() {
        for cause in [
            RunError::Cancelled,
            RunError::Disconnected,
            RunError::Deadline,
        ] {
            let error = RunError::Cleanup {
                cause: Box::new(cause),
                detail: "simulated wait failure".into(),
            };
            assert!(
                error.stopped(),
                "cleanup failure must never become optional profile success"
            );
            assert!(error.to_string().contains("simulated wait failure"));
        }
    }
    #[test]
    fn processing_and_acquisition_budgets_preserve_audio_defaults() {
        let p = defaults();
        let args = serde_json::Map::new();
        assert_eq!(
            p.budget("analyze_recording", &args).unwrap().as_secs(),
            21600
        );
        assert_eq!(p.budget("record_position", &args).unwrap().as_secs(), 240);
        assert_eq!(p.budget("check_microphone", &args).unwrap().as_secs(), 125);
        assert_eq!(p.profile.as_secs(), 60);
        let args = serde_json::json!({"seconds": 3600})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(p.budget("record_position", &args).unwrap().as_secs(), 3720);
    }
    #[test]
    fn configuration_rejects_invalid_values_and_deadline_overflow() {
        for name in [PROCESSING, GRACE, PROFILE] {
            for value in ["bad", "NaN", "inf", "0", "-1", "1e300", "1e-300"] {
                let error = Policy::parse(|key| Ok((key == name).then(|| value.into())))
                    .err()
                    .unwrap();
                assert!(error.contains(name), "{error}");
            }
        }
        assert!(deadline(std::time::Duration::MAX).is_err());
        let p = Policy::parse(|_| Ok(Some("0.01".into()))).unwrap();
        assert_eq!(p.processing, std::time::Duration::from_millis(10));
    }
}
