use clap::Parser;
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};

use pretty_specs::ir::{Item, load_proof_manifest};
use pretty_specs::linker::{ModuleSpec, SymbolTable};
use pretty_specs::parser::parse;
use pretty_specs::render_json::render_json;
use pretty_specs::render_md::{RenderOptions, render_multi_file_with_prefix, render_single_file};

#[derive(Parser, Debug)]
#[command(name = "pretty-specs", about = "Cryptol-to-Markdown renderer")]
struct Cli {
    /// Path(s) to .cry input files or directories containing .cry files
    inputs: Vec<PathBuf>,

    /// Output directory
    #[arg(short, long, default_value = "./output")]
    output: PathBuf,

    /// Emit a single Markdown file instead of a directory of files
    #[arg(long)]
    single_file: bool,

    /// Emit JSON IR instead of Markdown
    #[arg(long)]
    emit_json: bool,

    /// Omit detailed function bodies and property explanations
    #[arg(long)]
    no_details: bool,

    /// Path to a proof-status JSON file (maps property labels to statuses)
    #[arg(long, value_name = "FILE")]
    proof_status: Option<PathBuf>,

    /// Document title (overrides the module name)
    #[arg(long, value_name = "TITLE")]
    title: Option<String>,
}

#[derive(Debug, Clone)]
struct ModuleBundle {
    module_name: String,
    output_prefix: String,
    source_path: PathBuf,
    items: Vec<Item>,
}

#[derive(Debug, Serialize)]
struct JsonModule<'a> {
    module: &'a str,
    output_prefix: &'a str,
    source_path: String,
    imports: Vec<String>,
    items: &'a [Item],
}

