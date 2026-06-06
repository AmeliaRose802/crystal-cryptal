// Per-function detail pages under `functions/{name}.md`.

use std::fmt::Write as FmtWrite;
use std::fs;
use std::io;
use std::path::Path;

use crate::coverage::{function_banner, function_title_badge};
use crate::describe::auto_describe_function;
use crate::ir::Item;
use crate::linker::SymbolTable;

use super::RenderOptions;
use super::mermaid::render_flowchart_mermaid;
use super::proof::{
    proof_detail_line, render_failure_details_callout, render_proof_details_callout,
    render_verify_command_section,
};
use super::signature::{extract_param_names, parse_signature, render_structured_signature};
use super::util::{
    anchor_for, camel_to_spaced, is_simple_constructor, prefixed_file, render_doc_body,
};

pub(super) fn render_function_files(
    items: &[Item],
    symbols: &SymbolTable,
    output_dir: &Path,
    options: &RenderOptions,
    path_prefix: &str,
) -> io::Result<()> {
    for item in items {
        if let Item::Function {
            name,
            signature,
            branches,
            body,
            doc,
            proof_status,
            is_private,
        } = item
        {
            if !signature.contains("->") && branches.is_empty() {
                continue;
            }
            if is_simple_constructor(name, signature, branches, body) {
                continue;
            }

            let current_file = prefixed_file(path_prefix, &format!("functions/{name}.md"));
            let mut out = String::new();

            let badge = function_title_badge(options.ledger.as_ref(), name, proof_status);
            let private_badge = if *is_private { "`internal helper`" } else { "" };
            let badge_str = match (badge.is_empty(), private_badge.is_empty()) {
                (false, false) => format!("  {badge}  {private_badge}"),
                (false, true) => format!("  {badge}"),
                (true, false) => format!("  {private_badge}"),
                (true, true) => String::new(),
            };
            let _ = writeln!(out, "# `{name}`{badge_str}\n");

            if let Some(banner) = function_banner(options.ledger.as_ref(), name) {
                out.push_str(&banner);
            }

            let parsed_sig = parse_signature(signature);
            let param_names = extract_param_names(body, name);
            render_structured_signature(
                &mut out,
                &parsed_sig,
                &param_names,
                !options.no_details,
                |ty| symbols.resolve_links(ty, &current_file),
            );

            if !body.is_empty() {
                let _ = writeln!(out, "### Formal definition (Cryptol)\n");
                let _ = writeln!(out, "```haskell\n{body}\n```\n");
            }

            if let Some(callout) = render_proof_details_callout(proof_status) {
                out.push_str(&callout);
            }
            if let Some(detail) = proof_detail_line(proof_status) {
                let _ = writeln!(out, "> {detail}\n");
            }
            if let Some(callout) = render_failure_details_callout(proof_status) {
                out.push_str(&callout);
            }
            if let Some(section) = render_verify_command_section(proof_status) {
                out.push_str(&section);
            }

            let effective_doc = if doc.is_empty() {
                auto_describe_function(name, signature, branches, body)
            } else {
                doc.clone()
            };
            if !effective_doc.is_empty() {
                render_doc_body(&mut out, &effective_doc, |l| {
                    symbols.resolve_links(l, &current_file)
                });
            }

            if branches.len() > 1
                && let Some(chart) = render_flowchart_mermaid(name, branches)
            {
                out.push_str(&chart);
                out.push('\n');
            }

            if let Some(related) = symbols.related_properties.get(name) {
                let _ = writeln!(out, "### Related Properties");
                for (label, prop_name, cat_file) in related {
                    let display = camel_to_spaced(prop_name);
                    let _ = writeln!(
                        out,
                        "- [{label} — {display}](../{cat_file}#{anchor})",
                        anchor = anchor_for(label, prop_name),
                    );
                }
                out.push('\n');
            }

            let fn_path = output_dir.join("functions").join(format!("{name}.md"));
            fs::write(&fn_path, out)
                .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", fn_path.display())))?;
        }
    }
    Ok(())
}
