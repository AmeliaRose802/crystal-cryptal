// Proof-status badges, detail lines, failure callouts, and rerun command.

use std::collections::HashSet;
use std::fmt::Write as FmtWrite;

use crate::ir::ProofStatus;
use crate::linker::SymbolTable;

/// Short verification marker for use next to a heading or name.
///
/// Verdicts: `✓` proven · `✗` failed/not yet verified · `~` assumed · empty
pub(super) fn proof_badge(status: &Option<ProofStatus>) -> String {
    match status {
        Some(ProofStatus::Proven { .. }) => "✓".into(),
        Some(ProofStatus::Failed { .. }) => "✗".into(),
        Some(ProofStatus::Assumed) => "~".into(),
        Some(ProofStatus::NotAttempted) => "✗".into(),
        None => String::new(),
    }
}

/// One-cell summary of a function's SAW-equivalence proof state for the
/// Functions index table.
pub(super) fn proof_status_cell(status: &Option<ProofStatus>) -> String {
    match status {
        Some(ProofStatus::Proven { .. }) => "✓ proven".into(),
        Some(ProofStatus::Assumed) => "~ assumed".into(),
        Some(ProofStatus::Failed { .. }) => "✗ failed".into(),
        Some(ProofStatus::NotAttempted) => "✗ not attempted".into(),
        None => "—".into(),
    }
}

/// Whether a candidate first-line summary is presentable in a table cell.
pub(super) fn is_useful_summary(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.ends_with(':') {
        return false;
    }
    true
}

/// Detect a property whose doc-comment declares it as an intentional
/// counterexample (`EXPECTED VERDICT: FAILS`).
pub(super) fn is_intentional_counterexample(doc: &[String]) -> bool {
    doc.iter()
        .any(|line| line.contains("EXPECTED VERDICT: FAILS"))
}

/// Loud, unambiguous callout placed immediately below the heading of an
/// intentional-counterexample property.
pub(super) fn intentional_counterexample_callout() -> String {
    "> **✗ Intentionally disproven.** This property is a *deliberately \
     false* claim about the protocol. The Cryptol prover refutes it with \
     a concrete counterexample (see the **Note** below); the property \
     exists to make the failure mode visible to readers and is **not** a \
     safety guarantee of the implementation.\n\n"
        .to_string()
}

/// Long-form verdict reason, suitable for a callout below a property heading.
pub(super) fn proof_detail_line(status: &Option<ProofStatus>) -> Option<String> {
    match status {
        Some(ProofStatus::Failed { reason, .. }) => {
            Some(format!("**Verification failed:** {reason}"))
        }
        Some(ProofStatus::NotAttempted) => Some("**Not yet verified.**".into()),
        Some(ProofStatus::Assumed) => Some("**Assumed** (treated as an axiom).".into()),
        _ => None,
    }
}

/// Render an expanded "Verification failure" callout for a `Failed` status.
pub(super) fn render_failure_details_callout(status: &Option<ProofStatus>) -> Option<String> {
    let (reason, counterexample, log_excerpt) = match status {
        Some(ProofStatus::Failed {
            reason,
            counterexample,
            log_excerpt,
            ..
        }) => (
            reason.as_str(),
            counterexample.as_deref(),
            log_excerpt.as_deref(),
        ),
        _ => return None,
    };
    if counterexample.is_none() && log_excerpt.is_none() {
        return None;
    }

    let mut out = String::new();
    let _ = writeln!(out, "> **Why this failed** — {reason}.");
    out.push('\n');
    if let Some(cx) = counterexample {
        let _ = writeln!(out, "<details><summary>Counterexample</summary>\n");
        let _ = writeln!(out, "```text\n{}\n```\n", cx.trim_end());
        let _ = writeln!(out, "</details>\n");
    }
    if let Some(log) = log_excerpt {
        let _ = writeln!(out, "<details><summary>Verifier log excerpt</summary>\n");
        let _ = writeln!(out, "```text\n{}\n```\n", log.trim_end());
        let _ = writeln!(out, "</details>\n");
    }
    Some(out)
}

