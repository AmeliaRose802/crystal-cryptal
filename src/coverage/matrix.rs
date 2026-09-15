// Render the shared coverage summary and per-badge tables.

use std::fmt::Write as FmtWrite;

use crate::ir::ProofStatus;

use super::ledger::{CoverageBadge, CoverageReason, Ledger, LedgerEntry, LedgerSource};

/// Render the dedicated coverage page.
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
    out.push_str(&render_coverage_content(ledger));
    out
}

/// Render the substantive coverage sections shared verbatim by `coverage.md`
/// and the home page. Keeping this as one renderer prevents rows, links, and
/// diagnostics from diverging between the two entry points.
pub fn render_coverage_content(ledger: &Ledger) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "## Coverage summary\n");
    render_summary(&mut out, ledger);

    for badge in [
        CoverageBadge::Unverified,
        CoverageBadge::Proven,
        CoverageBadge::ProvenBounded,
        CoverageBadge::TrustedAssumption,
        CoverageBadge::AbiAdapter,
        CoverageBadge::SpecOnly,
    ] {
        render_badge_section(&mut out, ledger, badge);
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

fn render_summary(out: &mut String, ledger: &Ledger) {
    let total = ledger.entries.len();
    let n_proven = ledger.count(CoverageBadge::Proven);
    let n_bounded = ledger.count(CoverageBadge::ProvenBounded);
    let n_trusted = ledger.count(CoverageBadge::TrustedAssumption);
    let n_abs = ledger.count(CoverageBadge::AbiAdapter);
    let n_unv = ledger.count(CoverageBadge::Unverified);
    let n_spec = ledger.count(CoverageBadge::SpecOnly);

    let _ = writeln!(out, "| Badge | Meaning | Count |");
    let _ = writeln!(out, "|-------|---------|-------|");
    for (badge, count) in [
        (CoverageBadge::Proven, n_proven),
        (CoverageBadge::ProvenBounded, n_bounded),
        (CoverageBadge::TrustedAssumption, n_trusted),
        (CoverageBadge::AbiAdapter, n_abs),
        (CoverageBadge::Unverified, n_unv),
        (CoverageBadge::SpecOnly, n_spec),
    ] {
        let _ = writeln!(out, "| {} | {} | {count} |", badge.emoji(), badge.label());
    }
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
}

fn render_badge_section(out: &mut String, ledger: &Ledger, badge: CoverageBadge) {
    let rows: Vec<&LedgerEntry> = ledger.entries.iter().filter(|e| e.badge == badge).collect();
    if rows.is_empty() {
        return;
    }
    let title = if badge == CoverageBadge::TrustedAssumption {
        "Trusted assumptions"
    } else {
        badge.label()
    };
    let _ = writeln!(out, "## {} {title}\n", badge.emoji());
    let _ = writeln!(out, "{}\n", section_lede(badge));
    let has_reason_codes = rows.iter().any(|entry| !entry.reason_codes.is_empty());
    if has_reason_codes {
        let _ = writeln!(
            out,
            "| Function | Source | Maps to | Reason codes | Notes |"
        );
        let _ = writeln!(
            out,
            "|----------|--------|---------|--------------|-------|"
        );
    } else {
        let _ = writeln!(out, "| Function | Source | Maps to | Notes |");
        let _ = writeln!(out, "|----------|--------|---------|-------|");
    }
    for entry in rows {
        let function = function_link(entry);
        let source = source_cell(entry);
        let maps = maps_cell(ledger, entry);
        let notes = notes_cell(entry);
        if has_reason_codes {
            let reasons = reason_codes_cell(entry);
            let _ = writeln!(
                out,
                "| {function} | {source} | {maps} | {reasons} | {notes} |"
            );
        } else {
            let _ = writeln!(out, "| {function} | {source} | {maps} | {notes} |");
        }
    }
    out.push('\n');
}

fn section_lede(badge: CoverageBadge) -> &'static str {
    match badge {
        CoverageBadge::Proven => "Machine-checked equivalence on all ABI inputs.",
        CoverageBadge::ProvenBounded => {
            "Equivalence proven only up to an iteration / size bound. The general-`n` case is a prose structural argument."
        }
        CoverageBadge::TrustedAssumption => {
            "Assumed contracts for real external dependencies. These are explicit trust boundaries, not machine-checked equivalence proofs."
        }
        CoverageBadge::AbiAdapter => {
            "Cryptol definitions with no real-code counterpart (placeholders, uninterpreted functions, ABI adapters). The notes column explains what each one stands in for."
        }
        CoverageBadge::Unverified => "Real production functions with no proof. This is the gap.",
        CoverageBadge::SpecOnly => {
            "Lives in the model on purpose (gap-exhibiting reference functions, etc.) — no implementation expected."
        }
    }
}

