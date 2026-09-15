use std::fs;
use std::path::Path;

use pretty_specs::coverage::{
    CoverageConfig, ImplementationInventory, InventoryEntry, build_ledger, load_coverage_config,
    load_inventory, render_coverage_content, render_coverage_matrix,
};
use pretty_specs::ir::{Item, ProofStatus, load_proof_manifest};
use pretty_specs::linker::SymbolTable;
use pretty_specs::parser::parse;
use pretty_specs::render_md::{RenderOptions, render_multi_file, render_single_file};

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
            && let Some(status) = manifest.properties.get(label)
        {
            *proof_status = Some(status.clone());
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
        ledger: None,
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

// ── Coverage ledger snapshots ───────────────────────────────────────────────

fn load_sdep_with_coverage() -> (Vec<Item>, SymbolTable, RenderOptions) {
    let source = fs::read_to_string("tests/fixtures/SDEP.cry").expect("fixture");
    let mut items = parse(&source);
    let manifest = load_proof_manifest(Path::new("tests/fixtures/proof_manifest.json")).unwrap();
    for item in &mut items {
        match item {
            Item::Property {
                label,
                proof_status,
                ..
            } => {
                if let Some(status) = manifest.properties.get(label) {
                    *proof_status = Some(status.clone());
                }
            }
            Item::Function {
                name, proof_status, ..
            } => {
                if let Some(status) = manifest.functions.get(name) {
                    *proof_status = Some(status.clone());
                }
            }
            _ => {}
        }
    }
    let symbols = SymbolTable::build(&items);
    let inv = load_inventory(Path::new("tests/fixtures/implementation_inventory.json")).unwrap();
    let cfg = load_coverage_config(Path::new("tests/fixtures/coverage.toml")).unwrap();
    let mut ledger = build_ledger(
        &[("SDEP".to_string(), "".to_string(), items.as_slice())],
        &inv,
        &cfg,
    );
    ledger.source_url_base = Some("https://github.com/example/protocol/blob/verification/".into());
    let opts = RenderOptions {
        ledger: Some(ledger),
        ..RenderOptions::default()
    };
    (items, symbols, opts)
}

#[test]
fn snapshot_coverage_matrix() {
    let (_items, _symbols, opts) = load_sdep_with_coverage();
    let ledger = opts.ledger.as_ref().expect("ledger");
    let md = render_coverage_matrix(ledger);
    insta::assert_snapshot!("coverage_matrix", md);
}

#[test]
fn snapshot_function_page_with_abstraction_banner() {
    // hmacSha256 is marked as a trusted assumption in coverage.toml; the
    // page must carry a 🔒 banner explaining the trust boundary.
    let (items, symbols, opts) = load_sdep_with_coverage();
    let dir = render_to("cov_fn_abs");
    render_multi_file(&items, &symbols, &dir, &opts).unwrap();
    let content = fs::read_to_string(dir.join("functions/hmacSha256.md")).unwrap();
    assert!(
        content.contains("🔒"),
        "trusted-assumption banner missing: {content}"
    );
    assert!(
        content.contains("Trusted HMAC contract"),
        "trusted-assumption note missing: {content}"
    );
    insta::assert_snapshot!("coverage_fn_hmac_sha256", content);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn snapshot_function_page_with_unverified_badge() {
    // provisionKey has no proof in the fixture manifest (only properties
    // are proven). With the ledger active, its page should render the
    // ⚠️ "Implemented, unverified" banner instead of the legacy ✗.
    let (items, symbols, opts) = load_sdep_with_coverage();
    let dir = render_to("cov_fn_unv");
    render_multi_file(&items, &symbols, &dir, &opts).unwrap();
    let content = fs::read_to_string(dir.join("functions/provisionKey.md")).unwrap();
    assert!(
        content.contains("⚠️"),
        "unverified banner missing: {content}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn snapshot_index_with_coverage_section() {
    let (items, symbols, opts) = load_sdep_with_coverage();
    let dir = render_to("cov_index");
    render_multi_file(&items, &symbols, &dir, &opts).unwrap();
    let content = fs::read_to_string(dir.join("index.md")).unwrap();
    assert!(
        content.contains("## Coverage summary"),
        "coverage summary missing: {content}"
    );
    assert!(
        content.contains("[Coverage Matrix](coverage.md)"),
        "coverage link missing: {content}"
    );
    insta::assert_snapshot!("coverage_index", content);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn coverage_md_is_written_to_output() {
    // Smoke test: when a ledger is in RenderOptions, render_multi_file
    // itself does NOT write coverage.md (main.rs owns that step). The
    // matrix renderer is called separately. Verify it produces non-empty
    // output without panicking.
    let (_items, _symbols, opts) = load_sdep_with_coverage();
    let ledger = opts.ledger.as_ref().expect("ledger");
    let md = render_coverage_matrix(ledger);
    assert!(md.contains("# Coverage Matrix"));
    assert!(md.contains("⚠️"));
    assert!(md.contains("🧩"));
    assert!(md.contains("✅") || md.contains("🔲"));
}

#[test]
fn home_embeds_the_filtered_coverage_report_with_unverified_first() {
    let source = r#"
module CoverageAcceptance where

provenModel : Bit -> Bit
provenModel x = x

unverifiedModel : Bit -> Bit
unverifiedModel x = x

// @coverage trusted: external primitive contract.
trustedModel : Bit -> Bit
trustedModel x = x

excludedHelper : Bit -> Bit
excludedHelper x = x

private
  // @coverage exclude
  internalPost : Bit -> Bit
  internalPost x = x
"#;
    let mut items = parse(source);
    for item in &mut items {
        if let Item::Function {
            name, proof_status, ..
        } = item
        {
            *proof_status = match name.as_str() {
                "provenModel" => Some(ProofStatus::Proven {
                    solver: "z3".into(),
                    time_secs: Some(0.2),
                    overrides: vec![],
                    iterations: None,
                    verify_command: None,
                    verify_script: None,
                }),
                "unverifiedModel" => Some(ProofStatus::Failed {
                    reason: "unsupported type: %reference".into(),
                    counterexample: None,
                    log_excerpt: Some(
                        "Error: unsupported type: %reference\nUnknown type alias Ident \"reference\""
                            .into(),
                    ),
                    verify_command: None,
                    verify_script: Some("verify_out/out_unverified/generated.saw".into()),
                }),
                _ => None,
            };
        }
    }
    let inventory = ImplementationInventory {
        functions: vec![
            InventoryEntry {
                name: "provenImpl".into(),
                lang: "cpp".into(),
                symbol: None,
                file: Some("cpp/src/proven.cpp".into()),
                models: Some("provenModel".into()),
                models_note: None,
                composes: vec![],
                reason_codes: vec![],
            },
            InventoryEntry {
                name: "unverifiedImpl".into(),
                lang: "cpp".into(),
                symbol: None,
                file: Some("cpp/src/unverified.cpp".into()),
                models: Some("unverifiedModel".into()),
                models_note: None,
                composes: vec![],
                reason_codes: vec![],
            },
        ],
    };
    let config = CoverageConfig {
        exclude: vec!["excludedHelper".into()],
        ..CoverageConfig::default()
    };
    let mut ledger = build_ledger(
        &[("CoverageAcceptance".into(), "".into(), items.as_slice())],
        &inventory,
        &config,
    );
    ledger.source_url_base = Some("https://github.com/example/protocol/blob/verification/".into());
    assert!(
        ledger.lookup("provenImpl").is_none(),
        "implementation row was duplicated"
    );
    assert_eq!(
        ledger.lookup("provenModel").unwrap().impl_file.as_deref(),
        Some("cpp/src/proven.cpp")
    );

    let shared = render_coverage_content(&ledger);
    let matrix = render_coverage_matrix(&ledger);
    assert!(matrix.ends_with(&shared));
    assert!(shared.find("⚠️ Implemented, unverified").unwrap() < shared.find("✅ Proven").unwrap());
    assert!(shared.find("✅ Proven").unwrap() < shared.find("🔒 Trusted assumptions").unwrap());
    assert!(shared.contains("failed: unsupported type: %reference"));
    assert!(shared.contains("Complete verifier diagnostics — <code>unverifiedImpl</code>"));
    assert!(shared.contains(
        "<summary>Complete verifier diagnostics — <code>unverifiedImpl</code></summary>\n\n```text\nError: unsupported type: %reference\nUnknown type alias Ident \"reference\"\n```"
    ));
    assert!(shared.contains("aria-label=\"Copy verifier log\""));
    assert!(shared.contains(
        "[source](https://github.com/example/protocol/blob/verification/cpp/src/unverified.cpp)"
    ));
    assert!(shared.contains(
        "[`unverifiedImpl`](https://github.com/example/protocol/blob/verification/cpp/src/unverified.cpp)"
    ));
    assert!(shared.contains(
        "implementation `unverifiedImpl` ↔ model [`unverifiedModel`](functions/unverifiedModel.md)"
    ));
    assert!(shared.contains(
        "Generated SAW script: <code>generated.saw</code> (local verifier artifact; not published with this site)."
    ));
    assert!(!shared.contains("[generated SAW script]"));
    for line in shared.lines().filter(|line| line.starts_with('|')) {
        assert!(
            !line.contains("<details") && !line.contains("<pre>"),
            "DocFX-unsafe content leaked into table row: {line}"
        );
    }
    let unverified_section = shared.split("## ✅ Proven").next().unwrap();
    let table_end = unverified_section.find("\n\n<details>").unwrap();
    assert!(
        unverified_section[..table_end]
            .lines()
            .filter(|line| line.starts_with('|'))
            .all(|line| !line.contains("Complete verifier diagnostics"))
    );
    let tables = shared.split("## Excluded helpers").next().unwrap();
    assert!(!tables.contains("excludedHelper"));
    assert!(!tables.contains("internalPost"));

    let symbols = SymbolTable::build(&items);
    let options = RenderOptions {
        ledger: Some(ledger),
        docfx: true,
        ..RenderOptions::default()
    };
    let dir = render_to("coverage-acceptance");
    render_multi_file(&items, &symbols, &dir, &options).unwrap();
    fs::write(dir.join("coverage.md"), &matrix).unwrap();
    let home = fs::read_to_string(dir.join("index.md")).unwrap();
    assert!(home.contains(&shared), "home and coverage content diverged");
    let functions_section = home.split("## Functions").nth(1).unwrap();
    assert!(!functions_section.contains("excludedHelper"));
    assert!(!functions_section.contains("internalPost"));
    assert_docfx_coverage_table(&dir);
    let _ = fs::remove_dir_all(dir);
}

fn assert_docfx_coverage_table(dir: &Path) {
    if std::process::Command::new("docfx")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    fs::write(
        dir.join("docfx.json"),
        r#"{
  "build": {
    "content": [{ "files": ["**/*.md"] }],
    "dest": "_site",
    "globalMetadata": { "_disableContribution": true }
  }
}"#,
    )
    .unwrap();
    let output = std::process::Command::new("docfx")
        .arg("docfx.json")
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "DocFX failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let build_log = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for warning in build_log
        .lines()
        .filter(|line| line.contains("InvalidFileLink"))
    {
        assert!(
            !warning.contains("cpp/src/")
                && !warning.contains("generated.saw")
                && !warning.contains("verify_out"),
            "coverage emitted a broken DocFX link: {warning}"
        );
    }
    for page in ["index.html", "coverage.html"] {
        let html = fs::read_to_string(dir.join("_site").join(page)).unwrap();
        assert!(
            html.contains("<table"),
            "coverage table missing from {page} DOM"
        );
        assert!(
            !html.contains("| Function | Source | Maps to | Notes |")
                && !html.contains("| Function | Source | Maps to | Reason codes | Notes |"),
            "coverage table rendered as literal Markdown in {page}"
        );
        assert!(html.contains("Complete verifier diagnostics"));
        assert!(
            html.contains("<pre><code class=\"lang-text\">")
                || html.contains("<pre><code class=\"language-text\">"),
            "diagnostics are not a highlighted text code block in {page}"
        );
        assert!(
            html.contains("aria-label=\"Copy verifier log\"")
                && html.contains("navigator.clipboard.writeText"),
            "diagnostics have no copy control in {page}"
        );
    }
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
    assert!(
        fs::read_to_string(dir.join("index.md"))
            .unwrap()
            .contains("Specification")
    );
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
    assert!(
        items
            .iter()
            .any(|item| matches!(item, Item::Module { name, .. } if name == "BomTest"))
    );
}

#[test]
fn windows_line_endings() {
    let source = "module WinTest where\r\n\r\ntype Foo = [8]\r\n";
    let items = parse(source);
    assert!(
        items
            .iter()
            .any(|item| matches!(item, Item::Module { name, .. } if name == "WinTest"))
    );
}

// The home-page / functions-index call graph was retired (it didn't pull
// its weight visually and made the page noisy).  Per-function decision
// flowcharts still render via `render_flowchart_mermaid`.  If we resurface
// a callgraph view in the future, restore an equivalent assertion here.
