// Loader for `implementation_inventory.json` — the full list of real
// C++/Rust functions that the codebase contains, regardless of whether
// they were modeled in Cryptol or proven.
//
// Emitted by saw-spec-gen (which already walks the clang AST + mir-json
// when generating specs). Pretty-specs treats it as a passive input — if
// absent, the ⚠️ "implemented, unverified" column is simply empty.
//
// Schema:
//
// ```jsonc
// {
//   "functions": [
//     { "name": "hmac_sha256", "lang": "cpp",
//       "symbol": "?hmac_sha256@sdep@@…",
//       "file": "cpp/src/hmac.cpp",
//       "models": "hmacSha256" },
//     { "name": "canonicalizePayload", "lang": "cpp",
//       "file": "cpp/src/canonical.cpp",
//       "models": "canonicalize_lp",
//       "models_note": "bounded model only" },
//     { "name": "handle_provision", "lang": "cpp",
//       "file": "cpp/src/controller.cpp",
//       "reason_codes": ["R6"],
//       "composes": ["provisionKey"] }
//   ]
// }
// ```

use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct InventoryEntry {
    /// Source-language identifier (unmangled). Used to look up matching
    /// proof manifest entries and to render the matrix row.
    pub name: String,

    /// `"cpp"` / `"rust"`. Free-form so saw-spec-gen can grow new
    /// languages without forcing a pretty-specs schema bump.
    pub lang: String,

    #[serde(default)]
    pub symbol: Option<String>,

    #[serde(default)]
    pub file: Option<String>,

    /// Name of the Cryptol model function this implementation maps to,
    /// when known. Drives the 🧩 ↔ ⚠️ cross-link in the rendered matrix.
    #[serde(default)]
    pub models: Option<String>,

    /// Free-form qualifier on `models` (e.g. "bounded model only").
    #[serde(default)]
    pub models_note: Option<String>,

    /// Other functions this one calls / composes. Rendered as a small list
    /// in the matrix so a reader can see that e.g. `handle_provision`
    /// inherits whatever coverage `provisionKey` carries.
    #[serde(default)]
    pub composes: Vec<String>,

    /// Why this function is currently ⚠️ implemented-but-unverified.
    /// Expected values are R1/R2/R3/R4/R6.
    #[serde(default)]
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ImplementationInventory {
    #[serde(default)]
    pub functions: Vec<InventoryEntry>,
}

/// Load an `implementation_inventory.json` file. Returns an empty inventory
/// if the file is missing — callers should treat that case as "no implementation
/// data available" rather than an error, since the file is optional.
pub fn load_inventory(path: &Path) -> Result<ImplementationInventory, String> {
    if !path.exists() {
        return Ok(ImplementationInventory::default());
    }
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read inventory {}: {e}", path.display()))?;
    let inv: ImplementationInventory = serde_json::from_str(&contents)
        .map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
    Ok(inv)
}
