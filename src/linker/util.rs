// Misc helpers for the linker: slug normalization, name predicates, and
// word-boundary search.

use convert_case::{Case, Casing};
use regex::Regex;

use crate::ir::Branch;

/// Keep linker behavior aligned with renderer: simple tuple constructors such as
/// `none`/`some` are not rendered as standalone function pages.
pub(super) fn is_simple_constructor(name: &str, branches: &[Branch], body: &str) -> bool {
    if name.chars().next().is_some_and(|c| c.is_uppercase()) {
        return false;
    }
    if branches.len() > 1 {
        return false;
    }
    if branches.iter().any(|b| b.condition.is_some()) {
        return false;
    }

    let rhs = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join(" ");
    let rhs = rhs.find('=').map(|p| rhs[p + 1..].trim()).unwrap_or(&rhs);
    rhs.starts_with('(') && rhs.contains(',') && rhs.len() < 40
}

/// Replace characters that are illegal in Windows filenames and collapse runs of `-`.
///
/// After `to_case(Case::Kebab)` converts a section title to a slug, any character
/// that survived the conversion but is rejected by Windows (e.g. `*` from `void*`)
/// is replaced with `-`.  Consecutive dashes are collapsed and leading/trailing
/// dashes are stripped.  Windows reserved base-names (CON, NUL, COM1, …) get a
/// trailing `_` appended so they remain usable as file-stems.
pub(crate) fn sanitize_slug(s: &str) -> String {
    const INVALID: &[char] = &['*', '?', '<', '>', ':', '|', '"', '\\', '/'];
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];

    let mut slug = String::with_capacity(s.len());
    let mut last_dash = true; // treat start as dash to trim leading `-`
    for c in s.chars() {
        if INVALID.contains(&c) || c == '-' {
            if !last_dash {
                slug.push('-');
                last_dash = true;
            }
        } else {
            slug.push(c);
            last_dash = false;
        }
    }
    // Trim trailing dash.
    while slug.ends_with('-') {
        slug.pop();
    }
    // Avoid Windows reserved base-names (case-insensitive).
    if RESERVED.iter().any(|&r| slug.eq_ignore_ascii_case(r)) {
        slug.push('_');
    }
    if slug.is_empty() {
        slug.push_str("unnamed");
    }
    slug
}

/// Derive a category slug from a section title like "Category A: Key Lifecycle Safety".
pub(super) fn category_slug(title: &str) -> String {
    // Strip "Category X: " prefix if present.
    let payload = if let Some(pos) = title.find(':') {
        title[pos + 1..].trim()
    } else {
        title.trim()
    };
    sanitize_slug(&payload.to_case(Case::Kebab))
}

/// Generate a property anchor from label and name.
/// e.g., "P1" + "KeyMonotonicity" → "p1--key-monotonicity"
pub(super) fn property_anchor(label: &str, name: &str) -> String {
    let label_lower = label.to_lowercase();
    let name_kebab = name.to_case(Case::Kebab);
    format!("{label_lower}--{name_kebab}")
}

/// Check if `text` contains `word` as a whole word (not part of a larger identifier).
pub(crate) fn contains_word(text: &str, word: &str) -> bool {
    let pattern = format!(r"\b{}\b", regex::escape(word));
    Regex::new(&pattern)
        .map(|re| re.is_match(text))
        .unwrap_or(false)
}
