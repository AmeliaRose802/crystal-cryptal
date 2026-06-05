// Markdown renderer: emits .md files from linked IR.

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io;
use std::path::Path;

use convert_case::{Case, Casing};

use crate::describe::{auto_describe_function, auto_describe_property};
use crate::ir::{Branch, Item, ProofStatus};
use crate::linker::{SymbolTable, sanitize_slug};

const TYPE_DOC_INTERNAL_MARKERS: &[&str] = &[
    "counterexample",
    "scope of this proof",
    "bounded model checking",
    "this was p",
    "z3",
    "sat",
    "fix.",
    "extends to arbitrary",
    "production c++",
    "theorem prover",
    "dafny",
    "lean",
    "coq",
    "future",
    "purely additive",
    "~1s/property",
    "not injective",
    "smuggled",
    "structural argument",
    "we add a structured layer",
    "existing properties",
    "new properties",
];

pub struct RenderOptions {
    pub no_details: bool,
    pub title_override: Option<String>,
    /// Emit DocFX-compatible front-matter and toc.yml files.
    pub docfx: bool,
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

    let mut index = render_index(items, symbols, options, path_prefix);
    if options.docfx {
        index = format!("{}{}", docfx_frontmatter(&module_name, &title), index);
    }
    fs::write(output_dir.join("index.md"), index)?;

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

/// Render all items into a single Markdown document.
pub fn render_single_file(
    items: &[Item],
    symbols: &SymbolTable,
    options: &RenderOptions,
) -> String {
    let mut out = String::new();

    // ── Title & module doc ──────────────────────────────────────────────
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

    // ── Types ───────────────────────────────────────────────────────────
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

    // ── Functions ────────────────────────────────────────────────────────
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
                let badge = proof_badge(proof_status);
                let private_badge = if *is_private { "`internal helper`" } else { "" };
                let badge_str = match (badge.is_empty(), private_badge.is_empty()) {
                    (false, false) => format!("  {badge}  {private_badge}"),
                    (false, true) => format!("  {badge}"),
                    (true, false) => format!("  {private_badge}"),
                    (true, true) => String::new(),
                };
                let _ = writeln!(out, "### `{name}`{badge_str}\n");

                let parsed_sig = parse_signature(signature);
                let param_names = extract_param_names(body, name);
                render_structured_signature(
                    &mut out,
                    &parsed_sig,
                    &param_names,
                    !options.no_details,
                    |ty| symbols.resolve_links_single_file(ty),
                );

                // Proof-details callout (overrides + bounded-loop iterations)
                if let Some(callout) = render_proof_details_callout(proof_status) {
                    out.push_str(&callout);
                }
                // Failure callout (reason + counterexample/log fold).
                if let Some(detail) = proof_detail_line(proof_status) {
                    let _ = writeln!(out, "> {detail}\n");
                }
                if let Some(callout) = render_failure_details_callout(proof_status) {
                    out.push_str(&callout);
                }
                // "Verify this yourself" rerun command from the manifest.
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
                        symbols.resolve_links_single_file(l)
                    });
                }

                if branches.len() > 1 {
                    // Flowchart only — the prior decision table duplicated
                    // the same information one scroll above the diagram.
                    if let Some(chart) = render_flowchart_mermaid(name, branches) {
                        out.push_str(&chart);
                        out.push('\n');
                    }
                }
                // Single-branch result is shown only inside the formal definition accordion

                // Related properties
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

    // ── Properties by category ──────────────────────────────────────────
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

                // Surface "intentionally disproven" status above any other
                // verification callouts so it cannot be mistaken for a
                // proven guarantee when skimming.
                if intentional_cex {
                    out.push_str(&intentional_counterexample_callout());
                }

                // Surface verification context (failure reason, "not yet verified", etc.)
                // immediately below the heading rather than baking it into the title.
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

                // Proof-details callout (overrides + bounded-loop iterations)
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

                // Transitive implementation-equivalence callout.
                //
                // Suppressed for intentional counterexamples: a "✓
                // Implementation equivalence proven" line next to a
                // deliberately-false property reads as "this claim is
                // safe", which is exactly the wrong takeaway. The
                // counterexample callout above already states the right
                // thing — the implementation refutes the claim too.
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

    // Coverage map
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

// ── index.md ────────────────────────────────────────────────────────────────

