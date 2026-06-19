// `render_single_file` — single-page rendering used for `--single-file` mode.

use std::fmt::Write as FmtWrite;

use crate::coverage::{function_banner, function_title_badge, is_coverage_directive_line};
use crate::describe::{auto_describe_function, auto_describe_property};
use crate::ir::Item;
use crate::linker::SymbolTable;

use super::RenderOptions;
use super::equivalence::{
    find_involved_function_names, function_status_map, render_implementation_equivalence_callout,
};
use super::mermaid::{render_coverage_map_mermaid, render_flowchart_mermaid};
use super::proof::{
    intentional_counterexample_callout, is_intentional_counterexample, proof_badge,
    proof_detail_line, render_failure_details_callout, render_proof_details_callout,
    render_verify_command_section,
};
use super::signature::{extract_param_names, parse_signature, render_structured_signature};
use super::util::{
    anchor_for, camel_to_spaced, describe_type, is_simple_constructor, render_doc_body,
    render_type_alias_width, sanitize_type_doc, strip_category_prefix,
};

pub fn render_single_file(
    items: &[Item],
    symbols: &SymbolTable,
    options: &RenderOptions,
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

    let has_types = items.iter().any(|i| {
        matches!(
            i,
            Item::TypeAlias { .. } | Item::EnumGroup { .. } | Item::RecordType { .. }
        )
    });

    if has_types {
        let _ = writeln!(out, "## Types\n");

        for item in items {
            match item {
                Item::TypeAlias { name, width, doc } => {
                    let _ = writeln!(out, "### {name}");
                    render_type_alias_width(&mut out, width);
                    if let Some(clean_doc) = sanitize_type_doc(doc) {
                        let _ = writeln!(out, "{clean_doc}");
                    }
                    out.push('\n');
                }
                Item::EnumGroup {
                    type_name,
                    width,
                    variants,
                    doc,
                    ..
                } => {
                    let _ = writeln!(out, "### {type_name}");
                    if let Some(clean_doc) = sanitize_type_doc(doc) {
                        let _ = writeln!(out, "{clean_doc}\n");
                    } else {
                        let n = variants.len();
                        let _ = writeln!(out, "A {width}-bit enumeration with {n} variants.\n");
                    }
                    let _ = writeln!(out, "| Name | Value | Description |");
                    let _ = writeln!(out, "|------|-------|-------------|");
                    for v in variants {
                        let _ = writeln!(out, "| `{}` | {} | |", v.name, v.value);
                    }
                    out.push('\n');
                }
                Item::RecordType { name, fields, doc } => {
                    let _ = writeln!(out, "### {name}");
                    if let Some(clean_doc) = sanitize_type_doc(doc) {
                        let _ = writeln!(out, "{clean_doc}\n");
                    }
                    out.push('\n');
                    let _ = writeln!(out, "| Field | Type | Description |");
                    let _ = writeln!(out, "|-------|------|-------------|");
                    for (fname, ftype) in fields {
                        let linked_type = symbols.resolve_links_single_file(ftype);
                        let desc = describe_type(ftype);
                        let _ = writeln!(out, "| `{fname}` | {linked_type} | {desc} |");
                    }
                    out.push('\n');
                }
                _ => {}
            }
        }
    }

    let functions: Vec<_> = items
        .iter()
        .filter(|item| match item {
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
        })
        .collect();

    if !functions.is_empty() {
        let _ = writeln!(out, "## Functions\n");

        for item in &functions {
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
                let badge = function_title_badge(options.ledger.as_ref(), name, proof_status);
                let private_badge = if *is_private { "`internal helper`" } else { "" };
                let badge_str = match (badge.is_empty(), private_badge.is_empty()) {
                    (false, false) => format!("  {badge}  {private_badge}"),
                    (false, true) => format!("  {badge}"),
                    (true, false) => format!("  {private_badge}"),
                    (true, true) => String::new(),
                };
                let _ = writeln!(out, "### `{name}`{badge_str}\n");

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
                    |ty| symbols.resolve_links_single_file(ty),
                );

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

                let visible_doc: Vec<String> = doc
                    .iter()
                    .filter(|l| !is_coverage_directive_line(l))
                    .cloned()
                    .collect();
                let effective_doc = if visible_doc.is_empty() {
                    auto_describe_function(name, signature, branches, body)
                } else {
                    visible_doc
                };
                if !effective_doc.is_empty() {
                    render_doc_body(&mut out, &effective_doc, |l| {
                        symbols.resolve_links_single_file(l)
                    });
                }

                if branches.len() > 1
                    && let Some(chart) = render_flowchart_mermaid(name, branches)
                {
                    out.push_str(&chart);
                    out.push('\n');
                }

                if let Some(related) = symbols.related_properties.get(name) {
                    let _ = writeln!(out, "#### Related Properties");
                    for (label, prop_name, _) in related {
                        let display = camel_to_spaced(prop_name);
                        let anchor = anchor_for(label, prop_name);
                        let _ = writeln!(out, "- [{label} — {display}](#{anchor})");
                    }
                    out.push('\n');
                }

                if !options.no_details && !body.is_empty() {
                    let _ = writeln!(
                        out,
                        "<details><summary>Formal definition (Cryptol)</summary>\n"
                    );
                    let _ = writeln!(out, "```haskell\n{body}\n```\n");
                    let _ = writeln!(out, "</details>\n");
                }
            }
        }
    }

    let mut category_items: Vec<(String, Vec<&Item>)> = Vec::new();
    let mut current_title = String::new();

    for item in items {
        if let Item::Section {
            level: 3, title, ..
        } = item
        {
            current_title = strip_category_prefix(title);
        }
        if matches!(item, Item::Property { .. }) {
            let title = if current_title.is_empty() {
                "Miscellaneous".into()
            } else {
                current_title.clone()
            };
            if let Some(entry) = category_items.iter_mut().find(|(t, _)| t == &title) {
                entry.1.push(item);
            } else {
                category_items.push((title, vec![item]));
            }
        }
    }

    for (cat_title, props) in &category_items {
        let _ = writeln!(out, "## {cat_title}\n");

        for prop_item in props {
            if let Item::Property {
                label,
                name,
                params,
                body,
                doc,
                proof_status,
                ..
            } = prop_item
            {
                let display_name = camel_to_spaced(name);
                let intentional_cex = is_intentional_counterexample(doc);
                let icon = if intentional_cex && proof_badge(proof_status).is_empty() {
                    "✗".to_string()
                } else {
                    proof_badge(proof_status)
                };
                let icon_prefix = if icon.is_empty() {
                    String::new()
                } else {
                    format!("{icon} ")
                };
                let _ = writeln!(out, "### {icon_prefix}{label} — {display_name}\n");

                if intentional_cex {
                    out.push_str(&intentional_counterexample_callout());
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

                if !doc.is_empty() {
                    let mut in_expected_verdict = false;
                    for line in doc {
                        let linked = symbols.resolve_links_single_file(line);
                        if linked.contains("EXPECTED VERDICT") {
                            in_expected_verdict = true;
                            let _ = writeln!(out, "> **Note:** {linked}");
                        } else if in_expected_verdict {
                            if linked.trim().is_empty() {
                                in_expected_verdict = false;
                                out.push('\n');
                            } else {
                                let _ = writeln!(out, "> {linked}");
                            }
                        } else {
                            let _ = writeln!(out, "{linked}");
                        }
                    }
                    out.push('\n');
                } else if let Some(desc) = auto_describe_property(name, params, body) {
                    let _ = writeln!(out, "{desc}\n");
                }

                if let Some(callout) = render_proof_details_callout(proof_status) {
                    out.push_str(&callout);
                }

                if !options.no_details && !body.is_empty() {
                    let _ = writeln!(
                        out,
                        "<details><summary>Formal property (Cryptol)</summary>\n"
                    );
                    let _ = writeln!(out, "```haskell\n{body}\n```\n");
                    let _ = writeln!(out, "</details>\n");
                }

                if !intentional_cex {
                    let fn_status = function_status_map(items);
                    let involved_fn_names = find_involved_function_names(body, doc, &fn_status);
                    if let Some(callout) =
                        render_implementation_equivalence_callout(&involved_fn_names, &fn_status)
                    {
                        out.push_str(&callout);
                    }
                }
            }
        }
    }

    let all_fn_names: Vec<String> = items
        .iter()
        .filter_map(|item| match item {
            Item::Function {
                name,
                signature,
                branches,
                body,
                ..
            } => {
                if !signature.contains("->") && branches.is_empty() {
                    return None;
                }
                if is_simple_constructor(name, signature, branches, body) {
                    return None;
                }
                Some(name.clone())
            }
            _ => None,
        })
        .collect();
    out.push_str(&render_coverage_map_mermaid(symbols, &all_fn_names));

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_items() -> Vec<Item> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("SDEP.cry");
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("SDEP.cry not found at {}: {e}", path.display()));
        crate::parser::parse(&src)
    }

    #[test]
    fn render_single_file_has_all_sections() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let options = RenderOptions {
            no_details: false,
            title_override: None,
            docfx: false,
            ledger: None,
        };
        let doc = render_single_file(&items, &symbols, &options);
        assert!(doc.contains("# SDEP"), "should contain module title");
        assert!(doc.contains("## Types"), "should contain Types section");
        assert!(
            doc.contains("## Functions"),
            "should contain Functions section"
        );
        assert!(
            doc.contains("### FleetMode"),
            "should contain FleetMode type"
        );
        assert!(
            doc.contains("### `provisionKey`"),
            "should contain provisionKey function"
        );
    }

    #[test]
    fn render_single_file_uses_anchor_links() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let options = RenderOptions {
            no_details: false,
            title_override: None,
            docfx: false,
            ledger: None,
        };
        let doc = render_single_file(&items, &symbols, &options);
        assert!(
            !doc.contains("](types.md"),
            "single-file should not contain types.md links"
        );
        assert!(
            !doc.contains("](functions/"),
            "single-file should not contain functions/ links"
        );
        assert!(
            !doc.contains("](properties/"),
            "single-file should not contain properties/ links"
        );
        assert!(
            doc.contains("](#"),
            "single-file should contain anchor-only links"
        );
    }

    #[test]
    fn render_single_file_no_details() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let options = RenderOptions {
            no_details: true,
            title_override: None,
            docfx: false,
            ledger: None,
        };
        let doc = render_single_file(&items, &symbols, &options);
        assert!(
            !doc.contains("<details>"),
            "no_details should suppress detail folds in single-file"
        );
    }
}
