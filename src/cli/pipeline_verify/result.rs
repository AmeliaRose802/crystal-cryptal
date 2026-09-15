use std::path::Path;
use std::process::Output;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResultKind {
    Verified,
    ProofFailed,
    NotAttempted,
    Unknown,
}

pub(super) struct ParsedResult {
    pub kind: ResultKind,
    pub text: String,
}

pub(super) fn read_result(out_dir: &Path) -> Result<ParsedResult, String> {
    let path = out_dir.join("result.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
    let raw = value
        .get("status")
        .or_else(|| value.get("verdict"))
        .and_then(|value| value.as_str())
        .ok_or_else(|| format!("{} has no string status or verdict", path.display()))?;
    let kind = match raw.to_ascii_lowercase().as_str() {
        "verified" | "q.e.d." | "valid" | "equivalent" => ResultKind::Verified,
        "counterexample" | "disproved" | "not equivalent" | "invalid" | "sat" | "timeout"
        | "failed" => ResultKind::ProofFailed,
        "not_attempted" | "not-attempted" | "not attempted" | "not_run" => ResultKind::NotAttempted,
        "unknown" => ResultKind::Unknown,
        "error" => ResultKind::ProofFailed,
        other => {
            return Err(format!(
                "{} has unrecognized result '{other}'",
                path.display()
            ));
        }
    };
    Ok(ParsedResult { kind, text })
}

pub(super) fn remove_stale_output(out_dir: &Path) -> Result<(), String> {
    if out_dir.exists() {
        std::fs::remove_dir_all(out_dir)
            .map_err(|e| format!("cannot remove stale output {}: {e}", out_dir.display()))?;
    }
    Ok(())
}

pub(super) fn restore_result(out_dir: &Path, text: &str) {
    if let Err(e) = std::fs::create_dir_all(out_dir)
        .and_then(|()| std::fs::write(out_dir.join("result.json"), text))
    {
        eprintln!(
            "warning: cannot restore not-attempted result under {}: {e}",
            out_dir.display()
        );
    }
}

/// Add the subprocess transcript and reproduction command to a verifier result.
/// saw-spec-gen sometimes reports only `error during verification` in JSON while
/// the actionable type/parser error is present on stderr, so derive the concise
/// summary from the complete captured diagnostic.
pub(super) fn enrich_result(out_dir: &Path, output: &Output, program: &str, argv: &[String]) {
    let path = out_dir.join("result.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return;
    };
    let Some(object) = value.as_object_mut() else {
        return;
    };

    let diagnostic = subprocess_diagnostic(output);
    if !diagnostic.is_empty() {
        let current = object
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if let Some(summary) = first_actionable_error(&diagnostic)
            && (is_generic_summary(current)
                || diagnostic_score(&summary) > diagnostic_score(current))
        {
            object.insert("message".into(), serde_json::json!(summary));
        }
        object.insert("log_excerpt".into(), serde_json::json!(diagnostic));
    }
    object.entry("verify_command").or_insert_with(|| {
        serde_json::json!(normalize_machine_paths(&format_command(program, argv)))
    });
    if (!object.contains_key("verify_script") || !object.contains_key("proof_script"))
        && let Some(script) = find_generated_script(out_dir)
    {
        object
            .entry("verify_script")
            .or_insert_with(|| serde_json::json!(script.path));
        object
            .entry("proof_script")
            .or_insert_with(|| serde_json::json!(normalize_machine_paths(&script.contents)));
    }

    if let Ok(serialized) = serde_json::to_string_pretty(&value) {
        let _ = std::fs::write(path, format!("{serialized}\n"));
    }
}

pub(super) fn subprocess_diagnostic(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = match (stderr.trim().is_empty(), stdout.trim().is_empty()) {
        (false, false) => format!("{}\n{}", stderr.trim_end(), stdout.trim_end()),
        (false, true) => stderr.into_owned(),
        (true, false) => stdout.into_owned(),
        (true, true) => String::new(),
    };
    normalize_machine_paths(combined.trim())
}

fn is_generic_summary(message: &str) -> bool {
    let normalized = message.trim().to_ascii_lowercase();
    normalized.is_empty()
        || normalized == "error during verification"
        || normalized == "verification error"
        || normalized == "verification failed"
        || normalized == "unknown"
        || diagnostic_score(message) == 0
}

pub(super) fn first_actionable_error(diagnostic: &str) -> Option<String> {
    let mut best: Option<(u8, String)> = None;
    for line in diagnostic.lines() {
        let candidate = line
            .trim()
            .trim_start_matches("Error:")
            .trim_start_matches("error:")
            .trim();
        if candidate.is_empty() || is_generic_summary(candidate) {
            continue;
        }
        let score = diagnostic_score(candidate);
        if score > 0
            && best
                .as_ref()
                .is_none_or(|(best_score, _)| score > *best_score)
        {
            best = Some((score, candidate.to_string()));
        }
    }
    best.map(|(_, message)| message)
}

fn diagnostic_score(candidate: &str) -> u8 {
    let lower = candidate.to_ascii_lowercase();
    if lower.starts_with("loading file")
        || lower.starts_with("cryptol: [error] at")
        || lower.starts_with("saw: [error] at")
        || lower.starts_with("at ")
        || lower == "stack trace:"
    {
        return 0;
    }
    if [
        "could not find definition",
        "type mismatch",
        "unsupported type",
        "unknown type alias",
        "incompatible types",
        "width mismatch",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return 4;
    }
    if ["expected", "counterexample", "failed", "error", "panic"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return 3;
    }
    1
}

fn normalize_machine_paths(text: &str) -> String {
    let Ok(cwd) = std::env::current_dir() else {
        return text.replace('\\', "/");
    };
    let cwd_native = cwd.to_string_lossy();
    let cwd_slashes = cwd_native.replace('\\', "/");
    text.replace(cwd_native.as_ref(), ".")
        .replace(&cwd_slashes, ".")
        .replace('\\', "/")
}

fn format_command(program: &str, argv: &[String]) -> String {
    std::iter::once(program)
        .chain(argv.iter().map(String::as_str))
        .map(|arg| {
            if arg.contains([' ', '\t', '"']) {
                format!("\"{}\"", arg.replace('"', "\\\""))
            } else {
                arg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

struct GeneratedScript {
    path: String,
    contents: String,
}

fn find_generated_script(out_dir: &Path) -> Option<GeneratedScript> {
    let entries = std::fs::read_dir(out_dir).ok()?;
    let mut paths: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        if path.extension().and_then(|extension| extension.to_str()) == Some("saw") {
            let contents = std::fs::read_to_string(&path).ok()?;
            return Some(GeneratedScript {
                path: normalize_machine_paths(&path.to_string_lossy()),
                contents,
            });
        }
    }
    None
}

pub(super) fn write_error_result(
    out_dir: &Path,
    function: &str,
    cryptol_fn: &str,
    impl_lang: &str,
    message: &str,
) {
    if let Err(e) = std::fs::create_dir_all(out_dir) {
        eprintln!("warning: cannot create {}: {e}", out_dir.display());
        return;
    }
    let diagnostic = normalize_machine_paths(message);
    let summary = first_actionable_error(&diagnostic)
        .unwrap_or_else(|| "saw-spec-gen produced no usable result".to_string());
    let json = serde_json::json!({
        "schema_version": "1",
        "side": impl_lang,
        "function": function,
        "cryptol_fn": cryptol_fn,
        "status": "error",
        "verdict": "UNKNOWN",
        "kind": "pipeline_invocation_error",
        "message": summary,
        "log_excerpt": diagnostic,
    });
    let dest = out_dir.join("result.json");
    let text = serde_json::to_string_pretty(&json).unwrap_or_default() + "\n";
    if let Err(e) = std::fs::write(&dest, text) {
        eprintln!("warning: cannot write {}: {e}", dest.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actionable_error_skips_generic_wrapper() {
        let diagnostic = "error during verification\nError: unsupported type: %reference\nUnknown type alias Ident \\\"reference\\\"";
        assert_eq!(
            first_actionable_error(diagnostic).as_deref(),
            Some("unsupported type: %reference")
        );
    }

    #[test]
    fn actionable_error_skips_loading_and_location_headers() {
        let diagnostic = "Loading file \"verify.saw\"\nCryptol: [error] at verify.saw:8:1\nCould not find definition of UnknownAlias";
        assert_eq!(
            first_actionable_error(diagnostic).as_deref(),
            Some("Could not find definition of UnknownAlias")
        );

        let mismatch =
            "Cryptol: [error] at verify.saw:12:4\nType mismatch: expected [256], found [16]";
        assert_eq!(
            first_actionable_error(mismatch).as_deref(),
            Some("Type mismatch: expected [256], found [16]")
        );
    }
}