fn function_link(entry: &LedgerEntry) -> String {
    match (&entry.module_prefix, &entry.module) {
        (Some(prefix), Some(_)) if !prefix.is_empty() => format!(
            "[`{name}`]({prefix}/functions/{name}.md)",
            name = entry.name,
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
    let source = match (&entry.impl_lang, &entry.module) {
        (Some(lang), Some(module)) => format!("{kind} ({module} ↔ {lang})"),
        (Some(lang), None) => format!("{kind} ({lang})"),
        (None, Some(module)) => format!("{kind} ({module})"),
        (None, None) => kind.to_string(),
    };
    entry.impl_file.as_ref().map_or(source.clone(), |file| {
        format!(
            "{source} ([source]({}))",
            markdown_target(&source_path(file))
        )
    })
}

fn maps_cell(ledger: &Ledger, entry: &LedgerEntry) -> String {
    let mut parts = Vec::new();
    if let Some(model) = &entry.models {
        let note = entry
            .models_note
            .as_deref()
            .map(|n| format!(" *({n})*"))
            .unwrap_or_default();
        let target = ledger
            .lookup(model)
            .filter(|target| target.name != entry.name)
            .map(function_link)
            .unwrap_or_else(|| format!("`{model}`"));
        parts.push(format!("{target}{note}"));
    }
    if !entry.composes.is_empty() {
        parts.push(format!(
            "composes {}",
            entry
                .composes
                .iter()
                .map(|c| format!("`{c}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !entry.stands_in_for.is_empty() {
        parts.push(format!(
            "stands in for {}",
            entry
                .stands_in_for
                .iter()
                .map(|c| format!("`{c}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join("; ")
    }
}

fn reason_codes_cell(entry: &LedgerEntry) -> String {
    if entry.reason_codes.is_empty() {
        "—".to_string()
    } else {
        reason_codes_inline(&entry.reason_codes)
    }
}

pub(super) fn reason_codes_inline(codes: &[CoverageReason]) -> String {
    codes
        .iter()
        .map(|r| format!("`{} {}`", r.code(), r.short_label()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn notes_cell(entry: &LedgerEntry) -> String {
    let mut parts = Vec::new();
    if let Some(note) = &entry.abstraction_note {
        parts.push(escape_cell(note));
    }
    if let Some(note) = &entry.assumption_note {
        parts.push(escape_cell(note));
    }
    if let Some(proof) = &entry.proof {
        match proof {
            ProofStatus::Proven {
                overrides,
                iterations,
                ..
            } => {
                parts.push(iterations.map_or_else(
                    || "Verified return value and post-state".to_string(),
                    |n| format!("Verified return value and post-state for ≤{n} iterations"),
                ));
                if !overrides.is_empty() {
                    parts.push(format!(
                        "assumes callees {}",
                        overrides
                            .iter()
                            .map(|name| format!("`{name}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }
            ProofStatus::Assumed => parts.push("assumed".into()),
            ProofStatus::Failed {
                reason,
                log_excerpt,
                verify_script,
                ..
            } => {
                parts.push(format!("failed: {}", escape_cell(reason)));
                if let Some(script) = verify_script {
                    parts.push(format!(
                        "[generated SAW script]({})",
                        markdown_target(&source_path(script))
                    ));
                }
                if let Some(diagnostic) = log_excerpt {
                    parts.push(format!(
                        "<details><summary>Complete verifier diagnostics</summary><pre>{}</pre></details>",
                        escape_html(diagnostic)
                    ));
                }
            }
            ProofStatus::NotAttempted => parts.push("not attempted".into()),
        }
    }
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join(" · ")
    }
}

fn markdown_target(path: &str) -> String {
    path.replace(' ', "%20")
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\n', "&#10;")
}

fn source_path(file: &str) -> String {
    let path = std::path::Path::new(file);
    let relative = std::env::current_dir()
        .ok()
        .and_then(|dir| path.strip_prefix(dir).ok())
        .unwrap_or(path);
    if relative.is_relative() {
        return relative.to_string_lossy().replace('\\', "/");
    }
    let repo_path: std::path::PathBuf = path
        .components()
        .skip_while(|component| {
            !matches!(component.as_os_str().to_str(), Some("cpp" | "rust" | "src"))
        })
        .collect();
    if repo_path.as_os_str().is_empty() {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    } else {
        repo_path.to_string_lossy().replace('\\', "/")
    }
}

fn escape_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}
