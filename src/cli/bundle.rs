// Module bundling: collect input files, detect dependencies, topologically
// sort, and render the multi-module index page.

use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};

use pretty_specs::ir::Item;
use serde::Serialize;

#[derive(Debug, Clone)]
pub(crate) struct ModuleBundle {
    pub(crate) module_name: String,
    pub(crate) output_prefix: String,
    pub(crate) source_path: PathBuf,
    pub(crate) items: Vec<Item>,
}

#[derive(Debug, Serialize)]
pub(crate) struct JsonModule<'a> {
    pub(crate) module: &'a str,
    pub(crate) output_prefix: &'a str,
    pub(crate) source_path: String,
    pub(crate) imports: Vec<String>,
    pub(crate) items: &'a [Item],
}

pub(crate) fn collect_input_files(inputs: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for input in inputs {
        if input.is_file() {
            if input.extension().and_then(|s| s.to_str()) == Some("cry") {
                files.push(input.clone());
            }
            continue;
        }

        if input.is_dir() {
            collect_cry_files_recursive(input, &mut files)?;
            continue;
        }

        return Err(format!("input path does not exist: {}", input.display()));
    }

    files.sort();
    files.dedup();
    Ok(files)
}

fn collect_cry_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir)
        .map_err(|e| format!("failed to read directory {}: {e}", dir.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read directory entry: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_cry_files_recursive(&path, files)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("cry") {
            files.push(path);
        }
    }
    Ok(())
}

pub(crate) fn detect_module_name(items: &[Item]) -> Option<String> {
    items.iter().find_map(|item| match item {
        Item::Module { name, .. } => Some(name.clone()),
        _ => None,
    })
}

pub(crate) fn extract_module_dependencies(items: &[Item]) -> Vec<String> {
    let mut deps = Vec::new();
    for item in items {
        if let Item::Import { module_path, .. } = item {
            deps.push(module_path.clone());
        }
    }
    deps.sort();
    deps.dedup();
    deps
}

pub(crate) fn topological_module_order(
    modules: &[ModuleBundle],
) -> Result<Vec<usize>, String> {
    let mut name_to_idx = BTreeMap::new();
    for (idx, module) in modules.iter().enumerate() {
        name_to_idx.insert(module.module_name.clone(), idx);
    }

    let mut indegree = vec![0usize; modules.len()];
    let mut edges = vec![Vec::<usize>::new(); modules.len()];

    for (idx, module) in modules.iter().enumerate() {
        for dep in extract_module_dependencies(&module.items) {
            if let Some(dep_idx) = name_to_idx.get(&dep) {
                edges[*dep_idx].push(idx);
                indegree[idx] += 1;
            }
        }
    }

    let mut queue = VecDeque::new();
    for (idx, degree) in indegree.iter().enumerate() {
        if *degree == 0 {
            queue.push_back(idx);
        }
    }

    let mut order = Vec::new();
    while let Some(idx) = queue.pop_front() {
        order.push(idx);
        for next in &edges[idx] {
            indegree[*next] -= 1;
            if indegree[*next] == 0 {
                queue.push_back(*next);
            }
        }
    }

    if order.len() != modules.len() {
        let blocked = indegree
            .iter()
            .enumerate()
            .filter(|(_, d)| **d > 0)
            .map(|(idx, _)| modules[idx].module_name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "module dependency cycle detected involving: {blocked}"
        ));
    }

    Ok(order)
}

pub(crate) fn render_multi_module_index(modules: &[ModuleBundle]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Specification Modules\n");

    let _ = writeln!(out, "## Modules\n");
    let _ = writeln!(out, "| Module | Types | Functions | Properties |");
    let _ = writeln!(out, "|--------|-------|-----------|------------|");
    for module in modules {
        let base = &module.output_prefix;
        let _ = writeln!(
            out,
            "| [{name}]({base}/index.md) | [types]({base}/types.md) | [functions]({base}/functions/index.md) | [properties]({base}/properties/) |",
            name = module.module_name
        );
    }
    out.push('\n');

    let _ = writeln!(out, "## Module Hierarchy\n");
    for module in modules {
        let depth = module.module_name.split("::").count().saturating_sub(1);
        let indent = "  ".repeat(depth);
        let _ = writeln!(
            out,
            "{indent}- [{name}]({base}/index.md)",
            name = module.module_name,
            base = module.output_prefix
        );
    }
    out.push('\n');

    let _ = writeln!(out, "## Dependencies\n");
    let _ = writeln!(out, "```mermaid");
    let _ = writeln!(out, "graph TD");
    for module in modules {
        let node = module.module_name.replace("::", "_");
        let _ = writeln!(out, "  {node}[\"{}\"]", module.module_name);
    }
    for module in modules {
        let from = module.module_name.replace("::", "_");
        for dep in extract_module_dependencies(&module.items) {
            if modules.iter().any(|m| m.module_name == dep) {
                let to = dep.replace("::", "_");
                let _ = writeln!(out, "  {from} --> {to}");
            }
        }
    }
    let _ = writeln!(out, "```");

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_specs::parser::parse;

    #[test]
    fn parse_imports_are_detected_for_dependencies() {
        let items = parse(
            "module B where\n\nimport A::Core as Core hiding (tmp, debug)\nfoo : [8]\nfoo = 1\n",
        );
        let deps = extract_module_dependencies(&items);
        assert_eq!(deps, vec!["A::Core".to_string()]);
    }

    #[test]
    fn topological_order_places_dependencies_first() {
        let mod_a = ModuleBundle {
            module_name: "A".to_string(),
            output_prefix: "A".to_string(),
            source_path: PathBuf::from("A.cry"),
            items: parse("module A where\n\na : [8]\na = 0\n"),
        };
        let mod_b = ModuleBundle {
            module_name: "B".to_string(),
            output_prefix: "B".to_string(),
            source_path: PathBuf::from("B.cry"),
            items: parse("module B where\n\nimport A\nb : [8]\nb = a\n"),
        };

        let modules = vec![mod_b, mod_a];
        let order = topological_module_order(&modules).expect("topological order");
        let ordered_names: Vec<String> = order
            .into_iter()
            .map(|idx| modules[idx].module_name.clone())
            .collect();
        assert_eq!(ordered_names, vec!["A".to_string(), "B".to_string()]);
    }

    #[test]
    fn topological_order_reports_cycles() {
        let mod_a = ModuleBundle {
            module_name: "A".to_string(),
            output_prefix: "A".to_string(),
            source_path: PathBuf::from("A.cry"),
            items: parse("module A where\n\nimport B\na : [8]\na = 0\n"),
        };
        let mod_b = ModuleBundle {
            module_name: "B".to_string(),
            output_prefix: "B".to_string(),
            source_path: PathBuf::from("B.cry"),
            items: parse("module B where\n\nimport A\nb : [8]\nb = a\n"),
        };

        let modules = vec![mod_a, mod_b];
        let err = topological_module_order(&modules).expect_err("cycle expected");
        assert!(err.contains("cycle"));
    }
}
