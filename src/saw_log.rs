//! Parser for raw SAW `prove_print` / `prove` console output.
//!
//! Supports:
//! - Timestamp-prefixed lines: `[HH:MM:SS.mmm] Proving propName ...`
//! - Inline verdicts: `propName: Q.E.D.` or `propName: Counterexample`
//! - Lookahead verdicts: `Proving propName ...` followed by `Q.E.D.` on a nearby line

use crate::ir::ProofStatus;

#[derive(Debug, PartialEq)]
pub struct SawLogRecord {
    pub name: String,
    pub status: ProofStatus,
}

/// Parse a raw SAW log text and return one record per proven/disproven property.
pub fn parse_saw_log(text: &str) -> Vec<SawLogRecord> {
    let lines: Vec<&str> = text.lines().collect();
    let mut records = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let raw = lines[i];
        let line = strip_timestamp(raw);

        // ── Inline form: "name: Q.E.D." or "name: Counterexample" ──────────
        if let Some(record) = try_parse_inline(line) {
            records.push(record);
            i += 1;
            continue;
        }

        // ── Pending form: "Proving name ..." then result on later line ──────
        if let Some(name) = try_extract_pending_name(line) {
            let mut j = i + 1;
            let mut found = false;
            while j < lines.len() {
                let next_raw = lines[j];
                let next = strip_timestamp(next_raw);
                if let Some(status) = try_parse_verdict(next) {
                    records.push(SawLogRecord { name, status });
                    i = j + 1;
                    found = true;
                    break;
                } else if is_ignorable(next_raw) {
                    j += 1;
                } else {
                    break;
                }
            }
            if !found {
                i += 1;
            }
            continue;
        }

        i += 1;
    }

    records
}

/// Strip an optional `[HH:MM:SS.mmm]` or `[HH:MM:SS]` timestamp prefix.
pub fn strip_timestamp(line: &str) -> &str {
    let line = line.trim_start();
    if line.starts_with('[') {
        if let Some(end) = line.find(']') {
            let bracket = &line[1..end];
            // Only strip if it looks like a time/date (contains ':' or only non-alpha chars)
            if bracket.contains(':') || bracket.chars().all(|c| !c.is_alphabetic()) {
                return line[end + 1..].trim_start();
            }
        }
    }
    line
}

/// Try to parse a verdict from a line.
pub fn try_parse_verdict(line: &str) -> Option<ProofStatus> {
    let trimmed = line.trim();
    if trimmed == "Q.E.D." || trimmed.starts_with("Q.E.D.") {
        Some(ProofStatus::Proven {
            solver: "saw".to_string(),
            time_secs: None,
        })
    } else if trimmed == "Valid" || trimmed == "Valid." {
        Some(ProofStatus::Proven {
            solver: "saw".to_string(),
            time_secs: None,
        })
    } else if trimmed.starts_with("Counterexample")
        || trimmed.starts_with("Invalid")
        || trimmed.starts_with("SAT")
    {
        Some(ProofStatus::Failed {
            reason: "counterexample found".to_string(),
        })
    } else {
        None
    }
}

/// Return true for lines that should be skipped while looking ahead for a verdict.
pub fn is_ignorable(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return true;
    }
    if t.starts_with("Running ") || t.starts_with("Checking ") || t.starts_with("Loading ") {
        return true;
    }
    // Indented continuation lines (solver output, counterexample witnesses)
    if line.starts_with("  ") || line.starts_with('\t') {
        return true;
    }
    false
}

/// Try to extract a pending property name from recognisable prefix patterns:
///   `Proving propName ...`
///   `Verifying propName ...`
///   `prove propName`
///   `Property: propName`
///   `Checking property propName`
pub fn try_extract_pending_name(line: &str) -> Option<String> {
    let prefixes: &[&str] = &[
        "Proving ",
        "Verifying ",
        "prove ",
        "Property: ",
        "property: ",
        "Checking property ",
        "prove_print ",
    ];
    for prefix in prefixes {
        if let Some(rest) = line.strip_prefix(prefix) {
            let name = rest
                .trim()
                .trim_end_matches("...")
                .trim_end_matches('.')
                .trim();
            if !name.is_empty() && is_valid_name(name) {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Return true if `s` looks like a plausible Cryptol property / function name.
pub fn is_valid_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 120 {
        return false;
    }
    let first = s.chars().next().unwrap_or(' ');
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    // Allow alphanumeric + _ + :: module qualifier + {{ }} Cryptol interpolation
    s.chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '_' | ':' | '{' | '}' | ' '))
        && !s.contains("  ") // double space would indicate a sentence
}

/// Try to parse an inline SAW result: `name: Q.E.D.` or `name: Counterexample`.
fn try_parse_inline(line: &str) -> Option<SawLogRecord> {
    let colon_pos = line.find(": ")?;
    let name_part = line[..colon_pos].trim();
    let verdict_part = line[colon_pos + 2..].trim();
    if !is_valid_name(name_part) {
        return None;
    }
    let status = try_parse_verdict(verdict_part)?;
    Some(SawLogRecord {
        name: name_part.to_string(),
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::ProofStatus;

    fn proven() -> ProofStatus {
        ProofStatus::Proven {
            solver: "saw".to_string(),
            time_secs: None,
        }
    }
    fn failed() -> ProofStatus {
        ProofStatus::Failed {
            reason: "counterexample found".to_string(),
        }
    }

    #[test]
    fn inline_qed() {
        let log = "P1: Q.E.D.\nP2: Counterexample\n  x = 0\n";
        let recs = parse_saw_log(log);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].name, "P1");
        assert_eq!(recs[0].status, proven());
        assert_eq!(recs[1].name, "P2");
        assert_eq!(recs[1].status, failed());
    }

    #[test]
    fn proving_prefix_with_timestamp() {
        let log = "[14:23:01.456] Proving P1 ...\nQ.E.D.\n[14:23:01.789] Proving P2 ...\nCounterexample\n  x = 0\n";
        let recs = parse_saw_log(log);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].name, "P1");
        assert_eq!(recs[0].status, proven());
        assert_eq!(recs[1].name, "P2");
        assert_eq!(recs[1].status, failed());
    }

    #[test]
    fn mixed_formats() {
        let log = "Proving P1 ...\nQ.E.D.\nP2: Q.E.D.\n[00:00:00.001] Proving P3 ...\n\nQ.E.D.\n";
        let recs = parse_saw_log(log);
        assert_eq!(recs.len(), 3);
        assert!(recs.iter().all(|r| r.status == proven()));
    }

    #[test]
    fn verifying_prefix() {
        let log = "Verifying hmacSha256 ...\nQ.E.D.\n";
        let recs = parse_saw_log(log);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].name, "hmacSha256");
    }

    #[test]
    fn valid_verdict_alias() {
        let log = "P1: Valid\n";
        let recs = parse_saw_log(log);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].status, proven());
    }

    #[test]
    fn sat_verdict_alias() {
        let log = "Proving P1 ...\nSAT\n";
        let recs = parse_saw_log(log);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].status, failed());
    }

    #[test]
    fn empty_log() {
        assert!(parse_saw_log("").is_empty());
        assert!(parse_saw_log("\n\n\n").is_empty());
    }

    #[test]
    fn ignores_non_property_lines() {
        let log = "Loading module Cryptol\nRunning Z3...\nP1: Q.E.D.\n";
        let recs = parse_saw_log(log);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].name, "P1");
    }

    #[test]
    fn module_qualified_name() {
        let log = "Proving SDEP::P1 ...\nQ.E.D.\n";
        let recs = parse_saw_log(log);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].name, "SDEP::P1");
    }
}
