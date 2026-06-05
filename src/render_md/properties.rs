// Per-category property pages under `properties/{slug}.md`.

use std::fmt::Write as FmtWrite;
use std::fs;
use std::io;
use std::path::Path;

use crate::describe::auto_describe_property;
use crate::ir::Item;
use crate::linker::SymbolTable;

use super::equivalence::{
    find_involved_function_names, function_status_map, render_implementation_equivalence_callout,
};
use super::proof::{
    find_involved_symbols, intentional_counterexample_callout, is_intentional_counterexample,
    proof_badge, proof_detail_line, render_failure_details_callout,
    render_proof_details_callout, render_verify_command_section,
};
use super::util::{
    camel_to_spaced, category_slug_from_title, prefixed_file, strip_category_prefix,
};
use super::RenderOptions;

pub(super) fn render_property_files(
    items: &[Item],
    symbols: &SymbolTable,
    output_dir: &Path,
    options: &RenderOptions,
    path_prefix: &str,
) -> io::Result<()> {
    let mut category_items: Vec<(String, String, Vec<&Item>)> = Vec::new();
    let mut current_title = String::new();
    let mut current_slug = String::new();

    for item in items {
        if let Item::Section {
            level: 3, title, ..
        } = item
        {
            current_title = strip_category_prefix(title);
            current_slug = category_slug_from_title(title);
        }
        if let Item::Property { label, .. } = item {
            let slug = if current_slug.is_empty() {
                symbols
                    .property_categories
                    .get(label)
                    .cloned()
                    .unwrap_or_else(|| "misc".into())
            } else {
                current_slug.clone()
            };
            let title = if current_title.is_empty() {
                "Miscellaneous".into()
            } else {
                current_title.clone()
            };

            if let Some(entry) = category_items.iter_mut().find(|(_, s, _)| s == &slug) {
                entry.2.push(item);
            } else {
                category_items.push((title, slug, vec![item]));
            }
        }
    }

    for (cat_title, cat_slug, props) in &category_items {
        let current_file = prefixed_file(path_prefix, &format!("properties/{cat_slug}.md"));
        let mut out = String::new();

        let _ = writeln!(out, "# {cat_title}\n");

        let all_intentional_cex = !props.is_empty()
            && props.iter().all(|item| {
                matches!(item, Item::Property { doc, .. } if is_intentional_counterexample(doc))
            });

        if all_intentional_cex {
            let _ = writeln!(
                out,
                "> **How to read this page.** Every property below is a \
                 *deliberately false* claim about the protocol — a \
                 tempting-but-wrong intuition that the Cryptol prover \
                 refutes with a concrete counterexample. They are listed \
                 here as `✗` so a reader can see, side-by-side with the \
                 proven (`✓`) safety properties elsewhere, exactly which \
                 intuitions the implementation does **not** uphold and \
                 why. A `✗` on this page is the *intended* outcome — not \
                 a regression.\n"
            );
        } else {
            let _ = writeln!(
                out,
                "> **How to read these verdicts.** A property's ✓ means a \
                 solver discharged the logical claim against the **Cryptol \
                 model**. That guarantee carries over to the compiled \
                 implementation only when every function the property mentions \
                 *also* has a SAW equivalence proof — surfaced below each \
                 property as an **Implementation equivalence** callout. A \
                 green property over a partly-proven function set still tells \
                 you the design is sound; it does **not** by itself certify the \
                 binary.\n"
            );
        }
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

                if !doc.is_empty() {
                    let mut in_expected_verdict = false;
                    for line in doc {
                        let linked = symbols.resolve_links(line, &current_file);
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
                if let Some(section) = render_verify_command_section(proof_status) {
                    out.push_str(&section);
                }

                if !options.no_details && !body.is_empty() {
                    let _ = writeln!(
                        out,
                        "<details><summary>Formal property (Cryptol)</summary>\n"
                    );
                    let _ = writeln!(out, "```haskell\n{body}\n```\n");
                    let _ = writeln!(out, "</details>\n");
                }

                let involved = find_involved_symbols(body, doc, symbols, &current_file);

                if !intentional_cex {
                    let fn_status = function_status_map(items);
                    let involved_fn_names = find_involved_function_names(body, doc, &fn_status);
                    if let Some(callout) =
                        render_implementation_equivalence_callout(&involved_fn_names, &fn_status)
                    {
                        out.push_str(&callout);
                    }
                }

                if !involved.is_empty() {
                    let _ = writeln!(out, "**Involved:** {}\n", involved.join(", "));
                }
            }
        }

        let prop_path = output_dir.join("properties").join(format!("{cat_slug}.md"));
        fs::write(&prop_path, out)
            .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", prop_path.display())))?;
    }

    Ok(())
}
