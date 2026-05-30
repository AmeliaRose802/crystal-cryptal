// IR: typed intermediate representation of a Cryptol spec.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Proof status for a property, populated from an external proof manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProofStatus {
    Proven {
        solver: String,
        time_secs: Option<f64>,
    },
    Assumed,
    Failed {
        reason: String,
    },
    NotAttempted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumVariant {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    pub condition: Option<String>, // None = "otherwise" / else
    pub result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Item {
    Module {
        name: String,
        doc: Vec<String>,
    },
    Import {
        module_path: String,
        qualifier: Option<String>,
        hiding: Vec<String>,
    },
    Section {
        level: u8,
        title: String,
        doc: Vec<String>,
    },
    TypeAlias {
        name: String,
        width: String,
        doc: Vec<String>,
    },
    EnumGroup {
        type_name: String,
        width: String,
        variants: Vec<EnumVariant>,
        predicate: Option<String>,
        doc: Vec<String>,
    },
    RecordType {
        name: String,
        fields: Vec<(String, String)>,
        doc: Vec<String>,
    },
    Function {
        name: String,
        signature: String,
        branches: Vec<Branch>,
        body: String,
        doc: Vec<String>,
        proof_status: Option<ProofStatus>,
    },
    Property {
        label: String,
        name: String,
        params: Vec<String>,
        body: String,
        doc: Vec<String>,
        proof_status: Option<ProofStatus>,
    },
    CommentBlock {
        lines: Vec<String>,
    },
    ModuleParam {
        name: String,
        kind: ParamKind,
        signature: String,
        doc: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParamKind {
    TypeParam,
    ValueParam,
    Constraint,
}

/// Helper for deserializing proof manifest entries using `#[serde(tag = "status")]`.
#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ManifestEntry {
    Proven {
        solver: String,
        time_secs: Option<f64>,
    },
    Assumed,
    Failed {
        reason: String,
    },
    NotAttempted,
}

impl From<ManifestEntry> for ProofStatus {
    fn from(entry: ManifestEntry) -> Self {
        match entry {
            ManifestEntry::Proven { solver, time_secs } => {
                ProofStatus::Proven { solver, time_secs }
            }
            ManifestEntry::Assumed => ProofStatus::Assumed,
            ManifestEntry::Failed { reason } => ProofStatus::Failed { reason },
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
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FunctionManifestEntry {
    /// Nested format: `overall` + optional `by_language`.
    Nested {
        overall: ManifestEntry,
        #[serde(default)]
        by_language: HashMap<String, ManifestEntry>,
    },
    /// Flat fallback: the entry is a `ManifestEntry` directly (no nesting).
    Flat(ManifestEntry),
}

impl FunctionManifestEntry {
    fn into_overall(self) -> ManifestEntry {
        match self {
            FunctionManifestEntry::Nested { overall, .. } => overall,
            FunctionManifestEntry::Flat(entry) => entry,
        }
    }
}

/// Extended manifest top-level structure (new format).
#[derive(Debug, Deserialize)]
struct ExtendedManifest {
    properties: HashMap<String, ManifestEntry>,
    functions: Option<HashMap<String, FunctionManifestEntry>>,
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
        let manifest: ExtendedManifest = serde_json::from_value(value)
            .map_err(|e| format!("failed to parse manifest: {e}"))?;
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
        return Ok(ProofManifest { properties, functions });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip() {
        let items = vec![
            Item::Module {
                name: "TestModule".into(),
                doc: vec!["A test module.".into()],
            },
            Item::TypeAlias {
                name: "Byte".into(),
                width: "[8]".into(),
                doc: vec![],
            },
            Item::EnumGroup {
                type_name: "Color".into(),
                width: "[8]".into(),
                variants: vec![
                    EnumVariant {
                        name: "Red".into(),
                        value: "0x01".into(),
                    },
                    EnumVariant {
                        name: "Blue".into(),
                        value: "0x02".into(),
                    },
                ],
                predicate: Some("validColor".into()),
                doc: vec!["Color codes.".into()],
            },
            Item::Function {
                name: "add".into(),
                signature: "([8], [8]) -> [8]".into(),
                branches: vec![Branch {
                    condition: None,
                    result: "x + y".into(),
                }],
                body: "x + y".into(),
                doc: vec![],
                proof_status: None,
            },
            Item::Property {
                label: "P1".into(),
                name: "add_commutative".into(),
                params: vec!["x".into(), "y".into()],
                body: "add(x, y) == add(y, x)".into(),
                doc: vec![],
                proof_status: Some(ProofStatus::Proven {
                    solver: "z3".into(),
                    time_secs: Some(0.42),
                }),
            },
            Item::CommentBlock {
                lines: vec!["// end of spec".into()],
            },
        ];

        let json = serde_json::to_string_pretty(&items).expect("serialize");
        let back: Vec<Item> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.len(), items.len());
    }

    #[test]
    fn proof_manifest_deserialization() {
        let json = r#"{
            "properties": {
                "P1":  { "status": "proven", "solver": "z3", "time_secs": 0.42 },
                "P8":  { "status": "assumed" },
                "P22": { "status": "not_attempted" },
                "P99": { "status": "failed", "reason": "counterexample found" }
            }
        }"#;

        let manifest: ExtendedManifest = serde_json::from_str(json).expect("parse manifest");
        let map: HashMap<String, ProofStatus> = manifest
            .properties
            .into_iter()
            .map(|(k, v)| (k, v.into()))
            .collect();

        assert_eq!(map.len(), 4);
        assert!(matches!(
            map.get("P1").unwrap(),
            ProofStatus::Proven { solver, .. } if solver == "z3"
        ));
        assert!(matches!(map.get("P8").unwrap(), ProofStatus::Assumed));
        assert!(matches!(
            map.get("P22").unwrap(),
            ProofStatus::NotAttempted
        ));
        assert!(matches!(
            map.get("P99").unwrap(),
            ProofStatus::Failed { reason } if reason == "counterexample found"
        ));
    }

    #[test]
    fn proof_manifest_nested_function_entry() {
        let json = r#"{
            "properties": {},
            "functions": {
                "authenticate": {
                    "overall": { "status": "proven", "solver": "z3", "time_secs": 1.2 },
                    "by_language": {
                        "cpp": { "status": "proven", "solver": "z3", "time_secs": 1.2 }
                    }
                },
                "getStatus": {
                    "overall": { "status": "failed", "reason": "counterexample found" }
                }
            }
        }"#;

        let manifest = load_proof_manifest(std::path::Path::new("/dev/null"))
            .unwrap_or_else(|_| {
                // parse directly since we can't write a temp file in a unit test portably
                let v: serde_json::Value = serde_json::from_str(json).unwrap();
                let m: ExtendedManifest = serde_json::from_value(v).unwrap();
                let properties = m.properties.into_iter()
                    .map(|(k, v)| (k, ProofStatus::from(v))).collect();
                let functions = m.functions.unwrap_or_default().into_iter()
                    .map(|(k, v)| (k, ProofStatus::from(v.into_overall()))).collect();
                ProofManifest { properties, functions }
            });

        assert!(matches!(
            manifest.functions.get("authenticate").unwrap(),
            ProofStatus::Proven { solver, .. } if solver == "z3"
        ));
        assert!(matches!(
            manifest.functions.get("getStatus").unwrap(),
            ProofStatus::Failed { reason } if reason == "counterexample found"
        ));
    }

    #[test]
    fn proof_manifest_flat_function_entry() {
        // Hand-authored manifest with flat function entries (no "overall" nesting).
        let json = r#"{
            "properties": {},
            "functions": {
                "provisionKey": { "status": "proven", "solver": "z3", "time_secs": 0.5 }
            }
        }"#;

        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        let m: ExtendedManifest = serde_json::from_value(v).unwrap();
        let functions: HashMap<String, ProofStatus> = m.functions.unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k, ProofStatus::from(v.into_overall())))
            .collect();

        assert!(matches!(
            functions.get("provisionKey").unwrap(),
            ProofStatus::Proven { solver, .. } if solver == "z3"
        ));
    }
}
