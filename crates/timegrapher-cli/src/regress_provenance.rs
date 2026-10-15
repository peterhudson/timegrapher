//! Additive evidence for newly written regression entries; never acceptance rules.

use crate::corpus::{Corpus, VerifiedFile};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use timegrapher_core::session::RecordingEntry;
use timegrapher_core::stream::StreamConfig;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntryProvenance {
    pub schema: String,
    /// Reported build metadata, not an executable digest or freshness guarantee.
    pub software: Value,
    pub manifest: Value,
    pub inputs: Vec<Value>,
    pub settings: Value,
    pub measurement_context: Value,
    pub comparator: Value,
    /// Retain unrecognized additive metadata when only other takes are rewritten.
    #[serde(flatten)]
    pub additional_fields: std::collections::BTreeMap<String, Value>,
}

/// Captured from the actual read_one resolution, in decoder input order.
#[derive(Debug)]
pub struct ReadContext {
    inputs: Vec<(PathBuf, String, u64)>,
    settings: Value,
    measurement: Value,
}

impl ReadContext {
    pub fn capture(
        files: &[PathBuf],
        base: &Path,
        cfg: &StreamConfig,
        measurement: Value,
    ) -> Result<Self, String> {
        let inputs = files
            .iter()
            .map(|path| {
                let canonical = path
                    .canonicalize()
                    .map_err(|e| format!("provenance {}: {e}", path.display()))?;
                let bytes = path
                    .metadata()
                    .map_err(|e| format!("provenance {}: {e}", path.display()))?
                    .len();
                let name = path
                    .strip_prefix(base)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/");
                Ok((canonical, name, bytes))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let a = &cfg.analysis;
        Ok(Self {
            inputs,
            settings: json!({
                "requested_bph": a.bph, "lift_deg": a.amplitude.lift_deg,
                "onset_fraction": a.amplitude.onset_fraction, "escape_teeth": a.escape_teeth,
                "configured_lift_known": a.lift_known,
                "rate_window_s": a.rate_window_s, "amplitude_window_s": a.amplitude_window_s,
                "chunk_s": cfg.chunk_s, "margin_s": cfg.margin_s,
                "envelope": { "highpass_hz": a.envelope.highpass_hz,
                    "notch_hz": a.envelope.notch_hz, "smooth_s": a.envelope.smooth_s,
                    "burst_gate": a.envelope.burst_gate },
            }),
            measurement,
        })
    }

    pub fn finish(
        self,
        session: &str,
        digest: &str,
        index: usize,
        entry: &RecordingEntry,
        corpus: Option<(&Path, &Corpus)>,
        verified: &[VerifiedFile],
    ) -> EntryProvenance {
        let inputs = self
            .inputs
            .into_iter()
            .map(|(path, name, bytes)| {
                let verification = verified
                    .iter()
                    .find(|v| v.canonical_path.as_ref() == Some(&path));
                json!({
                    "path_relative_to_session": name,
                    "bytes_observed_after_analysis": bytes,
                    "declared_corpus_identity": declared_identity(&path, corpus),
                    "verified_identity": verification.map(|v| json!({
                        "sha256": v.sha256, "bytes": v.bytes,
                        "timing": "verified_at_fetch_this_run",
                    })),
                })
            })
            .collect();
        EntryProvenance {
            schema: "timegrapher.regress-entry-provenance/1".into(),
            software: crate::output::software(),
            manifest: json!({ "path_relative_to_regression_root": session,
                "sha256": digest, "recording_index_1based": index, "recording": entry }),
            inputs,
            settings: self.settings,
            measurement_context: self.measurement,
            comparator: json!({ "reference_as_declared": entry.reference,
                "executable": null, "version": null, "sha256": null }),
            additional_fields: Default::default(),
        }
    }
}

fn declared_identity(path: &Path, corpus: Option<(&Path, &Corpus)>) -> Option<Value> {
    let (root, corpus) = corpus?;
    for take in &corpus.takes {
        for file in &take.files {
            let candidate = crate::corpus::takes_dir(root)
                .join(&take.id)
                .join(&file.name);
            if candidate.canonicalize().ok().as_deref() == Some(path) {
                return Some(json!({
                    "take": take.id, "release": take.release, "name": file.name,
                    "bytes": file.bytes, "sha256": file.sha256,
                }));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "tg-provenance-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn context_preserves_order_duplicates_exact_bounds_and_resolved_settings() {
        let f = Fixture::new();
        std::fs::write(f.0.join("a.wav"), b"a").unwrap();
        std::fs::write(f.0.join("b.wav"), b"bbb").unwrap();
        let files = [f.0.join("b.wav"), f.0.join("a.wav"), f.0.join("b.wav")];
        let mut cfg = StreamConfig::default();
        cfg.analysis.bph = Some(28800);
        cfg.analysis.amplitude.lift_deg = 49.5;
        cfg.analysis.escape_teeth = 21;
        let context = ReadContext::capture(
            &files,
            &f.0,
            &cfg,
            json!({"measured_start_s": 0.125, "measured_end_s": 9.875}),
        )
        .unwrap();
        let entry = RecordingEntry {
            file: "take".into(),
            start_s: Some(0.125),
            end_s: Some(9.875),
            ..Default::default()
        };
        let p = context.finish("take/session.toml", "manifest-digest", 2, &entry, None, &[]);
        assert_eq!(
            p.inputs
                .iter()
                .map(|v| v["path_relative_to_session"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["b.wav", "a.wav", "b.wav"]
        );
        assert_eq!(p.inputs[0]["bytes_observed_after_analysis"], 3);
        assert!(p.inputs[0]["verified_identity"].is_null());
        assert!(p.inputs[0]["declared_corpus_identity"].is_null());
        assert_eq!(p.manifest["recording"]["start_s"], 0.125);
        assert_eq!(p.manifest["recording_index_1based"], 2);
        assert_eq!(p.settings["requested_bph"], 28800);
        assert_eq!(p.settings["lift_deg"], 49.5);
        assert_eq!(p.settings["escape_teeth"], 21);
        assert_eq!(p.measurement_context["measured_end_s"], 9.875);
        assert!(p.comparator["sha256"].is_null());
    }

    #[test]
    fn declared_identity_is_not_current_fetch_verification() {
        let f = Fixture::new();
        let dir = f.0.join("takes/t");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.wav");
        std::fs::write(&path, b"audio").unwrap();
        let corpus = Corpus {
            takes: vec![crate::corpus::Take {
                id: "t".into(),
                tier: "referenced".into(),
                release: "t".into(),
                files: vec![crate::corpus::File {
                    name: "a.wav".into(),
                    bytes: 5,
                    sha256: "declared".into(),
                }],
            }],
        };
        let entry = RecordingEntry::default();
        let capture = || {
            ReadContext::capture(
                std::slice::from_ref(&path),
                &dir,
                &StreamConfig::default(),
                json!({}),
            )
            .unwrap()
        };
        let declared =
            capture().finish("t/session.toml", "d", 1, &entry, Some((&f.0, &corpus)), &[]);
        assert_eq!(
            declared.inputs[0]["declared_corpus_identity"]["sha256"],
            "declared"
        );
        assert!(declared.inputs[0]["verified_identity"].is_null());
        let verified = [VerifiedFile {
            canonical_path: path.canonicalize().ok(),
            bytes: 5,
            sha256: "actual-verified".into(),
        }];
        let actual = capture().finish(
            "t/session.toml",
            "d",
            1,
            &entry,
            Some((&f.0, &corpus)),
            &verified,
        );
        assert_eq!(
            actual.inputs[0]["verified_identity"]["sha256"],
            "actual-verified"
        );
        assert_eq!(
            actual.inputs[0]["verified_identity"]["timing"],
            "verified_at_fetch_this_run"
        );
        assert_eq!(
            actual.inputs[0]["declared_corpus_identity"],
            declared.inputs[0]["declared_corpus_identity"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_changed_symlink_does_not_reassign_fetch_verification() {
        let f = Fixture::new();
        std::fs::write(f.0.join("old.wav"), b"old").unwrap();
        std::fs::write(f.0.join("new.wav"), b"new").unwrap();
        let path = f.0.join("input.wav");
        std::os::unix::fs::symlink(f.0.join("old.wav"), &path).unwrap();
        let verified = [VerifiedFile {
            canonical_path: path.canonicalize().ok(),
            bytes: 3,
            sha256: "old-verified".into(),
        }];
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(f.0.join("new.wav"), &path).unwrap();
        let context =
            ReadContext::capture(&[path], &f.0, &StreamConfig::default(), json!({})).unwrap();
        let p = context.finish(
            "session.toml",
            "d",
            1,
            &RecordingEntry::default(),
            None,
            &verified,
        );
        assert!(p.inputs[0]["verified_identity"].is_null());
    }

    #[test]
    fn provenance_metadata_failure_is_an_error() {
        let f = Fixture::new();
        assert!(ReadContext::capture(
            &[f.0.join("missing.wav")],
            &f.0,
            &StreamConfig::default(),
            json!({})
        )
        .unwrap_err()
        .contains("provenance"));
    }

    #[test]
    fn manifest_digest_is_of_the_bytes_that_were_parsed() {
        let f = Fixture::new();
        let path = f.0.join("session.toml");
        let original = "# original\n[[recording]]\nfile = 'a.wav'\nstart_s = 0.125\n";
        std::fs::write(&path, original).unwrap();
        let (m, _, digest) = crate::session_cmd::load_manifest_file(&path).unwrap();
        std::fs::write(&path, "different subsequent bytes").unwrap();
        assert_eq!(m.recordings[0].start_s, Some(0.125));
        assert_eq!(digest, crate::archive::sha256_hex(original.as_bytes()));
        assert_ne!(
            digest,
            crate::archive::sha256_hex(&std::fs::read(&path).unwrap())
        );
    }
}
