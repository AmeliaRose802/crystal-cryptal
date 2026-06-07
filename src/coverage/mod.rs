// Coverage ledger: joins (Implementation ∪ Model) with the proof manifest
// to produce a five-state badge per function. The ledger is the source of
// truth for the `coverage.md` matrix page and the per-page badges that
// replace the bare ✓ / ✗ proof glyph.
//
// See `01-coverage-clarity.md` and `02-coverage-ledger.md` at the repo root
// for the design rationale.

mod config;
mod inventory;
mod ledger;
mod render;
#[cfg(test)]
mod tests;

pub use config::{CoverageConfig, load_coverage_config};
pub use inventory::{ImplementationInventory, InventoryEntry, load_inventory};
pub use ledger::{CoverageBadge, Ledger, LedgerEntry, LedgerSource, build_ledger};
pub use render::{
    function_banner, function_status_cell, function_title_badge, render_coverage_matrix,
};