fn render_index(
    items: &[Item],
    symbols: &SymbolTable,
    options: &RenderOptions,
    _path_prefix: &str,
) -> String {
    let mut out = String::new();

    // Title from Module or override.
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

    // Two-layer-trust explainer. Anchored on the home page so newcomers
    // immediately see what proof verdicts in this site do (and do not)
    // mean before they start clicking around. Kept short on purpose —
    // each property page restates the same idea in context.
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

    // Module parameters section
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

    // Types section
    let _ = writeln!(out, "## Types\n");
    let _ = writeln!(out, "All type definitions: [types.md](types.md)\n");

    // Functions link
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
        // Embed the function summary table directly on the home page so
        // readers see the per-function SAW-equivalence status without an
        // extra navigation hop.  The dedicated `functions/index.md` page
        // still exists for TOC navigation and links back here.
        let fns = collect_functions_for_index(items);
        if !fns.is_empty() {
            out.push_str(&render_functions_table(&fns, "functions/"));
        }
        let _ = writeln!(
            out,
            "Per-function detail pages: [functions](functions/index.md)\n"
        );
    }

    // Properties by category
    let categories = collect_categories(items, symbols);
    if !categories.is_empty() {
        // Build a label → (ProofStatus, body, doc) lookup so each category
        // row can show an aggregate verification badge alongside the
        // property range AND flag properties that rely on functions
        // lacking a SAW equivalence proof.
        let prop_info: std::collections::HashMap<&str, (&Option<ProofStatus>, &str, &[String])> =
            items
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

    // Proof status summary
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

// ── functions/index.md ───────────────────────────────────────────────────────

fn render_functions_index(
    items: &[Item],
    _symbols: &SymbolTable,
    _options: &RenderOptions,
    _path_prefix: &str,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Functions\n");

    // Quick orientation: clarify what the proof badges next to each function
    // mean, so readers don't confuse "function proven" (SAW equivalence
    // against the implementation) with "property proven" (Cryptol logic-
    // level argument).
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
        // The full Function | Status | Description table now lives on the
        // top-level [index.md](../index.md#functions) so the home page
        // surfaces verification status at a glance.  This page keeps a
        // lightweight bullet list as a navigation aid into the per-function
        // detail pages without duplicating the home-page table.
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
///
/// Both `functions/index.md` (when we still emit a table there) and the
/// top-level `index.md` page need the same filtered, summarised list of
/// functions. Centralising the logic here keeps the two views in sync so
/// readers don't see different sets of functions depending on which entry
/// point they used.
fn collect_functions_for_index(items: &[Item]) -> Vec<(String, String, Option<ProofStatus>)> {
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
                // Prefer the hand-written doc, but if its lead paragraph is
                // just a section header (e.g. "C++ body:" followed by a code
                // block), fall back to the auto-generated summary so the
                // Description column never displays a dangling header label.
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
///
/// `link_prefix` is prepended to each function name so the same table can
/// be embedded at different depths in the doc tree (empty string when the
/// caller is already inside `functions/`, `"functions/"` when called from
/// the top-level `index.md`).
fn render_functions_table(
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

/// One-cell summary of a function's SAW-equivalence proof state for the
/// Functions index table. Mirrors `proof_badge`. When the manifest has no
/// entry for this function we render an em dash so the column reads
/// uniformly rather than leaving a visually inconsistent blank gap next
/// to populated rows — Cryptol-only helpers and predicates legitimately
/// have no implementation to verify against, and `—` communicates that
/// more clearly than an empty cell.
fn proof_status_cell(status: &Option<ProofStatus>) -> String {
    match status {
        Some(ProofStatus::Proven { .. }) => "✓ proven".into(),
        Some(ProofStatus::Assumed) => "~ assumed".into(),
        Some(ProofStatus::Failed { .. }) => "✗ failed".into(),
        Some(ProofStatus::NotAttempted) => "✗ not attempted".into(),
        None => "—".into(),
    }
}

/// Whether a candidate first-line summary is presentable in a table cell.
/// Rejects empty strings and bare section headers (a short line ending in
/// `:` like "C++ body:") that would otherwise leave the Description column
/// looking truncated.
fn is_useful_summary(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.ends_with(':') {
        return false;
    }
    true
}

// ── types.md ────────────────────────────────────────────────────────────────

fn render_types(items: &[Item], symbols: &SymbolTable, path_prefix: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Types\n");

    // Build type → functions back-links by scanning function signatures and bodies.
    let type_names: Vec<String> = items
        .iter()
        .filter_map(|item| match item {
            Item::TypeAlias { name, .. } => Some(name.clone()),
            Item::EnumGroup { type_name, .. } => Some(type_name.clone()),
            Item::RecordType { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();

    let functions: Vec<(String, String)> = items
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
                Some((name.clone(), signature.clone()))
            }
            _ => None,
        })
        .collect();

    use std::collections::HashMap as StdHashMap;
    let mut type_to_fns: StdHashMap<String, Vec<String>> = StdHashMap::new();
    for (fn_name, sig) in &functions {
        for type_name in &type_names {
            if crate::linker::contains_word(sig, type_name) {
                type_to_fns
                    .entry(type_name.clone())
                    .or_default()
                    .push(fn_name.clone());
            }
        }
    }

    for item in items {
        match item {
            Item::TypeAlias { name, width, doc } => {
                let _ = writeln!(out, "### {name}");
                render_type_alias_width(&mut out, width);
                if let Some(clean_doc) = sanitize_type_doc(doc) {
                    let _ = writeln!(out, "{clean_doc}");
                }
                out.push('\n');
                if let Some(fns) = type_to_fns.get(name) {
                    let links: Vec<String> = fns
                        .iter()
                        .map(|f| format!("[`{f}`](functions/{f}.md)"))
                        .collect();
                    let _ = writeln!(out, "Used by: {}\n", links.join(", "));
                }
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
                if let Some(fns) = type_to_fns.get(type_name) {
                    let links: Vec<String> = fns
                        .iter()
                        .map(|f| format!("[`{f}`](functions/{f}.md)"))
                        .collect();
                    let _ = writeln!(out, "Used by: {}\n", links.join(", "));
                }
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
                    let current_file = prefixed_file(path_prefix, "types.md");
                    let linked_type = symbols.resolve_links(ftype, &current_file);
                    let desc = describe_type(ftype);
                    let _ = writeln!(out, "| `{fname}` | {linked_type} | {desc} |");
                }
                out.push('\n');
                if let Some(fns) = type_to_fns.get(name) {
                    let links: Vec<String> = fns
                        .iter()
                        .map(|f| format!("[`{f}`](functions/{f}.md)"))
                        .collect();
                    let _ = writeln!(out, "Used by: {}\n", links.join(", "));
                }
            }
            _ => {}
        }
    }

    out
}

// ── functions/{name}.md ─────────────────────────────────────────────────────

fn render_function_files(
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
            // Skip enum constants that were absorbed but left as functions
            if !signature.contains("->") && branches.is_empty() {
                continue;
            }
            if is_simple_constructor(name, signature, branches, body) {
                continue;
            }

            let current_file = prefixed_file(path_prefix, &format!("functions/{name}.md"));
            let mut out = String::new();

            let badge = proof_badge(proof_status);
            let private_badge = if *is_private { "`internal helper`" } else { "" };
            let badge_str = match (badge.is_empty(), private_badge.is_empty()) {
                (false, false) => format!("  {badge}  {private_badge}"),
                (false, true) => format!("  {badge}"),
                (true, false) => format!("  {private_badge}"),
                (true, true) => String::new(),
            };
            let _ = writeln!(out, "# `{name}`{badge_str}\n");

            // Structured signature with cross-linked types.
            let parsed_sig = parse_signature(signature);
            let param_names = extract_param_names(body, name);
            render_structured_signature(
                &mut out,
                &parsed_sig,
                &param_names,
                !options.no_details,
                |ty| symbols.resolve_links(ty, &current_file),
            );

            // Formal Cryptol definition — shown right after the signature so
            // readers see the actual code before any prose. Rendered inline
            // (not in a <details> fold) at the user's request. Heading is
            // H3 so it sits *below* the page title (H1) and the implicit
            // structured-signature section without competing for top-level
            // attention in the right-rail outline.
            if !body.is_empty() {
                let _ = writeln!(out, "### Formal definition (Cryptol)\n");
                let _ = writeln!(out, "```haskell\n{body}\n```\n");
            }

            // Proof-details callout (overrides + bounded-loop iterations)
            // when the manifest carries that metadata.  Placed right after
            // the Cryptol body so the trust caveats sit next to the code
            // they apply to, before the doc/flowchart consumes attention.
            if let Some(callout) = render_proof_details_callout(proof_status) {
                out.push_str(&callout);
            }
            // For failed / not-yet-verified / assumed functions, surface the
            // short reason directly under the signature and, when the
            // manifest carries solver diagnostics, drop the counterexample
            // and log excerpt into folds below it.
            if let Some(detail) = proof_detail_line(proof_status) {
                let _ = writeln!(out, "> {detail}\n");
            }
            if let Some(callout) = render_failure_details_callout(proof_status) {
                out.push_str(&callout);
            }
            // "Verify this yourself" — copy-pasteable rerun command from
            // the manifest.  Rendered immediately after the failure callout
            // so a frustrated reader's eye lands on an actionable next step.
            if let Some(section) = render_verify_command_section(proof_status) {
                out.push_str(&section);
            }

            // Doc comment (auto-generated when absent)
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

            // Branches: render flowchart only. The decision table
            // duplicated the same information one scroll above the
            // diagram, so per UX feedback we keep just the chart.
            if branches.len() > 1
                && let Some(chart) = render_flowchart_mermaid(name, branches)
            {
                out.push_str(&chart);
                out.push('\n');
            }
            // Single-branch result is shown only inside the formal definition accordion

            // Related properties
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

            // (Formal Cryptol definition is rendered inline above, right
            //  after the signature — no <details> fold.)

            let fn_path = output_dir.join("functions").join(format!("{name}.md"));
            fs::write(&fn_path, out)
                .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", fn_path.display())))?;
        }
    }
    Ok(())
}

// ── properties/{category}.md ────────────────────────────────────────────────

fn render_property_files(
    items: &[Item],
    symbols: &SymbolTable,
    output_dir: &Path,
    options: &RenderOptions,
    path_prefix: &str,
) -> io::Result<()> {
    // Group properties by category.
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

        // Detect "all properties on this page are intentional
        // counterexamples" — happens for the "Intentional counterexamples"
        // category (and any future page authored the same way). In that
        // case the standard "✓ means proved" preamble would mislead,
        // because every entry on the page is *deliberately* refuted.
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
            // Two-layer trust reminder. Each property's `✓` is a *Cryptol-level*
            // verdict; for that verdict to apply to the production C++/Rust
            // code, every function the property mentions must additionally have
            // a SAW equivalence proof. The per-property *Implementation
            // equivalence* callouts below summarise that transitive coverage.
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

                // Surface "intentionally disproven" status above any other
                // verification callouts so it cannot be mistaken for a
                // proven guarantee when skimming.
                if intentional_cex {
                    out.push_str(&intentional_counterexample_callout());
                }

                if let Some(detail) = proof_detail_line(proof_status) {
                    let _ = writeln!(out, "> {detail}\n");
                }
                if let Some(callout) = render_failure_details_callout(proof_status) {
                    out.push_str(&callout);
                }

                // Doc lines
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

                // Proof-details callout (overrides + bounded-loop iterations)
                // when the manifest carries that metadata for this property.
                if let Some(callout) = render_proof_details_callout(proof_status) {
                    out.push_str(&callout);
                }
                // "Verify this yourself" rerun command from the manifest.
                if let Some(section) = render_verify_command_section(proof_status) {
                    out.push_str(&section);
                }

                // Formal-property fold first — the code itself is the most
                // information-dense thing on the page, so put it where the
                // eye lands after the heading + prose, with the "Involved"
                // cross-reference list as a follow-on aid.
                if !options.no_details && !body.is_empty() {
                    let _ = writeln!(
                        out,
                        "<details><summary>Formal property (Cryptol)</summary>\n"
                    );
                    // Use `haskell` here — highlight.js (docfx's bundled
                    // highlighter) doesn't recognize a `cryptol` tag, but
                    // Cryptol's surface syntax (`module`, `where`, `if /
                    // then / else`, `==>`, `--` comments, numeric literals)
                    // overlaps closely enough with Haskell that the Haskell
                    // grammar produces a perfectly reasonable colorization.
                    let _ = writeln!(out, "```haskell\n{body}\n```\n");
                    let _ = writeln!(out, "</details>\n");
                }

                // Involved functions and types
                let involved = find_involved_symbols(body, doc, symbols, &current_file);

                // Transitive implementation-equivalence callout.
                // Computed from the same body+doc text so it never goes out
                // of sync with the "Involved:" link list rendered below.
                //
                // Suppressed for intentional counterexamples: a "✓
                // Implementation equivalence proven" line next to a
                // deliberately-false property reads as "this claim is
                // safe", which is exactly the wrong takeaway. The
                // counterexample callout above already states the right
                // thing — the implementation refutes the claim too.
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

// ── DocFX helpers ────────────────────────────────────────────────────────────

/// Emit a DocFX YAML front-matter block for a Markdown page.
fn docfx_frontmatter(uid: &str, title: &str) -> String {
    format!("---\nuid: {uid}\ntitle: {title}\n---\n\n")
}

/// Emit a `toc.yml` for a module's output directory.
fn render_docfx_toc(title: &str, items: &[Item]) -> String {
    let has_functions = items.iter().any(|i| matches!(i, Item::Function { .. }));
    let has_types = items.iter().any(|i| {
        matches!(
            i,
            Item::TypeAlias { .. } | Item::EnumGroup { .. } | Item::RecordType { .. }
        )
    });

    // Collect property categories (slug, title) in order.
    let mut cats: Vec<(String, String)> = Vec::new();
    let mut cur_title = String::new();
    let mut cur_slug = String::new();
    for item in items {
        if let Item::Section {
            level: 3,
            title: sec_title,
            ..
        } = item
        {
            cur_title = strip_category_prefix(sec_title);
            cur_slug = category_slug_from_title(sec_title);
        }
        if let Item::Property { label, .. } = item {
            let slug = if cur_slug.is_empty() {
                "misc".to_string()
            } else {
                cur_slug.clone()
            };
            let ttl = if cur_title.is_empty() {
                "Miscellaneous".to_string()
            } else {
                cur_title.clone()
            };
            if !cats.iter().any(|(s, _)| s == &slug) {
                cats.push((slug, ttl));
            }
            let _ = label; // suppress unused warning
        }
    }

    let mut out = String::new();
    out.push_str(&format!("- name: {title}\n  href: index.md\n"));
    if has_types {
        out.push_str("- name: Types\n  href: types.md\n");
    }
    if has_functions {
        out.push_str("- name: Functions\n  href: functions/index.md\n");
    }
    if !cats.is_empty() {
        out.push_str("- name: Properties\n  items:\n");
        for (slug, cat_title) in &cats {
            out.push_str(&format!(
                "  - name: {cat_title}\n    href: properties/{slug}.md\n"
            ));
        }
    }
    out
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn first_doc_line(doc: &[String]) -> String {
    let mut parts = Vec::new();
    for line in doc {
        // A blank line ends the lead paragraph.
        if line.trim().is_empty() {
            break;
        }
        // An indented line marks the start of a code/layout block.
        // Stop before it so the description column never spills the block
        // contents in a flattened, comma-less wall.
        if line.starts_with("  ") || line.starts_with('\t') {
            break;
        }
        let trimmed = line.trim();
        parts.push(trimmed);
        if trimmed.ends_with('.') || trimmed.ends_with('!') || trimmed.ends_with('?') {
            break;
        }
    }
    parts.join(" ")
}

/// Escape `|` characters and collapse newlines so a string can be embedded
/// in a single Markdown table cell without spilling into extra columns or
/// breaking the row.
fn escape_md_cell(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '|' => out.push_str("\\|"),
            '\n' | '\r' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

/// Build a short noun-phrase describing a Cryptol type so the
/// Description column of record/struct field tables is never empty.
///
/// Recognizes the common shapes that appear in real specs:
///   `Bit`            → "Boolean flag"
///   `[N]`            → "`N`-bit value"      (N may be a literal or named type)
///   `[N][8]`         → "Buffer of `N` bytes"
///   `[N][M]`         → "Array of `N` `M`-bit values"
///   `[N]TypeName`    → "Array of `N` `TypeName` values"
///   `TypeName`       → "`TypeName` value"
///   `A -> B`         → ""  (function types are self-documenting)
fn describe_type(ty: &str) -> String {
    let t = ty.trim();
    if t.is_empty() {
        return String::new();
    }
    // Don't try to describe function types — they're already readable.
    if t.contains("->") {
        return String::new();
    }
    if t == "Bit" {
        return "Boolean flag".into();
    }
    // [N] ...
    if let Some(rest) = t.strip_prefix('[')
        && let Some(close) = rest.find(']')
    {
        let count = rest[..close].trim();
        let inner = rest[close + 1..].trim();
        if inner.is_empty() {
            return format!("`{count}`-bit value");
        }
        if inner == "[8]" {
            return format!("Buffer of `{count}` bytes");
        }
        if let Some(inner_rest) = inner.strip_prefix('[')
            && let Some(inner_close) = inner_rest.find(']')
            && inner_rest[inner_close + 1..].trim().is_empty()
        {
            let width = inner_rest[..inner_close].trim();
            return format!("Array of `{count}` `{width}`-bit values");
        }
        return format!("Array of `{count}` `{inner}` values");
    }
    // Capitalized single identifier → "`Foo` value"
    if t.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return format!("`{t}` value");
    }
    String::new()
}

fn prefixed_file(prefix: &str, file: &str) -> String {
    if prefix.is_empty() {
        file.to_string()
    } else {
        format!("{}/{}", prefix.trim_matches('/'), file)
    }
}

fn clean_type_width(width: &str) -> String {
    width
        .split_once("//")
        .map(|(head, _)| head)
        .unwrap_or(width)
        .trim()
        .to_string()
}

/// Emit the underlying-type line for a `type Foo = …` alias.
///
/// The parser strips the surrounding `[...]` brackets off the RHS, so what
/// we get back here is e.g. `"256"` for `type HmacKey = [256]`.  We
/// re-bracket and look up a friendly noun-phrase via `describe_type`, then
/// emit a single labeled line so the page reads as
///
/// ```text
/// **Type:** `[256]` — `256`-bit value
/// ```
///
/// instead of an orphan magenta `256` floating under the heading.
fn render_type_alias_width(out: &mut String, width: &str) {
    let clean = clean_type_width(width);
    if clean.is_empty() {
        return;
    }
    let bracketed = if clean.starts_with('[') {
        clean.clone()
    } else {
        format!("[{clean}]")
    };
    let friendly = describe_type(&bracketed);
    if friendly.is_empty() {
        let _ = writeln!(out, "**Type:** `{bracketed}`\n");
    } else {
        let _ = writeln!(out, "**Type:** `{bracketed}` — {friendly}\n");
    }
}

fn sanitize_type_doc(doc: &[String]) -> Option<String> {
    let joined = doc
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if joined.is_empty() {
        return None;
    }

    let mut kept = Vec::new();
    for sentence in split_sentences(&joined) {
        let cleaned = sentence
            .split_once("//")
            .map(|(head, _)| head)
            .unwrap_or(sentence)
            .trim();
        if cleaned.is_empty() {
            continue;
        }
        let lower = cleaned.to_lowercase();
        if TYPE_DOC_INTERNAL_MARKERS
            .iter()
            .any(|marker| lower.contains(marker))
        {
            continue;
        }
        if lower.contains("prove") {
            kept.push("Bounded check over the configured finite model.".to_string());
            continue;
        }
        if cleaned.len() > 200 {
            continue;
        }
        let finalized = if cleaned.ends_with('.') {
            cleaned.to_string()
        } else {
            format!("{cleaned}.")
        };
        kept.push(finalized);
        if kept.len() >= 2 {
            break;
        }
    }

    if kept.is_empty() {
        None
    } else {
        Some(kept.join(" "))
    }
}

fn split_sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for part in text.split(". ") {
        let trimmed = part.trim();
        if !trimmed.is_empty() {
            out.push(trimmed);
        }
    }
    if out.is_empty() {
        out.push(text.trim());
    }
    out
}

/// Split CamelCase into spaced words: "KeyMonotonicity" → "Key Monotonicity"
fn camel_to_spaced(name: &str) -> String {
    name.to_case(Case::Title)
}

/// Find functions and types referenced in a property body/doc, returning markdown links.
fn find_involved_symbols(
    body: &str,
    doc: &[String],
    symbols: &SymbolTable,
    current_file: &str,
) -> Vec<String> {
    use crate::linker::contains_word;

    let all_text = format!("{body} {}", doc.join(" "));
    let mut involved: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // Sort by name length descending to match longer names first
    let mut syms: Vec<(&String, &(String, String))> = symbols.symbols.iter().collect();
    syms.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));

    for (name, (target_file, anchor)) in &syms {
        // Skip self-references and property labels (P1, P2, etc.)
        if current_file == *target_file {
            continue;
        }
        if name.len() <= 3 && name.starts_with('P') && name[1..].chars().all(|c| c.is_ascii_digit())
        {
            continue;
        }
        if contains_word(&all_text, name) && seen.insert((*name).clone()) {
            let rel = SymbolTable::relative_path(current_file, target_file);
            let link = if anchor.is_empty() {
                format!("[`{name}`]({rel})")
            } else {
                format!("[`{name}`]({rel}#{anchor})")
            };
            involved.push(link);
        }
    }
    involved.sort();
    involved
}

