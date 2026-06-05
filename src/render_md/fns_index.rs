// Functions/index.md page and shared function-table helpers.

use std::fmt::Write as FmtWrite;

use crate::describe::auto_describe_function;
use crate::ir::{Item, ProofStatus};
use crate::linker::SymbolTable;

use super::RenderOptions;
use super::proof::{is_useful_summary, proof_status_cell};
use super::util::{escape_md_cell, first_doc_line, is_constant_binding, is_simple_constructor};

pub(super) fn render_functions_index(
    items: &[Item],
    _symbols: &SymbolTable,
    _options: &RenderOptions,
    _path_prefix: &str,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Functions\n");

    let _ = writeln!(
        out,
        "> **What `✓` means here.** Each function below is a Cryptol *shim* \
         that mirrors the production C++/Rust implementation at the bit \
         level. A `✓` badge means SAW has discharged an `llvm_verify` / \
         `mir_verify` proof showing the implementation and the Cryptol shim \
         produce identical outputs on **all** inputs. A `✗` means the proof \
         failed, errored, or has not yet been attempted. The security \
         [Properties](../properties/) are proven against the shim, and \
         transfer to the implementation only as far as these function-level \
         equivalence proofs go — see each property page's *Implementation \
         equivalence* callout.\n"
    );

    let functions = collect_functions_for_index(items);

    if !functions.is_empty() {
        let _ = writeln!(
            out,
            "See the [home page](../index.md#functions) for the full \
             Function · Status · Description table.\n"
        );
        let _ = writeln!(out, "## All functions\n");
        for (name, _description, _status) in &functions {
            let _ = writeln!(out, "- [{name}]({name}.md)");
        }
        out.push('\n');
    }

    out
}

/// Shared collector for the Functions summary table.
pub(super) fn collect_functions_for_index(
    items: &[Item],
) -> Vec<(String, String, Option<ProofStatus>)> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Function {
                name,
                signature,
                branches,
                body,
                doc,
                proof_status,
                ..
            } => {
                if !signature.contains("->") && branches.is_empty() {
                    return None;
                }
                if is_simple_constructor(name, signature, branches, body) {
                    return None;
                }
                if is_constant_binding(name, signature, body, branches) {
                    return None;
                }
                let from_doc = first_doc_line(doc);
                let description = if is_useful_summary(&from_doc) {
                    from_doc
                } else {
                    let synthetic = auto_describe_function(name, signature, branches, body);
                    first_doc_line(&synthetic)
                };
                Some((name.clone(), description, proof_status.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Render the Function | Status | Description table.
pub(super) fn render_functions_table(
    functions: &[(String, String, Option<ProofStatus>)],
    link_prefix: &str,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "| Function | Status | Description |");
    let _ = writeln!(out, "|----------|--------|-------------|");
    for (name, description, status) in functions {
        let cell = escape_md_cell(description);
        let status_cell = proof_status_cell(status);
        let _ = writeln!(
            out,
            "| [{name}]({link_prefix}{name}.md) | {status_cell} | {cell} |"
        );
    }
    out.push('\n');
    out
}
