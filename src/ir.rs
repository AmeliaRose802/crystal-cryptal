// IR: typed intermediate representation of a Cryptol spec.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Proof status for a property, populated from an external proof manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Load proof status from a JSON manifest file.
/// The manifest maps property labels (e.g. "P1") to proof status entries.
pub fn load_proof_manifest(path: &Path) -> Result<HashMap<String, ProofStatus>, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read manifest {}: {e}", path.display()))?;
    let raw: HashMap<String, ManifestEntry> =
        serde_json::from_str(&contents).map_err(|e| format!("failed to parse manifest: {e}"))?;
    Ok(raw.into_iter().map(|(k, v)| (k, v.into())).collect())
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
            "P1":  { "status": "proven", "solver": "z3", "time_secs": 0.42 },
            "P8":  { "status": "assumed" },
            "P22": { "status": "not_attempted" },
            "P99": { "status": "failed", "reason": "counterexample found" }
        }"#;

        let raw: HashMap<String, ManifestEntry> =
            serde_json::from_str(json).expect("parse manifest");
        let map: HashMap<String, ProofStatus> =
            raw.into_iter().map(|(k, v)| (k, v.into())).collect();

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
}
