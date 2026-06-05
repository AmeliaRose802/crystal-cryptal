mod cli;

use std::collections::HashSet;
use std::path::PathBuf;

use clap::Parser;

use pretty_specs::ir::{Item, ProofStatus, load_proof_manifest};
use pretty_specs::linker::{ModuleSpec, SymbolTable};
use pretty_specs::parser::parse;
use pretty_specs::render_json::render_json;
use pretty_specs::render_md::{RenderOptions, render_multi_file_with_prefix, render_single_file};

use cli::assets::handle_branding_assets;
use cli::bundle::{
    JsonModule, ModuleBundle, collect_input_files, detect_module_name,
    extract_module_dependencies, render_multi_module_index, topological_module_order,
};
use cli::extra_docs::handle_extra_docs;
use cli::functions::run_emit_function_list;
use cli::saw_adapt::{run_adapt_saw_results, run_saw_log_adapter};

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

    /// When used with --emit-function-list, also include `private` declarations
    /// (by default only exported/public functions are listed)
    #[arg(long)]
    include_private: bool,

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

    /// Path to a logo image (svg/png) to copy into <output>/images/.
    /// When combined with --docfx, prints the matching `_appLogoPath`
    /// globalMetadata snippet to stderr.
    #[arg(long, value_name = "PATH")]
    logo: Option<PathBuf>,

    /// Path to a favicon (.ico/.png/.svg) to copy into <output>/images/.
    /// When combined with --docfx, prints the matching `_appFaviconPath`
    /// globalMetadata snippet to stderr.
    #[arg(long, value_name = "PATH")]
    favicon: Option<PathBuf>,

    /// Directory of additional Markdown (and supporting) files to include
    /// verbatim in the generated site. Each directory is copied into
    /// `<output>/<basename>/` preserving structure. In `--docfx` mode an
    /// entry is appended to the top-level `toc.yml` so the pages appear in
    /// the navbar. May be repeated to include multiple directories.
    ///
    /// Optional syntax `DIR:Display Name` overrides the toc label;
    /// otherwise the directory basename (Title Case) is used.
    #[arg(long = "extra-docs", value_name = "DIR[:NAME]")]
    extra_docs: Vec<String>,
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
        apply_proof_manifest(&mut modules, manifest_path);
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
        run_emit_function_list(&modules, &cli.output, cli.include_private);
        return;
    }

    if modules.len() == 1 {
        run_single_module(cli, &modules[0], &unified_symbols);
        return;
    }

    run_multi_module(cli, &modules, &unified_symbols);
}

fn apply_proof_manifest(modules: &mut [ModuleBundle], manifest_path: &std::path::Path) {
    match load_proof_manifest(manifest_path) {
        Ok(manifest) => {
            let mut consumed_props: HashSet<String> = HashSet::new();
            for module in modules.iter_mut() {
                for item in &mut module.items {
                    match item {
                        Item::Property {
                            label,
                            name,
                            proof_status,
                            ..
                        } => {
                            // Try `{label}_{name}` (Cryptol convention `P1_FooBar`),
                            // then `name` alone, then `label` alone.
                            let full = if label == name {
                                label.clone()
                            } else {
                                format!("{label}_{name}")
                            };
                            let lookup = manifest
                                .properties
                                .get(&full)
                                .or_else(|| manifest.properties.get(name))
                                .or_else(|| manifest.properties.get(label));
                            if let Some(status) = lookup {
                                *proof_status = Some(status.clone());
                                consumed_props.insert(full);
                            }
                        }
                        Item::Function {
                            name, proof_status, ..
                        } => {
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
                    if matches!(
                        status,
                        ProofStatus::Failed { .. } | ProofStatus::NotAttempted
                    ) {
                        last_module.items.push(Item::Property {
                            label: key.clone(),
                            name: key.to_lowercase().replace(['-', ' ', '.'], "_"),
                            params: vec![],
                            body: String::new(),
                            doc: vec![format!(
                                "*(Property `{key}` appears in proof manifest but was not found in the spec.)*"
                            )],
                            proof_status: Some(status.clone()),
                            is_private: false,
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

fn run_single_module(cli: Cli, module: &ModuleBundle, _symbols: &SymbolTable) {
    // Rebuild the symbol table with an empty prefix so that stored paths are
    // bare ("types.md", "functions/foo.md", …).  The unified_symbols passed in
    // were constructed with output_prefix = module_name (e.g. "SDEP"), which
    // would cause relative_path to produce "../SDEP/types.md" from a property
    // file instead of the correct "../types.md".
    let symbols = SymbolTable::build_for_module_with_prefix(&module.items, "");
    let symbols = &symbols;

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
            eprintln!(
                "error: {}: render failed: {e}",
                module.source_path.display()
            );
            std::process::exit(2);
        });
    handle_branding_assets(
        &cli.output,
        cli.logo.as_deref(),
        cli.favicon.as_deref(),
        cli.docfx,
    );
    handle_extra_docs(&cli.output, &cli.extra_docs, cli.docfx);
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
            eprintln!(
                "error: {}: render failed: {e}",
                module.source_path.display()
            );
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
        let mut toc =
            String::from("- name: Overview\n  href: index.md\n- name: Modules\n  items:\n");
        for m in modules {
            toc.push_str(&format!(
                "  - name: {}\n    href: {}/index.md\n",
                m.module_name, m.output_prefix
            ));
        }
        std::fs::write(cli.output.join("toc.yml"), toc).unwrap_or_else(|e| {
            eprintln!("error: cannot write toc.yml: {e}");
            std::process::exit(2);
        });
    }

    handle_branding_assets(
        &cli.output,
        cli.logo.as_deref(),
        cli.favicon.as_deref(),
        cli.docfx,
    );
    handle_extra_docs(&cli.output, &cli.extra_docs, cli.docfx);

    eprintln!("wrote output to {}", cli.output.display());
}