/// Render a "Verify this yourself" section.
pub(super) fn render_verify_command_section(status: &Option<ProofStatus>) -> Option<String> {
    let (verify_command, verify_script) = match status {
        Some(ProofStatus::Proven {
            verify_command,
            verify_script,
            ..
        })
        | Some(ProofStatus::Failed {
            verify_command,
            verify_script,
            ..
        }) => (verify_command.as_deref(), verify_script.as_deref()),
        _ => return None,
    };
    if verify_command.is_none() && verify_script.is_none() {
        return None;
    }

    let mut out = String::new();
    let _ = writeln!(out, "### Verify this yourself\n");
    let command = verify_command
        .map(|s| s.to_string())
        .or_else(|| verify_script.map(|path| format!("saw \"{path}\"")));
    if let Some(cmd) = command {
        let _ = writeln!(out, "Re-run the proof locally:\n");
        let _ = writeln!(out, "```sh\n{}\n```\n", cmd.trim());
    }
    if let Some(script) = verify_script {
        if verify_command
            .map(|cmd| !cmd.contains(script))
            .unwrap_or(true)
        {
            let _ = writeln!(out, "Script: `{script}`\n");
        }
    }
    Some(out)
}

/// Render an expanded "Proof details" blockquote for `Proven` statuses that
/// carry override or bounded-loop metadata.
pub(super) fn render_proof_details_callout(status: &Option<ProofStatus>) -> Option<String> {
    let (solver, time_secs, overrides, iterations) = match status {
        Some(ProofStatus::Proven {
            solver,
            time_secs,
            overrides,
            iterations,
            ..
        }) => (
            solver.as_str(),
            *time_secs,
            overrides.as_slice(),
            *iterations,
        ),
        _ => return None,
    };
    if overrides.is_empty() && iterations.is_none() {
        return None;
    }

    let mut out = String::new();
    let _ = writeln!(out, "> **Proof details** — discharged with `{solver}`.");
    if let Some(n) = iterations {
        let _ = writeln!(out, ">");
        let plural = if n == 1 { "iteration" } else { "iterations" };
        let _ = writeln!(
            out,
            "> Bounded-loop proof: validated for **{n} loop {plural}**. Inputs that exercise the loop more times than this fall outside the proof's scope."
        );
    }
    if !overrides.is_empty() {
        let _ = writeln!(out, ">");
        let plural = if overrides.len() == 1 {
            "override"
        } else {
            "overrides"
        };
        let _ = writeln!(
            out,
            "> Used {n} {plural} — each is **trusted** to behave per its spec and not re-verified here:",
            n = overrides.len()
        );
        for o in overrides {
            let _ = writeln!(out, "> - `{o}`");
        }
    }
    if let Some(t) = time_secs {
        let _ = writeln!(out, ">");
        let _ = writeln!(out, "> Solver wall-clock: {t:.2}s.");
    }
    out.push('\n');
    Some(out)
}

