// Proof-manifest loading: parses external JSON manifests that pair Cryptol
// property/function names with their verification verdicts.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use super::ProofStatus;

/// Helper for deserializing proof manifest entries using `#[serde(tag = "status")]`.
#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum ManifestEntry {
    Proven {
        solver: String,
        time_secs: Option<f64>,
        #[serde(default)]
        overrides: Vec<String>,
        #[serde(default)]
        iterations: Option<u64>,
        #[serde(default)]
        verify_command: Option<String>,
        #[serde(default)]
        verify_script: Option<String>,
    },
    Assumed,
    Failed {
        reason: String,
        #[serde(default)]
        counterexample: Option<String>,
        #[serde(default)]
        log_excerpt: Option<String>,
        #[serde(default)]
        verify_command: Option<String>,
        #[serde(default)]
        verify_script: Option<String>,
    },
    NotAttempted,
}

impl From<ManifestEntry> for ProofStatus {
    fn from(entry: ManifestEntry) -> Self {
        match entry {
            ManifestEntry::Proven {
                solver,
                time_secs,
                overrides,
                iterations,
                verify_command,
                verify_script,
            } => ProofStatus::Proven {
                solver,
                time_secs,
                overrides,
                iterations,
                verify_command,
                verify_script,
            },
            ManifestEntry::Assumed => ProofStatus::Assumed,
            ManifestEntry::Failed {
                reason,
                counterexample,
                log_excerpt,
                verify_command,
                verify_script,
            } => ProofStatus::Failed {
                reason,
                counterexample,
                log_excerpt,
                verify_command,
                verify_script,
            },
            ManifestEntry::NotAttempted => ProofStatus::NotAttempted,
        }
    }
}

/// Per-function entry in an extended manifest.
///
/// Supports both the nested format written by `--adapt-saw-results`:
/// ```json
/// { "overall": {"status":"proven","solver":"z3"}, "by_language": {"cpp":{...}} }
/// ```
/// and a flat fallback (no `overall` key) for hand-authored manifests that
/// use the same shape as property entries.
///
/// The optional `by_language` breakdown is accepted by the JSON schema but
/// not surfaced through this loader — serde silently ignores it (we never
/// store it on the resulting [`ProofManifest`]). Add it back to the
/// `Nested` variant if you need to expose the per-language detail later.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum FunctionManifestEntry {
    /// Nested format: `overall` (plus an ignored `by_language`).
    Nested { overall: ManifestEntry },
    /// Flat fallback: the entry is a `ManifestEntry` directly (no nesting).
    Flat(ManifestEntry),
}

impl FunctionManifestEntry {
    pub(super) fn into_overall(self) -> ManifestEntry {
        match self {
            FunctionManifestEntry::Nested { overall } => overall,
            FunctionManifestEntry::Flat(entry) => entry,
        }
    }
}

/// Extended manifest top-level structure (new format).
#[derive(Debug, Deserialize)]
pub(super) struct ExtendedManifest {
    pub(super) properties: HashMap<String, ManifestEntry>,
    pub(super) functions: Option<HashMap<String, FunctionManifestEntry>>,
}

/// Loaded proof manifest containing per-property and per-function statuses.
#[derive(Debug, Default)]
pub struct ProofManifest {
    pub properties: HashMap<String, ProofStatus>,
    pub functions: HashMap<String, ProofStatus>,
}

/// Load proof status from a JSON manifest file.
///
/// Accepts two formats:
/// - Extended: `{"properties": {...}, "functions": {...}}`
/// - Flat (legacy): `{"P1": {...}, "P2": {...}}` — treated as properties only
pub fn load_proof_manifest(path: &Path) -> Result<ProofManifest, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read manifest {}: {e}", path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&contents).map_err(|e| format!("failed to parse manifest: {e}"))?;

    // Extended format: top-level object has a "properties" key.
    if value.get("properties").is_some() {
        let manifest: ExtendedManifest =
            serde_json::from_value(value).map_err(|e| format!("failed to parse manifest: {e}"))?;
        let properties = manifest
            .properties
            .into_iter()
            .map(|(k, v)| (k, ProofStatus::from(v)))
            .collect();
        let functions = manifest
            .functions
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k, ProofStatus::from(v.into_overall())))
            .collect();
        return Ok(ProofManifest {
            properties,
            functions,
        });
    }

    // Flat (legacy) format: treat entire object as properties map.
    let flat: HashMap<String, ManifestEntry> = serde_json::from_value(value)
        .map_err(|e| format!("failed to parse manifest (flat format): {e}"))?;
    let properties = flat
        .into_iter()
        .map(|(k, v)| (k, ProofStatus::from(v)))
        .collect();
    Ok(ProofManifest {
        properties,
        functions: HashMap::new(),
    })
}
