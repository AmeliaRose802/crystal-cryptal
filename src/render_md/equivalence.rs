// Transitive implementation-equivalence: maps each property to the proof
// state of the functions it mentions, and renders the trust-gap callout.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as FmtWrite;

use crate::ir::{Item, ProofStatus};

use super::util::is_constant_binding;

/// Build a `function name → proof status` lookup over the items in this
/// module.
pub(super) fn function_status_map(items: &[Item]) -> HashMap<&str, &Option<ProofStatus>> {
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

/// Return the bare names of functions referenced in a property body/doc.
pub(super) fn find_involved_function_names(
    body: &str,
    doc: &[String],
    fn_status: &HashMap<&str, &Option<ProofStatus>>,
) -> Vec<String> {
    use crate::linker::contains_word;
    let all_text = format!("{body} {}", doc.join(" "));
    let mut names: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
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

/// Render a callout describing implementation-equivalence coverage for a
/// property.
pub(super) fn render_implementation_equivalence_callout(
    involved: &[String],
    fn_status: &HashMap<&str, &Option<ProofStatus>>,
) -> Option<String> {
    if involved.is_empty() {
        return None;
    }
    let mut proven: Vec<&str> = Vec::new();
    let mut assumed: Vec<&str> = Vec::new();
    let mut failed: Vec<&str> = Vec::new();
    let mut unverified: Vec<&str> = Vec::new();
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
