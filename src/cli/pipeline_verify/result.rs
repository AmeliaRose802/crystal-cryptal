use std::path::Path;

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
        "error" => {
            let message = value
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("verification error");
            return Err(format!("{} reports an error: {message}", path.display()));
        }
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
    let json = serde_json::json!({
        "schema_version": "1",
        "side": impl_lang,
        "function": function,
        "cryptol_fn": cryptol_fn,
        "status": "error",
        "verdict": "UNKNOWN",
        "kind": "pipeline_invocation_error",
        "message": message,
    });
    let dest = out_dir.join("result.json");
    let text = serde_json::to_string_pretty(&json).unwrap_or_default() + "\n";
    if let Err(e) = std::fs::write(&dest, text) {
        eprintln!("warning: cannot write {}: {e}", dest.display());
    }
}
