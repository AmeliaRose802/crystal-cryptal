use clap::Parser;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};

use pretty_specs::ir::{Item, ProofStatus, load_proof_manifest};
use pretty_specs::linker::{ModuleSpec, SymbolTable};
use pretty_specs::parser::parse;
use pretty_specs::render_json::render_json;
use pretty_specs::render_md::{RenderOptions, render_multi_file_with_prefix, render_single_file};
use pretty_specs::saw_log::parse_saw_log;

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

    /// Emit a JSON array of functions (name, signature, arity) for use with saw-spec-gen
    #[arg(long)]
    emit_function_list: bool,

    /// Omit detailed function bodies and property explanations
    #[arg(long)]
    no_details: bool,

    /// Path to a proof-status JSON manifest (properties and/or functions)
    #[arg(long, value_name = "FILE")]
    proof_status: Option<PathBuf>,

    /// Parse a raw SAW prove_print / prove log file and emit a proof manifest
    #[arg(long, value_name = "FILE")]
    adapt_saw_log: Option<PathBuf>,

    /// Scan a directory for saw-spec-gen result.json files and emit a unified proof manifest
    #[arg(long, value_name = "DIR")]
    adapt_saw_results: Option<PathBuf>,

    /// Output path for --adapt-saw-log / --adapt-saw-results (default: proof_manifest.json)
    #[arg(long, value_name = "FILE", default_value = "proof_manifest.json")]
    manifest_output: PathBuf,

    /// Document title (overrides the module name)
    #[arg(long, value_name = "TITLE")]
    title: Option<String>,

    /// Emit DocFX-compatible front-matter and toc.yml alongside each index.md
    #[arg(long)]
    docfx: bool,
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

    if let Some(log_path) = &cli.adapt_saw_log {
        run_saw_log_adapter(log_path, &cli.manifest_output);
        return;
    }

    if let Some(results_dir) = &cli.adapt_saw_results {
        run_adapt_saw_results(results_dir, &cli.manifest_output);
        return;
    }

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
            Ok(manifest) => {
                let mut consumed_props: HashSet<String> = HashSet::new();
                for module in &mut modules {
                    for item in &mut module.items {
                        match item {
                            Item::Property { label, proof_status, .. } => {
                                if let Some(status) = manifest.properties.get(label) {
                                    *proof_status = Some(status.clone());
                                    consumed_props.insert(label.clone());
                                }
                            }
                            Item::Function { name, proof_status, .. } => {
                                if let Some(status) = manifest.functions.get(name) {
                                    *proof_status = Some(status.clone());
                                }
                            }
                            _ => {}
                        }
                    }
                }

                // x8p fix: inject placeholder properties for manifest entries whose
                // status is failed or not_attempted but which have no matching property
                // in the parsed spec — these are the most important gaps to surface.
                if let Some(last_module) = modules.last_mut() {
                    for (key, status) in &manifest.properties {
                        if consumed_props.contains(key) {
                            continue;
                        }
                        if matches!(status, ProofStatus::Failed { .. } | ProofStatus::NotAttempted) {
                            last_module.items.push(Item::Property {
                                label: key.clone(),
                                name: key
                                    .to_lowercase()
                                    .replace(['-', ' ', '.'], "_"),
                                params: vec![],
                                body: String::new(),
                                doc: vec![format!(
                                    "*(Property `{key}` appears in proof manifest but was not found in the spec.)*"
                                )],
                                proof_status: Some(status.clone()),
                            });
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

    if cli.emit_function_list {
        run_emit_function_list(&modules, &cli.output);
        return;
    }

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
        docfx: cli.docfx,
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
        docfx: cli.docfx,
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

    if cli.docfx {
        let mut toc = String::from("- name: Overview\n  href: index.md\n- name: Modules\n  items:\n");
        for m in modules {
            toc.push_str(&format!("  - name: {}\n    href: {}/index.md\n", m.module_name, m.output_prefix));
        }
        std::fs::write(cli.output.join("toc.yml"), toc).unwrap_or_else(|e| {
            eprintln!("error: cannot write toc.yml: {e}");
            std::process::exit(2);
        });
    }

    eprintln!("wrote output to {}", cli.output.display());
}

fn run_saw_log_adapter(log_path: &Path, output: &Path) {
    let text = std::fs::read_to_string(log_path).unwrap_or_else(|e| {
        eprintln!("error: cannot read {}: {e}", log_path.display());
        std::process::exit(2);
    });

    let records = parse_saw_log(&text);

    let mut properties = serde_json::Map::new();
    for record in &records {
        let entry = proof_status_to_json(&record.status);
        properties.insert(record.name.clone(), entry);
    }

    let manifest = serde_json::json!({
        "properties": properties,
        "functions": {},
    });

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                eprintln!("error: cannot create {}: {e}", parent.display());
                std::process::exit(2);
            });
        }
    }
    let serialized = serde_json::to_string_pretty(&manifest).unwrap_or_else(|e| {
        eprintln!("error: failed to serialize manifest: {e}");
        std::process::exit(2);
    });
    std::fs::write(output, format!("{serialized}\n")).unwrap_or_else(|e| {
        eprintln!("error: cannot write {}: {e}", output.display());
        std::process::exit(2);
    });

    eprintln!(
        "wrote {} ({} propert{})",
        output.display(),
        records.len(),
        if records.len() == 1 { "y" } else { "ies" }
    );
}

