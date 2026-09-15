// Coverage ledger: joins (Implementation ∪ Model) with the proof manifest
// to produce a five-state badge per function. The ledger is the source of
// truth for the `coverage.md` matrix page and the per-page badges that
// replace the bare ✓ / ✗ proof glyph.
//
// See `01-coverage-clarity.md` and `02-coverage-ledger.md` at the repo root
// for the design rationale.

mod config;
#[cfg(test)]
mod disproved_tests;
mod inventory;
mod ledger;
mod matrix;
mod render;
#[cfg(test)]
mod tests;

pub use config::{CoverageConfig, load_coverage_config};
pub use inventory::{ImplementationInventory, InventoryEntry, load_inventory};
pub use ledger::is_coverage_directive_line;
pub use ledger::{CoverageBadge, CoverageReason, Ledger, LedgerEntry, LedgerSource, build_ledger};
#[cfg(test)]
pub(crate) use ledger::{DirectiveKind, parse_coverage_directive};
pub use matrix::{render_coverage_content, render_coverage_matrix};
pub use render::{
    function_banner, function_implementation_source, function_status_cell, function_title_badge,
};
