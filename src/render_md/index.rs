// Renders the top-level `index.md` for a single module.

use std::collections::HashMap;
use std::fmt::Write as FmtWrite;

use crate::ir::{Item, ProofStatus};
use crate::linker::SymbolTable;

use super::categories::{collect_categories, property_range, render_category_status};
use super::equivalence::function_status_map;
use super::fns_index::{collect_functions_for_index, render_functions_table};
use super::util::is_simple_constructor;
use super::RenderOptions;

pub(super) fn render_index(
    items: &[Item],
    symbols: &SymbolTable,
    options: &RenderOptions,
    _path_prefix: &str,
) -> String {
    let mut out = String::new();

    let (module_name, module_doc) = items
        .iter()
        .find_map(|item| match item {
            Item::Module { name, doc } => Some((name.clone(), doc.clone())),
            _ => None,
        })
        .unwrap_or_else(|| ("Specification".into(), vec![]));

    let title = options.title_override.as_deref().unwrap_or(&module_name);
    let _ = writeln!(out, "# {title}\n");

    if !module_doc.is_empty() {
        for line in &module_doc {
            let _ = writeln!(out, "{line}");
        }
        out.push('\n');
    }

    if items
        .iter()
        .any(|i| matches!(i, Item::Property { .. } | Item::Function { .. }))
    {
        let _ = writeln!(out, "## How verification works here\n");
        let _ = writeln!(
            out,
            "This site reports **two independent layers of proof**, and a \
             security claim about the production binary needs *both*:\n"
        );
        let _ = writeln!(
            out,
            "1. **Properties are proven against the design.** Each entry in \
             [Security Properties](#security-properties) is a Cryptol \
             `property` discharged by a solver (typically Z3) over the \
             Cryptol model. A `✓` here says the *logic of the spec* is \
             sound.\n"
        );
        let _ = writeln!(
            out,
            "2. **Functions are proven against the implementation.** Each \
             entry in [Functions](functions/index.md) is a Cryptol shim \
             paired with a SAW `llvm_verify` / `mir_verify` proof showing \
             the C++/Rust implementation produces identical outputs on every \
             input. A `✓` here says *the code matches the model*.\n"
        );
        let _ = writeln!(
            out,
            "A property's guarantee therefore only transfers to the compiled \
             binary insofar as **every function it mentions** also carries a \
             SAW equivalence proof. Each property page surfaces this \
             transitive status as an *Implementation equivalence* callout: \
             if any involved function is unproven, assumed, or failed, the \
             callout calls that out explicitly so a green Cryptol verdict \
             isn't read as an end-to-end certificate.\n"
        );
    }

    let params: Vec<_> = items
        .iter()
        .filter(|i| matches!(i, Item::ModuleParam { .. }))
        .collect();
    if !params.is_empty() {
        let _ = writeln!(out, "## Module Parameters\n");
        for item in &params {
            if let Item::ModuleParam {
                name,
                kind,
                signature,
                doc,
            } = item
            {
                match kind {
                    crate::ir::ParamKind::TypeParam => {
                        let doc_str = if doc.is_empty() {
                            String::new()
                        } else {
                            format!(" — {}", doc.join(" ").trim())
                        };
                        let _ = writeln!(out, "- **`type {name}`** : `{signature}`{doc_str}");
                    }
                    crate::ir::ParamKind::ValueParam => {
                        let doc_str = if doc.is_empty() {
                            String::new()
                        } else {
                            format!(" — {}", doc.join(" ").trim())
                        };
                        let _ = writeln!(out, "- **`{name}`** : `{signature}`{doc_str}");
                    }
                    crate::ir::ParamKind::Constraint => {
                        let _ = writeln!(out, "- *Constraint:* `{signature}`");
                    }
                }
            }
        }
        out.push('\n');
    }

    let _ = writeln!(out, "## Types\n");
    let _ = writeln!(out, "All type definitions: [types.md](types.md)\n");

    let has_functions = items.iter().any(|item| match item {
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

    if has_functions {
        let _ = writeln!(out, "## Functions\n");
        let fns = collect_functions_for_index(items);
        if !fns.is_empty() {
            out.push_str(&render_functions_table(&fns, "functions/"));
        }
        let _ = writeln!(
            out,
            "Per-function detail pages: [functions](functions/index.md)\n"
        );
    }

    let categories = collect_categories(items, symbols);
    if !categories.is_empty() {
        let prop_info: HashMap<&str, (&Option<ProofStatus>, &str, &[String])> = items
            .iter()
            .filter_map(|i| match i {
                Item::Property {
                    label,
                    body,
                    doc,
                    proof_status,
                    ..
                } => Some((
                    label.as_str(),
                    (proof_status, body.as_str(), doc.as_slice()),
                )),
                _ => None,
            })
            .collect();
        let fn_status = function_status_map(items);

        let has_any_status = prop_info.values().any(|(s, _, _)| s.is_some());

        let _ = writeln!(out, "## Security Properties\n");
        if has_any_status {
            let _ = writeln!(out, "| Category | Properties | Status |");
            let _ = writeln!(out, "|----------|------------|--------|");
        } else {
            let _ = writeln!(out, "| Category | Properties |");
            let _ = writeln!(out, "|----------|------------|");
        }
        for (cat_title, cat_slug, labels) in &categories {
            let range = property_range(labels);
            if has_any_status {
                let status_cell = render_category_status(labels, &prop_info, &fn_status);
                let _ = writeln!(
                    out,
                    "| [{cat_title}](properties/{cat_slug}.md) | {range} | {status_cell} |"
                );
            } else {
                let _ = writeln!(out, "| [{cat_title}](properties/{cat_slug}.md) | {range} |");
            }
        }
        out.push('\n');
    }

    let all_props: Vec<_> = items
        .iter()
        .filter_map(|item| match item {
            Item::Property { proof_status, .. } => Some(proof_status),
            _ => None,
        })
        .collect();

    let has_any_status = all_props.iter().any(|s| s.is_some());
    if has_any_status {
        let total = all_props.len();
        let mut proven = 0usize;
        let mut assumed = 0usize;
        let mut failed = 0usize;
        let mut not_attempted = 0usize;
        for ps in &all_props {
            match ps {
                Some(ProofStatus::Proven { .. }) => proven += 1,
                Some(ProofStatus::Assumed) => assumed += 1,
                Some(ProofStatus::Failed { .. }) => failed += 1,
                Some(ProofStatus::NotAttempted) => not_attempted += 1,
                None => {}
            }
        }
        let mut parts = vec![format!("{proven}/{total} properties proven")];
        if assumed > 0 {
            parts.push(format!("{assumed} assumed"));
        }
        if failed > 0 {
            parts.push(format!("{failed} failed"));
        }
        if not_attempted > 0 {
            parts.push(format!("{not_attempted} not attempted"));
        }
        let _ = writeln!(out, "{}\n", parts.join(", "));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_items() -> Vec<Item> {
        let src = std::fs::read_to_string("examples/SDEP.cry").expect("SDEP.cry not found");
        crate::parser::parse(&src)
    }

    #[test]
    fn render_index_contains_title() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let options = RenderOptions {
            no_details: false,
            title_override: None,
            docfx: false,
        };
        let index = render_index(&items, &symbols, &options, "");
        assert!(
            index.contains("# SDEP"),
            "index should contain module title"
        );
    }

    #[test]
    fn render_index_contains_functions_link() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let options = RenderOptions {
            no_details: false,
            title_override: None,
            docfx: false,
        };
        let index = render_index(&items, &symbols, &options, "");
        assert!(
            index.contains("[functions](functions/index.md)"),
            "index should link to functions index page"
        );
    }

    #[test]
    fn render_index_contains_properties_table() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let options = RenderOptions {
            no_details: false,
            title_override: None,
            docfx: false,
        };
        let index = render_index(&items, &symbols, &options, "");
        assert!(
            index.contains("[Key Lifecycle Safety](properties/key-lifecycle-safety.md)"),
            "index should link to key-lifecycle-safety"
        );
    }
}