/// A single-character verification marker for use at the start of a heading
/// or inline next to a name.  Kept intentionally minimal so the rendered
/// docs scan cleanly instead of feeling like AI-generated bullet soup.
///
/// Verdicts:
///   `✓` proven   |  `✗` failed or not yet verified   |  `~` assumed   |  empty
fn proof_badge(status: &Option<ProofStatus>) -> String {
    match status {
        Some(ProofStatus::Proven { .. }) => "✓".into(),
        Some(ProofStatus::Failed { .. }) => "✗".into(),
        Some(ProofStatus::Assumed) => "~".into(),
        Some(ProofStatus::NotAttempted) => "✗".into(),
        None => String::new(),
    }
}

/// Detect a property whose doc-comment declares it as an intentional
/// counterexample (`EXPECTED VERDICT: FAILS`). Such properties encode
/// *deliberately false* claims about the protocol — the Cryptol prover
/// is expected to refute each one with a concrete counterexample, and
/// the rendered docs must surface that disproof unambiguously so the
/// page cannot be misread as a proven safety guarantee.
///
/// We detect it from the doc text rather than the proof manifest because
/// the Cryptol-property prover log is not always wired into pretty-specs's
/// `--proof-status` input; the spec author's `EXPECTED VERDICT: FAILS`
/// comment is the authoritative source-level signal.
fn is_intentional_counterexample(doc: &[String]) -> bool {
    doc.iter()
        .any(|line| line.contains("EXPECTED VERDICT: FAILS"))
}

/// Loud, unambiguous callout placed immediately below the heading of an
/// intentional-counterexample property. Renders as a blockquote so it sits
/// visually above the `EXPECTED VERDICT` note that follows from the doc
/// text, and uses bold `✗` markers on both ends so it can never be skimmed
/// past as a "✓ proven" guarantee.
fn intentional_counterexample_callout() -> String {
    "> **✗ Intentionally disproven.** This property is a *deliberately \
     false* claim about the protocol. The Cryptol prover refutes it with \
     a concrete counterexample (see the **Note** below); the property \
     exists to make the failure mode visible to readers and is **not** a \
     safety guarantee of the implementation.\n\n"
        .to_string()
}

