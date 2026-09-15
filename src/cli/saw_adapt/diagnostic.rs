// Extract concise root causes and normalize verifier diagnostics.

pub(super) fn best_failure_reason(
    message: Option<&str>,
    diagnostic: Option<&str>,
) -> Option<String> {
    let existing = message
        .filter(|message| !is_generic_failure(message))
        .map(|message| (diagnostic_score(message), message.to_string()));
    let extracted = diagnostic
        .and_then(first_actionable_diagnostic)
        .map(|message| (diagnostic_score(&message), message));
    match (existing, extracted) {
        (Some(left), Some(right)) => Some(if right.0 > left.0 { right.1 } else { left.1 }),
        (Some((_, message)), None) | (None, Some((_, message))) => Some(message),
        (None, None) => None,
    }
}

fn is_generic_failure(message: &str) -> bool {
    diagnostic_score(message) == 0
        || matches!(
            message.trim().to_ascii_lowercase().as_str(),
            "" | "error during verification"
                | "verification error"
                | "verification failed"
                | "unknown"
        )
}

fn first_actionable_diagnostic(diagnostic: &str) -> Option<String> {
    let mut best: Option<(u8, String)> = None;
    for line in diagnostic.lines() {
        let candidate = line
            .trim()
            .trim_start_matches("Error:")
            .trim_start_matches("error:")
            .trim();
        if candidate.is_empty() || is_generic_failure(candidate) {
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

pub(super) fn normalize_machine_paths(text: &str) -> String {
    let Ok(cwd) = std::env::current_dir() else {
        return text.replace('\\', "/");
    };
    let cwd_native = cwd.to_string_lossy();
    let cwd_slashes = cwd_native.replace('\\', "/");
    text.replace(cwd_native.as_ref(), ".")
        .replace(&cwd_slashes, ".")
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_root_cause_over_loading_and_location_headers() {
        let diagnostic = "Loading file verify.saw\nCryptol: [error] at verify.saw:8:1\nType mismatch: expected [256], found [16]";
        assert_eq!(
            best_failure_reason(Some("Loading file verify.saw"), Some(diagnostic)).as_deref(),
            Some("Type mismatch: expected [256], found [16]")
        );
    }
}
