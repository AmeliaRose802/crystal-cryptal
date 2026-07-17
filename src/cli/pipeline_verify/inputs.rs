use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(super) fn expand_impl_files(
    inputs: &[PathBuf],
    impl_lang: &str,
) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for input in inputs {
        if input.is_file() {
            files.push(input.clone());
        } else if input.is_dir() {
            collect_source_files(input, impl_lang, &mut files)?;
        } else {
            return Err(format!(
                "implementation path does not exist: {}",
                input.display()
            ));
        }
    }

    let mut unique = HashSet::new();
    let mut canonical = Vec::new();
    for file in files {
        let file = std::fs::canonicalize(&file)
            .map_err(|e| format!("cannot resolve implementation {}: {e}", file.display()))?;
        if unique.insert(file.clone()) {
            canonical.push(file);
        }
    }
    canonical.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase());

    if canonical.is_empty() {
        return Err(format!(
            "no {impl_lang} implementation files found in the supplied --impl paths"
        ));
    }
    Ok(canonical)
}

fn collect_source_files(
    dir: &Path,
    impl_lang: &str,
    files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| {
        format!(
            "cannot read implementation directory {}: {e}",
            dir.display()
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read entry under {}: {e}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| format!("cannot inspect {}: {e}", path.display()))?;
        if file_type.is_dir() {
            collect_source_files(&path, impl_lang, files)?;
        } else if file_type.is_file() && has_source_extension(&path, impl_lang) {
            files.push(path);
        }
    }
    Ok(())
}

fn has_source_extension(path: &Path, impl_lang: &str) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if impl_lang == "rust" {
        extension == "rs"
    } else {
        matches!(extension.as_str(), "c" | "cc" | "cpp" | "cxx" | "c++")
    }
}
