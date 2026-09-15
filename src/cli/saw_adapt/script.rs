use std::path::Path;

use super::diagnostic::normalize_machine_paths;

/// Older saw-spec-gen results record only a path (or no script metadata at
/// all).  Capture a sibling `.saw` artifact while adapting so the published
/// manifest remains self-contained and the renderer can explain it later.
pub(super) fn attach_proof_script(value: &mut serde_json::Value, result_path: &Path) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if object.contains_key("proof_script") {
        return;
    }
    let Some(parent) = result_path.parent() else {
        return;
    };

    let preferred = object
        .get("verify_script")
        .and_then(|path| path.as_str())
        .and_then(|path| resolve_recorded_script(parent, path));
    let script_path = preferred.or_else(|| first_saw_file(parent));
    let Some(script_path) = script_path else {
        return;
    };
    let Ok(contents) = std::fs::read_to_string(&script_path) else {
        return;
    };
    object.insert(
        "proof_script".into(),
        serde_json::json!(normalize_machine_paths(&contents)),
    );
    object.entry("verify_script").or_insert_with(|| {
        serde_json::json!(normalize_machine_paths(&script_path.to_string_lossy()))
    });
}

fn resolve_recorded_script(parent: &Path, recorded: &str) -> Option<std::path::PathBuf> {
    let path = Path::new(recorded);
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    let file_name = path.file_name()?;
    let sibling = parent.join(file_name);
    sibling.is_file().then_some(sibling)
}

fn first_saw_file(parent: &Path) -> Option<std::path::PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(parent)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("saw"))
        .collect();
    paths.sort();
    paths.into_iter().next()
}
