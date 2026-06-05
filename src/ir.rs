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
        /// Functions whose specs were used as `*_unsafe_assume_spec` /
        /// `llvm_verify` overrides while discharging this proof. Each entry
        /// records a dependency of the verdict — the property/function is
        /// only as trustworthy as those overrides.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        overrides: Vec<String>,
        /// For bounded-loop proofs: the loop-unroll bound (or `MAX_LEN`) that
        /// the proof was discharged at. `None` for proofs that don't involve
        /// a bounded loop.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        iterations: Option<u64>,
        /// Copy-pasteable shell command that reproduces this proof from a
        /// clean checkout. Surfaced on the rendered page so readers can
        /// re-run the verification locally without grepping the pipeline.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        verify_command: Option<String>,
        /// Path (relative to the manifest) of the generated SAW script that
        /// drives this proof. Used as a fallback when `verify_command` is
        /// absent — the page can synthesise `saw <path>`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        verify_script: Option<String>,
    },
    Assumed,
    Failed {
        reason: String,
        /// Concrete counterexample emitted by the solver, when available
        /// (e.g. "x = 0, y = 1"). Rendered as a code block on the per-item
        /// page so readers can see the exact witness that broke the claim.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        counterexample: Option<String>,
        /// Excerpt of the verifier log / stderr surrounding the failure —
        /// useful when SAW errors out with a stack trace rather than a clean
        /// counterexample (e.g. memory-model failures, type mismatches).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        log_excerpt: Option<String>,
        /// Same as `ProofStatus::Proven::verify_command` — lets readers
        /// re-run a failing proof locally to inspect it interactively.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        verify_command: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        verify_script: Option<String>,
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
        /// True when the declaration appeared inside a Cryptol `private` block.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        is_private: bool,
    },
    Property {
        label: String,
        name: String,
        params: Vec<String>,
        body: String,
        doc: Vec<String>,
        proof_status: Option<ProofStatus>,
        /// True when the declaration appeared inside a Cryptol `private` block.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        is_private: bool,
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
                is_private: false,
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
                    overrides: vec![],
                    iterations: None,
                    verify_command: None,
                    verify_script: None,
                }),
                is_private: false,
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
            ProofStatus::Failed { reason, .. } if reason == "counterexample found"
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
            ProofStatus::Failed { reason, .. } if reason == "counterexample found"
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

    #[test]
    fn private_block_items_marked() {
        use crate::parser::parse;

        let src = r#"
module Test where

pub : [8] -> [8]
pub x = x

private

  helper : [8] -> [8]
  helper x = x + 1

  helperB : [8] -> [8]
  helperB x = x + 2
"#;
        let items = parse(src);

        let pub_item = items.iter().find(|i| matches!(i, Item::Function { name, .. } if name == "pub"));
        if let Some(Item::Function { is_private, .. }) = pub_item {
            assert!(!is_private, "pub should not be marked private");
        } else {
            panic!("pub function not found");
        }

        let helper_item = items.iter().find(|i| matches!(i, Item::Function { name, .. } if name == "helper"));
        if let Some(Item::Function { is_private, .. }) = helper_item {
            assert!(is_private, "helper should be marked private");
        } else {
            panic!("helper function not found");
        }

        let helperb_item = items.iter().find(|i| matches!(i, Item::Function { name, .. } if name == "helperB"));
        if let Some(Item::Function { is_private, .. }) = helperb_item {
            assert!(is_private, "helperB should be marked private");
        } else {
            panic!("helperB function not found");
        }
    }

    #[test]
    fn private_not_serialized_when_false() {
        // is_private: false should not appear in the JSON output.
        let item = Item::Function {
            name: "foo".into(),
            signature: "() -> ()".into(),
            branches: vec![],
            body: String::new(),
            doc: vec![],
            proof_status: None,
            is_private: false,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(!json.contains("is_private"), "is_private=false should be omitted from JSON");
    }

    #[test]
    fn private_serialized_when_true() {
        // is_private: true SHOULD appear in the JSON output.
        let item = Item::Function {
            name: "helper".into(),
            signature: "() -> ()".into(),
            branches: vec![],
            body: String::new(),
            doc: vec![],
            proof_status: None,
            is_private: true,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("is_private"), "is_private=true should be present in JSON");
    }
}
