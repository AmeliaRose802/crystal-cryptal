use super::*;
use crate::ir::{Item, ProofStatus};

fn mk_fn(name: &str, status: Option<ProofStatus>) -> Item {
    Item::Function {
        name: name.into(),
        signature: "Bit -> Bit".into(),
        branches: vec![],
        body: "f x = x".into(),
        doc: vec![],
        proof_status: status,
        is_private: false,
    }
}

fn proven(iters: Option<u64>) -> ProofStatus {
    ProofStatus::Proven {
        solver: "z3".into(),
        time_secs: Some(0.1),
        overrides: vec![],
        iterations: iters,
        verify_command: None,
        verify_script: None,
        verify_script_body: None,
        override_specs: std::collections::HashMap::new(),
    }
}

#[test]
fn classify_proven_full() {
    let items = vec![mk_fn("provisionKey", Some(proven(None)))];
    let modules = vec![("SDEP".to_string(), "".to_string(), items.as_slice())];
    let inv = ImplementationInventory::default();
    let cfg = CoverageConfig::default();
    let ledger = build_ledger(&modules, &inv, &cfg);
    let e = ledger.lookup("provisionKey").expect("present");
    assert_eq!(e.badge, CoverageBadge::Proven);
}

#[test]
fn classify_proven_bounded() {
    let items = vec![mk_fn("canonicalize_lp", Some(proven(Some(16))))];
    let modules = vec![("SDEP".to_string(), "".to_string(), items.as_slice())];
    let ledger = build_ledger(
        &modules,
        &ImplementationInventory::default(),
        &CoverageConfig::default(),
    );
    assert_eq!(
        ledger.lookup("canonicalize_lp").unwrap().badge,
        CoverageBadge::ProvenBounded
    );
}

#[test]
fn classify_abstraction_via_config() {
    let items = vec![mk_fn("hmacSha256", None)];
    let modules = vec![("SDEP".to_string(), "".to_string(), items.as_slice())];
    let mut cfg = CoverageConfig::default();
    cfg.abstraction
        .insert("hmacSha256".into(), "Algebraic placeholder.".into());
    let ledger = build_ledger(&modules, &ImplementationInventory::default(), &cfg);
    let e = ledger.lookup("hmacSha256").unwrap();
    assert_eq!(e.badge, CoverageBadge::AbiAdapter);
    assert_eq!(
        e.abstraction_note.as_deref(),
        Some("Algebraic placeholder.")
    );
}

#[test]
fn classify_trusted_assumption_via_config() {
    let items = vec![mk_fn("hmacSha256", None)];
    let modules = vec![("SDEP".to_string(), "".to_string(), items.as_slice())];
    let mut cfg = CoverageConfig::default();
    cfg.assumption
        .insert("hmacSha256".into(), "Trusted primitive.".into());
    let ledger = build_ledger(&modules, &ImplementationInventory::default(), &cfg);
    let e = ledger.lookup("hmacSha256").unwrap();
    assert_eq!(e.badge, CoverageBadge::TrustedAssumption);
    assert_eq!(e.assumption_note.as_deref(), Some("Trusted primitive."));
}

#[test]
fn classify_spec_only_via_config() {
    let items = vec![mk_fn("secureProvisionKey", None)];
    let modules = vec![("SDEP".to_string(), "".to_string(), items.as_slice())];
    let cfg = CoverageConfig {
        spec_only: vec!["secureProvisionKey".into()],
        ..CoverageConfig::default()
    };
    let ledger = build_ledger(&modules, &ImplementationInventory::default(), &cfg);
    assert_eq!(
        ledger.lookup("secureProvisionKey").unwrap().badge,
        CoverageBadge::SpecOnly
    );
}

#[test]
fn spec_only_overrides_proof() {
    // [spec_only] is an authorial decision; it wins over any incidental
    // proven status (a counterexample fixture might claim it, but the
    // taxonomy says spec-only).
    let items = vec![mk_fn("secureProvisionKey", Some(proven(None)))];
    let modules = vec![("SDEP".to_string(), "".to_string(), items.as_slice())];
    let cfg = CoverageConfig {
        spec_only: vec!["secureProvisionKey".into()],
        ..CoverageConfig::default()
    };
    let ledger = build_ledger(&modules, &ImplementationInventory::default(), &cfg);
    assert_eq!(
        ledger.lookup("secureProvisionKey").unwrap().badge,
        CoverageBadge::SpecOnly
    );
}

