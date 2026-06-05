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

/// Copy `src` into `<output>/images/<basename>`, creating the directory
/// if needed. Returns the basename so callers can build a docfx-relative
/// reference (e.g. "images/logo.svg").
fn copy_asset_to_images(output: &Path, src: &Path, kind: &str) -> Option<String> {
    let file_name = match src.file_name().and_then(|s| s.to_str()) {
        Some(n) => n.to_string(),
        None => {
            eprintln!(
                "warning: --{kind} path {} has no filename, skipping copy",
                src.display()
            );
            return None;
        }
    };
    let images_dir = output.join("images");
    if let Err(e) = std::fs::create_dir_all(&images_dir) {
        eprintln!(
            "warning: cannot create {}: {e} (skipping --{kind})",
            images_dir.display()
        );
        return None;
    }
    let dest = images_dir.join(&file_name);
    match std::fs::copy(src, &dest) {
        Ok(_) => {
            eprintln!("copied {} → {}", src.display(), dest.display());
            Some(format!("images/{file_name}"))
        }
        Err(e) => {
            eprintln!(
                "warning: cannot copy {} → {}: {e}",
                src.display(),
                dest.display()
            );
            None
        }
    }
}

/// Copy logo/favicon assets (if any) into `<output>/images/` and print
/// the matching docfx `globalMetadata` snippet when --docfx is set.
fn handle_branding_assets(
    output: &Path,
    logo: Option<&Path>,
    favicon: Option<&Path>,
    docfx: bool,
) {
    let logo_rel = logo.and_then(|p| copy_asset_to_images(output, p, "logo"));
    let favicon_rel = favicon.and_then(|p| copy_asset_to_images(output, p, "favicon"));

    if !docfx || (logo_rel.is_none() && favicon_rel.is_none()) {
        return;
    }

    eprintln!();
    eprintln!("docfx: add the following to docfx.json `globalMetadata` (paths are");
    eprintln!("docfx: relative to the generated site root):");
    if let Some(p) = &logo_rel {
        eprintln!("  \"_appLogoPath\": \"{p}\",");
    }
    if let Some(p) = &favicon_rel {
        eprintln!("  \"_appFaviconPath\": \"{p}\",");
    }
}

