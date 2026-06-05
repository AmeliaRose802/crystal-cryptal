// Property-category collection and aggregate-status badge rendering.

use std::collections::HashMap;
use std::fmt::Write as FmtWrite;

use crate::ir::{Item, ProofStatus};
use crate::linker::SymbolTable;

use super::equivalence::find_involved_function_names;
use super::util::{category_slug_from_title, strip_category_prefix};

/// Collect categories in document order for the index table.
pub(super) fn collect_categories(
    items: &[Item],
    symbols: &SymbolTable,
) -> Vec<(String, String, Vec<String>)> {
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
pub(super) fn property_range(labels: &[String]) -> String {
    if labels.is_empty() {
        return String::new();
    }
    if labels.len() == 1 {
        return labels[0].clone();
    }
    format!("{}–{}", labels[0], labels[labels.len() - 1])
}

/// Aggregate verification status across the properties of a single category.
pub(super) fn render_category_status(
    labels: &[String],
    prop_info: &HashMap<&str, (&Option<ProofStatus>, &str, &[String])>,
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

    if end_to_end + design_only + assumed + failed + not_attempted == 0 {
        return format!("0/{total}");
    }

    let mut parts: Vec<String> = Vec::new();
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

    if parts.is_empty() {
        let mut out = String::new();
        let _ = write!(out, "0/{total}");
        return out;
    }

    parts.join(" · ")
}