/// Long-form verdict reason, suitable for a callout below a property heading.
/// Returns `None` when the status carries no extra context worth showing.
fn proof_detail_line(status: &Option<ProofStatus>) -> Option<String> {
    match status {
        Some(ProofStatus::Failed { reason, .. }) => {
            Some(format!("**Verification failed:** {reason}"))
        }
        Some(ProofStatus::NotAttempted) => Some("**Not yet verified.**".into()),
        Some(ProofStatus::Assumed) => Some("**Assumed** (treated as an axiom).".into()),
        _ => None,
    }
}

/// Render an expanded "Verification failure" callout for a `Failed` status
/// that carries solver diagnostics (counterexample text and/or a verifier
/// log excerpt). Returns `None` when the status is not `Failed` or when no
/// diagnostics are present — in which case the short `proof_detail_line`
/// reason on its own is the right amount of detail.
///
/// Counterexamples are rendered in a `<details>` fold so the page stays
/// scannable while still letting curious readers click through to the
/// concrete witness or solver trace that broke the claim.
fn render_failure_details_callout(status: &Option<ProofStatus>) -> Option<String> {
    let (reason, counterexample, log_excerpt) = match status {
        Some(ProofStatus::Failed {
            reason,
            counterexample,
            log_excerpt,
            ..
        }) => (
            reason.as_str(),
            counterexample.as_deref(),
            log_excerpt.as_deref(),
        ),
        _ => return None,
    };
    if counterexample.is_none() && log_excerpt.is_none() {
        return None;
    }

    let mut out = String::new();
    let _ = writeln!(out, "> **Why this failed** — {reason}.");
    out.push('\n');
    if let Some(cx) = counterexample {
        let _ = writeln!(out, "<details><summary>Counterexample</summary>\n");
        let _ = writeln!(out, "```text\n{}\n```\n", cx.trim_end());
        let _ = writeln!(out, "</details>\n");
    }
    if let Some(log) = log_excerpt {
        let _ = writeln!(out, "<details><summary>Verifier log excerpt</summary>\n");
        let _ = writeln!(out, "```text\n{}\n```\n", log.trim_end());
        let _ = writeln!(out, "</details>\n");
    }
    Some(out)
}

/// Render a "Verify this yourself" section that shows the copy-pasteable
/// shell command (and/or generated SAW script path) recorded in the proof
/// manifest. Returns `None` when neither field is present.
///
/// The section lives below the proof-details / failure callouts so a
/// reader can act on what they just saw: re-run a failing proof to inspect
/// the counterexample interactively, or sanity-check a green verdict
/// against the same script the pipeline ran.
fn render_verify_command_section(status: &Option<ProofStatus>) -> Option<String> {
    let (verify_command, verify_script) = match status {
        Some(ProofStatus::Proven {
            verify_command,
            verify_script,
            ..
        })
        | Some(ProofStatus::Failed {
            verify_command,
            verify_script,
            ..
        }) => (verify_command.as_deref(), verify_script.as_deref()),
        _ => return None,
    };
    if verify_command.is_none() && verify_script.is_none() {
        return None;
    }

    let mut out = String::new();
    let _ = writeln!(out, "### Verify this yourself\n");
    // Prefer the manifest-recorded command verbatim (it knows the working
    // directory and any env vars).  Fall back to synthesising `saw <path>`
    // from just the script path when that's all we have.
    let command = verify_command
        .map(|s| s.to_string())
        .or_else(|| verify_script.map(|path| format!("saw \"{path}\"")));
    if let Some(cmd) = command {
        let _ = writeln!(out, "Re-run the proof locally:\n");
        let _ = writeln!(out, "```sh\n{}\n```\n", cmd.trim());
    }
    if let Some(script) = verify_script {
        // Only emit the script-path note when it adds information beyond
        // what `verify_command` already shows.
        if verify_command
            .map(|cmd| !cmd.contains(script))
            .unwrap_or(true)
        {
            let _ = writeln!(out, "Script: `{script}`\n");
        }
    }
    Some(out)
}

/// Render an expanded "Proof details" blockquote for a `Proven` status that
/// carries override or bounded-loop metadata.  Returns `None` when the
/// status is anything other than `Proven` or when neither `overrides` nor
/// `iterations` is populated — that way pages for ordinary unbounded proofs
/// stay clutter-free.
///
/// When present, the callout surfaces:
/// - the solver the proof was discharged with;
/// - the loop bound (e.g. `MAX_LEN`) the proof was checked at, so readers
///   know the bounded-model caveat;
/// - the list of `*_unsafe_assume_spec` / overridden functions the proof
///   depended on, so the trust story is explicit (the verdict is only as
///   strong as those overrides).
fn render_proof_details_callout(status: &Option<ProofStatus>) -> Option<String> {
    let (solver, time_secs, overrides, iterations) = match status {
        Some(ProofStatus::Proven {
            solver,
            time_secs,
            overrides,
            iterations,
            ..
        }) => (
            solver.as_str(),
            *time_secs,
            overrides.as_slice(),
            *iterations,
        ),
        _ => return None,
    };
    if overrides.is_empty() && iterations.is_none() {
        return None;
    }

    let mut out = String::new();
    let _ = writeln!(out, "> **Proof details** — discharged with `{solver}`.");
    if let Some(n) = iterations {
        let _ = writeln!(out, ">");
        let plural = if n == 1 { "iteration" } else { "iterations" };
        let _ = writeln!(
            out,
            "> Bounded-loop proof: validated for **{n} loop {plural}**. Inputs that exercise the loop more times than this fall outside the proof's scope."
        );
    }
    if !overrides.is_empty() {
        let _ = writeln!(out, ">");
        let plural = if overrides.len() == 1 {
            "override"
        } else {
            "overrides"
        };
        let _ = writeln!(
            out,
            "> Used {n} {plural} — each is **trusted** to behave per its spec and not re-verified here:",
            n = overrides.len()
        );
        for o in overrides {
            let _ = writeln!(out, "> - `{o}`");
        }
    }
    if let Some(t) = time_secs {
        let _ = writeln!(out, ">");
        let _ = writeln!(out, "> Solver wall-clock: {t:.2}s.");
    }
    out.push('\n');
    Some(out)
}

/// Build a `function name → proof status` lookup over the items in this
/// module.  Used by the property renderer to compute the *transitive*
/// implementation-equivalence story for each property: a property whose
/// Cryptol-level proof passes is only as strong against the C++/Rust
/// implementation as the weakest equivalence proof of every function the
/// property mentions.
fn function_status_map(items: &[Item]) -> HashMap<&str, &Option<ProofStatus>> {
    items
        .iter()
        .filter_map(|i| match i {
            Item::Function {
                name,
                signature,
                branches,
                body,
                proof_status,
                ..
            } => {
                // Skip value bindings — they aren't independently verified.
                if is_constant_binding(name, signature, body, branches) {
                    return None;
                }
                if !signature.contains("->") && branches.is_empty() {
                    return None;
                }
                Some((name.as_str(), proof_status))
            }
            _ => None,
        })
        .collect()
}

/// Return the **bare names** of functions referenced in a property body/doc.
/// Mirrors `find_involved_symbols` but yields raw identifiers so callers can
/// look them up in `function_status_map`.
fn find_involved_function_names(
    body: &str,
    doc: &[String],
    fn_status: &HashMap<&str, &Option<ProofStatus>>,
) -> Vec<String> {
    use crate::linker::contains_word;
    let all_text = format!("{body} {}", doc.join(" "));
    let mut names: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    // Sort by length descending so the longer-name-first rule of
    // `find_involved_symbols` is preserved (avoids spurious sub-string hits).
    let mut keys: Vec<&str> = fn_status.keys().copied().collect();
    keys.sort_by_key(|name| std::cmp::Reverse(name.len()));
    for name in keys {
        if contains_word(&all_text, name) && seen.insert(name.to_string()) {
            names.push(name.to_string());
        }
    }
    names.sort();
    names
}

