//! Recording safely when a chat app retries or overlaps calls: one tool on
//! the microphone at a time, and a recording retried with the same
//! `take_id` answered from the take already made rather than recorded again.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde_json::{Map, Value};

static MIC_BUSY: AtomicBool = AtomicBool::new(false);

/// Said when a second call wants the microphone.
pub const BUSY: &str = "The microphone is in use by another recording or microphone check. \
    Wait for that call to finish, then try again. Nothing was recorded by this call.";

/// Holds the microphone for one call; dropping it frees it, however the
/// call ends.
pub struct Mic(());

impl Drop for Mic {
    fn drop(&mut self) {
        MIC_BUSY.store(false, Ordering::Release);
    }
}

/// The microphone, unless another call has it.
pub fn claim_mic() -> Option<Mic> {
    MIC_BUSY
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
        .then_some(Mic(()))
}

/// A take as the agent named it: its `take_id`, folder and position.
pub type Key = (String, String, String);

static TAKES: Mutex<Option<HashMap<Key, Value>>> = Mutex::new(None);

/// The call's take, if it named one.
pub fn key(args: &Map<String, Value>) -> Option<Key> {
    let text = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_string);
    Some((
        text(crate::tools::TAKE_ID)?,
        text("folder")?,
        text("position")?,
    ))
}

/// The result of the take, if an earlier call recorded it, marked as such.
pub fn recorded(key: &Key) -> Option<Value> {
    let takes = TAKES.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = takes.as_ref()?.get(key)?.clone();
    if let Some(text) = r["content"][0]["text"].as_str() {
        r["content"][0]["text"] = Value::String(format!(
            "{text}\n\nThis take was recorded by an earlier call with take_id {:?}; \
             nothing was recorded again.",
            key.0
        ));
    }
    Some(r)
}

/// Keep a take's result for a retry.
pub fn remember(key: Key, result: &Value) {
    let mut takes = TAKES.lock().unwrap_or_else(|e| e.into_inner());
    takes
        .get_or_insert_with(HashMap::new)
        .insert(key, result.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn one_call_at_a_time_has_the_mic() {
        let first = claim_mic().expect("free");
        assert!(claim_mic().is_none());
        drop(first);
        assert!(claim_mic().is_some());
    }

    #[test]
    fn a_retried_take_comes_back_marked() {
        let args = |id: &str, pos: &str| {
            json!({"folder": "/w", "position": pos, "take_id": id})
                .as_object()
                .unwrap()
                .clone()
        };
        let k = key(&args("t-1", "DU")).unwrap();
        assert!(recorded(&k).is_none());
        remember(
            k.clone(),
            &json!({"content": [{"type": "text", "text": "{}"}]}),
        );
        let again = recorded(&k).unwrap();
        assert!(again["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("nothing was recorded again"));
        // The same name in another position is another take.
        assert!(recorded(&key(&args("t-1", "DD")).unwrap()).is_none());
        let no_id = json!({"folder": "/w", "position": "DU"});
        assert!(key(no_id.as_object().unwrap()).is_none());
    }
}
