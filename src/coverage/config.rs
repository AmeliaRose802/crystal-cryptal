// Parser for `coverage.toml` — opt-in/opt-out config for the coverage
// ledger.
//
// Schema:
//
// ```toml
// [exclude]                          # do not flag these as ⚠️ unverified
// functions = ["to_lower", "trim"]
//
// [assumption]                       # force-classify as 🔒 with this note
// hmacSha256       = "Trusted HMAC contract; real SHA-256 not proved here."
//
// [abstraction]                      # force-classify as 🧩 with this note
// hmacSha256       = "Algebraic placeholder; NOT SHA-256."
// canonLenPrefixed = "Bounded fixed-width encoder."
//
// [spec_only]                        # explicitly 📄 spec-only (no impl)
// functions = ["secureProvisionKey"]
//
// [reason_codes]                     # per-function ⚠️ reason codes
// canonicalizePayload = ["R2", "R1"]
// handle_provision   = ["R6"]
// ```
//
// All sections are optional. An absent file is equivalent to an empty
// config — every implementation function defaults to ⚠️ unless excluded,
// every model function falls back to its proof status.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
struct ListSection {
    functions: Vec<String>,
}

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
struct RawCoverageConfig {
    exclude: ListSection,
    assumption: HashMap<String, String>,
    abstraction: HashMap<String, String>,
    spec_only: ListSection,
    reason_codes: HashMap<String, Vec<String>>,
}

/// Parsed, deduplicated `coverage.toml`.
#[derive(Debug, Default, Clone)]
pub struct CoverageConfig {
    /// Functions to drop from the ⚠️ "unverified" count (helpers / trivia).
    /// Stored as a set for O(1) lookup; the original order is irrelevant.
    pub exclude: Vec<String>,

    /// Names that should always render as 🔒 (trusted assumption) with the
    /// associated note shown in the per-page banner.
    pub assumption: HashMap<String, String>,

    /// Names that should always render as 🧩 (ABI adapter / stand-in) with
    /// the associated note shown in the per-page banner.
    pub abstraction: HashMap<String, String>,

    /// Functions that exist only in the model on purpose (📄 spec-only).
    pub spec_only: Vec<String>,

    /// Per-function reason codes for ⚠️ rows/pages, normalized to uppercase
    /// and deduplicated while preserving order.
    pub reason_codes: HashMap<String, Vec<String>>,
}

impl CoverageConfig {
    pub fn is_excluded(&self, name: &str) -> bool {
        self.exclude.iter().any(|n| n == name)
    }

    pub fn assumption_note(&self, name: &str) -> Option<&str> {
        self.assumption.get(name).map(|s| s.as_str())
    }

    pub fn abstraction_note(&self, name: &str) -> Option<&str> {
        self.abstraction.get(name).map(|s| s.as_str())
    }

    pub fn is_spec_only(&self, name: &str) -> bool {
        self.spec_only.iter().any(|n| n == name)
    }

    pub fn reason_codes(&self, name: &str) -> &[String] {
        self.reason_codes
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

/// Load `coverage.toml`. Returns a default (empty) config if the file does
/// not exist; only surface an error for malformed content.
pub fn load_coverage_config(path: &Path) -> Result<CoverageConfig, String> {
    if !path.exists() {
        return Ok(CoverageConfig::default());
    }
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read coverage config {}: {e}", path.display()))?;
    let raw: RawCoverageConfig = toml::from_str(&contents)
        .map_err(|e| format!("failed to parse {}: {e}", path.display()))?;

    let mut exclude = raw.exclude.functions;
    exclude.sort();
    exclude.dedup();
    let mut spec_only = raw.spec_only.functions;
    spec_only.sort();
    spec_only.dedup();

    let reason_codes = raw
        .reason_codes
        .into_iter()
        .map(|(name, codes)| (name, normalize_codes(codes)))
        .collect();

    Ok(CoverageConfig {
        exclude,
        assumption: raw.assumption,
        abstraction: raw.abstraction,
        spec_only,
        reason_codes,
    })
}

fn normalize_codes(codes: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for code in codes {
        let norm = code.trim().to_ascii_uppercase();
        if norm.is_empty() || out.iter().any(|c| c == &norm) {
            continue;
        }
        out.push(norm);
    }
    out
}