/// Render a callout describing **implementation-equivalence coverage** for a
/// property — i.e., the *transitive* part of the proof.
///
/// The Cryptol property only guarantees a fact about the Cryptol shim.
/// For that guarantee to carry over to the production implementation, every
/// function the property mentions must also have a SAW `llvm_verify` /
/// `mir_verify` equivalence proof.  This callout makes that gap explicit so
/// readers don't mistake a green Cryptol `✓` for an end-to-end proof.
///
/// Returns `None` when no manifest data is available at all (in which case
/// the property page omits the callout entirely rather than imply
/// "unverified" when nothing was ever attempted).
fn render_implementation_equivalence_callout(
    involved: &[String],
    fn_status: &HashMap<&str, &Option<ProofStatus>>,
) -> Option<String> {
    if involved.is_empty() {
        return None;
    }
    let mut proven: Vec<&str> = Vec::new();
    let mut assumed: Vec<&str> = Vec::new();
    let mut failed: Vec<&str> = Vec::new();
    let mut unverified: Vec<&str> = Vec::new(); // NotAttempted or no manifest entry
    let mut any_status_seen = false;
    for name in involved {
        match fn_status.get(name.as_str()).and_then(|s| s.as_ref()) {
            Some(ProofStatus::Proven { .. }) => {
                any_status_seen = true;
                proven.push(name.as_str());
            }
            Some(ProofStatus::Assumed) => {
                any_status_seen = true;
                assumed.push(name.as_str());
            }
            Some(ProofStatus::Failed { .. }) => {
                any_status_seen = true;
                failed.push(name.as_str());
            }
            Some(ProofStatus::NotAttempted) => {
                any_status_seen = true;
                unverified.push(name.as_str());
            }
            None => unverified.push(name.as_str()),
        }
    }
    if !any_status_seen {
        // No manifest data for any of the involved functions — silent rather
        // than misleading.
        return None;
    }

    let fmt = |xs: &[&str]| {
        xs.iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let total = involved.len();
    let mut out = String::new();
    if failed.is_empty() && assumed.is_empty() && unverified.is_empty() {
        let _ = writeln!(
            out,
            "> ✓ **Implementation equivalence proven.** All {total} involved \
             function(s) have a SAW equivalence proof against the C++/Rust \
             implementation, so this property's guarantee transfers to the \
             compiled code."
        );
    } else {
        let _ = writeln!(
            out,
            "> ⚠ **Implementation equivalence is incomplete.** This property \
             holds against the Cryptol model. For the guarantee to carry over \
             to the compiled code, every involved function must also have a \
             SAW equivalence proof."
        );
        let _ = writeln!(out, ">");
        if !proven.is_empty() {
            let _ = writeln!(out, "> - ✓ proven equivalent: {}", fmt(&proven));
        }
        if !assumed.is_empty() {
            let _ = writeln!(
                out,
                "> - ~ **assumed** (treated as an axiom — *not* verified against the implementation): {}",
                fmt(&assumed)
            );
        }
        if !failed.is_empty() {
            let _ = writeln!(out, "> - ✗ equivalence proof **failed**: {}", fmt(&failed));
        }
        if !unverified.is_empty() {
            let _ = writeln!(
                out,
                "> - ✗ equivalence proof **not yet attempted**: {}",
                fmt(&unverified)
            );
        }
    }
    out.push('\n');
    Some(out)
}

/// Aggregate verification status across the properties of a single category.
///
/// Returns a short, minimalist badge string suitable for embedding as a
/// table cell.  Stays terse on purpose so the home-page table reads as a
/// scan-friendly status column rather than a wall of emoji.
///
/// Shapes:
///   "5/5 ✓"
///   "3/5 ✓ · 2 ✗"
///   "4/5 ✓ end-to-end · 1 ⚠ design-only"
///   "0/5"          (no manifest data for this category)
///
/// A property only counts as ✓ end-to-end when (a) its own proof_status is
/// Proven AND (b) every Cryptol function it mentions also has a Proven SAW
/// equivalence proof. Properties that pass the solver but rely on at least
/// one unverified function are bucketed as "design-only" (⚠) so the home
/// page reflects the transitive trust gap surfaced on individual property
/// pages.
fn render_category_status(
    labels: &[String],
    prop_info: &std::collections::HashMap<&str, (&Option<ProofStatus>, &str, &[String])>,
    fn_status: &HashMap<&str, &Option<ProofStatus>>,
) -> String {
    let total = labels.len();
    let mut end_to_end = 0usize;
    let mut design_only = 0usize;
    let mut assumed = 0usize;
    let mut failed = 0usize;
    let mut not_attempted = 0usize;
    let mut missing = 0usize;

    for label in labels {
        match prop_info.get(label.as_str()) {
            Some((status, body, doc)) => match status {
                Some(ProofStatus::Proven { .. }) => {
                    let involved = find_involved_function_names(body, doc, fn_status);
                    let all_proven = involved.iter().all(|name| {
                        matches!(
                            fn_status.get(name.as_str()).and_then(|s| s.as_ref()),
                            Some(ProofStatus::Proven { .. })
                        )
                    });
                    if all_proven {
                        end_to_end += 1;
                    } else {
                        design_only += 1;
                    }
                }
                Some(ProofStatus::Assumed) => assumed += 1,
                Some(ProofStatus::Failed { .. }) => failed += 1,
                Some(ProofStatus::NotAttempted) => not_attempted += 1,
                None => missing += 1,
            },
            None => missing += 1,
        }
    }

    // No proof-manifest data at all for this category.
    if end_to_end + design_only + assumed + failed + not_attempted == 0 {
        return format!("0/{total}");
    }

    let mut parts: Vec<String> = Vec::new();
    // Skip the "end-to-end" segment entirely when nothing has been proven
    // end-to-end yet — "0/5 ✓ end-to-end · 5 ⚠ design-only" reads as
    // self-contradictory.  Lead with the meaningful status instead.
    if end_to_end > 0 {
        parts.push(format!("{end_to_end}/{total} ✓ end-to-end"));
    }
    if design_only > 0 {
        parts.push(format!("{design_only} ⚠ design-only"));
    }
    if failed > 0 {
        parts.push(format!("{failed} ✗"));
    }
    if not_attempted + missing > 0 {
        parts.push(format!("{} unverified", not_attempted + missing));
    }
    if assumed > 0 {
        parts.push(format!("{assumed} ~ assumed"));
    }

    // Defensive: if every counter was zero except end_to_end (which we
    // skipped), fall back to the bare ratio so the cell isn't empty.
    if parts.is_empty() {
        return format!("0/{total}");
    }

    parts.join(" · ")
}

fn anchor_for(label: &str, name: &str) -> String {
    let label_lower = label.to_lowercase();
    let name_kebab = name.to_case(Case::Kebab);
    format!("{label_lower}--{name_kebab}")
}

/// Collect categories in document order for the index table.
fn collect_categories(items: &[Item], symbols: &SymbolTable) -> Vec<(String, String, Vec<String>)> {
    let mut result: Vec<(String, String, Vec<String>)> = Vec::new();
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
            if let Some(entry) = result.iter_mut().find(|(_, s, _)| s == &slug) {
                entry.2.push(label.clone());
            } else {
                result.push((title, slug, vec![label.clone()]));
            }
        }
    }
    result
}

/// Format a range like "P1–P5" from a list of labels.
fn property_range(labels: &[String]) -> String {
    if labels.is_empty() {
        return String::new();
    }
    if labels.len() == 1 {
        return labels[0].clone();
    }
    format!("{}–{}", labels[0], labels[labels.len() - 1])
}

/// Strip "Category X: " prefix and trailing dashes from section titles.
fn strip_category_prefix(title: &str) -> String {
    let payload = if let Some(pos) = title.find(':') {
        title[pos + 1..].trim()
    } else {
        title.trim()
    };
    // Strip trailing dashes (from parser's "---- Category A: Title ----" pattern)
    payload.trim_end_matches('-').trim().to_string()
}

/// Returns true for simple value constructors (e.g. `some`, `none`) that
/// should not appear in the top-level function listing.
fn is_simple_constructor(name: &str, _signature: &str, branches: &[Branch], body: &str) -> bool {
    // Must be lowercase (decision functions like provisionKey are camelCase)
    if name.chars().next().is_some_and(|c| c.is_uppercase()) {
        return false;
    }
    // Must have trivial branching (0 or 1 unconditional branches)
    if branches.len() > 1 {
        return false;
    }
    if branches.iter().any(|b| b.condition.is_some()) {
        return false;
    }
    // Extract the RHS of the definition (after "name ... =")
    let rhs = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join(" ");
    let rhs = rhs.find('=').map(|p| rhs[p + 1..].trim()).unwrap_or(&rhs);
    // Only treat as constructor if the body is a tuple literal like "(False, zero)"
    // or "(True, u)".  This catches Option-style some/none constructors but not
    // real functions like hmacSha256, lpField, etc.
    rhs.starts_with('(') && rhs.contains(',') && rhs.len() < 40
}

/// Detect "constant" value bindings like `FM_Disabled_b = 0 : [8]`.
///
/// These are top-level value declarations with **no parameters** and **no
/// arrow signature**. They're parsed into `Item::Function` (Cryptol doesn't
/// distinguish "constant" from "function"), but they aren't really functions
/// and shouldn't be listed in the Functions index next to real decision
/// procedures.
fn is_constant_binding(name: &str, signature: &str, body: &str, branches: &[Branch]) -> bool {
    // A real function has either a top-level arrow signature or branching logic.
    if signature.contains("->") {
        return false;
    }
    if branches.iter().any(|b| b.condition.is_some()) {
        return false;
    }
    let first_line = body.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let lhs = match first_line.find('=') {
        Some(p) => first_line[..p].trim(),
        None => return false,
    };
    // Strip optional `: type` annotation on the LHS.
    let lhs = match lhs.find(':') {
        Some(p) => lhs[..p].trim(),
        None => lhs,
    };
    lhs == name
}

/// Render a doc-comment block, preserving paragraph structure and turning
/// contiguous indented runs into fenced code blocks. Blank lines become
/// paragraph breaks; lines with 2+ spaces (or a tab) of leading indent are
/// grouped into ```` ```text ```` blocks so byte-layout listings and inline
/// snippets render as monospace instead of collapsing into a single wall
/// of prose.
fn render_doc_body<F>(out: &mut String, doc: &[String], mut resolve: F)
where
    F: FnMut(&str) -> String,
{
    let mut in_code = false;
    let mut blank_pending = false;
    let mut wrote_any = false;
    for line in doc {
        let trimmed_line = line.trim();
        if trimmed_line.is_empty() {
            // Inside a code block, blank lines are part of the block.
            if in_code {
                out.push('\n');
            } else if wrote_any {
                blank_pending = true;
            }
            continue;
        }
        let indented = line.starts_with("  ") || line.starts_with('\t');
        if indented {
            if !in_code {
                if wrote_any {
                    out.push('\n');
                }
                out.push_str("```text\n");
                in_code = true;
            }
            out.push_str(line);
            out.push('\n');
            wrote_any = true;
        } else {
            if in_code {
                out.push_str("```\n\n");
                in_code = false;
            } else if blank_pending {
                out.push('\n');
            }
            let _ = writeln!(out, "{}", resolve(line));
            wrote_any = true;
        }
        blank_pending = false;
    }
    if in_code {
        out.push_str("```\n");
    }
    if wrote_any {
        out.push('\n');
    }
}

