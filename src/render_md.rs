// Markdown renderer: emits .md files from linked IR.

use std::collections::HashSet;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io;
use std::path::Path;

use convert_case::{Case, Casing};

use crate::describe::{auto_describe_function, auto_describe_property};
use crate::ir::{Branch, Item, ProofStatus};
use crate::linker::{function_call_graph, sanitize_slug, SymbolTable};

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
    fs::create_dir_all(output_dir.join("functions"))?;
    fs::create_dir_all(output_dir.join("properties"))?;

    let module_name = items
        .iter()
        .find_map(|i| if let Item::Module { name, .. } = i { Some(name.clone()) } else { None })
        .unwrap_or_else(|| "Specification".into());
    let title = options.title_override.as_deref().unwrap_or(&module_name).to_string();

    let mut index = render_index(items, symbols, options, path_prefix);
    if options.docfx {
        index = format!("{}{}", docfx_frontmatter(&module_name, &title), index);
    }
    fs::write(output_dir.join("index.md"), index)?;

    let types = render_types(items, symbols, path_prefix);
    fs::write(output_dir.join("types.md"), types)?;

    let mut functions_index = render_functions_index(items, symbols, options, path_prefix);
    if options.docfx {
        let fn_uid = format!("{module_name}.functions");
        functions_index = format!("{}{}", docfx_frontmatter(&fn_uid, "Functions"), functions_index);
    }
    fs::write(output_dir.join("functions/index.md"), functions_index)?;

    render_function_files(items, symbols, output_dir, options, path_prefix)?;
    render_property_files(items, symbols, output_dir, options, path_prefix)?;

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

    let title = options
        .title_override
        .as_deref()
        .unwrap_or(&module_name);
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
                    let clean_width = clean_type_width(width);
                    let _ = writeln!(out, "`{clean_width}`");
                    if let Some(clean_doc) = sanitize_type_doc(doc) {
                        let _ = writeln!(out, "\n{clean_doc}");
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
                        let _ = writeln!(out, "| `{fname}` | {linked_type} | |");
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
            Item::Function { name, signature, branches, body, .. } => {
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

                let effective_doc = if doc.is_empty() {
                    auto_describe_function(name, signature, branches, body)
                } else {
                    doc.clone()
                };
                if !effective_doc.is_empty() {
                    for line in &effective_doc {
                        let linked = symbols.resolve_links_single_file(line);
                        let _ = writeln!(out, "{linked}");
                    }
                    out.push('\n');
                }

                if branches.len() > 1 {
                    let _ = writeln!(out, "| # | Condition | Result |");
                    let _ = writeln!(out, "|---|-----------|--------|");
                    for (i, branch) in branches.iter().enumerate() {
                        let cond = match &branch.condition {
                            Some(c) => symbols.resolve_links_single_file(c),
                            None => "*(otherwise)*".into(),
                        };
                        let result = symbols.resolve_links_single_file(&branch.result);
                        let _ = writeln!(out, "| {} | {} | {} |", i + 1, cond, result);
                    }
                    out.push('\n');

                    // Flowchart
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
                    let _ = writeln!(out, "```cryptol\n{body}\n```\n");
                    let _ = writeln!(out, "</details>\n");
                }
            }
        }
    }

    // Call graph
    if !functions.is_empty() {
        let fn_names: Vec<String> = functions
            .iter()
            .filter_map(|item| {
                if let Item::Function { name, .. } = item {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();
        let edges = function_call_graph(items, &fn_names);
        out.push_str(&render_call_graph_mermaid(&edges, &fn_names, items));
    }

    // ── Properties by category ──────────────────────────────────────────
    let mut category_items: Vec<(String, Vec<&Item>)> = Vec::new();
    let mut current_title = String::new();

    for item in items {
        if let Item::Section { level: 3, title, .. } = item {
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
                let badge = proof_badge(proof_status);
                let badge_str = if badge.is_empty() {
                    String::new()
                } else {
                    format!("  {badge}")
                };
                let _ = writeln!(out, "### {label} — {display_name}{badge_str}\n");

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

                if !options.no_details && !body.is_empty() {
                    let _ = writeln!(
                        out,
                        "<details><summary>Formal property (Cryptol)</summary>\n"
                    );
                    let _ = writeln!(out, "```cryptol\n{body}\n```\n");
                    let _ = writeln!(out, "</details>\n");
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

    let title = options
        .title_override
        .as_deref()
        .unwrap_or(&module_name);
    let _ = writeln!(out, "# {title}\n");

    if !module_doc.is_empty() {
        for line in &module_doc {
            let _ = writeln!(out, "{line}");
        }
        out.push('\n');
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
        Item::Function { name, signature, branches, body, .. } => {
            (signature.contains("->") || !branches.is_empty())
                && !is_simple_constructor(name, signature, branches, body)
        }
        _ => false,
    });

    if has_functions {
        let _ = writeln!(out, "## Functions\n");
        let _ = writeln!(out, "All function definitions: [functions](functions/index.md)\n");

        // Call graph on index page
        let fn_names: Vec<String> = items
            .iter()
            .filter_map(|item| match item {
                Item::Function { name, signature, branches, body, .. } => {
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
        let edges = function_call_graph(items, &fn_names);
        out.push_str(&render_call_graph_mermaid(&edges, &fn_names, items));
    }

    // Properties by category
    let categories = collect_categories(items, symbols);
    if !categories.is_empty() {
        let _ = writeln!(out, "## Security Properties\n");
        let _ = writeln!(out, "| Category | Properties |");
        let _ = writeln!(out, "|----------|------------|");
        for (cat_title, cat_slug, labels) in &categories {
            let range = property_range(labels);
            let _ = writeln!(
                out,
                "| [{cat_title}](properties/{cat_slug}.md) | {range} |"
            );
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

    let functions: Vec<_> = items
        .iter()
        .filter_map(|item| match item {
            Item::Function {
                name,
                signature,
                branches,
                body,
                doc,
                ..
            } => {
                if !signature.contains("->") && branches.is_empty() {
                    return None;
                }
                if is_simple_constructor(name, signature, branches, body) {
                    return None;
                }
                let effective_doc = if doc.is_empty() {
                    auto_describe_function(name, signature, branches, body)
                } else {
                    doc.clone()
                };
                Some((name.clone(), effective_doc))
            }
            _ => None,
        })
        .collect();

    if !functions.is_empty() {
        let _ = writeln!(out, "| Function | Description |");
        let _ = writeln!(out, "|----------|-------------|");
        for (name, doc) in &functions {
            let first_line = first_doc_line(doc);
            let _ = writeln!(out, "| [{name}]({name}.md) | {first_line} |");
        }
        out.push('\n');

        // Call graph
        let fn_names: Vec<String> =
            functions.iter().map(|(n, _)| n.clone()).collect();
        let edges = function_call_graph(items, &fn_names);
        out.push_str(&render_call_graph_mermaid(&edges, &fn_names, items));
    }

    out
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
            Item::Function { name, signature, branches, body, .. } => {
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
                let clean_width = clean_type_width(width);
                let _ = writeln!(out, "`{clean_width}`");
                if let Some(clean_doc) = sanitize_type_doc(doc) {
                    let _ = writeln!(out, "\n{clean_doc}");
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
                    let _ = writeln!(out, "| `{fname}` | {linked_type} | |");
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

            // Doc comment (auto-generated when absent)
            let effective_doc = if doc.is_empty() {
                auto_describe_function(name, signature, branches, body)
            } else {
                doc.clone()
            };
            if !effective_doc.is_empty() {
                for line in &effective_doc {
                    let linked = symbols.resolve_links(line, &current_file);
                    let _ = writeln!(out, "{linked}");
                }
                out.push('\n');
            }

            // Branches (decision table + flowchart for multi-branch functions)
            if branches.len() > 1 {
                // Decision table
                let _ = writeln!(out, "| # | Condition | Result |");
                let _ = writeln!(out, "|---|-----------|--------|");
                for (i, branch) in branches.iter().enumerate() {
                    let cond = match &branch.condition {
                        Some(c) => symbols.resolve_links(c, &current_file),
                        None => "*(otherwise)*".into(),
                    };
                    let result =
                        symbols.resolve_links(&branch.result, &current_file);
                    let _ = writeln!(out, "| {} | {} | {} |", i + 1, cond, result);
                }
                out.push('\n');

                // Flowchart
                if let Some(chart) = render_flowchart_mermaid(name, branches) {
                    out.push_str(&chart);
                    out.push('\n');
                }
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

            // Details fold with raw body
            if !options.no_details && !body.is_empty() {
                let _ = writeln!(out, "<details><summary>Formal definition (Cryptol)</summary>\n");
                let _ = writeln!(out, "```cryptol\n{body}\n```\n");
                let _ = writeln!(out, "</details>");
            }

            let fn_path = output_dir.join("functions").join(format!("{name}.md"));
            fs::write(&fn_path, out).map_err(|e| {
                io::Error::new(e.kind(), format!("{}: {e}", fn_path.display()))
            })?;
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
        if let Item::Section { level: 3, title, .. } = item {
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
                let badge = proof_badge(proof_status);
                let badge_str = if badge.is_empty() {
                    String::new()
                } else {
                    format!("  {badge}")
                };
                let _ = writeln!(out, "### {label} — {display_name}{badge_str}\n");

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

                // Involved functions and types
                let involved = find_involved_symbols(body, doc, symbols, &current_file);
                if !involved.is_empty() {
                    let _ = writeln!(out, "**Involved:** {}\n", involved.join(", "));
                }

                // Details fold
                if !options.no_details && !body.is_empty() {
                    let _ = writeln!(
                        out,
                        "<details><summary>Formal property (Cryptol)</summary>\n"
                    );
                    let _ = writeln!(out, "```cryptol\n{body}\n```\n");
                    let _ = writeln!(out, "</details>\n");
                }
            }
        }

        let prop_path = output_dir.join("properties").join(format!("{cat_slug}.md"));
        fs::write(&prop_path, out).map_err(|e| {
            io::Error::new(e.kind(), format!("{}: {e}", prop_path.display()))
        })?;
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
        matches!(i, Item::TypeAlias { .. } | Item::EnumGroup { .. } | Item::RecordType { .. })
    });

    // Collect property categories (slug, title) in order.
    let mut cats: Vec<(String, String)> = Vec::new();
    let mut cur_title = String::new();
    let mut cur_slug = String::new();
    for item in items {
        if let Item::Section { level: 3, title: sec_title, .. } = item {
            cur_title = strip_category_prefix(sec_title);
            cur_slug = category_slug_from_title(sec_title);
        }
        if let Item::Property { label, .. } = item {
            let slug = if cur_slug.is_empty() { "misc".to_string() } else { cur_slug.clone() };
            let ttl = if cur_title.is_empty() { "Miscellaneous".to_string() } else { cur_title.clone() };
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
            out.push_str(&format!("  - name: {cat_title}\n    href: properties/{slug}.md\n"));
        }
    }
    out
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn first_doc_line(doc: &[String]) -> String {
    let mut parts = Vec::new();
    for line in doc {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        parts.push(trimmed);
        if trimmed.ends_with('.') || trimmed.ends_with('!') || trimmed.ends_with('?') {
            break;
        }
    }
    parts.join(" ")
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
    syms.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    for (name, (target_file, anchor)) in &syms {
        // Skip self-references and property labels (P1, P2, etc.)
        if current_file == *target_file {
            continue;
        }
        if name.len() <= 3 && name.starts_with('P') && name[1..].chars().all(|c| c.is_ascii_digit()) {
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

fn proof_badge(status: &Option<ProofStatus>) -> String {
    match status {
        Some(ProofStatus::Proven { solver, time_secs }) => {
            let time_str = time_secs
                .map(|t| format!(", {t:.2}s"))
                .unwrap_or_default();
            format!("✅ Proven ({solver}{time_str})")
        }
        Some(ProofStatus::Assumed) => "⚠\u{fe0f} Assumed".into(),
        Some(ProofStatus::Failed { reason }) => format!("❌ Failed: {reason}"),
        Some(ProofStatus::NotAttempted) => "⬚ Not yet verified".into(),
        None => String::new(),
    }
}

fn anchor_for(label: &str, name: &str) -> String {
    let label_lower = label.to_lowercase();
    let name_kebab = name.to_case(Case::Kebab);
    format!("{label_lower}--{name_kebab}")
}

/// Collect categories in document order for the index table.
fn collect_categories(
    items: &[Item],
    symbols: &SymbolTable,
) -> Vec<(String, String, Vec<String>)> {
    let mut result: Vec<(String, String, Vec<String>)> = Vec::new();
    let mut current_title = String::new();
    let mut current_slug = String::new();

    for item in items {
        if let Item::Section { level: 3, title, .. } = item {
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
    let rhs = rhs
        .find('=')
        .map(|p| rhs[p + 1..].trim())
        .unwrap_or(&rhs);
    // Only treat as constructor if the body is a tuple literal like "(False, zero)"
    // or "(True, u)".  This catches Option-style some/none constructors but not
    // real functions like hmacSha256, lpField, etc.
    rhs.starts_with('(') && rhs.contains(',') && rhs.len() < 40
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
    let normalized = signature
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

    let (schema_part, core_part) = if let Some((left, right)) = split_top_level_once(&normalized, "=>") {
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
    if parsed.raw.is_empty() {
        let _ = writeln!(out, "**Signature:** *(not available)*\n");
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
        if let Item::Function { name, branches, body, .. } = item {
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
    let _ = writeln!(out, "  classDef default fill:#f8fafc,stroke:#475569,stroke-width:1.5px,color:#0f172a");
    let _ = writeln!(out, "  classDef decision fill:#ecfeff,stroke:#0e7490,stroke-width:1.5px,color:#164e63");
    let _ = writeln!(out, "  classDef stub fill:#fff7ed,stroke:#c2410c,stroke-width:1.5px,stroke-dasharray: 5 5,color:#7c2d12");
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
                    let _ = writeln!(
                        out,
                        "  {prev} -->|{label}| {cid}{{\"{clabel}\"}}"
                    );
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
                    let _ = writeln!(
                        out,
                        "  {prev} -->|{label}| {rid}(\"{rlabel}\")"
                    );
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
    let _ = writeln!(out, "  classDef default fill:#e8f4fd,stroke:#2196F3,stroke-width:2px,color:#1565C0");
    let _ = writeln!(out, "  style Start fill:#1565C0,stroke:#0D47A1,color:#fff,stroke-width:2px");
    let _ = writeln!(out, "```");
    Some(out)
}

fn render_coverage_map_mermaid(
    symbols: &SymbolTable,
    fn_names: &[String],
) -> String {
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
        let slug = symbols.property_categories.get(*prop).cloned().unwrap_or_else(|| "misc".into());
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
    let _ = writeln!(out, "  classDef default fill:#e8f4fd,stroke:#2196F3,stroke-width:2px,color:#1565C0");
    if !uncovered.is_empty() {
        let _ = writeln!(out, "  classDef gap fill:#fff3e0,stroke:#FF9800,stroke-width:2px,stroke-dasharray: 5 5,color:#E65100");
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
        assert!(index.contains("# SDEP"), "index should contain module title");
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
        assert!(types.contains("### FleetMode"), "types should contain FleetMode enum");
        assert!(types.contains("`FM_Disabled`"), "types should list FM_Disabled");
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
        assert!(types.contains("### UUID"), "type aliases should render as headings");
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

        let provision =
            stdfs::read_to_string(tmpdir.join("functions/provisionKey.md")).unwrap();
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
            })),
            "✅ Proven (z3, 0.42s)"
        );
        assert_eq!(proof_badge(&Some(ProofStatus::Assumed)), "⚠\u{fe0f} Assumed");
        assert_eq!(proof_badge(&None), "");
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
        assert!(doc.contains("## Functions"), "should contain Functions section");
        assert!(doc.contains("### FleetMode"), "should contain FleetMode type");
        assert!(doc.contains("### `provisionKey`"), "should contain provisionKey function");
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
        let sig = "{k, n} (width (8 * k) <= B, width (8 * (n + B)) <= B) => [k][8] -> [n][8] -> [L][8]";
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
