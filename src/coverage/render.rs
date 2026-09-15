// Render `coverage.md` — the headline "what is and isn't proven" matrix.

use crate::ir::ProofStatus;

use super::ledger::{CoverageBadge, Ledger, LedgerEntry};
use super::matrix::{reason_codes_inline, source_path, source_url};

/// Title-line badge for a function page. When a ledger is present and
/// covers `name`, returns the new coverage badge; otherwise falls back
/// to the legacy `✓ / ✗ / ~ / ""` glyph derived from `proof`.
pub fn function_title_badge(
    ledger: Option<&Ledger>,
    name: &str,
    proof: &Option<ProofStatus>,
) -> String {
    if let Some(l) = ledger
        && let Some(entry) = l.lookup(name)
    {
        return entry.badge.emoji().to_string();
    }
    legacy_proof_glyph(proof).to_string()
}

/// Cell text for the Functions index "Status" column. Same fallback rule
/// as [`function_title_badge`].
pub fn function_status_cell(
    ledger: Option<&Ledger>,
    name: &str,
    proof: &Option<ProofStatus>,
) -> String {
    if let Some(l) = ledger
        && let Some(entry) = l.lookup(name)
    {
        return format!("{} {}", entry.badge.emoji(), entry.badge.label());
    }
    legacy_status_cell(proof).to_string()
}

/// Per-page banner shown immediately under the function title. Spells out
/// what the badge actually means and what's missing — the page is the
/// place a reader lands from a search result, so the banner is the load-
/// bearing piece of the honesty story. Returns `None` when no ledger is
/// available or no banner makes sense for this badge.
pub fn function_banner(ledger: Option<&Ledger>, name: &str) -> Option<String> {
    let entry = ledger?.lookup(name)?;
    let body = match entry.badge {
        CoverageBadge::Proven => return None,
        CoverageBadge::ProvenBounded => bounded_banner(entry),
        CoverageBadge::TrustedAssumption => trusted_assumption_banner(entry),
        CoverageBadge::AbiAdapter => abstraction_banner(entry),
        CoverageBadge::Unverified => unverified_banner(entry),
        CoverageBadge::SpecOnly => spec_only_banner(entry),
    };
    Some(format!("> {body}\n\n"))
}

/// Link a model function page to its corresponding production implementation.
pub fn function_implementation_source(ledger: Option<&Ledger>, name: &str) -> Option<String> {
    let ledger = ledger?;
    let entry = ledger.lookup(name)?;
    let file = entry.impl_file.as_deref()?;
    let path = source_path(file);
    let implementation = entry.impl_name.as_deref().unwrap_or(name);
    let location = source_url(ledger, &path)
        .map_or_else(|| format!("`{path}`"), |url| format!("[`{path}`]({url})"));
    Some(format!(
        "**Implementation source:** `{implementation}` in {location}.\n\n"
    ))
}

fn bounded_banner(entry: &LedgerEntry) -> String {
    let iters = match &entry.proof {
        Some(ProofStatus::Proven {
            iterations: Some(n),
            ..
        }) => Some(*n),
        _ => None,
    };
    let bound = iters
        .map(|n| format!(" up to **{n} loop iterations**"))
        .unwrap_or_default();
    format!(
        "🔲 **Proven (bounded).** SAW discharged this equivalence{bound}. \
         Inputs that exercise the loop more times than the bound fall \
         **outside** the proof; the general-`n` case rests on a prose \
         structural argument, not a machine proof."
    )
}

fn abstraction_banner(entry: &LedgerEntry) -> String {
    let note = entry
        .abstraction_note
        .as_deref()
        .unwrap_or("Model abstraction with no real-code counterpart.");
    format!(
        "🧩 **Model abstraction.** {note} No production function is proven \
         equivalent to this definition on this page."
    )
}

fn trusted_assumption_banner(entry: &LedgerEntry) -> String {
    let note = entry
        .assumption_note
        .as_deref()
        .unwrap_or("This primitive is treated as a trusted external dependency.");
    let stand_in_for = if entry.stands_in_for.is_empty() {
        String::new()
    } else {
        format!(
            " Stands in for real function{plural}: {names}.",
            plural = if entry.stands_in_for.len() == 1 {
                ""
            } else {
                "s"
            },
            names = entry
                .stands_in_for
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!(
        "🔒 **Trusted assumption — not proven here.** {note}{stand_in_for} \
         Any proof that depends on this definition inherits that assumption."
    )
}

fn unverified_banner(entry: &LedgerEntry) -> String {
    let where_str = entry
        .impl_file
        .as_deref()
        .map(|f| format!(" Real implementation: `{f}`."))
        .unwrap_or_default();
    let proof_str = match &entry.proof {
        Some(ProofStatus::Failed { reason, .. }) => {
            format!(" Verification **failed**: {reason}.")
        }
        Some(ProofStatus::NotAttempted) => " Proof has not been attempted yet.".to_string(),
        _ => String::new(),
    };
    let reason_str = if entry.reason_codes.is_empty() {
        "".to_string()
    } else {
        format!(
            " Reason code(s): {}.",
            reason_codes_inline(&entry.reason_codes)
        )
    };
    format!(
        "⚠️ **Implemented, unverified.** This function exists in the \
         codebase but **no machine-checked equivalence proof** has been \
         discharged.{where_str}{reason_str}{proof_str}"
    )
}

fn spec_only_banner(_entry: &LedgerEntry) -> String {
    "📄 **Spec-only.** This definition lives in the Cryptol model on \
     purpose — typically as a gap-exhibiting reference function — and has \
     **no production implementation**."
        .to_string()
}

fn legacy_proof_glyph(status: &Option<ProofStatus>) -> &'static str {
    match status {
        Some(ProofStatus::Proven { .. }) => "✓",
        Some(ProofStatus::Failed { .. }) | Some(ProofStatus::NotAttempted) => "✗",
        Some(ProofStatus::Assumed) => "~",
        None => "",
    }
}

fn legacy_status_cell(status: &Option<ProofStatus>) -> &'static str {
    match status {
        Some(ProofStatus::Proven { .. }) => "✓ proven",
        Some(ProofStatus::Assumed) => "~ assumed",
        Some(ProofStatus::Failed { .. }) => "✗ failed",
        Some(ProofStatus::NotAttempted) => "✗ not attempted",
        None => "—",
    }
}