/// Derive category slug from section title.
fn category_slug_from_title(title: &str) -> String {
    let payload = strip_category_prefix(title);
    sanitize_slug(&payload.to_case(Case::Kebab))
}

#[derive(Debug, Default)]
struct ParsedSignature {
    type_params: Vec<String>,
    constraints: Vec<String>,
    param_types: Vec<String>,
    return_type: Option<String>,
    raw: String,
}

fn parse_signature(signature: &str) -> ParsedSignature {
    // Cryptol accepts `//` line comments inside type signatures (e.g.
    //   `Bit ->          // fleetEnabled`
    //    `Bit ->          // hasKey`
    //    ...
    // ).  If we don't strip them out here, two things break:
    //
    //   (1) the comment text leaks into the rendered parameter type
    //       (`hasKey: // fleetEnabled Bit`), and
    //   (2) any `(` / `)` / `[` / `]` inside the comment text desyncs the
    //       bracket counter in `split_top_level_token`, which can collapse
    //       a whole `->`-chain into a single "return type" string.
    //
    // Strip line-by-line BEFORE we whitespace-normalize so we never see the
    // `//` sequence in the collapsed string.
    let stripped: String = signature
        .lines()
        .map(strip_line_comment)
        .collect::<Vec<_>>()
        .join("\n");

    let normalized = stripped
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();

    if normalized.is_empty() {
        return ParsedSignature::default();
    }

    let mut parsed = ParsedSignature {
        raw: normalized.clone(),
        ..ParsedSignature::default()
    };

    let (schema_part, core_part) =
        if let Some((left, right)) = split_top_level_once(&normalized, "=>") {
            (Some(left.trim()), right.trim())
        } else {
            (None, normalized.as_str())
        };

    if let Some(schema) = schema_part {
        let mut rest = schema.to_string();
        if rest.starts_with('{')
            && let Some((inside, after)) = extract_group(&rest, '{', '}')
        {
            parsed.type_params = split_top_level_char(&inside, ',')
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            rest = after.trim().to_string();
        }

        if rest.starts_with('(')
            && let Some((inside, _after)) = extract_group(&rest, '(', ')')
        {
            parsed.constraints = split_top_level_char(&inside, ',')
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }

    let parts = split_top_level_token(core_part, "->");
    if parts.is_empty() {
        return parsed;
    }
    if parts.len() == 1 {
        parsed.return_type = Some(parts[0].trim().to_string());
    } else {
        parsed.param_types = parts[..parts.len() - 1]
            .iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        parsed.return_type = Some(parts[parts.len() - 1].trim().to_string());
    }

    parsed
}

fn render_structured_signature<F>(
    out: &mut String,
    parsed: &ParsedSignature,
    param_names: &[String],
    show_raw_signature: bool,
    mut resolve_type: F,
) where
    F: FnMut(&str) -> String,
{
    // When the spec has no signature line (e.g. a byte-constant binding
    // such as `FM_Disabled_b = 0 : [8]`), silently skip the Signature
    // section.  The Formal-definition fold below already shows the body,
    // which carries the inline type annotation, so an explicit
    // "(not available)" line only adds noise.
    if parsed.raw.is_empty() {
        return;
    }

    let _ = writeln!(out, "### Signature\n");

    if !parsed.type_params.is_empty() {
        let _ = writeln!(out, "**Type Parameters**");
        for tp in &parsed.type_params {
            let _ = writeln!(out, "- `{}`", tp);
        }
        out.push('\n');
    }

    if !parsed.constraints.is_empty() {
        let _ = writeln!(out, "**Constraints**");
        for c in &parsed.constraints {
            let _ = writeln!(out, "- {}", resolve_type(c));
        }
        out.push('\n');
    }

    let _ = writeln!(out, "**Parameters**");
    if parsed.param_types.is_empty() {
        let _ = writeln!(out, "- *(none)*");
    } else {
        for (idx, param_ty) in parsed.param_types.iter().enumerate() {
            let default_name = format!("arg{}", idx + 1);
            let pname = param_names.get(idx).unwrap_or(&default_name);
            let _ = writeln!(out, "- `{}`: {}", pname, resolve_type(param_ty));
        }
    }
    out.push('\n');

    let _ = writeln!(out, "**Returns**");
    if let Some(ret) = &parsed.return_type {
        let _ = writeln!(out, "- {}\n", resolve_type(ret));
    } else {
        let _ = writeln!(out, "- *(unknown)*\n");
    }

    if show_raw_signature {
        let _ = writeln!(out, "<details><summary>Raw signature</summary>\n");
        let _ = writeln!(out, "`{}`\n", parsed.raw);
        let _ = writeln!(out, "</details>\n");
    }
}

fn extract_param_names(body: &str, fn_name: &str) -> Vec<String> {
    let first_line = body.lines().next().unwrap_or("").trim();
    let lhs = first_line
        .split_once('=')
        .map(|(left, _)| left.trim())
        .unwrap_or(first_line);

    let mut tokens = lhs.split_whitespace();
    if tokens.next() != Some(fn_name) {
        return Vec::new();
    }

    tokens
        .map(|tok| tok.trim_matches(|c: char| c == '(' || c == ')' || c == ','))
        .filter(|tok| !tok.is_empty() && *tok != "=")
        .map(|tok| tok.to_string())
        .collect()
}

fn split_top_level_once<'a>(s: &'a str, token: &str) -> Option<(&'a str, &'a str)> {
    if token.is_empty() || s.len() < token.len() {
        return None;
    }

    let bytes = s.as_bytes();
    let token_bytes = token.as_bytes();
    let mut i = 0usize;
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;

    while i + token_bytes.len() <= bytes.len() {
        match bytes[i] {
            b'(' => paren += 1,
            b')' => paren -= 1,
            b'[' => bracket += 1,
            b']' => bracket -= 1,
            b'{' => brace += 1,
            b'}' => brace -= 1,
            _ => {}
        }

        if paren == 0
            && bracket == 0
            && brace == 0
            && &bytes[i..i + token_bytes.len()] == token_bytes
        {
            let left = &s[..i];
            let right = &s[i + token_bytes.len()..];
            return Some((left, right));
        }
        i += 1;
    }

    None
}

/// Strip the first `//` line comment from a single line of a signature,
/// returning the prefix.  Used by `parse_signature` so trailing per-param
/// comments don't leak into the rendered type or break bracket counting.
///
/// Cryptol allows `//` line comments anywhere whitespace is legal, including
/// mid-signature (e.g. `Bit ->          // fleetEnabled`).  Without this
/// stripping, the comment text bleeds into the next parameter's type and any
/// `)` / `]` in the comment desyncs the bracket-balance tracker used to find
/// top-level `->` splits.
///
/// This is safe to apply unconditionally to signature strings: type
/// signatures never contain string literals (Cryptol has no string-typed
/// values at the type level), so a `//` here is always either a comment or
/// invalid Cryptol.
fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(idx) => &line[..idx],
        None => line,
    }
}

fn split_top_level_token(s: &str, token: &str) -> Vec<String> {
    if token.is_empty() {
        return vec![s.trim().to_string()];
    }
    let bytes = s.as_bytes();
    let token_bytes = token.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;

    while i + token_bytes.len() <= bytes.len() {
        match bytes[i] {
            b'(' => paren += 1,
            b')' => paren -= 1,
            b'[' => bracket += 1,
            b']' => bracket -= 1,
            b'{' => brace += 1,
            b'}' => brace -= 1,
            _ => {}
        }

        if paren == 0
            && bracket == 0
            && brace == 0
            && &bytes[i..i + token_bytes.len()] == token_bytes
        {
            parts.push(s[start..i].trim().to_string());
            start = i + token_bytes.len();
            i = start;
            continue;
        }
        i += 1;
    }

    parts.push(s[start..].trim().to_string());
    parts.into_iter().filter(|p| !p.is_empty()).collect()
}

fn split_top_level_char(s: &str, sep: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;

    for ch in s.chars() {
        match ch {
            '(' => paren += 1,
            ')' => paren -= 1,
            '[' => bracket += 1,
            ']' => bracket -= 1,
            '{' => brace += 1,
            '}' => brace -= 1,
            _ => {}
        }

        if ch == sep && paren == 0 && bracket == 0 && brace == 0 {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
            part.clear();
        } else {
            part.push(ch);
        }
    }

    let trimmed = part.trim();
    if !trimmed.is_empty() {
        parts.push(trimmed.to_string());
    }
    parts
}

