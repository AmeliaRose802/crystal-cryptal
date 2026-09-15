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
    let normalized = text.replace('\\', "/").replace("//?/", "");
    let Ok(cwd) = std::env::current_dir() else {
        return normalized;
    };
    let cwd_slashes = cwd.to_string_lossy().replace('\\', "/");
    normalized.replace(&cwd_slashes, ".")
}

pub(super) fn normalize_verify_command(command: &str) -> String {
    let normalized = normalize_machine_paths(command).replace("//?/./", "");
    let start = normalized
        .find(" verify-cpp ")
        .or_else(|| normalized.find(" verify-rust "));
    let mut portable = start.map_or(normalized.clone(), |index| {
        format!("saw-spec-gen{}", &normalized[index..])
    });
    if let Some(spec) = command_option(&portable, "--cryptol-spec") {
        let config = std::path::Path::new(&spec).with_extension("toml");
        if config.is_file() {
            let replacement = format!("--config={}", config.to_string_lossy().replace('\\', "/"));
            if let Some(config_start) = portable.find("--config=") {
                let config_end = portable[config_start..]
                    .find(char::is_whitespace)
                    .map_or(portable.len(), |offset| config_start + offset);
                portable.replace_range(config_start..config_end, &replacement);
            }
        }
    }
    portable
        .split_whitespace()
        .map(|argument| argument.trim_start_matches("./"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn command_option(command: &str, option: &str) -> Option<String> {
    let arguments: Vec<_> = command.split_whitespace().collect();
    arguments.iter().enumerate().find_map(|(index, argument)| {
        argument
            .strip_prefix(&format!("{option}="))
            .map(str::to_string)
            .or_else(|| {
                (*argument == option)
                    .then(|| arguments.get(index + 1).copied())
                    .flatten()
                    .map(str::to_string)
            })
    })
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

    #[test]
    fn verifier_command_drops_machine_executable_and_extended_paths() {
        let command = "C:/Users/person/bin/saw-spec-gen.exe verify-cpp --cpp-file //?/./cpp/src/key.cpp --cryptol-spec cpp/saw/S.cry";
        assert_eq!(
            normalize_verify_command(command),
            "saw-spec-gen verify-cpp --cpp-file cpp/src/key.cpp --cryptol-spec cpp/saw/S.cry"
        );
    }
}