fn main() {
    let cli = Cli::parse();
    if cli.inputs.is_empty() {
        eprintln!("error: at least one input file or directory is required");
        std::process::exit(2);
    }

    let input_files = collect_input_files(&cli.inputs).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(2);
    });
    if input_files.is_empty() {
        eprintln!("error: no .cry files found in the provided input paths");
        std::process::exit(2);
    }

    let mut modules = Vec::new();
    for file in &input_files {
        let source = std::fs::read_to_string(file).unwrap_or_else(|e| {
            eprintln!("error: cannot read {}: {e}", file.display());
            std::process::exit(2);
        });

        let source = source.strip_prefix('\u{FEFF}').unwrap_or(&source);
        let mut items = parse(source);

        let module_name = detect_module_name(&items).unwrap_or_else(|| {
            file.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Specification")
                .to_string()
        });

        if !items.iter().any(|i| matches!(i, Item::Module { .. })) {
            items.insert(
                0,
                Item::Module {
                    name: module_name.clone(),
                    doc: Vec::new(),
                },
            );
        }

        modules.push(ModuleBundle {
            output_prefix: module_name.replace("::", "/"),
            module_name,
            source_path: file.clone(),
            items,
        });
    }

    let order = topological_module_order(&modules).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(2);
    });
    let mut modules: Vec<ModuleBundle> = order.into_iter().map(|i| modules[i].clone()).collect();

    if let Some(manifest_path) = &cli.proof_status {
        match load_proof_manifest(manifest_path) {
            Ok(statuses) => {
                for module in &mut modules {
                    for item in &mut module.items {
                        if let Item::Property {
                            label,
                            proof_status,
                            ..
                        } = item
                            && let Some(status) = statuses.get(label)
                        {
                            *proof_status = Some(status.clone());
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "warning: failed to load proof manifest {}: {e}",
                    manifest_path.display()
                );
            }
        }
    }

    let module_specs: Vec<ModuleSpec<'_>> = modules
        .iter()
        .map(|m| ModuleSpec {
            name: &m.module_name,
            output_prefix: &m.output_prefix,
            items: &m.items,
        })
        .collect();
    let unified_symbols = SymbolTable::build_for_modules(&module_specs);

    if modules.len() == 1 {
        run_single_module(cli, &modules[0], &unified_symbols);
        return;
    }

    run_multi_module(cli, &modules, &unified_symbols);
}

fn run_single_module(cli: Cli, module: &ModuleBundle, symbols: &SymbolTable) {
    if cli.emit_json {
        let output_path = if cli.output == *"./output" {
            None
        } else {
            Some(cli.output.as_path())
        };
        render_json(&module.items, output_path).unwrap_or_else(|e| {
            eprintln!(
                "error: {}: failed to write JSON: {e}",
                module.source_path.display()
            );
            std::process::exit(2);
        });
        return;
    }

    let options = RenderOptions {
        no_details: cli.no_details,
        title_override: cli.title.clone(),
    };

    if cli.single_file {
        let md = render_single_file(&module.items, symbols, &options);
        if cli.output == *"./output" {
            println!("{md}");
        } else {
            std::fs::write(&cli.output, &md).unwrap_or_else(|e| {
                eprintln!("error: cannot write {}: {e}", cli.output.display());
                std::process::exit(2);
            });
            eprintln!("wrote {}", cli.output.display());
        }
        return;
    }

    render_multi_file_with_prefix(&module.items, symbols, &cli.output, &options, "")
        .unwrap_or_else(|e| {
            eprintln!("error: {}: render failed: {e}", module.source_path.display());
            std::process::exit(2);
        });
    eprintln!("wrote output to {}", cli.output.display());
}

fn run_multi_module(cli: Cli, modules: &[ModuleBundle], symbols: &SymbolTable) {
    if cli.single_file {
        eprintln!("error: --single-file only supports a single module input");
        std::process::exit(2);
    }

    if cli.emit_json {
        let payload: Vec<JsonModule<'_>> = modules
            .iter()
            .map(|m| JsonModule {
                module: &m.module_name,
                output_prefix: &m.output_prefix,
                source_path: m.source_path.display().to_string(),
                imports: extract_module_dependencies(&m.items),
                items: &m.items,
            })
            .collect();

        let json = serde_json::to_string_pretty(&payload).unwrap_or_else(|e| {
            eprintln!("error: failed to serialize JSON output: {e}");
            std::process::exit(2);
        });

        if cli.output == *"./output" {
            println!("{json}");
        } else {
            std::fs::write(&cli.output, format!("{json}\n")).unwrap_or_else(|e| {
                eprintln!("error: cannot write {}: {e}", cli.output.display());
                std::process::exit(2);
            });
            eprintln!("wrote {}", cli.output.display());
        }
        return;
    }

    let options = RenderOptions {
        no_details: cli.no_details,
        title_override: cli.title.clone(),
    };

    for module in modules {
        let module_output_dir = cli.output.join(&module.output_prefix);
        render_multi_file_with_prefix(
            &module.items,
            symbols,
            &module_output_dir,
            &options,
            &module.output_prefix,
        )
        .unwrap_or_else(|e| {
            eprintln!("error: {}: render failed: {e}", module.source_path.display());
            std::process::exit(2);
        });
    }

    std::fs::create_dir_all(&cli.output).unwrap_or_else(|e| {
        eprintln!("error: cannot create {}: {e}", cli.output.display());
        std::process::exit(2);
    });
    let root_index = render_multi_module_index(modules);
    std::fs::write(cli.output.join("index.md"), root_index).unwrap_or_else(|e| {
        eprintln!("error: cannot write {}/index.md: {e}", cli.output.display());
        std::process::exit(2);
    });

    eprintln!("wrote output to {}", cli.output.display());
}

fn collect_input_files(inputs: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
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

fn detect_module_name(items: &[Item]) -> Option<String> {
    items.iter().find_map(|item| match item {
        Item::Module { name, .. } => Some(name.clone()),
        _ => None,
    })
}

fn extract_module_dependencies(items: &[Item]) -> Vec<String> {
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

fn topological_module_order(modules: &[ModuleBundle]) -> Result<Vec<usize>, String> {
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

fn render_multi_module_index(modules: &[ModuleBundle]) -> String {
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
