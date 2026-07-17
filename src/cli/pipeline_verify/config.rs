use std::path::{Path, PathBuf};

use super::PipelineArgs;

pub(super) fn prepare_config(spec: &Path, args: &PipelineArgs) -> Result<PathBuf, String> {
    let base_config = match &args.saw_spec_gen_config {
        Some(path) => Some(path.clone()),
        None => discover_config(spec),
    };
    let mut config = match &base_config {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read saw-spec-gen config {}: {e}", path.display()))?;
            toml::from_str::<toml::Table>(&text)
                .map_err(|e| format!("cannot parse saw-spec-gen config {}: {e}", path.display()))?
        }
        None => toml::Table::new(),
    };

    let soft_missing = !args.strict_on_missing;
    config.insert(
        "spec_only_on_missing".into(),
        toml::Value::Boolean(soft_missing),
    );
    if !soft_missing && let Some(toml::Value::Table(functions)) = config.get_mut("functions") {
        for (_, function) in functions.iter_mut() {
            if let toml::Value::Table(function) = function {
                function.insert("spec_only_on_missing".into(), toml::Value::Boolean(false));
            }
        }
    }

    std::fs::create_dir_all(&args.verify_output).map_err(|e| {
        format!(
            "cannot create verification directory {}: {e}",
            args.verify_output.display()
        )
    })?;
    let generated = args.verify_output.join("pretty-specs-saw-spec-gen.toml");
    let mut text = toml::to_string_pretty(&config)
        .map_err(|e| format!("cannot serialize saw-spec-gen config: {e}"))?;
    text.push('\n');
    std::fs::write(&generated, text).map_err(|e| {
        format!(
            "cannot write saw-spec-gen config {}: {e}",
            generated.display()
        )
    })?;
    let generated = std::fs::canonicalize(&generated).map_err(|e| {
        format!(
            "cannot resolve generated config {}: {e}",
            generated.display()
        )
    })?;

    match base_config {
        Some(base) => eprintln!(
            "  saw-spec-gen config: {} (generated from {})",
            generated.display(),
            base.display()
        ),
        None => eprintln!("  saw-spec-gen config: {}", generated.display()),
    }
    Ok(generated)
}

fn discover_config(spec: &Path) -> Option<PathBuf> {
    let sibling = spec.with_extension("toml");
    if sibling.is_file() {
        return Some(sibling);
    }
    if let Some(parent) = spec.parent()
        && let Some(config) = find_config_upward(parent)
    {
        return Some(config);
    }
    std::env::current_dir()
        .ok()
        .and_then(|cwd| find_config_upward(&cwd))
}

fn find_config_upward(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join("saw-spec-gen.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}
