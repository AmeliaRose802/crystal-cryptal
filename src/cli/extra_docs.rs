// `--extra-docs` handling: copy hand-written Markdown directories into the
// output tree and patch the homepage / toc to link them.

use std::path::{Path, PathBuf};

/// A resolved extra-docs entry, ready to be wired into the top-level toc.
pub(crate) struct ExtraDocsEntry {
    pub(crate) display_name: String,
    /// Optional toc-target href (`<basename>/toc.yml` or `<basename>/index.md`).
    /// `None` when the source dir has no obvious entry point — files are
    /// still copied so docfx picks them up via its content glob.
    pub(crate) href: Option<String>,
    /// Optional Markdown-friendly href (`<basename>/index.md`) used when
    /// linking from the generated homepage. `None` when the source dir has
    /// no `index.md` at its root.
    pub(crate) md_href: Option<String>,
}

/// Parse a `--extra-docs` argument of the form `DIR` or `DIR:Display Name`.
/// Returns `(dir, display_name_override)`. Skips the drive-letter colon on
/// Windows paths like `C:\foo` so it isn't mistaken for the name separator.
fn parse_extra_docs_arg(arg: &str) -> (PathBuf, Option<String>) {
    let bytes = arg.as_bytes();
    let search_start = if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        2
    } else {
        0
    };
    if let Some(idx) = arg[search_start..].find(':') {
        let split = search_start + idx;
        let dir = &arg[..split];
        let name = arg[split + 1..].trim();
        if !name.is_empty() {
            return (PathBuf::from(dir), Some(name.to_string()));
        }
    }
    (PathBuf::from(arg), None)
}

/// Recursively copy every file under `src` into `dest`, preserving directory
/// structure. Skips hidden entries (names starting with `.`). Returns the
/// number of files copied.
fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<usize> {
    let mut copied = 0usize;
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        let src_path = entry.path();
        let dest_path = dest.join(&name);
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copied += copy_dir_recursive(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&src_path, &dest_path)?;
            copied += 1;
        }
        // Symlinks and other entry kinds are skipped.
    }
    Ok(copied)
}

/// Title-case a directory basename for use as a toc label. Splits on `-`,
/// `_`, and whitespace and uppercases each word's first letter.
fn humanize_basename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut first = true;
    for word in name.split(|c: char| c == '-' || c == '_' || c.is_whitespace()) {
        if word.is_empty() {
            continue;
        }
        if !first {
            out.push(' ');
        }
        first = false;
        let mut chars = word.chars();
        if let Some(c) = chars.next() {
            for u in c.to_uppercase() {
                out.push(u);
            }
        }
        out.push_str(chars.as_str());
    }
    if out.is_empty() {
        name.to_string()
    } else {
        out
    }
}

/// Copy each `--extra-docs` directory into `<output>/<basename>/` and
/// return the resolved toc entries in the order given on the command line.
/// Warnings (not errors) are emitted for missing or unreadable directories.
fn copy_extra_docs(output: &Path, extra_docs: &[String]) -> Vec<ExtraDocsEntry> {
    let mut entries = Vec::new();
    for raw in extra_docs {
        let (dir, name_override) = parse_extra_docs_arg(raw);
        if !dir.is_dir() {
            eprintln!(
                "warning: --extra-docs {}: not a directory (skipped)",
                dir.display()
            );
            continue;
        }
        let basename = match dir.file_name().and_then(|s| s.to_str()) {
            Some(b) => b.to_string(),
            None => {
                eprintln!(
                    "warning: --extra-docs {}: cannot derive basename (skipped)",
                    dir.display()
                );
                continue;
            }
        };
        let dest = output.join(&basename);
        match copy_dir_recursive(&dir, &dest) {
            Ok(n) => eprintln!(
                "copied {n} file(s) from {} → {}",
                dir.display(),
                dest.display()
            ),
            Err(e) => {
                eprintln!(
                    "warning: --extra-docs {}: copy failed: {e} (skipped)",
                    dir.display()
                );
                continue;
            }
        }

        let md_href = if dest.join("index.md").is_file() {
            Some(format!("{basename}/index.md"))
        } else {
            None
        };
        let href = if dest.join("toc.yml").is_file() {
            Some(format!("{basename}/toc.yml"))
        } else if let Some(md) = &md_href {
            Some(md.clone())
        } else {
            eprintln!(
                "note: --extra-docs {}: no toc.yml or index.md at root; skipping toc entry",
                dir.display()
            );
            None
        };

        let display_name = name_override.unwrap_or_else(|| humanize_basename(&basename));
        entries.push(ExtraDocsEntry {
            display_name,
            href,
            md_href,
        });
    }
    entries
}

/// Append extra-docs entries to the top-level `toc.yml` at `<output>/toc.yml`.
/// No-op when there are no entries or when the toc.yml doesn't exist
/// (i.e. `--docfx` was not used).
fn append_extra_docs_to_toc(output: &Path, entries: &[ExtraDocsEntry]) {
    if entries.is_empty() {
        return;
    }
    let toc_path = output.join("toc.yml");
    if !toc_path.is_file() {
        return;
    }
    let mut existing = match std::fs::read_to_string(&toc_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "warning: cannot read {} to append --extra-docs entries: {e}",
                toc_path.display()
            );
            return;
        }
    };
    if !existing.ends_with('\n') {
        existing.push('\n');
    }
    for entry in entries {
        let Some(href) = &entry.href else { continue };
        existing.push_str(&format!(
            "- name: {}\n  href: {}\n",
            entry.display_name, href
        ));
    }
    if let Err(e) = std::fs::write(&toc_path, existing) {
        eprintln!("warning: cannot write updated {}: {e}", toc_path.display());
    }
}

/// Append an "Additional Documentation" section to `<output>/index.md`
/// linking each extra-docs entry that has a markdown landing page
/// (`<basename>/index.md`). No-op when there are no entries, no homepage
/// to patch, or none of the entries expose an `index.md`.
fn append_extra_docs_to_index(output: &Path, entries: &[ExtraDocsEntry]) {
    if entries.is_empty() {
        return;
    }
    let linkable: Vec<&ExtraDocsEntry> = entries.iter().filter(|e| e.md_href.is_some()).collect();
    if linkable.is_empty() {
        return;
    }
    let index_path = output.join("index.md");
    if !index_path.is_file() {
        return;
    }
    let mut existing = match std::fs::read_to_string(&index_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "warning: cannot read {} to add extra-docs links: {e}",
                index_path.display()
            );
            return;
        }
    };
    if !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str("\n## Additional Documentation\n\n");
    existing.push_str("This site ships with additional hand-written documentation:\n\n");
    for entry in linkable {
        let href = entry.md_href.as_deref().unwrap();
        existing.push_str(&format!("- [{}]({})\n", entry.display_name, href));
    }
    if let Err(e) = std::fs::write(&index_path, &existing) {
        eprintln!(
            "warning: cannot write {} with extra-docs links: {e}",
            index_path.display()
        );
    }
}

/// Copy `--extra-docs` directories, link them from the homepage, and
/// (in `--docfx` mode) patch the top-level `toc.yml` so the pages appear
/// in the navbar.
pub(crate) fn handle_extra_docs(output: &Path, extra_docs: &[String], docfx: bool) {
    if extra_docs.is_empty() {
        return;
    }
    let entries = copy_extra_docs(output, extra_docs);
    append_extra_docs_to_index(output, &entries);
    if docfx {
        append_extra_docs_to_toc(output, &entries);
    }
}