fn extract_group(s: &str, open: char, close: char) -> Option<(String, String)> {
    let mut chars = s.chars();
    if chars.next()? != open {
        return None;
    }

    let mut depth = 0i32;
    let mut end_idx: Option<usize> = None;
    for (idx, ch) in s.char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                end_idx = Some(idx);
                break;
            }
        }
    }

    let end = end_idx?;
    let inside = s[1..end].to_string();
    let after = s[end + close.len_utf8()..].to_string();
    Some((inside, after))
}

// ── Mermaid diagrams ────────────────────────────────────────────────────────

fn sanitize_mermaid(text: &str) -> String {
    text.replace('"', "'").replace('#', "&#35;")
}

/// Render the cross-function call graph as a Mermaid diagram.
///
/// Currently unused in the rendered docs (the home-page call-graph block
/// was retired because it didn't pull its weight visually), but kept
/// around so we can resurface it elsewhere without re-deriving the logic.
#[allow(dead_code)]
fn render_call_graph_mermaid(
    edges: &[(String, String)],
    function_names: &[String],
    items: &[Item],
) -> String {
    // Classify functions:
    //   decision = multi-branch if/else
    //   stub = declared but no definition body (type signature only)
    let mut decision_fns: HashSet<String> = HashSet::new();
    let mut stub_fns: HashSet<String> = HashSet::new();
    for item in items {
        if let Item::Function {
            name,
            branches,
            body,
            ..
        } = item
        {
            if branches.is_empty() && !body.contains('=') {
                stub_fns.insert(name.clone());
            } else if branches.len() > 1 {
                decision_fns.insert(name.clone());
            }
        }
    }

    // Collect unique nodes, sorted for deterministic output.
    // Always include declared functions so isolated nodes are rendered.
    let mut nodes: HashSet<String> = HashSet::new();
    nodes.extend(function_names.iter().cloned());
    for (caller, callee) in edges {
        nodes.insert(caller.clone());
        nodes.insert(callee.clone());
    }
    if nodes.is_empty() {
        return String::new();
    }
    let mut sorted_nodes: Vec<&String> = nodes.iter().collect();
    sorted_nodes.sort();

    let mut out = String::new();
    let _ = writeln!(out, "### Call Graph\n");
    let _ = writeln!(out, "```mermaid");
    let _ = writeln!(out, "graph LR");

    // Node definitions with tooltips and click links
    for node in &sorted_nodes {
        if stub_fns.contains(*node) {
            let _ = writeln!(out, "  {node}[\"{node}\"]:::stub");
        } else if decision_fns.contains(*node) {
            let _ = writeln!(out, "  {node}[\"{node}\"]:::decision");
        } else {
            let _ = writeln!(out, "  {node}[\"{node}\"]");
        }
        let _ = writeln!(out, "  click {node} \"functions/{node}.md\" \"{node}\"");
    }

    for (caller, callee) in edges {
        let _ = writeln!(out, "  {caller} --> {callee}");
    }

    // Styling — Mermaid-friendly palette with high-contrast text.
    let _ = writeln!(
        out,
        "  classDef default fill:#f8fafc,stroke:#475569,stroke-width:1.5px,color:#0f172a"
    );
    let _ = writeln!(
        out,
        "  classDef decision fill:#ecfeff,stroke:#0e7490,stroke-width:1.5px,color:#164e63"
    );
    let _ = writeln!(
        out,
        "  classDef stub fill:#fff7ed,stroke:#c2410c,stroke-width:1.5px,stroke-dasharray: 5 5,color:#7c2d12"
    );
    let _ = writeln!(out, "```\n");

    // Legend
    let has_decision = sorted_nodes.iter().any(|n| decision_fns.contains(*n));
    let has_stub = sorted_nodes.iter().any(|n| stub_fns.contains(*n));
    if has_decision || has_stub {
        let _ = write!(out, "**Key:** ");
        let mut parts = vec!["🔵 function"];
        if has_decision {
            parts.push("🟢 decision");
        }
        if has_stub {
            parts.push("🟠 stub");
        }
        let _ = writeln!(out, "{}\n", parts.join(" · "));
    }

    out
}

fn render_flowchart_mermaid(name: &str, branches: &[Branch]) -> Option<String> {
    if branches.len() <= 2 {
        return None;
    }
    let mut out = String::new();
    let _ = writeln!(out, "```mermaid");
    let _ = writeln!(out, "flowchart TD");
    let _ = writeln!(out, "  Start([\"{}\"])", sanitize_mermaid(name));

    let mut cond_idx = 0usize;
    let mut res_idx = 0usize;
    let mut prev = "Start".to_string();
    let mut edge_label: Option<&str> = None;

    for branch in branches {
        if let Some(cond) = &branch.condition {
            let cid = format!("C{cond_idx}");
            cond_idx += 1;
            let clabel = sanitize_mermaid(cond);
            match edge_label {
                Some(label) => {
                    let _ = writeln!(out, "  {prev} -->|{label}| {cid}{{\"{clabel}\"}}");
                }
                None => {
                    let _ = writeln!(out, "  {prev} --> {cid}{{\"{clabel}\"}}");
                }
            }
            if !branch.result.trim().is_empty() {
                let rid = format!("R{res_idx}");
                res_idx += 1;
                let rlabel = sanitize_mermaid(&branch.result);
                let _ = writeln!(out, "  {cid} -->|Yes| {rid}(\"{rlabel}\")");
            }
            prev = cid;
            edge_label = Some("No");
        } else {
            let rid = format!("R{res_idx}");
            res_idx += 1;
            let rlabel = sanitize_mermaid(&branch.result);
            match edge_label {
                Some(label) => {
                    let _ = writeln!(out, "  {prev} -->|{label}| {rid}(\"{rlabel}\")");
                }
                None => {
                    let _ = writeln!(out, "  {prev} --> {rid}(\"{rlabel}\")");
                }
            }
            prev = rid;
            edge_label = None;
        }
    }

    // Styling
    let _ = writeln!(
        out,
        "  classDef default fill:#e8f4fd,stroke:#2196F3,stroke-width:2px,color:#1565C0"
    );
    let _ = writeln!(
        out,
        "  style Start fill:#1565C0,stroke:#0D47A1,color:#fff,stroke-width:2px"
    );
    let _ = writeln!(out, "```");
    Some(out)
}

