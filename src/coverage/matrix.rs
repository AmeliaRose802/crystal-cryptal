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
    for entry in &rows {
        let function = function_link(ledger, entry);
        let source = source_cell(ledger, entry);
        let maps = maps_cell(entry);
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
    render_diagnostics(out, &rows);
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

fn function_link(ledger: &Ledger, entry: &LedgerEntry) -> String {
    match (&entry.module_prefix, &entry.module) {
        (Some(prefix), Some(_)) if !prefix.is_empty() => format!(
            "[`{display}`]({prefix}/functions/{name}.md)",
            display = entry.impl_name.as_deref().unwrap_or(&entry.name),
            name = entry.name
        ),
        (Some(_), Some(_)) => format!(
            "[`{display}`](functions/{name}.md)",
            display = entry.impl_name.as_deref().unwrap_or(&entry.name),
            name = entry.name
        ),
        _ => entry.impl_name.as_ref().map_or_else(
            || format!("`{}`", entry.name),
            |impl_name| {
                entry.impl_file.as_ref().map_or_else(
                    || format!("`{impl_name}`"),
                    |file| source_link(ledger, impl_name, file),
                )
            },
        ),
    }
}

fn source_cell(ledger: &Ledger, entry: &LedgerEntry) -> String {
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
        let path = source_path(file);
        source_url(ledger, &path).map_or_else(
            || format!("{source} (`{path}`)"),
            |url| format!("{source} ([source]({url}))"),
        )
    })
}

fn maps_cell(entry: &LedgerEntry) -> String {
    let mut parts = Vec::new();
    if entry.module.is_some() && entry.impl_name.is_some() {
        let model = entry.models.as_deref().unwrap_or(&entry.name);
        let note = entry
            .models_note
            .as_deref()
            .map(|n| format!(" *({n})*"))
            .unwrap_or_default();
        parts.push(format!(
            "implementation `{}` ↔ model {}{note}",
            entry.impl_name.as_deref().unwrap_or(&entry.name),
            model_link(entry, model)
        ));
    } else if let Some(model) = &entry.models {
        parts.push(model_link(entry, model));
    } else if entry.badge == CoverageBadge::TrustedAssumption {
        parts.push(format!(
            "external contract ↔ model {}",
            model_link(entry, &entry.name)
        ));
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
        let stand_ins = entry
            .stands_in_for
            .iter()
            .filter(|name| Some(name.as_str()) != entry.impl_name.as_deref())
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>();
        if !stand_ins.is_empty() {
            parts.push(format!("stands in for {}", stand_ins.join(", ")));
        }
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
            ProofStatus::Failed { reason, .. } => {
                parts.push(format!("failed: {}", escape_cell(reason)));
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

fn render_diagnostics(out: &mut String, rows: &[&LedgerEntry]) {
    for entry in rows {
        let Some(ProofStatus::Failed {
            log_excerpt: Some(diagnostic),
            verify_script,
            ..
        }) = &entry.proof
        else {
            continue;
        };
        let display = entry.impl_name.as_deref().unwrap_or(&entry.name);
        let _ = writeln!(
            out,
            "<details><summary>Complete verifier diagnostics — <code>{}</code></summary>\n",
            escape_html(display)
        );
        render_copyable_text_block(out, diagnostic);
        if let Some(script) = verify_script {
            let file = std::path::Path::new(script)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            let _ = writeln!(
                out,
                "\nGenerated SAW script: <code>{}</code> (local verifier artifact; not published with this site).",
                escape_html(&file)
            );
        }
        let _ = writeln!(out, "\n</details>\n");
    }
}

fn render_copyable_text_block(out: &mut String, text: &str) {
    let fence = "`".repeat(longest_backtick_run(text).max(2) + 1);
    let _ = writeln!(out, "{fence}text\n{}\n{fence}", text.trim_end());
    let _ = writeln!(
        out,
        "\n<button type=\"button\" class=\"btn btn-default btn-xs\" aria-label=\"Copy verifier log\" onclick=\"navigator.clipboard.writeText(this.previousElementSibling.textContent)\">Copy</button>"
    );
}

fn longest_backtick_run(text: &str) -> usize {
    text.split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0)
}

fn model_link(entry: &LedgerEntry, model: &str) -> String {
    match (&entry.module_prefix, &entry.module) {
        (Some(prefix), Some(_)) if !prefix.is_empty() => {
            format!("[`{model}`]({prefix}/functions/{model}.md)")
        }
        (Some(_), Some(_)) => format!("[`{model}`](functions/{model}.md)"),
        _ => format!("`{model}`"),
    }
}

fn source_link(ledger: &Ledger, label: &str, file: &str) -> String {
    let path = source_path(file);
    source_url(ledger, &path)
        .map(|url| format!("[`{label}`]({url})"))
        .unwrap_or_else(|| format!("`{label}`"))
}

pub(super) fn source_url(ledger: &Ledger, path: &str) -> Option<String> {
    ledger.source_url_base.as_ref().map(|base| {
        format!(
            "{}{}",
            base,
            path.split('/')
                .map(|part| part.replace(' ', "%20"))
                .collect::<Vec<_>>()
                .join("/")
        )
    })
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\n', "&#10;")
}

pub(super) fn source_path(file: &str) -> String {
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
