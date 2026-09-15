use super::*;
use crate::ir::{Item, ProofStatus};

fn disproved_item(name: &str, counterexample: Option<&str>) -> Item {
    Item::Function {
        name: name.into(),
        signature: "Bit -> Bit".into(),
        branches: vec![],
        body: format!("{name} x = x"),
        doc: vec![],
        proof_status: Some(ProofStatus::Failed {
            reason: "counterexample found".into(),
            counterexample: counterexample.map(str::to_string),
            log_excerpt: Some("Solver returned SAT".into()),
            verify_command: None,
            verify_script: None,
            proof_script: None,
            clauses: vec![],
        }),
        is_private: false,
    }
}

fn ledger_for(item: &Item) -> Ledger {
    let modules = vec![(
        "SDEP".to_string(),
        "".to_string(),
        std::slice::from_ref(item),
    )];
    build_ledger(
        &modules,
        &ImplementationInventory::default(),
        &CoverageConfig::default(),
    )
}

#[test]
fn classify_counterexample_as_disproved() {
    let item = disproved_item("badClaim", Some("x = 1"));
    let ledger = ledger_for(&item);

    assert_eq!(
        ledger.lookup("badClaim").unwrap().badge,
        CoverageBadge::Disproved
    );
}

#[test]
fn render_matrix_separates_disproved_and_shows_counterexample() {
    let item = disproved_item(
        "authenticate_spec_mismatch",
        Some("role = 0\nexpected = false\nactual = true"),
    );
    let md = render_coverage_matrix(&ledger_for(&item));

    assert!(md.contains("❌ Disproved"), "matrix: {md}");
    assert!(
        md.contains("disproved: counterexample found"),
        "matrix: {md}"
    );
    assert!(
        md.contains("<details open><summary>Counterexample"),
        "matrix: {md}"
    );
    assert!(md.contains("role = 0"), "matrix: {md}");
    assert!(md.contains("Copy counterexample"), "matrix: {md}");
    assert!(!md.contains("⚠️ Implemented, unverified"), "matrix: {md}");
}
