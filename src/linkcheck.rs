//! Generation-time link integrity checking.
//!
//! After Markdown is rendered, this module walks the output tree and verifies
//! that every local (relative) link points at a file that actually exists.
//! External links (`http`, `https`, `mailto`) and pure in-page anchors are
//! ignored. This catches broken cross-file references before they ship.

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

/// A single dead link discovered during checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadLink {
    /// The Markdown file containing the link, relative to the scanned root.
    pub source: String,
    /// The raw link target as written in the document.
    pub target: String,
}

impl std::fmt::Display for DeadLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} -> {}", self.source, self.target)
    }
}

fn is_external(target: &str) -> bool {
    target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
        || target.starts_with('#')
}

/// Recursively collect every `*.md` file under `root`.
fn collect_markdown(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(path);
            }
        }
    }
    out
}

/// Scan `root` for dead local Markdown links. Returns a list sorted by source
/// path for deterministic reporting.
pub fn find_dead_links(root: &Path) -> Vec<DeadLink> {
    // Matches Markdown inline links: [text](target). Targets may contain an
    // optional #anchor and may be wrapped in <...> (rare). We deliberately
    // skip image embeds handled the same way since the target rules match.
    let re = Regex::new(r"\]\(([^)\s]+)\)").expect("valid link regex");
    let mut dead = Vec::new();

    for file in collect_markdown(root) {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let base = file.parent().unwrap_or(root);
        for cap in re.captures_iter(&text) {
            let raw = &cap[1];
            if is_external(raw) {
                continue;
            }
            // Strip any in-page anchor; only the file part must exist.
            let path_part = raw.split('#').next().unwrap_or(raw);
            if path_part.is_empty() {
                continue; // pure anchor like (#foo)
            }
            let candidate = base.join(path_part);
            if !candidate.exists() {
                dead.push(DeadLink {
                    source: file
                        .strip_prefix(root)
                        .unwrap_or(&file)
                        .to_string_lossy()
                        .replace('\\', "/"),
                    target: raw.to_string(),
                });
            }
        }
    }

    dead.sort_by(|a, b| a.source.cmp(&b.source).then(a.target.cmp(&b.target)));
    dead
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detects_dead_and_ignores_external() {
        let dir = std::env::temp_dir().join(format!("linkcheck_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("functions")).unwrap();
        fs::write(dir.join("types.md"), "# Types\n").unwrap();
        fs::write(
            dir.join("index.md"),
            "[ok](types.md)\n[ext](https://example.com)\n[anchor](#x)\n[dead](missing.md)\n[deep](functions/nope.md#a)\n",
        )
        .unwrap();

        let dead = find_dead_links(&dir);
        let targets: Vec<&str> = dead.iter().map(|d| d.target.as_str()).collect();
        assert!(targets.contains(&"missing.md"), "missing.md should be dead");
        assert!(
            targets.contains(&"functions/nope.md#a"),
            "deep dead link should be reported"
        );
        assert_eq!(dead.len(), 2, "only the two dead links should be reported");

        let _ = fs::remove_dir_all(&dir);
    }
}
