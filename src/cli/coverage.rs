//! CLI glue for the coverage feature: resolve config + inventory paths,
//! build the ledger, and write `coverage.md` to the output root.
//!
//! Kept out of `main.rs` so that file stays comfortably under the 500-line
//! CI cap.

use std::path::{Path, PathBuf};

use pretty_specs::coverage::{
    Ledger, build_ledger, load_coverage_config, load_inventory, render_coverage_matrix,
};
use pretty_specs::ir::Item;

use super::bundle::ModuleBundle;

/// Inputs from the CLI relevant to the coverage feature. Both fields are
/// optional — passing neither yields the pre-coverage-feature behavior
/// (bare `✓/✗` badges, no `coverage.md` page).
pub(crate) struct CoverageInputs<'a> {
    pub implementation_inventory: Option<&'a Path>,
    pub coverage_config: Option<&'a Path>,
    pub proof_status: Option<&'a Path>,
}

/// Resolve config + inventory inputs (with auto-detection) and build the
/// ledger. Returns `None` when neither input is present.
pub(crate) fn build_ledger_from_cli(
    inputs: CoverageInputs<'_>,
    modules: &[ModuleBundle],
) -> Option<Ledger> {
    // Inventory: explicit flag, or auto-detect `implementation_inventory.json`
    // sitting next to `--proof-status`. Auto-detection keeps the saw-spec-gen
    // pipeline working without a new flag.
    let inventory_path: Option<PathBuf> = inputs
        .implementation_inventory
        .map(PathBuf::from)
        .or_else(|| {
            inputs.proof_status.and_then(|p| {
                let candidate = p
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join("implementation_inventory.json");
                candidate.exists().then_some(candidate)
            })
        });

    // Config: explicit flag, or `coverage.toml` in the cwd.
    let config_path: Option<PathBuf> = inputs.coverage_config.map(PathBuf::from).or_else(|| {
        let candidate = PathBuf::from("coverage.toml");
        candidate.exists().then_some(candidate)
    });

    if inventory_path.is_none() && config_path.is_none() {
        return None;
    }

    let inventory = match inventory_path.as_deref().map(load_inventory) {
        Some(Ok(inv)) => inv,
        Some(Err(e)) => {
            eprintln!("warning: {e}");
            Default::default()
        }
        None => Default::default(),
    };

    let config = match config_path.as_deref().map(load_coverage_config) {
        Some(Ok(c)) => c,
        Some(Err(e)) => {
            eprintln!("warning: {e}");
            Default::default()
        }
        None => Default::default(),
    };

    // In single-module mode the renderer writes pages flat (no module subdir),
    // so coverage links must be `functions/foo.md`, not
    // `<Module>/functions/foo.md`. Pass an empty prefix in that case so
    // `function_link` picks the flat form.
    let single_module = modules.len() == 1;
    let ledger_inputs: Vec<(String, String, &[Item])> = modules
        .iter()
        .map(|m| {
            let prefix = if single_module {
                String::new()
            } else {
                m.output_prefix.clone()
            };
            (m.module_name.clone(), prefix, m.items.as_slice())
        })
        .collect();
    Some(build_ledger(&ledger_inputs, &inventory, &config))
}

/// Render the coverage matrix and write it to `<output>/coverage.md`.
pub(crate) fn write_coverage_matrix(output: &Path, ledger: &Ledger) {
    let md = render_coverage_matrix(ledger);
    let target = output.join("coverage.md");
    if let Err(e) = std::fs::write(&target, md) {
        eprintln!("warning: cannot write {}: {e}", target.display());
    }
}