fn proof_status_to_json(status: &ProofStatus) -> serde_json::Value {
    match status {
        ProofStatus::Proven { solver, time_secs } => {
            let mut m = serde_json::Map::new();
            m.insert("status".into(), serde_json::json!("proven"));
            m.insert("solver".into(), serde_json::json!(solver));
            if let Some(t) = time_secs {
                m.insert("time_secs".into(), serde_json::json!(t));
            }
            serde_json::Value::Object(m)
        }
        ProofStatus::Failed { reason } => {
            serde_json::json!({ "status": "failed", "reason": reason })
        }
        ProofStatus::Assumed => serde_json::json!({ "status": "assumed" }),
        ProofStatus::NotAttempted => serde_json::json!({ "status": "not_attempted" }),
    }
}

// ── --adapt-saw-results ───────────────────────────────────────────────────────

/// Scan a directory tree for saw-spec-gen `result.json` files and emit a
/// unified `proof_manifest.json`.
///
/// Each `result.json` must contain:
/// ```json
/// {
///   "cryptol_fn": "provisionKey",
///   "status": "verified",        // "verified" | "counterexample" | "timeout" | "error" | "not_run"
///   "solver": "z3",              // optional
///   "time_secs": 1.2,            // optional
///   "impl_lang": "cpp",          // optional
///   "impl_file": "sdep.cpp",     // optional
///   "message": null              // optional error/CE text
/// }
/// ```
fn run_adapt_saw_results(dir: &Path, output: &Path) {
    let mut result_files = Vec::new();
    collect_result_json_recursive(dir, &mut result_files).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(2);
    });

    if result_files.is_empty() {
        eprintln!("warning: no result.json files found under {}", dir.display());
    }

    let mut functions_map = serde_json::Map::new();

    for path in &result_files {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("warning: cannot read {}: {e}", path.display());
                continue;
            }
        };
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("warning: cannot parse {}: {e}", path.display());
                continue;
            }
        };

        let fn_name = value
            .get("cryptol_fn")
            .and_then(|v| v.as_str())
            .or_else(|| value.get("function").and_then(|v| v.as_str()))
            .unwrap_or_else(|| {
                // Fall back to the parent directory name (e.g. out_provisionKey → provisionKey)
                path.parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
            })
            .to_string();

        // Accept both the legacy "status" field and the newer "verdict" field
        // emitted by saw-spec-gen's Write-VerifyResult (schema_version 1).
        // "verdict" values are uppercase (VERIFIED / DISPROVED / UNKNOWN);
        // "status" values are lowercase (verified / counterexample / error / …).
        let raw_status = value
            .get("status")
            .or_else(|| value.get("verdict"))
            .and_then(|v| v.as_str())
            .unwrap_or("not_run");
        let solver = value
            .get("solver")
            .and_then(|v| v.as_str())
            .unwrap_or("saw");
        let time_secs = value.get("time_secs").and_then(|v| v.as_f64());
        let message = value
            .get("message")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let proof_status = match raw_status {
            "verified" | "VERIFIED" | "Q.E.D." | "valid" | "EQUIVALENT" => {
                ProofStatus::Proven {
                    solver: solver.to_string(),
                    time_secs,
                }
            }
            "counterexample" | "DISPROVED" | "NOT EQUIVALENT" | "invalid" | "sat" => {
                ProofStatus::Failed {
                    reason: message.unwrap_or_else(|| "counterexample found".into()),
                }
            }
            "timeout" => ProofStatus::Failed {
                reason: message.unwrap_or_else(|| "timeout".into()),
            },
            "error" | "UNKNOWN" => ProofStatus::Failed {
                reason: message.unwrap_or_else(|| "error during verification".into()),
            },
            _ => ProofStatus::NotAttempted,
        };

        let mut entry = serde_json::Map::new();
        entry.insert("overall".into(), proof_status_to_json(&proof_status));
        if let Some(lang) = value.get("impl_lang").and_then(|v| v.as_str()) {
            let mut lang_entry = proof_status_to_json(&proof_status);
            if let Some(obj) = lang_entry.as_object_mut() {
                if let Some(f) = value.get("impl_file").and_then(|v| v.as_str()) {
                    obj.insert("impl_file".into(), serde_json::json!(f));
                }
            }
            entry.insert(
                "by_language".into(),
                serde_json::json!({ lang: lang_entry }),
            );
        }
        functions_map.insert(fn_name, serde_json::Value::Object(entry));
    }

    let fn_count = functions_map.len();
    let manifest = serde_json::json!({
        "properties": {},
        "functions": functions_map,
    });

    write_manifest_file(&manifest, output);
    eprintln!(
        "wrote {} ({} function{})",
        output.display(),
        fn_count,
        if fn_count == 1 { "" } else { "s" }
    );
}

