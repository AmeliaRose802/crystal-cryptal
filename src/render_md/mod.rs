// Markdown renderer: emits .md files from linked IR.
//
// This module is split across topical submodules so each file stays under
// the CI line-count cap. See `.github/copilot-instructions.md` for the
// per-file size rule.

use std::fs;
use std::io;
use std::path::Path;

use crate::ir::Item;
use crate::linker::SymbolTable;

mod categories;
mod docfx;
mod equivalence;
mod fns_index;
mod functions;
mod index;
mod mermaid;
mod proof;
mod properties;
mod signature;
mod single_file;
mod types;
mod util;

pub use single_file::render_single_file;

use docfx::{docfx_frontmatter, render_docfx_toc};
use fns_index::render_functions_index;
use functions::render_function_files;
use index::render_index;
use properties::render_property_files;
use types::render_types;
use util::is_simple_constructor;

#[derive(Default)]
pub struct RenderOptions {
    pub no_details: bool,
    pub title_override: Option<String>,
    /// Emit DocFX-compatible front-matter and toc.yml files.
    pub docfx: bool,
    /// Optional coverage ledger. When present, per-page badges and the
    /// Functions index switch from the legacy single-glyph `✓ / ✗`
    /// vocabulary to the five-badge taxonomy (`✅ 🔲 🧩 ⚠️ 📄`), and a
    /// `coverage.md` matrix page is emitted at the output root.
    pub ledger: Option<crate::coverage::Ledger>,
}

/// Render a complete set of Markdown files to the output directory.
pub fn render_multi_file(
    items: &[Item],
    symbols: &SymbolTable,
    output_dir: &Path,
    options: &RenderOptions,
) -> io::Result<()> {
    render_multi_file_with_prefix(items, symbols, output_dir, options, "")
}

