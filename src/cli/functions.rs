// `--emit-function-list` adapter for saw-spec-gen.

use std::path::Path;

use pretty_specs::ir::Item;
use serde::Serialize;

use super::bundle::ModuleBundle;

#[derive(Debug, Serialize)]
struct FunctionEntry {
    module: String,
    name: String,
    signature: String,
    arity: usize,
    doc_summary: String,
}

/// Emit a JSON array of all functions across all modules, suitable for use as
/// saw-spec-gen batch input.
pub(crate) fn run_emit_function_list(
    modules: &[ModuleBundle],
    output: &Path,
    include_private: bool,
) {
    let mut entries: Vec<FunctionEntry> = Vec::new();

    for module in modules {
        for item in &module.items {
            if let Item::Function {
                name,
                signature,
                branches,
                body,
                doc,
                is_private,
                ..
            } = item
            {
                if !signature.contains("->") && branches.is_empty() {
                    continue;
                }
                if is_simple_constructor_by_name(name) {
                    continue;
                }
                // Skip private helpers unless caller asked for them.
                if *is_private && !include_private {
                    continue;
                }

                let arity = count_arity(signature);
                let doc_summary = doc.first().cloned().unwrap_or_else(|| {
                    // Use the auto-describe if no doc
                    use pretty_specs::describe::auto_describe_function;
                    auto_describe_function(name, signature, branches, body)
                        .into_iter()
                        .next()
                        .unwrap_or_default()
                });

                entries.push(FunctionEntry {
                    module: module.module_name.clone(),
                    name: name.clone(),
                    signature: signature.clone(),
                    arity,
                    doc_summary,
                });
            }
        }
    }

    let json = serde_json::to_string_pretty(&entries).unwrap_or_else(|e| {
        eprintln!("error: failed to serialize function list: {e}");
        std::process::exit(2);
    });

    if output == Path::new("./output") {
        println!("{json}");
    } else {
        if let Some(parent) = output.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                eprintln!("error: cannot create {}: {e}", parent.display());
                std::process::exit(2);
            });
        }
        std::fs::write(output, format!("{json}\n")).unwrap_or_else(|e| {
            eprintln!("error: cannot write {}: {e}", output.display());
            std::process::exit(2);
        });
        eprintln!("wrote {} ({} functions)", output.display(), entries.len());
    }
}

fn count_arity(signature: &str) -> usize {
    // Count the number of "->" at the top level (not inside parentheses/braces).
    let mut depth = 0usize;
    let bytes = signature.as_bytes();
    let mut arrows = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'{' | b'[' => depth += 1,
            b')' | b'}' | b']' => depth = depth.saturating_sub(1),
            b'-' if depth == 0 && i + 1 < bytes.len() && bytes[i + 1] == b'>' => {
                arrows += 1;
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    arrows
}

fn is_simple_constructor_by_name(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_uppercase())
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !name.contains("is")
        && name.len() <= 30
}