/// Find functions and types referenced in a property body/doc, returning
/// markdown links.
pub(super) fn find_involved_symbols(
    body: &str,
    doc: &[String],
    symbols: &SymbolTable,
    current_file: &str,
) -> Vec<String> {
    use crate::linker::contains_word;

    let all_text = format!("{body} {}", doc.join(" "));
    let mut involved: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let mut syms: Vec<(&String, &(String, String))> = symbols.symbols.iter().collect();
    syms.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));

    for (name, (target_file, anchor)) in &syms {
        if current_file == *target_file {
            continue;
        }
        if name.len() <= 3 && name.starts_with('P') && name[1..].chars().all(|c| c.is_ascii_digit())
        {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_badge_rendering() {
        assert_eq!(
            proof_badge(&Some(ProofStatus::Proven {
                solver: "z3".into(),
                time_secs: Some(0.42),
                overrides: vec![],
                iterations: None,
                verify_command: None,
                verify_script: None,
            })),
            "✓"
        );
        assert_eq!(
            proof_badge(&Some(ProofStatus::Failed {
                reason: "counterexample".into(),
                counterexample: None,
                log_excerpt: None,
                verify_command: None,
                verify_script: None,
            })),
            "✗"
        );
        assert_eq!(proof_badge(&Some(ProofStatus::Assumed)), "~");
        assert_eq!(proof_badge(&Some(ProofStatus::NotAttempted)), "✗");
        assert_eq!(proof_badge(&None), "");
    }

    #[test]
    fn proof_details_callout_omitted_without_extras() {
        let status = Some(ProofStatus::Proven {
            solver: "z3".into(),
            time_secs: Some(1.0),
            overrides: vec![],
            iterations: None,
            verify_command: None,
            verify_script: None,
        });
        assert!(render_proof_details_callout(&status).is_none());

        assert!(render_proof_details_callout(&Some(ProofStatus::Assumed)).is_none());
        assert!(
            render_proof_details_callout(&Some(ProofStatus::Failed {
                reason: "x".into(),
                counterexample: None,
                log_excerpt: None,
                verify_command: None,
                verify_script: None,
            }))
            .is_none()
        );
        assert!(render_proof_details_callout(&None).is_none());
    }

    #[test]
    fn proof_details_callout_reports_overrides_and_iterations() {
        let status = Some(ProofStatus::Proven {
            solver: "z3".into(),
            time_secs: Some(12.3),
            overrides: vec!["memcpy".into(), "operator new".into()],
            iterations: Some(4),
            verify_command: None,
            verify_script: None,
        });
        let out = render_proof_details_callout(&status).expect("callout present");
        assert!(out.contains("Proof details"), "header missing: {out}");
        assert!(out.contains("`z3`"), "solver missing: {out}");
        assert!(
            out.contains("**4 loop iterations**"),
            "iterations missing: {out}"
        );
        assert!(out.contains("2 overrides"), "override count missing: {out}");
        assert!(out.contains("`memcpy`"), "override name missing: {out}");
        assert!(out.contains("`operator new`"), "override name missing: {out}");
        assert!(out.contains("12.30s"), "wall-clock missing: {out}");
    }

    #[test]
    fn proof_details_callout_singular_iteration() {
        let status = Some(ProofStatus::Proven {
            solver: "z3".into(),
            time_secs: None,
            overrides: vec![],
            iterations: Some(1),
            verify_command: None,
            verify_script: None,
        });
        let out = render_proof_details_callout(&status).expect("callout present");
        assert!(
            out.contains("**1 loop iteration**"),
            "singular missing: {out}"
        );
        assert!(!out.contains("iterations**"), "incorrect plural: {out}");
    }

    #[test]
    fn failure_details_callout_omitted_without_diagnostics() {
        let status = Some(ProofStatus::Failed {
            reason: "error during verification".into(),
            counterexample: None,
            log_excerpt: None,
            verify_command: None,
            verify_script: None,
        });
        assert!(render_failure_details_callout(&status).is_none());

        assert!(
            render_failure_details_callout(&Some(ProofStatus::Proven {
                solver: "z3".into(),
                time_secs: None,
                overrides: vec![],
                iterations: None,
                verify_command: None,
                verify_script: None,
            }))
            .is_none()
        );
        assert!(render_failure_details_callout(&None).is_none());
    }

    #[test]
    fn failure_details_callout_renders_counterexample_and_log() {
        let status = Some(ProofStatus::Failed {
            reason: "counterexample found".into(),
            counterexample: Some("x = 0\ny = 1\n".into()),
            log_excerpt: Some("LLVM verification failed at line 42".into()),
            verify_command: None,
            verify_script: None,
        });
        let out = render_failure_details_callout(&status).expect("callout present");
        assert!(out.contains("Why this failed"), "header missing: {out}");
        assert!(
            out.contains("counterexample found"),
            "reason missing: {out}"
        );
        assert!(
            out.contains("<details><summary>Counterexample</summary>"),
            "counterexample fold missing: {out}"
        );
        assert!(out.contains("x = 0"), "counterexample body missing: {out}");
        assert!(
            out.contains("<details><summary>Verifier log excerpt</summary>"),
            "log fold missing: {out}"
        );
        assert!(out.contains("line 42"), "log body missing: {out}");
    }

    #[test]
    fn intentional_counterexample_detection() {
        let doc = vec![
            "P99: \"some tempting but wrong claim.\"".to_string(),
            "".to_string(),
            "EXPECTED VERDICT: FAILS.".to_string(),
            "Counterexample: x = 0.".to_string(),
        ];
        assert!(is_intentional_counterexample(&doc));

        let pass_doc = vec![
            "P1: \"a real safety claim.\"".to_string(),
            "EXPECTED VERDICT: PASS.".to_string(),
        ];
        assert!(!is_intentional_counterexample(&pass_doc));

        assert!(!is_intentional_counterexample(&[]));
    }
}