fn render_coverage_map_mermaid(symbols: &SymbolTable, fn_names: &[String]) -> String {
    if symbols.related_properties.is_empty() {
        return String::new();
    }

    let mut edges: Vec<(String, String)> = Vec::new();
    let mut covered: HashSet<String> = HashSet::new();
    let fn_set: HashSet<&str> = fn_names.iter().map(|s| s.as_str()).collect();

    for (fn_name, props) in &symbols.related_properties {
        if !fn_set.contains(fn_name.as_str()) {
            continue;
        }
        for (label, _, _) in props {
            edges.push((label.clone(), fn_name.clone()));
            covered.insert(fn_name.clone());
        }
    }

    edges.sort();

    let uncovered: Vec<&String> = fn_names
        .iter()
        .filter(|n| !covered.contains(n.as_str()))
        .collect();

    if edges.is_empty() && uncovered.is_empty() {
        return String::new();
    }

    // Collect unique prop and func nodes for click/tooltip, sorted
    let mut prop_nodes: HashSet<String> = HashSet::new();
    let mut func_nodes: HashSet<String> = HashSet::new();
    for (prop, func) in &edges {
        prop_nodes.insert(prop.clone());
        func_nodes.insert(func.clone());
    }
    let mut sorted_props: Vec<&String> = prop_nodes.iter().collect();
    sorted_props.sort();
    let mut sorted_funcs: Vec<&String> = func_nodes.iter().collect();
    sorted_funcs.sort();

    let mut out = String::new();
    let _ = writeln!(out, "### Property Coverage\n");
    let _ = writeln!(out, "```mermaid");
    let _ = writeln!(out, "graph LR");

    // Node definitions with click links
    for prop in &sorted_props {
        let slug = symbols
            .property_categories
            .get(*prop)
            .cloned()
            .unwrap_or_else(|| "misc".into());
        let _ = writeln!(out, "  {prop}[\"{prop}\"]");
        let _ = writeln!(out, "  click {prop} \"properties/{slug}.md\" \"{prop}\"");
    }
    for func in &sorted_funcs {
        let _ = writeln!(out, "  {func}[\"{func}\"]");
        let _ = writeln!(out, "  click {func} \"functions/{func}.md\" \"{func}\"");
    }

    for (prop, func) in &edges {
        let _ = writeln!(out, "  {prop} --> {func}");
    }
    for func in &uncovered {
        let _ = writeln!(out, "  {func}:::gap");
    }

    // Styling
    let _ = writeln!(
        out,
        "  classDef default fill:#e8f4fd,stroke:#2196F3,stroke-width:2px,color:#1565C0"
    );
    if !uncovered.is_empty() {
        let _ = writeln!(
            out,
            "  classDef gap fill:#fff3e0,stroke:#FF9800,stroke-width:2px,stroke-dasharray: 5 5,color:#E65100"
        );
    }
    let _ = writeln!(out, "```\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linker::SymbolTable;
    use crate::parser::parse;
    use std::fs as stdfs;

    fn load_items() -> Vec<Item> {
        let src = stdfs::read_to_string("examples/SDEP.cry").expect("SDEP.cry not found");
        parse(&src)
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

    #[test]
    fn render_types_has_enums() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let types = render_types(&items, &symbols, "");
        assert!(
            types.contains("### FleetMode"),
            "types should contain FleetMode enum"
        );
        assert!(
            types.contains("`FM_Disabled`"),
            "types should list FM_Disabled"
        );
    }

    #[test]
    fn render_types_has_records() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let types = render_types(&items, &symbols, "");
        assert!(
            types.contains("### EnrollmentStatus"),
            "types should contain EnrollmentStatus record"
        );
    }

    #[test]
    fn render_types_aliases_are_headings() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let types = render_types(&items, &symbols, "");
        assert!(
            types.contains("### UUID"),
            "type aliases should render as headings"
        );
    }

    #[test]
    fn sanitize_type_width_strips_inline_comments() {
        assert_eq!(clean_type_width("2 // bytes"), "2");
        assert_eq!(clean_type_width("[16]"), "[16]");
    }

    #[test]
    fn sanitize_type_doc_removes_internal_proof_notes() {
        let doc = vec![
            "P23 proves injectivity.".to_string(),
            "SCOPE OF THIS PROOF (bounded model checking).".to_string(),
            "Field length in bytes.".to_string(),
        ];
        let cleaned = sanitize_type_doc(&doc).expect("doc should not be empty");
        assert!(cleaned.contains("Bounded check"));
        assert!(cleaned.contains("Field length in bytes"));
        assert!(!cleaned.to_lowercase().contains("scope of this proof"));
    }

    #[test]
    fn render_multi_file_creates_files() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let options = RenderOptions {
            no_details: false,
            title_override: None,
            docfx: false,
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
    fn proof_badge_rendering() {
        assert_eq!(
            proof_badge(&Some(ProofStatus::Proven {
                solver: "z3".into(),
                time_secs: Some(0.42),
                overrides: vec![],
                iterations: None,
                verify_command: None,
                verify_script: None,
            })),
            "✓"
        );
        assert_eq!(
            proof_badge(&Some(ProofStatus::Failed {
                reason: "counterexample".into(),
                counterexample: None,
                log_excerpt: None,
                verify_command: None,
                verify_script: None,
            })),
            "✗"
        );
        assert_eq!(proof_badge(&Some(ProofStatus::Assumed)), "~");
        assert_eq!(proof_badge(&Some(ProofStatus::NotAttempted)), "✗");
        assert_eq!(proof_badge(&None), "");
    }

    #[test]
    fn proof_details_callout_omitted_without_extras() {
        // No overrides + no iterations → no callout (page stays clean).
        let status = Some(ProofStatus::Proven {
            solver: "z3".into(),
            time_secs: Some(1.0),
            overrides: vec![],
            iterations: None,
            verify_command: None,
            verify_script: None,
        });
        assert!(render_proof_details_callout(&status).is_none());

        // Non-proven statuses never produce a callout.
        assert!(render_proof_details_callout(&Some(ProofStatus::Assumed)).is_none());
        assert!(
            render_proof_details_callout(&Some(ProofStatus::Failed {
                reason: "x".into(),
                counterexample: None,
                log_excerpt: None,
                verify_command: None,
                verify_script: None,
            }))
            .is_none()
        );
        assert!(render_proof_details_callout(&None).is_none());
    }

    #[test]
    fn proof_details_callout_reports_overrides_and_iterations() {
        let status = Some(ProofStatus::Proven {
            solver: "z3".into(),
            time_secs: Some(12.3),
            overrides: vec!["memcpy".into(), "operator new".into()],
            iterations: Some(4),
            verify_command: None,
            verify_script: None,
        });
        let out = render_proof_details_callout(&status).expect("callout present");
        assert!(out.contains("Proof details"), "header missing: {out}");
        assert!(out.contains("`z3`"), "solver missing: {out}");
        assert!(
            out.contains("**4 loop iterations**"),
            "iterations missing: {out}"
        );
        assert!(out.contains("2 overrides"), "override count missing: {out}");
        assert!(out.contains("`memcpy`"), "override name missing: {out}");
        assert!(
            out.contains("`operator new`"),
            "override name missing: {out}"
        );
        assert!(out.contains("12.30s"), "wall-clock missing: {out}");
    }

    #[test]
    fn proof_details_callout_singular_iteration() {
        let status = Some(ProofStatus::Proven {
            solver: "z3".into(),
            time_secs: None,
            overrides: vec![],
            iterations: Some(1),
            verify_command: None,
            verify_script: None,
        });
        let out = render_proof_details_callout(&status).expect("callout present");
        assert!(
            out.contains("**1 loop iteration**"),
            "singular missing: {out}"
        );
        assert!(!out.contains("iterations**"), "incorrect plural: {out}");
    }

    #[test]
    fn failure_details_callout_omitted_without_diagnostics() {
        // A plain Failed (no counterexample, no log) produces no fold — the
        // short reason from proof_detail_line is enough on its own.
        let status = Some(ProofStatus::Failed {
            reason: "error during verification".into(),
            counterexample: None,
            log_excerpt: None,
            verify_command: None,
            verify_script: None,
        });
        assert!(render_failure_details_callout(&status).is_none());

        // Non-failed statuses never produce a failure callout.
        assert!(
            render_failure_details_callout(&Some(ProofStatus::Proven {
                solver: "z3".into(),
                time_secs: None,
                overrides: vec![],
                iterations: None,
                verify_command: None,
                verify_script: None,
            }))
            .is_none()
        );
        assert!(render_failure_details_callout(&None).is_none());
    }

    #[test]
    fn failure_details_callout_renders_counterexample_and_log() {
        let status = Some(ProofStatus::Failed {
            reason: "counterexample found".into(),
            counterexample: Some("x = 0\ny = 1\n".into()),
            log_excerpt: Some("LLVM verification failed at line 42".into()),
            verify_command: None,
            verify_script: None,
        });
        let out = render_failure_details_callout(&status).expect("callout present");
        assert!(out.contains("Why this failed"), "header missing: {out}");
        assert!(
            out.contains("counterexample found"),
            "reason missing: {out}"
        );
        assert!(
            out.contains("<details><summary>Counterexample</summary>"),
            "counterexample fold missing: {out}"
        );
        assert!(out.contains("x = 0"), "counterexample body missing: {out}");
        assert!(
            out.contains("<details><summary>Verifier log excerpt</summary>"),
            "log fold missing: {out}"
        );
        assert!(out.contains("line 42"), "log body missing: {out}");
    }

    #[test]
    fn render_single_file_has_all_sections() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let options = RenderOptions {
            no_details: false,
            title_override: None,
            docfx: false,
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
    fn intentional_counterexample_detection() {
        // Positive: the documented marker triggers the badge override.
        let doc = vec![
            "P99: \"some tempting but wrong claim.\"".to_string(),
            "".to_string(),
            "EXPECTED VERDICT: FAILS.".to_string(),
            "Counterexample: x = 0.".to_string(),
        ];
        assert!(is_intentional_counterexample(&doc));

        // Negative: a regular PASS expectation must NOT be flagged.
        let pass_doc = vec![
            "P1: \"a real safety claim.\"".to_string(),
            "EXPECTED VERDICT: PASS.".to_string(),
        ];
        assert!(!is_intentional_counterexample(&pass_doc));

        // Negative: empty doc is not a counterexample.
        assert!(!is_intentional_counterexample(&[]));
    }

    #[test]
    fn intentional_counterexample_rendering_in_category_page() {
        // Minimal spec with one Category III section containing a single
        // EXPECTED VERDICT: FAILS property. Verifies that the rendered
        // category page (a) marks the heading with ✗, (b) emits the loud
        // "Intentionally disproven" callout, (c) swaps in the page-level
        // "deliberately false" intro, and (d) suppresses the misleading
        // "Implementation equivalence proven" callout that would otherwise
        // render alongside a deliberately-false claim.
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
        };
        render_multi_file(&items, &symbols, &tmpdir, &options).unwrap();

        let cat_md =
            stdfs::read_to_string(tmpdir.join("properties/intentional-counterexamples.md"))
                .unwrap_or_else(|_| {
                    // Fall back to whatever single category file was produced — the
                    // test fixture only contains one section so there's exactly one.
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

    #[test]
    fn render_single_file_uses_anchor_links() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let options = RenderOptions {
            no_details: false,
            title_override: None,
            docfx: false,
        };
        let doc = render_single_file(&items, &symbols, &options);
        // Should NOT contain relative file links
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
        // Should contain anchor-only links
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
        };
        let doc = render_single_file(&items, &symbols, &options);
        assert!(
            !doc.contains("<details>"),
            "no_details should suppress detail folds in single-file"
        );
    }

    #[test]
    fn parse_signature_splits_schema_params_and_return() {
        let sig =
            "{k, n} (width (8 * k) <= B, width (8 * (n + B)) <= B) => [k][8] -> [n][8] -> [L][8]";
        let parsed = parse_signature(sig);

        assert_eq!(parsed.type_params, vec!["k", "n"]);
        assert_eq!(
            parsed.constraints,
            vec!["width (8 * k) <= B", "width (8 * (n + B)) <= B"]
        );
        assert_eq!(parsed.param_types, vec!["[k][8]", "[n][8]"]);
        assert_eq!(parsed.return_type.as_deref(), Some("[L][8]"));
    }

    #[test]
    fn parse_signature_handles_higher_order_parameter() {
        let sig = "{ a } (fin c) => (a -> b) -> [c]a -> [c]b";
        let parsed = parse_signature(sig);

        assert_eq!(parsed.param_types, vec!["(a -> b)", "[c]a"]);
        assert_eq!(parsed.return_type.as_deref(), Some("[c]b"));
    }
}