/// Parse a `--extra-docs` argument of the form `DIR` or `DIR:Display Name`.
/// Returns `(dir, display_name_override)`. Skips the drive-letter colon on
/// Windows paths like `C:\foo` so it isn't mistaken for the name separator.
fn parse_extra_docs_arg(arg: &str) -> (PathBuf, Option<String>) {
    let bytes = arg.as_bytes();
    let search_start = if bytes.len() >= 2
        && bytes[1] == b':'
        && bytes[0].is_ascii_alphabetic()
    {
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

/// A resolved extra-docs entry, ready to be wired into the top-level toc.
struct ExtraDocsEntry {
    display_name: String,
    /// Optional toc-target href (`<basename>/toc.yml` or `<basename>/index.md`).
    /// `None` when the source dir has no obvious entry point — files are
    /// still copied so docfx picks them up via its content glob.
    href: Option<String>,
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
    if out.is_empty() { name.to_string() } else { out }
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

        let href = if dest.join("toc.yml").is_file() {
            Some(format!("{basename}/toc.yml"))
        } else if dest.join("index.md").is_file() {
            Some(format!("{basename}/index.md"))
        } else {
            eprintln!(
                "note: --extra-docs {}: no toc.yml or index.md at root; skipping toc entry",
                dir.display()
            );
            None
        };

        let display_name = name_override.unwrap_or_else(|| humanize_basename(&basename));
        entries.push(ExtraDocsEntry { display_name, href });
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
        eprintln!(
            "warning: cannot write updated {}: {e}",
            toc_path.display()
        );
    }
}

/// Copy `--extra-docs` directories and (in `--docfx` mode) patch the
/// top-level `toc.yml` so the pages appear in the navbar.
fn handle_extra_docs(output: &Path, extra_docs: &[String], docfx: bool) {
    if extra_docs.is_empty() {
        return;
    }
    let entries = copy_extra_docs(output, extra_docs);
    if docfx {
        append_extra_docs_to_toc(output, &entries);
    }
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
                            Item::Property { label, name, proof_status, .. } => {
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
            eprintln!("error: {}: render failed: {e}", module.source_path.display());
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

    handle_branding_assets(
        &cli.output,
        cli.logo.as_deref(),
        cli.favicon.as_deref(),
        cli.docfx,
    );
    handle_extra_docs(&cli.output, &cli.extra_docs, cli.docfx);

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

    // Preserve any existing `functions` entries so subsequent runs of
    // --adapt-saw-results don't clobber Cryptol property results and
    // vice versa.  Both adapters merge into the same manifest by name.
    let existing_functions = load_existing_section(output, "functions");

    let manifest = serde_json::json!({
        "properties": properties,
        "functions": existing_functions,
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

/// Read an existing manifest at `path` and return the named top-level
/// section (`properties` or `functions`) as a JSON object.  Used so the
/// two adapters can merge into the same file: each preserves the other
/// adapter's section instead of overwriting it.  Missing file or
/// unparseable manifest yields an empty object (which is also the
/// fresh-manifest case, so behavior is unchanged for first runs).
fn load_existing_section(path: &Path, key: &str) -> serde_json::Value {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return serde_json::json!({}),
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return serde_json::json!({}),
    };
    v.get(key)
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}))
}

fn proof_status_to_json(status: &ProofStatus) -> serde_json::Value {
    match status {
        ProofStatus::Proven {
            solver,
            time_secs,
            overrides,
            iterations,
            verify_command,
            verify_script,
        } => {
            let mut m = serde_json::Map::new();
            m.insert("status".into(), serde_json::json!("proven"));
            m.insert("solver".into(), serde_json::json!(solver));
            if let Some(t) = time_secs {
                m.insert("time_secs".into(), serde_json::json!(t));
            }
            if !overrides.is_empty() {
                m.insert("overrides".into(), serde_json::json!(overrides));
            }
            if let Some(n) = iterations {
                m.insert("iterations".into(), serde_json::json!(n));
            }
            if let Some(cmd) = verify_command {
                m.insert("verify_command".into(), serde_json::json!(cmd));
            }
            if let Some(scr) = verify_script {
                m.insert("verify_script".into(), serde_json::json!(scr));
            }
            serde_json::Value::Object(m)
        }
        ProofStatus::Failed {
            reason,
            counterexample,
            log_excerpt,
            verify_command,
            verify_script,
        } => {
            let mut m = serde_json::Map::new();
            m.insert("status".into(), serde_json::json!("failed"));
            m.insert("reason".into(), serde_json::json!(reason));
            if let Some(cx) = counterexample {
                m.insert("counterexample".into(), serde_json::json!(cx));
            }
            if let Some(log) = log_excerpt {
                m.insert("log_excerpt".into(), serde_json::json!(log));
            }
            if let Some(cmd) = verify_command {
                m.insert("verify_command".into(), serde_json::json!(cmd));
            }
            if let Some(scr) = verify_script {
                m.insert("verify_script".into(), serde_json::json!(scr));
            }
            serde_json::Value::Object(m)
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
        // Optional metadata: overridden function names and bounded-loop bound.
        // saw-spec-gen emits these when a SAW spec uses overrides or runs at
        // a bounded MAX_LEN; we round-trip both into the manifest so the
        // rendered docs can show what the proof actually depended on.
        let overrides: Vec<String> = value
            .get("overrides")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let iterations: Option<u64> = value
            .get("iterations")
            .or_else(|| value.get("loop_bound"))
            .or_else(|| value.get("max_len"))
            .and_then(|v| v.as_u64());
        // Failure diagnostics: a clean counterexample (when the solver
        // returned one) and/or an excerpt of the SAW/Cryptol log. Either
        // helps a reader of the rendered page see *why* the proof failed
        // instead of just "error during verification".
        //
        // saw-spec-gen (schema v1) emits the counterexample as a structured
        // array of `{name, value, bits}` records.  Pretty-specs has always
        // exposed it as free-form text in the rendered fold, so when we see
        // an array we format it as `name = value` lines here rather than
        // pushing that formatting concern into the renderer.  A `counterexample_text`
        // string field (saw-spec-gen schema v2+) wins when present so the
        // upstream tool can override formatting if it wants to.
        let counterexample: Option<String> = value
            .get("counterexample_text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                value
                    .get("counterexample")
                    .or_else(|| value.get("witness"))
                    .and_then(|v| match v {
                        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
                        serde_json::Value::Array(arr) if !arr.is_empty() => {
                            let lines: Vec<String> = arr
                                .iter()
                                .filter_map(|entry| {
                                    let obj = entry.as_object()?;
                                    let name = obj.get("name").and_then(|n| n.as_str())?;
                                    let val = obj
                                        .get("value")
                                        .map(|x| match x {
                                            serde_json::Value::String(s) => s.clone(),
                                            other => other.to_string(),
                                        })
                                        .unwrap_or_else(|| "<unknown>".into());
                                    let bits = obj.get("bits").and_then(|b| b.as_u64());
                                    Some(match bits {
                                        Some(b) => format!("{name} = {val}  ({b}-bit)"),
                                        None => format!("{name} = {val}"),
                                    })
                                })
                                .collect();
                            if lines.is_empty() {
                                None
                            } else {
                                Some(lines.join("\n"))
                            }
                        }
                        _ => None,
                    })
            });
        let log_excerpt: Option<String> = value
            .get("log_excerpt")
            .or_else(|| value.get("log"))
            .or_else(|| value.get("stderr"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        // Copy-pasteable shell command (and/or absolute path to the verify
        // script) that re-runs this proof locally.  The renderer surfaces
        // these in a "Verify this yourself" section so readers can poke at
        // the proof without grepping the pipeline.
        let verify_command: Option<String> = value
            .get("verify_command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let verify_script: Option<String> = value
            .get("verify_script")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let proof_status = match raw_status {
            "verified" | "VERIFIED" | "Q.E.D." | "valid" | "EQUIVALENT" => {
                ProofStatus::Proven {
                    solver: solver.to_string(),
                    time_secs,
                    overrides,
                    iterations,
                    verify_command: verify_command.clone(),
                    verify_script: verify_script.clone(),
                }
            }
            "counterexample" | "DISPROVED" | "NOT EQUIVALENT" | "invalid" | "sat" => {
                ProofStatus::Failed {
                    reason: message.unwrap_or_else(|| "counterexample found".into()),
                    counterexample,
                    log_excerpt,
                    verify_command: verify_command.clone(),
                    verify_script: verify_script.clone(),
                }
            }
            "timeout" => ProofStatus::Failed {
                reason: message.unwrap_or_else(|| "timeout".into()),
                counterexample,
                log_excerpt,
                verify_command: verify_command.clone(),
                verify_script: verify_script.clone(),
            },
            "error" | "UNKNOWN" => ProofStatus::Failed {
                reason: message.unwrap_or_else(|| "error during verification".into()),
                counterexample,
                log_excerpt,
                verify_command: verify_command.clone(),
                verify_script: verify_script.clone(),
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
    // Preserve any existing `properties` section so a prior
    // --adapt-saw-log run (Cryptol property verdicts) is not clobbered
    // when this adapter writes function verdicts to the same manifest.
    let existing_properties = load_existing_section(output, "properties");
    let manifest = serde_json::json!({
        "properties": existing_properties,
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
fn run_emit_function_list(modules: &[ModuleBundle], output: &Path, include_private: bool) {
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
