use clap::Parser;
use std::path::PathBuf;

use pretty_specs::ir::load_proof_manifest;
use pretty_specs::linker::SymbolTable;
use pretty_specs::parser::parse;
use pretty_specs::render_json::render_json;
use pretty_specs::render_md::{render_multi_file, render_single_file, RenderOptions};

#[derive(Parser, Debug)]
#[command(name = "pretty-specs", about = "Cryptol-to-Markdown renderer")]
struct Cli {
    /// Path to the .cry input file
    input: PathBuf,

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

    /// Path to a proof-status JSON file (maps property names to PASS/FAIL)
    #[arg(long, value_name = "FILE")]
    proof_status: Option<PathBuf>,

    /// Document title (overrides the module name)
    #[arg(long, value_name = "TITLE")]
    title: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    let source = std::fs::read_to_string(&cli.input).unwrap_or_else(|e| {
        eprintln!("error: cannot read {}: {e}", cli.input.display());
        std::process::exit(2);
    });

    // Strip UTF-8 BOM if present.
    let source = source.strip_prefix('\u{FEFF}').unwrap_or(&source);

    let mut items = parse(source);

    // Optionally load proof manifest and apply to properties.
    if let Some(manifest_path) = &cli.proof_status {
        match load_proof_manifest(manifest_path) {
            Ok(statuses) => {
                for item in &mut items {
                    if let pretty_specs::ir::Item::Property { label, proof_status, .. } = item
                        && let Some(status) = statuses.get(label)
                    {
                        *proof_status = Some(status.clone());
                    }
                }
            }
            Err(e) => {
                eprintln!("warning: failed to load proof manifest {}: {e}", manifest_path.display());
            }
        }
    }

    let symbols = SymbolTable::build(&items);

    if cli.emit_json {
        // If the user left -o at its default ("./output"), write to stdout.
        // If they explicitly set -o, treat it as a file path.
        let output_path = if cli.output == *"./output" {
            None
        } else {
            Some(cli.output.as_path())
        };
        render_json(&items, output_path).unwrap_or_else(|e| {
            eprintln!("error: {}: failed to write JSON: {e}", cli.input.display());
            std::process::exit(2);
        });
        return;
    }

    let options = RenderOptions {
        no_details: cli.no_details,
        title_override: cli.title.clone(),
    };

    if cli.single_file {
        let md = render_single_file(&items, &symbols, &options);
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

    render_multi_file(&items, &symbols, &cli.output, &options).unwrap_or_else(|e| {
        eprintln!("error: {}: render failed: {e}", cli.input.display());
        std::process::exit(2);
    });

    eprintln!("wrote output to {}", cli.output.display());
}