pub fn render_multi_file_with_prefix(
    items: &[Item],
    symbols: &SymbolTable,
    output_dir: &Path,
    options: &RenderOptions,
    path_prefix: &str,
) -> io::Result<()> {
    fs::create_dir_all(output_dir)?;

    let has_types = items.iter().any(|i| {
        matches!(
            i,
            Item::TypeAlias { .. } | Item::EnumGroup { .. } | Item::RecordType { .. }
        )
    });

    let has_functions = items.iter().any(|i| match i {
        Item::Function {
            name,
            signature,
            branches,
            body,
            ..
        } => {
            (signature.contains("->") || !branches.is_empty())
                && !is_simple_constructor(name, signature, branches, body)
        }
        _ => false,
    });

    let has_properties = items.iter().any(|i| matches!(i, Item::Property { .. }));

    let module_name = items
        .iter()
        .find_map(|i| {
            if let Item::Module { name, .. } = i {
                Some(name.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "Specification".into());
    let title = options
        .title_override
        .as_deref()
        .unwrap_or(&module_name)
        .to_string();

    let mut index_md = render_index(items, symbols, options, path_prefix);
    if options.docfx {
        index_md = format!("{}{}", docfx_frontmatter(&module_name, &title), index_md);
    }
    fs::write(output_dir.join("index.md"), index_md)?;

    if has_types {
        let types = render_types(items, symbols, path_prefix);
        fs::write(output_dir.join("types.md"), types)?;
    }

    if has_functions {
        fs::create_dir_all(output_dir.join("functions"))?;
        let mut functions_index = render_functions_index(items, symbols, options, path_prefix);
        if options.docfx {
            let fn_uid = format!("{module_name}.functions");
            functions_index = format!(
                "{}{}",
                docfx_frontmatter(&fn_uid, "Functions"),
                functions_index
            );
        }
        fs::write(output_dir.join("functions/index.md"), functions_index)?;
        render_function_files(items, symbols, output_dir, options, path_prefix)?;
    }

    if has_properties {
        fs::create_dir_all(output_dir.join("properties"))?;
        render_property_files(items, symbols, output_dir, options, path_prefix)?;
    }

    if options.docfx {
        let toc = render_docfx_toc(&title, items);
        fs::write(output_dir.join("toc.yml"), toc)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;

    fn load_items() -> Vec<Item> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("SDEP.cry");
        let src = stdfs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("SDEP.cry not found at {}: {e}", path.display()));
        crate::parser::parse(&src)
    }

    #[test]
    fn render_multi_file_creates_files() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let options = RenderOptions {
            no_details: false,
            title_override: None,
            docfx: false,
            ledger: None,
        };
        let tmpdir = std::env::temp_dir().join("pretty_specs_test");
        let _ = stdfs::remove_dir_all(&tmpdir);
        render_multi_file(&items, &symbols, &tmpdir, &options).expect("render failed");

        assert!(tmpdir.join("index.md").exists(), "index.md should exist");
        assert!(tmpdir.join("types.md").exists(), "types.md should exist");
        assert!(
            tmpdir.join("functions/provisionKey.md").exists(),
            "provisionKey.md should exist"
        );
        assert!(
            tmpdir.join("properties/key-lifecycle-safety.md").exists(),
            "key-lifecycle-safety.md should exist"
        );

        let _ = stdfs::remove_dir_all(&tmpdir);
    }

    #[test]
    fn title_override_works() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let options = RenderOptions {
            no_details: false,
            title_override: Some("My Custom Title".into()),
            docfx: false,
            ledger: None,
        };
        let index = render_index(&items, &symbols, &options, "");
        assert!(
            index.contains("# My Custom Title"),
            "index should use title override"
        );
    }

    #[test]
    fn no_details_omits_folds() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let options = RenderOptions {
            no_details: true,
            title_override: None,
            docfx: false,
            ledger: None,
        };
        let tmpdir = std::env::temp_dir().join("pretty_specs_test_nodetails");
        let _ = stdfs::remove_dir_all(&tmpdir);
        render_multi_file(&items, &symbols, &tmpdir, &options).expect("render failed");

        let provision = stdfs::read_to_string(tmpdir.join("functions/provisionKey.md")).unwrap();
        assert!(
            !provision.contains("<details>"),
            "no_details should suppress detail folds"
        );

        let _ = stdfs::remove_dir_all(&tmpdir);
    }

    #[test]
    fn intentional_counterexample_rendering_in_category_page() {
        let source = r#"
module Demo where

// ---- Category Z: Intentional counterexamples ------------------------------

// P99: "Some tempting but false claim about the protocol."
//
// EXPECTED VERDICT: FAILS.
// Counterexample: x = 0 disproves the claim.
property P99_TemptingButFalse x = x > 0
"#;
        let items = crate::parser::parse(source);
        let symbols = SymbolTable::build(&items);
        let tmpdir = std::env::temp_dir().join("pretty_specs_intentional_cex_test");
        let _ = stdfs::remove_dir_all(&tmpdir);
        let options = RenderOptions {
            no_details: false,
            title_override: None,
            docfx: false,
            ledger: None,
        };
        render_multi_file(&items, &symbols, &tmpdir, &options).unwrap();

        let cat_md =
            stdfs::read_to_string(tmpdir.join("properties/intentional-counterexamples.md"))
                .unwrap_or_else(|_| {
                    let dir = tmpdir.join("properties");
                    let first = stdfs::read_dir(&dir)
                        .unwrap()
                        .next()
                        .expect("expected at least one category file")
                        .unwrap()
                        .path();
                    stdfs::read_to_string(first).unwrap()
                });

        assert!(
            cat_md.contains("### ✗ P99"),
            "heading should be prefixed with ✗ for intentional counterexample, got:\n{cat_md}"
        );
        assert!(
            cat_md.contains("**✗ Intentionally disproven.**"),
            "loud disproven callout missing, got:\n{cat_md}"
        );
        assert!(
            cat_md.contains("**How to read this page.**") && cat_md.contains("deliberately false"),
            "page-level intro should swap to the deliberately-false variant, got:\n{cat_md}"
        );
        assert!(
            !cat_md.contains("Implementation equivalence proven"),
            "misleading equivalence callout must be suppressed for intentional counterexamples, got:\n{cat_md}"
        );

        let _ = stdfs::remove_dir_all(&tmpdir);
    }
}