#[test]
fn impl_only_function_shows_up_as_unverified() {
    let inv = ImplementationInventory {
        functions: vec![InventoryEntry {
            name: "sha256".into(),
            lang: "cpp".into(),
            symbol: None,
            file: Some("cpp/src/sha256.cpp".into()),
            models: None,
            models_note: None,
            composes: vec![],
            reason_codes: vec![],
        }],
    };
    let ledger = build_ledger(&[], &inv, &CoverageConfig::default());
    let e = ledger.lookup("sha256").expect("inventory-only entry");
    assert_eq!(e.badge, CoverageBadge::Unverified);
    assert_eq!(e.source, LedgerSource::ImplementationOnly);
}

#[test]
fn excluded_helper_is_dropped_and_counted() {
    let inv = ImplementationInventory {
        functions: vec![InventoryEntry {
            name: "to_lower".into(),
            lang: "cpp".into(),
            symbol: None,
            file: None,
            models: None,
            models_note: None,
            composes: vec![],
            reason_codes: vec![],
        }],
    };
    let cfg = CoverageConfig {
        exclude: vec!["to_lower".into()],
        ..CoverageConfig::default()
    };
    let ledger = build_ledger(&[], &inv, &cfg);
    assert!(ledger.lookup("to_lower").is_none());
    assert_eq!(ledger.excluded, vec!["to_lower".to_string()]);
}

#[test]
fn render_matrix_emits_all_sections() {
    let items = vec![
        mk_fn("provisionKey", Some(proven(None))),
        mk_fn("canonicalize_lp", Some(proven(Some(16)))),
        mk_fn("hmacSha256", None),
        mk_fn("secureProvisionKey", None),
    ];
    let modules = vec![("SDEP".to_string(), "".to_string(), items.as_slice())];
    let inv = ImplementationInventory {
        functions: vec![InventoryEntry {
            name: "sha256".into(),
            lang: "cpp".into(),
            symbol: None,
            file: Some("cpp/src/sha256.cpp".into()),
            models: Some("hmacSha256".into()),
            models_note: None,
            composes: vec![],
            reason_codes: vec!["R2".into()],
        }],
    };
    let cfg = CoverageConfig {
        exclude: vec![],
        assumption: std::collections::HashMap::new(),
        abstraction: [("hmacSha256".to_string(), "Placeholder.".to_string())]
            .into_iter()
            .collect(),
        spec_only: vec!["secureProvisionKey".into()],
        reason_codes: std::collections::HashMap::new(),
    };
    let ledger = build_ledger(&modules, &inv, &cfg);
    let md = render_coverage_matrix(&ledger);

    assert!(md.contains("# Coverage Matrix"));
    assert!(md.contains("✅ Proven"));
    assert!(md.contains("🔲 Proven (bounded)"));
    assert!(md.contains("🧩 ABI adapter / stand-in"));
    assert!(md.contains("⚠️ Implemented, unverified"));
    assert!(md.contains("📄 Spec-only"));
    assert!(md.contains("sha256"));
    assert!(md.contains("Placeholder."));
}

#[test]
fn config_parses_toml() {
    let tmp = std::env::temp_dir().join("pretty_specs_cov_cfg.toml");
    std::fs::write(
        &tmp,
        r#"
[exclude]
functions = ["to_lower", "trim"]

[abstraction]
hmacSha256 = "Not SHA-256."

[assumption]
isValidSignature = "Trusted external check."

[spec_only]
functions = ["secureFoo"]

[reason_codes]
canonicalizePayload = ["r2", "R1"]
"#,
    )
    .unwrap();
    let cfg = load_coverage_config(&tmp).expect("parse");
    assert!(cfg.is_excluded("to_lower"));
    assert!(cfg.is_excluded("trim"));
    assert_eq!(cfg.abstraction_note("hmacSha256"), Some("Not SHA-256."));
    assert_eq!(
        cfg.assumption_note("isValidSignature"),
        Some("Trusted external check.")
    );
    assert!(cfg.is_spec_only("secureFoo"));
    assert_eq!(
        cfg.reason_codes("canonicalizePayload"),
        vec!["R2".to_string(), "R1".to_string()]
    );
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn missing_config_file_is_empty_default() {
    let cfg = load_coverage_config(std::path::Path::new("nonexistent_coverage_config.toml"))
        .expect("missing file is ok");
    assert!(cfg.exclude.is_empty());
    assert!(cfg.assumption.is_empty());
    assert!(cfg.abstraction.is_empty());
    assert!(cfg.spec_only.is_empty());
    assert!(cfg.reason_codes.is_empty());
}

#[test]
fn missing_inventory_file_is_empty_default() {
    let inv = load_inventory(std::path::Path::new("nonexistent_inventory.json"))
        .expect("missing file is ok");
    assert!(inv.functions.is_empty());
}
