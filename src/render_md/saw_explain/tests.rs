use super::*;

const SCRIPT: &str = r#"// Auto-generated SAW verification script
// Step 1: Load bitcode
m <- llvm_load_module "key_store.bc";

// Step 2: Bitcode-derived extern overrides
// override: ?_Mymtx@_Mutex_base@std@@AEAAPEAU_Mtx_internal_imp_t@@XZ  [msvc-mutex-helper]
let mutex_spec = do {
    p0 <- llvm_fresh_pointer (llvm_int 8);
    llvm_execute_func [p0];
    rv <- llvm_fresh_pointer (llvm_int 8);
    llvm_return rv;
};
ov_mutex <- llvm_unsafe_assume_spec m "?_Mymtx@_Mutex_base@std@@AEAAPEAU_Mtx_internal_imp_t@@XZ" mutex_spec;

// override: _Mtx_lock  [declare-only]
let lock_spec = do {
    p0 <- llvm_fresh_pointer (llvm_int 8);
    llvm_execute_func [p0];
    llvm_return (llvm_term {{ 0 : [32] }});
};
ov_lock <- llvm_unsafe_assume_spec m "_Mtx_lock" lock_spec;

// Step 3: Import Cryptol spec
import "SDEP.cry";

// Step 4: Uninterpreted primitive contracts
// uninterpreted: uuidEq (symbol: ??8sdep@@YA_NAEBUUuid@0@0@Z)
let uuidEq_uninterp_spec = do {
    a0_ptr <- llvm_alloc_readonly (llvm_array 16 (llvm_int 8));
    a0 <- llvm_fresh_var "a0" (llvm_array 16 (llvm_int 8));
    llvm_points_to a0_ptr (llvm_term a0);
    a1_ptr <- llvm_alloc_readonly (llvm_array 16 (llvm_int 8));
    a1 <- llvm_fresh_var "a1" (llvm_array 16 (llvm_int 8));
    llvm_points_to a1_ptr (llvm_term a1);
    llvm_execute_func [a0_ptr, a1_ptr];
    llvm_return (llvm_term {{ [uuidEq a0 a1] : [1] }});
};
ov_uuid <- llvm_unsafe_assume_spec m "??8sdep@@YA_NAEBUUuid@0@0@Z" uuidEq_uninterp_spec;

// Step 5: Equivalence spec
"#;

fn proven_with_script(script: &str) -> Option<ProofStatus> {
    Some(ProofStatus::Proven {
        solver: "z3".into(),
        time_secs: None,
        overrides: vec![],
        iterations: None,
        verify_command: None,
        verify_script: Some("verify_out/out_provision/verify.saw".into()),
        proof_script: Some(script.into()),
        clauses: vec![],
    })
}

#[test]
fn parses_setup_and_retains_exact_contract_segments() {
    let setup = parse_proof_setup(SCRIPT);
    assert_eq!(setup.bitcode.as_ref().unwrap().file, "key_store.bc");
    assert_eq!(setup.extern_overrides.len(), 2);
    assert_eq!(setup.extern_overrides[1].symbol, "_Mtx_lock");
    assert_eq!(setup.extern_overrides[1].category, Some("declare-only"));
    assert!(setup.extern_overrides[0].source.contains("llvm_return rv"));
    assert_eq!(setup.uninterpreted.len(), 1);
    assert_eq!(setup.uninterpreted[0].name, "uuidEq");
    assert_eq!(setup.uninterpreted[0].symbol, "??8sdep@@YA_NAEBUUuid@0@0@Z");
}

#[test]
fn renders_a_beginner_readable_and_auditable_trust_boundary() {
    let rendered = render_saw_explanation(&proven_with_script(SCRIPT)).unwrap();
    assert!(rendered.contains("How this proof connects to the program"));
    assert!(rendered.contains("<code>key_store.bc</code>"));
    assert!(rendered.contains("program representation SAW analyzed"));
    assert!(rendered.contains("2 external runtime contracts"));
    assert!(rendered.contains("Access the C++ mutex handle"));
    assert!(rendered.contains("It may return **any pointer**"));
    assert!(rendered.contains("fixed value <code>0</code> as a 32-bit result"));
    assert!(rendered.contains("Exact linked symbol: ?_Mymtx@_Mutex_base@std@@"));
    assert!(rendered.contains("Mathematical primitive contracts"));
    assert!(rendered.contains("<code>a0</code> is arbitrary"));
    assert!(rendered.contains("every 16-byte array value"));
    assert!(rendered.contains("Cryptol expression <code>[uuidEq a0 a1] : [1]</code>"));
    assert!(rendered.contains("Exact SAW contract"));
    assert!(rendered.contains("```saw"));
    assert!(rendered.contains("llvm_unsafe_assume_spec"));
    assert!(rendered.contains("Copy exact SAW"));
}

#[test]
fn omits_the_section_when_no_embedded_script_is_available() {
    let status = Some(ProofStatus::Proven {
        solver: "z3".into(),
        time_secs: None,
        overrides: vec![],
        iterations: None,
        verify_command: None,
        verify_script: Some("verify.saw".into()),
        proof_script: None,
        clauses: vec![],
    });
    assert!(render_saw_explanation(&status).is_none());
    assert!(render_generated_script(&status).is_none());
}

#[test]
fn renders_the_complete_embedded_script_in_one_collapsed_accordion() {
    let rendered = render_generated_script(&proven_with_script(SCRIPT)).unwrap();
    assert!(rendered.contains("Generated SAW verification script"));
    assert!(rendered.contains("Show complete generated script · includes 3 trusted contracts"));
    assert!(rendered.contains("// Step 5: Equivalence spec"));
    assert_eq!(rendered.matches("```saw").count(), 1);
    assert!(rendered.contains("without a local <code>verify_out</code> directory"));
}
