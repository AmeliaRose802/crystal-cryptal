// Small helpers used across the render_md submodules.

use std::fmt::Write as FmtWrite;

use convert_case::{Case, Casing};

use crate::ir::Branch;
use crate::linker::sanitize_slug;

pub(super) const TYPE_DOC_INTERNAL_MARKERS: &[&str] = &[
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

pub(super) fn first_doc_line(doc: &[String]) -> String {
    let mut parts = Vec::new();
    for line in doc {
        if line.trim().is_empty() {
            break;
        }
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

pub(super) fn escape_md_cell(s: &str) -> String {
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
pub(super) fn describe_type(ty: &str) -> String {
    let t = ty.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.contains("->") {
        return String::new();
    }
    if t == "Bit" {
        return "Boolean flag".into();
    }
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
    if t.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return format!("`{t}` value");
    }
    String::new()
}

pub(super) fn prefixed_file(prefix: &str, file: &str) -> String {
    if prefix.is_empty() {
        file.to_string()
    } else {
        format!("{}/{}", prefix.trim_matches('/'), file)
    }
}

pub(super) fn clean_type_width(width: &str) -> String {
    width
        .split_once("//")
        .map(|(head, _)| head)
        .unwrap_or(width)
        .trim()
        .to_string()
}

pub(super) fn render_type_alias_width(out: &mut String, width: &str) {
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

pub(super) fn sanitize_type_doc(doc: &[String]) -> Option<String> {
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
pub(super) fn camel_to_spaced(name: &str) -> String {
    name.to_case(Case::Title)
}

/// Returns true for simple value constructors (e.g. `some`, `none`) that
/// should not appear in the top-level function listing.
pub(super) fn is_simple_constructor(
    name: &str,
    _signature: &str,
    branches: &[Branch],
    body: &str,
) -> bool {
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

/// Detect "constant" value bindings like `FM_Disabled_b = 0 : [8]`.
pub(super) fn is_constant_binding(
    name: &str,
    signature: &str,
    body: &str,
    branches: &[Branch],
) -> bool {
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
    let lhs = match lhs.find(':') {
        Some(p) => lhs[..p].trim(),
        None => lhs,
    };
    lhs == name
}

/// Render a doc-comment block, preserving paragraph structure and turning
/// contiguous indented runs into fenced code blocks.
pub(super) fn render_doc_body<F>(out: &mut String, doc: &[String], mut resolve: F)
where
    F: FnMut(&str) -> String,
{
    let mut in_code = false;
    let mut blank_pending = false;
    let mut wrote_any = false;
    for line in doc {
        let trimmed_line = line.trim();
        if trimmed_line.is_empty() {
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

/// Strip "Category X: " or trailing dashes from a section title.
pub(super) fn strip_category_prefix(title: &str) -> String {
    let payload = if let Some(pos) = title.find(':') {
        title[pos + 1..].trim()
    } else {
        title.trim()
    };
    payload.trim_end_matches('-').trim().to_string()
}

/// Derive a property-category slug from a section title.
pub(super) fn category_slug_from_title(title: &str) -> String {
    let payload = strip_category_prefix(title);
    sanitize_slug(&payload.to_case(Case::Kebab))
}

/// Anchor used for in-page links on properties (e.g. `p1--my-name`).
pub(super) fn anchor_for(label: &str, name: &str) -> String {
    let label_lower = label.to_lowercase();
    let name_kebab = name.to_case(Case::Kebab);
    format!("{label_lower}--{name_kebab}")
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
