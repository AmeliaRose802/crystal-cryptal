// Render `coverage.md` — the headline "what is and isn't proven" matrix.

use std::fmt::Write as FmtWrite;

use crate::ir::ProofStatus;

use super::ledger::{CoverageBadge, Ledger, LedgerEntry, LedgerSource};

/// Title-line badge for a function page. When a ledger is present and
/// covers `name`, returns the new five-emoji badge; otherwise falls back
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
        CoverageBadge::ModelAbstraction => abstraction_banner(entry),
        CoverageBadge::Unverified => unverified_banner(entry),
        CoverageBadge::SpecOnly => spec_only_banner(entry),
    };
    Some(format!("> {body}\n\n"))
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
    format!(
        "⚠️ **Implemented, unverified.** This function exists in the \
         codebase but **no machine-checked equivalence proof** has been \
         discharged.{where_str}{proof_str}"
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

/// Render the full coverage matrix.
pub fn render_coverage_matrix(ledger: &Ledger) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Coverage Matrix\n");

    out.push_str(
        "> **What this page is.** Every function in the union of (the \
         Cryptol model) and (the production codebase, as reported by the \
         implementation inventory) is listed here exactly once, classified \
         by one of five badges. Functions that are *implemented but \
         unverified* are listed by default — silence is impossible. To \
         drop a helper from this page, add it to `coverage.toml` under \
         `[exclude].functions`; excluded names are reported as a count at \
         the bottom, never silently dropped.\n\n",
    );

    let _ = writeln!(out, "## Summary\n");
    let total = ledger.entries.len();
    let n_proven = ledger.count(CoverageBadge::Proven);
    let n_bounded = ledger.count(CoverageBadge::ProvenBounded);
    let n_abs = ledger.count(CoverageBadge::ModelAbstraction);
    let n_unv = ledger.count(CoverageBadge::Unverified);
    let n_spec = ledger.count(CoverageBadge::SpecOnly);

    let _ = writeln!(out, "| Badge | Meaning | Count |");
    let _ = writeln!(out, "|-------|---------|-------|");
    let _ = writeln!(
        out,
        "| {} | {} | {} |",
        CoverageBadge::Proven.emoji(),
        CoverageBadge::Proven.label(),
        n_proven
    );
    let _ = writeln!(
        out,
        "| {} | {} | {} |",
        CoverageBadge::ProvenBounded.emoji(),
        CoverageBadge::ProvenBounded.label(),
        n_bounded
    );
    let _ = writeln!(
        out,
        "| {} | {} | {} |",
        CoverageBadge::ModelAbstraction.emoji(),
        CoverageBadge::ModelAbstraction.label(),
        n_abs
    );
    let _ = writeln!(
        out,
        "| {} | {} | {} |",
        CoverageBadge::Unverified.emoji(),
        CoverageBadge::Unverified.label(),
        n_unv
    );
    let _ = writeln!(
        out,
        "| {} | {} | {} |",
        CoverageBadge::SpecOnly.emoji(),
        CoverageBadge::SpecOnly.label(),
        n_spec
    );
    let _ = writeln!(out, "| | **Total** | **{total}** |\n");

    if n_unv > 0 {
        let _ = writeln!(
            out,
            "> ⚠️ **{n_unv} real function{plural} ha{verb} no proof and no \
             declared abstraction.** These are the gaps a security review \
             needs to inspect first.\n",
            plural = if n_unv == 1 { "" } else { "s" },
            verb = if n_unv == 1 { "s" } else { "ve" },
        );
    }

    // Per-badge sections.
    for badge in [
        CoverageBadge::Unverified,
        CoverageBadge::ProvenBounded,
        CoverageBadge::Proven,
        CoverageBadge::ModelAbstraction,
        CoverageBadge::SpecOnly,
    ] {
        let rows: Vec<&LedgerEntry> = ledger.entries.iter().filter(|e| e.badge == badge).collect();
        if rows.is_empty() {
            continue;
        }
        let _ = writeln!(out, "## {} {}\n", badge.emoji(), badge.label());
        let _ = writeln!(out, "{}\n", section_lede(badge));
        let _ = writeln!(out, "| Function | Source | Maps to | Notes |");
        let _ = writeln!(out, "|----------|--------|---------|-------|");
        for entry in rows {
            let function_cell = function_link(entry);
            let source_cell = source_cell(entry);
            let maps_cell = maps_cell(entry);
            let notes_cell = notes_cell(entry);
            let _ = writeln!(
                out,
                "| {function_cell} | {source_cell} | {maps_cell} | {notes_cell} |"
            );
        }
        out.push('\n');
    }

    if !ledger.excluded.is_empty() {
        let _ = writeln!(
            out,
            "## Excluded helpers\n\n\
             {n} function{plural} excluded from the matrix via \
             `coverage.toml [exclude].functions` (not security-relevant): \
             {names}.\n",
            n = ledger.excluded.len(),
            plural = if ledger.excluded.len() == 1 { "" } else { "s" },
            names = ledger
                .excluded
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }

    out
}

fn section_lede(badge: CoverageBadge) -> &'static str {
    match badge {
        CoverageBadge::Proven => "Machine-checked equivalence on all ABI inputs.",
        CoverageBadge::ProvenBounded => {
            "Equivalence proven only up to an iteration / size bound. The \
             general-`n` case is a prose structural argument."
        }
        CoverageBadge::ModelAbstraction => {
            "Cryptol definitions with no real-code counterpart \
             (placeholders, uninterpreted functions, ABI adapters). The \
             notes column explains what each one stands in for."
        }
        CoverageBadge::Unverified => "Real production functions with no proof. This is the gap.",
        CoverageBadge::SpecOnly => {
            "Lives in the model on purpose (gap-exhibiting reference \
             functions, etc.) — no implementation expected."
        }
    }
}