fn collect_result_json_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir)
        .map_err(|e| format!("failed to read directory {}: {e}", dir.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read entry: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_result_json_recursive(&path, out)?;
        } else if path.file_name().and_then(|n| n.to_str()) == Some("result.json") {
            out.push(path);
        }
    }
    Ok(())
}

// ── --emit-function-list ──────────────────────────────────────────────────────

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
fn run_emit_function_list(modules: &[ModuleBundle], output: &Path) {
    let mut entries: Vec<FunctionEntry> = Vec::new();

    for module in modules {
        for item in &module.items {
            if let Item::Function {
                name,
                signature,
                branches,
                body,
                doc,
                ..
            } = item
            {
                if !signature.contains("->") && branches.is_empty() {
                    continue;
                }
                if is_simple_constructor_by_name(name) {
                    continue;
                }

                let arity = count_arity(signature);
                let doc_summary = doc
                    .first()
                    .cloned()
                    .unwrap_or_else(|| {
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
        if let Some(parent) = output.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                    eprintln!("error: cannot create {}: {e}", parent.display());
                    std::process::exit(2);
                });
            }
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
    // arity = arrows (number of parameters), capped at 0 if empty
    arrows
}

fn is_simple_constructor_by_name(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_uppercase())
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !name.contains("is")
        && name.len() <= 30
}

fn write_manifest_file(manifest: &serde_json::Value, output: &Path) {
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                eprintln!("error: cannot create {}: {e}", parent.display());
                std::process::exit(2);
            });
        }
    }
    let serialized = serde_json::to_string_pretty(manifest).unwrap_or_else(|e| {
        eprintln!("error: failed to serialize manifest: {e}");
        std::process::exit(2);
    });
    std::fs::write(output, format!("{serialized}\n")).unwrap_or_else(|e| {
        eprintln!("error: cannot write {}: {e}", output.display());
        std::process::exit(2);
    });
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
