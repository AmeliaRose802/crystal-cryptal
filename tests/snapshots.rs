use std::fs;
use std::path::Path;

use pretty_specs::ir::{load_proof_manifest, Item};
use pretty_specs::linker::SymbolTable;
use pretty_specs::parser::parse;
use pretty_specs::render_md::{render_multi_file, render_single_file, RenderOptions};

fn load_sdep() -> (Vec<Item>, SymbolTable) {
    let source = fs::read_to_string("tests/fixtures/SDEP.cry").expect("fixture");
    let items = parse(&source);
    let symbols = SymbolTable::build(&items);
    (items, symbols)
}

fn load_sdep_with_proofs() -> (Vec<Item>, SymbolTable) {
    let source = fs::read_to_string("tests/fixtures/SDEP.cry").expect("fixture");
    let mut items = parse(&source);
    let manifest = load_proof_manifest(Path::new("tests/fixtures/proof_manifest.json")).unwrap();
    for item in &mut items {
        if let Item::Property {
            label,
            proof_status,
            ..
        } = item
        {
            if let Some(status) = manifest.properties.get(label) {
                *proof_status = Some(status.clone());
            }
        }
    }
    let symbols = SymbolTable::build(&items);
    (items, symbols)
}

fn default_options() -> RenderOptions {
    RenderOptions {
        no_details: false,
        title_override: None,
        docfx: false,
    }
}

// ── Multi-file mode snapshots ───────────────────────────────────────────────

fn render_to(suffix: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pretty_specs_snap_{suffix}"));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn snapshot_multi_file_index() {
    let (items, symbols) = load_sdep();
    let dir = render_to("index");
    render_multi_file(&items, &symbols, &dir, &default_options()).unwrap();
    let content = fs::read_to_string(dir.join("index.md")).unwrap();
    insta::assert_snapshot!("multi_file_index", content);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn snapshot_multi_file_types() {
    let (items, symbols) = load_sdep();
    let dir = render_to("types");
    render_multi_file(&items, &symbols, &dir, &default_options()).unwrap();
    let content = fs::read_to_string(dir.join("types.md")).unwrap();
    insta::assert_snapshot!("multi_file_types", content);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn snapshot_multi_file_fn_provision_key() {
    let (items, symbols) = load_sdep();
    let dir = render_to("fn_pk");
    render_multi_file(&items, &symbols, &dir, &default_options()).unwrap();
    let content = fs::read_to_string(dir.join("functions/provisionKey.md")).unwrap();
    insta::assert_snapshot!("multi_file_fn_provision_key", content);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn snapshot_multi_file_fn_authenticate() {
    let (items, symbols) = load_sdep();
    let dir = render_to("fn_auth");
    render_multi_file(&items, &symbols, &dir, &default_options()).unwrap();
    let content = fs::read_to_string(dir.join("functions/authenticate.md")).unwrap();
    insta::assert_snapshot!("multi_file_fn_authenticate", content);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn snapshot_multi_file_fn_verifier_timestamp_current() {
    let (items, symbols) = load_sdep();
    let dir = render_to("fn_verifier_timestamp");
    render_multi_file(&items, &symbols, &dir, &default_options()).unwrap();
    let content = fs::read_to_string(dir.join("functions/verifierTimestamp_current.md")).unwrap();
    insta::assert_snapshot!("multi_file_fn_verifier_timestamp_current", content);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn snapshot_multi_file_fn_enroll_device() {
    let (items, symbols) = load_sdep();
    let dir = render_to("fn_enroll");
    render_multi_file(&items, &symbols, &dir, &default_options()).unwrap();
    let content = fs::read_to_string(dir.join("functions/enrollDevice.md")).unwrap();
    insta::assert_snapshot!("multi_file_fn_enroll_device", content);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn snapshot_multi_file_property_key_lifecycle() {
    let (items, symbols) = load_sdep();
    let dir = render_to("prop_kl");
    render_multi_file(&items, &symbols, &dir, &default_options()).unwrap();
    let content = fs::read_to_string(dir.join("properties/key-lifecycle-safety.md")).unwrap();
    insta::assert_snapshot!("multi_file_property_key_lifecycle", content);
    let _ = fs::remove_dir_all(&dir);
}

// ── Single-file mode snapshot ───────────────────────────────────────────────

#[test]
fn snapshot_single_file() {
    let (items, symbols) = load_sdep();
    let content = render_single_file(&items, &symbols, &default_options());
    insta::assert_snapshot!("single_file", content);
}

// ── JSON mode snapshot ──────────────────────────────────────────────────────

#[test]
fn snapshot_json() {
    let (items, _symbols) = load_sdep();
    let json = serde_json::to_string_pretty(&items).unwrap();
    insta::assert_snapshot!("json_output", json);
}

// ── Proof manifest mode snapshot ────────────────────────────────────────────

#[test]
fn snapshot_property_with_proofs() {
    let (items, symbols) = load_sdep_with_proofs();
    let dir = render_to("proofs");
    render_multi_file(&items, &symbols, &dir, &default_options()).unwrap();
    let content = fs::read_to_string(dir.join("properties/key-lifecycle-safety.md")).unwrap();
    insta::assert_snapshot!("property_with_proofs", content);
    let _ = fs::remove_dir_all(&dir);
}

// ── Edge case tests ─────────────────────────────────────────────────────────

#[test]
fn empty_input_does_not_panic() {
    let items = parse("");
    assert!(items.is_empty());
    let symbols = SymbolTable::build(&items);
    // single-file mode
    let md = render_single_file(&items, &symbols, &default_options());
    assert!(md.contains("Specification"));
    // multi-file mode
    let dir = render_to("empty");
    render_multi_file(&items, &symbols, &dir, &default_options()).unwrap();
    assert!(fs::read_to_string(dir.join("index.md")).unwrap().contains("Specification"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn no_functions_does_not_panic() {
    let source = "module OnlyTypes where\n\ntype Foo = [8]\n";
    let items = parse(source);
    let symbols = SymbolTable::build(&items);
    let md = render_single_file(&items, &symbols, &default_options());
    assert!(md.contains("OnlyTypes"));
}

#[test]
fn no_properties_does_not_panic() {
    let source = "module NoProps where\n\nfoo : [8] -> [8]\nfoo x = x\n";
    let items = parse(source);
    let symbols = SymbolTable::build(&items);
    let md = render_single_file(&items, &symbols, &default_options());
    assert!(md.contains("NoProps"));
}

#[test]
fn utf8_bom_is_stripped() {
    let source = "\u{FEFF}module BomTest where\n\ntype X = [8]\n";
    // Strip BOM the same way main.rs does
    let stripped = source.strip_prefix('\u{FEFF}').unwrap_or(source);
    let items = parse(stripped);
    assert!(items.iter().any(|item| matches!(item, Item::Module { name, .. } if name == "BomTest")));
}

#[test]
fn windows_line_endings() {
    let source = "module WinTest where\r\n\r\ntype Foo = [8]\r\n";
    let items = parse(source);
    assert!(items.iter().any(|item| matches!(item, Item::Module { name, .. } if name == "WinTest")));
}

// The home-page / functions-index call graph was retired (it didn't pull
// its weight visually and made the page noisy).  Per-function decision
// flowcharts still render via `render_flowchart_mermaid`.  If we resurface
// a callgraph view in the future, restore an equivalent assertion here.