fn function_link(entry: &LedgerEntry) -> String {
    match (&entry.module_prefix, &entry.module) {
        (Some(prefix), Some(_)) if !prefix.is_empty() => format!(
            "[`{name}`]({prefix}/functions/{name}.md)",
            name = entry.name,
            prefix = prefix
        ),
        (Some(_), Some(_)) => format!("[`{name}`](functions/{name}.md)", name = entry.name),
        _ => format!("`{name}`", name = entry.name),
    }
}

fn source_cell(entry: &LedgerEntry) -> String {
    let kind = match entry.source {
        LedgerSource::ModelOnly => "model",
        LedgerSource::ImplementationOnly => "impl",
        LedgerSource::Both => "model + impl",
    };
    match (&entry.impl_lang, &entry.module) {
        (Some(lang), Some(module)) => format!("{kind} ({module} ↔ {lang})"),
        (Some(lang), None) => format!("{kind} ({lang})"),
        (None, Some(module)) => format!("{kind} ({module})"),
        (None, None) => kind.to_string(),
    }
}

fn maps_cell(entry: &LedgerEntry) -> String {
    // For impl-side rows: link to the model fn they model, with note.
    // For model-side rows with abstraction: link to the impl entry that
    // declared `models = <this>` (if we had reverse-lookup) — punt for
    // now; the abstraction note in the next column carries the same info.
    let mut parts = Vec::new();
    if let Some(model) = &entry.models {
        let note = entry
            .models_note
            .as_deref()
            .map(|n| format!(" *({n})*"))
            .unwrap_or_default();
        parts.push(format!("`{model}`{note}"));
    }
    if !entry.composes.is_empty() {
        let composed: Vec<String> = entry.composes.iter().map(|c| format!("`{c}`")).collect();
        parts.push(format!("composes {}", composed.join(", ")));
    }
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join("; ")
    }
}

fn notes_cell(entry: &LedgerEntry) -> String {
    let mut parts = Vec::new();
    if let Some(note) = &entry.abstraction_note {
        parts.push(escape_cell(note));
    }
    if let Some(file) = &entry.impl_file {
        parts.push(format!("`{}`", file));
    }
    if let Some(proof) = &entry.proof {
        match proof {
            crate::ir::ProofStatus::Proven {
                solver, iterations, ..
            } => {
                if let Some(n) = iterations {
                    parts.push(format!("`{solver}`, ≤{n} iters"));
                } else {
                    parts.push(format!("`{solver}`"));
                }
            }
            crate::ir::ProofStatus::Assumed => parts.push("assumed".into()),
            crate::ir::ProofStatus::Failed { reason, .. } => {
                parts.push(format!("failed: {}", escape_cell(reason)))
            }
            crate::ir::ProofStatus::NotAttempted => parts.push("not attempted".into()),
        }
    }
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join(" · ")
    }
}

fn escape_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}
