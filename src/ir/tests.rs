use std::collections::HashMap;

use super::manifest::{ExtendedManifest, ManifestEntry};
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
    assert!(matches!(map.get("P22").unwrap(), ProofStatus::NotAttempted));
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

    let manifest = load_proof_manifest(std::path::Path::new("/dev/null")).unwrap_or_else(|_| {
        // parse directly since we can't write a temp file in a unit test portably
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        let m: ExtendedManifest = serde_json::from_value(v).unwrap();
        let properties = m
            .properties
            .into_iter()
            .map(|(k, v)| (k, ProofStatus::from(v)))
            .collect();
        let functions = m
            .functions
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k, ProofStatus::from(v.into_overall())))
            .collect();
        ProofManifest {
            properties,
            functions,
        }
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
    let functions: HashMap<String, ProofStatus> = m
        .functions
        .unwrap_or_default()
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

    let pub_item = items
        .iter()
        .find(|i| matches!(i, Item::Function { name, .. } if name == "pub"));
    if let Some(Item::Function { is_private, .. }) = pub_item {
        assert!(!is_private, "pub should not be marked private");
    } else {
        panic!("pub function not found");
    }

    let helper_item = items
        .iter()
        .find(|i| matches!(i, Item::Function { name, .. } if name == "helper"));
    if let Some(Item::Function { is_private, .. }) = helper_item {
        assert!(is_private, "helper should be marked private");
    } else {
        panic!("helper function not found");
    }

    let helperb_item = items
        .iter()
        .find(|i| matches!(i, Item::Function { name, .. } if name == "helperB"));
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
    assert!(
        !json.contains("is_private"),
        "is_private=false should be omitted from JSON"
    );
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
    assert!(
        json.contains("is_private"),
        "is_private=true should be present in JSON"
    );
}
