// Markdown renderer: emits .md files from linked IR.

use std::fmt::Write as FmtWrite;
use std::fs;
use std::io;
use std::path::Path;

use convert_case::{Case, Casing};

use crate::ir::{Item, ProofStatus};
use crate::linker::SymbolTable;

pub struct RenderOptions {
    pub no_details: bool,
    pub title_override: Option<String>,
}

/// Render a complete set of Markdown files to the output directory.
pub fn render_multi_file(
    items: &[Item],
    symbols: &SymbolTable,
    output_dir: &Path,
    options: &RenderOptions,
) -> io::Result<()> {
    fs::create_dir_all(output_dir.join("functions"))?;
    fs::create_dir_all(output_dir.join("properties"))?;

    let index = render_index(items, symbols, options);
    fs::write(output_dir.join("index.md"), index)?;

    let types = render_types(items, symbols);
    fs::write(output_dir.join("types.md"), types)?;

    render_function_files(items, symbols, output_dir, options)?;
    render_property_files(items, symbols, output_dir, options)?;

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
                    let doc_str = if doc.is_empty() {
                        String::new()
                    } else {
                        format!(" — {}", doc.join(" ").trim())
                    };
                    let _ = writeln!(out, "**{name}** — `{width}`{doc_str}\n");
                }
                Item::EnumGroup {
                    type_name,
                    width,
                    variants,
                    doc,
                    ..
                } => {
                    let _ = writeln!(out, "### {type_name}");
                    if !doc.is_empty() {
                        let _ = writeln!(out, "{}\n", doc.join(" ").trim());
                    } else {
                        let _ = writeln!(out, "A {width} value.\n");
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
                    if !doc.is_empty() {
                        let _ = writeln!(out, "{}\n", doc.join(" ").trim());
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
        .filter(|item| matches!(
            item,
            Item::Function { signature, branches, .. }
                if signature.contains("->") || !branches.is_empty()
        ))
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
            } = item
            {
                let _ = writeln!(out, "### `{name}`\n");

                let linked_sig = symbols.resolve_links_single_file(signature);
                let _ = writeln!(out, "**Signature:** `{linked_sig}`\n");

                if !doc.is_empty() {
                    for line in doc {
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
                } else if branches.len() == 1 {
                    let branch = &branches[0];
                    let result = symbols.resolve_links_single_file(&branch.result);
                    let _ = writeln!(out, "{result}\n");
                }

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

    out
}

// ── index.md ────────────────────────────────────────────────────────────────

fn render_index(items: &[Item], symbols: &SymbolTable, options: &RenderOptions) -> String {
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

    // Types section
    let _ = writeln!(out, "## Types\n");
    let _ = writeln!(out, "All type definitions: [types.md](types.md)\n");

    // Functions table
    let functions: Vec<_> = items
        .iter()
        .filter_map(|item| match item {
            Item::Function {
                name,
                signature,
                branches,
                doc,
                ..
            } => {
                // Skip enum constants (no signature arrow, no branches, no doc)
                if !signature.contains("->") && branches.is_empty() {
                    return None;
                }
                Some((name.clone(), doc.clone()))
            }
            _ => None,
        })
        .collect();

    if !functions.is_empty() {
        let _ = writeln!(out, "## Functions\n");
        let _ = writeln!(out, "| Function | Description |");
        let _ = writeln!(out, "|----------|-------------|");
        for (name, doc) in &functions {
            let first_line = first_doc_line(doc);
            let _ = writeln!(
                out,
                "| [{name}](functions/{name}.md) | {first_line} |"
            );
        }
        out.push('\n');
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

// ── types.md ────────────────────────────────────────────────────────────────

fn render_types(items: &[Item], symbols: &SymbolTable) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Types\n");

    for item in items {
        match item {
            Item::TypeAlias { name, width, doc } => {
                let doc_str = if doc.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", doc.join(" ").trim())
                };
                let _ = writeln!(out, "**{name}** — `{width}`{doc_str}\n");
            }
            Item::EnumGroup {
                type_name,
                width,
                variants,
                doc,
                ..
            } => {
                let _ = writeln!(out, "### {type_name}");
                if !doc.is_empty() {
                    let _ = writeln!(out, "{}\n", doc.join(" ").trim());
                } else {
                    let _ = writeln!(out, "A {width} value.\n");
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
                if !doc.is_empty() {
                    let _ = writeln!(out, "{}\n", doc.join(" ").trim());
                }
                out.push('\n');
                let _ = writeln!(out, "| Field | Type | Description |");
                let _ = writeln!(out, "|-------|------|-------------|");
                for (fname, ftype) in fields {
                    let linked_type =
                        symbols.resolve_links(ftype, "types.md");
                    let _ = writeln!(out, "| `{fname}` | {linked_type} | |");
                }
                out.push('\n');
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
) -> io::Result<()> {
    for item in items {
        if let Item::Function {
            name,
            signature,
            branches,
            body,
            doc,
        } = item
        {
            // Skip enum constants that were absorbed but left as functions
            if !signature.contains("->") && branches.is_empty() {
                continue;
            }

            let current_file = format!("functions/{name}.md");
            let mut out = String::new();

            let _ = writeln!(out, "# `{name}`\n");

            // Signature with cross-linked types
            let linked_sig = symbols.resolve_links(signature, &current_file);
            let _ = writeln!(out, "**Signature:** `{linked_sig}`\n");

            // Doc comment
            if !doc.is_empty() {
                for line in doc {
                    let linked = symbols.resolve_links(line, &current_file);
                    let _ = writeln!(out, "{linked}");
                }
                out.push('\n');
            }

            // Branches or expression
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
            } else if branches.len() == 1 {
                let branch = &branches[0];
                let result = symbols.resolve_links(&branch.result, &current_file);
                let _ = writeln!(out, "{result}\n");
            }

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

            fs::write(output_dir.join("functions").join(format!("{name}.md")), out)?;
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
        let current_file = format!("properties/{cat_slug}.md");
        let mut out = String::new();

        let _ = writeln!(out, "# {cat_title}\n");

        for prop_item in props {
            if let Item::Property {
                label,
                name,
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

        fs::write(
            output_dir.join("properties").join(format!("{cat_slug}.md")),
            out,
        )?;
    }

    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn first_doc_line(doc: &[String]) -> String {
    doc.first()
        .map(|s| {
            let trimmed = s.trim();
            if let Some(pos) = trimmed.find(". ") {
                trimmed[..=pos].to_string()
            } else {
                trimmed.to_string()
            }
        })
        .unwrap_or_default()
}

/// Split CamelCase into spaced words: "KeyMonotonicity" → "Key Monotonicity"
fn camel_to_spaced(name: &str) -> String {
    name.to_case(Case::Title)
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

/// Derive category slug from section title.
fn category_slug_from_title(title: &str) -> String {
    let payload = strip_category_prefix(title);
    payload.to_case(Case::Kebab)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linker::SymbolTable;
    use crate::parser::parse;
    use std::fs as stdfs;

    fn load_items() -> Vec<Item> {
        let src = stdfs::read_to_string("SDEP.cry").expect("SDEP.cry not found");
        parse(&src)
    }

    #[test]
    fn render_index_contains_title() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let options = RenderOptions {
            no_details: false,
            title_override: None,
        };
        let index = render_index(&items, &symbols, &options);
        assert!(index.contains("# SDEP"), "index should contain module title");
    }

    #[test]
    fn render_index_contains_functions_table() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let options = RenderOptions {
            no_details: false,
            title_override: None,
        };
        let index = render_index(&items, &symbols, &options);
        assert!(
            index.contains("[provisionKey](functions/provisionKey.md)"),
            "index should link to provisionKey"
        );
        assert!(
            index.contains("[enrollDevice](functions/enrollDevice.md)"),
            "index should link to enrollDevice"
        );
    }

    #[test]
    fn render_index_contains_properties_table() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let options = RenderOptions {
            no_details: false,
            title_override: None,
        };
        let index = render_index(&items, &symbols, &options);
        assert!(
            index.contains("[Key Lifecycle Safety](properties/key-lifecycle-safety.md)"),
            "index should link to key-lifecycle-safety"
        );
    }

    #[test]
    fn render_types_has_enums() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let types = render_types(&items, &symbols);
        assert!(types.contains("### FleetMode"), "types should contain FleetMode enum");
        assert!(types.contains("`FM_Disabled`"), "types should list FM_Disabled");
    }

    #[test]
    fn render_types_has_records() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let types = render_types(&items, &symbols);
        assert!(
            types.contains("### EnrollmentStatus"),
            "types should contain EnrollmentStatus record"
        );
    }

    #[test]
    fn render_multi_file_creates_files() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let options = RenderOptions {
            no_details: false,
            title_override: None,
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
        };
        let index = render_index(&items, &symbols, &options);
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
        };
        let doc = render_single_file(&items, &symbols, &options);
        assert!(
            !doc.contains("<details>"),
            "no_details should suppress detail folds in single-file"
        );
    }
}
