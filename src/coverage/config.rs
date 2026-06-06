// Parser for `coverage.toml` — opt-in/opt-out config for the coverage
// ledger.
//
// Schema:
//
// ```toml
// [exclude]                          # do not flag these as ⚠️ unverified
// functions = ["to_lower", "trim"]
//
// [abstraction]                      # force-classify as 🧩 with this note
// hmacSha256       = "Algebraic placeholder; NOT SHA-256."
// canonLenPrefixed = "Bounded fixed-width encoder."
//
// [spec_only]                        # explicitly 📄 spec-only (no impl)
// functions = ["secureProvisionKey"]
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
    abstraction: HashMap<String, String>,
    spec_only: ListSection,
}

/// Parsed, deduplicated `coverage.toml`.
#[derive(Debug, Default, Clone)]
pub struct CoverageConfig {
    /// Functions to drop from the ⚠️ "unverified" count (helpers / trivia).
    /// Stored as a set for O(1) lookup; the original order is irrelevant.
    pub exclude: Vec<String>,

    /// Names that should always render as 🧩 (model abstraction) with the
    /// associated note shown in the per-page banner.
    pub abstraction: HashMap<String, String>,

    /// Functions that exist only in the model on purpose (📄 spec-only).
    pub spec_only: Vec<String>,
}

impl CoverageConfig {
    pub fn is_excluded(&self, name: &str) -> bool {
        self.exclude.iter().any(|n| n == name)
    }

    pub fn abstraction_note(&self, name: &str) -> Option<&str> {
        self.abstraction.get(name).map(|s| s.as_str())
    }

    pub fn is_spec_only(&self, name: &str) -> bool {
        self.spec_only.iter().any(|n| n == name)
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

    Ok(CoverageConfig {
        exclude,
        abstraction: raw.abstraction,
        spec_only,
    })
}
